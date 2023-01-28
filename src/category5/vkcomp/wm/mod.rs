//! # Vulkan Compositor
//!
//! Also known as the `vkcomp` subsystem.
//!
//! The naming of this subsystem is somewhat confusing. It is called a
//! compositor, but it seems to act as a window manager and doesn't
//! actually do any composition itself. Category5 is the compositor,
//! but `vkcomp` is the code that handles window composition. `vkcomp`
//! takes the current state of the `atmosphere` and draws the windows on
//! the user's display.
//!
//! `vkcomp` itself is just the behavior for generating draw commands and
//! handling window presentation. The window positions are determined by
//! `ways` and `input`, and the Vulkan rendering is handled by `thundr`.
//!
//! `thundr` exists because during development I observed that the type of
//! Vulkan renderer I was creating could be reused in other places. For
//! example, a UI toolkit might also want to composite multiple buffers or
//! panels together to make a user interface. `vkcomp` is just one such
//! usage of `thundr`.
//!
//! ### Code
//! * `wm` - The window manager
//!   * `wm` specifies how to draw a typical desktop. There are a
//!   collection of applications at various sizes/positions, a desktop
//!   background, toolbars, etc. It generates a list of surfaces which are
//!   handed to `thundr` to be drawn.
//! * `wm/tasks.rs` - A list of tasks that the atmosphere passes to
//! vkcomp. These are one-time events, and usually just tell `vkcomp` that
//! an object was created and it needs to allocate gpu resources.
//! * `release_info.rs` - Release info are structs that specify values to
//! drop after `vkcomp` is done using them. This is used to release
//! wl_buffers once they are no longer in use by the gpu.

// Austin Shafer - 2020

#![allow(dead_code)]
extern crate image;
extern crate thundr;
extern crate utils;

use thundr as th;

use crate::category5::atmosphere::property_list::PropertyList;
use crate::category5::atmosphere::*;

pub use crate::utils::{anyhow, Result};
use utils::{log, timing::*, *};

pub mod task;
use super::release_info::DmabufReleaseInfo;
use task::*;

#[cfg(feature = "renderdoc")]
extern crate renderdoc;
#[cfg(feature = "renderdoc")]
use renderdoc::RenderDoc;

/// This consolidates the multiple resources needed
/// to represent a titlebar
struct Titlebar {
    /// The thick bar itself
    bar: th::Image,
    /// One dot to rule them all. Used for buttons
    dot: th::Image,
}

/// This represents a client window.
///
/// All drawn components are tracked with Image, this struct
/// keeps track of the window components (content imagees and
/// titlebar image).
///
/// See WindowManager::record_draw for how this is displayed.
#[derive(Clone)]
pub struct App {
    /// This id uniquely identifies the App
    a_id: WindowId,
    /// Because the images for imagees are used for both
    /// buffers in a double buffer system, when an App is
    /// deleted we need to avoid recording it in the next
    /// frame's cbuf.
    ///
    /// When this flag is set, the we will not be recorded
    /// and will instead be destroyed
    a_marked_for_death: bool,
    /// This is the set of geometric objects in the application
    a_surf: th::Surface,
    /// The image attached to `a_surf`
    a_image: Option<th::Image>,
    /// Any server side decorations for this app.
    /// Right now this is (dot, bar)
    a_ssd: Option<(th::Surface, th::Surface)>,
}

/// Encapsulates vkcomp and provides a sensible windowing API
///
/// This layer provides graphical operations to the above
/// layers. It will support two classes of displayed objs,
/// windows (has content and a titlebar) and sprites.
///
/// Sprites should only be used for desktop effects, such
/// as notifications. Sprites are not owned by a client
/// whereas windows are.
pub struct WindowManager {
    /// The vulkan renderer. It implements the draw logic,
    /// whereas WindowManager implements organizational logic
    wm_thundr: th::Thundr,
    /// This is the thundr surface list constructed from the resources that
    /// ways notified us of. Our job is to keep this up to date and call Thundr.
    wm_surfaces: th::SurfaceList,
    wm_atmos_ids: Vec<WindowId>,
    /// These are the surfaces that have been removed, and need their resources
    /// torn down. We keep this in a separate array so that we don't have to
    /// rescan the entire surface list every time we check for dead windows.
    wm_will_die: Vec<WindowId>,
    /// This is the set of applications in this scene
    wm_apps: PropertyList<App>,
    /// The background picture of the desktop
    wm_background: Option<th::Surface>,
    /// Image representing the software cursor
    wm_cursor: Option<th::Surface>,
    /// Title bar to draw above the windows
    wm_titlebar: Titlebar,
    #[cfg(feature = "renderdoc")]
    wm_renderdoc: RenderDoc<renderdoc::V141>,
}

impl WindowManager {
    /// Create a Titlebar resource
    ///
    /// The Titlebar will hold all of the components which make
    /// up all of the titlebars in a scene. These imagees will
    /// be colored differently when multidrawn
    fn get_default_titlebar(rend: &mut th::Thundr) -> Titlebar {
        let img = image::open(format!(
            "{}/{}",
            std::env::current_dir().unwrap().to_str().unwrap(),
            "images/bar.png"
        ))
        .unwrap()
        .to_bgra8();
        let pixels: Vec<u8> = img.into_vec();

        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);

        // TODO: make a way to change titlebar colors
        let mut bar = rend.create_image_from_bits(&mimg, None).unwrap();
        bar.set_damage(0, 0, 64, 64);

        let img = image::open(format!(
            "{}/{}",
            std::env::current_dir().unwrap().to_str().unwrap(),
            "images/dot.png"
        ))
        .unwrap()
        .to_bgra8();
        let pixels: Vec<u8> = img.into_vec();
        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);
        let mut dot = rend.create_image_from_bits(&mimg, None).unwrap();
        dot.set_damage(0, 0, 64, 64);

        Titlebar { bar: bar, dot: dot }
    }

    fn get_default_cursor(rend: &mut th::Thundr) -> Option<th::Surface> {
        let img = image::open(format!(
            "{}/{}",
            std::env::current_dir().unwrap().to_str().unwrap(),
            "images/cursor.png"
        ))
        .unwrap()
        .to_bgra8();
        let pixels: Vec<u8> = img.into_vec();
        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);

        let mut image = rend.create_image_from_bits(&mimg, None).unwrap();
        image.set_damage(0, 0, 64, 64);
        let mut surf = rend.create_surface(0.0, 0.0, 16.0, 16.0);
        rend.bind_image(&mut surf, image);

        Some(surf)
    }

    /// Create a new WindowManager
    ///
    /// This will create all the graphical resources needed for
    /// the compositor. The WindowManager will create and own
    /// the Thundr, thereby readying the display to draw.
    pub fn new(atmos: &mut Atmosphere) -> WindowManager {
        #[cfg(feature = "renderdoc")]
        let doc = RenderDoc::new().unwrap();

        let info = th::CreateInfo::builder()
            .enable_traditional_composition()
            .build();
        let mut rend = th::Thundr::new(&info).unwrap();
        let mut list = th::SurfaceList::new(&mut rend);
        let cursor = WindowManager::get_default_cursor(&mut rend);
        list.push(cursor.as_ref().unwrap().clone());

        let mut wm = WindowManager {
            wm_titlebar: WindowManager::get_default_titlebar(&mut rend),
            wm_cursor: cursor,
            wm_thundr: rend,
            wm_surfaces: list,
            wm_apps: PropertyList::new(),
            wm_will_die: Vec::new(),
            wm_atmos_ids: Vec::new(),
            wm_background: None,
            #[cfg(feature = "renderdoc")]
            wm_renderdoc: doc,
        };

        // Tell the atmosphere rend's resolution
        let res = wm.wm_thundr.get_resolution();
        atmos.set_resolution(res.0, res.1);
        let (major, minor) = wm.wm_thundr.get_drm_dev();
        atmos.set_drm_dev(major, minor);
        // first set the background
        let img = image::open("images/beach.png").unwrap().to_bgra8();
        let dims = img.dimensions();
        let pixels: Vec<u8> = img.into_vec();
        wm.set_background_from_mem(
            pixels.as_slice(),
            // dimensions of the texture
            dims.0,
            dims.1,
        )
        .unwrap();

        return wm;
    }

    /// Set the desktop background for the renderer
    ///
    /// This basically just creates a image with the max
    /// depth that takes up the entire screen.
    fn set_background_from_mem(
        &mut self,
        texture: &[u8],
        tex_width: u32,
        tex_height: u32,
    ) -> Result<()> {
        let mimg = MemImage::new(
            texture.as_ptr() as *mut u8,
            4,
            tex_width as usize,
            tex_height as usize,
        );

        let mut image = self.wm_thundr.create_image_from_bits(&mimg, None).unwrap();
        image.set_damage(0, 0, tex_width as i32, tex_height as i32);
        let res = self.wm_thundr.get_resolution();
        let mut surf = self
            .wm_thundr
            .create_surface(0.0, 0.0, res.0 as f32, res.1 as f32);
        self.wm_thundr.bind_image(&mut surf, image);
        self.wm_surfaces
            .insert(self.wm_surfaces.len() as usize, surf.clone());
        self.wm_background = Some(surf);
        Ok(())
    }

    /// Add a image to the renderer to be displayed.
    ///
    /// The imagees are added to a list, and will be individually
    /// dispatched for drawing later.
    ///
    /// Images need to be in an indexed vertex format.
    ///
    /// tex_res is the resolution of `texture`
    /// window_res is the size of the on screen window
    fn create_window(&mut self, id: WindowId) -> Result<()> {
        log::info!("wm: Creating new window {:?}", id);
        // This surface will have its dimensions updated during recording
        let surf = self.wm_thundr.create_surface(0.0, 0.0, 8.0, 8.0);
        // The bar should be a percentage of the screen height
        //let barsize = atmos.get_barsize();

        // TODO: Server side decorations
        // draw buttons on the titlebar
        // ----------------------------------------------------------------
        //let dims = Self::get_dot_dims(barsize, &(8.0, 8.0));
        //let mut dot = self
        //    .wm_thundr
        //    .create_surface(dims.0, dims.1, dims.2, dims.3);
        //self.wm_thundr
        //    .bind_image(&mut dot, self.wm_titlebar.dot.clone());
        //// add the dot as a subsurface above the window
        //surf.add_subsurface(dot);
        //// ----------------------------------------------------------------

        //// now render the bar itself, as wide as the window
        //// the bar needs to be behind the dots
        //// ----------------------------------------------------------------
        //let dims = Self::get_bar_dims(barsize, &(8.0, 8.0));
        //let mut bar = self
        //    .wm_thundr
        //    .create_surface(dims.0, dims.1, dims.2, dims.3);
        //self.wm_thundr
        //    .bind_image(&mut bar, self.wm_titlebar.bar.clone());
        //surf.add_subsurface(bar);
        // ----------------------------------------------------------------

        self.wm_apps.update_or_create(
            id.into(),
            App {
                a_id: id,
                a_marked_for_death: false,
                a_surf: surf,
                a_image: None,
                a_ssd: None,
            },
        );
        Ok(())
    }

    /// Handles an update from dmabuf task
    ///
    /// Translates the task update structure into lower
    /// level calls to import a dmabuf and update a image.
    /// Creates a new image if one doesn't exist yet.
    fn update_window_contents_from_dmabuf(
        &mut self,
        atmos: &mut Atmosphere,
        info: &UpdateWindowContentsFromDmabuf,
    ) -> Result<()> {
        log::debug!("Updating window {:?} with {:#?}", info.ufd_id, info);
        // Find the app corresponding to that window id
        // don't use a helper here because of the borrow checker
        let mut app = match self.wm_apps[info.ufd_id.into()].as_mut() {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => return Err(anyhow!("Could not find id {:?}", info.ufd_id)),
        };

        if let Some(image) = app.a_image.as_mut() {
            self.wm_thundr.update_image_from_dmabuf(
                image,
                &info.ufd_dmabuf,
                Some(Box::new(DmabufReleaseInfo {
                    dr_fd: info.ufd_dmabuf.db_fd,
                    dr_wl_buffer: info.ufd_wl_buffer.clone(),
                })),
            );
        } else {
            // If it does not have a image, then this must be the
            // first time contents were attached to it. Go ahead
            // and make one now
            app.a_image = self.wm_thundr.create_image_from_dmabuf(
                &info.ufd_dmabuf,
                Some(Box::new(DmabufReleaseInfo {
                    dr_fd: info.ufd_dmabuf.db_fd,
                    dr_wl_buffer: info.ufd_wl_buffer.clone(),
                })),
            );
        }

        if let Some(damage) = atmos.take_buffer_damage(info.ufd_id) {
            app.a_image.as_mut().map(|i| i.reset_damage(damage));
        }
        if let Some(damage) = atmos.take_surface_damage(info.ufd_id) {
            app.a_surf.damage(damage);
        }
        if let Some(image) = app.a_image.as_ref() {
            self.wm_thundr.bind_image(&mut app.a_surf, image.clone());
        }

        Ok(())
    }

    /// Handle update from memimage task
    ///
    /// Copies the shm buffer into the app's image.
    /// Creates a new image if one doesn't exist yet.
    fn update_window_contents_from_mem(
        &mut self,
        atmos: &mut Atmosphere,
        info: &UpdateWindowContentsFromMem,
    ) -> Result<()> {
        log::debug!("Updating window {:?} with {:#?}", info.id, info);
        // Find the app corresponding to that window id
        // don't use a helper here because of the borrow checker
        let mut app = match self.wm_apps[info.id.into()].as_mut() {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => return Err(anyhow!("Could not find id {:?}", info.id)),
        };

        let buffer_damage = atmos.take_buffer_damage(info.id);
        let surface_damage = atmos.take_surface_damage(info.id);
        // Damage the image
        if let Some(damage) = buffer_damage {
            app.a_image.as_mut().map(|i| i.reset_damage(damage));
        }
        if let Some(damage) = surface_damage {
            app.a_surf.damage(damage);
        }

        if let Some(image) = app.a_image.as_mut() {
            // TODO: maybe don't do anything if there isn't damage?
            let damage = self.wm_thundr.get_image_damage(&mut app.a_surf);
            self.wm_thundr
                .update_image_from_bits(image, &info.pixels, damage.as_ref(), None);
        } else {
            // If it does not have a image, then this must be the
            // first time contents were attached to it. Go ahead
            // and make one now
            app.a_image = self.wm_thundr.create_image_from_bits(&info.pixels, None);
        }

        self.wm_thundr
            .bind_image(&mut app.a_surf, app.a_image.as_ref().unwrap().clone());

        Ok(())
    }

    /// We have to pass in the barsize to get around some annoying borrow checker stuff
    fn get_bar_dims(barsize: f32, surface_size: &(f32, f32)) -> (f32, f32, f32, f32) {
        (
            // align it at the top right
            0.0,
            // draw the bar above the window
            -barsize,
            // the bar is as wide as the window
            surface_size.0,
            // use a percentage of the screen size
            barsize,
        )
    }

    fn get_dot_dims(barsize: f32, surface_size: &(f32, f32)) -> (f32, f32, f32, f32) {
        // The dotsize should be just slightly smaller
        let dotsize = barsize * 0.95;

        (
            surface_size.0
                // we don't want to go past the end of the bar
                    - barsize,
            -barsize,
            // align it at the top right
            dotsize, // width
            dotsize, // height
        )
    }

    /// Record all the drawing operations for the current scene
    ///
    /// Vulkan requires that we record a list of operations into a command
    /// buffer which is later submitted for display. This method organizes
    /// the recording of draw operations for all elements in the desktop.
    ///
    /// params: a private info structure for the Thundr. It holds all
    /// the data about what we are recording.
    fn record_draw(&mut self, atmos: &mut Atmosphere) {
        // get the latest cursor position
        // ----------------------------------------------------------------
        let (cursor_x, cursor_y) = atmos.get_cursor_pos();
        log::profiling!("Drawing cursor at ({}, {})", cursor_x, cursor_y);
        if let Some(cursor) = self.wm_cursor.as_mut() {
            cursor.set_pos(cursor_x as f32, cursor_y as f32);
        }
        // ----------------------------------------------------------------

        // Draw all of our windows on the desktop
        // Each app should have one or more windows,
        // all of which we need to draw.
        // ----------------------------------------------------------------
        self.wm_atmos_ids.clear();

        // Cache the inorder surface list from atmos
        // This helps us avoid nasty borrow checker stuff by avoiding recursion
        // ----------------------------------------------------------------
        let aids = &mut self.wm_atmos_ids;
        atmos.map_inorder_on_surfs(|id| {
            aids.push(id);
            return true;
        });

        // do the draw call separately due to the borrow checker
        // throwing a fit if it is in the loop above.
        //
        // This section really just updates the size and position of all the
        // surfaces. They should already have images attached, and damage will
        // be calculated from the result.
        // ----------------------------------------------------------------
        for r_id in self.wm_atmos_ids.iter() {
            let id = *r_id;
            // Now render the windows
            let a = match self.wm_apps[id.into()].as_mut() {
                Some(a) => a,
                // app must have been closed
                None => {
                    log::error!("Could not find id {:?} to record for drawing", id);
                    return;
                }
            };
            // If this window has been closed or if it is not ready for
            // rendering, ignore it
            if a.a_marked_for_death || !atmos.get_window_in_use(a.a_id) {
                return;
            }

            // get parameters
            // ----------------------------------------------------------------
            let surface_pos = atmos.get_surface_pos(a.a_id);
            let surface_size = atmos.get_surface_size(a.a_id);

            // update the th::Surface pos and size
            a.a_surf.set_pos(surface_pos.0, surface_pos.1);
            a.a_surf.set_size(surface_size.0, surface_size.1);
            // ----------------------------------------------------------------

            // Send any pending frame callbacks
            atmos.send_frame_callbacks_for_surf(a.a_id);

            // Only display the bar for toplevel surfaces
            // i.e. don't for popups
            //if atmos.get_toplevel(id) {
            //    // The bar should be a percentage of the screen height
            //    let barsize = atmos.get_barsize();

            //    // Each toplevel window has two subsurfaces (in thundr): the
            //    // window bar and the window dot. If it's toplevel we are drawing SSD,
            //    // so we need to update the positions of these as well.
            //    let mut sub = a.a_surf.get_subsurface(0);
            //    let dims = Self::get_dot_dims(barsize, &surface_size);
            //    sub.set_pos(dims.0, dims.1);
            //    sub.set_size(dims.2, dims.3);

            //    let dims = Self::get_bar_dims(barsize, &surface_size);
            //    let mut sub = a.a_surf.get_subsurface(0);
            //    sub.set_pos(dims.0, dims.1);
            //    sub.set_size(dims.2, dims.3);
            //}
        }
    }

    /// Flag this window to be killed.
    ///
    /// This adds it to our death list, which will be reaped next frame after
    /// we are done using its resources.
    fn close_window(&mut self, id: WindowId) -> Result<()> {
        // atmosphere skiplist not being propogated?
        assert!(self.wm_apps.id_exists(id.into()));
        log::debug!("Closing window {:?}", id);

        let mut app = self.wm_apps[id.into()].as_mut().unwrap();
        app.a_marked_for_death = true;
        self.wm_will_die.push(id);

        // Remove the surface. The surfacelist will damage the region that the
        // window occupied
        // This is haneld in the reordering bits
        // The window order is still showing removed subsurfaces....
        match self.wm_surfaces.remove_surface(app.a_surf.clone()) {
            Ok(()) => {}
            // If the surface wasn't found, it's because this is
            // the first time it's been placed in the surface list,
            // so we can ignore this
            Err(th::ThundrError::SURFACE_NOT_FOUND) => {}
            Err(e) => {
                return Err(anyhow!(e)).context(format!(
                    "Failed to remove window {:?} from the surface list",
                    id
                ))
            }
        };
        Ok(())
    }

    /// Remove any apps marked for death. Usually we can't remove
    /// a window immediately because its image(s) are still being
    /// used by thundr
    fn reap_dead_windows(&mut self) {
        // Only retain alive windows in the array
        for id in self.wm_will_die.drain(..) {
            if let Some(app) = self.wm_apps[id.into()].as_ref() {
                assert!(app.a_marked_for_death);

                self.wm_apps.deactivate(id.into())
            }
        }
    }

    /// Begin rendering a frame
    ///
    /// Vulkan is asynchronous, meaning that commands are submitted
    /// and later waited on. This method records the next cbuf
    /// and asks the Thundr to submit it.
    ///
    /// The frame is not presented to the display until
    /// WindowManager::end_frame is called.
    fn begin_frame(&mut self, atmos: &mut Atmosphere) -> Result<()> {
        self.record_draw(atmos);
        let res = self.wm_thundr.get_resolution();
        let viewport = th::Viewport::new(0, 0, res.0 as i32, res.1 as i32);

        // First push our CPU data to the GPU
        self.wm_thundr.flush_surface_data(&mut self.wm_surfaces)?;

        // Now kick off drawing
        match self.wm_thundr.begin_recording() {
            Ok(_) => Ok(()),
            Err(th::ThundrError::OUT_OF_DATE) => Ok(()),
            Err(e) => Err(anyhow!(e)),
        }?;
        self.wm_thundr
            .draw_surfaces(&mut self.wm_surfaces, &viewport)?;
        match self.wm_thundr.end_recording() {
            Ok(_) => Ok(()),
            Err(th::ThundrError::OUT_OF_DATE) => Ok(()),
            Err(e) => Err(anyhow!(e)),
        }
    }

    /// End a frame
    ///
    /// Once the frame's cbuf has been recorded and submitted, we
    /// can present it to the physical display.
    ///
    /// It is possible that the upper layers may want to perform
    /// operations between submission of the frame and when that
    /// frame is presented, which is why begin/end frame is split
    /// into two methods.
    fn end_frame(&mut self) -> Result<()> {
        match self.wm_thundr.present() {
            Ok(_) => {}
            Err(th::ThundrError::OUT_OF_DATE) => {}
            Err(e) => panic!("Failed to draw frame: {:?}", e),
        };
        Ok(())
    }

    /// A helper that uses a window id to do a static lookup of
    /// vkcomp's App structure. The index for the App structure matches
    /// the window id number
    fn lookup_app_from_id<'a>(&'a self, id: WindowId) -> Result<&'a App> {
        match self.wm_apps[id.into()].as_ref() {
            Some(a) => Ok(a),
            // app must have been closed
            None => Err(anyhow!("Could not find id {:?} in app list", id)),
        }
    }

    fn lookup_app_from_id_mut<'a>(&'a mut self, id: WindowId) -> Result<&'a mut App> {
        match self.wm_apps[id.into()].as_mut() {
            Some(a) => Ok(a),
            // app must have been closed
            None => Err(anyhow!("Could not find id {:?} in app list", id)),
        }
    }

    /// Move the window to the front of the th::SurfaceList
    ///
    /// There is really only one toplevel window movement
    /// event: moving something to the top of the window stack
    /// when the user clicks on it and puts it into focus.
    fn move_to_front(&mut self, atmos: &mut Atmosphere, win: WindowId) -> Result<()> {
        // get and use the root window for this subsurface
        // in case it is a subsurface.
        let root = match atmos.get_root_window(win) {
            Some(parent) => parent,
            None => win,
        };
        let surf = self.lookup_app_from_id(root)?.a_surf.clone();

        match self.wm_surfaces.remove_surface(surf.clone()) {
            Ok(()) => {}
            // If the surface wasn't found, it's because this is
            // the first time it's been placed in the surface list,
            // so we can ignore this
            Err(th::ThundrError::SURFACE_NOT_FOUND) => {}
            Err(e) => {
                return Err(anyhow!(e)).context(format!(
                    "Failed to remove window {:?} from the surface list",
                    win
                ))
            }
        };

        // Move to front really only moves to the second to front,
        // since we always have a cursor surface at the front
        self.wm_surfaces.insert(1, surf);
        Ok(())
    }

    /// Adds a new subsurface to the parent.
    ///
    /// The new subsurface will be moved to the top of the subsurface
    /// stack, as this is the default. The position may later be changed
    /// through the wl_subsurface interface.
    fn new_subsurface(&mut self, win: WindowId, parent: WindowId) -> Result<()> {
        let surf = self.lookup_app_from_id(win)?.a_surf.clone();
        let mut parent_surf = self.lookup_app_from_id(parent)?.a_surf.clone();

        parent_surf.add_subsurface(surf);
        Ok(())
    }

    /// Look up and place a surface above another in the subsurface list
    ///
    /// win will be placed above other. The parent will be looked up by
    /// searching the root window from atmos.
    fn subsurf_place_above(
        &mut self,
        atmos: &mut Atmosphere,
        win: WindowId,
        other: WindowId,
    ) -> Result<()> {
        self.subsurf_reorder_common(atmos, th::SubsurfaceOrder::Above, win, other)
    }

    /// Same as above, but place the subsurface below other.
    fn subsurf_place_below(
        &mut self,
        atmos: &mut Atmosphere,
        win: WindowId,
        other: WindowId,
    ) -> Result<()> {
        self.subsurf_reorder_common(atmos, th::SubsurfaceOrder::Below, win, other)
    }

    fn subsurf_reorder_common(
        &mut self,
        atmos: &mut Atmosphere,
        order: th::SubsurfaceOrder,
        win: WindowId,
        other: WindowId,
    ) -> Result<()> {
        let surf = self.lookup_app_from_id(win)?.a_surf.clone();
        let other_surf = self.lookup_app_from_id(other)?.a_surf.clone();
        let root = atmos
            .get_root_window(win)
            .expect("The window should have a root since it is a subsurface");
        let mut root_surf = self.lookup_app_from_id(root)?.a_surf.clone();

        root_surf.reorder_subsurface(order, surf, other_surf)?;

        Ok(())
    }

    pub fn process_task(&mut self, atmos: &mut Atmosphere, task: &Task) {
        log::info!("wm: got task {:?}", task);
        let err = match task {
            Task::begin_frame => self.begin_frame(atmos).context("Task: Starting a frame"),
            Task::end_frame => self.end_frame().context("Task: Ending a frame"),
            // set background from mem
            Task::sbfm(sb) => self.set_background_from_mem(sb.pixels.as_ref(), sb.width, sb.height),
            // create new window
            Task::create_window(id) => self.create_window(*id).context("Task: create_window"),
            Task::move_to_front(id) => self.move_to_front(atmos, *id).context("Task: close_window"),
            Task::new_subsurface { id, parent } => self
                .new_subsurface(*id, *parent)
                .context("Task: new_subsurface"),
            Task::place_subsurface_above { id, other } => self
                .subsurf_place_above(atmos, *id, *other)
                .context("Task: place_subsurface_above"),
            Task::place_subsurface_below { id, other } => self
                .subsurf_place_below(atmos, *id, *other)
                .context("Task: place_subsurface_below"),
            Task::close_window(id) => self.close_window(*id).context("Task: close_window"),
            // update window from gpu buffer
            Task::uwcfd(uw) => self
                .update_window_contents_from_dmabuf(atmos, uw)
                .context(format!("Task: Updating window {:?} from dmabuf", uw.ufd_id)),
            // update window from shm
            Task::uwcfm(uw) => self
                .update_window_contents_from_mem(atmos, uw)
                .context(format!(
                    "Task: Updating window {:?} from shared memory",
                    uw.id
                )),
        };

        match err {
            Ok(()) => {}
            Err(e) => log::error!("vkcomp: Task handler had error: {:?}", e),
        }
    }

    /// The main event loop of the vkcomp thread
    pub fn render_frame(&mut self, atmos: &mut Atmosphere) -> Result<()> {
        // how much time is spent drawing/presenting
        let mut draw_stop = StopWatch::new();

        // Now that we have completed the previous frame, we can
        // release all the resources used to construct it while
        // we wait for our draw calls
        // note: -bad- this probably calls wayland locks
        self.wm_thundr.release_pending_resources();

        #[cfg(feature = "renderdoc")]
        if atmos.get_renderdoc_recording() {
            self.wm_renderdoc
                .start_frame_capture(std::ptr::null(), std::ptr::null());
        }

        // iterate through all the tasks that ways left
        // us in this hemisphere
        //  (aka process the work queue)
        while let Some(task) = atmos.get_next_wm_task() {
            self.process_task(atmos, &task);
        }

        // start recording how much time we spent doing graphics
        log::debug!("_____________________________ FRAME BEGIN");
        // Create a frame out of the hemisphere we got from ways
        draw_stop.start();
        self.begin_frame(atmos)?;
        draw_stop.end();
        log::debug!(
            "spent {} ms drawing this frame",
            draw_stop.get_duration().as_millis()
        );

        atmos.print_surface_tree();

        // present our frame
        draw_stop.start();
        self.end_frame()?;
        draw_stop.end();

        log::debug!(
            "spent {} ms presenting this frame",
            draw_stop.get_duration().as_millis()
        );

        #[cfg(feature = "renderdoc")]
        if atmos.get_renderdoc_recording() {
            self.wm_renderdoc
                .end_frame_capture(std::ptr::null(), std::ptr::null());
        }
        self.reap_dead_windows();
        atmos.release_consumables();

        log::debug!("_____________________________ FRAME END");

        Ok(())
    }
}
