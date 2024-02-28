// Vulkan device representation
//
// This stores per-GPU state, such as the Vulkan Device objects and
// logic to copy data to and from this GPU.
//
// Austin Shafer - 2024

use ash::vk;

extern crate utils as cat5_utils;
use crate::instance::Instance;
use crate::platform::VKDeviceFeatures;
use crate::{CreateInfo, Damage, Result, ThundrError};
use cat5_utils::log;

use std::sync::{Arc, RwLock};

/// Thundr Device
///
/// This holds all of the Vulkan logic for one GPU.
pub struct Device {
    pub(crate) inst: Arc<Instance>,
    /// the logical device we are using
    pub(crate) dev: ash::Device,
    /// Details about what this device supports
    pub(crate) dev_features: VKDeviceFeatures,
    /// the physical device selected to display to
    pub(crate) pdev: vk::PhysicalDevice,
    pub(crate) mem_props: vk::PhysicalDeviceMemoryProperties,

    d_internal: Arc<RwLock<DeviceInternal>>,
}

/// This is the set of per-device data that needs to be "externally synchronized"
/// according to Vulkan.
pub struct DeviceInternal {
    /// queue for copy operations
    pub(crate) transfer_queue: vk::Queue,

    pub(crate) copy_cmd_pool: vk::CommandPool,
    /// command buffer for copying shm images
    pub(crate) copy_cbuf: vk::CommandBuffer,
    pub(crate) copy_cbuf_fence: vk::Fence,

    /// These are for loading textures into images
    pub(crate) transfer_buf_len: usize,
    pub(crate) transfer_buf: vk::Buffer,
    pub(crate) transfer_mem: vk::DeviceMemory,
}

impl Device {
    /// Create a vkDevice from a vkPhysicalDevice
    ///
    /// Create a logical device for interfacing with the physical device.
    /// once again we specify any device extensions we need, the swapchain
    /// being the most important one.
    ///
    /// A queue is created in the specified queue family in the
    /// present_queue argument.
    fn create_device(
        dev_features: &VKDeviceFeatures,
        inst: &ash::Instance,
        pdev: vk::PhysicalDevice,
        queues: &[u32],
    ) -> ash::Device {
        let dev_extension_names = dev_features.get_device_extensions();

        let features = vk::PhysicalDeviceFeatures::builder()
            .shader_clip_distance(true)
            .vertex_pipeline_stores_and_atomics(true)
            .fragment_stores_and_atomics(true)
            .build();

        // for now we only have one graphics queue, so one priority
        let priorities = [1.0];
        let mut queue_infos = Vec::new();
        for i in queues {
            queue_infos.push(
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(*i)
                    .queue_priorities(&priorities)
                    .build(),
            );
        }

        let mut dev_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(queue_infos.as_ref())
            .enabled_extension_names(dev_extension_names.as_slice())
            .enabled_features(&features)
            .build();

        if dev_features.vkc_supports_desc_indexing {
            let indexing_info = vk::PhysicalDeviceDescriptorIndexingFeaturesEXT::builder()
                .shader_sampled_image_array_non_uniform_indexing(true)
                .runtime_descriptor_array(true)
                .descriptor_binding_variable_descriptor_count(true)
                .descriptor_binding_partially_bound(true)
                .descriptor_binding_update_unused_while_pending(true)
                .build();

            dev_create_info.p_next = &indexing_info as *const _ as *mut std::ffi::c_void;
        }

        #[cfg(feature = "aftermath")]
        {
            let mut aftermath_info = vk::DeviceDiagnosticsConfigCreateInfoNV::builder()
                .flags(
                    vk::DeviceDiagnosticsConfigFlagsNV::ENABLE_SHADER_DEBUG_INFO
                        | vk::DeviceDiagnosticsConfigFlagsNV::ENABLE_RESOURCE_TRACKING,
                )
                .build();
            aftermath_info.p_next = dev_create_info.p_next;

            dev_create_info.p_next = &aftermath_info as *const _ as *mut std::ffi::c_void;
            // do our call here so aftermath_info is still in scope
            return inst.create_device(pdev, &dev_create_info, None).unwrap();
        }

        // return a newly created device
        unsafe { inst.create_device(pdev, &dev_create_info, None).unwrap() }
    }

    /// Get the major/minor of the DRM node in use
    ///
    /// This uses VK_EXT_physical_device_drm, and will fail an assert
    /// if it is not in use.
    ///
    /// return is drm (renderMajor, renderMinor).
    pub fn get_drm_dev(&self) -> (i64, i64) {
        if !self.dev_features.vkc_supports_phys_dev_drm {
            log::error!("Using drm Vulkan extensions but the underlying vulkan library doesn't support them. This will cause problems");
        }
        let mut drm_info = vk::PhysicalDeviceDrmPropertiesEXT::builder().build();

        let mut info = vk::PhysicalDeviceProperties2::builder().build();
        info.p_next = &mut drm_info as *mut _ as *mut std::ffi::c_void;

        unsafe {
            self.inst
                .inst
                .get_physical_device_properties2(self.pdev, &mut info)
        };
        assert!(drm_info.has_render != 0);

        (drm_info.render_major, drm_info.render_minor)
    }

    /// get the vkPhysicalDeviceMemoryProperties structure for a vkPhysicalDevice
    pub(crate) fn get_pdev_mem_properties(
        inst: &ash::Instance,
        pdev: vk::PhysicalDevice,
    ) -> vk::PhysicalDeviceMemoryProperties {
        unsafe { inst.get_physical_device_memory_properties(pdev) }
    }

    /// Choose a queue family
    ///
    /// returns an index into the array of queue types.
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface
    fn select_queue_family(
        inst: &ash::Instance,
        pdev: vk::PhysicalDevice,
        flags: vk::QueueFlags,
    ) -> u32 {
        // get the properties per queue family
        unsafe { inst.get_physical_device_queue_family_properties(pdev) }
            // for each property info
            .iter()
            .enumerate()
            .filter_map(|(index, info)| {
                // add the device and the family to a list of
                // candidates for use later
                match info.queue_flags.contains(flags) {
                    // return the pdevice/family pair
                    true => Some(index as u32),
                    false => None,
                }
            })
            .nth(0)
            .expect("Could not find a suitable queue family")
    }

    /// Choose a vkPhysicalDevice and queue family index.
    ///
    /// selects a physical device and a queue family
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface.
    pub(crate) fn select_pdev(inst: &ash::Instance) -> vk::PhysicalDevice {
        let pdevices = unsafe {
            inst.enumerate_physical_devices()
                .expect("Physical device error")
        };

        // for each physical device
        *pdevices
            .iter()
            // eventually there needs to be a way of grabbing
            // the configured pdev from the user
            .nth(0)
            // for now we are just going to get the first one
            .expect("Couldn't find suitable device.")
    }

    /// Create a new default Device
    ///
    /// This creates a new device for the default chosen physical device
    /// in the Instance.
    pub fn new(instance: Arc<Instance>, info: &CreateInfo) -> Result<Self> {
        let pdev = Self::select_pdev(&instance.inst);

        let transfer_queue_family =
            Self::select_queue_family(&instance.inst, pdev, vk::QueueFlags::TRANSFER);
        let mem_props = Self::get_pdev_mem_properties(&instance.inst, pdev);

        let dev_features = VKDeviceFeatures::new(&info, &instance.inst, pdev);
        if !dev_features.vkc_supports_desc_indexing {
            return Err(ThundrError::VK_NOT_ALL_EXTENSIONS_AVAILABLE);
        }
        let dev = Self::create_device(
            &dev_features,
            &instance.inst,
            pdev,
            &[transfer_queue_family],
        );

        let transfer_queue = unsafe { dev.get_device_queue(transfer_queue_family, 0) };

        // Make a fence which will be signalled after
        // copies are completed
        let copy_fence = unsafe {
            dev.create_fence(
                &vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
            .expect("Could not create fence")
        };

        let ret = Self {
            inst: instance,
            dev: dev,
            dev_features: dev_features,
            pdev: pdev,
            mem_props: mem_props,
            d_internal: Arc::new(RwLock::new(DeviceInternal {
                copy_cmd_pool: vk::CommandPool::null(),
                copy_cbuf: vk::CommandBuffer::null(),
                copy_cbuf_fence: copy_fence,
                transfer_queue: transfer_queue,
                transfer_buf: vk::Buffer::null(), // Initialize in its own method
                transfer_mem: vk::DeviceMemory::null(),
                transfer_buf_len: 0,
            })),
        };

        {
            let copy_cmd_pool = ret.create_command_pool(transfer_queue_family);
            let copy_cbuf = ret.create_command_buffers(copy_cmd_pool, 1)[0];

            let mut internal = ret.d_internal.write().unwrap();
            internal.copy_cmd_pool = copy_cmd_pool;
            internal.copy_cbuf = copy_cbuf;
        }

        Ok(ret)
    }

    /// returns a new vkCommandPool
    ///
    /// Command buffers are allocated from command pools. That's about
    /// all they do. They just manage memory. Command buffers will be allocated
    /// as part of the queue_family specified.
    pub(crate) fn create_command_pool(&self, queue_family: u32) -> vk::CommandPool {
        let pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);

        unsafe {
            self.dev
                .create_command_pool(&pool_create_info, None)
                .unwrap()
        }
    }

    /// Allocate a vec of vkCommandBuffers
    ///
    /// Command buffers are constructed once, and can be executed
    /// many times. They also have the added bonus of being added to
    /// by multiple threads. Command buffer is shortened to `cbuf` in
    /// many areas of the code.
    ///
    /// For now we are only allocating two: one to set up the resources
    /// and one to do all the work.
    pub(crate) fn create_command_buffers(
        &self,
        pool: vk::CommandPool,
        count: u32,
    ) -> Vec<vk::CommandBuffer> {
        let cbuf_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(count)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY);

        unsafe {
            self.dev
                .allocate_command_buffers(&cbuf_allocate_info)
                .unwrap()
        }
    }

    /// Create an image sampler for the swapchain fbs
    ///
    /// Samplers are used to filter data from an image when
    /// it is referenced from a fragment shader. It allows
    /// for additional processing effects on the input.
    pub(crate) fn create_sampler(&self) -> vk::Sampler {
        let info = vk::SamplerCreateInfo::builder()
            // filter for magnified (oversampled) pixels
            .mag_filter(vk::Filter::LINEAR)
            // filter for minified (undersampled) pixels
            .min_filter(vk::Filter::LINEAR)
            // don't repeat the texture on wraparound
            // There is some weird thing where one/two pixels on each border
            // will repeat, which makes text rendering borked. Idk why this
            // is the case, but given that it only affects the very edges just
            // turn off repeat since we will never be doing it anyway)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            // disable this for performance
            .anisotropy_enable(false)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            // texture coords are [0,1)
            .unnormalized_coordinates(false)
            .compare_enable(false)
            .compare_op(vk::CompareOp::ALWAYS)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR);

        unsafe { self.dev.create_sampler(&info, None).unwrap() }
    }

    /// Wait for the previous copy operation to complete
    ///
    /// If no copy operation is in flight this returns immediately.
    pub fn wait_for_copy(&self) {
        let internal = self.d_internal.write().unwrap();
        unsafe {
            self.dev
                .wait_for_fences(
                    &[internal.copy_cbuf_fence],
                    true,          // wait for all
                    std::u64::MAX, //timeout
                )
                .expect("Could not wait for the copy fence");
        }
    }

    /// Load a memory region into our staging area
    fn upload_memimage_to_transfer(&self, data: &[u8]) {
        unsafe {
            // We might be in the middle of copying the transfer buf to an image
            // wait for that if its the case
            self.wait_for_copy();
            let mut internal = self.d_internal.write().unwrap();
            if data.len() > internal.transfer_buf_len {
                let (buffer, buf_mem) = self.create_buffer(
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    vk::SharingMode::EXCLUSIVE,
                    vk::MemoryPropertyFlags::HOST_VISIBLE
                        | vk::MemoryPropertyFlags::HOST_COHERENT
                        | vk::MemoryPropertyFlags::HOST_CACHED,
                    data,
                );

                self.dev.free_memory(internal.transfer_mem, None);
                self.dev.destroy_buffer(internal.transfer_buf, None);
                internal.transfer_buf = buffer;
                internal.transfer_mem = buf_mem;
                internal.transfer_buf_len = data.len();
            } else {
                // copy the data into the staging buffer
                self.update_memory(internal.transfer_mem, 0, data);
            }
        }
    }

    /// Wrapper for freeing device memory
    ///
    /// Having this in one place lets us quickly handle any additional
    /// allocation tracking
    pub(crate) unsafe fn free_memory(&self, mem: vk::DeviceMemory) {
        self.dev.free_memory(mem, None);
    }

    /// Allocates a buffer/memory pair of size `size`.
    ///
    /// This is just a helper for `create_buffer`. It does not fill
    /// the buffer with anything.
    pub(crate) fn create_buffer_with_size(
        &self,
        usage: vk::BufferUsageFlags,
        mode: vk::SharingMode,
        flags: vk::MemoryPropertyFlags,
        size: u64,
    ) -> (vk::Buffer, vk::DeviceMemory) {
        let create_info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(usage)
            .sharing_mode(mode);

        let buffer = unsafe { self.dev.create_buffer(&create_info, None).unwrap() };
        let req = unsafe { self.dev.get_buffer_memory_requirements(buffer) };
        // find the memory type that best suits our requirements
        let index = Self::find_memory_type_index(&self.mem_props, &req, flags).unwrap();

        // now we need to allocate memory to back the buffer
        let alloc_info = vk::MemoryAllocateInfo {
            allocation_size: req.size,
            memory_type_index: index,
            ..Default::default()
        };

        let memory = unsafe { self.dev.allocate_memory(&alloc_info, None).unwrap() };

        return (buffer, memory);
    }

    /// Writes `data` to `memory`
    ///
    /// This is a helper method for mapping and updating the value stored
    /// in device memory Memory needs to be host visible and coherent.
    /// This does not flush after writing.
    pub(crate) fn update_memory<T: Copy>(
        &self,
        memory: vk::DeviceMemory,
        offset: isize,
        data: &[T],
    ) {
        if data.len() == 0 {
            return;
        }

        // Now we copy our data into the buffer
        let data_size = std::mem::size_of_val(data) as u64;
        unsafe {
            let ptr = self
                .dev
                .map_memory(
                    memory,
                    offset as u64, // offset
                    data_size,
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap();

            // rust doesn't have a raw memcpy, so we need to transform the void
            // ptr to a slice. This is unsafe as the length needs to be correct
            let dst = std::slice::from_raw_parts_mut(ptr as *mut T, data.len());
            dst.copy_from_slice(data);

            self.dev.unmap_memory(memory);
        }
    }

    /// Update our memory, calling `callback` to provide the caller an opportunity
    /// to fill in the data.
    pub(crate) fn update_memory_from_callback<T: Copy, F: FnOnce(&mut [T])>(
        &self,
        memory: vk::DeviceMemory,
        offset: isize,
        len: usize,
        callback: F,
    ) {
        // Now we copy our data into the buffer
        let data_size = std::mem::size_of::<T>();
        unsafe {
            let ptr = self
                .dev
                .map_memory(
                    memory,
                    offset as u64, // offset
                    (data_size * len) as u64,
                    vk::MemoryMapFlags::empty(),
                )
                .unwrap();

            // rust doesn't have a raw memcpy, so we need to transform the void
            // ptr to a slice. This is unsafe as the length needs to be correct
            let dst = std::slice::from_raw_parts_mut(ptr as *mut T, len);

            callback(dst);

            self.dev.unmap_memory(memory)
        };
    }

    /// allocates a buffer/memory pair and fills it with `data`
    ///
    /// There are two components to a memory backed resource in vulkan:
    /// vkBuffer which is the actual buffer itself, and vkDeviceMemory which
    /// represents a region of allocated memory to hold the buffer contents.
    ///
    /// Both are returned, as both need to be destroyed when they are done.
    pub(crate) fn create_buffer<T: Copy>(
        &self,
        usage: vk::BufferUsageFlags,
        mode: vk::SharingMode,
        flags: vk::MemoryPropertyFlags,
        data: &[T],
    ) -> (vk::Buffer, vk::DeviceMemory) {
        let size = std::mem::size_of_val(data) as u64;
        let (buffer, memory) = self.create_buffer_with_size(usage, mode, flags, size);

        self.update_memory(memory, 0, data);

        // Until now the buffer has not had any memory assigned
        unsafe { self.dev.bind_buffer_memory(buffer, memory, 0).unwrap() };

        (buffer, memory)
    }

    /// Records and submits a one-time command buffer.
    ///
    /// cbuf - the command buffer to use
    /// queue - the queue to submit cbuf to
    /// wait_stages - a list of pipeline stages to wait on
    /// wait_semas - semaphores we consume
    /// signal_semas - semaphores we notify
    ///
    /// All operations in the `record_fn` argument will be
    /// submitted in the command buffer `cbuf`. This aims to make
    /// constructing buffers more ergonomic.
    pub(crate) fn cbuf_submit_and_wait(
        &self,
        cbuf: vk::CommandBuffer,
        queue: vk::Queue,
        wait_stages: &[vk::PipelineStageFlags],
        wait_semas: &[vk::Semaphore],
        signal_semas: &[vk::Semaphore],
    ) {
        self.cbuf_end_recording(cbuf);

        // once the one-time buffer has been recorded we can submit
        // it for execution.
        // Interesting: putting the cbuf into a list in the builder
        // struct makes it segfault in release mode... Deep dive
        // needed...
        let cbufs = [cbuf];
        let submit_info = vk::SubmitInfo::builder()
            .wait_semaphores(wait_semas)
            .wait_dst_stage_mask(wait_stages)
            .command_buffers(&cbufs)
            .signal_semaphores(signal_semas)
            .build();

        unsafe {
            let fence = self
                .dev
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .expect("Could not create fence");

            // create a fence to be notified when the commands have finished
            // executing. Wait immediately for the fence.
            self.dev
                .queue_submit(queue, &[submit_info], fence)
                .expect("Could not submit buffer to queue");

            self.dev
                .wait_for_fences(
                    &[fence],
                    true,          // wait for all
                    std::u64::MAX, //timeout
                )
                .expect("Could not wait for the submit fence");
            // the commands are now executed
            self.dev.destroy_fence(fence, None);
        }
    }

    /// Submits a command buffer asynchronously.
    ///
    /// Simple wrapper for queue submission. Does not
    /// wait for anything.
    ///
    /// The buffer MUST have been recorded before this
    ///
    /// cbuf - the command buffer to use
    /// queue - the queue to submit cbuf to
    /// wait_stages - a list of pipeline stages to wait on
    /// wait_semas - semaphores we consume
    /// signal_semas - semaphores we notify
    pub(crate) fn cbuf_submit_async(
        &self,
        cbuf: vk::CommandBuffer,
        queue: vk::Queue,
        wait_stages: &[vk::PipelineStageFlags],
        wait_semas: &[vk::Semaphore],
        signal_semas: &[vk::Semaphore],
        signal_fence: vk::Fence,
    ) {
        // The buffer must have been recorded before we can submit
        // it for execution.
        let submits = [vk::SubmitInfo::builder()
            .wait_semaphores(wait_semas)
            .wait_dst_stage_mask(wait_stages)
            .command_buffers(&[cbuf])
            .signal_semaphores(signal_semas)
            .build()];

        // create a fence to be notified when the commands have finished
        // executing.
        unsafe {
            self.dev
                .queue_submit(queue, &submits, signal_fence)
                .unwrap();
        }
    }

    /// Records but does not submit a command buffer.
    ///
    /// cbuf - the command buffer to use
    /// flags - the usage flags for the buffer
    ///
    /// All operations in the `record_fn` argument will be
    /// recorded in the command buffer `cbuf`.
    pub(crate) fn cbuf_begin_recording(
        &self,
        cbuf: vk::CommandBuffer,
        flags: vk::CommandBufferUsageFlags,
    ) {
        unsafe {
            // first reset the queue so we know it is empty
            self.dev
                .reset_command_buffer(cbuf, vk::CommandBufferResetFlags::RELEASE_RESOURCES)
                .expect("Could not reset command buffer");

            // this cbuf will only be used once, so tell vulkan that
            // so it can optimize accordingly
            let record_info = vk::CommandBufferBeginInfo::builder().flags(flags);

            // start recording the command buffer, call the function
            // passed to load it with operations, and then end the
            // command buffer
            self.dev
                .begin_command_buffer(cbuf, &record_info)
                .expect("Could not start command buffer");
        }
    }

    /// Records but does not submit a command buffer.
    ///
    /// cbuf - the command buffer to use
    pub(crate) fn cbuf_end_recording(&self, cbuf: vk::CommandBuffer) {
        unsafe {
            self.dev
                .end_command_buffer(cbuf)
                .expect("Could not end command buffer");
        }
    }

    /// Transitions `image` to the `new` layout using `cbuf`
    ///
    /// Images need to be manually transitioned from two layouts. A
    /// normal use case is transitioning an image from an undefined
    /// layout to the optimal shader access layout. This is also
    /// used  by depth images.
    ///
    /// It is assumed this is for textures referenced from the fragment
    /// shader, and so it is a bit specific.
    pub unsafe fn transition_image_layout(
        dev: &ash::Device,
        image: vk::Image,
        cbuf: vk::CommandBuffer,
        old: vk::ImageLayout,
        new: vk::ImageLayout,
    ) {
        // use defaults here, and set them in the next section
        let mut layout_barrier = vk::ImageMemoryBarrier::builder()
            .image(image)
            .src_access_mask(vk::AccessFlags::TRANSFER_READ)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            // go from an undefined old layout to whatever the
            // driver decides is the optimal depth layout
            .old_layout(old)
            .new_layout(new)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1)
                    .level_count(1)
                    .build(),
            )
            .build();
        #[allow(unused_assignments)]
        let mut src_stage = vk::PipelineStageFlags::TOP_OF_PIPE;
        #[allow(unused_assignments)]
        let mut dst_stage = vk::PipelineStageFlags::TOP_OF_PIPE;

        // automatically detect the pipeline src/dest stages to use.
        // straight from `transitionImageLayout` in the tutorial.
        if old == vk::ImageLayout::UNDEFINED {
            layout_barrier.src_access_mask = vk::AccessFlags::default();
            layout_barrier.dst_access_mask = vk::AccessFlags::TRANSFER_WRITE;

            src_stage = vk::PipelineStageFlags::TOP_OF_PIPE;
            dst_stage = vk::PipelineStageFlags::TRANSFER;
        } else {
            layout_barrier.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
            layout_barrier.dst_access_mask = vk::AccessFlags::SHADER_READ;

            src_stage = vk::PipelineStageFlags::TRANSFER;
            dst_stage = vk::PipelineStageFlags::FRAGMENT_SHADER;
        }

        // process the barrier we created, which will perform
        // the actual transition.
        dev.cmd_pipeline_barrier(
            cbuf,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[layout_barrier],
        );
    }

    /// Update a Vulkan image from a raw memory region
    ///
    /// This will upload the MemImage to the tansfer buffer, copy it to the image,
    /// and perform any needed layout conversions along the way.
    ///
    /// A stride of zero implies the data is tightly packed.
    pub(crate) fn update_image_from_data(
        &self,
        image: vk::Image,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
    ) {
        self.update_image_contents_from_damaged_data(image, data, width, height, stride, None);
    }

    /// Copies a list of regions from a buffer into an image.
    ///
    /// Instead of copying the entire buffer, use a thundr::Damage to
    /// populate only certain parts of the image. `damage` takes place
    /// in the image's coordinate system.
    pub(crate) fn update_image_contents_from_damaged_data(
        &self,
        image: vk::Image,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        damage: Option<Damage>,
    ) {
        log::debug!("Updating image with damage: {:?}", damage);
        log::debug!("Using {}x{} buffer with stride {}", width, height, stride);

        // Adjust our stride. If the special value zero is specified then we
        // should default to tighly packed, aka the width
        let stride = match stride {
            0 => width,
            s => s,
        };

        // If we have damage to use, then generate our copy regions. If not,
        // then just create
        let mut regions = Vec::new();
        if let Some(damage) = damage {
            for d in damage.d_regions.iter() {
                regions.push(
                    vk::BufferImageCopy::builder()
                        .buffer_offset((stride as i32 * d.r_pos.1 + d.r_pos.0) as u64 * 4)
                        .buffer_row_length(stride)
                        // 0 specifies that the pixels are tightly packed
                        .buffer_image_height(0)
                        .image_subresource(
                            vk::ImageSubresourceLayers::builder()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .mip_level(0)
                                .base_array_layer(0)
                                .layer_count(1)
                                .build(),
                        )
                        .image_offset(vk::Offset3D {
                            x: d.r_pos.0,
                            y: d.r_pos.1,
                            z: 0,
                        })
                        .image_extent(vk::Extent3D {
                            width: d.r_size.0 as u32,
                            height: d.r_size.1 as u32,
                            depth: 1,
                        })
                        .build(),
                );
            }
        } else {
            regions.push(
                vk::BufferImageCopy::builder()
                    .buffer_offset(0)
                    // 0 means tightly packed.
                    .buffer_row_length(stride)
                    .buffer_image_height(0)
                    .image_subresource(
                        vk::ImageSubresourceLayers::builder()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .mip_level(0)
                            .base_array_layer(0)
                            .layer_count(1)
                            .build(),
                    )
                    .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                    .image_extent(vk::Extent3D {
                        width: width,
                        height: height,
                        depth: 1,
                    })
                    .build(),
            );
        }

        // Now copy the bits into the image
        // TODO: only upload damaged regions
        self.upload_memimage_to_transfer(data);

        let int_lock = self.d_internal.clone();
        let internal = int_lock.write().unwrap();
        unsafe {
            // Reset the fences for our cbuf submission below
            self.dev.reset_fences(&[internal.copy_cbuf_fence]).unwrap();

            // transition us into the appropriate memory layout for shaders
            self.cbuf_begin_recording(
                internal.copy_cbuf,
                vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
            );

            // First thing to do here is to copy the transfer memory into the image
            let layout_barrier = vk::ImageMemoryBarrier::builder()
                .image(image)
                .src_access_mask(vk::AccessFlags::default())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .level_count(1)
                        .build(),
                )
                .build();
            self.dev.cmd_pipeline_barrier(
                internal.copy_cbuf,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[layout_barrier],
            );

            self.dev.cmd_copy_buffer_to_image(
                internal.copy_cbuf,
                internal.transfer_buf,
                image,
                // this is the layout the image is currently using
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                // Region to copy
                regions.as_slice(),
            );

            let layout_barrier = vk::ImageMemoryBarrier::builder()
                .image(image)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .level_count(1)
                        .build(),
                )
                .build();
            self.dev.cmd_pipeline_barrier(
                internal.copy_cbuf,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[layout_barrier],
            );
            self.cbuf_end_recording(internal.copy_cbuf);

            self.cbuf_submit_async(
                internal.copy_cbuf,
                internal.transfer_queue,
                &[], // wait_stages
                &[], // wait_semas
                &[], // signal_semas
                internal.copy_cbuf_fence,
            );
        }
    }

    /// Returns an index into the array of memory types for the memory
    /// properties
    ///
    /// Memory types specify the location and accessability of memory. Device
    /// local memory is resident on the GPU, while host visible memory can be
    /// read from the system side. Both of these are part of the
    /// vk::MemoryPropertyFlags type.
    fn find_memory_type_index(
        props: &vk::PhysicalDeviceMemoryProperties,
        reqs: &vk::MemoryRequirements,
        flags: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        // for each memory type
        for (i, ref mem_type) in props.memory_types.iter().enumerate() {
            // Bit i of memoryBitTypes will be set if the resource supports
            // the ith memory type in props.
            //
            // ash autogenerates common operations for bitfield style structs
            // they can be found in `vk_bitflags_wrapped`
            if (reqs.memory_type_bits >> i) & 1 == 1 && mem_type.property_flags.contains(flags) {
                // log!(LogLevel::profiling, "Selected type with flags {:?}",
                //          mem_type.property_flags);
                // return the index into the memory type array
                return Some(i as u32);
            }
        }
        None
    }

    /// Acquire a dmabuf VkImage
    ///
    /// In order to use a dmabuf VkImage we need to transition it to the foreign
    /// queue. This gives the driver a chance to perform any residency operations
    /// or format conversions.
    pub(crate) fn acquire_dmabuf_image_from_external_queue(
        &self,
        image: vk::Image,
        queue_family: u32,
    ) {
        self.wait_for_copy();

        let int_lock = self.d_internal.clone();
        let internal = int_lock.write().unwrap();

        unsafe { self.dev.reset_fences(&[internal.copy_cbuf_fence]).unwrap() };

        // now perform the copy
        self.cbuf_begin_recording(
            internal.copy_cbuf,
            vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        );

        let acquire_barrier = vk::ImageMemoryBarrier::builder()
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(queue_family)
            .image(image)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1)
                    .level_count(1)
                    .build(),
            )
            .build();

        unsafe {
            self.dev.cmd_pipeline_barrier(
                internal.copy_cbuf,
                vk::PipelineStageFlags::TOP_OF_PIPE, // src
                vk::PipelineStageFlags::FRAGMENT_SHADER
                    | vk::PipelineStageFlags::VERTEX_SHADER
                    | vk::PipelineStageFlags::COMPUTE_SHADER, // dst
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[acquire_barrier],
            );
        }

        self.cbuf_end_recording(internal.copy_cbuf);
        self.cbuf_submit_async(
            internal.copy_cbuf,
            internal.transfer_queue,
            &[], // wait_stages
            &[], // wait_semas
            &[], // signal_semas
            internal.copy_cbuf_fence,
        );
    }

    /// Create a vkImage and the resources needed to use it
    ///   (vkImageView and vkDeviceMemory)
    ///
    /// Images are generic buffers which can be used as sources or
    /// destinations of data. Images are accessed through image views,
    /// which specify how the image will be modified or read. In vulkan
    /// memory management is more hands on, so we will allocate some device
    /// memory to back the image.
    ///
    /// This method may require some adjustment as it makes some assumptions
    /// about the type of image to be created.
    ///
    /// Resolution should probably be the same size as the swapchain's images
    /// usage defines the role the image will serve (transfer, depth data, etc)
    /// flags defines the memory type (probably DEVICE_LOCAL + others)
    pub(crate) fn create_image(
        &self,
        resolution: &vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
        flags: vk::MemoryPropertyFlags,
        tiling: vk::ImageTiling,
    ) -> (vk::Image, vk::ImageView, vk::DeviceMemory) {
        // we create the image now, but will have to bind
        // some memory to it later.
        let create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: resolution.width,
                height: resolution.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(tiling)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let image = unsafe { self.dev.create_image(&create_info, None).unwrap() };

        // we need to find a memory type that matches the type our
        // new image needs
        let mem_reqs = unsafe { self.dev.get_image_memory_requirements(image) };
        let memtype_index =
            Self::find_memory_type_index(&self.mem_props, &mem_reqs, flags).unwrap();

        let alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(mem_reqs.size)
            .memory_type_index(memtype_index);

        let image_memory = unsafe { self.dev.allocate_memory(&alloc_info, None).unwrap() };
        unsafe {
            self.dev
                .bind_image_memory(image, image_memory, 0)
                .expect("Unable to bind device memory to image")
        };

        let view_info = vk::ImageViewCreateInfo::builder()
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(aspect)
                    .level_count(1)
                    .layer_count(1)
                    .build(),
            )
            .image(image)
            .format(create_info.format)
            .view_type(vk::ImageViewType::TYPE_2D);

        let view = unsafe { self.dev.create_image_view(&view_info, None).unwrap() };

        return (image, view, image_memory);
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let int_lock = self.d_internal.clone();
        let internal = int_lock.write().unwrap();

        unsafe {
            // first wait for the device to finish working
            self.dev.device_wait_idle().unwrap();

            self.dev.destroy_buffer(internal.transfer_buf, None);
            self.free_memory(internal.transfer_mem);

            self.dev.destroy_command_pool(internal.copy_cmd_pool, None);
            self.dev.destroy_fence(internal.copy_cbuf_fence, None);
            self.dev.destroy_device(None);
        }
    }
}
