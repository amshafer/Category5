// This is the simplest and most traditional rendering backend
// It draws windows as textured quads
//
// Austin Shafer - 2020
#![allow(non_camel_case_types)]

use cgmath::{Matrix4, Vector2, Vector3};

use std::ffi::CString;
use std::io::Cursor;
use std::marker::Copy;
use std::mem;
use std::sync::Arc;

use ash::{util, vk};

use super::Pipeline;
use crate::display::frame::{PushConstants, RecordParams};
use crate::display::DisplayState;
use crate::{Device, Image, Result, Surface, Viewport};
use utils::{log, region::Rect};

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
    g_dev: Arc<Device>,
    pass: vk::RenderPass,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    /// Pool for command buffers
    g_pool: vk::CommandPool,
    /// the command buffers allocated from pool, there is one of these
    /// for each swapchain image
    g_cbufs: Vec<vk::CommandBuffer>,
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
    pub width: u32,
    pub height: u32,
}

impl Pipeline for GeomPipeline {
    /// Start recording a cbuf for one frame
    ///
    /// Each framebuffer has a set of resources, including command
    /// buffers. This records the cbufs for the framebuffer
    /// specified by `img`.
    fn begin_record(&mut self, dstate: &DisplayState) {
        // we need to clear any existing data when we start a pass
        let clear_vals = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 0.0],
            },
        }];

        // We want to start a render pass to hold all of
        // our drawing. The actual pass is started in the cbuf
        let pass_begin_info = vk::RenderPassBeginInfo::builder()
            .render_pass(self.pass)
            .framebuffer(self.framebuffers[dstate.d_current_image as usize])
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: dstate.d_resolution,
            })
            .clear_values(&clear_vals);

        let cbuf = self.g_cbufs[dstate.d_current_image as usize];

        unsafe {
            // start the cbuf
            self.g_dev
                .cbuf_begin_recording(cbuf, vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);

            // -- Setup static drawing resources
            // All of our drawing operations need
            // to be recorded inside a render pass.
            self.g_dev.dev.cmd_begin_render_pass(
                cbuf,
                &pass_begin_info,
                vk::SubpassContents::INLINE,
            );

            self.g_dev
                .dev
                .cmd_bind_pipeline(cbuf, vk::PipelineBindPoint::GRAPHICS, self.pipeline);

            // bind the vertex and index buffers from
            // the first image
            self.g_dev.dev.cmd_bind_vertex_buffers(
                cbuf,                // cbuf to draw in
                0,                   // first vertex binding updated by the command
                &[self.vert_buffer], // set of buffers to bind
                &[0],                // offsets for the above buffers
            );
            self.g_dev.dev.cmd_bind_index_buffer(
                cbuf,
                self.index_buffer,
                0, // offset
                vk::IndexType::UINT32,
            );
        }
    }

    /// Set the viewport
    ///
    /// This restricts the draw operations to within the specified region
    fn set_viewport(&mut self, dstate: &DisplayState, viewport: &Viewport) -> Result<()> {
        let cbuf = self.g_cbufs[dstate.d_current_image as usize];

        unsafe {
            log::info!("Viewport is : {:?}", viewport);

            // Set our current viewport
            self.g_dev.dev.cmd_set_viewport(
                cbuf,
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
            self.g_dev.dev.cmd_set_scissor(
                cbuf,
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
        }

        Ok(())
    }

    /// Our implementation of drawing one Surface
    ///
    /// This binds any resources for the surface's image and loads its
    /// data into the push constants. We can then draw the surface.
    fn draw(
        &mut self,
        params: &mut RecordParams,
        dstate: &DisplayState,
        surface: &Surface,
        image: Option<&Image>,
    ) -> bool {
        let cbuf = self.g_cbufs[dstate.d_current_image as usize];

        // update our cbuf constants. This is how we pass in
        // the viewport information
        self.update_surf_push_constants(surface, image, params);

        // If this surface has no content then skip drawing it
        let mut num_contents = (params.push.image_id >= 0) as i32;
        num_contents += params.push.use_color;
        if num_contents == 0 {
            return true;
        }

        // if we have an image bound to this surface grab its descriptor from the
        // imagevk. If not, then use the default tmp image
        let image_desc = match image {
            Some(img) => {
                let id = img.get_id();
                let imagevk = params
                    .image_vk
                    .get(&id)
                    .expect("Image does not have ImageVK");

                assert!(imagevk.iv_desc.d_set != vk::DescriptorSet::null());
                imagevk.iv_desc.d_set
            }
            None => self.g_dev.d_internal.read().unwrap().tmp_image_desc.d_set,
        };

        // TODO: If this surface is not contained in the viewport then don't draw it

        unsafe {
            // Bind this surface's backing texture if it has one. Descriptor
            // sets can be updated elsewhere, but they must be bound before drawing
            //
            // We need to bind both the uniform set, and the per-Image
            // set for the image sampler
            self.g_dev.dev.cmd_bind_descriptor_sets(
                cbuf,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0, // first set
                &[self.g_desc, image_desc],
                &[], // dynamic offsets
            );

            self.g_dev.dev.cmd_push_constants(
                cbuf,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0, // offset
                // Get the raw bytes for our push constants without doing any
                // expensinve serialization
                std::slice::from_raw_parts(
                    &params.push as *const _ as *const u8,
                    std::mem::size_of::<PushConstants>(),
                ),
            );

            // Draw this surface
            self.g_dev.dev.cmd_draw_indexed(
                cbuf,            // drawing command buffer
                self.vert_count, // number of verts
                1,               // number of instances
                0,               // first vertex
                0,               // vertex offset
                0,               // first instance
            );
            log::info!("Drawing surface at {:?}", surface.s_rect);
        }

        return true;
    }

    fn end_record(&mut self, dstate: &DisplayState) {
        let cbuf = self.g_cbufs[dstate.d_current_image as usize];
        unsafe {
            // make sure to end recording
            self.g_dev.dev.cmd_end_render_pass(cbuf);
            self.g_dev.cbuf_end_recording(cbuf);
        }
        // now submit the cbuf
        self.submit_frame(dstate);
    }

    /// Recreate our swapchain resources which are now out of date
    fn handle_ood(&mut self, dstate: &DisplayState) {
        unsafe {
            for f in self.framebuffers.iter() {
                self.g_dev.dev.destroy_framebuffer(*f, None);
            }

            let consts = GeomPipeline::get_shader_constants(dstate);
            self.g_dev
                .update_memory(self.uniform_buffers_memory, 0, &[consts]);

            self.framebuffers = GeomPipeline::create_framebuffers(&self.g_dev, self.pass, dstate);
            if self.g_cbufs.len() > 0 {
                self.g_dev
                    .dev
                    .free_command_buffers(self.g_pool, self.g_cbufs.as_slice());
            }
            self.g_cbufs.clear();

            self.g_cbufs = self
                .g_dev
                .create_command_buffers(self.g_pool, dstate.d_views.len() as u32);
        }
    }
}

impl Drop for GeomPipeline {
    fn drop(&mut self) {
        unsafe {
            self.g_dev.free_memory(self.vert_buffer_memory);
            self.g_dev.free_memory(self.index_buffer_memory);
            self.g_dev.dev.destroy_buffer(self.vert_buffer, None);
            self.g_dev.dev.destroy_buffer(self.index_buffer, None);

            self.g_dev
                .dev
                .free_command_buffers(self.g_pool, self.g_cbufs.as_slice());
            self.g_dev.dev.destroy_command_pool(self.g_pool, None);

            self.g_dev.dev.destroy_buffer(self.uniform_buffer, None);
            self.g_dev.free_memory(self.uniform_buffers_memory);

            self.g_dev.dev.destroy_render_pass(self.pass, None);

            self.g_dev
                .dev
                .destroy_descriptor_set_layout(self.g_desc_layout, None);

            self.g_dev
                .dev
                .destroy_descriptor_pool(self.g_desc_pool, None);

            self.g_dev
                .dev
                .destroy_pipeline_layout(self.pipeline_layout, None);

            for m in self.shader_modules.iter() {
                self.g_dev.dev.destroy_shader_module(*m, None);
            }

            for f in self.framebuffers.iter() {
                self.g_dev.dev.destroy_framebuffer(*f, None);
            }

            self.g_dev.dev.destroy_pipeline(self.pipeline, None);
        }
    }
}

impl GeomPipeline {
    /// Helper for getting the push constants
    ///
    /// This will be where we calculate the viewport scroll amount
    fn update_surf_push_constants(
        &mut self,
        surf: &Surface,
        image: Option<&Image>,
        params: &mut RecordParams,
    ) {
        // transform from blender's coordinate system to vulkan
        params.push.image_id = image.map(|i| i.get_id().get_raw_id() as i32).unwrap_or(-1);
        params.push.use_color = surf.s_color.is_some() as i32;
        params.push.color = match surf.s_color {
            Some((r, g, b, a)) => (r, g, b, a),
            // magic value so it's easy to debug
            // this is clear, since we don't have a color
            // assigned and we may not have an image bound.
            // In that case, we want this surface to be clear.
            None => (0.0, 50.0, 100.0, 0.0),
        };
        params.push.dims = Rect::new(
            surf.s_rect.r_pos.0,
            surf.s_rect.r_pos.1,
            surf.s_rect.r_size.0,
            surf.s_rect.r_size.1,
        );
    }

    /// Create a descriptor pool for the uniform buffer
    ///
    /// All other dynamic sets are tracked using a DescPool. This pool
    /// is for statically numbered resources.
    ///
    /// The pool returned is NOT thread safe
    pub unsafe fn create_descriptor_pool(dev: &Device) -> vk::DescriptorPool {
        let size = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)
            .build()];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);

        dev.dev.create_descriptor_pool(&info, None).unwrap()
    }

    /// Set up the application. This should *always* be called
    ///
    /// Once we have allocated a renderer with `new`, we should initialize
    /// the rendering pipeline so that we can display things. This method
    /// basically sets up all of the "application" specific resources like
    /// shaders, geometry, and the like.
    ///
    /// This fills in the GeomPipeline struct in the Renderer
    pub fn new(dev: Arc<Device>, dstate: &DisplayState) -> Result<GeomPipeline> {
        unsafe {
            let pass = GeomPipeline::create_pass(dstate.d_surface_format.format, &dev);

            // This is a really annoying issue with CString ptrs
            let program_entrypoint_name = CString::new("main").unwrap();
            // If the CString is created in `create_shaders`, and is inserted in
            // the return struct using the `.as_ptr()` method, then the CString
            // will still be dropped on return and our pointer will be garbage.
            // Instead we need to ensure that the CString will live long
            // enough. I have no idea why it is like this.
            let shader_stages = Box::new(GeomPipeline::create_shader_stages(
                &dev,
                program_entrypoint_name.as_ptr(),
            ));

            // prepare descriptors for all of the uniforms to pass to shaders
            //
            // NOTE: These need to be referenced in order by the `set` modifier
            // in the shaders
            let ubo_layout = GeomPipeline::create_ubo_layout(&dev);
            // These are the layout recognized by the pipeline
            let descriptor_layouts = &[
                ubo_layout, // set 0
                dev.d_internal.read().unwrap().descpool.ds_layout,
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
            let layout = dev.dev.create_pipeline_layout(&layout_info, None).unwrap();

            let pipeline =
                GeomPipeline::create_pipeline(dstate, &dev, layout, pass, &*shader_stages);

            // Allocate a pool only for the ubo descriptors
            let g_desc_pool = Self::create_descriptor_pool(&dev);
            let layouts = [ubo_layout];
            let info = vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(g_desc_pool)
                .set_layouts(&layouts)
                .build();

            let ubo = dev.dev.allocate_descriptor_sets(&info).unwrap()[0];

            let consts = GeomPipeline::get_shader_constants(dstate);

            // create a uniform buffer
            let (buf, mem) = dev.create_buffer(
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                // this specifies the constants to copy into the buffer
                &[consts],
            );

            // Allocate buffers for all geometry to be used
            let (vbuf, vmem, ibuf, imem) = GeomPipeline::create_default_geom_bufs(&dev);

            let graphics_queue_family = dstate.d_graphics_queue_family;
            dev.register_graphics_queue_family(graphics_queue_family);

            let pool = dev.create_command_pool(graphics_queue_family);

            // The app context contains the scene specific data
            let mut ctx = GeomPipeline {
                g_dev: dev,
                pass: pass,
                pipeline: pipeline,
                pipeline_layout: layout,
                g_desc_layout: ubo_layout,
                framebuffers: Vec::with_capacity(0),
                uniform_buffer: buf,
                uniform_buffers_memory: mem,
                g_pool: pool,
                g_cbufs: Vec::with_capacity(0),
                g_desc_pool: g_desc_pool,
                g_desc: ubo,
                shader_modules: shader_stages.iter().map(|info| info.module).collect(),
                vert_buffer: vbuf,
                vert_buffer_memory: vmem,
                // multiply the index len by the vector size
                vert_count: QUAD_INDICES.len() as u32 * 3,
                index_buffer: ibuf,
                index_buffer_memory: imem,
            };

            // now we need to update the descriptor set with the
            // buffer of the uniform constants to use
            ctx.update_uniform_descriptor_set();

            return Ok(ctx);
        }
    }

    /// Render a frame, but do not present it
    ///
    /// Think of this as the "main" rendering operation. It will draw
    /// all geometry to the current framebuffer. Presentation is
    /// done later, in case operations need to occur inbetween.
    fn submit_frame(&mut self, dstate: &DisplayState) {
        let mut wait_semas = Vec::new();
        if let Some(sema) = dstate.d_present_semas[dstate.d_current_image as usize] {
            wait_semas.push(sema);
        }

        let mut signal_semas = Vec::new();
        if dstate.d_needs_present_sema {
            signal_semas.push(dstate.d_frame_sema);
        }

        // Submit the recorded cbuf to perform the draw calls
        self.g_dev.cbuf_submit_async(
            // submit the cbuf for the current image
            self.g_cbufs[dstate.d_current_image as usize],
            dstate.d_present_queue, // the graphics queue
            wait_semas.as_slice(),
            signal_semas.as_slice(),
        );
    }

    /// create a renderpass for the color/depth attachments
    ///
    /// Render passses signify what attachments are used in which
    /// stages. They are composed of one or more subpasses.
    unsafe fn create_pass(format: vk::Format, dev: &Device) -> vk::RenderPass {
        // According to the spec we can only use PRESENT_SRC when vkSwapchain's
        // ext is enabled
        let layout = match dev.dev_features.vkc_supports_swapchain {
            true => vk::ImageLayout::PRESENT_SRC_KHR,
            false => vk::ImageLayout::GENERAL,
        };

        let attachments = [
            // the color dest. Its the surface we slected in Renderer::new.
            // see Renderer::create_swapchain for why we aren't using
            // the native surface formate
            vk::AttachmentDescription {
                format: format,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                store_op: vk::AttachmentStoreOp::STORE,
                initial_layout: vk::ImageLayout::UNDEFINED,
                final_layout: layout,
                ..Default::default()
            },
        ];

        // identify which of the above attachments
        let color_refs = [vk::AttachmentReference {
            attachment: 0, // index into the attachments variable
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];

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
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .build()];

        let create_info = vk::RenderPassCreateInfo::builder()
            .attachments(&attachments)
            .subpasses(&subpasses)
            .dependencies(&dependencies);

        dev.dev.create_render_pass(&create_info, None).unwrap()
    }

    /// Create a vkShaderModule for one of the dynamic pipeline stages
    ///
    /// dynamic portions of the graphics pipeline are programmable with
    /// spirv code. This helper function accepts a file name (`cursor`) and
    /// creates a shader module from it.
    ///
    /// `cursor` is accepted by ash's helper function, `read_spv`
    unsafe fn create_shader_module(
        dev: &Device,
        cursor: &mut Cursor<&'static [u8]>,
    ) -> vk::ShaderModule {
        let code = util::read_spv(cursor).expect("Could not read spv file");

        let info = vk::ShaderModuleCreateInfo::builder().code(&code);

        dev.dev
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
        dev: &Device,
        entrypoint: *const i8,
    ) -> [vk::PipelineShaderStageCreateInfo; 2] {
        let vert_shader = GeomPipeline::create_shader_module(
            dev,
            &mut Cursor::new(&include_bytes!("./shaders/vert.spv")[..]),
        );
        let frag_shader = GeomPipeline::create_shader_module(
            dev,
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
        dstate: &DisplayState,
        dev: &Device,
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
            width: dstate.d_resolution.width as f32,
            height: dstate.d_resolution.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        // no scissor test
        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: dstate.d_resolution,
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

        // Disable the depth test
        // We draw surfaces in order so we don't need this
        let depth_info = vk::PipelineDepthStencilStateCreateInfo {
            depth_test_enable: 0,
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
        dev.dev
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
        dev: &Device,
        pass: vk::RenderPass,
        dstate: &DisplayState,
    ) -> Vec<vk::Framebuffer> {
        // A framebuffer should be created for each of the swapchain
        // images. Reuse the depth buffer for all images since it
        // doesn't change.
        dstate
            .d_views
            .iter()
            .map(|&view| {
                let attachments = [view]; // color

                let info = vk::FramebufferCreateInfo::builder()
                    .render_pass(pass)
                    .attachments(&attachments)
                    .width(dstate.d_resolution.width)
                    .height(dstate.d_resolution.height)
                    .layers(1);

                dev.dev.create_framebuffer(&info, None).unwrap()
            })
            .collect()
    }

    /// Returns a `ShaderConstants` with the default values for this application
    ///
    /// Constants will be the contents of the uniform buffers which are
    /// processed by the shaders. The most obvious entry is the model + view
    /// + perspective projection matrix.
    fn get_shader_constants(dstate: &DisplayState) -> ShaderConstants {
        // transform from blender's coordinate system to vulkan
        let model = Matrix4::from_translation(Vector3::new(-1.0, -1.0, 0.0));

        ShaderConstants {
            model: model,
            width: dstate.d_resolution.width,
            height: dstate.d_resolution.height,
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
    unsafe fn create_ubo_layout(dev: &Device) -> vk::DescriptorSetLayout {
        // supplies `g_desc_layouts`
        // ubos for the MVP matrix and image samplers for textures
        let bindings = [vk::DescriptorSetLayoutBinding::builder()
            .binding(0)
            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .descriptor_count(1)
            .build()];

        let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        dev.dev.create_descriptor_set_layout(&info, None).unwrap()
    }

    /// Create vertex/index buffers for the default quad
    ///
    /// All onscreen regions will be represented by a quad, and
    /// we only need to create one set of vertex/index buffers
    /// for it.
    unsafe fn create_default_geom_bufs(
        dev: &Device,
    ) -> (vk::Buffer, vk::DeviceMemory, vk::Buffer, vk::DeviceMemory) {
        let (vbuf, vmem) = dev.create_buffer(
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            &QUAD_DATA,
        );
        let (ibuf, imem) = dev.create_buffer(
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
    unsafe fn update_uniform_descriptor_set(&mut self) {
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

        self.g_dev.dev.update_descriptor_sets(
            write_info, // descriptor writes
            &[],        // descriptor copies
        );
    }
}
