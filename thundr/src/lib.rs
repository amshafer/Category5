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
use std::marker::PhantomData;

mod damage;
mod descpool;
mod display;
mod image;
mod list;
mod pipelines;
mod platform;
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
use utils::log;

#[macro_use]
extern crate memoffset;
use pipelines::*;

pub struct Thundr {
    th_rend: Renderer,

    /// We keep a list of all the images allocated by this context
    /// so that Pipeline::draw doesn't have to dedup the surfacelist's images
    th_image_list: Vec<Image>,

    /// Application specific stuff that will be set up after
    /// the original initialization
    pub(crate) th_pipe: Box<dyn Pipeline>,
}

#[cfg(feature = "xcb")]
extern crate winit;

pub enum SurfaceType<'a> {
    /// it exists to make the lifetime parameter play nice with rust.
    /// Since the Display variant doesn't have a lifetime, we need one that
    /// does incase xcb/macos aren't enabled.
    Display(PhantomData<&'a usize>),
    #[cfg(feature = "xcb")]
    Xcb(&'a winit::window::Window),
    #[cfg(feature = "macos")]
    MacOS(&'a winit::window::Window),
}

/// Parameters for Renderer creation.
///
/// These will be set by Thundr based on the Pipelines that will
/// be enabled. See `Pipeline` for methods that drive the data
/// contained here.
pub struct CreateInfo<'a> {
    /// A list of queue family indexes to create the device with
    pub enable_compute_composition: bool,
    pub enable_traditional_composition: bool,
    pub surface_type: SurfaceType<'a>,
}

impl<'a> CreateInfo<'a> {
    pub fn builder() -> CreateInfoBuilder<'a> {
        CreateInfoBuilder {
            ci: CreateInfo {
                enable_compute_composition: true,
                enable_traditional_composition: false,
                surface_type: SurfaceType::Display(PhantomData),
            },
        }
    }
}

/// Implements the builder pattern for easier thundr creation
pub struct CreateInfoBuilder<'a> {
    ci: CreateInfo<'a>,
}
impl<'a> CreateInfoBuilder<'a> {
    pub fn enable_compute_composition(mut self) -> Self {
        self.ci.enable_compute_composition = true;
        self
    }

    pub fn enable_traditional_composition(mut self) -> Self {
        self.ci.enable_traditional_composition = true;
        self
    }
    pub fn surface_type(mut self, ty: SurfaceType<'a>) -> Self {
        self.ci.surface_type = ty;
        self
    }

    pub fn build(self) -> CreateInfo<'a> {
        self.ci
    }
}

// This is the public facing thundr api. Don't change it
impl Thundr {
    // TODO: make get_available_params and add customization
    pub fn new(info: &CreateInfo) -> Result<Thundr, &'static str> {
        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let mut rend = Renderer::new(&info);

        // Create the pipeline(s) requested
        let pipe: Box<dyn Pipeline> = if info.enable_traditional_composition {
            Box::new(GeomPipeline::new(&mut rend))
        } else if info.enable_compute_composition {
            Box::new(CompPipeline::new(&mut rend))
        } else {
            return Err("Please select a composition type in the thundr CreateInfo");
        };

        Ok(Thundr {
            th_rend: rend,
            th_image_list: Vec::new(),
            th_pipe: pipe,
        })
    }

    pub fn get_raw_vkdev_handle(&self) -> *const std::ffi::c_void {
        use ash::vk::Handle;
        self.th_rend.dev.handle().as_raw() as *const std::ffi::c_void
    }

    pub fn get_resolution(&self) -> (u32, u32) {
        (
            self.th_rend.resolution.width,
            self.th_rend.resolution.height,
        )
    }

    /// Helper for inserting a new image and updating its id
    fn push_image(&mut self, image: &mut Image) {
        self.th_image_list.push(image.clone());
        image.set_id((self.th_image_list.len() - 1) as i32);
    }

    /// Remove all attached damage.
    ///
    /// Damage is consumed by Thundr to ease the burden of developing
    /// apps with it. This internal func clears all the damage after
    /// a frame is drawn.
    fn clear_damage_on_all_images(&mut self) {
        for image in self.th_image_list.iter_mut() {
            image.clear_damage();
        }
    }

    /// Remove an image from the surfacelist.
    fn remove_image_at_index(&mut self, i: usize) {
        self.th_image_list.remove(i);

        // now that we have removed the image, we need to update all of the
        // ids, since some of them will have been shifted
        // TODO: OPTIMIZEME
        for (idx, i) in self.th_image_list.iter_mut().enumerate() {
            i.set_id(idx as i32);
        }
    }

    /// Helper for removing an image by handle.
    ///
    /// This may not be very performant. If you already know the index position,
    /// then use remove_image_at_index.
    fn remove_image(&mut self, image: &Image) {
        let i = match self.th_image_list.iter().position(|v| *v == *image) {
            Some(v) => v,
            // Error: This shouldn't happen, for some reason the image wasn't in
            // our image list
            None => return,
        };

        self.remove_image_at_index(i);
    }

    /// create_image_from_bits
    pub fn create_image_from_bits(
        &mut self,
        img: &MemImage,
        release_info: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        let mut ret = self.th_rend.create_image_from_bits(&img, release_info);
        if let Some(i) = ret.as_mut() {
            self.push_image(i);
        }
        return ret;
    }

    /// create_image_from_dmabuf
    pub fn create_image_from_dmabuf(
        &mut self,
        dmabuf: &Dmabuf,
        release_info: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        let mut ret = self.th_rend.create_image_from_dmabuf(dmabuf, release_info);
        if let Some(i) = ret.as_mut() {
            self.push_image(i)
        }
        return ret;
    }

    pub fn destroy_image(&mut self, image: Image) {
        self.th_rend.destroy_image(&image);
        self.remove_image(&image);
    }

    pub fn update_image_from_bits(
        &mut self,
        image: &mut Image,
        memimg: &MemImage,
        damage: Option<&Damage>,
        release_info: Option<Box<dyn Drop>>,
    ) {
        self.th_rend
            .update_image_from_bits(image, memimg, damage, release_info)
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
    pub fn draw_frame(&mut self, surfaces: &mut SurfaceList) {
        // record rendering commands
        let params = self.th_rend.begin_recording_one_frame(surfaces);
        self.th_rend.draw_call_submitted = self.th_pipe.draw(
            &mut self.th_rend,
            &params,
            self.th_image_list.as_slice(),
            surfaces,
        );
        // Now that we have processed this surfacelist, unmark it as changed
        surfaces.l_changed = false;
        self.clear_damage_on_all_images();

        // Debugging stats
        log::debug!("Thundr rendering frame:");
        log::debug!(">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>");
        log::debug!("Surface List:");
        log::debug!("--------------------------------");
        for (i, s) in surfaces.iter().enumerate() {
            let img = s.get_image();
            log::debug!(
                "[{}] Image={}, Pos={:?}, Size={:?}",
                i,
                match img {
                    Some(img) => img.get_id(),
                    None => -1,
                },
                s.get_pos(),
                s.get_size()
            );
        }
        log::debug!("Images List:");
        log::debug!("--------------------------------");
        for (i, img) in self.th_image_list.iter().enumerate() {
            log::debug!(
                "[{}] Id={}, Size={:?}",
                i,
                img.get_id(),
                img.get_resolution()
            );
        }
        log::debug!("Damaged vkPresentRegions:");
        log::debug!("--------------------------------");
        for (i, pr) in self.th_rend.current_damage.iter().enumerate() {
            log::debug!("[{}] Base={:?}, Size={:?}", i, pr.offset, pr.extent);
        }

        self.th_pipe.debug_frame_print();
        log::debug!("<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<");
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
