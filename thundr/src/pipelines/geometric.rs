// This is the simplest and most traditional rendering backend
// It draws windows as textured quads
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]

use cgmath::{Matrix4, Vector2, Vector3};

use std::ffi::CString;
use std::io::Cursor;
use std::marker::Copy;
use std::mem;

use ash::{util, vk};

use super::Pipeline;
use crate::display::Display;
use crate::renderer::{PushConstants, RecordParams, Renderer};
use crate::{SurfaceList, Viewport};
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
    pipeline_layout: vk::PipelineLayout,
    /// This descriptor pool allocates only the 1 ubo
    g_desc_pool: vk::DescriptorPool,
    /// (as per `create_descriptor_layouts`)
    /// This will only be the sets holding the uniform buffers,
    /// any image specific descriptors are in the image's sets.
    g_desc_layout: vk::DescriptorSetLayout,
    g_desc: vk::DescriptorSet,
    shader_modules: Vec<vk::ShaderModule>,
    framebuffers: Vec<vk::Framebuffer>,
    /// shader constants are shared by all swapchain images
    uniform_buffer: vk::Buffer,
    uniform_buffers_memory: vk::DeviceMemory,
    /// We will hold only one copy of the static QUAD_DATA
    /// which represents an onscreen window.
    vert_buffer: vk::Buffer,
    vert_buffer_memory: vk::DeviceMemory,
    vert_count: u32,
    /// Resources for the index buffer
    index_buffer: vk::Buffer,
    index_buffer_memory: vk::DeviceMemory,

    /// an image for recording depth test data
    depth_image: vk::Image,
    depth_image_view: vk::ImageView,
    /// because we create the image, we need to back it with memory
    depth_image_mem: vk::DeviceMemory,
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

impl Pipeline for GeomPipeline {
    fn is_ready(&self) -> bool {
        true
    }

    /// Start recording a cbuf for one frame
    ///
    /// Each framebuffer has a set of resources, including command
    /// buffers. This records the cbufs for the framebuffer
    /// specified by `img`.
    fn begin_record(&mut self, display: &Display, rend: &Renderer, params: &RecordParams) {
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
                        depth: 0.0,
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
                    extent: display.d_resolution,
                })
                .clear_values(&clear_vals);

            // start the cbuf
            rend.dev
                .cbuf_begin_recording(params.cbuf, vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);

            // -- Setup static drawing resources
            // All of our drawing operations need
            // to be recorded inside a render pass.
            rend.dev.dev.cmd_begin_render_pass(
                params.cbuf,
                &pass_begin_info,
                vk::SubpassContents::INLINE,
            );

            rend.dev.dev.cmd_bind_pipeline(
                params.cbuf,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );

            // bind the vertex and index buffers from
            // the first image
            rend.dev.dev.cmd_bind_vertex_buffers(
                params.cbuf,         // cbuf to draw in
                0,                   // first vertex binding updated by the command
                &[self.vert_buffer], // set of buffers to bind
                &[0],                // offsets for the above buffers
            );
            rend.dev.dev.cmd_bind_index_buffer(
                params.cbuf,
                self.index_buffer,
                0, // offset
                vk::IndexType::UINT32,
            );
        }
    }

    /// Our implementation of drawing one frame using geometry
    fn draw(
        &mut self,
        rend: &mut Renderer,
        params: &RecordParams,
        surfaces: &SurfaceList,
        pass_number: usize,
        viewport: &Viewport,
    ) -> bool {
        let pass = surfaces.l_pass[pass_number].as_ref().unwrap();

        unsafe {
            // Descriptor sets can be updated elsewhere, but
            // they must be bound before drawing
            //
            // We need to bind both the uniform set, and the per-Image
            // set for the image sampler
            rend.dev.dev.cmd_bind_descriptor_sets(
                params.cbuf,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0, // first set
                &[self.g_desc, pass.p_order_desc, rend.r_images_desc],
                &[], // dynamic offsets
            );

            // Now update our cbuf constants. This is how we pass in
            // the viewport information
            let consts = rend.get_push_constants(params, viewport);
            rend.dev.dev.cmd_push_constants(
                params.cbuf,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0, // offset
                // Get the raw bytes for our push constants without doing any
                // expensinve serialization
                std::slice::from_raw_parts(
                    &consts as *const _ as *const u8,
                    std::mem::size_of::<PushConstants>(),
                ),
            );

            log::info!("Viewport is : {:?}", viewport);

            // Set our current viewport
            rend.dev.dev.cmd_set_viewport(
                params.cbuf,
                0,
                &[vk::Viewport {
                    x: viewport.offset.0 as f32,
                    y: viewport.offset.1 as f32,
                    width: viewport.size.0 as f32,
                    height: viewport.size.1 as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            rend.dev.dev.cmd_set_scissor(
                params.cbuf,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D {
                        x: viewport.offset.0 as i32,
                        y: viewport.offset.1 as i32,
                    },
                    extent: vk::Extent2D {
                        width: viewport.size.0 as u32,
                        height: viewport.size.1 as u32,
                    },
                }],
            );

            // Actually draw our objects
            //
            // This is done with bindless+instanced drawing. We have one quad object that we will
            // draw and texture, but instance it for every surface in our surface list. This means
            // one draw call for any number of elements.
            //
            // Unfortunately it seems AMD's driver has a bug where they do not properly handle this
            // particular draw call sequence. The result is lots of corruption, in the form of
            // little squares which make it look like compressed pixel data was written to a linear
            // texture. So on AMD we do not do the instancing, but instead have a draw call for
            // every object (gross)
            if pass.p_window_order.len() > 0 {
                if rend.dev.dev_features.vkc_war_disable_instanced_drawing {
                    for i in 0..pass.p_window_order.len() as u32 {
                        // [WAR] Launch each instance manually :(
                        // TODO: skip if incorrect pass
                        rend.dev.dev.cmd_draw_indexed(
                            params.cbuf,     // drawing command buffer
                            self.vert_count, // number of verts
                            1,               // number of instances
                            0,               // first vertex
                            0,               // vertex offset
                            i as u32,        // first instance
                        );
                    }
                } else {
                    rend.dev.dev.cmd_draw_indexed(
                        params.cbuf,                      // drawing command buffer
                        self.vert_count,                  // number of verts
                        pass.p_window_order.len() as u32, // number of instances
                        0,                                // first vertex
                        0,                                // vertex offset
                        0,                                // first instance
                    );
                }
            }
            log::info!("Drawing {} objects", pass.p_window_order.len());
        }

        return true;
    }

    fn end_record(&mut self, rend: &mut Renderer, params: &RecordParams) {
        unsafe {
            // make sure to end recording
            rend.dev.dev.cmd_end_render_pass(params.cbuf);
            // Sync our dmabuf images
            //rend.add_image_barriers_for_dmabuf_images(params.cbuf, images);
            rend.dev.cbuf_end_recording(params.cbuf);
        }
        // now submit the cbuf
        self.submit_frame(rend);
    }

    fn debug_frame_print(&self) {
        log::debug!("Geometric Pipeline Debug Statistics:");
        log::debug!("---------------------------------");
        log::debug!("---------------------------------");
    }

    fn handle_ood(&mut self, display: &mut Display, rend: &mut Renderer) {
        unsafe {
            rend.dev.free_memory(self.depth_image_mem);
            rend.dev.dev.destroy_image_view(self.depth_image_view, None);
            rend.dev.dev.destroy_image(self.depth_image, None);
            for f in self.framebuffers.iter() {
                rend.dev.dev.destroy_framebuffer(*f, None);
            }

            let consts = GeomPipeline::get_shader_constants(display.d_resolution);
            rend.dev
                .update_memory(self.uniform_buffers_memory, 0, &[consts]);

            // the depth attachment needs to have its own resources
            let (depth_image, depth_image_view, depth_image_mem) = rend.dev.create_image(
                &display.d_resolution,
                vk::Format::D16_UNORM,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::ImageTiling::OPTIMAL,
            );
            self.depth_image = depth_image;
            self.depth_image_view = depth_image_view;
            self.depth_image_mem = depth_image_mem;
            self.setup_depth_image(rend);

            self.framebuffers = GeomPipeline::create_framebuffers(
                rend,
                self.pass,
                display.d_resolution,
                self.depth_image_view,
            );
        }
    }

    fn destroy(&mut self, rend: &mut Renderer) {
        unsafe {
            rend.dev.free_memory(self.vert_buffer_memory);
            rend.dev.free_memory(self.index_buffer_memory);
            rend.dev.dev.destroy_buffer(self.vert_buffer, None);
            rend.dev.dev.destroy_buffer(self.index_buffer, None);

            rend.dev.free_memory(self.depth_image_mem);
            rend.dev.dev.destroy_image_view(self.depth_image_view, None);
            rend.dev.dev.destroy_image(self.depth_image, None);

            rend.dev.dev.destroy_buffer(self.uniform_buffer, None);
            rend.dev.free_memory(self.uniform_buffers_memory);

            rend.dev.dev.destroy_render_pass(self.pass, None);

            rend.dev
                .dev
                .destroy_descriptor_set_layout(self.g_desc_layout, None);

            rend.dev.dev.destroy_descriptor_pool(self.g_desc_pool, None);

            rend.dev
                .dev
                .destroy_pipeline_layout(self.pipeline_layout, None);

            for m in self.shader_modules.iter() {
                rend.dev.dev.destroy_shader_module(*m, None);
            }

            for f in self.framebuffers.iter() {
                rend.dev.dev.destroy_framebuffer(*f, None);
            }

            rend.dev.dev.destroy_pipeline(self.pipeline, None);
        }
    }
}

impl GeomPipeline {
    /// Create a descriptor pool for the uniform buffer
    ///
    /// All other dynamic sets are tracked using a DescPool. This pool
    /// is for statically numbered resources.
    ///
    /// The pool returned is NOT thread safe
    pub unsafe fn create_descriptor_pool(rend: &mut Renderer) -> vk::DescriptorPool {
        let size = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .build()];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);

        rend.dev.dev.create_descriptor_pool(&info, None).unwrap()
    }

    /// Set up the application. This should *always* be called
    ///
    /// Once we have allocated a renderer with `new`, we should initialize
    /// the rendering pipeline so that we can display things. This method
    /// basically sets up all of the "application" specific resources like
    /// shaders, geometry, and the like.
    ///
    /// This fills in the GeomPipeline struct in the Renderer
    pub fn new(display: &mut Display, rend: &mut Renderer) -> GeomPipeline {
        unsafe {
            let pass = GeomPipeline::create_pass(display, rend);

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
                ubo_layout, // set 0
                rend.r_order_desc_layout,
                rend.r_images_desc_layout,
            ];

            // make a push constant entry for the z ordering of a window
            let constants = &[vk::PushConstantRange::builder()
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
                .offset(0)
                // depth is measured as a normalized float
                .size(std::mem::size_of::<PushConstants>() as u32)
                .build()];

            // even though we don't have anything special in our layout, we
            // still need to have a created layout for the pipeline
            let layout_info = vk::PipelineLayoutCreateInfo::builder()
                .push_constant_ranges(constants)
                .set_layouts(descriptor_layouts)
                .build();
            let layout = rend
                .dev
                .dev
                .create_pipeline_layout(&layout_info, None)
                .unwrap();

            let pipeline =
                GeomPipeline::create_pipeline(display, rend, layout, pass, &*shader_stages);

            // the depth attachment needs to have its own resources
            let (depth_image, depth_image_view, depth_image_mem) = rend.dev.create_image(
                &display.d_resolution,
                vk::Format::D16_UNORM,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::ImageTiling::OPTIMAL,
            );

            let framebuffers = GeomPipeline::create_framebuffers(
                rend,
                pass,
                display.d_resolution,
                depth_image_view,
            );

            // Allocate a pool only for the ubo descriptors
            let g_desc_pool = Self::create_descriptor_pool(rend);
            let ubo = rend.allocate_descriptor_sets(g_desc_pool, &[ubo_layout])[0];

            let consts = GeomPipeline::get_shader_constants(display.d_resolution);

            // create a uniform buffer
            let (buf, mem) = rend.dev.create_buffer(
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
                g_desc_layout: ubo_layout,
                framebuffers: framebuffers,
                uniform_buffer: buf,
                uniform_buffers_memory: mem,
                g_desc_pool: g_desc_pool,
                g_desc: ubo,
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
            ctx.update_uniform_descriptor_set(rend);
            ctx.setup_depth_image(rend);

            return ctx;
        }
    }

    /// Render a frame, but do not present it
    ///
    /// Think of this as the "main" rendering operation. It will draw
    /// all geometry to the current framebuffer. Presentation is
    /// done later, in case operations need to occur inbetween.
    fn submit_frame(&mut self, rend: &Renderer) {
        rend.wait_for_prev_submit();
        unsafe { rend.dev.dev.reset_fences(&[rend.submit_fence]).unwrap() };

        // Submit the recorded cbuf to perform the draw calls
        rend.dev.cbuf_submit_async(
            // submit the cbuf for the current image
            rend.cbufs[rend.current_image as usize],
            rend.r_present_queue, // the graphics queue
            // wait_stages
            &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
            &[rend.present_sema], // wait_semas
            &[rend.render_sema],  // signal_semas
            rend.submit_fence,    // signal fence
        );
    }

    /// create a renderpass for the color/depth attachments
    ///
    /// Render passses signify what attachments are used in which
    /// stages. They are composed of one or more subpasses.
    unsafe fn create_pass(display: &mut Display, rend: &Renderer) -> vk::RenderPass {
        let attachments = [
            // the color dest. Its the surface we slected in Renderer::new.
            // see Renderer::create_swapchain for why we aren't using
            // the native surface formate
            vk::AttachmentDescription {
                format: display.d_surface_format.format,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                store_op: vk::AttachmentStoreOp::STORE,
                initial_layout: vk::ImageLayout::UNDEFINED,
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

        rend.dev.dev.create_render_pass(&create_info, None).unwrap()
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
            .dev
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
        display: &mut Display,
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
            width: display.d_resolution.width as f32,
            height: display.d_resolution.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        // no scissor test
        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: display.d_resolution,
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
            depth_compare_op: vk::CompareOp::GREATER_OR_EQUAL,
            front: stencil_state,
            back: stencil_state,
            // one million objects is our max for now
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
            color_write_mask: vk::ColorComponentFlags::RGBA,
        }];

        let blend_info =
            vk::PipelineColorBlendStateCreateInfo::builder().attachments(&blend_attachment_states);

        // dynamic state specifies what parts of the pipeline will be
        // specified at draw time. (like moving the viewport)
        let dynamic_info = vk::PipelineDynamicStateCreateInfo::builder()
            .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR])
            .build();

        let pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_info)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport_info)
            .rasterization_state(&raster_info)
            .multisample_state(&multisample_info)
            .depth_stencil_state(&depth_info)
            .color_blend_state(&blend_info)
            .dynamic_state(&dynamic_info)
            .layout(layout)
            .render_pass(pass)
            .build();

        // Allocate one pipeline and return it
        rend.dev
            .dev
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

                rend.dev.dev.create_framebuffer(&info, None).unwrap()
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
        // supplies `g_desc_layouts`
        // ubos for the MVP matrix and image samplers for textures
        let bindings = [vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .descriptor_count(1)
            .build()];

        let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        rend.dev
            .dev
            .create_descriptor_set_layout(&info, None)
            .unwrap()
    }

    /// Create vertex/index buffers for the default quad
    ///
    /// All onscreen regions will be represented by a quad, and
    /// we only need to create one set of vertex/index buffers
    /// for it.
    unsafe fn create_default_geom_bufs(
        rend: &Renderer,
    ) -> (vk::Buffer, vk::DeviceMemory, vk::Buffer, vk::DeviceMemory) {
        let (vbuf, vmem) = rend.dev.create_buffer(
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            &QUAD_DATA,
        );
        let (ibuf, imem) = rend.dev.create_buffer(
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
    unsafe fn update_uniform_descriptor_set(&mut self, rend: &mut Renderer) {
        let info = &[vk::DescriptorBufferInfo::builder()
            .buffer(self.uniform_buffer)
            .offset(0)
            .range(mem::size_of::<ShaderConstants>() as u64)
            .build()];
        let write_info = &[vk::WriteDescriptorSet::builder()
            .dst_set(self.g_desc)
            .dst_binding(0)
            // descriptors can be arrays, so we need to specify an offset
            // into that array if applicable
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .buffer_info(info)
            .build()];

        rend.dev.dev.update_descriptor_sets(
            write_info, // descriptor writes
            &[],        // descriptor copies
        );
    }

    /// Apply a transform matrix to all images
    ///
    /// This updates the model matrix of the shader constants
    /// used for all models
    pub fn transform_images(
        &mut self,
        display: &mut Display,
        rend: &Renderer,
        transform: &Matrix4<f32>,
    ) {
        let mut consts = GeomPipeline::get_shader_constants(display.d_resolution);
        consts.model = consts.model * transform;

        rend.dev
            .update_memory(self.uniform_buffers_memory, 0, &[consts]);
    }

    /// set up the depth image in self.
    ///
    /// We need to transfer the format of the depth image to something
    /// usable. We will use an image barrier to set the image as a depth
    /// stencil attachment to be used later.
    pub unsafe fn setup_depth_image(&mut self, rend: &Renderer) {
        // allocate a new cbuf for us to work with
        let new_cbuf = rend.dev.create_command_buffers(rend.pool, 1)[0]; // only get one

        rend.dev
            .cbuf_begin_recording(new_cbuf, vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        // the depth image and view have already been created by new
        // we need to execute a cbuf to set up the memory we are
        // going to use later
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
        rend.dev.dev.cmd_pipeline_barrier(
            new_cbuf,
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[layout_barrier],
        );

        rend.dev.cbuf_submit_and_wait(
            new_cbuf,
            rend.r_present_queue,
            &[], // wait_stages
            &[], // wait_semas
            &[], // signal_semas
        );
    }
}
