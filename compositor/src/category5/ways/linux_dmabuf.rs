// Implementation of the unstable linux_dmabuf
// interfaces for importing GPU buffers into
// vkcomp.
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;

use super::protocol::linux_dmabuf::{
    zwp_linux_dmabuf_v1 as zldv1,
    zwp_linux_buffer_params_v1 as zlbpv1,
};

use std::clone::Clone;
use std::os::unix::io::RawFd;

// drm modifier saying to implicitly infer
// the modifier from the dmabuf
//
// specified in linux-dmabuf-unstable-v1.xml
const DRM_FORMAT_MOD_INVALID_HI: u32 = 0x00ffffff;
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

    // The above format events are legacy and will be ignored,
    // these modifier events do the real work
    dma.modifier(WL_DRM_FORMAT_XRGB8888,
                 DRM_FORMAT_MOD_INVALID_HI,
                 DRM_FORMAT_MOD_INVALID_LOW);
    dma.modifier(WL_DRM_FORMAT_ARGB8888,
                 DRM_FORMAT_MOD_INVALID_HI,
                 DRM_FORMAT_MOD_INVALID_LOW);
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

// Represents one dma buf the client has added.
// Will be referenced by Params during wl_buffer
// creation.
#[allow(dead_code)]
#[derive(Debug,Copy,Clone)]
pub struct DmaBuf {
    db_fd: RawFd,
    db_plane_idx: u32,
    db_offset: u32,
    db_stride: u32,
    // These will be added later during creation
    db_width: i32,
    db_height: i32,
}

struct Params {
    // The list of added dma buffers
    p_bufs: Vec<DmaBuf>,
}

impl Params {
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
                println!("linux_dmabuf_params: Creating a new wl_buffer");

                // TODO
                // for now just only assign the first dmabuf
                let mut dmabuf = self.p_bufs[0];
                dmabuf.db_width = width;
                dmabuf.db_height = height;

                buffer_id.quick_assign(|_, _, _| {});
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
           _mod_hi: u32,
           _mod_low: u32) {
        let d = DmaBuf{
            db_fd: fd,
            db_plane_idx: plane_idx,
            db_offset: offset,
            db_stride: stride,
            // These are null for now, will be updated
            // in creat/create_immed
            db_width: -1,
            db_height: -1,
        };
        println!("linux_dmabuf_params:Adding {:?}", d);
        self.p_bufs.push(d);
    }
}
