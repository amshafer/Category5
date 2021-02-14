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
    /// The channel to recieve work over
    wm_atmos: Atmosphere,
    /// The vulkan renderer. It implements the draw logic,
    /// whereas WindowManager implements organizational logic
    wm_thundr: th::Thundr,
    /// This is the thundr surface list constructed from the resources that
    /// ways notified us of. Our job is to keep this up to date and call Thundr.
    wm_surfaces: th::SurfaceList,
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
}

impl WindowManager {
    /// Create a Titlebar resource
    ///
    /// The Titlebar will hold all of the components which make
    /// up all of the titlebars in a scene. These imagees will
    /// be colored differently when multidrawn
    fn get_default_titlebar(rend: &mut th::Thundr) -> Titlebar {
        let img = image::open("images/bar.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();

        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);

        // TODO: make a way to change titlebar colors
        let mut bar = rend.create_image_from_bits(&mimg, None).unwrap();
        bar.set_damage(0, 0, 64, 64);

        let img = image::open("images/dot.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();
        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);
        let mut dot = rend.create_image_from_bits(&mimg, None).unwrap();
        dot.set_damage(0, 0, 64, 64);

        Titlebar { bar: bar, dot: dot }
    }

    fn get_default_cursor(rend: &mut th::Thundr) -> Option<th::Surface> {
        let img = image::open("images/cursor.png").unwrap().to_rgba();
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
        let info = th::CreateInfo::builder()
            //.enable_traditional_composition()
            .build();
        let mut rend = th::Thundr::new(&info).unwrap();

        let mut wm = WindowManager {
            wm_atmos: Atmosphere::new(tx, rx),
            wm_titlebar: WindowManager::get_default_titlebar(&mut rend),
            wm_cursor: WindowManager::get_default_cursor(&mut rend),
            wm_thundr: rend,
            wm_surfaces: th::SurfaceList::new(),
            wm_will_die: Vec::new(),
            wm_apps: PropertyList::new(),
            wm_background: None,
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

        self.wm_apps.update_or_create(
            id.into(),
            App {
                a_id: id,
                a_marked_for_death: false,
                a_surf: self.wm_thundr.create_surface(0.0, 0.0, 0.0, 0.0),
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

        // TODO: use real damage
        app.a_image.as_mut().map(|i| i.set_damage(0, 0, 500, 500));
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

        if let Some(image) = app.a_image.as_mut() {
            self.wm_thundr
                .update_image_from_bits(image, &info.pixels, None);
        } else {
            // If it does not have a image, then this must be the
            // first time contents were attached to it. Go ahead
            // and make one now
            app.a_image = self.wm_thundr.create_image_from_bits(&info.pixels, None);
        }

        // TODO: use correct damage
        app.a_image
            .as_mut()
            .map(|i| i.set_damage(0, 0, info.width as i32, info.height as i32));
        self.wm_thundr
            .bind_image(&mut app.a_surf, app.a_image.as_ref().unwrap().clone());
    }

    /// Handles generating draw commands for one window
    fn record_draw_for_id(&mut self, id: WindowId) {
        let a = match self.wm_apps[id.into()].as_mut() {
            Some(a) => a,
            // app must have been closed
            None => {
                log::debug!("Could not find id {:?} to record for drawing", id);
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
        // The bar should be a percentage of the screen height
        let barsize = self.wm_atmos.get_barsize();
        // The dotsize should be just slightly smaller
        let dotsize = barsize * 0.95;
        let surface_pos = self.wm_atmos.get_surface_pos(a.a_id);
        let surface_size = self.wm_atmos.get_surface_size(a.a_id);

        // update the th::Surface pos and size
        a.a_surf.set_pos(surface_pos.0, surface_pos.1);
        a.a_surf.set_size(surface_size.0, surface_size.1);
        // ----------------------------------------------------------------

        // Only display the bar for toplevel surfaces
        // i.e. don't for popups
        if self.wm_atmos.get_toplevel(id) {
            // draw buttons on the titlebar
            // ----------------------------------------------------------------
            let mut dot = self.wm_thundr.create_surface(
                surface_pos.0
                // Multiply by 2 (see vert shader for details)
                    + surface_size.0
                // we don't want to go past the end of the bar
                    - barsize,
                surface_pos.1 - barsize,
                // align it at the top right
                dotsize, // width
                dotsize, // height
            );
            self.wm_thundr
                .bind_image(&mut dot, self.wm_titlebar.dot.clone());
            self.wm_surfaces.push(dot);
            // ----------------------------------------------------------------

            // now render the bar itself, as wide as the window
            // the bar needs to be behind the dots
            // ----------------------------------------------------------------
            let mut bar = self.wm_thundr.create_surface(
                // align it at the top right
                surface_pos.0,
                // draw the bar above the window
                surface_pos.1 - barsize,
                // the bar is as wide as the window
                surface_size.0,
                // use a percentage of the screen size
                barsize,
            );
            self.wm_thundr
                .bind_image(&mut bar, self.wm_titlebar.bar.clone());
            self.wm_surfaces.push(bar);
            // ----------------------------------------------------------------
        }

        // Finally, we can draw the window itself
        // If the image does not exist, then only the titlebar
        // and other window decorations will be drawn
        if a.a_image.is_some() {
            // ----------------------------------------------------------------
            self.wm_surfaces.push(a.a_surf.clone());
            // ----------------------------------------------------------------
        }
    }

    /// Recursively get the list of surface and subsurface ids
    fn get_ids_to_record(&self, i: &mut i32, ids: &mut Vec<WindowId>, id: WindowId) {
        // Render any subsurfaces first. The surface list for thundr
        // is from front to back, so we push these before the main surface
        for sub in self.wm_atmos.visible_subsurfaces(id) {
            self.get_ids_to_record(i, ids, sub);
        }

        ids.push(id);
        // Increment the counter recursively
        *i += 1;
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
        // recreate our surface list to pass to thundr
        self.wm_surfaces.clear();

        // get the latest cursor position
        // ----------------------------------------------------------------
        let (cursor_x, cursor_y) = self.wm_atmos.get_cursor_pos();
        log::profiling!("Drawing cursor at ({}, {})", cursor_x, cursor_y);
        if let Some(cursor) = self.wm_cursor.as_mut() {
            cursor.set_pos(cursor_x as f32, cursor_y as f32);
            self.wm_surfaces.push(cursor.clone());
        }
        // ----------------------------------------------------------------

        // Draw all of our windows on the desktop
        // Each app should have one or more windows,
        // all of which we need to draw.
        // ----------------------------------------------------------------
        let mut ids = Vec::new();
        let mut i = 0;
        for id in self.wm_atmos.visible_windows() {
            self.get_ids_to_record(&mut i, &mut ids, id);
        }

        // do the draw call separately due to the borrow checker
        // throwing a fit if it is in the loop above
        for id in ids {
            // Now render the windows
            self.record_draw_for_id(id);
        }
        // ----------------------------------------------------------------

        // Draw the desktop background last
        // ----------------------------------------------------------------
        if let Some(back) = self.wm_background.as_ref() {
            self.wm_surfaces.push(back.clone());
        }
        // ----------------------------------------------------------------
    }

    /// Flag this window to be killed.
    ///
    /// This adds it to our death list, which will be reaped next frame after
    /// we are done using its resources.
    fn close_window(&mut self, id: WindowId) {
        assert!(self.wm_apps.id_exists(id));

        self.wm_apps[i].as_mut().unwrap().a_marked_for_death = true;
        self.wm_will_die.push(id);
    }

    /// Remove any apps marked for death. Usually we can't remove
    /// a window immediately because its image(s) are still being
    /// used by thundr
    fn reap_dead_windows(&mut self) {
        // Take a reference out here to avoid making the
        // borrow checker angry
        let thundr = &mut self.wm_thundr;

        // Only retain alive windows in the array
        for i in self.wm_will_die.drain(..) {
            if let Some(app) = self.wm_apps[i].as_ref() {
                assert!(app.a_marked_for_death);

                // Destroy the rendering resources
                app.a_image
                    .as_ref()
                    .map(|image| thundr.destroy_image(image.clone()));

                self.wm_apps.deactivate(i)
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

            self.reap_dead_windows();

            // present our frame
            draw_stop.start();
            self.end_frame();
            draw_stop.end();

            log::debug!(
                "spent {} ms presenting this frame",
                draw_stop.get_duration().as_millis()
            );

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
