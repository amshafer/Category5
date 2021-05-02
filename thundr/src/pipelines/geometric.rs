// This is the simplest and most traditional rendering backend
// It draws windows as textured quads
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]
use serde::{Deserialize, Serialize};

use cgmath::{Matrix4, Vector2, Vector3};

use std::ffi::CString;
use std::io::Cursor;
use std::marker::Copy;
use std::mem;

use ash::version::DeviceV1_0;
use ash::{util, vk};

use super::Pipeline;
use crate::renderer::{RecordParams, Renderer};
use crate::{Image, Surface, SurfaceList};

use utils::log;

// This is the reference data for a normal quad
// that will be used to draw client windows.
static QUAD_DATA: [VertData; 4] = [
    VertData {
        vertex: Vector2::new(0.0, 0.0),
        tex: Vector2::new(0.0, 0.0),
    },
    VertData {
        vertex: Vector2::new(1.0, 0.0),
        tex: Vector2::new(1.0, 0.0),
    },
    VertData {
        vertex: Vector2::new(0.0, 1.0),
        tex: Vector2::new(0.0, 1.0),
    },
    VertData {
        vertex: Vector2::new(1.0, 1.0),
        tex: Vector2::new(1.0, 1.0),
    },
];

static QUAD_INDICES: [Vector3<u32>; 2] = [Vector3::new(1, 2, 3), Vector3::new(1, 4, 2)];

/// an application specific set of resources to draw.
///
/// These are the "dynamic" parts of our application. The things
/// that change depending on the scene. It holds pipelines, layouts
/// shaders, and geometry.
///
/// Ideally the `Renderer` can render/present anything, and this
/// struct specifies what to draw. This allows the second half
/// of the initialization functions to just have a self ref.
///
/// images are created with Renderer::create_image. The renderer is in
/// charge of creating/destroying the images since all of the image
/// resources are created from the Renderer.
pub struct GeomPipeline {
    pass: vk::RenderPass,
    pipeline: vk::Pipeline,
    pub(crate) pipeline_layout: vk::PipelineLayout,
    /// This descriptor pool allocates only the 1 ubo
    uniform_pool: vk::DescriptorPool,
    /// (as per `create_descriptor_layouts`)
    /// This will only be the sets holding the uniform buffers,
    /// any image specific descriptors are in the image's sets.
    descriptor_uniform_layout: vk::DescriptorSetLayout,
    pub(crate) ubo_descriptor: vk::DescriptorSet,
    shader_modules: Vec<vk::ShaderModule>,
    framebuffers: Vec<vk::Framebuffer>,
    /// shader constants are shared by all swapchain images
    uniform_buffer: vk::Buffer,
    uniform_buffers_memory: vk::DeviceMemory,
    /// We will hold only one copy of the static QUAD_DATA
    /// which represents an onscreen window.
    vert_buffer: vk::Buffer,
    vert_buffer_memory: vk::DeviceMemory,
    pub(crate) vert_count: u32,
    /// Resources for the index buffer
    index_buffer: vk::Buffer,
    index_buffer_memory: vk::DeviceMemory,

    /// an image for recording depth test data
    pub(crate) depth_image: vk::Image,
    pub(crate) depth_image_view: vk::ImageView,
    /// because we create the image, we need to back it with memory
    pub(crate) depth_image_mem: vk::DeviceMemory,
}

/// Contiains a vertex and all its related data
///
/// Things like vertex normals and colors will be passed in
/// the same vertex input assembly, so this type provides
/// a wrapper for handling all of them at once.
#[repr(C)]
#[derive(Clone, Copy)]
struct VertData {
    pub vertex: Vector2<f32>,
    pub tex: Vector2<f32>,
}

/// Shader constants are used for
/// the larger uniform values which are
/// not changed very often.
#[derive(Clone, Copy)]
#[repr(C)]
struct ShaderConstants {
    pub model: Matrix4<f32>,
    pub width: f32,
    pub height: f32,
}

/// Push constants are used for small bits of data
/// which are changed often. We will use them to
/// transform the default square into the size of
/// the client window.
///
/// This should to be less than 128 bytes to guarantee
/// that there will be enough push constant space.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[repr(C)]
pub struct PushConstants {
    /// the z-ordering of the window being drawn
    pub order: f32,
    /// this is [0,resolution]
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Pipeline for GeomPipeline {
    fn is_ready(&self) -> bool {
        true
    }

    /// Our implementation of drawing one frame using geometry
    fn draw(
        &mut self,
        rend: &mut Renderer,
        params: &RecordParams,
        _images: &[Image],
        surfaces: &mut SurfaceList,
    ) -> bool {
        self.begin_recording(rend, params);
        let mut index = 0;

        for surf in surfaces.iter().rev() {
            // TODO: make a limit to the number of windows
            // we have to call rev before enumerate, so we need
            // to correct this by setting the depth of the earliest
            // surfaces to the deepest
            let depth = 1.0 - 0.000001 * index as f32;
            self.record_surface_draw(rend, params, surf, &(0.0, 0.0), depth);
            index += 1;

            // Now do the subsurfaces for this surf, in reverse order too
            let s = surf.s_internal.borrow();
            for sub in s.s_subsurfaces.iter().rev() {
                let depth = 1.0 - 0.000001 * index as f32;
                assert!(
                    sub.s_internal.borrow().s_subsurfaces.len() == 0,
                    "ERROR: recursive subsurfaces not supported"
                );
                self.record_surface_draw(rend, params, sub, &surf.get_pos(), depth);

                index += 1;
            }
        }

        // make sure to end recording
        unsafe {
            rend.dev.cmd_end_render_pass(params.cbuf);
            rend.cbuf_end_recording(params.cbuf);
        }

        // now start rendering
        self.begin_frame(rend);
        return true;
    }

    fn debug_frame_print(&self) {
        log::debug!("Geometric Pipeline Debug Statistics:");
        log::debug!("---------------------------------");
        log::debug!("---------------------------------");
    }

    fn destroy(&mut self, rend: &mut Renderer) {
        unsafe {
            rend.free_memory(self.vert_buffer_memory);
            rend.free_memory(self.index_buffer_memory);
            rend.dev.destroy_buffer(self.vert_buffer, None);
            rend.dev.destroy_buffer(self.index_buffer, None);

            rend.free_memory(self.depth_image_mem);
            rend.dev.destroy_image_view(self.depth_image_view, None);
            rend.dev.destroy_image(self.depth_image, None);

            rend.dev.destroy_buffer(self.uniform_buffer, None);
            rend.free_memory(self.uniform_buffers_memory);

            rend.dev.destroy_render_pass(self.pass, None);

            rend.dev
                .destroy_descriptor_set_layout(self.descriptor_uniform_layout, None);

            rend.dev.destroy_descriptor_pool(self.uniform_pool, None);

            rend.dev.destroy_pipeline_layout(self.pipeline_layout, None);

            for m in self.shader_modules.iter() {
                rend.dev.destroy_shader_module(*m, None);
            }

            for f in self.framebuffers.iter() {
                rend.dev.destroy_framebuffer(*f, None);
            }

            rend.dev.destroy_pipeline(self.pipeline, None);
        }
    }
}

impl GeomPipeline {
    /// Set up the application. This should *always* be called
    ///
    /// Once we have allocated a renderer with `new`, we should initialize
    /// the rendering pipeline so that we can display things. This method
    /// basically sets up all of the "application" specific resources like
    /// shaders, geometry, and the like.
    ///
    /// This fills in the GeomPipeline struct in the Renderer
    pub fn new(rend: &mut Renderer) -> GeomPipeline {
        unsafe {
            let pass = GeomPipeline::create_pass(rend);

            // This is a really annoying issue with CString ptrs
            let program_entrypoint_name = CString::new("main").unwrap();
            // If the CString is created in `create_shaders`, and is inserted in
            // the return struct using the `.as_ptr()` method, then the CString
            // will still be dropped on return and our pointer will be garbage.
            // Instead we need to ensure that the CString will live long
            // enough. I have no idea why it is like this.
            let shader_stages = Box::new(GeomPipeline::create_shader_stages(
                rend,
                program_entrypoint_name.as_ptr(),
            ));

            // prepare descriptors for all of the uniforms to pass to shaders
            //
            // NOTE: These need to be referenced in order by the `set` modifier
            // in the shaders
            let ubo_layout = GeomPipeline::create_ubo_layout(rend);
            // These are the layout recognized by the pipeline
            let descriptor_layouts = &[
                ubo_layout,            // set 0
                rend.desc_pool.layout, // set 1
            ];

            // make a push constant entry for the z ordering of a window
            let constants = &[vk::PushConstantRange::builder()
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
                // depth is measured as a normalized float
                .size(std::mem::size_of::<PushConstants>() as u32)
                .build()];

            // even though we don't have anything special in our layout, we
            // still need to have a created layout for the pipeline
            let layout_info = vk::PipelineLayoutCreateInfo::builder()
                .push_constant_ranges(constants)
                .set_layouts(descriptor_layouts);
            let layout = rend.dev.create_pipeline_layout(&layout_info, None).unwrap();

            let pipeline = GeomPipeline::create_pipeline(rend, layout, pass, &*shader_stages);

            // the depth attachment needs to have its own resources
            let (depth_image, depth_image_view, depth_image_mem) = Renderer::create_image(
                &rend.dev,
                &rend.mem_props,
                &rend.resolution,
                vk::Format::D16_UNORM,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::ImageTiling::OPTIMAL,
            );

            let framebuffers =
                GeomPipeline::create_framebuffers(rend, pass, rend.resolution, depth_image_view);

            // Allocate a pool only for the ubo descriptors
            let uniform_pool = rend.create_descriptor_pool();
            let ubo = rend.allocate_descriptor_sets(uniform_pool, &[ubo_layout])[0];

            let consts = GeomPipeline::get_shader_constants(rend.resolution);

            // create a uniform buffer
            let (buf, mem) = rend.create_buffer(
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                // this specifies the constants to copy into the buffer
                &[consts],
            );

            // Allocate buffers for all geometry to be used
            let (vbuf, vmem, ibuf, imem) = GeomPipeline::create_default_geom_bufs(rend);

            // The app context contains the scene specific data
            let mut ctx = GeomPipeline {
                pass: pass,
                pipeline: pipeline,
                pipeline_layout: layout,
                descriptor_uniform_layout: ubo_layout,
                framebuffers: framebuffers,
                uniform_buffer: buf,
                uniform_buffers_memory: mem,
                uniform_pool: uniform_pool,
                ubo_descriptor: ubo,
                shader_modules: shader_stages.iter().map(|info| info.module).collect(),
                vert_buffer: vbuf,
                vert_buffer_memory: vmem,
                // multiply the index len by the vector size
                vert_count: QUAD_INDICES.len() as u32 * 3,
                index_buffer: ibuf,
                index_buffer_memory: imem,
                depth_image: depth_image,
                depth_image_view: depth_image_view,
                depth_image_mem: depth_image_mem,
            };

            // now we need to update the descriptor set with the
            // buffer of the uniform constants to use
            ctx.update_uniform_descriptor_set(
                rend, buf, ubo, 0, // binding
                0, // element
            );
            ctx.setup_depth_image(rend);

            return ctx;
        }
    }

    /// Start recording a cbuf for one frame
    ///
    /// Each framebuffer has a set of resources, including command
    /// buffers. This records the cbufs for the framebuffer
    /// specified by `img`.
    fn begin_recording(&mut self, rend: &Renderer, params: &RecordParams) {
        unsafe {
            // we need to clear any existing data when we start a pass
            let clear_vals = [
                vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.0, 0.0, 0.0, 0.0],
                    },
                },
                vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            ];

            // We want to start a render pass to hold all of
            // our drawing. The actual pass is started in the cbuf
            let pass_begin_info = vk::RenderPassBeginInfo::builder()
                .render_pass(self.pass)
                .framebuffer(self.framebuffers[params.image_num])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: rend.resolution,
                })
                .clear_values(&clear_vals);

            // start the cbuf
            rend.cbuf_begin_recording(params.cbuf, vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);

            // -- Setup static drawing resources
            // All of our drawing operations need
            // to be recorded inside a render pass.
            rend.dev.cmd_begin_render_pass(
                params.cbuf,
                &pass_begin_info,
                vk::SubpassContents::INLINE,
            );

            rend.dev
                .cmd_bind_pipeline(params.cbuf, vk::PipelineBindPoint::GRAPHICS, self.pipeline);

            // bind the vertex and index buffers from
            // the first image
            rend.dev.cmd_bind_vertex_buffers(
                params.cbuf,         // cbuf to draw in
                0,                   // first vertex binding updated by the command
                &[self.vert_buffer], // set of buffers to bind
                &[0],                // offsets for the above buffers
            );
            rend.dev.cmd_bind_index_buffer(
                params.cbuf,
                self.index_buffer,
                0, // offset
                vk::IndexType::UINT32,
            );
        }
    }

    /// Render a frame, but do not present it
    ///
    /// Think of this as the "main" rendering operation. It will draw
    /// all geometry to the current framebuffer. Presentation is
    /// done later, in case operations need to occur inbetween.
    fn begin_frame(&mut self, rend: &Renderer) {
        // Submit the recorded cbuf to perform the draw calls
        rend.cbuf_submit(
            // submit the cbuf for the current image
            rend.cbufs[rend.current_image as usize],
            rend.present_queue, // the graphics queue
            // wait_stages
            &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
            &[rend.present_sema], // wait_semas
            &[rend.render_sema],  // signal_semas
        );
    }

    /// create a renderpass for the color/depth attachments
    ///
    /// Render passses signify what attachments are used in which
    /// stages. They are composed of one or more subpasses.
    unsafe fn create_pass(rend: &Renderer) -> vk::RenderPass {
        let attachments = [
            // the color dest. Its the surface we slected in Renderer::new.
            // see Renderer::create_swapchain for why we aren't using
            // the native surface formate
            vk::AttachmentDescription {
                format: vk::Format::R8G8B8A8_UNORM,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                store_op: vk::AttachmentStoreOp::STORE,
                final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                ..Default::default()
            },
            // the depth attachment
            vk::AttachmentDescription {
                format: vk::Format::D16_UNORM,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                initial_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                ..Default::default()
            },
        ];

        // identify which of the above attachments
        let color_refs = [vk::AttachmentReference {
            attachment: 0, // index into the attachments variable
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let depth_refs = vk::AttachmentReference {
            attachment: 1,
            layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };

        // our subpass isn't dependent on anything, and it writes to color output
        let dependencies = [vk::SubpassDependency {
            src_subpass: vk::SUBPASS_EXTERNAL,
            src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_READ
                | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ..Default::default()
        }];

        // our render pass only has one subpass, which only does graphical ops
        let subpasses = [vk::SubpassDescription::builder()
            .color_attachments(&color_refs)
            .depth_stencil_attachment(&depth_refs)
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .build()];

        let create_info = vk::RenderPassCreateInfo::builder()
            .attachments(&attachments)
            .subpasses(&subpasses)
            .dependencies(&dependencies);

        rend.dev.create_render_pass(&create_info, None).unwrap()
    }

    /// Create a vkShaderModule for one of the dynamic pipeline stages
    ///
    /// dynamic portions of the graphics pipeline are programmable with
    /// spirv code. This helper function accepts a file name (`cursor`) and
    /// creates a shader module from it.
    ///
    /// `cursor` is accepted by ash's helper function, `read_spv`
    unsafe fn create_shader_module(
        rend: &Renderer,
        cursor: &mut Cursor<&'static [u8]>,
    ) -> vk::ShaderModule {
        let code = util::read_spv(cursor).expect("Could not read spv file");

        let info = vk::ShaderModuleCreateInfo::builder().code(&code);

        rend.dev
            .create_shader_module(&info, None)
            .expect("Could not create new shader module")
    }

    /// Create the dynamic portions of the rendering pipeline
    ///
    /// Shader stages specify the usage of a shader module, such as the
    /// entrypoint name (usually main) and the type of shader. As of now,
    /// we only return two shader modules, vertex and fragment.
    ///
    /// `entrypoint`: should be a CString.as_ptr(). The CString that it
    /// represents should live as long as the return type of this method.
    ///  see: https://doc.rust-lang.org/std/ffi/struct.CString.html#method.as_ptr
    unsafe fn create_shader_stages(
        rend: &Renderer,
        entrypoint: *const i8,
    ) -> [vk::PipelineShaderStageCreateInfo; 2] {
        let vert_shader = GeomPipeline::create_shader_module(
            rend,
            &mut Cursor::new(&include_bytes!("./shaders/vert.spv")[..]),
        );
        let frag_shader = GeomPipeline::create_shader_module(
            rend,
            &mut Cursor::new(&include_bytes!("./shaders/frag.spv")[..]),
        );

        // note that the return size is 2 elements to match the return type
        [
            vk::PipelineShaderStageCreateInfo {
                module: vert_shader,
                p_name: entrypoint,
                stage: vk::ShaderStageFlags::VERTEX,
                ..Default::default()
            },
            vk::PipelineShaderStageCreateInfo {
                module: frag_shader,
                p_name: entrypoint,
                stage: vk::ShaderStageFlags::FRAGMENT,
                ..Default::default()
            },
        ]
    }

    /// Configure and create a graphics pipeline
    ///
    /// In vulkan, the programmer has explicit control over the format
    /// and layout of the entire graphical pipeline, both dynamic and
    /// fixed function portions. We will specify the vertex input, primitive
    /// assembly, viewport/stencil location, rasterization type, depth
    /// information, and color blending.
    ///
    /// Pipeline layouts specify the full set of resources that the pipeline
    /// can access while running.
    ///
    /// This method roughly follows the "fixed function" part of the
    /// vulkan tutorial.
    unsafe fn create_pipeline(
        rend: &Renderer,
        layout: vk::PipelineLayout,
        pass: vk::RenderPass,
        shader_stages: &[vk::PipelineShaderStageCreateInfo],
    ) -> vk::Pipeline {
        // This binds our vertex input to location 0 to be passed to the shader
        // Think of it like specifying the data stream given to the shader
        let vertex_bindings = [vk::VertexInputBindingDescription {
            binding: 0, // (location = 0)
            stride: mem::size_of::<VertData>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }];

        // These describe how the shader should parse the data passed
        // think of it like breaking the above data stream into variables
        let vertex_attributes = [
            // vertex location
            vk::VertexInputAttributeDescription {
                binding: 0,  // The data binding to parse
                location: 0, // the location of the attribute we are specifying
                // Common types
                //     float: VK_FORMAT_R32_SFLOAT
                //     vec2:  VK_FORMAT_R32G32_SFLOAT
                //     vec3:  VK_FORMAT_R32G32B32_SFLOAT
                //     vec4:  VK_FORMAT_R32G32B32A32_SFLOAT
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(VertData, vertex) as u32,
            },
            // Texture coordinates
            vk::VertexInputAttributeDescription {
                binding: 0,  // The data binding to parse
                location: 1, // the location of the attribute we are specifying
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(VertData, tex) as u32,
            },
        ];

        // now for the fixed function portions of the pipeline
        // This describes the layout of resources passed to the shaders
        let vertex_info = vk::PipelineVertexInputStateCreateInfo::builder()
            .vertex_binding_descriptions(&vertex_bindings)
            .vertex_attribute_descriptions(&vertex_attributes);

        // input assembly describes how to turn the vertex
        // and index buffers into primatives
        let assembly = vk::PipelineInputAssemblyStateCreateInfo {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            ..Default::default()
        };

        // will almost always be (0,0) with size (width, height)
        let viewport = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: rend.resolution.width as f32,
            height: rend.resolution.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        // no scissor test
        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: rend.resolution,
        }];

        let viewport_info = vk::PipelineViewportStateCreateInfo::builder()
            .scissors(&scissor)
            .viewports(&viewport);

        // we want the normal counter-clockwise vertices, and filled in polys
        let raster_info = vk::PipelineRasterizationStateCreateInfo {
            front_face: vk::FrontFace::CLOCKWISE,
            line_width: 1.0,
            polygon_mode: vk::PolygonMode::FILL,
            ..Default::default()
        };

        // combines all of the fragments found at a pixel for anti-aliasing
        // just disable this
        let multisample_info = vk::PipelineMultisampleStateCreateInfo {
            rasterization_samples: vk::SampleCountFlags::TYPE_1,
            ..Default::default()
        };

        // no stencil operations, so this just keeps everything
        let stencil_state = vk::StencilOpState {
            fail_op: vk::StencilOp::KEEP,
            pass_op: vk::StencilOp::KEEP,
            depth_fail_op: vk::StencilOp::KEEP,
            compare_op: vk::CompareOp::ALWAYS,
            ..Default::default()
        };

        // we do want a depth test enabled for this, using our noop stencil
        // test. This should record Z-order to 1,000
        let depth_info = vk::PipelineDepthStencilStateCreateInfo {
            depth_test_enable: 1,
            depth_write_enable: 1,
            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
            front: stencil_state,
            back: stencil_state,
            max_depth_bounds: 1.0,
            ..Default::default()
        };

        // just do basic alpha blending. This is straight from the tutorial
        let blend_attachment_states = [vk::PipelineColorBlendAttachmentState {
            blend_enable: 1, // VK_TRUE
            // blend the new contents over the old
            src_color_blend_factor: vk::BlendFactor::SRC_ALPHA,
            dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            color_blend_op: vk::BlendOp::ADD,
            src_alpha_blend_factor: vk::BlendFactor::ONE,
            dst_alpha_blend_factor: vk::BlendFactor::ZERO,
            alpha_blend_op: vk::BlendOp::ADD,
            color_write_mask: vk::ColorComponentFlags::all(),
        }];

        let blend_info = vk::PipelineColorBlendStateCreateInfo::builder()
            .logic_op(vk::LogicOp::CLEAR)
            .attachments(&blend_attachment_states);

        // dynamic state specifies what parts of the pipeline will be
        // specified at draw time. (like moving the viewport)
        // we don't want any of that atm

        let pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_info)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport_info)
            .rasterization_state(&raster_info)
            .multisample_state(&multisample_info)
            .depth_stencil_state(&depth_info)
            .color_blend_state(&blend_info)
            .layout(layout)
            .render_pass(pass)
            .build();

        // Allocate one pipeline and return it
        rend.dev
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .expect("Could not create graphics pipeline")[0]
    }

    /// Create framebuffers for each swapchain image
    ///
    /// Image views represent a portion of an allocated image, while
    /// framebuffers bind an image view for use in a render pass. A
    /// framebuffer is really just a collection of attachments.
    ///
    /// In our example, we pair color and depth attachments in our
    /// framebuffers.
    unsafe fn create_framebuffers(
        rend: &mut Renderer,
        pass: vk::RenderPass,
        res: vk::Extent2D,
        depth_image_view: vk::ImageView,
    ) -> Vec<vk::Framebuffer> {
        // A framebuffer should be created for each of the swapchain
        // images. Reuse the depth buffer for all images since it
        // doesn't change.
        rend.views
            .iter()
            .map(|&view| {
                // color, depth
                let attachments = [view, depth_image_view];

                let info = vk::FramebufferCreateInfo::builder()
                    .render_pass(pass)
                    .attachments(&attachments)
                    .width(res.width)
                    .height(res.height)
                    .layers(1);

                rend.dev.create_framebuffer(&info, None).unwrap()
            })
            .collect()
    }

    /// Returns a `ShaderConstants` with the default values for this application
    ///
    /// Constants will be the contents of the uniform buffers which are
    /// processed by the shaders. The most obvious entry is the model + view
    /// + perspective projection matrix.
    fn get_shader_constants(resolution: vk::Extent2D) -> ShaderConstants {
        // transform from blender's coordinate system to vulkan
        let model = Matrix4::from_translation(Vector3::new(-1.0, -1.0, 0.0));

        ShaderConstants {
            model: model,
            width: resolution.width as f32,
            height: resolution.height as f32,
        }
    }

    /// Create uniform buffer descriptor layout
    ///
    /// Descriptor layouts specify the number and characteristics of descriptor
    /// sets which will be made available to the pipeline through the pipeline
    /// layout.
    ///
    /// The layouts created will be the default for this application. This should
    /// usually be at least one descriptor for the MVP martrix.
    unsafe fn create_ubo_layout(rend: &Renderer) -> vk::DescriptorSetLayout {
        // supplies `descriptor_uniform_layouts`
        // ubos for the MVP matrix and image samplers for textures
        let bindings = [vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .descriptor_count(1)
            .build()];

        let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        rend.dev.create_descriptor_set_layout(&info, None).unwrap()
    }

    /// Create vertex/index buffers for the default quad
    ///
    /// All onscreen regions will be represented by a quad, and
    /// we only need to create one set of vertex/index buffers
    /// for it.
    unsafe fn create_default_geom_bufs(
        rend: &Renderer,
    ) -> (vk::Buffer, vk::DeviceMemory, vk::Buffer, vk::DeviceMemory) {
        let (vbuf, vmem) = rend.create_buffer(
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            &QUAD_DATA,
        );
        let (ibuf, imem) = rend.create_buffer(
            vk::BufferUsageFlags::INDEX_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            &QUAD_INDICES,
        );

        return (vbuf, vmem, ibuf, imem);
    }

    /// Update a uniform buffer descriptor set with `buf`
    ///
    /// Update the entry in `set` at offset `element` to use the
    /// values in `buf`. Descriptor sets can be updated outside of
    /// command buffers.
    unsafe fn update_uniform_descriptor_set(
        &mut self,
        rend: &mut Renderer,
        buf: vk::Buffer,
        set: vk::DescriptorSet,
        binding: u32,
        element: u32,
    ) {
        let info = vk::DescriptorBufferInfo::builder()
            .buffer(buf)
            .offset(0)
            .range(mem::size_of::<ShaderConstants>() as u64)
            .build();
        let write_info = [vk::WriteDescriptorSet::builder()
            .dst_set(set)
            .dst_binding(binding)
            // descriptors can be arrays, so we need to specify an offset
            // into that array if applicable
            .dst_array_element(element)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(&[info])
            .build()];

        rend.dev.update_descriptor_sets(
            &write_info, // descriptor writes
            &[],         // descriptor copies
        );
    }

    /// Apply a transform matrix to all images
    ///
    /// This updates the model matrix of the shader constants
    /// used for all models
    pub fn transform_images(&mut self, rend: &Renderer, transform: &Matrix4<f32>) {
        let mut consts = GeomPipeline::get_shader_constants(rend.resolution);
        consts.model = consts.model * transform;

        unsafe {
            rend.update_memory(self.uniform_buffers_memory, 0, &[consts]);
        }
    }

    /// set up the depth image in self.
    ///
    /// We need to transfer the format of the depth image to something
    /// usable. We will use an image barrier to set the image as a depth
    /// stencil attachment to be used later.
    pub unsafe fn setup_depth_image(&mut self, rend: &Renderer) {
        // allocate a new cbuf for us to work with
        let new_cbuf = Renderer::create_command_buffers(&rend.dev, rend.pool, 1)[0]; // only get one

        // the depth image and view have already been created by new
        // we need to execute a cbuf to set up the memory we are
        // going to use later
        rend.cbuf_onetime(
            new_cbuf,
            rend.present_queue,
            &[], // wait_stages
            &[], // wait_semas
            &[], // signal_semas
            // this closure will be the contents of the cbuf
            |rend, cbuf| {
                // We need to initialize the depth attachment by
                // performing a layout transition to the optimal
                // depth layout
                //
                // we do not use rend.transition_image_layout since that
                // is specific to texture images
                let layout_barrier = vk::ImageMemoryBarrier::builder()
                    .image(self.depth_image)
                    // access patern for the resulting layout
                    .dst_access_mask(
                        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    )
                    // go from an undefined old layout to whatever the
                    // driver decides is the optimal depth layout
                    .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .subresource_range(
                        vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::DEPTH)
                            .layer_count(1)
                            .level_count(1)
                            .build(),
                    )
                    .build();

                // process the barrier we created, which will perform
                // the actual transition.
                rend.dev.cmd_pipeline_barrier(
                    cbuf,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[layout_barrier],
                );
            },
        );
    }

    /// Generate draw calls for this image
    ///
    /// It is a very common operation to draw a image, this
    /// helper draws itself at the locations passed by `push`
    ///
    /// First all descriptor sets and input assembly is bound
    /// before the call to vkCmdDrawIndexed. The descriptor
    /// sets should be updated whenever window contents are
    /// changed, and then cbufs should be regenerated using this.
    ///
    /// Base, is the offset to add to the surface's position.
    /// This is useful for subsurface offsets.
    ///
    /// Must be called while recording a cbuf
    pub fn record_surface_draw(
        &self,
        rend: &Renderer,
        params: &RecordParams,
        thundr_surf: &Surface,
        base: &(f32, f32),
        depth: f32,
    ) {
        let surf = thundr_surf.s_internal.borrow();
        let image = match surf.s_image.as_ref() {
            Some(i) => i,
            None => return,
        }
        .i_internal
        .borrow();

        let push = PushConstants {
            order: depth,
            x: base.0 + surf.s_rect.r_pos.0,
            y: base.1 + surf.s_rect.r_pos.1,
            width: surf.s_rect.r_size.0,
            height: surf.s_rect.r_size.1,
        };

        unsafe {
            // Descriptor sets can be updated elsewhere, but
            // they must be bound before drawing
            //
            // We need to bind both the uniform set, and the per-Image
            // set for the image sampler
            rend.dev.cmd_bind_descriptor_sets(
                params.cbuf,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0, // first set
                &[
                    self.ubo_descriptor,
                    image.i_sampler_descriptors[params.image_num],
                ],
                &[], // dynamic offsets
            );

            // Set the z-ordering of the window we want to render
            // (this sets the visible window ordering)
            rend.dev.cmd_push_constants(
                params.cbuf,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0, // offset
                // get a &[u8] from our struct
                // TODO: This should go. It is showing up as a noticeable
                // hit in profiling. Idk if there is a safe way to
                // replace it.
                bincode::serialize(&push).unwrap().as_slice(),
            );

            // Here is where everything is actually drawn
            // technically 3 vertices are being drawn
            // by the shader
            rend.dev.cmd_draw_indexed(
                params.cbuf,     // drawing command buffer
                self.vert_count, // number of verts
                1,               // number of instances
                0,               // first vertex
                0,               // vertex offset
                1,               // first instance
            );
        }
    }
}
