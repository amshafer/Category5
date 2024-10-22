// This module handles flagging the available behaviors
// supported by the device. Things like drm import, surface type,
// mutable swapchain support, etc
//
// Austin Shafer - 2021
use ash::extensions::khr;
use ash::{vk, Instance};

use crate::{CreateInfo, SurfaceType};
use std::ffi::CStr;
use utils::log;

/// The available vulkan capabilities.
///
/// This is composed of two parts: flags for available features, and
/// lists of extensions to enable. The extension lists will be constructed
/// from the flags to avoid keeping them in memory forever.
pub struct VKDeviceFeatures {
    /// Does this device allow import/export from opaque fds
    pub vkc_supports_ext_mem: bool,
    /// Does this device allow import/export using dmabuf handles. Might not
    /// necessarily need drm.
    pub vkc_supports_dmabuf: bool,
    /// Does the device support using swapchain images as different types/formats?
    pub vkc_supports_mut_swapchain: bool,
    /// Does the device support massive indexing of descriptors. Mandatory for CompPipeline.
    pub vkc_supports_desc_indexing: bool,
    /// Does this device allow import/export using drm modifiers
    pub vkc_supports_drm_modifiers: bool,
    pub vkc_supports_incremental_present: bool,
    /// Does this device support telling us the DRM major/minor numbers in use?
    pub vkc_supports_phys_dev_drm: bool,
    /// Does this device support the nvidia aftermath sdk?
    pub vkc_supports_nvidia_aftermath: bool,
    /// Does this device support VkSwapchain
    pub vkc_supports_swapchain: bool,

    // The following are the lists of extensions that map to the above features
    vkc_ext_mem_exts: [*const i8; 1],
    vkc_dmabuf_exts: [*const i8; 3],
    vkc_mut_swapchain_exts: [*const i8; 3],
    vkc_desc_indexing_exts: [*const i8; 2],
    vkc_drm_modifiers_exts: [*const i8; 1],
    vkc_incremental_present_exts: [*const i8; 1],
    vkc_phys_dev_drm_exts: [*const i8; 1],
    vkc_nv_aftermath_exts: [*const i8; 2],
    vkc_timeline_exts: [*const i8; 1],
    vkc_swapchain_exts: [*const i8; 1],
}

unsafe impl Send for VKDeviceFeatures {}
unsafe impl Sync for VKDeviceFeatures {}

fn contains_extensions(exts: &[vk::ExtensionProperties], req: &[*const i8]) -> bool {
    let mut count = 0;

    for r in req.iter() {
        let rstr = unsafe { CStr::from_ptr(*r as *const std::os::raw::c_char) };

        for e in exts {
            let estr = unsafe { CStr::from_ptr(&e.extension_name as *const std::os::raw::c_char) };
            if rstr == estr {
                // increment our count, once we have verified all extensions are
                // present then return true
                count += 1;
                if count == req.len() {
                    return true;
                }
                break;
            }
        }
    }

    return false;
}

impl VKDeviceFeatures {
    pub fn new(info: &CreateInfo, inst: &Instance, pdev: vk::PhysicalDevice) -> Self {
        let mut pdev_props = vk::PhysicalDeviceProperties2::builder().build();
        unsafe {
            inst.get_physical_device_properties2(pdev, &mut pdev_props);
        }

        let mut ret = Self {
            vkc_supports_ext_mem: false,
            vkc_supports_dmabuf: false,
            vkc_supports_mut_swapchain: false,
            vkc_supports_desc_indexing: false,
            vkc_supports_drm_modifiers: false,
            vkc_supports_incremental_present: false,
            vkc_supports_phys_dev_drm: false,
            vkc_supports_nvidia_aftermath: false,
            vkc_supports_swapchain: false,
            vkc_ext_mem_exts: [khr::ExternalMemoryFd::name().as_ptr()],
            vkc_dmabuf_exts: [
                vk::ExtExternalMemoryDmaBufFn::name().as_ptr(),
                khr::ExternalMemoryFd::name().as_ptr(),
                vk::ExtQueueFamilyForeignFn::name().as_ptr(),
            ],
            vkc_mut_swapchain_exts: [
                vk::KhrSwapchainMutableFormatFn::name().as_ptr(),
                vk::KhrImageFormatListFn::name().as_ptr(),
                vk::KhrMaintenance2Fn::name().as_ptr(),
            ],
            vkc_desc_indexing_exts: [
                vk::KhrMaintenance3Fn::name().as_ptr(),
                vk::ExtDescriptorIndexingFn::name().as_ptr(),
            ],
            vkc_drm_modifiers_exts: [vk::ExtImageDrmFormatModifierFn::name().as_ptr()],
            vkc_incremental_present_exts: [vk::KhrIncrementalPresentFn::name().as_ptr()],
            vkc_phys_dev_drm_exts: [vk::ExtPhysicalDeviceDrmFn::name().as_ptr()],
            vkc_nv_aftermath_exts: [
                vk::NvDeviceDiagnosticsConfigFn::name().as_ptr(),
                vk::NvDeviceDiagnosticCheckpointsFn::name().as_ptr(),
            ],
            vkc_timeline_exts: [vk::KhrTimelineSemaphoreFn::name().as_ptr()],
            vkc_swapchain_exts: [khr::Swapchain::name().as_ptr()],
        };

        let exts = unsafe { inst.enumerate_device_extension_properties(pdev).unwrap() };

        let mut supports_ext_mem = false;
        match contains_extensions(exts.as_slice(), &ret.vkc_ext_mem_exts) {
            true => supports_ext_mem = true,
            false => log::error!("This vulkan device does not support external memory importing"),
        }
        let mut supports_dmabuf = false;
        match contains_extensions(exts.as_slice(), &ret.vkc_dmabuf_exts) {
            true => supports_dmabuf = true,
            false => log::error!("This vulkan device does not support dmabuf import/export"),
        }
        let mut supports_mut_swapchain = false;
        match contains_extensions(exts.as_slice(), &ret.vkc_mut_swapchain_exts) {
            true => supports_mut_swapchain = true,
            false => log::error!("This vulkan device does not support mutable swapchains"),
        }
        let mut supports_desc_indexing = false;
        match contains_extensions(exts.as_slice(), &ret.vkc_desc_indexing_exts) {
            true => supports_desc_indexing = true,
            false => log::error!("This vulkan device does not support descriptor indexing"),
        }
        let mut supports_drm_modifiers = false;
        match contains_extensions(exts.as_slice(), &ret.vkc_drm_modifiers_exts) {
            true => supports_drm_modifiers = true,
            false => {
                log::error!("This vulkan device does not support importing with drm modifiers")
            }
        }

        let mut supports_swapchain = false;
        match contains_extensions(exts.as_slice(), &ret.vkc_swapchain_exts) {
            true => supports_swapchain = true,
            false => {
                log::error!("This vulkan device does not support importing with drm modifiers")
            }
        }

        if !contains_extensions(exts.as_slice(), &ret.vkc_timeline_exts) {
            panic!("Thundr: required support for VK_KHR_timeline_semaphore not found");
        }

        let mut supports_incremental_present =
            match contains_extensions(exts.as_slice(), &ret.vkc_incremental_present_exts) {
                true => true,
                false => {
                    log::error!("This vulkan device does not support incremental presentation");
                    false
                }
            };
        // Force incremental presentation off if env var is defined
        if std::env::var("THUNDR_NO_INCREMENTAL_PRESENT").is_ok() {
            supports_incremental_present = false
        }

        let supports_aftermath =
            match contains_extensions(exts.as_slice(), &ret.vkc_nv_aftermath_exts) {
                true => true,
                false => {
                    log::error!("This vulkan device does not support incremental presentation");
                    false
                }
            };

        // Now test the device features to see if subcomponents of these extensions are available
        let mut features = vk::PhysicalDeviceFeatures2::builder().build();
        let mut index_features = vk::PhysicalDeviceDescriptorIndexingFeatures::builder().build();
        if supports_desc_indexing {
            features.p_next = &mut index_features as *mut _ as *mut std::ffi::c_void;
        }
        unsafe { inst.get_physical_device_features2(pdev, &mut features) }

        let uses_vk_surface = match info.surface_type {
            SurfaceType::Headless => false,
            _ => true,
        };

        ret.vkc_supports_ext_mem = supports_ext_mem;
        ret.vkc_supports_dmabuf = supports_dmabuf;
        ret.vkc_supports_drm_modifiers = supports_drm_modifiers;
        ret.vkc_supports_incremental_present = supports_incremental_present;
        ret.vkc_supports_desc_indexing = supports_desc_indexing
            && index_features.descriptor_binding_variable_descriptor_count > 0
            && index_features.descriptor_binding_partially_bound > 0
            && index_features.descriptor_binding_update_unused_while_pending > 0
            && index_features.descriptor_binding_storage_buffer_update_after_bind > 0
            && index_features.descriptor_binding_sampled_image_update_after_bind > 0;
        ret.vkc_supports_nvidia_aftermath = supports_aftermath;
        // Only enable VkSwapchain for a swapchain backend which uses it
        ret.vkc_supports_swapchain = supports_swapchain && uses_vk_surface;
        ret.vkc_supports_mut_swapchain = ret.vkc_supports_swapchain && supports_mut_swapchain;

        match contains_extensions(exts.as_slice(), &ret.vkc_phys_dev_drm_exts) {
            true => ret.vkc_supports_phys_dev_drm = true,
            false => log::error!("This vulkan device does not support VK_EXT_physical_device_drm"),
        }

        return ret;
    }

    pub fn get_device_extensions(&self) -> Vec<*const i8> {
        let mut ret = Vec::new();

        if self.vkc_supports_ext_mem {
            for e in self.vkc_ext_mem_exts.iter() {
                ret.push(*e)
            }
        }
        if self.vkc_supports_dmabuf {
            for e in self.vkc_dmabuf_exts.iter() {
                ret.push(*e)
            }
        }
        if self.vkc_supports_mut_swapchain {
            for e in self.vkc_mut_swapchain_exts.iter() {
                ret.push(*e)
            }
        }
        if self.vkc_supports_desc_indexing {
            for e in self.vkc_desc_indexing_exts.iter() {
                ret.push(*e)
            }
        }
        if self.vkc_supports_drm_modifiers {
            for e in self.vkc_drm_modifiers_exts.iter() {
                ret.push(*e)
            }
        }
        if self.vkc_supports_incremental_present {
            for e in self.vkc_incremental_present_exts.iter() {
                ret.push(*e)
            }
        }
        if self.vkc_supports_phys_dev_drm {
            for e in self.vkc_phys_dev_drm_exts.iter() {
                ret.push(*e)
            }
        }

        if self.vkc_supports_swapchain {
            for e in self.vkc_swapchain_exts.iter() {
                ret.push(*e)
            }
        }

        #[cfg(feature = "aftermath")]
        if self.vkc_supports_nvidia_aftermath {
            for e in self.vkc_nv_aftermath_exts.iter() {
                ret.push(*e)
            }
        }

        for e in self.vkc_timeline_exts.iter() {
            ret.push(*e)
        }

        return ret;
    }
}
