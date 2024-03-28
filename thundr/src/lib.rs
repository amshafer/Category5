//! # The Thundr rendering toolkit.
//!
//! Thundr is a Vulkan composition library for use in ui toolkits and
//! wayland compositors. You use it to create a set of images from
//! textures or window contents, attach those images to surfaces, and pass
//! a list of surfaces to thundr for rendering.
//!
//! Thundr also supports multiple methods of drawing:
//! * `geometric` - This is a more "traditional" manner of drawing ui elements:
//! surfaces are drawn as textured quads in 3D space.
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

extern crate lazy_static;
extern crate lluvia;
use lluvia as ll;

// Austin Shafer - 2020
use std::marker::PhantomData;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

mod damage;
mod descpool;
mod device;
mod display;
mod image;
mod instance;
mod list;
mod pipelines;
mod platform;
mod renderer;
mod surface;

pub use self::image::Image;
pub use self::image::{Dmabuf, DmabufPlane};
pub use damage::Damage;
pub use device::Device;
use display::Display;
use instance::Instance;
pub use list::SurfaceList;
pub use renderer::Renderer;
pub use surface::{SubsurfaceOrder, Surface};

use renderer::RecordParams;

// Re-export some things from utils so clients
// can use them
extern crate utils;
pub use crate::utils::region::Rect;
pub use crate::utils::{anyhow, Context, MemImage};
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
    #[error("Invalid Operation")]
    INVALID,
}

pub struct Thundr {
    /// The vulkan Instance
    th_inst: Arc<Instance>,
    /// Our primary device
    th_dev: Arc<Device>,
    /// Our core rendering resources
    ///
    /// This holds the majority of the vulkan objects, and allows them
    /// to be accessed by things in our ECS so they can tear down their
    /// vulkan allocations
    th_rend: Arc<Mutex<Renderer>>,
    /// vk_khr_display and vk_khr_surface wrapper.
    th_display: Display,
    /// This is the system used to track all Thundr resources
    th_ecs_inst: ll::Instance,

    /// The render pass a surface belongs to
    th_surface_pass: ll::Component<usize>,

    /// Application specific stuff that will be set up after
    /// the original initialization
    pub(crate) _th_pipe_type: PipelineType,
    pub(crate) th_pipe: Box<dyn Pipeline>,

    /// The current draw calls parameters
    th_params: Option<RecordParams>,

    /// We keep a list of all the images allocated by this context
    /// so that Pipeline::draw doesn't have to dedup the surfacelist's images
    pub th_image_ecs: ll::Instance,
    pub th_image_damage: ll::Component<Damage>,
}

/// A region to display to
///
/// The viewport will control what section of the screen is rendered
/// to. You will specify it when performing draw calls.
#[derive(Debug, Clone)]
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
    /// This performs bounds checking of `dx` and `dy` to ensure the are within
    /// `scroll_region`. If they are not, then no scrolling is performed.
    pub fn update_scroll_amount(&mut self, dx: i32, dy: i32) {
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
        let x_offset = self.scroll_offset.0 - dx;
        // clamp this offset within our bounds
        let x_clamped = x_offset.clamp(min_x, max_x);

        let min_y = -1 * (self.scroll_region.1 - self.size.1);
        let max_y = 0;
        let y_offset = self.scroll_offset.1 - dy;
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

/// Droppable trait that matches anything.
///
/// From <https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html#dyn-drop>
///
/// To work around passing dyn Drop we specify a trait that can accept anything. That
/// way this boxed object can be dropped when the last rendering resource references
/// it.
pub trait Droppable {}
impl<T> Droppable for T {}

// This is the public facing thundr api. Don't change it
impl Thundr {
    // TODO: make get_available_params and add customization
    pub fn new(info: &CreateInfo) -> Result<Thundr> {
        let mut ecs = ll::Instance::new();
        let pass_comp = ecs.add_component();
        // Create our own ECS for the image resources
        let mut img_ecs = ll::Instance::new();

        let inst = Arc::new(Instance::new(&info));
        let dev = Arc::new(Device::new(inst.clone(), &mut img_ecs, info)?);

        // creates a context, swapchain, images, and others
        // initialize the pipeline, renderpasses, and display engine
        let (mut rend, mut display) = Renderer::new(
            inst.clone(),
            dev.clone(),
            info,
            &mut ecs,
            img_ecs.clone(),
            pass_comp.clone(),
        )?;

        // Create the pipeline(s) requested
        // Record the type we are using so that we know which type to regenerate
        // on window resizing
        let (pipe, ty): (Box<dyn Pipeline>, PipelineType) = if info.enable_traditional_composition {
            (
                Box::new(GeomPipeline::new(&mut display, &mut rend)),
                PipelineType::GEOMETRIC,
            )
        } else {
            return Err(ThundrError::COMPOSITION_TYPE_NOT_SPECIFIED);
        };

        let img_damage_comp = img_ecs.add_component();

        Ok(Thundr {
            th_inst: inst,
            th_dev: dev,
            th_rend: Arc::new(Mutex::new(rend)),
            th_display: display,
            th_ecs_inst: ecs,
            th_surface_pass: pass_comp,
            _th_pipe_type: ty,
            th_pipe: pipe,
            th_params: None,
            th_image_ecs: img_ecs,
            th_image_damage: img_damage_comp,
        })
    }

    /// Get the Dots Per Inch for this display.
    ///
    /// For VK_KHR_display we will calculate it ourselves, and for
    /// SDL we will ask SDL to tell us it.
    pub fn get_dpi(&self) -> Result<(f32, f32)> {
        self.th_display.get_dpi()
    }

    pub fn get_resolution(&self) -> (u32, u32) {
        (
            self.th_display.d_resolution.width,
            self.th_display.d_resolution.height,
        )
    }

    /// Update an existing image from a shm buffer
    pub fn update_image_from_bits(
        &mut self,
        image: &Image,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        damage: Option<Damage>,
        release: Option<Box<dyn Droppable + Send + Sync>>,
    ) {
        self.th_dev
            .update_image_from_bits(image, data, width, height, stride, damage, release);

        self.update_image_vk_info(image.i_internal.read().as_ref().unwrap());
    }

    /// This gets damage in image-coords.
    ///
    /// This is used for getting the total amount of damage that the image should be
    /// updated by. It's a union of the unchanged image damage and the screen
    /// damage mapped on the image dimensions.
    pub fn get_image_damage(&mut self, surf: &mut Surface) -> Option<Damage> {
        surf.get_image_damage(&self.th_dev)
    }

    /// Creates a new surface.
    ///
    /// A surface represents a geometric region that will be
    /// drawn. It needs to have an image attached. The same
    /// image can be bound to multiple surfaces.
    pub fn create_surface(&mut self, x: f32, y: f32, width: f32, height: f32) -> Surface {
        let id = self.th_ecs_inst.add_entity();
        // Default new surfaces to the original render pass
        self.th_surface_pass.set(&id, 0);
        let surf = Surface::create_surface(id, x, y, width, height);
        let ecs_capacity = self.th_ecs_inst.num_entities();

        // Make space in our vulkan buffers
        self.th_rend
            .lock()
            .unwrap()
            .ensure_window_capacity(ecs_capacity);

        return surf;
    }

    /// Change the render pass this surface is a part of
    ///
    /// Render passes numbers are Thundr's way of segregating surfaces
    /// in the same draw lists into separate sub-drawlists while still respecting
    /// subsurface positioning. Render passes are drawn starting at zero and going
    /// up, so the user could mark a coupl subsurfaces as pass 1 to have them
    /// drawn after all other subsurfaces in the tree. This is useful for dakota
    /// drawing certain elements on top of viewports. Also useful for supporting
    /// partial drawing.
    pub fn surface_set_render_pass(&mut self, surf: &Surface, pass: usize) {
        self.th_surface_pass.set(&surf.get_ecs_id(), pass);
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

    /// Helper for removing all surfaces/objects currently loaded
    ///
    /// This will totally flush thundr, and reset it back to when it was
    /// created.
    pub fn clear_all(&mut self) {
        self.th_image_damage.clear();
    }

    /// This is a candidate for an out of date error. We should
    /// let the application know about this so it can recalculate anything
    /// that depends on the window size, so we exit returning OOD.
    ///
    /// We have to destroy and recreate our pipeline along the way since
    /// it depends on the swapchain.
    pub fn handle_ood(&mut self) {
        let mut rend = self.th_rend.lock().unwrap();
        unsafe {
            rend.recreate_swapchain(&mut self.th_display);
        }
        self.th_pipe
            .handle_ood(&mut self.th_display, rend.deref_mut());
    }

    pub fn get_drm_dev(&self) -> (i64, i64) {
        self.th_dev.get_drm_dev()
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
        self.th_pipe
            .begin_record(&mut self.th_display, rend.deref_mut(), &params);
        self.th_params = Some(params);

        Ok(())
    }

    /// Draw a set of surfaces within a viewport
    ///
    /// This is the function for recording drawing of a set of surfaces. The surfaces
    /// in the list will be rendered withing the region specified by viewport.
    pub fn draw_surfaces(
        &mut self,
        surfaces: &SurfaceList,
        viewport: &Viewport,
        pass: usize,
    ) -> Result<()> {
        let params = self
            .th_params
            .as_mut()
            .ok_or(ThundrError::RECORDING_NOT_IN_PROGRESS)?;
        if pass >= surfaces.l_pass.len() {
            log::error!(
                "Pass {} requested but SurfaceList only has {} passes",
                pass,
                surfaces.l_pass.len()
            );
            return Err(ThundrError::INVALID);
        }

        {
            let mut rend = self.th_rend.lock().unwrap();
            rend.draw_call_submitted =
                self.th_pipe
                    .draw(rend.deref_mut(), &params, surfaces, pass, viewport);
        }

        // Update the amount of depth used while drawing this surface list. This
        // is the depth we should start subtracting from when drawing the next
        // viewport.
        // This magic 0.0000001 must match geom.vert.glsl
        params.starting_depth +=
            surfaces.l_pass[pass].as_ref().unwrap().p_window_order.len() as f32 / 1000000000.0;

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
        // Clear damage from all Images
        self.th_image_damage.clear();
        self.th_params = None;

        Ok(())
    }

    // present
    pub fn present(&mut self) -> Result<()> {
        self.th_rend.lock().unwrap().present()
    }

    /// Helper for printing all of the subsurfaces under surf.
    #[cfg(debug_assertions)]
    fn print_surface(&self, surf: &Surface, i: usize, level: usize) {
        let img = surf.get_image();
        log::debug!(
            "{}[{}] Image={}, Pos={:?}, Size={:?}",
            std::iter::repeat('-').take(level).collect::<String>(),
            i,
            match img {
                Some(img) => img.get_id().get_raw_id() as i32,
                None => -1,
            },
            surf.get_pos(),
            surf.get_size()
        );

        for (i, sub) in surf
            .s_internal
            .read()
            .unwrap()
            .s_subsurfaces
            .iter()
            .enumerate()
        {
            self.print_surface(sub, i, level + 1);
        }
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
                self.print_surface(s, i, 0);
            }
            let rend = self.th_rend.lock().unwrap();

            if self.th_dev.dev_features.vkc_supports_incremental_present {
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
                        w.w_id,
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
        self.th_pipe.destroy(rend.deref_mut());
    }
}
