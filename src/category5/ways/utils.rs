// Common functions for wayland code
//
// Austin Shafer - 2020
pub extern crate wayland_server as ws;

use crate::category5::{
    atmosphere::{Atmosphere, ClientId},
    ClientInfo,
};

/// Grab the id belonging to this client
///
/// The id is stored in the userdata map, which is kind of annoying to deal with
/// we wrap it here so it can change easily
///
/// If the client does not currently have an id, register it
pub fn get_id_from_client(_atmos: &mut Atmosphere, client: ws::Client) -> ClientId {
    match client.get_data::<ClientInfo>() {
        Some(info) => info.ci_id.clone(),
        // The client hasn't been assigned an id
        None => panic!("This client wasn't initialized properly"),
    }
}
