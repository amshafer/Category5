// A compositor that uses compute kernels to blend windows
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]

use std::ffi::CString;
use std::io::Cursor;
use std::mem;

use ash::{util, vk, Instance};

use super::Pipeline;
use crate::display::Display;
use crate::renderer::{RecordParams, Renderer};
use crate::{Image, SurfaceList};

use utils::{log, timing::StopWatch};

/// This is the width of a work group. This must match our shaders
const TILESIZE: u32 = 16;

const MAX_IMAGE_LIMIT: u32 = 1024;

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

    /// Our visibility buffer.
    /// The visibility pass will fill this with the id of the window that is visible
    /// and should be drawn during the composition pass.
    /// It's filled with a window id, if no window is present then -1 is used. All ids
    /// listed in the color channels will be blended.
    cp_vis_buf: vk::Buffer,
    cp_vis_mem: vk::DeviceMemory,

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
        let width_in_tiles = (res_width / TILESIZE) + 1;
        Tile((y / TILESIZE) * width_in_tiles + (x / TILESIZE))
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
    tiles: Vec<Tile>,
    /// This is the list of tiles that have been added to `tiles`.
    /// If tile 4 has been added to `tiles`, `enabled_tiles[4]` will be set to true.
    enabled_tiles: Vec<bool>,
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

        let layouts = &[layout, rend.r_images_desc_layout];
        let pipe_layout_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(layouts)
            .build();
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
        ];
        let info = vk::DescriptorSetLayoutCreateInfo::builder()
            .bindings(&bindings)
            .build();

        unsafe { rend.dev.create_descriptor_set_layout(&info, None).unwrap() }
    }

    /// Create a descriptor pool to allocate from.
    /// The sizes in this must match `create_descriptor_layout`
    pub fn vis_create_descriptor_pool(rend: &Renderer) -> vk::DescriptorPool {
        let size = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(2)
            .build()];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);

        unsafe { rend.dev.create_descriptor_pool(&info, None).unwrap() }
    }

    fn vis_write_descs(&self, rend: &Renderer) {
        // Now update the actual descriptors
        let tile_info = vk::DescriptorBufferInfo::builder()
            .buffer(self.cp_tiles_buf)
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();
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
                .buffer_info(&[tile_info])
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
        let info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(&[layout])
            .build();
        let descs = unsafe { rend.dev.allocate_descriptor_sets(&info).unwrap()[0] };

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

        let layouts = &[layout, rend.r_images_desc_layout];
        let pipe_layout_info = vk::PipelineLayoutCreateInfo::builder()
            .set_layouts(layouts)
            .build();
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

        let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

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
                .descriptor_count(2)
                .build(),
        ];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);

        unsafe { rend.dev.create_descriptor_pool(&info, None).unwrap() }
    }

    fn comp_write_descs(&self, rend: &Renderer) {
        // Now update the actual descriptors
        let tile_info = vk::DescriptorBufferInfo::builder()
            .buffer(self.cp_tiles_buf)
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();
        let vis_info = vk::DescriptorBufferInfo::builder()
            .buffer(self.cp_vis_buf)
            .offset(0)
            .range(vk::WHOLE_SIZE)
            .build();

        // Our swapchain image we want to write to
        let fb_info = vk::DescriptorImageInfo::builder()
            .sampler(rend.image_sampler)
            .image_view(rend.views[rend.current_image as usize])
            .image_layout(vk::ImageLayout::GENERAL)
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
                .buffer_info(&[tile_info])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.cp_composite.p_descs)
                .dst_binding(2)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&[vis_info])
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
        // round up one tile since the shaders and `from_coord` do too.
        let tile_count = ((rend.resolution.width / TILESIZE + 1)
            * (rend.resolution.height / TILESIZE + 1)) as usize;
        let data = TileList {
            width: rend.resolution.width,
            height: rend.resolution.height,
            tiles: Vec::with_capacity(tile_count),
            enabled_tiles: std::iter::repeat(false).take(tile_count).collect(),
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
            // 4 ints for the base/blend (see visibility.glsl)
            (mem::size_of::<u32>() as u32 * 4 * rend.resolution.width * rend.resolution.height) as u64;
        let (vis_buf, vis_mem) = unsafe {
            rend.create_buffer_with_size(
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::STORAGE_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vis_size,
            )
        };

        // bind our memory to our buffer representations
        unsafe {
            rend.dev.bind_buffer_memory(vis_buf, vis_mem, 0).unwrap();
        }

        let family = Self::get_queue_family(&rend.inst, &rend.display, rend.pdev).unwrap();
        let queue = unsafe { rend.dev.get_device_queue(family, 0) };
        let cpool = unsafe { Renderer::create_command_pool(&rend.dev, family) };
        let cbuf = unsafe { Renderer::create_command_buffers(&rend.dev, cpool, 1)[0] };

        let mut cp = CompPipeline {
            cp_visibility: vis,
            cp_composite: comp,
            cp_tiles: data,
            cp_tiles_buf: storage,
            cp_tiles_mem: storage_mem,
            cp_vis_buf: vis_buf,
            cp_vis_mem: vis_mem,
            cp_queue: queue,
            cp_queue_family: family,
            cp_cbuf_pool: cpool,
            cp_cbuf: cbuf,
        };

        cp.gen_fullscreen_tilelist();
        unsafe { cp.flush_tile_mem(rend) };
        cp.vis_write_descs(rend);
        return cp;
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
                &display.d_surface_loader,
                display.d_surface,
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
    fn clamp_to_grid(x: i32, max_width: i32) -> i32 {
        let ts = TILESIZE as i32;
        let r = x / ts * ts;
        if r > max_width {
            max_width
        } else {
            r
        }
    }

    /// Fill in the tilelist for the entire screen. This is the initial
    /// value of the list.
    fn gen_fullscreen_tilelist(&mut self) {
        self.cp_tiles.tiles.clear();

        for i in 0..self.cp_tiles.tiles.capacity() {
            // Assigning each entry its own index is just adding
            // all possible tile ids in order
            self.cp_tiles.tiles.push(Tile(i as u32));
            self.cp_tiles.enabled_tiles[i] = true;
        }
    }

    /// Generate a list of tiles that need to be redrawn.
    ///
    /// Our display is grouped into 4x4 tiles of pixels, each
    /// of which is updated by one workgroup. This method take a list
    /// of damage regions, and generates a list of the tiles that need to be
    /// updated. This tilelist is passed to our drawing function.
    fn gen_tile_list(&mut self, rend: &Renderer) {
        // reset our current tile lists
        // by only clearing the tiles in the `tiles` list, we should prevent ourselves from
        // clearing the entire array when only 4 or 5 tiles are set
        for i in self.cp_tiles.tiles.iter_mut() {
            self.cp_tiles.enabled_tiles[i.0 as usize] = false;
        }
        self.cp_tiles.tiles.clear();

        // Use the damage regions calculated by Renderer
        for reg in rend.current_damage.iter() {
            // We need to clamp the values to our TILESIZExTILESIZE grid
            let mut start = (
                Self::clamp_to_grid(reg.offset.x, rend.resolution.width as i32),
                Self::clamp_to_grid(reg.offset.y, rend.resolution.width as i32),
            );

            // We need to clamp the extent to the proper width/height
            // Wayland clients may use INT_MAX for this
            let width = std::cmp::min(reg.extent.width, rend.resolution.width) as i32;
            let height = std::cmp::min(reg.extent.width, rend.resolution.height) as i32;

            let end = (
                Self::clamp_to_grid(reg.offset.x + width, rend.resolution.width as i32),
                Self::clamp_to_grid(reg.offset.y + height, rend.resolution.width as i32),
            );

            // Now we can go through the tiles this region overlaps with
            // and add them to the tile list
            while start.1 <= end.1 {
                let mut offset = start.0;
                while offset <= end.0 {
                    if offset >= 0 && start.1 >= 0 {
                        let tile =
                            Tile::from_coord(offset as u32, start.1 as u32, rend.resolution.width);
                        //log::debug!("adding {} for point ({}, {})", tile.0, offset, start.1);
                        // if this tile was not previously added, then add it now
                        if tile.0 < self.cp_tiles.enabled_tiles.len() as u32
                            && !self.cp_tiles.enabled_tiles[tile.0 as usize]
                        {
                            self.cp_tiles.enabled_tiles[tile.0 as usize] = true;
                            self.cp_tiles.tiles.push(tile);
                        }
                    }
                    offset += TILESIZE as i32;
                }

                start.1 += TILESIZE as i32;
            }
        }
    }

    unsafe fn flush_tile_mem(&self, rend: &Renderer) {
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
            self.cp_tiles.tiles.as_slice(),
        );
    }
}

impl Pipeline for CompPipeline {
    fn is_ready(&self) -> bool {
        true
    }

    fn draw(
        &mut self,
        rend: &mut Renderer,
        _params: &RecordParams,
        _images: &[Image],
        _surfaces: &mut SurfaceList,
    ) -> bool {
        unsafe {
            let mut stop = StopWatch::new();

            // Only update the tile list if we are doing incremental presentation
            // (aka damage). NVIDIA doesn't support this, so in that case we just
            // redraw the whole screen. The tile list should be constant in that case,
            // as it was initialized to be the entire screen.
            if rend.dev_features.vkc_supports_incremental_present {
                stop.start();
                self.gen_tile_list(rend);
                self.flush_tile_mem(rend);
                stop.end();
                log::debug!(
                    "Took {} ms to generate the tile list",
                    stop.get_duration().as_millis()
                );
            }

            // If no tiles were damaged, then we have nothing to render
            //if self.cp_tiles.tiles.len() == 0 {
            //    log::profiling!("No tiles damaged, not drawing anything");
            //    return false;
            //}

            // We need to do this afterwards, since it depends on cp_image_infos
            // This always needs to be done, since we are binding the latest swapchain image
            self.comp_write_descs(rend);
            // ------------------------------------------- RECORD
            stop.start();
            rend.cbuf_begin_recording(self.cp_cbuf, vk::CommandBufferUsageFlags::SIMULTANEOUS_USE);

            // First we need to transition our swapchain image to the GENERAL format
            // This is required by the spec for us to write to it from a compute shader
            let image_barrier = vk::ImageMemoryBarrier::builder()
                .image(rend.images[rend.current_image as usize])
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE)
                // go from an undefined layout to general
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(self.cp_queue_family)
                .dst_queue_family_index(self.cp_queue_family)
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .level_count(1)
                        .build(),
                )
                .build();
            rend.dev.cmd_pipeline_barrier(
                self.cp_cbuf,
                vk::PipelineStageFlags::TOP_OF_PIPE,    // src
                vk::PipelineStageFlags::COMPUTE_SHADER, // dst
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[image_barrier],
            );

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
                &[self.cp_visibility.p_descs, rend.r_images_desc],
                &[], // dynamic offsets
            );

            // Launch a wg for each tile
            rend.dev
                .cmd_dispatch(self.cp_cbuf, self.cp_tiles.tiles.len() as u32, 1, 1);
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
                &[self.cp_composite.p_descs, rend.r_images_desc],
                &[], // dynamic offsets
            );

            // Launch a wg for each tile
            rend.dev
                .cmd_dispatch(self.cp_cbuf, self.cp_tiles.tiles.len() as u32, 1, 1);

            // The final thing to do is transform the swapchain image back into
            // the presentable layout so it can be drawn to the screen.
            let image_barrier = vk::ImageMemoryBarrier::builder()
                .image(rend.images[rend.current_image as usize])
                .src_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::empty())
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_queue_family_index(self.cp_queue_family)
                .dst_queue_family_index(self.cp_queue_family)
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .level_count(1)
                        .build(),
                )
                .build();
            rend.dev.cmd_pipeline_barrier(
                self.cp_cbuf,
                vk::PipelineStageFlags::COMPUTE_SHADER, // src
                vk::PipelineStageFlags::BOTTOM_OF_PIPE, // dst
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[image_barrier],
            );

            //rend.add_image_barriers_for_dmabuf_images(self.cp_cbuf, images);
            rend.cbuf_end_recording(self.cp_cbuf);
            stop.end();
            log::debug!(
                "Took {} ms to record the cbuf for this frame",
                stop.get_duration().as_millis()
            );
            // -------------------------------------------

            rend.cbuf_submit(
                // submit the cbuf for the current image
                self.cp_cbuf,
                self.cp_queue, // use our compute queue
                // wait_stages
                &[vk::PipelineStageFlags::COMPUTE_SHADER],
                &[rend.present_sema], // wait_semas
                &[rend.render_sema],  // signal_semas
            );
        }
        return true;
    }

    fn debug_frame_print(&self) {
        log::debug!("Compute Pipeline Debug Statistics:");
        log::debug!("---------------------------------");
        log::debug!("Number of tiles to be drawn: {}", self.cp_tiles.tiles.len());
        log::debug!("---------------------------------");
    }

    fn destroy(&mut self, rend: &mut Renderer) {
        unsafe {
            rend.dev.destroy_buffer(self.cp_tiles_buf, None);
            rend.free_memory(self.cp_tiles_mem);
            rend.dev.destroy_command_pool(self.cp_cbuf_pool, None);

            rend.dev.destroy_buffer(self.cp_vis_buf, None);
            rend.free_memory(self.cp_vis_mem);

            self.cp_visibility.destroy(rend);
            self.cp_composite.destroy(rend);
        }
    }
}
