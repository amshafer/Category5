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
use std::sync::Arc;

/// The default size of each pool in DescPool
static POOL_SIZE: u32 = 4;

/// A pool of descriptor pools
/// All resources allocated by the Renderer which holds this
pub struct DescPool {
    dev: Arc<Device>,
    /// these are the layouts for mesh specific (texture) descriptors
    /// Window-speccific descriptors (texture sampler)
    /// one for each framebuffer image
    pub layout: vk::DescriptorSetLayout,
    pools: Vec<vk::DescriptorPool>,
    /// number of allocations in each pool, from 0 to POOL_SIZE
    capacities: Vec<usize>,
}

impl DescPool {
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

    /// Returns the index of the new pool
    pub fn add_pool(&mut self) -> usize {
        let sizes = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(POOL_SIZE)
            .build()];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&sizes)
            // we want to be able to free descriptor sets individually
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .max_sets(POOL_SIZE);

        self.pools
            .push(unsafe { self.dev.dev.create_descriptor_pool(&info, None).unwrap() });
        // Add an entry to record that there have not been
        // any allocations with this pool
        self.capacities.push(0);

        return self.pools.len() - 1;
    }

    /// rend should own this struct
    pub fn create(dev: Arc<Device>) -> DescPool {
        let mut ret = DescPool {
            layout: DescPool::create_layout(&dev),
            dev: dev,
            pools: Vec::new(),
            capacities: Vec::new(),
        };

        // Add one default pool to begin with
        ret.add_pool();

        return ret;
    }
}

impl Drop for DescPool {
    fn drop(&mut self) {
        unsafe {
            for p in self.pools.iter() {
                self.dev.dev.destroy_descriptor_pool(*p, None);
            }
            self.dev
                .dev
                .destroy_descriptor_set_layout(self.layout, None);
        }
    }
}
