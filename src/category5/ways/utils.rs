// Common functions for wayland code
//
// Austin Shafer - 2020
pub extern crate wayland_server as ws;
use ws::{Client, Filter};

use crate::category5::atmosphere::Atmosphere;
use utils::{log, ClientId};

use std::sync::{Arc, Mutex};

/// Helper method for registering the property id of a client
///
/// We need to make an id for the client for our entity component set in
/// the atmosphere. This method should be used when creating globals, so
/// we can register the new client with the atmos
///
/// Returns the id created
pub fn register_new_client(atmos_cell: Arc<Mutex<Atmosphere>>, client: Client) -> ClientId {
    let id;
    {
        let mut atmos = atmos_cell.lock().unwrap();
        // make a new client id
        id = atmos.mint_client_id();

        if !client.data_map().insert_if_missing(move || id) {
            log::error!("registering a client that has already been registered");
        }
    }

    // when the client is destroyed we need to tell the atmosphere
    // to free the reserved space
    // TODO add destructor
    client.add_destructor(Filter::new(move |_, _, _| {
        atmos_cell.lock().unwrap().free_client_id(id);
    }));

    return id;
}

/// Grab the id belonging to this client
///
/// The id is stored in the userdata map, which is kind of annoying to deal with
/// we wrap it here so it can change easily
///
/// If the client does not currently have an id, register it
pub fn get_id_from_client(atmos: Arc<Mutex<Atmosphere>>, client: Client) -> ClientId {
    match client.data_map().get::<ClientId>() {
        Some(id) => *id,
        // The client hasn't been assigned an id
        None => register_new_client(atmos, client),
    }
}

/// Tries to get the client id from the client, and returns none if
/// it has not been stored there yet.
#[allow(dead_code)]
pub fn try_get_id_from_client(client: Client) -> Option<ClientId> {
    match client.data_map().get::<ClientId>() {
        Some(id) => Some(*id),
        // The client hasn't been assigned an id
        None => None,
    }
}
