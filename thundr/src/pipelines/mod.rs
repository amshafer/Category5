// Pipeline trait implementations.
//
// Austin Shafer - 2020

pub mod geometric;
pub mod compute;

use crate::renderer::{Renderer,RecordParams};
use crate::SurfaceList;

// The pipeline trait is essentially a mini-backend for the
// renderer. It determines what draw calls we generate for the
// frame.
///
/// This allows us to use one vkcomp instance with multiple drawing
/// types. For now there are two: the traditional rendering pipeline
/// (geometric), and a compute pipeline.
pub trait Pipeline {
    /// This returns true if the pipeline is ready to be used.
    /// False if it is still waiting on operations to complete before
    /// it is ready.
    fn is_ready(&self) -> bool;

    /// Our function which records the cbufs used to draw
    /// a frame. `params` tells us which cbufs/image we are
    /// recording for. We need to generate draw calls to update
    /// changes that have happened in `surfaces`.
    fn draw(&mut self,
            rend: &Renderer,
            params: &RecordParams,
            surfaces: &SurfaceList);

    fn destroy(&mut self, rend: &mut Renderer);
}
