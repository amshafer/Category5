// Pipeline trait implementations.
//
// Austin Shafer - 2020

pub mod geometric;

// The pipeline trait is essentially a mini-backend for the
// renderer. It determines what draw calls we generate for the
// frame.
///
/// This allows us to use one vkcomp instance with multiple drawing
/// types. For now there are two: the traditional rendering pipeline
/// (geometric), and a compute pipeline.
pub trait Pipeline {
    /// Create a new pipeline backend for this renderer.
    /// This is where we should create any descriptor sets, cbufs,
    /// or targets needed.
    pub fn new(rend: &mut Renderer) -> Self;

    /// This sets up any pre-recording resources.
    /// For example, starting render passes.
    pub fn begin_recording_one_frame(&mut self,
                                     params: &RecordParams);

    /// Our function which records the cbufs used to draw
    /// a frame. `params` tells us which cbufs/image we are
    /// recording for. We need to generate draw calls to update
    /// changes that have happened in `surfaces`.
    pub fn draw(&mut self,
                rend: &Renderer,
                params: &RecordParams,
                surfaces: &SurfaceList);

    pub fn destroy(&mut self, rend: &mut Renderer);
}
