// Implementation of the unstable linux_dmabuf
// interfaces for importing GPU buffers into
// vkcomp.
//
// Austin Shafer - 2020
extern crate nix;
extern crate wayland_server as ws;

use utils::log_prelude::*;
use nix::unistd::close;
use ws::{Filter,Main,Resource};
use ws::protocol::wl_buffer;

use utils::Dmabuf;
use super::protocol::linux_dmabuf::{
    zwp_linux_dmabuf_v1 as zldv1,
    zwp_linux_buffer_params_v1 as zlbpv1,
};

use std::os::unix::io::RawFd;

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

pub fn linux_dmabuf_setup(dma: Main<zldv1::ZwpLinuxDmabufV1>) {
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


pub fn linux_dmabuf_handle_request(req: zldv1::Request,
                                   _dma: Main<zldv1::ZwpLinuxDmabufV1>)
{
    match req {
        zldv1::Request::CreateParams { params_id } => {
            let mut params = Params { p_bufs: Vec::new() };

            params_id.quick_assign(move |p, r, _| {
                params.handle_request(r, p);
            });
        },
        zldv1::Request::Destroy => {},
    };
}

struct Params {
    // The list of added dma buffers
    p_bufs: Vec<Dmabuf>,
}

impl Params {
    #[allow(unused_variables)]
    fn handle_request(&mut self,
                      req: zlbpv1::Request,
                      _params: Main<zlbpv1::ZwpLinuxBufferParamsV1>)
    {
        match req {
            zlbpv1::Request::CreateImmed { buffer_id,
                                           width,
                                           height,
                                           format,
                                           flags } => {
                log!(LogLevel::debug,
                     "linux_dmabuf_params: Creating a new wl_buffer");
                log!(LogLevel::debug,
                     "                     of size {}x{}", width, height);

                // TODO
                // for now just only assign the first dmabuf
                let mut dmabuf = self.p_bufs[0];
                dmabuf.db_width = width;
                dmabuf.db_height = height;

                // Add our dmabuf to the userdata so Surface
                // can later hand it to vkcomp
                buffer_id.quick_assign(|_, _, _| {});
                buffer_id.assign_destructor(Filter::new(
                    move |r: Resource<wl_buffer::WlBuffer>, _, _| {
                        let ud = r.user_data().get::<Dmabuf>().unwrap();
                        log!(LogLevel::profiling,
                             "Destroying wl_buffer: closing dmabuf with fd {}",
                             ud.db_fd);
                        close(ud.db_fd).unwrap();
                    }
                ));

                buffer_id.as_ref()
                    .user_data()
                    .set(move || dmabuf);
            },
            zlbpv1::Request::Add { fd,
                                   plane_idx,
                                   offset,
                                   stride,
                                   modifier_hi,
                                   modifier_lo } =>
                self.add(fd, plane_idx, offset, stride,
                         modifier_hi, modifier_lo),
            zlbpv1::Request::Destroy => {},
            _ => unimplemented!(),
        };
    }

    fn add(&mut self,
           fd: RawFd,
           plane_idx: u32,
           offset: u32,
           stride: u32,
           mod_hi: u32,
           mod_low: u32) {
        let d = Dmabuf::new(
            fd, plane_idx, offset, stride,
            (mod_hi as u64) << 32 | (mod_low as u64)
        );
        log!(LogLevel::profiling, "linux_dmabuf_params:Adding {:#?}", d);
        self.p_bufs.push(d);
    }
}
