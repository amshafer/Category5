//! # The Thundr rendering toolkit.
//!
//! Thundr is a Vulkan composition library for use in ui toolkits and
//! wayland compositors. You use it to create a set of images from
//! textures or window contents, attach those images to surfaces, and pass
//! a list of surfaces to thundr for rendering.
//!
//! Thundr also supports multiple methods of drawing:
//! * `compute` - Uses compute shaders to perform compositing.
//! * `geometric` - This is a more "traditional" manner of drawing ui elements:
//! surfaces are drawn as textured quads in 3D space.
//!
//! The compute pipeline is more optimized, and is the default. The
//! geometric pipeline serves as a backup for situations in which the
//! compute pipeline does not perform well or is not supported.
//!
//! ## Drawing API
//!
//! The general flow of a thundr client is as follows:
//! * Create an Image (`create_image_from*`)
//!   * Use a MemImage to load a texture from raw bits.
//!   * Use a dmabuf to load a image contents from a gpu buffer.
//! * Create a Surface (`create_surface`)
//!   * Assign it a location and a size
//! * Bind the image to the surface (`bind_image`)
//! * Create a surface list (`SurfaceList::new()`)
//!   * Push the surfaces you'd like rendered into the list from front to
//!   back (`SurfaceList.push`)
//! * Tell Thundr to launch the work on the gpu (`draw_frame`)
//! * Present the rendering results on screen (`present`)
//!
//! ```
//! use thundr as th;
//!
//! let thund: th::Thundr = Thundr::new();
//!
//! // First load our texture into memory
//! let img = image::open("images/cursor.png").unwrap().to_rgba();
//! let pixels: Vec<u8> = img.into_vec();
//! let mimg = MemImage::new(
//!     pixels.as_slice().as_ptr() as *mut u8,
//!     4,  // width of a pixel
//!     64, // width of texture
//!     64  // height of texture
//! );
//!
//! // Create an image from our MemImage
//! let image = thund.create_image_from_bits(&mimg, None).unwrap();
//! // Now create a 16x16 surface at position (0, 0)
//! let mut surf = thund.create_surface(0.0, 0.0, 16.0, 16.0);
//! // Assign our image to our surface
//! thund.bind_image(&mut surf, image);
//! ```
//! ## Requirements
//!
//! Thundr requires a system with vulkan 1.2+ installed. The following
//! extensions are used:
//! * VK_KHR_surface
//! * VK_KHR_display
//! * VK_EXT_maintenance2
//! * VK_KHR_debug_report
//! * VK_KHR_descriptor_indexing
//! * VK_KHR_external_memory

// Austin Shafer - 2020

mod damage;
mod descpool;
mod display;
mod image;
mod list;
mod pipelines;
mod renderer;
mod surface;

pub use damage::Damage;
pub use image::Image;
pub use list::SurfaceList;
pub use renderer::Renderer;
pub use surface::Surface;

// Re-export some things from utils so clients
// can use them
extern crate utils;
pub use crate::utils::region::Rect;
pub use crate::utils::{Dmabuf, MemImage};

#[macro_use]
extern crate memoffset;
use pipelines::*;

pub struct Thundr {
    th_rend: Renderer,

    /// Application specific stuff that will be set up after
    /// the original initialization
    pub(crate) th_pipe: Box<dyn Pipeline>,
}

pub enum ThundrSurfaceType {
    Display,
    X11,
}

/// Parameters for Renderer creation.
///
/// These will be set by Thundr based on the Pipelines that will
/// be enabled. See `Pipeline` for methods that drive the data
/// contained here.
pub struct ThundrCreateInfo {
    /// A list of queue family indexes to create the device with
    pub enable_compute_composition: bool,
    pub enable_traditional_composition: bool,
    pub surface_type: ThundrSurfaceType,
}

impl ThundrCreateInfo {
    pub fn builder() -> ThundrCreateInfoBuilder {
        ThundrCreateInfoBuilder {
            ci: ThundrCreateInfo {
                enable_compute_composition: true,
                enable_traditional_composition: false,
                surface_type: ThundrSurfaceType::Display,
            },
        }
    }
}

/// Implements the builder pattern for easier thundr creation
pub struct ThundrCreateInfoBuilder {
    ci: ThundrCreateInfo,
}
impl ThundrCreateInfoBuilder {
    pub fn enable_compute_composition<'a>(&'a mut self) -> &'a mut Self {
        self.ci.enable_compute_composition = true;
        self
    }

    pub fn enable_traditional_composition<'a>(&'a mut self) -> &'a mut Self {
        self.ci.enable_traditional_composition = true;
        self
    }
    pub fn surface_type<'a>(&'a mut self, ty: ThundrSurfaceType) -> &'a mut Self {
        self.ci.surface_type = ty;
        self
    }

    pub fn build(mut self) -> ThundrCreateInfo {
        self.ci
    }
}

// This is the public facing thundr api. Don't change it
impl Thundr {
    // TODO: make get_available_params and add customization
    pub fn new(info: &ThundrCreateInfo) -> Result<Thundr, &'static str> {
        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let mut rend = Renderer::new(&info);

        // Create the pipeline(s) requested
        let pipe: Box<dyn Pipeline> = if info.enable_compute_composition {
            Box::new(CompPipeline::new(&mut rend))
        } else if info.enable_traditional_composition {
            Box::new(GeomPipeline::new(&mut rend))
        } else {
            return Err("Please select a composition type in ThundrCreateInfo");
        };

        Ok(Thundr {
            th_rend: rend,
            th_pipe: pipe,
        })
    }

    pub fn get_resolution(&self) -> (u32, u32) {
        (
            self.th_rend.resolution.width,
            self.th_rend.resolution.height,
        )
    }

    // create_image_from_bits
    pub fn create_image_from_bits(
        &mut self,
        img: &MemImage,
        release_info: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        self.th_rend.create_image_from_bits(&img, release_info)
    }

    // create_image_from_dmabuf
    pub fn create_image_from_dmabuf(
        &mut self,
        dmabuf: &Dmabuf,
        release_info: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        self.th_rend.create_image_from_dmabuf(dmabuf, release_info)
    }

    pub fn destroy_image(&mut self, image: Image) {
        self.th_rend.destroy_image(&image);
    }

    pub fn update_image_from_bits(
        &mut self,
        image: &mut Image,
        memimg: &MemImage,
        release_info: Option<Box<dyn Drop>>,
    ) {
        self.th_rend
            .update_image_from_bits(image, memimg, release_info)
    }

    // create_image_from_dmabuf
    pub fn update_image_from_dmabuf(
        &mut self,
        image: &mut Image,
        dmabuf: &Dmabuf,
        release_info: Option<Box<dyn Drop>>,
    ) {
        self.th_rend
            .update_image_from_dmabuf(image, dmabuf, release_info)
    }

    /// Creates a new surface.
    ///
    /// A surface represents a geometric region that will be
    /// drawn. It needs to have an image attached. The same
    /// image can be bound to multiple surfaces.
    pub fn create_surface(&mut self, x: f32, y: f32, width: f32, height: f32) -> Surface {
        Surface::create_surface(x, y, width, height)
    }

    /// Attaches an image to this surface, when this surface
    /// is drawn the contents will be sample from `image`
    pub fn bind_image(&self, surf: &mut Surface, image: Image) {
        surf.bind_image(image);
    }

    // release_pending_resources
    pub fn release_pending_resources(&mut self) {
        self.th_rend.release_pending_resources();
    }

    // draw_frame
    pub fn draw_frame(&mut self, surfaces: &SurfaceList) {
        // record rendering commands
        let params = self.th_rend.begin_recording_one_frame(surfaces);
        self.th_pipe.draw(&self.th_rend, &params, surfaces);
    }

    // present
    pub fn present(&mut self) {
        self.th_rend.present();
    }
}

impl Drop for Thundr {
    fn drop(&mut self) {
        // first destroy the pipeline specific resources
        self.th_pipe.destroy(&mut self.th_rend);
        // th_rend will now be dropped
    }
}
