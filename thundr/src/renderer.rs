// A vulkan rendering backend
//
// This layer is very low, and as a result is mostly unsafe. Nothing
// unsafe/vulkan/ash/etc should be exposed to upper layers
//
// Austin Shafer - 2020
#![allow(non_camel_case_types)]
use serde::{Deserialize, Serialize};
use std::marker::Copy;
use std::sync::Arc;

use ash::vk;

use crate::display::Display;
use crate::instance::Instance;
use crate::list::SurfaceList;
use crate::{Device, Droppable, Surface, Viewport};

extern crate utils as cat5_utils;
use crate::{CreateInfo, Result};
use cat5_utils::{log, region::Rect};

use lluvia as ll;

/// This is the offset from the base of the winlist buffer to the
/// window array in the actual ssbo. This needs to match the `offset`
/// field in the `layout` qualifier in the shaders
pub const WINDOW_LIST_GLSL_OFFSET: isize = 16;

pub struct VkBarriers {
    /// Dmabuf import usage barrier list. Will be regenerated
    /// during every draw
    pub r_acquire_barriers: Vec<vk::ImageMemoryBarrier>,
    /// Dmabuf import release barriers. These let drm know vulkan
    /// is done using them.
    pub r_release_barriers: Vec<vk::ImageMemoryBarrier>,
}

// Manually define these for this struct, this is safe since it
// only references vulkan objects.
unsafe impl Send for VkBarriers {}
unsafe impl Sync for VkBarriers {}

/// Common bits of a vulkan renderer
///
/// The fields here are sure to change, as they are pretty
/// application specific.
///
/// The types in ash::vk:: are the 'normal' vulkan types
/// types in ash:: are normally 'loaders'. They take care of loading
/// function pointers and things. Think of them like a wrapper for
/// the raw vk:: type. In some cases you need both, surface
/// is a good example of this.
///
/// Application specific fields should be at the bottom of the
/// struct, with the commonly required fields at the top.
pub struct Renderer {
    /// The instance this rendering context was created from
    pub(crate) _inst: Arc<Instance>,
    /// The GPU this Renderer is resident on
    pub(crate) dev: Arc<Device>,

    /// One sampler for all swapchain images
    pub(crate) image_sampler: vk::Sampler,

    /// The pending release list
    /// This is the set of wayland resources used last frame
    /// for rendering that should now be released
    /// See WindowManger's worker_thread for more
    pub(crate) r_release: Vec<Box<dyn Droppable + Send + Sync>>,

    /// Our ECS
    pub r_ecs: ll::Instance,

    /// We keep a list of image views from the surface list's images
    /// to be passed as our unsized image array in our shader. This needs
    /// to be regenerated any time a change to the surfacelist is made
    pub(crate) r_images_desc_pool: vk::DescriptorPool,
    pub(crate) r_images_desc_layout: vk::DescriptorSetLayout,
    pub(crate) r_images_desc: vk::DescriptorSet,
    r_images_desc_size: usize,

    /// The descriptor layout for the surface list's window order desc
    pub r_order_desc_layout: vk::DescriptorSetLayout,

    /// The list of window dimensions that is passed to the shader
    pub r_windows: ll::Component<Window>,
    pub r_windows_buf: vk::Buffer,
    pub r_windows_mem: vk::DeviceMemory,
    /// The number of Windows that r_winlist_mem was allocate to hold
    pub r_windows_capacity: usize,

    /// Temporary image to bind to the image list when
    /// no images are attached.
    tmp_image: vk::Image,
    tmp_image_view: vk::ImageView,
    tmp_image_mem: vk::DeviceMemory,

    // We keep this around to ensure the image array isn't empty
    _r_null_image: ll::Entity,
    pub r_image_ecs: ll::Instance,
    pub r_image_infos: ll::NonSparseComponent<vk::DescriptorImageInfo>,

    /// Identical to the parent Thundr struct's session
    pub r_surface_pass: ll::Component<usize>,
}

/// This must match the definition of the Window struct in the
/// visibility shader.
///
/// This *MUST* be a power of two, as the layout of the shader ssbo
/// is dependent on offsetting using the size of this.
#[repr(C)]
#[derive(Default, Copy, Clone, Serialize, Deserialize, Debug)]
pub struct Window {
    /// The id of the image. This is the offset into the unbounded sampler array.
    /// id that's the offset into the unbound sampler array
    pub w_id: i32,
    /// if we should use w_color instead of texturing
    pub w_use_color: i32,
    /// the render pass count
    pub w_pass: i32,
    /// Padding to match our shader's struct
    w_padding: i32,
    /// Opaque color
    pub w_color: (f32, f32, f32, f32),
    /// The complete dimensions of the window.
    pub w_dims: Rect<i32>,
    /// Opaque region that tells the shader that we do not need to blend.
    /// This will have a r_pos.0 of -1 if no opaque data was attached.
    pub w_opaque: Rect<i32>,
}

/// Recording parameters
///
/// Layers above this one will need to call recording
/// operations. They need a private structure to pass
/// to Renderer to begin/end recording operations
/// This is that structure.
pub struct RecordParams {
    /// This calculates the depth we should use when starting to draw
    /// a set of surfaces in a viewport.
    ///
    /// This will start at 1.0, the max depth. Every surface will draw
    /// itself starting_depth - (gl_InstanceIndex * 0.00000001) away from the max
    /// depth. We will update this after every draw to calculate the
    /// new depth to offset from, so we don't collide with previously drawn
    /// surfaces in a different viewport.
    pub starting_depth: f32,
}

/// Shader push constants
///
/// These will be updated when we record the per-viewport draw commands
/// and will contain the scrolling model transformation of all content
/// within a viewport.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PushConstants {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub width: f32,
    pub height: f32,
    pub starting_depth: f32,
}

// Most of the functions below will be unsafe. Only the safe functions
// should be used by the applications. The unsafe functions are mostly for
// internal use.
impl Renderer {
    /// Returns true if there are any resources in
    /// the current release list.
    pub fn release_is_empty(&mut self) -> bool {
        return self.r_release.is_empty();
    }

    /// Drop all of the resources, this is used to
    /// release wl_buffers after they have been drawn.
    /// We should not deal with wayland structs
    /// directly, just with releaseinfo
    pub fn release_pending_resources(&mut self) {
        log::profiling!("-- releasing pending resources --");

        // This is the previous frames's pending release list
        // We will clear it, therefore dropping all the relinfos
        self.r_release.clear();
    }

    /// Add a ReleaseInfo to the list of resources to be
    /// freed this frame
    ///
    /// Takes care of choosing what list to add info to
    pub fn register_for_release(&mut self, release: Box<dyn Droppable + Send + Sync>) {
        self.r_release.push(release);
    }

    unsafe fn allocate_bindless_resources(
        dev: &Device,
        max_image_count: u32,
    ) -> (vk::DescriptorPool, vk::DescriptorSetLayout) {
        // create the bindless desc set resources
        let size = [
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                // Okay it looks like this must match the layout
                // TODO: should this be changed?
                .descriptor_count(max_image_count)
                .build(),
        ];
        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);
        let bindless_pool = dev.dev.create_descriptor_pool(&info, None).unwrap();

        let bindings = [
            // the window list
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(
                    vk::ShaderStageFlags::COMPUTE
                        | vk::ShaderStageFlags::VERTEX
                        | vk::ShaderStageFlags::FRAGMENT,
                )
                .descriptor_count(1)
                .build(),
            // the variable image list
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(
                    vk::ShaderStageFlags::COMPUTE
                        | vk::ShaderStageFlags::VERTEX
                        | vk::ShaderStageFlags::FRAGMENT,
                )
                // This is the upper bound on the amount of descriptors that
                // can be attached. The amount actually attached will be
                // determined by the amount allocated using this layout.
                .descriptor_count(max_image_count)
                .build(),
        ];
        let mut info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        // We need to attach some binding flags stating that we intend
        // to use the storage image as an unsized array
        let usage_info = vk::DescriptorSetLayoutBindingFlagsCreateInfoEXT::builder()
            .binding_flags(&[
                vk::DescriptorBindingFlags::empty(), // the winlist
                Self::get_bindless_desc_flags(),     // The unbounded array of images
            ])
            .build();
        info.p_next = &usage_info as *const _ as *mut std::ffi::c_void;

        let bindless_layout = dev.dev.create_descriptor_set_layout(&info, None).unwrap();

        (bindless_pool, bindless_layout)
    }

    /// This helper ensures that our window list can hold `capacity` elements
    ///
    /// This will doube the winlist capacity until it fits.
    pub fn ensure_window_capacity(&mut self, capacity: usize) {
        if capacity >= self.r_windows_capacity {
            let mut new_capacity = 0;
            while new_capacity <= self.r_windows_capacity {
                new_capacity += self.r_windows_capacity;
            }

            unsafe {
                self.reallocate_windows_buf_with_cap(new_capacity);
            }
        }
    }

    /// This is a helper for reallocating the vulkan resources of the winlist
    unsafe fn reallocate_windows_buf_with_cap(&mut self, capacity: usize) {
        self.wait_for_prev_submit();

        self.dev.dev.destroy_buffer(self.r_windows_buf, None);
        self.dev.free_memory(self.r_windows_mem);

        // create our data and a storage buffer for the window list
        let (wl_storage, wl_storage_mem) = self.dev.create_buffer_with_size(
            vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::DEVICE_LOCAL
                | vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT,
            (std::mem::size_of::<Window>() * capacity) as u64 + WINDOW_LIST_GLSL_OFFSET as u64,
        );
        self.dev
            .dev
            .bind_buffer_memory(wl_storage, wl_storage_mem, 0)
            .unwrap();
        self.r_windows_buf = wl_storage;
        self.r_windows_mem = wl_storage_mem;
        self.r_windows_capacity = capacity;
    }

    /// Create a new Vulkan Renderer
    ///
    /// This renderer is very application specific. It is not meant to be
    /// a generic safe wrapper for vulkan. This method constructs a new context,
    /// creating a vulkan instance, finding a physical gpu, setting up a logical
    /// device, and creating a swapchain.
    ///
    /// All methods called after this only need to take a mutable reference to
    /// self, avoiding any nasty argument lists like the functions above.
    /// The goal is to have this make dealing with the api less wordy.
    pub fn new(
        instance: Arc<Instance>,
        dev: Arc<Device>,
        info: &CreateInfo,
        ecs: &mut ll::Instance,
        mut img_ecs: ll::Instance,
        pass_comp: ll::Component<usize>,
    ) -> Result<(Renderer, Display)> {
        unsafe {
            // Our display is in charge of choosing a medium to draw on,
            // and will create a surface on that medium
            let display = Display::new(info, dev.clone())?;

            let sampler = dev.create_sampler();

            // TODO:
            // We need to handle the case where the ISV doesn't support
            // a large enough number of bound samplers. In that case, I guess
            // we need to do multiple instanced draw calls of the largest
            // size supported. This will only be doable with geom I guess
            // On moltenvk this is like 128, so that's bad
            let (bindless_pool, bindless_layout) =
                // Subtract three resources from the theoretical max that the driver reported.
                // This is to account for our null image and other resources we create in
                // addition to our bindless count.
                Self::allocate_bindless_resources(&dev, dev.dev_features.max_sampler_count - 3);
            let bindless_desc =
                Self::allocate_bindless_desc(&dev, bindless_pool, &[bindless_layout], 1);

            let (tmp, tmp_view, tmp_mem) = dev.create_image(
                &vk::Extent2D {
                    width: 2,
                    height: 2,
                },
                display.d_state.d_surface_format.format,
                vk::ImageUsageFlags::SAMPLED,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::ImageTiling::LINEAR,
            );

            // Allocate our window order desc layout
            let bindings = [
                // the window order list
                vk::DescriptorSetLayoutBinding::builder()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .stage_flags(
                        vk::ShaderStageFlags::COMPUTE
                            | vk::ShaderStageFlags::VERTEX
                            | vk::ShaderStageFlags::FRAGMENT,
                    )
                    .descriptor_count(1)
                    .build(),
            ];
            let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
            let order_layout = dev.dev.create_descriptor_set_layout(&info, None).unwrap();

            // Create the window list component
            let win_comp = ecs.add_component();

            // Create the image vk info component
            // We have deleted this image, but it's invalid to pass a
            // NULL VkImageView as a descriptor. Instead we will populate
            // it with our "null"/"tmp" image, which is just a black square
            let null_sampler = sampler;
            let null_view = tmp_view;
            let img_info_comp = img_ecs.add_non_sparse_component(move || {
                vk::DescriptorImageInfo::builder()
                    .sampler(null_sampler)
                    .image_view(null_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .build()
            });

            // Add our null image
            let null_image = img_ecs.add_entity();
            img_info_comp.set(
                &null_image,
                vk::DescriptorImageInfo::builder()
                    .sampler(sampler)
                    .image_view(tmp_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .build(),
            );

            // you are now the proud owner of a half complete
            // rendering context
            // p.s. you still need a Pipeline
            let mut rend = Renderer {
                _inst: instance,
                dev: dev,
                image_sampler: sampler,
                r_release: Vec::new(),
                r_ecs: ecs.clone(),
                r_images_desc_pool: bindless_pool,
                r_images_desc_layout: bindless_layout,
                r_images_desc: bindless_desc,
                r_images_desc_size: 0,
                r_windows: win_comp,
                r_windows_buf: vk::Buffer::null(),
                r_windows_mem: vk::DeviceMemory::null(),
                r_windows_capacity: 8,
                r_order_desc_layout: order_layout,
                tmp_image: tmp,
                tmp_image_view: tmp_view,
                tmp_image_mem: tmp_mem,
                _r_null_image: null_image,
                r_image_ecs: img_ecs,
                r_image_infos: img_info_comp,
                r_surface_pass: pass_comp,
            };
            rend.reallocate_windows_buf_with_cap(rend.r_windows_capacity);

            return Ok((rend, display));
        }
    }

    /// Recursively update the shader window parameters for surf
    ///
    /// This is used to push all CPU-side thundr data to the GPU for the shader
    /// to ork with. The offset is used through this to calculate the position of
    /// the subsurfaces relative to their parent.
    /// The flush argument forces the surface's data to be written back.
    fn update_window_list_recurse(
        &mut self,
        list: &mut SurfaceList,
        mut surf: Surface,
        offset: (i32, i32),
        flush: bool,
    ) {
        {
            // Only draw this surface if it has contents defined. Either
            // an image or a color
            //
            // Add this surface before its children, since we need to draw it
            // first so that any alpha in the children will see this underneath
            let internal = surf.s_internal.read().unwrap();
            if internal.s_image.is_some() || internal.s_color.is_some() {
                list.push_raw_order(self, &surf.get_ecs_id());
            }
        }

        if surf.modified() || flush {
            self.update_surf_shader_window(&surf, offset);
            surf.set_modified(false);
        }

        let surf_off = surf.get_pos();
        for i in 0..surf.get_subsurface_count() {
            let child = surf.get_subsurface(i);

            self.update_window_list_recurse(
                list,
                child,
                (offset.0 + surf_off.0 as i32, offset.1 + surf_off.1 as i32),
                // If the parent surface was moved, then we need to update all
                // children, since their positions are out of date.
                surf.modified() | flush,
            );
        }
    }

    /// Extract information for shaders from a surface list
    ///
    /// This includes dimensions, the image bound, etc.
    fn update_window_list(&mut self, surfaces: &mut SurfaceList) {
        surfaces.clear_order_buf();

        for i in (0..surfaces.len()).rev() {
            let s = surfaces[i as usize].clone();
            self.update_window_list_recurse(surfaces, s, (0, 0), false);
        }
    }

    /// Write our Thundr Surface's data to the window list we will pass to the shader
    ///
    /// The shader needs a contiguous list of surfaces, so we turn our surfaces
    /// into a bunch of "windows". These windows will have their size and offset
    /// populated, along with any other drawing data. These live in r_windows, and
    /// the order is set by the surfacelist's l_window_order.
    ///
    /// The offset parameter comes from the offset of this window due to its
    /// surface being a subsurface.
    fn update_surf_shader_window(&mut self, surf_rc: &Surface, offset: (i32, i32)) {
        // Our iterator is going to take into account the dimensions of the
        // parent surface(s), and give us the offset from which we should start
        // doing our calculations. Basically off_x is the parent surfaces X position.
        let surf = surf_rc.s_internal.read().unwrap();
        let opaque_reg = match surf_rc.get_opaque(&self.dev) {
            Some(r) => r,
            // If no opaque data was attached, place a -1 in the start.x component
            // to tell the shader to ignore this
            None => Rect::new(-1, 0, -1, 0),
        };
        let image_id = match surf.s_image.as_ref() {
            Some(i) => i.get_id().get_raw_id() as i32,
            None => -1,
        };

        self.r_windows.set(
            &surf_rc.s_window_id,
            Window {
                w_id: image_id,
                w_use_color: surf.s_color.is_some() as i32,
                w_pass: 0,
                w_padding: 0,
                w_color: match surf.s_color {
                    Some((r, g, b, a)) => (r, g, b, a),
                    // magic value so it's easy to debug
                    // this is clear, since we don't have a color
                    // assigned and we may not have an image bound.
                    // In that case, we want this surface to be clear.
                    None => (0.0, 50.0, 100.0, 0.0),
                },
                w_dims: Rect::new(
                    offset.0 + surf.s_rect.r_pos.0 as i32,
                    offset.1 + surf.s_rect.r_pos.1 as i32,
                    surf.s_rect.r_size.0 as i32,
                    surf.s_rect.r_size.1 as i32,
                ),
                w_opaque: opaque_reg,
            },
        );
    }

    /// Helper for getting the push constants
    ///
    /// This will be where we calculate the viewport scroll amount
    pub fn get_push_constants(
        &mut self,
        params: &RecordParams,
        viewport: &Viewport,
    ) -> PushConstants {
        // transform from blender's coordinate system to vulkan
        PushConstants {
            scroll_x: viewport.scroll_offset.0 as f32,
            scroll_y: viewport.scroll_offset.1 as f32,
            width: viewport.size.0 as f32,
            height: viewport.size.1 as f32,
            starting_depth: params.starting_depth,
        }
    }

    /// Wait for the submit_fence
    ///
    /// This waits for the last frame render operation to finish submitting.
    pub fn wait_for_prev_submit(&self) {
        self.dev.wait_for_copy();

        unsafe {
            // can do read lock here since the fence isn't externally synchronized during
            // this vkWaitForFences call.
            let internal = self.dev.d_internal.read().unwrap();
            match self.dev.dev.wait_for_fences(
                &[internal.submit_fence],
                true,          // wait for all
                std::u64::MAX, //timeout
            ) {
                Ok(_) => {}
                Err(e) => match e {
                    vk::Result::ERROR_DEVICE_LOST => {
                        // If aftermath support is enabled, wait for aftermath
                        // to dump the GPU state
                        #[cfg(feature = "aftermath")]
                        {
                            self.inst.aftermath.wait_for_dump();
                        }
                    }
                    _ => panic!("Could not wait for vulkan fences"),
                },
            };
        }
    }

    pub fn get_recording_parameters(&mut self) -> RecordParams {
        RecordParams {
            // Start at max depth of 1.0 and go to zero
            starting_depth: 0.0,
        }
    }

    /// Start recording a cbuf for one frame
    pub fn begin_recording_one_frame(&mut self) -> Result<RecordParams> {
        // At least wait for any image copies to complete
        self.dev.wait_for_copy();

        Ok(self.get_recording_parameters())
    }

    /// End a total frame recording
    pub fn end_recording_one_frame(&mut self) {}

    /// Descriptor flags for the unbounded array of images
    /// we need to say that it is a variably sized array, and that it is partially
    /// bound (aka we aren't populating the full MAX_IMAGE_LIMIT)
    pub fn get_bindless_desc_flags() -> vk::DescriptorBindingFlags {
        vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT
            | vk::DescriptorBindingFlags::PARTIALLY_BOUND
    }

    fn allocate_bindless_desc(
        dev: &Device,
        pool: vk::DescriptorPool,
        layouts: &[vk::DescriptorSetLayout],
        desc_count: u32,
    ) -> vk::DescriptorSet {
        // if thundr has allocated a different number of images than we were expecting,
        // we need to realloc the variable descriptor memory
        let mut info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts)
            .build();
        let variable_info = vk::DescriptorSetVariableDescriptorCountAllocateInfo::builder()
            // This list specifies the number of allocations for the variable
            // descriptor entry in each layout. We only have one layout.
            .descriptor_counts(&[desc_count])
            .build();

        info.p_next = &variable_info as *const _ as *mut std::ffi::c_void;

        unsafe { dev.dev.allocate_descriptor_sets(&info).unwrap()[0] }
    }

    pub fn refresh_window_resources(&mut self, surfaces: &mut SurfaceList) {
        self.wait_for_prev_submit();

        // Construct a list of image views from the submitted surface list
        // this will be our unsized texture array that the composite shader will reference
        // TODO: make this a changed flag
        if self.r_images_desc_size < self.r_image_ecs.capacity() {
            // free the previous descriptor sets
            unsafe {
                self.dev
                    .dev
                    .reset_descriptor_pool(
                        self.r_images_desc_pool,
                        vk::DescriptorPoolResetFlags::empty(),
                    )
                    .unwrap();
            }

            self.r_images_desc_size = self.r_image_ecs.capacity();
            self.r_images_desc = Self::allocate_bindless_desc(
                &self.dev,
                self.r_images_desc_pool,
                &[self.r_images_desc_layout],
                self.r_images_desc_size as u32,
            );
        }

        // Now that we have possibly reallocated the descriptor sets,
        // refresh the window list to put it back in gpu mem
        self.refresh_window_list(surfaces);

        // Now write the new bindless descriptor
        let write_infos = &[
            vk::WriteDescriptorSet::builder()
                .dst_set(self.r_images_desc)
                .dst_binding(0)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&[vk::DescriptorBufferInfo::builder()
                    .buffer(self.r_windows_buf)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build()])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.r_images_desc)
                .dst_binding(1)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(self.r_image_infos.get_data_slice().data())
                .build(),
        ];
        log::info!(
            "Raw image infos is {:#?}",
            self.r_image_infos.get_data_slice().data()
        );

        unsafe {
            self.dev.dev.update_descriptor_sets(
                write_infos, // descriptor writes
                &[],         // descriptor copies
            );
        }

        // We also need to tell the surface list to update its window
        // order resource
        surfaces.allocate_order_desc(self);
    }

    /// This refreshes the renderer's internal variable size window
    /// list that will be used as part of the bindless shader code.
    pub fn refresh_window_list(&mut self, surfaces: &mut SurfaceList) {
        // Only do this if the surface list has changed and the shader needs a new
        // window ordering
        // The surfacelist ordering didn't change, but the individual
        // surfaces might have. We need to copy the new values for
        // any changed
        self.update_window_list(surfaces);
        let num_entities = self.r_ecs.capacity();
        self.ensure_window_capacity(num_entities);

        surfaces.update_window_order_buf(self);

        // TODO: don't even use CPU copies of the datastructs and perform
        // the tile/window updates in the mapped GPU memory
        // (requires benchmark)
        // Don't update vulkan memory unless we have more than one valid id.
        if self.r_ecs.num_entities() > 0 && num_entities > 0 {
            // Shader expects struct WindowList { int count; Window windows[] }
            self.dev
                .update_memory(self.r_windows_mem, 0, &[num_entities]);
            self.dev.update_memory_from_callback(
                self.r_windows_mem,
                WINDOW_LIST_GLSL_OFFSET,
                num_entities,
                |dst| {
                    // For each valid window entry, extract the Window
                    // type from the option so that we can write it to
                    // the Vulkan memory
                    for p in surfaces.l_pass.iter() {
                        if let Some(pass) = p {
                            for id in pass.p_window_order.iter() {
                                let i = id.get_raw_id();
                                let win = self.r_windows.get(&id).unwrap();
                                log::debug!("Winlist index {}: writing window {:?}", i, *win);
                                dst[i] = *win;
                            }
                        }
                    }
                },
            );
        }
    }

    /// Returns true if we are ready to call present
    pub fn frame_submission_complete(&mut self) -> bool {
        // can do read lock here since the fence isn't externally synchronized
        let internal = self.dev.d_internal.read().unwrap();
        match unsafe { self.dev.dev.get_fence_status(internal.submit_fence) } {
            // true means vk::Result::SUCCESS
            // false means vk::Result::NOT_READY
            Ok(complete) => return complete,
            Err(_) => panic!("Failed to get fence status"),
        };
    }
}

// Clean up after ourselves when the renderer gets destroyed.
//
// This is pretty straightforward, things are destroyed in roughly
// the reverse order that they were created in. Don't forget to add
// new fields of Renderer here if needed.
//
// Could probably use some error checking, but if this gets called we
// are exiting anyway.
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            log::profiling!("Stoping the renderer");

            // first wait for the device to finish working
            self.dev.dev.device_wait_idle().unwrap();

            self.dev.dev.destroy_image(self.tmp_image, None);
            self.dev.dev.destroy_image_view(self.tmp_image_view, None);
            self.dev.free_memory(self.tmp_image_mem);

            self.dev.dev.destroy_sampler(self.image_sampler, None);
            self.dev
                .dev
                .destroy_descriptor_set_layout(self.r_images_desc_layout, None);

            self.dev
                .dev
                .destroy_descriptor_set_layout(self.r_order_desc_layout, None);
            self.dev
                .dev
                .destroy_descriptor_pool(self.r_images_desc_pool, None);
            self.dev.dev.destroy_buffer(self.r_windows_buf, None);
            self.dev.free_memory(self.r_windows_mem);
        }
    }
}
