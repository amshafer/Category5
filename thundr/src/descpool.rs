/*
 * An allocator for descriptor sets, targeted for
 * creating many sets of image samplers
 *
 * Austin Shafer - 2020
 */

#![allow(non_camel_case_types)]
extern crate ash;

use ash::vk;
use std::sync::{Arc, Mutex};

/// The default size of each pool in DescSingleVKPool
static POOL_SIZE: u32 = 4;

/// Single descriptor
///
/// This tracks the lifetime of one texture descriptor. When this
/// is destroyed the descriptor will be freed and returned to the pool.
#[derive(Clone)]
pub struct Descriptor {
    /// The owning pool
    d_pool: Arc<Mutex<DescSingleVKPool>>,
    /// The descriptor set itself. This is borrowed from the above pool and
    /// will be returned when this struct is freed.
    pub d_set: vk::DescriptorSet,
}

impl Descriptor {
    pub fn destroy(&mut self) {
        self.d_pool.lock().unwrap().free_set(self.d_set);
        self.d_set = vk::DescriptorSet::null();
    }
}

/// A pool of descriptor pools
/// All resources allocated by the Renderer which holds this
pub struct DescSingleVKPool {
    dp_pool: vk::DescriptorPool,
    /// The descriptors allocated form this pool
    ///
    /// These are all allocated up front. We are repeatedly creating
    /// and binding images constantly, so this avoids overhead from
    /// reallocating the sets in the driver.
    dp_descriptors: Vec<vk::DescriptorSet>,
}

impl DescSingleVKPool {
    /// Destroy this pool
    fn destroy(&mut self, dev: &ash::Device) {
        unsafe {
            dev.destroy_descriptor_pool(self.dp_pool, None);
        }
    }

    /// Allocate one Descriptor from the first available pool
    ///
    /// This may add a new pool to the system if needed. Returns None
    /// if this pool is full.
    pub fn alloc_descriptor(&mut self) -> Option<vk::DescriptorSet> {
        if self.dp_descriptors.len() == 0 {
            return None;
        }

        Some(self.dp_descriptors.pop().unwrap())
    }

    /// Free one set in this pool
    ///
    /// This frees the set object, and decrements the tracker of sets
    /// allocated from this pool
    fn free_set(&mut self, set: vk::DescriptorSet) {
        self.dp_descriptors.push(set);
    }
}

/// The overall descriptor tracker
///
/// This is in charge of fulfilling allocation requests by finding an
/// open pool to allocate from.
pub struct DescPool {
    /// these are the layouts for mesh specific (texture) descriptors
    /// Window-speccific descriptors (texture sampler)
    /// one for each framebuffer image
    pub ds_layout: vk::DescriptorSetLayout,
    ds_pools: Vec<Arc<Mutex<DescSingleVKPool>>>,
}

impl DescPool {
    /// Allocate one Descriptor from the first available pool
    ///
    /// This may add a new pool to the system if needed.
    pub fn alloc_descriptor(&mut self, dev: &ash::Device) -> Descriptor {
        for pool in self.ds_pools.iter() {
            if let Some(set) = pool.lock().unwrap().alloc_descriptor() {
                return Descriptor {
                    d_pool: pool.clone(),
                    d_set: set,
                };
            }
        }

        // If we couldn't find a pool then add a new one
        let pool = self.add_pool(dev);
        let ret = Descriptor {
            d_pool: pool.clone(),
            d_set: pool.lock().unwrap().alloc_descriptor().unwrap(),
        };

        return ret;
    }

    /// Create an image sampler layout
    ///
    /// Descriptor layouts specify the number and characteristics
    /// of descriptor sets which will be made available to the
    /// pipeline through the pipeline layout.
    fn create_layout(dev: &ash::Device) -> vk::DescriptorSetLayout {
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

        unsafe { dev.create_descriptor_set_layout(&info, None).unwrap() }
    }

    /// Adds and returns a new DescSingleVKPool in the system
    pub fn add_pool(&mut self, dev: &ash::Device) -> Arc<Mutex<DescSingleVKPool>> {
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

        let pool = unsafe { dev.create_descriptor_pool(&info, None).unwrap() };

        // Allocate all of our descriptors
        let layouts: Vec<vk::DescriptorSetLayout> = std::iter::repeat(self.ds_layout)
            .take(POOL_SIZE as usize)
            .collect();
        let alloc_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts.as_slice())
            .build();
        let sets = unsafe { dev.allocate_descriptor_sets(&alloc_info).unwrap().to_vec() };

        let ret = Arc::new(Mutex::new(DescSingleVKPool {
            dp_pool: pool,
            dp_descriptors: sets,
        }));

        self.ds_pools.push(ret.clone());

        return ret;
    }

    pub fn new(dev: &ash::Device) -> Self {
        Self {
            ds_layout: Self::create_layout(dev),
            ds_pools: Vec::new(),
        }
    }

    /// Destroy our descriptor system.
    ///
    /// We can't use drop here since Device will own this struct and we
    /// will be called from it. It instead just passes its device in.
    pub fn destroy(&mut self, dev: &ash::Device) {
        for pool in self.ds_pools.iter() {
            pool.lock().unwrap().destroy(dev);
        }

        unsafe {
            dev.destroy_descriptor_set_layout(self.ds_layout, None);
        }
    }
}
