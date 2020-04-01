// A window management API for the vulkan backend
//
// Austin Shafer - 2020
#![allow(dead_code)]

// Renderer: This is basically a big engine that
// drives the vulkan drawing commands.
// This is the slimy unsafe bit
mod renderer;
use renderer::*;
use renderer::mesh::Mesh;

pub mod task;
use task::*;

use std::sync::mpsc;
use std::sync::mpsc::{Receiver};
use std::time::Duration;
use std::thread;

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
// titlebar mesh) and the location/size (push constants).
//
// See WindowManager::record_draw for how this is displayed.
pub struct App {
    // This id uniquely identifies the App
    id: u64,
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
    // The position and size of the window
    push: PushConstants,
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
    // The vulkan renderer. It implements the draw logic,
    // whereas WindowManager implements organizational logic
    rend: Renderer,
    // The channel to recieve work over
    rx: Receiver<Task>,
    // This is the set of applications in this scene
    apps: Vec<App>,
    // The background picture of the desktop
    background: Option<Mesh>,
    // Image representing the software cursor
    cursor: Option<Mesh>,
    // The location of the cursor
    cursor_x: f64,
    cursor_y: f64,
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
        let bar = rend.create_mesh(
            // TODO: make a way to change titlebar colors
            pixels.as_slice(),
            64,
            64,
        ).unwrap();

        let img = image::open("../dot.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();
        let dot = rend.create_mesh(
            // TODO: make a way to change titlebar colors
            pixels.as_slice(),
            64,
            64,
        ).unwrap();

        Titlebar {
            bar: bar,
            dot: dot,
        }
    }

    fn get_default_cursor(rend: &mut Renderer) -> Option<Mesh> {
        let img = image::open("../cursor.png").unwrap().to_rgba();
        let pixels: Vec<u8> = img.into_vec();

        rend.create_mesh(
            // TODO: calculate correct cursor size
            pixels.as_slice(),
            64,
            64,
        )
    }

    // Create a new WindowManager
    //
    // This will create all the graphical resources needed for
    // the compositor. The WindowManager will create and own
    // the Renderer, thereby readying the display to draw.
    pub fn new(rx: Receiver<Task>) -> WindowManager {
        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let mut rend = Renderer::new();
        rend.setup();

        WindowManager {
            titlebar: WindowManager::get_default_titlebar(&mut rend),
            cursor: WindowManager::get_default_cursor(&mut rend),
            cursor_x: 0.0,
            cursor_y: 0.0,
            rend: rend,
            apps: Vec::new(),
            background: None,
            rx: rx,
        }
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
        let mesh = self.rend.create_mesh(
            texture,
            tex_width,
            tex_height,
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
    fn create_window(&mut self, info: &CreateWindow) {
        println!("wm: Creating new window of size {}x{}",
                 info.window_width, info.window_height,
        );

        self.apps.insert(0, App {
            id: info.id,
            marked_for_death: false,
            mesh: None,
            // TODO: properly track window orderings
            push: PushConstants {
                order: 0.005,
                x: info.x,
                y: info.y,
                width: info.window_width as f32,
                height: info.window_height as f32,
            },
        });
    }

    fn update_window_contents_from_mem(&mut self,
                                       info: &UpdateWindowContentsFromMem)
    {
        println!("wm: Updating app mesh using texture of size {}x{}",
                 info.width, info.height,
        );

        // Find the app corresponding to that window id
        let app = match self.apps
            .iter_mut()
            .find(|app| app.id == info.id)
        {
            Some(a) => a,
            // If the id is not found, then don't update anything
            None => { println!("Could not find id {}", info.id); return; },
        };

        if !app.mesh.is_none() {
            self.rend.update_app_contents_from_mem(
                app,
                &info.pixels,
            );
        } else {
            // If it does not have a mesh, then this must be the
            // first time contents were attached to it. Go ahead
            // and make one now
            app.mesh = Some(self.rend.create_mesh(
                info.pixels.as_ref(),
                info.width as u32,
                info.height as u32,
            ).unwrap());
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
        for a in self.apps.iter() {
            // If this window has been closed, ignore it
            if a.marked_for_death {
                continue;
            }

            // The bar should be a percentage of the screen height
            let barsize =
                self.rend.resolution.height as f32 * 0.02;
            // The dotsize should be just slightly smaller
            let dotsize = barsize * 0.95;
            // now render the bar itself, as wide as the window
            // the bar needs to be behind the dots
            let push = PushConstants {
                order: a.push.order - 0.001, // depth
                // align it at the top right
                x: a.push.x,
                y: a.push.y,
                // the bar is as wide as the window
                width: a.push.width,
                // use a percentage of the screen size
                height: barsize,
            };
            self.titlebar.bar
                .record_draw(&self.rend, params, &push);

            // We should render the dot second, so alpha blending
            // has a color to use
            let push = PushConstants {
                order: a.push.order - 0.002, // depth
                // the x position needs to be all the way to the
                // right side of the bar
                x: a.push.x
                // Multiply by 2 (see vert shader for details)
                    + a.push.width as u32 * 2
                // we don't want to go past the end of the bar
                    - barsize as u32 * 2,
                y: a.push.y,
                // align it at the top right
                width: dotsize,
                height: dotsize,
            };
            // render buttons on the titlebar
            self.titlebar.dot
                .record_draw(&self.rend, params, &push);

            // Finally, we can draw the window itself
            // If the mesh does not exist, then only the titlebar
            // and other window decorations will be drawn
            if let Some(mesh) = &a.mesh {
                // TODO: else draw blank mesh?
                mesh.record_draw(&self.rend, params, &a.push);
            }
        }

        // Draw the background last, painter style
        self.background.as_ref().map(|back| {
            back.record_draw(
                &self.rend,
                params,
                &PushConstants {
                    order: 0.99999, // make it the max depth
                    // size of the window on screen
                    x: 0,
                    y: 0,
                    // align it at the top left
                    width: self.rend.resolution.width as f32,
                    height: self.rend.resolution.height as f32,
                },
            );
        });

        self.cursor.as_ref().map(|cursor| {
            cursor.record_draw(
                &self.rend,
                params,
                &PushConstants {
                    order: 0.0001, // make it the min depth
                    // put it in the center
                    x: self.cursor_x as u32,
                    y: self.cursor_y as u32,
                    // TODO: calculate cursor size
                    width: 16.0,
                    height: 16.0,
                },
            );
        });
    }

    fn close_window(&mut self, id: u64) {
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
                    app.mesh.as_ref().map(|mesh| mesh.destroy(rend));

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
        match task {
            Task::begin_frame => self.begin_frame(),
            Task::end_frame => self.end_frame(),
            Task::mc(mc) => {
                self.cursor_x += mc.x;
                self.cursor_y += mc.y;
            },
            Task::close_window(id) => self.close_window(*id),
            Task::sbfm(sb) => {
                self.set_background_from_mem(
                    sb.pixels.as_ref(),
                    sb.width,
                    sb.height,
                );
            },
            Task::cw(cw) => {
                self.create_window(cw);
            },
            Task::uwcfm(uw) => {
                self.update_window_contents_from_mem(uw);
            }
        };
    }

    pub fn worker_thread(&mut self) {
        loop {
            // Block for any new tasks
            let task = self.rx.recv().unwrap();
            self.process_task(&task);

            // We have already done one task, but the previous
            // frame might not be done. We should keep processing
            // tasks until it is ready
            while !self.rend.try_get_next_swapchain_image() {
                match self.rx.try_recv() {
                    Ok(task) => self.process_task(&task),
                    // If it times out just continue
                    Err(mpsc::TryRecvError::Empty) => {
                        // We are not able to use recv_timeout due
                        // to https://github.com/rust-lang/rust/issues/39364
                        // Instead we need to try to recv and wait
                        // some number of ms if it was not successful
                        //
                        // It doesn't look like this bug will be fixed
                        // anytime soon
                        thread::sleep(Duration::from_millis(8));
                    },
                    Err(err) =>
                        panic!("Error while waiting for tasks: {:?}", err),
                };
            }

            self.begin_frame();
            self.reap_dead_windows();
            self.end_frame();
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
