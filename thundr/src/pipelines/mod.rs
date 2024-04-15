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

use crate::display::DisplayState;
use crate::renderer::{RecordParams, Renderer};
use crate::{SurfaceList, Viewport};

// The pipeline trait is essentially a mini-backend for the
// renderer. It determines what draw calls we generate for the
// frame.
///
/// This allows us to use one vkcomp instance with multiple drawing
/// types. For now there is one: the traditional rendering pipeline
/// (geometric).
pub trait Pipeline {
    /// This returns true if the pipeline is ready to be used.
    /// False if it is still waiting on operations to complete before
    /// it is ready.
    fn is_ready(&self) -> bool;

    fn begin_record(&mut self, dstate: &DisplayState, rend: &Renderer, params: &RecordParams);

    /// Our function which records the cbufs used to draw
    /// a frame. `params` tells us which cbufs/image we are
    /// recording for. We need to generate draw calls to update
    /// changes that have happened in `surfaces`.
    ///
    /// pass_number represents the render pass id, this determines which surfaces
    /// out of the list get drawn.
    ///
    /// Returns if vkQueueSubmit was called, and if Renderer.render_sema
    /// should be waited on during presentation.
    fn draw(
        &mut self,
        rend: &mut Renderer,
        params: &RecordParams,
        surfaces: &SurfaceList,
        pass_number: usize,
        viewport: &Viewport,
    ) -> bool;

    fn end_record(&mut self, dstate: &DisplayState, rend: &mut Renderer, params: &RecordParams);

    /// This helper prints out any per-frame statistics for debug
    /// info, such as the window positions and the attached images.
    fn debug_frame_print(&self);

    /// Handle swapchain out of date
    ///
    /// This call tells the pipeline to recreate any resources that
    /// depend on the swapchain/screen size. i.e. VkFramebuffers
    fn handle_ood(&mut self, dstate: &DisplayState, rend: &mut Renderer);

    fn destroy(&mut self, rend: &mut Renderer);
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum PipelineType {
    GEOMETRIC,
}
