// Rendering object for one frame
//
// ashafer - 2024

use crate::device::Device;
use crate::display::{DisplayState, Swapchain};
use crate::image::ImageVk;
use crate::pipelines::*;
use crate::*;

/// Shader push constants
///
/// These will be updated when we record the per-viewport draw commands
/// and will contain the scrolling model transformation of all content
/// within a viewport.
///
/// This is also where we pass in the Surface's data.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PushConstants {
    pub width: u32,
    pub height: u32,
    /// The id of the image. This is the offset into the unbounded sampler array.
    /// id that's the offset into the unbound sampler array
    pub image_id: i32,
    /// if we should use color instead of texturing
    pub use_color: i32,
    /// Opaque color
    pub color: (f32, f32, f32, f32),
    /// The complete dimensions of the window.
    pub dims: Rect<i32>,
}

/// Recording parameters
///
/// Layers above this one will need to call recording
/// operations. They need a private structure to pass
/// to begin/end recording operations
/// This is that structure.
pub(crate) struct RecordParams<'a> {
    /// our cached pushbuffer constants
    pub push: PushConstants,
    /// From our Display's Device
    pub image_vk: ll::Snapshot<'a, Arc<ImageVk>>,
}

impl<'a> RecordParams<'a> {
    pub fn new(dev: &'a Device) -> Self {
        Self {
            image_vk: dev.d_image_vk.snapshot(),
            push: PushConstants {
                width: 0,
                height: 0,
                image_id: -1,
                use_color: -1,
                color: (0.0, 0.0, 0.0, 0.0),
                dims: Rect::new(0, 0, 0, 0),
            },
        }
    }
}

/// Renderer for a single frame
///
/// This object controls a current batch of drawing commands which will
/// be presented. This holds a read lock for the thundr resources for
/// the duration of its lifetime to avoid overhead.
///
/// Rendering is done with the series of drawing commands. These create
/// a Vulkan command buffer which is submitted for presentation in the
/// present() command. This object should be freed before waiting for
/// the next frame.
pub struct FrameRenderer<'a> {
    pub(crate) fr_swapchain: &'a mut Box<dyn Swapchain>,
    pub(crate) fr_dstate: &'a DisplayState,
    pub(crate) fr_pipe: &'a mut Box<dyn Pipeline>,
    /// The current draw calls parameters
    pub(crate) fr_params: RecordParams<'a>,
}

impl<'a> FrameRenderer<'a> {
    /// Set the viewport
    ///
    /// This restricts the draw operations to within the specified region
    pub fn set_viewport(&mut self, viewport: &Viewport) -> Result<()> {
        self.fr_pipe.set_viewport(&self.fr_dstate, viewport)
    }

    /// Draw a set of surfaces within a viewport
    ///
    /// This is the function for recording drawing of a set of surfaces. The surfaces
    /// in the list will be rendered withing the region specified by viewport.
    pub fn draw_surface(&mut self, surface: &Surface) -> Result<()> {
        self.fr_pipe
            .draw(&mut self.fr_params, &self.fr_dstate, surface);

        Ok(())
    }

    /// Present the current swapchain image to the screen.
    ///
    /// Finally we can actually flip the buffers and present
    /// this image.
    ///
    /// Once this has been called this object can no longer be used
    pub fn present(&mut self) -> Result<()> {
        self.fr_pipe.end_record(&self.fr_dstate);
        self.fr_swapchain.present(&self.fr_dstate)
    }
}
