// Implementation of the unstable linux_dmabuf
// interfaces for importing GPU buffers into
// vkcomp.
//
// Austin Shafer - 2020
extern crate wayland_protocols;
extern crate wayland_server as ws;

use crate::category5::Atmosphere;
use crate::category5::Climate;
use utils::log;
use ws::protocol::wl_buffer;

use dakota as dak;
use dakota::{Dmabuf, DmabufPlane};
use wayland_protocols::wp::linux_dmabuf::zv1::server::{
    zwp_linux_buffer_params_v1 as zlbpv1, zwp_linux_dmabuf_v1 as zldv1,
};

#[cfg(debug_assertions)]
use std::os::unix::io::AsRawFd;
use std::os::unix::io::OwnedFd;
use std::sync::{Arc, Mutex};

// drm formats specified in mesa's private wl_drm
// protocol. We need this for mesa clients.
//
// gross
const WL_DRM_FORMAT_XRGB8888: u32 = 0x34325258;
const WL_DRM_FORMAT_ARGB8888: u32 = 0x34325241;

#[allow(unused_variables)]
impl ws::GlobalDispatch<zldv1::ZwpLinuxDmabufV1, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<zldv1::ZwpLinuxDmabufV1>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        let dma = data_init.init(resource, ());

        let drm_formats = [WL_DRM_FORMAT_XRGB8888, WL_DRM_FORMAT_ARGB8888];

        // we need to advertise the format/modifier
        // combinations we support
        for format in drm_formats {
            dma.format(format);

            for modifier in state.c_primary_render_mods.iter() {
                let mod_hi = (modifier >> 32) as u32;
                let mod_low = (modifier & 0xffffffff) as u32;
                dma.modifier(format, mod_hi, mod_low);
            }

            // Send our linear modifier as it is always supported
            dma.modifier(format, 0, 0);
        }
    }
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<zldv1::ZwpLinuxDmabufV1, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &zldv1::ZwpLinuxDmabufV1,
        request: zldv1::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            zldv1::Request::CreateParams { params_id } => {
                let params = Arc::new(Mutex::new(Params { p_bufs: Vec::new() }));

                data_init.init(params_id, params);
            }
            _ => {}
        };
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &zldv1::ZwpLinuxDmabufV1,
        data: &(),
    ) {
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<zlbpv1::ZwpLinuxBufferParamsV1, Arc<Mutex<Params>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &zlbpv1::ZwpLinuxBufferParamsV1,
        request: zlbpv1::Request,
        data: &Arc<Mutex<Params>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data.lock().unwrap().handle_request(
            &mut state.c_scene,
            state.c_atmos.lock().as_mut().unwrap(),
            request,
            resource,
            data_init,
        );
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &zlbpv1::ZwpLinuxBufferParamsV1,
        data: &Arc<Mutex<Params>>,
    ) {
    }
}

struct Params {
    // The list of added dma buffers
    p_bufs: Vec<DmabufPlane>,
}

impl Params {
    #[allow(unused_variables)]
    fn handle_request(
        &mut self,
        scene: &mut dak::Scene,
        atmos: &mut Atmosphere,
        req: zlbpv1::Request,
        params: &zlbpv1::ZwpLinuxBufferParamsV1,
        data_init: &mut ws::DataInit<'_, Climate>,
    ) {
        match req {
            zlbpv1::Request::CreateImmed {
                buffer_id,
                width,
                height,
                format,
                flags,
            } => {
                log::debug!(
                    "linux_dmabuf_params: Creating a new wl_buffer of size {}x{}",
                    width,
                    height
                );

                // First create our userdata and initialize our wl_buffer. We need this
                // so we can have a valid buffer object to use as the release data in
                // the dmabuf import
                let dmabuf = self.create(width, height, format);
                let tmp = atmos.mint_buffer_id(scene);
                // Test that we can import this dmabuf
                match scene.define_resource_from_dmabuf(&tmp, &dmabuf, None) {
                    Ok(res) => res,
                    Err(e) => {
                        log::error!("Failed to import dmabuf: {:?}", e);
                        params.failed();
                        return;
                    }
                };

                let buffer = data_init.init(buffer_id, dmabuf);

                params.created(&buffer);
            }
            zlbpv1::Request::Add {
                fd,
                plane_idx,
                offset,
                stride,
                modifier_hi,
                modifier_lo,
            } => self.add(fd, plane_idx, offset, stride, modifier_hi, modifier_lo),
            zlbpv1::Request::Destroy => log::debug!("Destroying Dmabuf params"),
            _ => unimplemented!(),
        };
    }

    /// Constructs a Dmabuf object from these parameters
    fn create(&mut self, width: i32, height: i32, _format: u32) -> Dmabuf {
        let mut dmabuf = dak::Dmabuf::new(width, height);

        for plane in self.p_bufs.drain(0..) {
            dmabuf.db_planes.push(plane);
        }

        return dmabuf;
    }

    fn add(
        &mut self,
        fd: OwnedFd,
        plane_idx: u32,
        offset: u32,
        stride: u32,
        mod_hi: u32,
        mod_low: u32,
    ) {
        let d = DmabufPlane::new(
            fd,
            plane_idx,
            offset,
            stride,
            (mod_hi as u64) << 32 | (mod_low as u64),
        );
        log::debug!("linux_dmabuf_params: Adding {:#?}", d);
        self.p_bufs.push(d);
    }
}

// Handle wl_buffer with a dmabuf attached
// This will clean up the fd when released
#[allow(unused_variables)]
impl ws::Dispatch<wl_buffer::WlBuffer, dak::Dmabuf> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_buffer::WlBuffer,
        request: wl_buffer::Request,
        data: &dak::Dmabuf,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &wl_buffer::WlBuffer,
        data: &dak::Dmabuf,
    ) {
        // Close our dmabuf fd since this object was deleted
        log::debug!(
            "Destroying wl_buffer: closing dmabuf with fd {}",
            data.db_planes[0].db_fd.as_raw_fd()
        );
    }
}
