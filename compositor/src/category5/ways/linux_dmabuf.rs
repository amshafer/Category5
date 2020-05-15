// Implementation of the unstable linux_dmabuf
// interfaces for importing GPU buffers into
// vkcomp.
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::wl_buffer;

use crate::category5::vkcomp::wm;
use super::sys::drm_fourcc::DRM_FORMAT_MOD_NONE;
use super::surface::*;
use super::protocol::linux_dmabuf::{
    zwp_linux_dmabuf_v1 as zldv1,
    zwp_linux_buffer_params_v1 as zlbpv1,
};

use std::rc::Rc;
use std::cell::RefCell;
use std::clone::Clone;

pub fn linux_dmabuf_handle_request(req: zldv1::Request,
                                   dma: Main<zldv1::ZwpLinuxDmabufV1>)
{
    match req {
        zldv1::Request::CreateParams { params_id } =>
            params_id.quick_assign(|d, r, _| {
                dmabuf_params_handle_request(r, d)
            }),
        _ => unimplemented!(),
    };
}

pub fn dmabuf_params_handle_request(req: zlbpv1::Request,
                                    dma: Main<zlbpv1::ZwpLinuxBufferParamsV1>)
{
    match req {
        zlbpv1::Request::CreateImmed { buffer_id,
                                       width,
                                       height,
                                       format,
                                       flags } => {
            println!("linux_dmabuf: Creating a new wl_buffer");
        },
        zlbpv1::Request::Destroy => {},
        _ => unimplemented!(),
    };
}
