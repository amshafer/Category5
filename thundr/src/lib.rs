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
#![allow(dyn_drop)]

extern crate lazy_static;
extern crate lluvia;
use lluvia as ll;

// Austin Shafer - 2020
use std::marker::PhantomData;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

mod damage;
mod descpool;
mod display;
mod image;
mod list;
mod pipelines;
mod platform;
mod renderer;
mod surface;

pub use self::image::Image;
pub use damage::Damage;
pub use list::SurfaceList;
pub use renderer::Renderer;
pub use surface::{SubsurfaceOrder, Surface};

use renderer::RecordParams;

// Re-export some things from utils so clients
// can use them
extern crate utils;
pub use crate::utils::region::Rect;
pub use crate::utils::{anyhow, Context, Dmabuf, MemImage};
use utils::log;

pub type Result<T> = std::result::Result<T, ThundrError>;

#[cfg(feature = "wayland")]
extern crate wayland_client as wc;

#[macro_use]
extern crate memoffset;
use pipelines::*;

extern crate thiserror;
use thiserror::Error;

/// Thundr error codes
/// These signify that action should be taken by the app.
#[derive(Error, Eq, PartialEq, Debug)]
#[allow(non_camel_case_types)]
pub enum ThundrError {
    #[error("Operation timed out")]
    TIMEOUT,
    #[error("Allocation failure")]
    OUT_OF_MEMORY,
    #[error("Operation is not ready, it needs to be redone")]
    NOT_READY,
    #[error("Failed to acquire the next swapchain image")]
    COULD_NOT_ACQUIRE_NEXT_IMAGE,
    #[error("vkQueuePresent failed")]
    PRESENT_FAILED,
    #[error("The internal Vulkan swapchain is out of date")]
    OUT_OF_DATE,
    #[error("Vulkan surface does not support R8G8B8A8_UNORM")]
    VK_SURF_NOT_SUPPORTED,
    #[error("Vulkan surface does not support the necessary (bindless) extensions")]
    VK_NOT_ALL_EXTENSIONS_AVAILABLE,
    #[error("Please select a composition type in the thundr CreateInfo")]
    COMPOSITION_TYPE_NOT_SPECIFIED,
    #[error("Vulkan surface or subsurface could not be found")]
    SURFACE_NOT_FOUND,
    #[error("Thundr Usage Bug: Recording already in progress")]
    RECORDING_ALREADY_IN_PROGRESS,
    #[error("Thundr Usage Bug: Recording has not been started")]
    RECORDING_NOT_IN_PROGRESS,
}

pub struct Thundr {
    /// Our core rendering resources
    ///
    /// This holds the majority of the vulkan objects, and allows them
    /// to be accessed by things in our ECS so they can tear down their
    /// vulkan allocations
    th_rend: Arc<Mutex<Renderer>>,
    /// This is the system used to track all Thundr resources
    th_ecs_inst: ll::Instance,

    /// Application specific stuff that will be set up after
    /// the original initialization
    pub(crate) _th_pipe_type: PipelineType,
    pub(crate) th_pipe: Box<dyn Pipeline>,

    /// The current draw calls parameters
    th_params: Option<RecordParams>,
}

/// A region to display to
///
/// The viewport will control what section of the screen is rendered
/// to. You will specify it when performing draw calls.
#[derive(Debug)]
pub struct Viewport {
    /// This is the position of the viewport on the output
    pub offset: (i32, i32),
    /// Size of the viewport within the output
    pub size: (i32, i32),
    /// The scrolling region of this viewport, basically the maximum bounds
    /// within which it is valid to update `scroll_offset`. This is similar to
    /// the panning region in X11.
    pub scroll_region: (i32, i32),
    /// This is the amount to offset everything within this viewport by. It
    /// can be used to move around all internal elements without updating
    /// them.
    ///
    /// This may be in the [0, scroll_region] range
    pub scroll_offset: (i32, i32),
}

impl Viewport {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            offset: (x, y),
            size: (width, height),
            scroll_region: (width, height),
            scroll_offset: (0, 0),
        }
    }

    /// Update the valid scrolling region within this viewport
    pub fn set_scroll_region(&mut self, x: i32, y: i32) {
        self.scroll_region = (x, y);
    }

    /// Set the scrolling within this viewport. This is a global transform
    ///
    /// This performs bounds checking of `x` and `y` to ensure the are within
    /// `scroll_region`. If they are not, then no scrolling is performed.
    pub fn set_scroll_amount(&mut self, x: i32, y: i32) {
        // The min and max bounds here are weird. Think of it like moving the
        // scroll region, not moving the scroll area. It looks like this:
        //
        // R: scroll region
        // A: scroll area
        //
        // Here they are at zero, content has just been loaded:
        //              0
        //              R--------------------R
        //              A-------------A
        //
        // Now here they are with the scroll all the way complete:
        //              0
        //       R--------------------R
        //              A-------------A
        //
        // The offset is actually from [-(R - A), 0]
        let min_x = -1 * (self.scroll_region.0 - self.size.0);
        let max_x = 0;
        // now get the new offset
        let x_offset = self.scroll_offset.0 + x;
        // clamp this offset within our bounds
        let x_clamped = x_offset.clamp(min_x, max_x);

        let min_y = -1 * (self.scroll_region.1 - self.size.1);
        let max_y = 0;
        let y_offset = self.scroll_offset.1 + y;
        let y_clamped = y_offset.clamp(min_y, max_y);

        self.scroll_offset = (x_clamped, y_clamped);
    }
}

#[cfg(feature = "sdl")]
extern crate sdl2;

pub enum SurfaceType<'a> {
    /// it exists to make the lifetime parameter play nice with rust.
    /// Since the Display variant doesn't have a lifetime, we need one that
    /// does incase xcb/macos aren't enabled.
    Display(PhantomData<&'a usize>),
    #[cfg(feature = "sdl")]
    SDL2(&'a sdl2::VideoSubsystem, &'a sdl2::video::Window),
    #[cfg(feature = "wayland")]
    Wayland(wc::Display, wc::protocol::wl_surface::WlSurface),
}

/// Parameters for Renderer creation.
///
/// These will be set by Thundr based on the Pipelines that will
/// be enabled. See `Pipeline` for methods that drive the data
/// contained here.
pub struct CreateInfo<'a> {
    /// Enable the compute pipeline. This is only suitable for small scenes
    /// and is not recommended for normal use.
    pub enable_compute_composition: bool,
    /// Enable the traditional quad rendering method. This is a bindless
    /// engine that draws on a set of quads to composite images. This
    /// is the default and recommended option
    pub enable_traditional_composition: bool,
    pub surface_type: SurfaceType<'a>,
}

impl<'a> CreateInfo<'a> {
    pub fn builder() -> CreateInfoBuilder<'a> {
        CreateInfoBuilder {
            ci: CreateInfo {
                enable_compute_composition: false,
                // This should always be used
                enable_traditional_composition: true,
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
    pub fn new(info: &CreateInfo) -> Result<Thundr> {
        let mut ecs = ll::Instance::new();

        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let mut rend = Renderer::new(&info, &mut ecs)?;

        // Create the pipeline(s) requested
        // Record the type we are using so that we know which type to regenerate
        // on window resizing
        let (pipe, ty): (Box<dyn Pipeline>, PipelineType) = if info.enable_traditional_composition {
            (
                Box::new(GeomPipeline::new(&mut rend)),
                PipelineType::GEOMETRIC,
            )
        } else if info.enable_compute_composition {
            (
                Box::new(CompPipeline::new(&mut rend)),
                PipelineType::COMPUTE,
            )
        } else {
            return Err(ThundrError::COMPOSITION_TYPE_NOT_SPECIFIED);
        };

        Ok(Thundr {
            th_rend: Arc::new(Mutex::new(rend)),
            th_ecs_inst: ecs,
            _th_pipe_type: ty,
            th_pipe: pipe,
            th_params: None,
        })
    }

    /// Get the Dots Per Inch for this display.
    ///
    /// For VK_KHR_display we will calculate it ourselves, and for
    /// SDL we will ask SDL to tell us it.
    pub fn get_dpi(&self) -> (f32, f32) {
        self.th_rend.lock().unwrap().display.get_dpi()
    }

    pub fn get_raw_vkdev_handle(&self) -> *const std::ffi::c_void {
        use ash::vk::Handle;
        self.th_rend.lock().unwrap().dev.handle().as_raw() as *const std::ffi::c_void
    }

    pub fn get_resolution(&self) -> (u32, u32) {
        let rend = self.th_rend.lock().unwrap();
        (rend.resolution.width, rend.resolution.height)
    }

    /// create_image_from_bits
    pub fn create_image_from_bits(
        &mut self,
        img: &MemImage,
        release_info: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        let rend_mtx = self.th_rend.clone();
        let mut rend = self.th_rend.lock().unwrap();
        let mut ret = rend.create_image_from_bits(rend_mtx, &img, release_info);
        if let Some(i) = ret.as_mut() {
            rend.push_image(i);
        }
        return ret;
    }

    /// create_image_from_dmabuf
    pub fn create_image_from_dmabuf(
        &mut self,
        dmabuf: &Dmabuf,
        release_info: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        let rend_mtx = self.th_rend.clone();
        let mut rend = self.th_rend.lock().unwrap();

        let mut ret = rend.create_image_from_dmabuf(rend_mtx, dmabuf, release_info);
        if let Some(i) = ret.as_mut() {
            rend.push_image(i)
        }
        return ret;
    }

    pub fn update_image_from_bits(
        &mut self,
        image: &mut Image,
        memimg: &MemImage,
        damage: Option<&Damage>,
        release_info: Option<Box<dyn Drop>>,
    ) {
        self.th_rend
            .lock()
            .unwrap()
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
            .lock()
            .unwrap()
            .update_image_from_dmabuf(image, dmabuf, release_info)
    }

    /// Creates a new surface.
    ///
    /// A surface represents a geometric region that will be
    /// drawn. It needs to have an image attached. The same
    /// image can be bound to multiple surfaces.
    pub fn create_surface(&mut self, x: f32, y: f32, width: f32, height: f32) -> Surface {
        let id = self.th_ecs_inst.add_entity();
        let surf = Surface::create_surface(id, x, y, width, height);
        let ecs_capacity = self.th_ecs_inst.num_entities();

        // Make space in our vulkan buffers
        self.th_rend
            .lock()
            .unwrap()
            .ensure_window_capacity(ecs_capacity);

        return surf;
    }

    /// Attaches an image to this surface, when this surface
    /// is drawn the contents will be sample from `image`
    pub fn bind_image(&self, surf: &mut Surface, image: Image) {
        surf.bind_image(image);
    }

    // release_pending_resources
    pub fn release_pending_resources(&mut self) {
        self.th_rend.lock().unwrap().release_pending_resources();
    }

    /// This is a candidate for an out of date error. We should
    /// let the application know about this so it can recalculate anything
    /// that depends on the window size, so we exit returning OOD.
    ///
    /// We have to destroy and recreate our pipeline along the way since
    /// it depends on the swapchain.
    pub fn handle_ood(&mut self) {
        let mut rend = self.th_rend.lock().unwrap();
        rend.wait_for_prev_submit();
        rend.wait_for_copy();
        unsafe {
            rend.recreate_swapchain();
        }
        self.th_pipe.handle_ood(rend.deref_mut());
    }

    pub fn get_drm_dev(&self) -> (i64, i64) {
        unsafe { self.th_rend.lock().unwrap().get_drm_dev() }
    }

    /// Flushes all surface updates to the GPU
    ///
    /// This should be called immediately for all surface lists right before beginning the
    /// draw sequence. This cannot happen during drawing since it will update the window
    /// and image lists and Vulkan may already have references too them.
    pub fn flush_surface_data(&mut self, surfaces: &mut SurfaceList) -> Result<()> {
        if self.th_params.is_some() {
            return Err(ThundrError::RECORDING_ALREADY_IN_PROGRESS);
        }

        let mut rend = self.th_rend.lock().unwrap();
        rend.add_damage_for_list(surfaces)?;

        // TODO: check and see if the image list has been changed, or if
        // any images have been updated.
        rend.refresh_window_resources(surfaces);

        // Now that we have processed this surfacelist, unmark it as changed
        surfaces.l_changed = false;

        Ok(())
    }

    /// Begin recording a frame
    ///
    /// This is first called when trying to draw a frame. It will set
    /// up the command buffers and resources that Thundr will use while
    /// recording draw commands.
    pub fn begin_recording(&mut self) -> Result<()> {
        if self.th_params.is_some() {
            return Err(ThundrError::RECORDING_ALREADY_IN_PROGRESS);
        }

        // record rendering commands
        let res = self.th_rend.lock().unwrap().begin_recording_one_frame();
        let params = match res {
            Ok(params) => params,
            Err(ThundrError::OUT_OF_DATE) => {
                self.handle_ood();
                return Err(ThundrError::OUT_OF_DATE);
            }
            Err(e) => return Err(e),
        };

        let mut rend = self.th_rend.lock().unwrap();
        self.th_pipe.begin_record(rend.deref_mut(), &params);
        self.th_params = Some(params);

        Ok(())
    }

    /// Draw a set of surfaces within a viewport
    ///
    /// This is the function for recording drawing of a set of surfaces. The surfaces
    /// in the list will be rendered withing the region specified by viewport.
    pub fn draw_surfaces(&mut self, surfaces: &SurfaceList, viewport: &Viewport) -> Result<()> {
        let params = self
            .th_params
            .as_mut()
            .ok_or(ThundrError::RECORDING_NOT_IN_PROGRESS)?;

        {
            let mut rend = self.th_rend.lock().unwrap();
            rend.draw_call_submitted =
                self.th_pipe
                    .draw(rend.deref_mut(), &params, surfaces, viewport);
        }

        // Update the amount of depth used while drawing this surface list. This
        // is the depth we should start subtracting from when drawing the next
        // viewport.
        // This magic 0.0000001 must match geom.vert.glsl
        params.starting_depth -= surfaces.l_window_order.len() as f32 * 0.0000001;

        self.draw_surfaces_debug_prints(surfaces, viewport);

        Ok(())
    }

    /// This finishes all recording operations and submits the work to the GPU.
    ///
    /// This should only be called after a proper begin_recording + draw_surfaces sequence.
    pub fn end_recording(&mut self) -> Result<()> {
        let params = self
            .th_params
            .as_ref()
            .ok_or(ThundrError::RECORDING_NOT_IN_PROGRESS)?;

        self.th_pipe
            .end_record(self.th_rend.lock().unwrap().deref_mut(), params);
        self.th_rend.lock().unwrap().clear_damage_on_all_images();
        self.th_params = None;

        Ok(())
    }

    /// Helper for removing all surfaces/objects currently loaded
    ///
    /// This will totally flush thundr, and reset it back to when it was
    /// created.
    pub fn clear_all(&mut self) {
        self.th_rend.lock().unwrap().clear_all();
    }

    // present
    pub fn present(&mut self) -> Result<()> {
        self.th_rend.lock().unwrap().present()
    }

    pub fn draw_surfaces_debug_prints(&mut self, _surfaces: &SurfaceList, _viewport: &Viewport) {
        // Debugging stats
        #[cfg(debug_assertions)]
        {
            log::debug!("Thundr rendering frame:");
            log::debug!(">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>");
            log::debug!("Surface List:");
            log::debug!("--------------------------------");
            for (i, s) in _surfaces.iter().enumerate() {
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
            let rend = self.th_rend.lock().unwrap();
            for (i, img) in rend.r_image_list.iter().enumerate() {
                log::debug!(
                    "[{}] Id={:?}, Size={:?}",
                    i,
                    img.i_internal.borrow().i_image,
                    img.get_resolution()
                );
            }

            if rend.dev_features.vkc_supports_incremental_present {
                log::debug!("Damaged vkPresentRegions:");
                log::debug!("--------------------------------");
                for (i, pr) in rend.current_damage.iter().enumerate() {
                    log::debug!("[{}] Base={:?}, Size={:?}", i, pr.offset, pr.extent);
                }
            }

            log::debug!("Window list:");
            for (i, val) in rend.r_windows.iter().enumerate() {
                if let Some(w) = val {
                    log::debug!(
                        "[{}] Image={}, Pos={:?}, Size={:?}, Opaque(Pos={:?}, Size={:?})",
                        i,
                        w.w_id.0,
                        w.w_dims.r_pos,
                        w.w_dims.r_size,
                        w.w_opaque.r_pos,
                        w.w_opaque.r_size
                    );
                }
            }
        }

        self.th_pipe.debug_frame_print();
        log::debug!("<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<<");
    }
}

impl Drop for Thundr {
    fn drop(&mut self) {
        // first destroy the pipeline specific resources
        let mut rend = self.th_rend.lock().unwrap();
        rend.wait_for_prev_submit();
        rend.wait_for_copy();
        self.th_pipe.destroy(rend.deref_mut());
        rend.clear_all();
        // th_rend will now be dropped
    }
}
