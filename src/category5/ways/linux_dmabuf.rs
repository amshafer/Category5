// Implementation of the unstable linux_dmabuf
// interfaces for importing GPU buffers into
// vkcomp.
//
// Austin Shafer - 2020
extern crate nix;
extern crate wayland_protocols;
extern crate wayland_server as ws;

use crate::category5::Climate;
use nix::unistd::close;
use utils::log;
use ws::protocol::wl_buffer;

use utils::Dmabuf;
use wayland_protocols::wp::linux_dmabuf::zv1::server::{
    zwp_linux_buffer_params_v1 as zlbpv1, zwp_linux_dmabuf_v1 as zldv1,
};

use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::{Arc, Mutex};

// drm modifier saying to implicitly infer
// the modifier from the dmabuf
//
// specified in linux-dmabuf-unstable-v1.xml
#[allow(dead_code)]
const DRM_FORMAT_MOD_INVALID_HI: u32 = 0x00ffffff;
#[allow(dead_code)]
const DRM_FORMAT_MOD_INVALID_LOW: u32 = 0xffffffff;

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
        // we need to advertise the format/modifier
        // combinations we support
        dma.format(WL_DRM_FORMAT_XRGB8888);
        dma.format(WL_DRM_FORMAT_ARGB8888);

        // The above format events are implicitly ignored by mesa,
        // these modifier events do the real work
        //
        // Sending zeroe as the modifier bits is the linear
        // drm format
        dma.modifier(WL_DRM_FORMAT_XRGB8888, 0, 0);
        dma.modifier(WL_DRM_FORMAT_ARGB8888, 0, 0);
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
        _resource: ws::backend::ObjectId,
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
        data.lock()
            .unwrap()
            .handle_request(request, resource, data_init);
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &Arc<Mutex<Params>>,
    ) {
    }
}

struct Params {
    // The list of added dma buffers
    p_bufs: Vec<Dmabuf>,
}

impl Params {
    #[allow(unused_variables)]
    fn handle_request(
        &mut self,
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

                // TODO
                // for now just only assign the first dmabuf
                let mut dmabuf = self.p_bufs[0];
                dmabuf.db_width = width;
                dmabuf.db_height = height;

                // Add our dmabuf to the userdata so Surface
                // can later hand it to vkcomp
                let buffer = data_init.init(buffer_id, Arc::new(dmabuf));

                params.created(&buffer);
            }
            zlbpv1::Request::Add {
                fd,
                plane_idx,
                offset,
                stride,
                modifier_hi,
                modifier_lo,
            } => self.add(
                fd.as_raw_fd(),
                plane_idx,
                offset,
                stride,
                modifier_hi,
                modifier_lo,
            ),
            zlbpv1::Request::Destroy => log::debug!("Destroying Dmabuf params"),
            _ => unimplemented!(),
        };
    }

    fn add(
        &mut self,
        fd: RawFd,
        plane_idx: u32,
        offset: u32,
        stride: u32,
        mod_hi: u32,
        mod_low: u32,
    ) {
        let d = Dmabuf::new(
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
impl ws::Dispatch<wl_buffer::WlBuffer, Arc<Dmabuf>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_buffer::WlBuffer,
        request: wl_buffer::Request,
        data: &Arc<Dmabuf>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &Arc<Dmabuf>,
    ) {
        // Close our dmabuf fd since this object was deleted
        log::debug!(
            "Destroying wl_buffer: closing dmabuf with fd {}",
            data.db_fd
        );
        close(data.db_fd).unwrap();
    }
}
