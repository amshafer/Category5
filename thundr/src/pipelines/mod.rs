//!# Thundr Render Pipelines
//!
//!Thundr supports drawing surfaces in multiple ways which have different
//!performance characteristics.
//!
//!* `GeomPipeline` - renders surfaces using a traditional graphics
//!  pipeline. Surfaces are drawn as textured quads.
//!
//!The `Pipeline` trait outlines how the main Thundr instance interacts
//!with the pipeline code. All pipeline resources must be isolated from
//!Thundr, but Thundr resources may be modified by the pipeline implementation.
//!

// Austin Shafer - 2020
pub mod geometric;

pub use geometric::GeomPipeline;

use crate::display::{frame::RecordParams, DisplayState};
use crate::{Result, Surface, Viewport};

// The pipeline trait is essentially a mini-backend for the
// renderer. It determines what draw calls we generate for the
// frame.
///
/// This allows us to use one vkcomp instance with multiple drawing
/// types. For now there is one: the traditional rendering pipeline
/// (geometric).
pub(crate) trait Pipeline {
    fn begin_record(&mut self, dstate: &DisplayState);

    /// Set the viewport
    ///
    /// This restricts the draw operations to within the specified region
    fn set_viewport(&mut self, dstate: &DisplayState, viewport: &Viewport) -> Result<()>;

    /// Our function which records the cbufs used to draw
    /// a Surface.
    fn draw(
        &mut self,
        params: &mut RecordParams,
        dstate: &DisplayState,
        surfaces: &Surface,
    ) -> bool;

    fn end_record(&mut self, dstate: &DisplayState);

    /// Handle swapchain out of date
    ///
    /// This call tells the pipeline to recreate any resources that
    /// depend on the swapchain/screen size. i.e. VkFramebuffers
    fn handle_ood(&mut self, dstate: &DisplayState);
}
