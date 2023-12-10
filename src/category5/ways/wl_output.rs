// Implementation of the wl_output interface
//
// wl_output advertises what (physical) displays are available
// for clients to present surfaces on
//
// Austin Shafer 2020
extern crate wayland_server as ws;

use crate::category5::ws::Resource;
use crate::category5::Climate;
use ws::protocol::wl_output;
use ws::protocol::wl_output::{Mode, Subpixel, Transform};

#[allow(unused_variables)]
impl ws::GlobalDispatch<wl_output::WlOutput, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<wl_output::WlOutput>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        let out = data_init.init(resource, ());
        state.send_geometry(out.clone());

        // Add this new output object to our list to notify
        // when the output size changes
        state.c_outputs.push(out);
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<wl_output::WlOutput, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_output::WlOutput,
        request: wl_output::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        resource: &wl_output::WlOutput,
        data: &(),
    ) {
        // keep all of the outputs except this one
        state.c_outputs.retain(|o| o.id() != resource.id());
    }
}

impl Climate {
    pub fn send_geometry(&mut self, out: wl_output::WlOutput) {
        let res = self.c_atmos.lock().unwrap().get_resolution();
        // send geometry
        out.geometry(
            0,
            0,
            res.0 as i32,
            res.1 as i32,
            Subpixel::Unknown,
            "monitor".to_string(),
            "".to_string(),
            Transform::Normal,
        );

        out.mode(
            Mode::Current,
            res.0 as i32,
            res.1 as i32,
            60, // 60 Hz default
        );

        // let the client know we are done with the monitor config
        out.done();
    }

    pub fn send_all_geometry(&mut self) {
        for i in 0..self.c_outputs.len() {
            let out = self.c_outputs[i].clone();
            self.send_geometry(out);
        }
    }
}
