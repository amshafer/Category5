// A compositor that uses compute kernels to blend windows
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::ffi::CString;
use std::io::Cursor;
use std::mem;

use ash::version::DeviceV1_0;
use ash::{util, vk, Instance};

use super::Pipeline;
use crate::display::Display;
use crate::list::SurfaceList;
use crate::renderer::{RecordParams, Renderer};

use utils::region::Rect;

/// This is the width of a work group. This must match our shaders
const TILESIZE: u32 = 16;

struct Pass {
    /// A compute pipeline, which we will use to launch our shader
    p_pipeline: vk::Pipeline,
    p_pipeline_layout: vk::PipelineLayout,
    /// Our descriptor layout, specifying the format of data fed to the pipeline
    p_descriptor_layout: vk::DescriptorSetLayout,
    /// The module for our compute shader
    p_shader_modules: vk::ShaderModule,
    /// The pool that all descs in this struct are allocated from
    p_desc_pool: vk::DescriptorPool,
    p_descs: vk::DescriptorSet,
}

impl Pass {
    fn destroy(&mut self, rend: &mut Renderer) {
        unsafe {
            rend.dev
                .destroy_descriptor_set_layout(self.p_descriptor_layout, None);

            rend.dev.destroy_descriptor_pool(self.p_desc_pool, None);

            rend.dev
                .destroy_pipeline_layout(self.p_pipeline_layout, None);
            rend.dev.destroy_shader_module(self.p_shader_modules, None);
            rend.dev.destroy_pipeline(self.p_pipeline, None);
        }
    }
}

/// A compute pipeline
///
///
pub struct CompPipeline {
    /// These are the passes that render an image.
    /// This stage calculates which surfaces are visible for every
    /// pixel. Outputs to a visibility buffer
    cp_visibility: Pass,
    /// This stage consumes the visibility buffer, and samples
    /// the window contents that are stored in the final frame image
    cp_composite: Pass,

    /// Our buffer containing our window locations
    cp_tiles: TileList,
    cp_tiles_buf: vk::Buffer,
    cp_tiles_mem: vk::DeviceMemory,

    /// The list of window dimensions that is passed to the shader
    cp_winlist: Vec<Window>,
    cp_winlist_buf: vk::Buffer,
    cp_winlist_mem: vk::DeviceMemory,

    /// Our visibility buffer.
    /// The visibility pass will fill this with the id of the window that is visible
    /// and should be drawn during the composition pass.
    /// A storage texel buffer is used with the rg32ui format, since it allows us
    /// to more efficiently load/store the components. Each color channel will be
    /// filled with a window id, if no window is present then -1 is used. All ids
    /// listed in the color channels will be blended.
    cp_vis_store_buf: vk::Buffer,
    cp_vis_uniform_buf: vk::Buffer,
    cp_vis_mem: vk::DeviceMemory,
    cp_vis_store_view: vk::BufferView,
    cp_vis_uni_view: vk::BufferView,

    /// We keep a list of image views from the surface list's images
    /// to be passed as our unsized image array in our shader. This needs
    /// to be regenerated any time a change to the surfacelist is made
    cp_image_infos: Vec<vk::DescriptorImageInfo>,

    /// The compute queue
    cp_queue: vk::Queue,
    /// Queue family index for `cp_queue`
    cp_queue_family: u32,
}

/// Tile identifier
///
/// A tile is a number referring to a tile in our display.
/// The tile location is calculated by `get_base`.
#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
struct Tile(u32);

impl Tile {
    /// Convert screen coordinates into a Tile id
    /// `res_width` - the resolution stride (i.e. the row length)
    fn from_coord(x: u32, y: u32, res_width: u32) -> Tile {
        Tile((y / TILESIZE) * res_width + (x / TILESIZE))
    }

    /// Convert a tile number to an offset into a display
    /// `rw` - resolution width
    fn get_base(&self, rw: u32) -> (u32, u32) {
        let x = self.0 % rw; // get the
        let y = self.0 / rw; // get the number of rows into
        (x * TILESIZE, y * TILESIZE)
    }
}

/// Our representation of window positions in the storage buffer
struct TileList {
    /// Resolution width
    width: u32,
    /// Resolution height
    height: u32,
    /// A list of tile ids that needs to be updated next frame
    tiles: HashMap<Tile, bool>,
}

/// This must match the definition of the Window struct in the
/// visibility shader.
#[repr(C)]
#[derive(Copy, Clone, Serialize, Deserialize)]
struct Window {
    /// The complete dimensions of the window.
    w_dims: Rect<i32>,
    /// Opaque region that tells the shader that we do not need to blend
    w_opaque: Rect<i32>,
    /// Should the shader use w_opaque while processing
    w_has_opaque: bool,
}

impl CompPipeline {
    fn vis_create_pass(rend: &mut Renderer) -> Pass {
        let layout = Self::vis_create_descriptor_layout(rend);
        let pool = Self::vis_create_descriptor_pool(rend);
        let descs = unsafe { rend.allocate_descriptor_sets(pool, &[layout])[0] };

        // This is a really annoying issue with CString ptrs
        let program_entrypoint_name = CString::new("main").unwrap();
        // If the CString is created in `create_shaders`, and is inserted in
        // the return struct using the `.as_ptr()` method, then the CString
        // will still be dropped on return and our pointer will be garbage.
        // Instead we need to ensure that the CString will live long
        // enough. I have no idea why it is like this.
        let mut curse = Cursor::new(&include_bytes!("./shaders/visibility.spv")[..]);
        let shader_stage = unsafe {
            CompPipeline::create_shader_stages(rend, program_entrypoint_name.as_ptr(), &mut curse)
        };

        let layouts = &[layout];
        let pipe_layout_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(layouts);
        let pipe_layout = unsafe {
            rend.dev
                .create_pipeline_layout(&pipe_layout_info, None)
                .unwrap()
        };

        let pipe_info = vk::ComputePipelineCreateInfo::builder()
            .stage(shader_stage)
            .layout(pipe_layout)
            .build();
        let pipeline = unsafe {
            rend.dev
                .create_compute_pipelines(vk::PipelineCache::null(), &[pipe_info], None)
                .unwrap()[0]
        };

        Pass {
            p_pipeline: pipeline,
            p_pipeline_layout: pipe_layout,
            p_shader_modules: shader_stage.module,
            p_descriptor_layout: layout,
            p_desc_pool: pool,
            p_descs: descs,
        }
    }

    /// Creates descriptor sets for our compute resources.
    /// For now this just includes a swapchain image to render things
    /// to, and a storage buffer.
    pub fn vis_create_descriptor_layout(rend: &Renderer) -> vk::DescriptorSetLayout {
        let bindings = [
            // See visibility.comp.glsl for details
            // This is our visibility buffer, but we store it as an image so that
            // we can refer to components of a 32-bit value efficiently
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_TEXEL_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .descriptor_count(1)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .descriptor_count(1)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .descriptor_count(1)
                .build(),
        ];
        let info = vk::DescriptorSetLayoutCreateInfo::builder()
            .bindings(&bindings)
            .build();

        unsafe { rend.dev.create_descriptor_set_layout(&info, None).unwrap() }
    }

    /// Create a descriptor pool to allocate from.
    /// The sizes in this must match `create_descriptor_layout`
    pub fn vis_create_descriptor_pool(rend: &Renderer) -> vk::DescriptorPool {
        let size = [
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_TEXEL_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .build(),
        ];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);

        unsafe { rend.dev.create_descriptor_pool(&info, None).unwrap() }
    }

    fn vis_write_descs(
        &self,
        rend: &Renderer,
        tile_info: &[vk::DescriptorBufferInfo],
        window_info: &[vk::DescriptorBufferInfo],
    ) {
        let vis_info = vk::DescriptorBufferInfo::builder()
            .buffer(self.cp_vis_store_buf)
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();

        let write_info = [
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_visibility.p_descs)
                .dst_binding(0)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_TEXEL_BUFFER)
                .buffer_info(&[vis_info])
                .texel_buffer_view(&[self.cp_vis_store_view])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_visibility.p_descs)
                .dst_binding(1)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(tile_info)
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_visibility.p_descs)
                .dst_binding(2)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(window_info)
                .build(),
        ];
        unsafe {
            rend.dev.update_descriptor_sets(
                &write_info, // descriptor writes
                &[],         // descriptor copies
            );
        }
    }

    fn comp_create_pass(rend: &mut Renderer) -> Pass {
        let layout = Self::comp_create_descriptor_layout(rend);
        let pool = Self::comp_create_descriptor_pool(rend);
        let descs = unsafe { rend.allocate_descriptor_sets(pool, &[layout])[0] };

        // This is a really annoying issue with CString ptrs
        let program_entrypoint_name = CString::new("main").unwrap();
        // If the CString is created in `create_shaders`, and is inserted in
        // the return struct using the `.as_ptr()` method, then the CString
        // will still be dropped on return and our pointer will be garbage.
        // Instead we need to ensure that the CString will live long
        // enough. I have no idea why it is like this.
        let mut curse = Cursor::new(&include_bytes!("./shaders/composite.spv")[..]);
        let shader_stage = unsafe {
            CompPipeline::create_shader_stages(rend, program_entrypoint_name.as_ptr(), &mut curse)
        };

        let layouts = &[layout];
        let pipe_layout_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(layouts);
        let pipe_layout = unsafe {
            rend.dev
                .create_pipeline_layout(&pipe_layout_info, None)
                .unwrap()
        };

        let pipe_info = vk::ComputePipelineCreateInfo::builder()
            .stage(shader_stage)
            .layout(pipe_layout)
            .build();
        let pipeline = unsafe {
            rend.dev
                .create_compute_pipelines(vk::PipelineCache::null(), &[pipe_info], None)
                .unwrap()[0]
        };

        Pass {
            p_pipeline: pipeline,
            p_pipeline_layout: pipe_layout,
            p_shader_modules: shader_stage.module,
            p_descriptor_layout: layout,
            p_desc_pool: pool,
            p_descs: descs,
        }
    }

    /// Creates descriptor sets for our compute resources.
    /// For now this just includes a swapchain image to render things
    /// to, and a storage buffer.
    pub fn comp_create_descriptor_layout(rend: &Renderer) -> vk::DescriptorSetLayout {
        let bindings = [
            // See visibility.comp.glsl for details
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .descriptor_count(1)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(2)
                .descriptor_type(vk::DescriptorType::UNIFORM_TEXEL_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorSetLayoutBinding::builder()
                .binding(3)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .descriptor_count(1)
                .build(),
        ];
        let mut info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        // We need to attach some binding flags stating that we intend
        // to use the storage image as an unsized array
        // TODO: request update_after_bind
        let usage_info = vk::DescriptorSetLayoutBindingFlagsCreateInfoEXT::builder()
            .binding_flags(&[
                vk::DescriptorBindingFlags::empty(), // the storage image
                vk::DescriptorBindingFlags::empty(), // the storage buffer
                vk::DescriptorBindingFlags::empty(), // the visibility buffer
                vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT, // the image array
            ])
            .build();
        info.p_next = &usage_info as *const _ as *mut std::ffi::c_void;

        unsafe { rend.dev.create_descriptor_set_layout(&info, None).unwrap() }
    }

    /// Create a descriptor pool to allocate from.
    /// The sizes in this must match `create_descriptor_layout`
    pub fn comp_create_descriptor_pool(rend: &Renderer) -> vk::DescriptorPool {
        let size = [
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1)
                .build(),
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::UNIFORM_TEXEL_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(1)
                .build(),
        ];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);

        unsafe { rend.dev.create_descriptor_pool(&info, None).unwrap() }
    }

    fn comp_write_descs(&self, rend: &Renderer, tile_info: &[vk::DescriptorBufferInfo]) {
        let vis_info = vk::DescriptorBufferInfo::builder()
            .buffer(self.cp_vis_uniform_buf)
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();

        // Our swapchain image we want to write to
        let fb_info = vk::DescriptorImageInfo::builder()
            .sampler(rend.image_sampler)
            .image_view(rend.views[rend.current_image as usize])
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .build();

        let write_info = [
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_composite.p_descs)
                .dst_binding(0)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .image_info(&[fb_info])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_composite.p_descs)
                .dst_binding(1)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(tile_info)
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_composite.p_descs)
                .dst_binding(2)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::UNIFORM_TEXEL_BUFFER)
                .buffer_info(&[vis_info])
                .texel_buffer_view(&[self.cp_vis_uni_view])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_composite.p_descs)
                .dst_binding(3)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                .image_info(self.cp_image_infos.as_slice())
                .build(),
        ];
        unsafe {
            rend.dev.update_descriptor_sets(
                &write_info, // descriptor writes
                &[],         // descriptor copies
            );
        }
    }

    pub fn new(rend: &mut Renderer) -> Self {
        let vis = Self::vis_create_pass(rend);
        let comp = Self::comp_create_pass(rend);

        // create our data and a storage buffer
        // calculate the total number of tiles based on the wg (16x16) size
        let tile_count =
            (rend.resolution.width * rend.resolution.height) as usize / (16 * 16) as usize;
        let data = TileList {
            width: rend.resolution.width,
            height: rend.resolution.height,
            tiles: HashMap::with_capacity(tile_count),
        };
        let (storage, storage_mem) = unsafe {
            rend.create_buffer_with_size(
                vk::BufferUsageFlags::STORAGE_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                // two ints for w/h and n for our tiles
                (mem::size_of::<u32>() * 2 + mem::size_of::<u32>() * tile_count) as u64,
            )
        };
        unsafe {
            rend.dev
                .bind_buffer_memory(storage, storage_mem, 0)
                .unwrap();
        }

        // Create the visibility buffer
        let vis_size =
            (mem::size_of::<u32>() as u32 * rend.resolution.width * rend.resolution.height) as u64;
        let (vis_buf, vis_mem) = unsafe {
            rend.create_buffer_with_size(
                vk::BufferUsageFlags::STORAGE_TEXEL_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vis_size,
            )
        };
        // Storage Texel buffers require that we have a buffer view
        let vis_store_view = unsafe {
            let info = vk::BufferViewCreateInfo::builder()
                .buffer(vis_buf)
                .format(vk::Format::R16G16_SINT)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build();
            rend.dev.create_buffer_view(&info, None).unwrap()
        };
        // We also need a uniform buffer pointing at the same data
        let vis_uniform = unsafe {
            let info = vk::BufferCreateInfo::builder()
                .usage(vk::BufferUsageFlags::UNIFORM_TEXEL_BUFFER)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .size(vis_size)
                .build();

            rend.dev.create_buffer(&info, None).unwrap()
        };
        let vis_uni_view = unsafe {
            let info = vk::BufferViewCreateInfo::builder()
                .buffer(vis_uniform)
                .format(vk::Format::R16G16_SINT)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build();
            rend.dev.create_buffer_view(&info, None).unwrap()
        };
        // bind our memory to our buffer representations
        unsafe {
            rend.dev.bind_buffer_memory(vis_buf, vis_mem, 0).unwrap();
            rend.dev
                .bind_buffer_memory(vis_uniform, vis_mem, 0)
                .unwrap();
        }

        // create our data and a storage buffer
        let winlist: Vec<Window> = Vec::with_capacity(64);
        let (wl_storage, wl_storage_mem) = unsafe {
            rend.create_buffer(
                vk::BufferUsageFlags::STORAGE_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                bincode::serialize(&winlist).unwrap().as_slice(),
            )
        };

        let family = Self::get_queue_family(&rend.inst, &rend.display, rend.pdev).unwrap();
        let queue = unsafe { rend.dev.get_device_queue(family, 0) };

        CompPipeline {
            cp_visibility: vis,
            cp_composite: comp,
            cp_tiles: data,
            cp_tiles_buf: storage,
            cp_tiles_mem: storage_mem,
            cp_winlist: winlist,
            cp_winlist_buf: wl_storage,
            cp_winlist_mem: wl_storage_mem,
            cp_vis_uniform_buf: vis_uniform,
            cp_vis_store_buf: vis_buf,
            cp_vis_mem: vis_mem,
            cp_vis_store_view: vis_store_view,
            cp_vis_uni_view: vis_uni_view,
            cp_image_infos: Vec::new(),
            cp_queue: queue,
            cp_queue_family: family,
        }
    }

    /// Get a queue family that this pipeline needs to support.
    /// This needs to be added to the renderer's `create_device`.
    pub fn get_queue_family(
        inst: &Instance,
        display: &Display,
        pdev: vk::PhysicalDevice,
    ) -> Option<u32> {
        // get the properties per queue family
        Some(unsafe {
            Renderer::select_queue_family(
                inst,
                pdev,
                &display.surface_loader,
                display.surface,
                vk::QueueFlags::COMPUTE,
            )
        })
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
        curse: &mut Cursor<&[u8]>,
    ) -> vk::PipelineShaderStageCreateInfo {
        let code = util::read_spv(curse).expect("Could not read spv file");

        let info = vk::ShaderModuleCreateInfo::builder().code(&code);

        let shader = rend
            .dev
            .create_shader_module(&info, None)
            .expect("Could not create new shader module");

        vk::PipelineShaderStageCreateInfo {
            module: shader,
            p_name: entrypoint,
            stage: vk::ShaderStageFlags::COMPUTE,
            ..Default::default()
        }
    }

    /// Clamps a value to the 4x4 tilegrid positions. i.e. `62 -> 60`. This is
    /// used to get the address of a tile from an arbitrary point in the display.
    fn clamp_to_grid(x: u32, tilesize: u32) -> u32 {
        let r = x / tilesize * tilesize;
        if r > 60 {
            60
        } else {
            r
        }
    }

    /// Generate a list of tiles that need to be redrawn.
    ///
    /// Our display is grouped into 4x4 tiles of pixels, each
    /// of which is updated by one workgroup. This method take a list
    /// of damage regions, and generates a list of the tiles that need to be
    /// updated. This tilelist is passed to our drawing function.
    fn gen_tile_list(&mut self, rend: &Renderer, surfaces: &SurfaceList) {
        for surf_rc in surfaces.iter() {
            // If the surface does not have damage attached, then don't generate tiles
            let surf = surf_rc.s_internal.borrow();
            let image = match surf.s_image.as_ref() {
                Some(i) => i.i_internal.borrow(),
                None => continue,
            };

            let d = match image.i_damage.as_ref() {
                Some(d) => &d.d_region,
                None => continue,
            };
            let w = &surf.s_rect;

            // get the true offset, since the damage is relative to the window
            //
            // Rect stores base and size, so add the size to the base to get the extent
            let d_end = (d.r_pos.0 + d.r_size.0, d.r_pos.1 + d.r_size.1);
            // Now offset the damage values from the window base
            let mut start = (
                (w.r_pos.0 + d.r_pos.0) as u32,
                (w.r_pos.1 + d.r_pos.1) as u32,
            );
            // do the same for the extent
            let mut end = ((w.r_pos.0 + d_end.0) as u32, (w.r_pos.1 + d_end.1) as u32);

            // We need to clamp the values to our TILESIZExTILESIZE grid
            start = (
                Self::clamp_to_grid(start.0, TILESIZE),
                Self::clamp_to_grid(start.1, TILESIZE),
            );
            end = (
                Self::clamp_to_grid(end.0, TILESIZE),
                Self::clamp_to_grid(end.1, TILESIZE),
            );

            // Now we can go through the tiles this region overlaps with
            // and add them to the tile list
            while start.1 <= end.1 {
                let mut offset = start.0;
                while offset <= end.0 {
                    self.cp_tiles.tiles.insert(
                        Tile::from_coord(offset, start.1, rend.resolution.width),
                        true,
                    );
                    offset += TILESIZE;
                }

                start.1 += TILESIZE;
            }
        }
    }
}

impl Pipeline for CompPipeline {
    fn is_ready(&self) -> bool {
        true
    }

    fn draw(&mut self, rend: &Renderer, params: &RecordParams, surfaces: &SurfaceList) {
        unsafe {
            // before recording, update our descriptor for our render target
            // get the current swapchain image
            // TODO: fill in window list
            self.gen_tile_list(rend, surfaces);
            let tile_vec: Vec<_> = self.cp_tiles.tiles.keys().map(|k| *k).collect();

            // Shader expects struct WindowList { int width; int height; Window windows[] }
            // so we need to write the length first
            rend.update_memory(
                self.cp_tiles_mem,
                0,
                &[rend.resolution.width, rend.resolution.height],
            );
            rend.update_memory(
                self.cp_tiles_mem,
                // We need to offset by the size of two ints, which is
                // the first field in the struct expected by the shader
                mem::size_of::<u32>() as isize * 2,
                tile_vec.as_slice(),
            );

            // Shader expects struct WindowList { int count; Window windows[] }
            rend.update_memory(self.cp_winlist_mem, 0, &[self.cp_winlist.len()]);
            rend.update_memory(
                self.cp_winlist_mem,
                (mem::size_of::<Window>() * self.cp_winlist.len()) as isize,
                self.cp_winlist.as_slice(),
            );

            // Now update the actual descriptors
            let tiles_write = vk::DescriptorBufferInfo::builder()
                .buffer(self.cp_tiles_buf)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build();
            let windows_write = vk::DescriptorBufferInfo::builder()
                .buffer(self.cp_winlist_buf)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build();

            // Construct a list of image views from the submitted surface list
            // this will be our unsized texture array that the composite shader will reference
            self.cp_image_infos.clear();
            for s in surfaces.iter() {
                if let Some(image) = s.get_image() {
                    self.cp_image_infos.push(
                        vk::DescriptorImageInfo::builder()
                            .sampler(rend.image_sampler)
                            .image_view(image.get_view())
                            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                            .build(),
                    );
                }
            }

            self.vis_write_descs(rend, &[tiles_write], &[windows_write]);
            self.comp_write_descs(rend, &[tiles_write]);

            // ------------------------------------------- RECORD
            rend.cbuf_begin_recording(params.cbuf, vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);

            // ----------- VISIBILITY PASS
            rend.dev.cmd_bind_pipeline(
                params.cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_visibility.p_pipeline,
            );

            rend.dev.cmd_bind_descriptor_sets(
                params.cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_visibility.p_pipeline_layout,
                0, // first set
                &[self.cp_visibility.p_descs],
                &[], // dynamic offsets
            );

            rend.dev.cmd_dispatch(
                params.cbuf,
                // Add an extra wg in to account for not dividing perfectly
                rend.resolution.width / 16 + 1,
                rend.resolution.height / 16 + 1,
                1,
            );
            // ----------- END VISIBILITY PASS

            // We need to wait for the previous compute stage to complete
            rend.dev.cmd_pipeline_barrier(
                params.cbuf,
                vk::PipelineStageFlags::COMPUTE_SHADER, // src_stage_mask
                vk::PipelineStageFlags::COMPUTE_SHADER, // dst_stage_mask
                vk::DependencyFlags::empty(),
                &[vk::MemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .build()],
                &[],
                &[],
            );

            // ----------- COMPOSITION PASS
            rend.dev.cmd_bind_pipeline(
                params.cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_composite.p_pipeline,
            );

            rend.dev.cmd_bind_descriptor_sets(
                params.cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_composite.p_pipeline_layout,
                0, // first set
                &[self.cp_composite.p_descs],
                &[], // dynamic offsets
            );

            rend.dev.cmd_dispatch(
                params.cbuf,
                // Add an extra wg in to account for not dividing perfectly
                rend.resolution.width / 16 + 1,
                rend.resolution.height / 16 + 1,
                1,
            );

            rend.cbuf_end_recording(params.cbuf);
            // -------------------------------------------

            rend.cbuf_submit(
                // submit the cbuf for the current image
                rend.cbufs[rend.current_image as usize],
                self.cp_queue, // use our compute queue
                // wait_stages
                &[vk::PipelineStageFlags::COMPUTE_SHADER],
                &[rend.present_sema], // wait_semas
                &[rend.render_sema],  // signal_semas
            );
        }
    }

    fn destroy(&mut self, rend: &mut Renderer) {
        unsafe {
            rend.dev.destroy_buffer(self.cp_tiles_buf, None);
            rend.free_memory(self.cp_tiles_mem);
            rend.dev.destroy_buffer(self.cp_winlist_buf, None);
            rend.free_memory(self.cp_winlist_mem);

            rend.dev.destroy_buffer(self.cp_vis_store_buf, None);
            rend.dev.destroy_buffer(self.cp_vis_uniform_buf, None);
            rend.dev.destroy_buffer_view(self.cp_vis_store_view, None);
            rend.dev.destroy_buffer_view(self.cp_vis_uni_view, None);
            rend.free_memory(self.cp_vis_mem);

            self.cp_visibility.destroy(rend);
            self.cp_composite.destroy(rend);
        }
    }
}
