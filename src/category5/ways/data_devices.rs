// Implementations of inter-app data transfer operations. aka copy/paste and drag/drop
//
// Austin Shafer - 2020
use ws::protocol::wl_data_device_manager as wlddm;
use ws::Main;

use crate::category5::atmosphere::Atmosphere;
use utils::WindowId;

use std::cell::RefCell;
use std::rc::Rc;

pub fn wl_data_device_manager_handle_request(
    req: wlddm::Request,
    _: Main<wlddm::WlDataDeviceManager>,
) {
    match req {
        wlddm::Request::CreateDataSource { id } => {}
        wlddm::Request::GetDataDevice { id } => {}
        _ => {}
    }
}
