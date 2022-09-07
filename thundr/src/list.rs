// A list of surfaces to be displayed
//
// Austin Shafer - 2020

use super::surface::Surface;
use crate::renderer::{Renderer, WINDOW_LIST_GLSL_OFFSET};
use crate::Thundr;
use crate::{Damage, Result};
use ash::vk;
use lluvia as ll;
use std::iter::DoubleEndedIterator;
use std::ops::Index;
use utils::log;

#[derive(Debug)]
pub struct SurfaceList {
    /// This will get cleared during Thundr::draw
    pub(crate) l_changed: bool,
    l_vec: Vec<Surface>,
    /// List of damage caused by removing/adding surfaces
    pub(crate) l_damage: Vec<Damage>,
    /// The order of windows to be drawn. References r_windows.
    ///
    /// This is sorted back to front, where back comes first. i.e. the
    /// things you want to draw first should be in front of things that
    /// you want to be able to blend overtop of.
    pub l_window_order: Vec<ll::Entity>,
    pub l_order_buf: vk::Buffer,
    pub l_order_mem: vk::DeviceMemory,
    pub l_order_capacity: usize,
    /// The window order descriptor
    pub(crate) l_order_desc: vk::DescriptorSet,
    pub(crate) l_order_desc_pool: vk::DescriptorPool,
}

impl SurfaceList {
    pub fn new(thund: &mut Thundr) -> Self {
        let rend = &mut thund.th_rend;

        let mut ret = Self {
            l_changed: false,
            l_vec: Vec::new(),
            l_damage: Vec::new(),
            l_window_order: Vec::new(),
            l_order_buf: vk::Buffer::null(),
            l_order_mem: vk::DeviceMemory::null(),
            l_order_capacity: 8,
            l_order_desc_pool: vk::DescriptorPool::null(),
            l_order_desc: vk::DescriptorSet::null(),
        };

        unsafe {
            ret.reallocate_order_buf_with_cap(rend, ret.l_order_capacity);
            ret.allocate_order_resources(rend);
        }

        return ret;
    }

    pub fn update_window_order_buf(&mut self, rend: &Renderer) {
        unsafe {
            // Turn our vec of ll::Entitys into a vec of actual ids.
            let mut window_order = Vec::new();
            for ecs in self.l_window_order.iter() {
                window_order.push(ecs.get_raw_id() as i32);
            }
            log::debug!("Window order is {:?}", window_order);

            self.reallocate_order_buf_with_cap(rend, self.l_window_order.len());
            if window_order.len() > 0 {
                rend.update_memory(self.l_order_mem, 0, &[self.l_window_order.len()]);
                rend.update_memory(
                    self.l_order_mem,
                    WINDOW_LIST_GLSL_OFFSET,
                    window_order.as_slice(),
                );
            }
        }
    }

    /// This is a helper for reallocating the vulkan resources of the window order list
    unsafe fn reallocate_order_buf_with_cap(&mut self, rend: &Renderer, capacity: usize) {
        rend.wait_for_prev_submit();

        rend.dev.destroy_buffer(self.l_order_buf, None);
        rend.free_memory(self.l_order_mem);

        // create our data and a storage buffer for the window list
        let (wl_storage, wl_storage_mem) = rend.create_buffer_with_size(
            vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::DEVICE_LOCAL
                | vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT,
            (std::mem::size_of::<i32>() * 4 * (capacity / 4 + 1)) as u64
                + WINDOW_LIST_GLSL_OFFSET as u64,
        );
        rend.dev
            .bind_buffer_memory(wl_storage, wl_storage_mem, 0)
            .unwrap();
        self.l_order_buf = wl_storage;
        self.l_order_mem = wl_storage_mem;
        self.l_order_capacity = capacity;
    }

    /// Alloce the window order list's vulkan resources
    ///
    /// This will allocate the descriptor pool and descriptor layout
    /// and store them in self.
    unsafe fn allocate_order_resources(&mut self, rend: &Renderer) {
        // First make the descriptor pool and layout
        let size = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .build()];
        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);
        let order_pool = rend.dev.create_descriptor_pool(&info, None).unwrap();

        self.l_order_desc_pool = order_pool;
        self.allocate_order_desc(rend);
    }

    /// Update the window order descriptor
    ///
    /// This descriptor keeps a list of the window ids that need to be presented.
    /// These will each be rendered, and index into the global window list which
    /// contains their details.
    pub unsafe fn allocate_order_desc(&mut self, rend: &Renderer) {
        rend.dev
            .reset_descriptor_pool(
                self.l_order_desc_pool,
                vk::DescriptorPoolResetFlags::empty(),
            )
            .unwrap();

        // Now allocate our descriptor
        let info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(self.l_order_desc_pool)
            .set_layouts(&[rend.r_order_desc_layout])
            .build();
        self.l_order_desc = rend.dev.allocate_descriptor_sets(&info).unwrap()[0];

        let write_info = &[vk::WriteDescriptorSet::builder()
            .dst_set(self.l_order_desc)
            .dst_binding(0)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&[vk::DescriptorBufferInfo::builder()
                .buffer(self.l_order_buf)
                .offset(0)
                .range(vk::WHOLE_SIZE)
                .build()])
            .build()];
        rend.dev.update_descriptor_sets(
            write_info, // descriptor writes
            &[],        // descriptor copies
        );
    }

    fn damage_removed_surf(&mut self, mut surf: Surface) {
        surf.record_damage();
        match surf.take_surface_damage() {
            Some(d) => self.l_damage.push(d),
            None => {}
        };
    }

    pub fn remove(&mut self, index: usize) {
        self.l_changed = true;
        let surf = self.l_vec.remove(index);
        self.damage_removed_surf(surf);
    }

    pub fn remove_surface(&mut self, surf: Surface) -> Result<()> {
        // Check if the surface is present in the surface list. If so,
        // remove it.
        if let Some((index, _)) = self.l_vec.iter().enumerate().find(|(_, s)| **s == surf) {
            log::debug!("Removing surface at index {}", index);
            self.remove(index);
        }

        if let Some(mut parent) = surf.get_parent() {
            log::debug!("Removing subsurface");
            parent.remove_subsurface(surf)?;
        }

        Ok(())
    }

    pub fn insert(&mut self, order: usize, mut surf: Surface) {
        self.l_changed = true;
        surf.record_damage();
        self.l_vec.insert(order, surf);
    }

    pub fn push(&mut self, mut surf: Surface) {
        self.l_changed = true;
        surf.record_damage();
        self.l_vec.push(surf);
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Surface> {
        self.l_vec.iter()
    }
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Surface> {
        self.l_vec.iter_mut()
    }
    pub fn damage(&self) -> impl DoubleEndedIterator<Item = &Damage> {
        self.l_damage.iter()
    }

    fn map_per_surf_recurse<F>(&self, func: &mut F, surf: &Surface, x: i32, y: i32) -> bool
    where
        F: FnMut(&Surface, i32, i32) -> bool,
    {
        let internal = surf.s_internal.borrow();
        let surf_pos = &internal.s_rect.r_pos;

        // Note that the subsurface list is "reversed", with the front subsurface
        // being at the end of the array
        for sub in internal.s_subsurfaces.iter().rev() {
            // Add this surfaces offset to the subdsurface calculations.
            if !self.map_per_surf_recurse(func, sub, x + surf_pos.0 as i32, y + surf_pos.1 as i32) {
                return false;
            }
        }
        func(surf, x, y)
    }

    /// This is the generic map implementation, entrypoint to the recursive
    /// surface evaluation.
    pub fn map_on_all_surfaces<F>(&self, mut func: F)
    where
        F: FnMut(&Surface, i32, i32) -> bool,
    {
        for surf in self.l_vec.iter() {
            // Start here at no offset
            if !self.map_per_surf_recurse(&mut func, surf, 0, 0) {
                return;
            }
        }
    }

    pub fn clear_damage(&mut self) {
        self.l_damage.clear();
    }

    pub fn clear(&mut self) {
        self.l_changed = true;
        // Get the damage from all removed surfaces
        for mut surf in self.l_vec.drain(..) {
            surf.record_damage();
            match surf.take_surface_damage() {
                Some(d) => self.l_damage.push(d),
                None => {}
            };
        }
    }

    /// The length only considering immediate surfaces in the list
    pub fn len(&self) -> u32 {
        self.l_vec.len() as u32
    }

    /// The length accounting for subsurfaces
    pub fn len_with_subsurfaces(&self) -> u32 {
        let mut count = 0;
        self.map_on_all_surfaces(|_, _, _| {
            count += 1;
            return true;
        });

        count
    }

    pub fn destroy(&mut self, rend: &mut Renderer) {
        unsafe {
            rend.dev.destroy_buffer(self.l_order_buf, None);
            rend.free_memory(self.l_order_mem);
            rend.dev
                .destroy_descriptor_pool(self.l_order_desc_pool, None);
        }
    }
}

impl Index<usize> for SurfaceList {
    type Output = Surface;

    fn index(&self, index: usize) -> &Self::Output {
        &self.l_vec[index]
    }
}
