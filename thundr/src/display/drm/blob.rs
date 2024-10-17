// This is a modified version of smithay's code to read the format/modifier
// info from the IN_FORMATS DRM blob. This is the only example of how to do
// this with the drm-rs crate and although it has some significant changes it's
// still similar enough I felt it needed to retain the original license header.
// For that reason it is in this separate file.

extern crate drm_ffi;
use super::drm::buffer;
use super::drm::control::{plane, Device as ControlDevice};
use super::drm_device::DrmDevice;

use crate::{Result, ThundrError};
use utils::log;

use std::convert::TryFrom;

// MIT License
//
// Copyright (c) 2017 Victor Berger and Victoria Brekenfeld
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
pub fn get_argb8888_modifiers(
    drm: &DrmDevice,
    plane: plane::Handle,
) -> Result<Vec<buffer::DrmModifier>> {
    let mut modifiers = Vec::new();

    let plane_props = drm.get_properties(plane).or(Err(ThundrError::NO_DISPLAY))?;

    // Get the ARGB8888 supported by this plane
    let in_formats = plane_props
        .as_hashmap(&*drm)
        .or(Err(ThundrError::NO_DISPLAY))?["IN_FORMATS"]
        .handle();
    let mods_info = drm.get_property(in_formats).map_err(|e| {
        log::error!("Could not get DRM format/modifier info: {:?}", e);
        ThundrError::NO_DISPLAY
    })?;

    // Start by finding the blob id for our IN_FORMATS property
    let (handles, raw_values) = plane_props.as_props_and_values();
    let blob_id = raw_values[handles
        .iter()
        .enumerate()
        .find_map(
            |(i, handle)| {
                if *handle == in_formats {
                    Some(i)
                } else {
                    None
                }
            },
        )
        .unwrap()];

    // Get the blob value instead of a raw int
    if let drm::control::property::Value::Blob(blob) = mods_info.value_type().convert_value(blob_id)
    {
        let data = drm.get_property_blob(blob).map_err(|e| {
            log::error!("Could not get DRM format/modifier info: {:?}", e);
            ThundrError::NO_DISPLAY
        })?;

        // Now we have to do the equivalent of drmModeFormatModifierBlobIterNext() be careful here,
        // we have no idea about the alignment inside the blob, so always copy using
        // `read_unaligned`, although slice::from_raw_parts would be so much nicer to iterate and
        // to read.
        unsafe {
            let fmt_mod_blob_ptr = data.as_ptr() as *const drm_ffi::drm_format_modifier_blob;
            let fmt_mod_blob = &*fmt_mod_blob_ptr;

            let formats_ptr: *const u32 = fmt_mod_blob_ptr
                .cast::<u8>()
                .offset(fmt_mod_blob.formats_offset as isize)
                as *const _;
            let modifiers_ptr: *const drm_ffi::drm_format_modifier = fmt_mod_blob_ptr
                .cast::<u8>()
                .offset(fmt_mod_blob.modifiers_offset as isize)
                as *const _;
            #[allow(clippy::unnecessary_cast)]
            let formats_ptr = formats_ptr as *const u32;
            #[allow(clippy::unnecessary_cast)]
            let modifiers_ptr = modifiers_ptr as *const drm_ffi::drm_format_modifier;

            for i in 0..fmt_mod_blob.count_modifiers {
                let mod_info = modifiers_ptr.offset(i as isize).read_unaligned();
                for j in 0..64 {
                    if mod_info.formats & (1u64 << j) != 0 {
                        let code = buffer::DrmFourcc::try_from(
                            formats_ptr
                                .offset((j + mod_info.offset) as isize)
                                .read_unaligned(),
                        )
                        .ok();

                        // We are only recording Argb8888 modifiers in this function
                        if let Some(code) = code {
                            if code != buffer::DrmFourcc::Argb8888 {
                                continue;
                            }

                            // Finally insert this modifier into our list
                            let new_mod = buffer::DrmModifier::from(mod_info.modifier);
                            if modifiers.iter().find(|&&m| m == new_mod).is_none() {
                                modifiers.push(new_mod);
                            }
                        }
                    }
                }
            }
        }

        return Ok(modifiers);
    }

    return Err(ThundrError::NO_DISPLAY);
}
