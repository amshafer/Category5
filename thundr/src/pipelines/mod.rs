//!# Thundr Render Pipelines
//!
//!Thundr supports drawing surfaces in multiple ways which have different
//!performance characteristics.
//!
//!* `CompPipeline` - a compute pipeline that performs composition and
//!  blending in compute shaders.
//!* `GeomPipeline` - renders surfaces using a traditional graphics
//!  pipeline. Surfaces are drawn as textured quads.
//!
//!The compute pipeline sees the majority of development, and the
//!geometry pipeline is a fallback. The geometry pipeline may perform
//!better in certain situations, such as with software renderers.
//!
//!The `Pipeline` trait outlines how the main Thundr instance interacts
//!with the pipeline code. All pipeline resources must be isolated from
//!Thundr, but Thundr resources may be modified by the pipeline implementation.
//!

// Austin Shafer - 2020
use ash::{vk, Instance};

pub mod compute;
pub mod geometric;

pub use compute::CompPipeline;
pub use geometric::GeomPipeline;

use crate::display::Display;
use crate::renderer::{RecordParams, Renderer};
use crate::{Image, SurfaceList};

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
    fn draw(
        &mut self,
        rend: &mut Renderer,
        params: &RecordParams,
        images: &[Image],
        surfaces: &mut SurfaceList,
    );

    fn destroy(&mut self, rend: &mut Renderer);
}

pub enum PipelineType {
    COMPUTE,
    GEOMETRIC,
}

impl PipelineType {
    pub fn get_queue_family(
        &self,
        inst: &Instance,
        display: &Display,
        pdev: vk::PhysicalDevice,
    ) -> Option<u32> {
        match self {
            Self::COMPUTE => CompPipeline::get_queue_family(inst, display, pdev),
            Self::GEOMETRIC => None,
        }
    }
}
