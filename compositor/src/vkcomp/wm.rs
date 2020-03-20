// A window management API for the vulkan backend
//
// Austin Shafer - 2020

use super::renderer::*;

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
// See Renderer::record_draw for how this is displayed.
pub struct App {
    // This is the set of geometric objects in the application
    meshes: Vec<Mesh>,
    // The position and size of the window
    push: PushConstants,
}

// Encapsulates vkcomp and provides a sensible windowing API
pub struct WindowManager {
    // The vulkan renderer. It implements the draw logic,
    // whereas WindowManager implements organizational logic
    rend: Renderer,
    // This is the set of applications in this scene
    apps: Vec<App>,
    background: Option<Mesh>,
    // Title bar to draw above the windows
    titlebar: Titlebar,
}

pub struct WindowCreateInfo<'a> {
    // Window position
    pub x: u32,
    pub y: u32,
    // Memory region to copy window contents from
    pub tex: &'a [u8],
    // The resolution of the texture
    pub tex_width: u32,
    pub tex_height: u32,
    // The size of the window (in pixels)
    pub window_width: u32,
    pub window_height: u32,
    // The depth z-ordering from [0.005, 1.0)
    pub order: f32
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

    pub fn new() -> WindowManager {
        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let mut rend = Renderer::new();
        rend.setup();

        WindowManager {
            titlebar: WindowManager::get_default_titlebar(&mut rend),
            rend: rend,
            apps: Vec::new(),
            background: None,
        }
    }

    // Set the desktop background for the renderer
    //
    // This basically just creates a mesh with the max
    // depth that takes up the entire screen.
    pub fn set_background_from_mem(&mut self,
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
    pub fn create_window(&mut self,
                         info: &WindowCreateInfo)
    {
        let meshes = vec!{
            self.rend.create_mesh(
                info.tex,
                info.tex_width,
                info.tex_height,
            ).unwrap(),
        };

        self.apps.insert(0, App {
            meshes: meshes,
            // TODO: properly track window orderings
            push: PushConstants {
                order: info.order,
                x: info.x,
                y: info.y,
                width: info.window_width as f32,
                height: info.window_height as f32,
            },
        });
    }

    fn record_draw(&self, params: &RecordParams) {
        // Each app should have one or more windows,
        // all of which we need to draw.
        for a in self.apps.iter() {
            for mesh in a.meshes.iter() {
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
                mesh.record_draw(&self.rend, params, &a.push);
            }
        }

        // Draw the background last, painter style
        self.background.as_ref().unwrap().record_draw(
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
    }

    // A helper which records the cbuf for the next frame
    pub fn record_next_frame(&mut self) {
        let params = self.rend.get_recording_parameters();
        self.rend.begin_recording_one_frame(&params);

        self.record_draw(&params);

        self.rend.end_recording_one_frame(&params);
    }

    pub fn begin_frame(&mut self) {
        self.record_next_frame();
        self.rend.begin_frame();
    }

    pub fn end_frame(&mut self) {
        self.rend.present();
    }
}

impl Drop for WindowManager {
    // We need to free our resources before we free
    // the renderer, since they were allocated from it.
    fn drop(&mut self) {
        // Free all meshes in each app
        for a in self.apps.iter_mut() {
            for mesh in a.meshes.iter_mut() {
                mesh.destroy(&self.rend);
            }
        }
        self.titlebar.bar.destroy(&self.rend);
        self.titlebar.dot.destroy(&self.rend);

        if let Some(m) = &mut self.background {
            m.destroy(&self.rend);
        }

        std::mem::drop(&self.rend);
    }
}
