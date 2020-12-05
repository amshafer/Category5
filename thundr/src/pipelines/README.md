# Thundr Render Pipelines

Thundr supports drawing surfaces in multiple ways which have different
performance characteristics.

* `CompPipeline` - a compute pipeline that performs composition and
  blending in compute shaders.
* `GeomPipeline` - renders surfaces using a traditional graphics
  pipeline. Surfaces are drawn as textured quads.

The compute pipeline sees the majority of development, and the
geometry pipeline is a fallback. The geometry pipeline may perform
better in certain situations, such as with software renderers.

The `Pipeline` trait outlines how the main Thundr instance interacts
with the pipeline code. All pipeline resources must be isolated from
Thundr, but Thundr resources may be modified by the pipeline implementation.