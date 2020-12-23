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

use utils::log;
use utils::region::Rect;

/// This is the width of a work group. This must match our shaders
const TILESIZE: u32 = 16;

/// This is the offset from the base of the winlist buffer to the
/// window array in the actual ssbo. This needs to match the `offset`
/// field in the `layout` qualifier in the shaders
const WINDOW_LIST_GLSL_OFFSET: isize = 16;

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
    /// It's filled with a window id, if no window is present then -1 is used. All ids
    /// listed in the color channels will be blended.
    cp_vis_buf: vk::Buffer,
    cp_vis_mem: vk::DeviceMemory,

    /// We keep a list of image views from the surface list's images
    /// to be passed as our unsized image array in our shader. This needs
    /// to be regenerated any time a change to the surfacelist is made
    cp_image_infos: Vec<vk::DescriptorImageInfo>,

    /// The compute queue
    cp_queue: vk::Queue,
    /// Queue family index for `cp_queue`
    cp_queue_family: u32,

    /// Allocated for our compute queue family
    cp_cbuf_pool: vk::CommandPool,
    cp_cbuf: vk::CommandBuffer,
}

/// Tile identifier
///
/// A tile is a number referring to a tile in our display.
/// The tile location is calculated by `get_base`.
use std::cmp::{Ord, PartialOrd};
#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone, PartialOrd, Ord)]
struct Tile(u32);

impl Tile {
    /// Convert screen coordinates into a Tile id
    /// `res_width` - the resolution stride (i.e. the row length)
    fn from_coord(x: u32, y: u32, res_width: u32) -> Tile {
        Tile((y / TILESIZE) * (res_width / TILESIZE) + (x / TILESIZE))
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
///
/// This *MUST* be a power of two, as the layout of the shader ssbo
/// is dependent on offsetting using the size of this.
#[repr(C)]
#[derive(Copy, Clone, Serialize, Deserialize)]
struct Window {
    /// The complete dimensions of the window.
    w_dims: Rect<i32>,
    /// Opaque region that tells the shader that we do not need to blend.
    /// This will have a r_pos.0 of -1 if no opaque data was attached.
    w_opaque: Rect<i32>,
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
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
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
                .ty(vk::DescriptorType::STORAGE_BUFFER)
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
            .buffer(self.cp_vis_buf)
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();

        let write_info = [
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_visibility.p_descs)
                .dst_binding(0)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&[vis_info])
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
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
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
                .ty(vk::DescriptorType::UNIFORM_BUFFER)
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
            .buffer(self.cp_vis_buf)
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
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&[vis_info])
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
            // two ints for the base/blend (see visibility.glsl)
            (mem::size_of::<u32>() as u32 * 2 * rend.resolution.width * rend.resolution.height) as u64;
        let (vis_buf, vis_mem) = unsafe {
            rend.create_buffer_with_size(
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::UNIFORM_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                vis_size,
            )
        };

        // bind our memory to our buffer representations
        unsafe {
            rend.dev.bind_buffer_memory(vis_buf, vis_mem, 0).unwrap();
        }

        // create our data and a storage buffer
        let winlist: Vec<Window> = Vec::with_capacity(64);
        let (wl_storage, wl_storage_mem) = unsafe {
            rend.create_buffer_with_size(
                vk::BufferUsageFlags::STORAGE_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                (std::mem::size_of::<Window>() * 64) as u64,
            )
        };
        unsafe {
            rend.dev
                .bind_buffer_memory(wl_storage, wl_storage_mem, 0)
                .unwrap();
        }

        let family = Self::get_queue_family(&rend.inst, &rend.display, rend.pdev).unwrap();
        let queue = unsafe { rend.dev.get_device_queue(family, 0) };
        let cpool = unsafe { Renderer::create_command_pool(&rend.dev, family) };
        let cbuf = unsafe { Renderer::create_command_buffers(&rend.dev, cpool, 1)[0] };

        CompPipeline {
            cp_visibility: vis,
            cp_composite: comp,
            cp_tiles: data,
            cp_tiles_buf: storage,
            cp_tiles_mem: storage_mem,
            cp_winlist: winlist,
            cp_winlist_buf: wl_storage,
            cp_winlist_mem: wl_storage_mem,
            cp_vis_buf: vis_buf,
            cp_vis_mem: vis_mem,
            cp_image_infos: Vec::new(),
            cp_queue: queue,
            cp_queue_family: family,
            cp_cbuf_pool: cpool,
            cp_cbuf: cbuf,
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
    fn clamp_to_grid(x: u32, max_width: u32) -> u32 {
        let r = x / TILESIZE * TILESIZE;
        if r > max_width {
            max_width
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
        self.cp_tiles.tiles.clear();
        for surf_rc in surfaces.iter() {
            // If the surface does not have damage attached, then don't generate tiles
            let surf = surf_rc.s_internal.borrow();
            let image = match surf.s_image.as_ref() {
                Some(i) => i.i_internal.borrow(),
                None => {
                    log::debug!(
                        "[thundr] warning: surface does not have image attached. Not drawing"
                    );
                    continue;
                }
            };

            let d = match surf_rc.get_damage() {
                Some(d) => d.d_region,
                None => {
                    log::debug!(
                        "[thundr] warning: surface does not have damage attached. Not drawing"
                    );
                    continue;
                }
            };
            let w = &surf.s_rect;

            // TODO: handle out of range values

            // get the true offset, since the damage is relative to the window
            //
            // Rect stores base and size, so add the size to the base to get the extent
            let d_end = (d.r_pos.0 + d.r_size.0, d.r_pos.1 + d.r_size.1);
            // Now offset the damage values from the window base
            let mut start = (
                w.r_pos.0 as u32 + d.r_pos.0 as u32,
                w.r_pos.1 as u32 + d.r_pos.1 as u32,
            );
            // do the same for the extent
            let mut end = (
                w.r_pos.0 as u32 + d_end.0 as u32,
                w.r_pos.1 as u32 + d_end.1 as u32,
            );

            // We need to clamp the values to our TILESIZExTILESIZE grid
            start = (
                Self::clamp_to_grid(start.0, rend.resolution.width),
                Self::clamp_to_grid(start.1, rend.resolution.width),
            );
            end = (
                Self::clamp_to_grid(end.0, rend.resolution.width),
                Self::clamp_to_grid(end.1, rend.resolution.width),
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

    fn gen_window_list(&mut self, surfaces: &SurfaceList) {
        self.cp_winlist.clear();
        for surf_rc in surfaces.iter() {
            let surf = surf_rc.s_internal.borrow();
            let opaque_reg = match surf_rc.get_opaque() {
                Some(r) => r,
                // If no opaque data was attached, place a -1 in the start.x component
                // to tell the shader to ignore this
                None => Rect::new(-1, 0, -1, 0),
            };

            self.cp_winlist.push(Window {
                w_dims: Rect::new(
                    surf.s_rect.r_pos.0 as i32,
                    surf.s_rect.r_pos.1 as i32,
                    surf.s_rect.r_size.0 as i32,
                    surf.s_rect.r_size.1 as i32,
                ),
                w_opaque: opaque_reg,
            });
        }
    }
}

impl Pipeline for CompPipeline {
    fn is_ready(&self) -> bool {
        true
    }

    fn draw(&mut self, rend: &Renderer, params: &RecordParams, surfaces: &SurfaceList) {
        unsafe {
            rend.dev
                .wait_for_fences(
                    &[rend.submit_fence],
                    true,          // wait for all
                    std::u64::MAX, //timeout
                )
                .unwrap();
            let ptr = rend
                .dev
                .map_memory(
                    self.cp_vis_mem,
                    0,
                    vk::WHOLE_SIZE,
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap();

            let dst = std::slice::from_raw_parts_mut(ptr as *mut i32, 256);
            println!("dst[] = {:?}", dst);

            rend.dev.unmap_memory(self.cp_vis_mem);

            // before recording, update our descriptor for our render target
            // get the current swapchain image
            self.gen_window_list(surfaces);
            self.gen_tile_list(rend, surfaces);
            let mut tile_vec: Vec<_> = self.cp_tiles.tiles.keys().map(|k| *k).collect();
            tile_vec.sort();

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
                WINDOW_LIST_GLSL_OFFSET,
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
            rend.cbuf_begin_recording(self.cp_cbuf, vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);

            // ----------- VISIBILITY PASS
            rend.dev.cmd_bind_pipeline(
                self.cp_cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_visibility.p_pipeline,
            );

            rend.dev.cmd_bind_descriptor_sets(
                self.cp_cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_visibility.p_pipeline_layout,
                0, // first set
                &[self.cp_visibility.p_descs],
                &[], // dynamic offsets
            );

            // Launch a wg for each tile
            rend.dev
                .cmd_dispatch(self.cp_cbuf, tile_vec.len() as u32, 1, 1);
            // ----------- END VISIBILITY PASS

            // We need to wait for the previous compute stage to complete
            rend.dev.cmd_pipeline_barrier(
                self.cp_cbuf,
                vk::PipelineStageFlags::COMPUTE_SHADER, // src_stage_mask
                vk::PipelineStageFlags::COMPUTE_SHADER // dst_stage_mask
                | vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[vk::MemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::HOST_READ)
                    .build()],
                &[vk::BufferMemoryBarrier::builder()
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::HOST_READ)
                    .src_queue_family_index(self.cp_queue_family)
                    .dst_queue_family_index(self.cp_queue_family)
                    .buffer(self.cp_vis_buf)
                    .offset(0)
                    .size(vk::WHOLE_SIZE)
                    .build()],
                &[],
            );

            // ----------- COMPOSITION PASS
            rend.dev.cmd_bind_pipeline(
                self.cp_cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_composite.p_pipeline,
            );

            rend.dev.cmd_bind_descriptor_sets(
                self.cp_cbuf,
                vk::PipelineBindPoint::COMPUTE,
                self.cp_composite.p_pipeline_layout,
                0, // first set
                &[self.cp_composite.p_descs],
                &[], // dynamic offsets
            );

            // Launch a wg for each tile
            rend.dev
                .cmd_dispatch(self.cp_cbuf, tile_vec.len() as u32, 1, 1);

            rend.cbuf_end_recording(self.cp_cbuf);
            // -------------------------------------------

            rend.cbuf_submit(
                // submit the cbuf for the current image
                self.cp_cbuf,
                self.cp_queue, // use our compute queue
                // wait_stages
                &[vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::HOST],
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

            rend.dev.destroy_buffer(self.cp_vis_buf, None);
            rend.free_memory(self.cp_vis_mem);

            self.cp_visibility.destroy(rend);
            self.cp_composite.destroy(rend);
        }
    }
}
