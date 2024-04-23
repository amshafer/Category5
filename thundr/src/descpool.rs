/*
 * An allocator for descriptor sets, targeted for
 * creating many sets of image samplers
 *
 * Austin Shafer - 2020
 */

#![allow(non_camel_case_types)]
extern crate ash;

use crate::device::Device;
use ash::vk;
use std::sync::{Arc, Mutex};

/// The default size of each pool in DescPool
static POOL_SIZE: u32 = 4;

/// Single descriptor
///
/// This tracks the lifetime of one texture descriptor. When this
/// is dropped the descriptor will be freed and returned to the pool.
pub struct Descriptor {
    /// The owning pool
    d_pool: Arc<Mutex<DescPool>>,
    /// The descriptor set itself
    pub(crate) d_set: vk::DescriptorSet,
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        self.d_pool.lock().unwrap().free_set(self.d_set);
    }
}

/// A pool of descriptor pools
/// All resources allocated by the Renderer which holds this
pub struct DescPool {
    dp_dev: Arc<Device>,
    dp_pool: vk::DescriptorPool,
    /// number of allocations made from this pool, from 0 to POOL_SIZE
    dp_capacity: usize,
}

impl Drop for DescPool {
    fn drop(&mut self) {
        unsafe {
            self.dp_dev.dev.destroy_descriptor_pool(self.dp_pool, None);
        }
    }
}

impl DescPool {
    /// Allocate one Descriptor from the first available pool
    ///
    /// This may add a new pool to the system if needed. Returns None
    /// if this pool is full.
    pub fn alloc_descriptor(
        &mut self,
        layout: vk::DescriptorSetLayout,
    ) -> Option<vk::DescriptorSet> {
        if self.dp_capacity + 1 < POOL_SIZE as usize {
            return None;
        }

        let info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(self.dp_pool)
            .set_layouts(&[layout])
            .build();

        let set = unsafe { self.dp_dev.dev.allocate_descriptor_sets(&info).unwrap()[0] };

        self.dp_capacity += 1;

        Some(set)
    }

    /// Free one set in this pool
    ///
    /// This frees the set object, and decrements the tracker of sets
    /// allocated from this pool
    fn free_set(&mut self, set: vk::DescriptorSet) {
        unsafe {
            self.dp_dev.dev.free_descriptor_sets(self.dp_pool, &[set]);
        }
        self.dp_capacity -= 1;
    }
}

/// The overall descriptor tracker
///
/// This is in charge of fulfilling allocation requests by finding an
/// open pool to allocate from.
pub struct DescriptorSystem {
    ds_dev: Arc<Device>,
    /// these are the layouts for mesh specific (texture) descriptors
    /// Window-speccific descriptors (texture sampler)
    /// one for each framebuffer image
    ds_layout: vk::DescriptorSetLayout,
    ds_pools: Vec<Arc<Mutex<DescPool>>>,
}

impl DescriptorSystem {
    /// Allocate one Descriptor from the first available pool
    ///
    /// This may add a new pool to the system if needed.
    pub fn alloc_descriptor(&mut self) -> Descriptor {
        for pool in self.ds_pools.iter() {
            if let Some(set) = pool.lock().unwrap().alloc_descriptor(self.ds_layout) {
                return Descriptor {
                    d_pool: pool.clone(),
                    d_set: set,
                };
            }
        }

        // If we couldn't find a pool then add a new one
        let pool = self.add_pool();
        let ret = Descriptor {
            d_pool: pool.clone(),
            d_set: pool
                .lock()
                .unwrap()
                .alloc_descriptor(self.ds_layout)
                .unwrap(),
        };

        return ret;
    }

    /// Create an image sampler layout
    ///
    /// Descriptor layouts specify the number and characteristics
    /// of descriptor sets which will be made available to the
    /// pipeline through the pipeline layout.
    fn create_layout(dev: &Device) -> vk::DescriptorSetLayout {
        // supplies `descriptor_mesh_layouts`
        // There will be a sampler for each window
        //
        // This descriptor needs to be second in the pipeline list
        // so the shader can reference it as set 1
        let bindings = [vk::DescriptorSetLayoutBinding::builder()
            .binding(1)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            .descriptor_count(1)
            .build()];
        let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        unsafe { dev.dev.create_descriptor_set_layout(&info, None).unwrap() }
    }

    /// Adds and returns a new DescPool in the system
    pub fn add_pool(&mut self) -> Arc<Mutex<DescPool>> {
        let sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(POOL_SIZE)
            .build()];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&sizes)
            // we want to be able to free descriptor sets individually
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .max_sets(POOL_SIZE)
            .build();

        let pool = unsafe { self.ds_dev.dev.create_descriptor_pool(&info, None).unwrap() };

        let ret = Arc::new(Mutex::new(DescPool {
            dp_dev: self.ds_dev.clone(),
            dp_pool: pool,
            dp_capacity: 0,
        }));

        self.ds_pools.push(ret.clone());

        return ret;
    }

    pub fn new(dev: Arc<Device>) -> Self {
        Self {
            ds_layout: Self::create_layout(&dev),
            ds_dev: dev,
            ds_pools: Vec::new(),
        }
    }
}

impl Drop for DescriptorSystem {
    fn drop(&mut self) {
        unsafe {
            self.ds_dev
                .dev
                .destroy_descriptor_set_layout(self.ds_layout, None);
        }
    }
}
