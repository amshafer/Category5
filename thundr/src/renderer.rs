// A vulkan rendering backend
//
// This layer is very low, and as a result is mostly unsafe. Nothing
// unsafe/vulkan/ash/etc should be exposed to upper layers
//
// Austin Shafer - 2020
#![allow(non_camel_case_types)]
use std::marker::Copy;
use std::sync::Arc;

use ash::vk;

use crate::display::Display;
use crate::instance::Instance;
use crate::Device;

extern crate utils as cat5_utils;
use crate::{CreateInfo, Result};
use cat5_utils::{log, region::Rect};

use lluvia as ll;

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
}

/// Recording parameters
///
/// Layers above this one will need to call recording
/// operations. They need a private structure to pass
/// to Renderer to begin/end recording operations
/// This is that structure.
pub struct RecordParams {
    /// our cached pushbuffer constants
    pub push: PushConstants,
}

impl RecordParams {
    pub fn new() -> Self {
        Self {
            push: PushConstants {
                width: 0,
                height: 0,
                image_id: -1,
                use_color: -1,
                color: (0.0, 0.0, 0.0, 0.0),
                dims: Rect::new(0, 0, 0, 0),
            },
        }
    }
}

/// Shader push constants
///
/// These will be updated when we record the per-viewport draw commands
/// and will contain the scrolling model transformation of all content
/// within a viewport.
///
/// This is also where we pass in the Surface's data.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PushConstants {
    pub width: u32,
    pub height: u32,
    /// The id of the image. This is the offset into the unbounded sampler array.
    /// id that's the offset into the unbound sampler array
    pub image_id: i32,
    /// if we should use color instead of texturing
    pub use_color: i32,
    /// Opaque color
    pub color: (f32, f32, f32, f32),
    /// The complete dimensions of the window.
    pub dims: Rect<i32>,
}

// Most of the functions below will be unsafe. Only the safe functions
// should be used by the applications. The unsafe functions are mostly for
// internal use.
impl Renderer {
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
        _img_ecs: ll::Instance,
    ) -> Result<(Renderer, Display)> {
        // Our display is in charge of choosing a medium to draw on,
        // and will create a surface on that medium
        let display = Display::new(info, dev.clone())?;

        // you are now the proud owner of a half complete
        // rendering context
        // p.s. you still need a Pipeline
        let rend = Renderer {
            _inst: instance,
            dev: dev,
        };

        return Ok((rend, display));
    }

    /// Wait for the submit_fence
    ///
    /// This waits for the last frame render operation to finish submitting.
    pub fn wait_for_prev_submit(&self) {
        self.dev.wait_for_latest_timeline();
    }

    /// Start recording a cbuf for one frame
    pub fn begin_recording_one_frame(&mut self) -> Result<RecordParams> {
        // At least wait for any image copies to complete
        self.dev.wait_for_copy();

        Ok(RecordParams::new())
    }

    /// End a total frame recording
    pub fn end_recording_one_frame(&mut self) {}
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
        }
    }
}
