/*
 * An allocator for descriptor sets, targeted for
 * creating many sets of image samplers
 *
 * Austin Shafer - 2020
 */

#![allow(dead_code, non_camel_case_types)]
extern crate ash;

use ash::{vk, Device};

/// The default size of each pool in DescPool
static POOL_SIZE: u32 = 4;

/// A pool of descriptor pools
/// All resources allocated by the Renderer which holds this
pub struct DescPool {
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

        unsafe { dev.create_descriptor_set_layout(&info, None).unwrap() }
    }

    /// Returns the index of the new pool
    pub fn add_pool(&mut self, dev: &Device) -> usize {
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
            .push(unsafe { dev.create_descriptor_pool(&info, None).unwrap() });
        // Add an entry to record that there have not been
        // any allocations with this pool
        self.capacities.push(0);

        return self.pools.len() - 1;
    }

    /// rend should own this struct
    pub fn create(dev: &Device) -> DescPool {
        let mut ret = DescPool {
            layout: DescPool::create_layout(dev),
            pools: Vec::new(),
            capacities: Vec::new(),
        };

        // Add one default pool to begin with
        ret.add_pool(dev);

        return ret;
    }

    fn get_ideal_pool(&mut self, dev: &Device, size: usize) -> usize {
        for (i, cap) in self.capacities.iter().enumerate() {
            // Check if this pool has room for the sets
            if cap + size < POOL_SIZE as usize {
                // A pool with space was found
                return i;
            }
        }

        // No existing pool was found, so create a new one
        return self.add_pool(&dev);
    }

    /// Allocate an image sampler descriptor set
    ///
    /// A descriptor set specifies a group of attachments that can
    /// be referenced by the graphics pipeline. Think of a descriptor
    /// as the hardware's handle to a resource. The set of descriptors
    /// allocated in each set is specified in the layout.
    pub fn allocate_samplers(
        &mut self,
        dev: &Device,
        count: usize,
    ) -> (usize, Vec<vk::DescriptorSet>) {
        // Find a pool to allocate from
        let pool_handle = self.get_ideal_pool(dev, count);

        // We should repeat the layout n times, so that only one
        // call to the allocation function needs to be made
        let mut layouts = Vec::new();
        for _ in 0..count {
            layouts.push(self.layout);
        }

        let info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(self.pools[pool_handle])
            .set_layouts(layouts.as_slice())
            .build();

        // record that n allocations were made from this pool
        self.capacities[pool_handle] += 1;

        unsafe {
            return (
                pool_handle,
                // Allocates a set for each layout specified
                dev.allocate_descriptor_sets(&info).unwrap(),
            );
        }
    }

    pub fn destroy_samplers(
        &mut self,
        dev: &Device,
        pool_handle: usize,
        samplers: &[vk::DescriptorSet],
    ) {
        assert!(pool_handle < self.pools.len());

        unsafe {
            for s in samplers {
                dev.free_descriptor_sets(self.pools[pool_handle], &[*s])
                    .unwrap();
            }
        }
    }

    /// Explicit destructor
    pub fn destroy(&mut self, dev: &Device) {
        unsafe {
            for p in self.pools.iter() {
                dev.destroy_descriptor_pool(*p, None);
            }
            dev.destroy_descriptor_set_layout(self.layout, None);
        }
    }
}
