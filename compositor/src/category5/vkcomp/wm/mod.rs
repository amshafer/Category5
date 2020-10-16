// A window management API for the vulkan backend
//
// Renderer: This is basically a big engine that
// drives the vulkan drawing commands.
// This is the slimy unsafe bit
//
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate image;

mod renderer;
use renderer::*;
use renderer::mesh::Mesh;

use crate::category5::utils::{
    *, atmosphere::*, timing::*, logging::*
};
use crate::log; // utils::logging::log
pub mod task;
use task::*;

use std::sync::mpsc::{Receiver, Sender};

// This consolidates the multiple resources needed
// to represent a titlebar
struct Titlebar {
    // The thick bar itself
    bar: Mesh,
    // One dot to rule them all. Used for buttons
    dot: Mesh
}

// This represents a client window.
//
// All drawn components are tracked with Mesh, this struct
// keeps track of the window components (content meshes and
// titlebar mesh).
//
// See WindowManager::record_draw for how this is displayed.
pub struct App {
    // This id uniquely identifies the App
    id: u32,
    // Because the images for meshes are used for both
    // buffers in a double buffer system, when an App is
    // deleted we need to avoid recording it in the next
    // frame's cbuf.
    //
    // When this flag is set, the we will not be recorded
    // and will instead be destroyed
    marked_for_death: bool,
    // This is the set of geometric objects in the application
    mesh: Option<Mesh>,
}

// Encapsulates vkcomp and provides a sensible windowing API
//
// This layer provides graphical operations to the above
// layers. It will support two classes of displayed objs,
// windows (has content and a titlebar) and sprites.
//
// Sprites should only be used for desktop effects, such
// as notifications. Sprites are not owned by a client
// whereas windows are.
pub struct WindowManager {
    // The channel to recieve work over
    wm_atmos: Atmosphere,
    // The vulkan renderer. It implements the draw logic,
    // whereas WindowManager implements organizational logic
    rend: Renderer,
    // This is the set of applications in this scene
    apps: Vec<App>,
    // The background picture of the desktop
    background: Option<Mesh>,
    // Image representing the software cursor
    cursor: Option<Mesh>,
    // Title bar to draw above the windows
    titlebar: Titlebar,
}

impl WindowManager {

    // Create a Titlebar resource
    //
    // The Titlebar will hold all of the components which make
    // up all of the titlebars in a scene. These meshes will
    // be colored differently when multidrawn
    fn get_default_titlebar(rend: &mut Renderer) -> Titlebar {
        let img = image::open("../bar.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();

        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8,
                                 4,
                                 64,
                                 64);

        let bar = rend.create_mesh(
            // TODO: make a way to change titlebar colors
            WindowContents::mem_image(&mimg),
            ReleaseInfo::none,
        ).unwrap();

        let img = image::open("../dot.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();
        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8,
                                 4,
                                 64,
                                 64);
        let dot = rend.create_mesh(
            WindowContents::mem_image(&mimg),
            ReleaseInfo::none,
        ).unwrap();

        Titlebar {
            bar: bar,
            dot: dot,
        }
    }

    fn get_default_cursor(rend: &mut Renderer) -> Option<Mesh> {
        let img = image::open("../cursor.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();
        let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8,
                                 4,
                                 64,
                                 64);

        rend.create_mesh(
            // TODO: calculate correct cursor size
            WindowContents::mem_image(&mimg),
            ReleaseInfo::none,
        )
    }

    // Create a new WindowManager
    //
    // This will create all the graphical resources needed for
    // the compositor. The WindowManager will create and own
    // the Renderer, thereby readying the display to draw.
    pub fn new(tx: Sender<Box<Hemisphere>>,
               rx: Receiver<Box<Hemisphere>>)
               -> WindowManager
    {
        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let mut rend = Renderer::new();
        rend.setup();

        let mut wm = WindowManager {
            wm_atmos: Atmosphere::new(tx, rx),
            titlebar: WindowManager::get_default_titlebar(&mut rend),
            cursor: WindowManager::get_default_cursor(&mut rend),
            rend: rend,
            apps: Vec::new(),
            background: None,
        };

        // Tell the atmosphere rend's resolution
        wm.wm_atmos.set_resolution(wm.rend.resolution.width,
                                   wm.rend.resolution.height);
        return wm;
    }

    // Set the desktop background for the renderer
    //
    // This basically just creates a mesh with the max
    // depth that takes up the entire screen.
    fn set_background_from_mem(&mut self,
                               texture: &[u8],
                               tex_width: u32,
                               tex_height: u32)
    {
        let mimg = MemImage::new(texture.as_ptr() as *mut u8,
                                 4,
                                 tex_width as usize,
                                 tex_height as usize);

        let mesh = self.rend.create_mesh(
            WindowContents::mem_image(&mimg),
            ReleaseInfo::none,
        );

        self.background = mesh;
    }
    
    // Add a mesh to the renderer to be displayed.
    //
    // The meshes are added to a list, and will be individually
    // dispatched for drawing later.
    //
    // Meshes need to be in an indexed vertex format.
    //
    // tex_res is the resolution of `texture`
    // window_res is the size of the on screen window
    fn create_window(&mut self, id: u32) {
        log!(LogLevel::info, "wm: Creating new window {}", id);

        self.apps.insert(0, App {
            id: id,
            marked_for_death: false,
            mesh: None,
        });
    }

    // Handles an update from dmabuf task
    //
    // Translates the task update structure into lower
    // level calls to import a dmabuf and update a mesh.
    // Creates a new mesh if one doesn't exist yet.
    fn update_window_contents_from_dmabuf(&mut self,
                                          info: &UpdateWindowContentsFromDmabuf)
    {
        // Find the app corresponding to that window id
        let app = match self.apps
            .iter_mut()
            .find(|app| app.id == info.ufd_id)
        {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => { log!(LogLevel::error, "Could not find id {}", info.ufd_id); return; },
        };

        if !app.mesh.is_none() {
            self.rend.update_app_contents(
                app,
                WindowContents::dmabuf(&info.ufd_dmabuf),
                ReleaseInfo::dmabuf(DmabufReleaseInfo {
                    dr_fd: info.ufd_dmabuf.db_fd,
                    dr_wl_buffer: info.ufd_wl_buffer.clone(),
                }),
            );
        } else {
            // If it does not have a mesh, then this must be the
            // first time contents were attached to it. Go ahead
            // and make one now
            app.mesh = self.rend.create_mesh(
                WindowContents::dmabuf(&info.ufd_dmabuf),
                ReleaseInfo::dmabuf(DmabufReleaseInfo {
                    dr_fd: info.ufd_dmabuf.db_fd,
                    dr_wl_buffer: info.ufd_wl_buffer.clone(),
                }),
            );
        }
    }

    // Handle update from memimage task
    //
    // Copies the shm buffer into the app's mesh.
    // Creates a new mesh if one doesn't exist yet.
    fn update_window_contents_from_mem(&mut self,
                                       info: &UpdateWindowContentsFromMem)
    {
        // Find the app corresponding to that window id
        let app = match self.apps
            .iter_mut()
            .find(|app| app.id == info.id)
        {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => {
                log!(LogLevel::error, "Could not find id {}", info.id);
                return;
            },
        };

        if !app.mesh.is_none() {
            self.rend.update_app_contents(
                app,
                WindowContents::mem_image(&info.pixels),
                ReleaseInfo::mem_image,
            );
        } else {
            // If it does not have a mesh, then this must be the
            // first time contents were attached to it. Go ahead
            // and make one now
            app.mesh = Some(self.rend.create_mesh(
                WindowContents::mem_image(&info.pixels),
                ReleaseInfo::mem_image,
            ).unwrap());
        }
    }

    // Handles generating draw commands for one window
    fn record_draw_for_id(&self,
                          id: WindowId,
                          order: usize,
                          params: &RecordParams)
    {
        let a = match self.apps.iter().find(|&a| a.id == id) {
            Some(a) => a,
            // app must have been closed
            None => return,
        };
        // If this window has been closed or if it is not ready for
        // rendering, ignore it
        if a.marked_for_death || !self.wm_atmos.is_in_use(a.id) {
            return;
        }

        // The bar should be a percentage of the screen height
        let barsize = self.wm_atmos.get_barsize();
        // The dotsize should be just slightly smaller
        let dotsize = barsize * 0.95;
        let window_dims = self.wm_atmos.get_window_dimensions(a.id);
        // Convert the order into a float from 0.0 to 1.0
        let order_depth = ((order + 1) as f32) / 100.0;

        // Only display the bar for toplevel surfaces
        // i.e. don't for popups
        if self.wm_atmos.is_toplevel(id) {
            // now render the bar itself, as wide as the window
            // the bar needs to be behind the dots
            let push = PushConstants {
                order: order_depth - 0.001, // depth
                // align it at the top right
                x: window_dims.0,
                // draw the bar above the window
                y: window_dims.1 - barsize,
                // the bar is as wide as the window
                width: window_dims.2,
                // use a percentage of the screen size
                height: barsize,
            };
            self.titlebar.bar
                .record_draw(&self.rend, params, &push);

            // We should render the dot second, so alpha blending
            // has a color to use
            let push = PushConstants {
                order: order_depth - 0.002, // depth
                // the x position needs to be all the way to the
                // right side of the bar
                x: window_dims.0
                // Multiply by 2 (see vert shader for details)
                    + window_dims.2
                // we don't want to go past the end of the bar
                    - barsize,
                y: window_dims.1 - barsize,
                // align it at the top right
                width: dotsize,
                height: dotsize,
            };
            // render buttons on the titlebar
            self.titlebar.dot
                .record_draw(&self.rend, params, &push);
        }

        // Finally, we can draw the window itself
        // If the mesh does not exist, then only the titlebar
        // and other window decorations will be drawn
        if let Some(mesh) = &a.mesh {
            // TODO: else draw blank mesh?
            let push = PushConstants {
                order: order_depth, // depth
                x: window_dims.0,
                y: window_dims.1,
                // align it at the top right
                width: window_dims.2,
                height: window_dims.3,
            };
            mesh.record_draw(&self.rend, params, &push);
        }
    }

    // Record all the drawing operations for the current scene
    //
    // Vulkan requires that we record a list of operations into a command
    // buffer which is later submitted for display. This method organizes
    // the recording of draw operations for all elements in the desktop.
    //
    // params: a private info structure for the Renderer. It holds all
    // the data about what we are recording.
    fn record_draw(&self, params: &RecordParams) {
        // Each app should have one or more windows,
        // all of which we need to draw.
        for (i, id) in self.wm_atmos.visible_windows().enumerate() {
            // Render any subsurfaces first
            for (j, sub) in self.wm_atmos.visible_subsurfaces(id).enumerate() {
                // TODO: Make this recursive??
                self.record_draw_for_id(sub, j, params);
            }
            // Now render this window
            self.record_draw_for_id(id, i, params);
        }

        // Draw the background last, painter style
        self.background.as_ref().map(|back| {
            back.record_draw(
                &self.rend,
                params,
                &PushConstants {
                    order: 0.99999, // make it the max depth
                    // size of the window on screen
                    x: 0.0,
                    y: 0.0,
                    // align it at the top left
                    width: self.rend.resolution.width as f32,
                    height: self.rend.resolution.height as f32,
                },
            );
        });

        // get the latest cursor position
        let (cursor_x, cursor_y) = self.wm_atmos.get_cursor_pos();
        log!(LogLevel::profiling, "Drawing cursor at ({}, {})",
             cursor_x,
             cursor_y);

        self.cursor.as_ref().map(|cursor| {
            cursor.record_draw(
                &self.rend,
                params,
                &PushConstants {
                    order: 0.0001, // make it the min depth
                    // put it in the center
                    x: cursor_x as f32,
                    y: cursor_y as f32,
                    // TODO: calculate cursor size
                    width: 16.0,
                    height: 16.0,
                },
            );
        });
    }

    fn close_window(&mut self, id: u32) {
        // if it exists, mark it for death
        self.apps.iter_mut().find(|app| app.id == id).map(|app| {
            app.marked_for_death = true;
        });
    }

    // Remove any apps marked for death
    fn reap_dead_windows(&mut self) {
        // Take a reference out here to avoid making the
        // borrow checker angry
        let rend = &self.rend;

        // Only retain alive windows in the array
        self.apps.retain(|app| {
                if app.marked_for_death {
                    // Destroy the rendering resources
                    app.mesh.as_ref().map(
                        |mesh| mesh.destroy(rend)
                    );

                    return false;
                }
                return true;
            });
    }

    // A helper which records the cbuf for the next frame
    //
    // Recording a frame follows this general pattern:
    //  1. The recording parameters are requested.
    //  2. Recording is started.
    //  3. WindowManager specifies the order/position of Meshes
    //     to be recorded.
    //  4. Recording is stopped.
    //
    // This *does not* present anything to the screen
    fn record_next_frame(&mut self) {
        let params = self.rend.get_recording_parameters();
        self.rend.begin_recording_one_frame(&params);

        self.record_draw(&params);

        self.rend.end_recording_one_frame(&params);
    }

    // Begin rendering a frame
    //
    // Vulkan is asynchronous, meaning that commands are submitted
    // and later waited on. This method records the next cbuf
    // and asks the Renderer to submit it.
    //
    // The frame is not presented to the display until
    // WindowManager::end_frame is called.
    fn begin_frame(&mut self) {
        self.record_next_frame();
        self.rend.begin_frame();
    }

    // End a frame
    //
    // Once the frame's cbuf has been recorded and submitted, we
    // can present it to the physical display.
    //
    // It is possible that the upper layers may want to perform
    // operations between submission of the frame and when that
    // frame is presented, which is why begin/end frame is split
    // into two methods.
    fn end_frame(&mut self) {
        self.rend.present();
    }

    pub fn process_task(&mut self, task: &Task) {
        log!(LogLevel::info, "wm: got task {:?}", task);
        match task {
            Task::begin_frame => self.begin_frame(),
            Task::end_frame => self.end_frame(),
            // set background from mem
            Task::sbfm(sb) => {
                self.set_background_from_mem(
                    sb.pixels.as_ref(),
                    sb.width,
                    sb.height,
                );
            },
            // create new window
            Task::create_window(id) => {
                self.create_window(*id);
            },
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

    pub fn worker_thread(&mut self) {
        // first set the background
        let img =
            image::open("/home/ashafer/git/compositor_playground/hurricane.png")
            .unwrap()
            .to_rgba();
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
            self.rend.release_pending_resources();

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
            draw_stop.start();

            // get the next frame to draw into
            self.rend.get_next_swapchain_image();

            // Create a frame out of the hemisphere we got from ways
            self.begin_frame();
            self.reap_dead_windows();
            // present our frame
            self.end_frame();
            draw_stop.end();

            log!(LogLevel::profiling, "spent {} ms drawing this frame",
                 draw_stop.get_duration().as_millis());
        }
    }
}

impl Drop for WindowManager {
    // We need to free our resources before we free
    // the renderer, since they were allocated from it.
    fn drop(&mut self) {
        // Free all meshes in each app
        for a in self.apps.iter_mut() {
            a.mesh.as_ref().unwrap().destroy(&self.rend);
        }
        self.titlebar.bar.destroy(&self.rend);
        self.titlebar.dot.destroy(&self.rend);

        if let Some(m) = &mut self.background {
            m.destroy(&self.rend);
        }

        std::mem::drop(&self.rend);
    }
}
