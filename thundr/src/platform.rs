// This module handles flagging the available behaviors
// supported by the device. Things like drm import, surface type,
// mutable swapchain support, etc
//
// Austin Shafer - 2021
use ash::extensions::khr;
use ash::version::InstanceV1_0;
use ash::{vk, Instance};

use crate::CreateInfo;
use std::ffi::CStr;
use utils::log;

/// The available vulkan capabilities.
///
/// This is composed of two parts: flags for available features, and
/// lists of extensions to enable. The extension lists will be constructed
/// from the flags to avoid keeping them in memory forever.
pub struct VKDeviceFeatures {
    /// Does this device allow import/export using drm modifiers
    pub vkc_supports_ext_mem: bool,
    /// Does this device allow import/export using dmabuf handles. Might not
    /// necessarily need drm.
    pub vkc_supports_dmabuf: bool,
    /// Does the device support using swapchain images as different types/formats?
    pub vkc_supports_mut_swapchain: bool,
    /// Does the device support massive indexing of descriptors. Mandatory for CompPipeline.
    pub vkc_supports_desc_indexing: bool,

    // The following are the lists of extensions that map to the above features
    vkc_ext_mem_exts: [*const i8; 1],
    vkc_dmabuf_exts: [*const i8; 1],
    vkc_mut_swapchain_exts: [*const i8; 3],
    vkc_desc_indexing_exts: [*const i8; 2],
}

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
    pub fn new(_info: &CreateInfo, inst: &Instance, pdev: vk::PhysicalDevice) -> Self {
        let mut ret = Self {
            vkc_supports_ext_mem: false,
            vkc_supports_dmabuf: false,
            vkc_supports_mut_swapchain: false,
            vkc_supports_desc_indexing: false,
            vkc_ext_mem_exts: [khr::ExternalMemoryFd::name().as_ptr()],
            vkc_dmabuf_exts: [khr::ExternalMemoryFd::name().as_ptr()],
            vkc_mut_swapchain_exts: [
                vk::KhrSwapchainMutableFormatFn::name().as_ptr(),
                vk::KhrImageFormatListFn::name().as_ptr(),
                vk::KhrMaintenance2Fn::name().as_ptr(),
            ],
            vkc_desc_indexing_exts: [
                vk::KhrMaintenance3Fn::name().as_ptr(),
                vk::ExtDescriptorIndexingFn::name().as_ptr(),
            ],
        };

        unsafe {
            let exts = inst.enumerate_device_extension_properties(pdev).unwrap();

            match contains_extensions(exts.as_slice(), &ret.vkc_ext_mem_exts) {
                true => ret.vkc_supports_ext_mem = true,
                false => {
                    log::error!("This vulkan device does not support external memory importing")
                }
            }
            match contains_extensions(exts.as_slice(), &ret.vkc_dmabuf_exts) {
                true => ret.vkc_supports_dmabuf = true,
                false => log::error!("This vulkan device does not support dmabuf import/export"),
            }
            match contains_extensions(exts.as_slice(), &ret.vkc_mut_swapchain_exts) {
                true => ret.vkc_supports_mut_swapchain = true,
                false => log::error!("This vulkan device does not support mutable swapchains"),
            }
            match contains_extensions(exts.as_slice(), &ret.vkc_desc_indexing_exts) {
                true => ret.vkc_supports_desc_indexing = true,
                false => log::error!("This vulkan device does not support descriptor indexing"),
            }
        }

        return ret;
    }

    pub fn get_device_extensions(&self) -> Vec<*const i8> {
        let mut ret = vec![khr::Swapchain::name().as_ptr()];

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
        if self.vkc_supports_dmabuf {
            for e in self.vkc_mut_swapchain_exts.iter() {
                ret.push(*e)
            }
        }
        if self.vkc_supports_desc_indexing {
            for e in self.vkc_desc_indexing_exts.iter() {
                ret.push(*e)
            }
        }

        return ret;
    }
}
