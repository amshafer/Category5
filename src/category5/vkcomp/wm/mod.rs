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

use utils::{log, timing::*, *};

pub mod task;
use super::release_info::DmabufReleaseInfo;
use task::*;

use std::sync::mpsc::{Receiver, Sender};

extern crate renderdoc;
use renderdoc::RenderDoc;

/// This consolidates the multiple resources needed
/// to represent a titlebar
struct Titlebar {
    /// The thick bar itself
    bar: th::Image,
    /// One dot to rule them all. Used for buttons
    dot: th::Image,
}

const IMAGE_PATH: &'static str = "/root/casa/compositor_playground/images/";

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
    /// The channel to recieve work over
    wm_atmos: Atmosphere,
    /// The vulkan renderer. It implements the draw logic,
    /// whereas WindowManager implements organizational logic
    wm_thundr: th::Thundr,
    /// This is the thundr surface list constructed from the resources that
    /// ways notified us of. Our job is to keep this up to date and call Thundr.
    wm_surfaces: th::SurfaceList,
    /// This is the same window order as wm_surfaces, but tracks windo ids instead.
    wm_surface_ids: Vec<WindowId>,
    wm_atmos_ids: Vec<WindowId>,
    /// These are the surfaces that have been removed, and need their resources
    /// torn down. We keep this in a separate array so that we don't have to
    /// rescan the entire surface list every time we check for dead windows.
    wm_will_die: Vec<WindowId>,
    /// This is a list of surfaces that have been reordered for the current frame.
    /// This is in WM because we don't want to be reallocating this every time.
    wm_reordered: Vec<WindowId>,
    /// This is the set of applications in this scene
    wm_apps: PropertyList<App>,
    /// The background picture of the desktop
    wm_background: Option<th::Surface>,
    /// Image representing the software cursor
    wm_cursor: Option<th::Surface>,
    /// Title bar to draw above the windows
    wm_titlebar: Titlebar,
    wm_renderdoc: RenderDoc<renderdoc::V141>,
}

impl WindowManager {
    /// Create a Titlebar resource
    ///
    /// The Titlebar will hold all of the components which make
    /// up all of the titlebars in a scene. These imagees will
    /// be colored differently when multidrawn
    fn get_default_titlebar(rend: &mut th::Thundr) -> Titlebar {
        let img = image::open(format!("{}/{}", IMAGE_PATH, "bar.png"))
            .unwrap()
            .to_rgba();
        let pixels: Vec<u8> = img.into_vec();

        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);

        // TODO: make a way to change titlebar colors
        let mut bar = rend.create_image_from_bits(&mimg, None).unwrap();
        bar.set_damage(0, 0, 64, 64);

        let img = image::open(format!("{}/{}", IMAGE_PATH, "/dot.png"))
            .unwrap()
            .to_rgba();
        let pixels: Vec<u8> = img.into_vec();
        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);
        let mut dot = rend.create_image_from_bits(&mimg, None).unwrap();
        dot.set_damage(0, 0, 64, 64);

        Titlebar { bar: bar, dot: dot }
    }

    fn get_default_cursor(rend: &mut th::Thundr) -> Option<th::Surface> {
        let img = image::open(format!("{}/{}", IMAGE_PATH, "/cursor.png"))
            .unwrap()
            .to_rgba();
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
    pub fn new(tx: Sender<Box<Hemisphere>>, rx: Receiver<Box<Hemisphere>>) -> WindowManager {
        let doc = RenderDoc::new().unwrap();
        let info = th::CreateInfo::builder()
            //.enable_traditional_composition()
            .build();
        let mut rend = th::Thundr::new(&info).unwrap();
        let mut list = th::SurfaceList::new();
        let cursor = WindowManager::get_default_cursor(&mut rend);
        list.push(cursor.as_ref().unwrap().clone());

        let mut wm = WindowManager {
            wm_atmos: Atmosphere::new(tx, rx),
            wm_titlebar: WindowManager::get_default_titlebar(&mut rend),
            wm_cursor: cursor,
            wm_thundr: rend,
            wm_surfaces: list,
            wm_apps: PropertyList::new(),
            wm_will_die: Vec::new(),
            wm_reordered: Vec::new(),
            wm_surface_ids: Vec::new(),
            wm_atmos_ids: Vec::new(),
            wm_background: None,
            wm_renderdoc: doc,
        };

        // Tell the atmosphere rend's resolution
        let res = wm.wm_thundr.get_resolution();
        wm.wm_atmos.set_resolution(res.0, res.1);
        return wm;
    }

    /// Set the desktop background for the renderer
    ///
    /// This basically just creates a image with the max
    /// depth that takes up the entire screen.
    fn set_background_from_mem(&mut self, texture: &[u8], tex_width: u32, tex_height: u32) {
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
    fn create_window(&mut self, id: WindowId) {
        log::info!("wm: Creating new window {:?}", id);
        // This surface will have its dimensions updated during recording
        let mut surf = self.wm_thundr.create_surface(0.0, 0.0, 8.0, 8.0);
        // The bar should be a percentage of the screen height
        let barsize = self.wm_atmos.get_barsize();

        // draw buttons on the titlebar
        // ----------------------------------------------------------------
        let dims = Self::get_dot_dims(barsize, &(8.0, 8.0));
        let mut dot = self
            .wm_thundr
            .create_surface(dims.0, dims.1, dims.2, dims.3);
        self.wm_thundr
            .bind_image(&mut dot, self.wm_titlebar.dot.clone());
        // add the dot as a subsurface above the window
        surf.add_subsurface(dot);
        // ----------------------------------------------------------------

        // now render the bar itself, as wide as the window
        // the bar needs to be behind the dots
        // ----------------------------------------------------------------
        let dims = Self::get_bar_dims(barsize, &(8.0, 8.0));
        let mut bar = self
            .wm_thundr
            .create_surface(dims.0, dims.1, dims.2, dims.3);
        self.wm_thundr
            .bind_image(&mut bar, self.wm_titlebar.bar.clone());
        surf.add_subsurface(bar);
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
    }

    /// Handles an update from dmabuf task
    ///
    /// Translates the task update structure into lower
    /// level calls to import a dmabuf and update a image.
    /// Creates a new image if one doesn't exist yet.
    fn update_window_contents_from_dmabuf(&mut self, info: &UpdateWindowContentsFromDmabuf) {
        log::error!("Updating window {:?} with {:#?}", info.ufd_id, info);
        // Find the app corresponding to that window id
        let mut app = match self.wm_apps[info.ufd_id.into()].as_mut() {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => {
                log::error!("Could not find id {:?}", info.ufd_id);
                return;
            }
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

        if let Some(damage) = self.wm_atmos.take_buffer_damage(info.ufd_id) {
            app.a_image.as_mut().map(|i| i.reset_damage(damage));
        }
        if let Some(damage) = self.wm_atmos.take_surface_damage(info.ufd_id) {
            app.a_surf.damage(damage);
        }
        self.wm_thundr
            .bind_image(&mut app.a_surf, app.a_image.as_ref().unwrap().clone());
    }

    /// Handle update from memimage task
    ///
    /// Copies the shm buffer into the app's image.
    /// Creates a new image if one doesn't exist yet.
    fn update_window_contents_from_mem(&mut self, info: &UpdateWindowContentsFromMem) {
        log::error!("Updating window {:?} with {:#?}", info.id, info);
        // Find the app corresponding to that window id
        let mut app = match self.wm_apps[info.id.into()].as_mut() {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => {
                log::error!("Could not find id {:?}", info.id);
                return;
            }
        };
        let buffer_damage = self.wm_atmos.take_buffer_damage(info.id);
        let surface_damage = self.wm_atmos.take_surface_damage(info.id);
        // Damage the image
        if let Some(damage) = buffer_damage {
            app.a_image.as_mut().map(|i| i.reset_damage(damage));
        }
        if let Some(damage) = surface_damage {
            app.a_surf.damage(damage);
        }

        if let Some(image) = app.a_image.as_mut() {
            self.wm_thundr.update_image_from_bits(
                image,
                &info.pixels,
                // TODO: maybe don't do anything if there isn't damage?
                app.a_surf.get_image_damage().as_ref(),
                None,
            );
        } else {
            // If it does not have a image, then this must be the
            // first time contents were attached to it. Go ahead
            // and make one now
            app.a_image = self.wm_thundr.create_image_from_bits(&info.pixels, None);
        }

        self.wm_thundr
            .bind_image(&mut app.a_surf, app.a_image.as_ref().unwrap().clone());
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
    fn record_draw(&mut self) {
        // get the latest cursor position
        // ----------------------------------------------------------------
        let (cursor_x, cursor_y) = self.wm_atmos.get_cursor_pos();
        log::profiling!("Drawing cursor at ({}, {})", cursor_x, cursor_y);
        if let Some(cursor) = self.wm_cursor.as_mut() {
            cursor.set_pos(cursor_x as f32, cursor_y as f32);
        }
        // ----------------------------------------------------------------

        // Draw all of our windows on the desktop
        // Each app should have one or more windows,
        // all of which we need to draw.
        // ----------------------------------------------------------------
        self.wm_reordered.clear();
        self.wm_atmos_ids.clear();

        // Cache the inorder surface list from atmos
        // This helps us avoid nasty borrow checker stuff by avoiding recursion
        // ----------------------------------------------------------------
        let aids = &mut self.wm_atmos_ids;
        self.wm_atmos.map_inorder_on_surfs(|id| {
            aids.push(id);
            return true;
        });
        log::debug!("Surfacelist from atmos: {:?}", self.wm_atmos_ids);
        log::debug!("Current surface list: {:?}", self.wm_surface_ids);

        // Let's begin our stupidly weird reordering code!
        // Update our th::SurfaceList based on the atmosphere list
        //
        // This exists because I hate myself and I split category5 in half. Ways is updating
        // the surface positions and ordering, but vkcomp won't know about them until now.
        // We need to get our thundr surface list up to speed on what's happened since we
        // last used it, and so we need to do some reordering. We reorder instead of completely
        // regenerating because a thundr surface list accumulates damage based on changes
        // in surface order/insertion/removal.
        // ----------------------------------------------------------------
        // This weird while loop exists because we need to iterate fully through both
        // the atmos ids and our outdated surface ids
        let mut i = 0;
        while i < std::cmp::max(self.wm_atmos_ids.len(), self.wm_surface_ids.len()) {
            // This means that we are past the end of the correct surface list from atmos
            // and we should just remove everything remaining
            if i >= self.wm_atmos_ids.len() && i < self.wm_surface_ids.len() {
                // Even more gross. Because of our nasty while loop the wm_surface_ids len
                // will be shrinking, so we have to cash it and do it all here
                for _ in i..self.wm_surface_ids.len() {
                    // Again, use i because everything will be shifted in wm_surfaces
                    // while we are removing things
                    self.wm_surfaces.remove(i + 1);
                }
                self.wm_surface_ids.truncate(self.wm_atmos_ids.len());
                break;
            } else {
                // This is the id for this window in atmos's surface list
                let aid = self.wm_atmos_ids[i];
                // 1) if it has been reordered, that means we have already processed
                //    it and inserted it previously. We should remove it now
                // 2) else if the current id doesn't match our surfacelist, then
                //   2.1) insert it
                //   2.2) mark it as reordered
                // 3) else do nothing. It is already correct
                let cloned_surf = self.wm_apps[aid.into()].as_ref().unwrap().a_surf.clone();

                if i >= self.wm_surface_ids.len() {
                    // If our index is larger than the arrays, we just push new surfaces
                    self.wm_surface_ids.push(aid);
                    self.wm_reordered.push(aid);
                    // len - 1 since the last entry of the surfaceslist is always the
                    // desktop background.
                    self.wm_surfaces
                        .insert(self.wm_surfaces.len() as usize - 1, cloned_surf);
                } else if self.wm_surface_ids[i] != aid {
                    // Atmos did not match the id for the window at position i in our (outdated) list
                    // exclude based on 1)
                    if !self.wm_reordered.contains(&aid) {
                        // 2.*
                        // we add 1 since the 0th surf will always be the cursor
                        self.wm_surfaces.insert(i + 1, cloned_surf);
                        self.wm_surface_ids.insert(i, aid);
                        self.wm_reordered.push(aid);
                    } else {
                        // This window has been reordered, so as per 1) remove it
                        self.wm_surface_ids.remove(i);
                        self.wm_surfaces.remove(i + 1);
                    }
                }
            }

            i += 1;
        }
        log::debug!("New surface list: {:?}", self.wm_surface_ids);

        for (i, sid) in self.wm_surface_ids.iter().enumerate() {
            assert!(*sid == self.wm_atmos_ids[i]);
        }

        // do the draw call separately due to the borrow checker
        // throwing a fit if it is in the loop above.
        //
        // This section really just updates the size and position of all the
        // surfaces. They should already have images attached, and damage will
        // be calculated from the result.
        // ----------------------------------------------------------------
        for r_id in self.wm_surface_ids.iter() {
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
            if a.a_marked_for_death || !self.wm_atmos.get_window_in_use(a.a_id) {
                return;
            }

            // get parameters
            // ----------------------------------------------------------------
            let surface_pos = self.wm_atmos.get_surface_pos(a.a_id);
            let surface_size = self.wm_atmos.get_surface_size(a.a_id);

            // update the th::Surface pos and size
            a.a_surf.set_pos(surface_pos.0, surface_pos.1);
            a.a_surf.set_size(surface_size.0, surface_size.1);
            // ----------------------------------------------------------------

            // Only display the bar for toplevel surfaces
            // i.e. don't for popups
            if self.wm_atmos.get_toplevel(id) {
                // The bar should be a percentage of the screen height
                let barsize = self.wm_atmos.get_barsize();

                // Each toplevel window has two subsurfaces (in thundr): the
                // window bar and the window dot. If it's toplevel we are drawing SSD,
                // so we need to update the positions of these as well.
                let mut sub = a.a_surf.get_subsurface(0);
                let dims = Self::get_dot_dims(barsize, &surface_size);
                sub.set_pos(dims.0, dims.1);
                sub.set_size(dims.2, dims.3);

                let dims = Self::get_bar_dims(barsize, &surface_size);
                let mut sub = a.a_surf.get_subsurface(0);
                sub.set_pos(dims.0, dims.1);
                sub.set_size(dims.2, dims.3);
            }
        }
    }

    /// Flag this window to be killed.
    ///
    /// This adds it to our death list, which will be reaped next frame after
    /// we are done using its resources.
    fn close_window(&mut self, id: WindowId) {
        assert!(self.wm_apps.id_exists(id.into()));

        let mut app = self.wm_apps[id.into()].as_mut().unwrap();
        app.a_marked_for_death = true;
        self.wm_will_die.push(id);

        // Remove the surface. The surfacelist will damage the region that the
        // window occupied
        // This is haneld in the reordering bits
        //self.wm_surfaces.remove_surface(app.a_surf.clone());
    }

    /// Remove any apps marked for death. Usually we can't remove
    /// a window immediately because its image(s) are still being
    /// used by thundr
    fn reap_dead_windows(&mut self) {
        // Take a reference out here to avoid making the
        // borrow checker angry
        let thundr = &mut self.wm_thundr;

        // Only retain alive windows in the array
        for id in self.wm_will_die.drain(..) {
            if let Some(app) = self.wm_apps[id.into()].as_ref() {
                assert!(app.a_marked_for_death);

                // Destroy the rendering resources
                app.a_image
                    .as_ref()
                    .map(|image| thundr.destroy_image(image.clone()));

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
    fn begin_frame(&mut self) {
        self.record_draw();
        self.wm_thundr.draw_frame(&mut self.wm_surfaces);
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
    fn end_frame(&mut self) {
        self.wm_thundr.present();
    }

    pub fn process_task(&mut self, task: &Task) {
        log::info!("wm: got task {:?}", task);
        match task {
            Task::begin_frame => self.begin_frame(),
            Task::end_frame => self.end_frame(),
            // set background from mem
            Task::sbfm(sb) => {
                self.set_background_from_mem(sb.pixels.as_ref(), sb.width, sb.height);
            }
            // create new window
            Task::create_window(id) => {
                self.create_window(*id);
            }
            Task::close_window(id) => self.close_window(*id),
            // update window from gpu buffer
            Task::uwcfd(uw) => {
                self.update_window_contents_from_dmabuf(uw);
            }
            // update window from shm
            Task::uwcfm(uw) => {
                self.update_window_contents_from_mem(uw);
            }
        };
    }

    /// The main event loop of the vkcomp thread
    pub fn worker_thread(&mut self) {
        // first set the background
        let img = image::open("images/hurricane.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();
        self.set_background_from_mem(
            pixels.as_slice(),
            // dimensions of the texture
            512,
            512,
        );

        // how much time is spent drawing/presenting
        let mut draw_stop = StopWatch::new();

        loop {
            // Now that we have completed the previous frame, we can
            // release all the resources used to construct it while
            // we wait for our draw calls
            // note: -bad- this probably calls wayland locks
            self.wm_thundr.release_pending_resources();

            // Flip hemispheres to push our updates to vkcomp
            // this must be terrible for the local fauna
            //
            // This is a synchronization point. It will block
            self.wm_atmos.flip_hemispheres();

            if self.wm_atmos.get_renderdoc_recording() {
                self.wm_renderdoc
                    .start_frame_capture(std::ptr::null(), std::ptr::null());
            }

            // iterate through all the tasks that ways left
            // us in this hemisphere
            //  (aka process the work queue)
            while let Some(task) = self.wm_atmos.get_next_wm_task() {
                self.process_task(&task);
            }

            // start recording how much time we spent doing graphics
            log::debug!("_____________________________ FRAME BEGIN");
            // Create a frame out of the hemisphere we got from ways
            draw_stop.start();
            self.begin_frame();
            draw_stop.end();
            log::debug!(
                "spent {} ms drawing this frame",
                draw_stop.get_duration().as_millis()
            );

            self.wm_atmos.print_surface_tree();

            // present our frame
            draw_stop.start();
            self.end_frame();
            draw_stop.end();

            log::debug!(
                "spent {} ms presenting this frame",
                draw_stop.get_duration().as_millis()
            );

            if self.wm_atmos.get_renderdoc_recording() {
                self.wm_renderdoc
                    .end_frame_capture(std::ptr::null(), std::ptr::null());
            }
            self.reap_dead_windows();
            self.wm_atmos.release_consumables();

            log::debug!("_____________________________ FRAME END");
        }
    }
}

impl Drop for WindowManager {
    /// We need to free our resources before we free
    /// the renderer, since they were allocated from it.
    fn drop(&mut self) {
        // Free all images in each app
        for i in 0..self.wm_apps.len() {
            if let Some(a) = self.wm_apps[i].as_mut() {
                // now destroy the image
                self.wm_thundr
                    .destroy_image(a.a_image.as_ref().unwrap().clone());
            }
        }
        std::mem::drop(&self.wm_thundr);
    }
}
