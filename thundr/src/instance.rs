// Vulkan rendering instance
//
// This holds all of the common instance code for the Vulkan context

use ash::extensions::ext;
use ash::{vk, Entry};

extern crate utils as cat5_utils;
use crate::display::Display;
use crate::CreateInfo;
use cat5_utils::log;

use std::ffi::{CStr, CString};
use std::os::raw::c_void;

// Nvidia aftermath SDK GPU crashdump support
#[cfg(feature = "aftermath")]
extern crate nvidia_aftermath_rs as aftermath;
#[cfg(feature = "aftermath")]
use aftermath::Aftermath;

// this happy little debug callback is from the ash examples
// all it does is print any errors/warnings thrown.
unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_types: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> u32 {
    log::error!(
        "[VK][{:?}][{:?}] {:?}",
        message_severity,
        message_types,
        CStr::from_ptr(p_callback_data.as_ref().unwrap().p_message)
    );
    println!();
    vk::FALSE
}

/// A Vulkan Instance
///
/// This holds our basic vulkan session data. We use this to create
/// any devices and such which Thundr will use internally to render.
pub struct Instance {
    /// debug callback sugar mentioned earlier
    debug_loader: ext::DebugUtils,
    debug_callback: vk::DebugUtilsMessengerEXT,

    /// the entry just loads function pointers from the dynamic library
    /// I am calling it a loader, because that's what it does
    pub(crate) loader: Entry,
    /// the big vulkan instance.
    pub(crate) inst: ash::Instance,

    /// Nvidia Aftermath SDK instance. Inclusion of this enables
    /// GPU crashdumps.
    #[cfg(feature = "aftermath")]
    aftermath: Aftermath,
}

impl Instance {
    /// Creates a new debug reporter and registers our function
    /// for debug callbacks so we get nice error messages
    fn setup_debug(
        entry: &Entry,
        instance: &ash::Instance,
    ) -> (ext::DebugUtils, vk::DebugUtilsMessengerEXT) {
        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                    | vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
            )
            .pfn_user_callback(Some(vulkan_debug_callback));

        let dr_loader = ext::DebugUtils::new(entry, instance);
        let callback = unsafe {
            dr_loader
                .create_debug_utils_messenger(&debug_info, None)
                .unwrap()
        };
        return (dr_loader, callback);
    }

    /// Create a vkInstance
    ///
    /// Most of the create info entries are straightforward, with
    /// some basic extensions being enabled. All of the work is
    /// done in subfunctions.
    pub fn new(info: &CreateInfo) -> Self {
        let entry = Entry::linked();
        let app_name = CString::new("Thundr").unwrap();

        // For some reason old versions of the validation layers segfault in renderpass on the
        // geometric one, so only use validation on compute
        let layer_names = vec![
            #[cfg(debug_assertions)]
            CString::new("VK_LAYER_KHRONOS_validation").unwrap(),
            #[cfg(target_os = "macos")]
            CString::new("VK_LAYER_KHRONOS_synchronization2").unwrap(),
        ];

        let layer_names_raw: Vec<*const i8> = layer_names
            .iter()
            .map(|raw_name: &CString| raw_name.as_ptr())
            .collect();

        let mut extension_names_raw = Display::extension_names(info);
        extension_names_raw.push(ext::DebugUtils::name().as_ptr());

        let appinfo = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .application_version(0)
            .engine_name(&app_name)
            .engine_version(0)
            .api_version(vk::API_VERSION_1_2)
            .build();

        let mut create_info = vk::InstanceCreateInfo::builder()
            .application_info(&appinfo)
            .enabled_layer_names(&layer_names_raw)
            .enabled_extension_names(&extension_names_raw)
            .build();

        let printf_info = vk::ValidationFeaturesEXT::builder()
            //.enabled_validation_features(&[vk::ValidationFeatureEnableEXT::DEBUG_PRINTF])
            .enabled_validation_features(&[
                vk::ValidationFeatureEnableEXT::SYNCHRONIZATION_VALIDATION,
            ])
            .build();
        create_info.p_next = &printf_info as *const _ as *const std::os::raw::c_void;

        let instance: ash::Instance = unsafe {
            entry
                .create_instance(&create_info, None)
                .expect("Instance creation error")
        };

        let (dr_loader, d_callback) = Self::setup_debug(&entry, &instance);

        // This *must* be done before we create our device
        #[cfg(feature = "aftermath")]
        let aftermath = Aftermath::initialize().expect("Could not enable Nvidia Aftermath SDK");

        Self {
            loader: entry,
            inst: instance,
            debug_loader: dr_loader,
            debug_callback: d_callback,
            #[cfg(feature = "aftermath")]
            aftermath: aftermath,
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            self.debug_loader
                .destroy_debug_utils_messenger(self.debug_callback, None);
            self.inst.destroy_instance(None);
        }
    }
}
