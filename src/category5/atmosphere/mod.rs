//! # Atmosphere: an entity-component set
//!
//! Atmosphere is our entity component set used to communicate between the
//! different subsystems. It assigns a numerical id to a resource which
//! can be used to get or set the value of different properties. For
//! example, `ways` will update the position property of a window. During
//! the next frame, vkcomp will read this updated position and use it to
//! draw that window in a new location.
//!
//! Atmosphere is hyper-specific to category5. It is not general purpose,
//! and has weird design constraints unique to category5.
//!
//! ## Properties
//!
//! Properties are defined by an enum with a set of variants containing
//! data. The variant data is the property datatype. Getters and Setters
//! for each property will be autogenerated.
//!
//! As an example here is the set of per-client properties.
//! ```
//! #[derive(Clone, Debug, AtmosECSGetSet)]
//! enum ClientProperty {
//!     // is this id in use?
//!     client_in_use(bool),
//!     // window ids belonging to this client
//!     windows_for_client(Vec<WindowId>),
//! }
//! ```
//!
//! A subsystem can then get the list of windows a client is associated
//! with using:
//! ```
//! self.get_windows_for_client(client);
//! ```
//!
//! ## Design
//!
//! One of the reasons that the atmosphere exists is to isolate `ways`
//! from `vkcomp`. Because we want `vkcomp` to run in a different process
//! in some configurations, we don't want to give it access to
//! datastructures owned by `ways`.
//!
//! The solution is to have a double-buffered database that tracks the
//! property values. There are two internal copies of the database, of
//! type `Hemisphere`, that each subsystem holds. Once at the beginning of
//! each frame, the two threads will *flip* hemispheres and replay any
//! new changes on top of the incoming hemisphere.
//!
//! This also serves as the synchronization mechanism between the two
//! threads. Once `vkcomp` is done rendering, it will wait for the
//! hemispheres to flip. `ways` can control the rate that rendering occurs
//! by choosing when to flip hemispheres and give `vkcomp` the updated
//! property values.
//!
//! The atmosphere uses getter/setters to interact with property values to
//! isolate the rest of the code from the internal data
//! representation. Atmosphere's internal mechanics for storing and
//! updating data can be changed without changing all of Category5.
//!
//! ## Property Map
//!
//! A database needs to have a backing store, and ours is a
//! PropertyMap. Our property map converts a entity id (WindowId/ClientId)
//! and a property id into the value for that property. It needs to be
//! able to do a few things
//!
//! * O(1) lookups
//! * Record changes to properties so they can be replayed later.
//! * Handle arbitrary property types
//!
//! The constant-time lookup requirement is necessary to prevent excessive
//! cpu usage while the compositor is running. Properties will be set many
//! times a second, and it needs to be instant. This is done by using the
//! id values and indices into a lookup table. It essentially looks like
//! this:
//!
//! ```
//! Indexed by entity id
//!   v
//!   _
//!  |_| ---> v indexed by property id
//!  |_|     |_|
//!  |_|	 |_|--> property value
//!  |_|	 |_|
//!  |_|	 |_|
//!  |_|
//!  |_|
//!  |_|
//! ```
//!
//! So a property lookup is two lookups: an index into the main table
//! using the entity id and a lookup into the property table using the
//! property id. Property indexes are automatically derived from the enums
//! specifying the property types.
//!
//! The second two requirements are the motivation for using enums as the
//! property type: enums can be used to record a change to one
//! property. We can save a list of these enums to represent the list of
//! changes that has happened this frame and need to be replayed on the
//! hemisphere. The enums variants can hold any type of data that
//! implements Clone.
//!
//! We then need a way to efficiently record the set of property changes that have
//! occurred during one frame. We do this by caching property changes into
//! a HashMap of the enums previously mentioned. This still allows for our
//! constant-time lookup, and makes sure we have one update for each
//! property modified. Effectively all changes during a frame happen in
//! this hashmap, which is then atomically applied to the hemisphere. We
//! "replay" these changes on our current hemisphere before sending it to
//! the other subsystem, and replay our changes over the incoming
//! hemisphere. Once changes are replayed on both, the hemispheres are
//! consistent.

// Austin Shafer - 2020
#![allow(dead_code)]
extern crate wayland_server as ws;
use ws::protocol::wl_surface;

extern crate thundr as th;

pub mod property;
use property::{Property, PropertyId};
pub mod property_map;
use property_map::PropertyMap;
pub mod property_list;
use property_list::PropertyList;
mod skiplist;

use crate::category5::vkcomp::wm;
use crate::category5::ways::{seat::Seat, surface::*, xdg_shell::xdg_toplevel::ResizeEdge};
use utils::{log, ClientId, WindowId};

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{SystemTime, UNIX_EPOCH};
use std::vec::Vec;

/// Global data not tied to a client or window
///
/// See `Property` for implementation comments
#[derive(Copy, Clone, Debug, AtmosECSGetSet)]
enum GlobalProperty {
    /// !! IF YOU CHANGE THIS UPDATE property_count BELOW!!
    cursor_pos(f64, f64),
    resolution(u32, u32),
    grabbed(Option<WindowId>),
    resizing(Option<WindowId>),
    /// the window the user is currently interacting with
    /// This tells us which one to start looking at for the skiplist
    ///
    /// Not to be confused with `surf_focus`, this refers to the *application*
    /// that is currently in focus. It is used to track the "root" window that
    /// was created by xdg/wl_shell.
    win_focus(Option<WindowId>),
    /// This is the current surface that is in focus, not respective of application.
    /// It is possible that this is the same as `win_focus`.
    ///
    /// This is the wl_surface that the user has entered, and it is highly likely
    /// that this is a subsurface. Therefore `win_focus` will be the "root" application
    /// toplevel window, and `surf_focus` may be a subsurface of that window tree.
    surf_focus(Option<WindowId>),
    /// Is recording traces with Renderdoc enabled?
    /// This is used for debugging. input will trigger this, which tells vkcomp
    /// to record frames.
    renderdoc_recording(bool),
    /// The name of the DRM node in use. This will be filled in by vkcomp
    /// and populated from VK_EXT_physical_device_drm
    drm_dev(i64, i64),
}

// These are indexed by ClientId
#[derive(Clone, Debug, AtmosECSGetSet)]
enum ClientProperty {
    // is this id in use?
    client_in_use(bool),
    // window ids belonging to this client
    windows_for_client(Vec<WindowId>),
}

// These are indexed by WindowId
#[derive(Clone, Debug, AtmosECSGetSet)]
enum WindowProperty {
    // is this id in use?
    window_in_use(bool),
    // The client that created this window
    owner(ClientId),
    // does this window have the toplevel role
    // this controls if SSD are drawn
    toplevel(bool),
    // the position of the visible portion of the window
    window_pos(f32, f32),
    // size of the visible portion (non-CSD) of the window
    // window manager uses this
    window_size(f32, f32),
    // If this window is a subsurface, then x and y will
    // be offsets from the base of the parent window
    surface_pos(f32, f32),
    // the size of the surface
    // aka the size of the last buffer attached
    // vkcomp uses this
    surface_size(f32, f32),
    // This window's position in the desktop order
    //
    // The next window behind this one
    skiplist_next(Option<WindowId>),
    // The window in front of this one
    skiplist_prev(Option<WindowId>),
    // The next *visible* window
    skiplist_skip(Option<WindowId>),
    // The toplevel child surface
    // because surfaces can be arbitrarily nested,
    // surfaces may be added to this list instead
    // of the main global ordering.
    //
    // The start of the subsurface skiplist
    top_child(Option<WindowId>),
    // If this is a subsurface of another window
    // aka not a toplevel
    parent_window(Option<WindowId>),
    // This is the root of the window tree that this window
    // is a part of. When this surface is in focus, this will
    // be the value of the `win_focus` global prop.
    root_window(Option<WindowId>),
}

// per-surface private data
//
// This holds all of the protocol resources
// that ways needs. An array of these are used (indexed
// by the window id) to tie an id to a set of protocol
// objects. i.e. find the surface/seat/etc for this id.
#[derive(Clone)]
enum Priv {
    // a surface to have its callbacks called
    surface(Option<Rc<RefCell<Surface>>>),
    // The protocol object for this surface
    // We need to store this here because some places
    // (`keyboard_enter`) will want to query for it to deliver
    // events while the above surface is borrowed
    wl_surface(wl_surface::WlSurface),
}

impl Priv {
    const SURFACE: PropertyId = 0;
    const WL_SURFACE: PropertyId = 1;
    const VARIANT_LEN: PropertyId = 2;
}

impl Property for Priv {
    fn get_property_id(&self) -> PropertyId {
        match self {
            Self::surface(_) => Self::SURFACE,
            Self::wl_surface(_) => Self::WL_SURFACE,
        }
    }

    fn variant_len() -> u32 {
        return Self::VARIANT_LEN as u32;
    }
}

// per-client private data
//
// Clients can have multiple windows, so we need this
// to hold resources that are tied only to the client
#[derive(Clone)]
enum ClientPriv {
    // a collection of input resources
    seat(Option<Rc<RefCell<Seat>>>),
}

impl ClientPriv {
    const SEAT: PropertyId = 0;
    const VARIANT_LEN: PropertyId = 1;
}

impl Property for ClientPriv {
    fn get_property_id(&self) -> PropertyId {
        match self {
            Self::seat(_) => Self::SEAT,
        }
    }

    fn variant_len() -> u32 {
        return Self::VARIANT_LEN as u32;
    }
}

/// Global state tracking
///
/// Don't make fun of my naming convention pls. We need a
/// place for all wayland code to stash meta information.
/// This is such a place, but it should not hold anything
/// exceptionally protocol-specific for sync reasons.
///
/// Although this is referenced everywhere, both sides
/// will have their own version of it. a_hemisphere is
/// the shared part.
///
/// Keep in mind this only holds any shared data, data
/// exclusive to subsystems will be held by said subsystem
#[allow(dead_code)]
pub struct Atmosphere {
    /// transfer channel
    a_tx: Sender<Box<Hemisphere>>,
    /// receive channel
    a_rx: Receiver<Box<Hemisphere>>,
    /// The current hemisphere
    ///
    /// While this is held we will retain ownership of the
    /// current hemisphere's mutex, and be able to service
    /// requests on this hemisphere
    ///
    /// To switch hemispheres we send this through the
    /// channel to the other subsystem.
    a_hemi: Option<Box<Hemisphere>>,

    // -- private subsystem specific resources --
    /// These keep track of what ids we have handed out to the
    /// property maps. We need to track this here since we are
    /// patching the prop maps and can't rely on them
    a_client_id_map: Vec<bool>,
    a_window_id_map: Vec<bool>,

    // -- ways --
    a_window_priv: PropertyMap<Priv>,
    a_client_priv: PropertyMap<ClientPriv>,
    /// A hashmap of patches based on the window id and the
    /// property name
    ///
    /// This needs to be a hashmap since we want to quickly
    /// update information. Searching would take too long.
    ///
    /// ways performs patch replay:
    /// Changes will be accrued in a batch here during hemisphere
    /// construction, and will then be applied before flipping.
    /// when receiving the other hemisphere, first replay all
    /// patches before constructing a new changeset.
    a_window_patches: HashMap<(WindowId, PropertyId), WindowProperty>,
    a_client_patches: HashMap<(ClientId, PropertyId), ClientProperty>,
    a_global_patches: HashMap<PropertyId, GlobalProperty>,
}

impl Atmosphere {
    /// Create a new atmosphere to be shared within a subsystem
    ///
    /// We pass in the hemispheres and lock(s) since they will have to
    /// also be passed to the other subsystem.
    /// One subsystem must be setup as index 0 and the other
    /// as index 1
    pub fn new(tx: Sender<Box<Hemisphere>>, rx: Receiver<Box<Hemisphere>>) -> Atmosphere {
        let mut atmos = Atmosphere {
            a_tx: tx,
            a_rx: rx,
            a_hemi: Some(Box::new(Hemisphere::new())),
            // TODO: only do this for ways
            a_window_priv: PropertyMap::new(),
            a_client_priv: PropertyMap::new(),
            a_client_id_map: Vec::new(),
            a_window_id_map: Vec::new(),
            a_client_patches: HashMap::new(),
            a_window_patches: HashMap::new(),
            a_global_patches: HashMap::new(),
        };

        // We need to set this property to the default since
        // vkcomp will expect it.
        atmos.set_cursor_pos(0.0, 0.0);
        // resolution is set by wm
        atmos.set_grabbed(None);
        atmos.set_resizing(None);
        atmos.set_win_focus(None);
        atmos.set_surf_focus(None);
        atmos.set_renderdoc_recording(false);

        return atmos;
    }

    /// Gets the next available id in a vec of bools
    /// generic id getter
    fn get_next_id(v: &mut Vec<bool>) -> usize {
        for (i, in_use) in v.iter_mut().enumerate() {
            if !*in_use {
                *in_use = true;
                return i;
            }
        }

        v.push(true);
        return v.len() - 1;
    }

    /// Get the next id
    ///
    /// Find a free id if one is available, if not then
    /// add a new one
    pub fn mint_client_id(&mut self) -> ClientId {
        let id = ClientId(Atmosphere::get_next_id(&mut self.a_client_id_map));
        self.reserve_client_id(id);
        return id;
    }

    pub fn mint_window_id(&mut self, client: ClientId) -> WindowId {
        let id = WindowId(Atmosphere::get_next_id(&mut self.a_window_id_map));
        self.reserve_window_id(client, id);
        return id;
    }

    /// Add a patch to be replayed on a flip
    ///
    /// All changes to the current hemisphere will get
    /// batched up into a set of patches. This is needed to
    /// keep both hemispheres in sync.
    fn set_global_prop(&mut self, value: &GlobalProperty) {
        self.mark_changed();
        let prop_id = value.get_property_id();
        // check if there is an existing patch to overwrite
        if let Some(v) = self.a_global_patches.get_mut(&prop_id) {
            // if so, just update it
            *v = *value;
        } else {
            self.a_global_patches.insert(prop_id, *value);
        }
    }

    fn get_global_prop(&self, prop_id: PropertyId) -> Option<&GlobalProperty> {
        // check if there is an existing patch to grab
        if let Some(v) = self.a_global_patches.get(&prop_id) {
            return Some(v);
        }
        return self.a_hemi.as_ref().unwrap().get_global_prop(prop_id);
    }

    fn set_client_prop(&mut self, client: ClientId, value: &ClientProperty) {
        self.mark_changed();
        let prop_id = value.get_property_id();
        // check if there is an existing patch to overwrite
        if let Some(v) = self.a_client_patches.get_mut(&(client, prop_id)) {
            // if so, just update it
            *v = value.clone();
        } else {
            self.a_client_patches
                .insert((client, prop_id), value.clone());
        }
    }

    fn get_client_prop(&self, client: ClientId, prop_id: PropertyId) -> Option<&ClientProperty> {
        // check if there is an existing patch to grab
        if let Some(v) = self.a_client_patches.get(&(client, prop_id)) {
            return Some(v);
        }
        return self
            .a_hemi
            .as_ref()
            .unwrap()
            .get_client_prop(client, prop_id);
    }

    fn set_window_prop(&mut self, id: WindowId, value: &WindowProperty) {
        self.mark_changed();
        let prop_id = value.get_property_id();
        // check if there is an existing patch to overwrite
        if let Some(v) = self.a_window_patches.get_mut(&(id, prop_id)) {
            // if so, just update it
            *v = value.clone();
        } else {
            self.a_window_patches.insert((id, prop_id), value.clone());
        }
    }

    fn get_window_prop(&self, id: WindowId, prop_id: PropertyId) -> Option<&WindowProperty> {
        // check if there is an existing patch to grab
        if let Some(v) = self.a_window_patches.get(&(id, prop_id)) {
            return Some(v);
        }
        return self.a_hemi.as_ref().unwrap().get_window_prop(id, prop_id);
    }

    /// Commit all our patches into the hemisphere
    ///
    /// We are batching all the changes into patches. We
    /// then need to apply those patches to the current
    /// hemisphere before we send it to update it. We also
    /// need to replay the patches on the new hemisphere to
    /// update it will all the info it's missing
    fn replay(&mut self, hemi: &mut Hemisphere) {
        log::info!("replaying on hemisphere");
        for (prop_id, prop) in self.a_global_patches.iter() {
            log::info!("   replaying {:?}", prop);
            hemi.set_global_prop(*prop_id, prop);
        }
        for ((id, prop_id), prop) in self.a_client_patches.iter() {
            log::info!("   replaying {:?}", prop);
            hemi.set_client_prop(*id, *prop_id, prop);
        }
        for ((id, prop_id), prop) in self.a_window_patches.iter() {
            log::info!("   replaying {:?}", prop);
            hemi.set_window_prop(*id, *prop_id, prop);
        }

        // Apply any remaining constant state like cursor
        // positions
        hemi.commit();
    }

    /// Must be called before recv_hemisphere
    pub fn send_hemisphere(&mut self) {
        // first grab our own hemi
        if let Some(mut h) = self.a_hemi.take() {
            log::info!("sending hemisphere");
            // second, we need to apply our changes to
            // our own hemisphere before we send it
            self.replay(&mut h);

            // actually flip hemispheres
            self.a_tx.send(h).expect("Could not send hemisphere");
        }
    }

    /// Must be called after send_hemisphere
    /// returns Some if we were able to get the other hemisphere
    /// if returns None, this needs to be called again
    pub fn recv_hemisphere(&mut self) -> Option<Box<Hemisphere>> {
        log::info!("trying to recv hemisphere");
        match self.a_rx.recv() {
            Ok(h) => Some(h),
            Err(_) => None,
        }
    }

    fn set_new_hemisphere(&mut self, mut new_hemi: Box<Hemisphere>) {
        log::info!("recieved hemisphere");

        // while we have the new one, go ahead and apply the
        // patches to make it up to date
        self.replay(&mut new_hemi);

        // Replace with the hemisphere from the
        // other subsystem
        self.a_hemi = Some(new_hemi);

        // Clear all patches
        self.a_global_patches.clear();
        self.a_client_patches.clear();
        self.a_window_patches.clear();
    }

    /// Try to exchange hemispheres between the two subsystems
    ///
    /// same as `flip_hemispheres`, but returns false if failed
    /// and could not recv.
    ///
    /// This is used by ways only
    pub fn try_flip_hemispheres(&mut self) -> bool {
        match self.recv_hemisphere() {
            Some(new_hemi) => {
                // Now that we got the hemi (from vkcomp), we
                // can send ours.
                self.send_hemisphere();
                self.set_new_hemisphere(new_hemi);

                true
            }
            None => false,
        }
    }

    /// Exchange hemispheres between the two subsystems
    ///
    /// This is in charge of sending and receiving hemisphere
    /// boxes over the channels. It also organizes the replays
    /// and clears the patches
    pub fn flip_hemispheres(&mut self) {
        self.send_hemisphere();
        // Loop while we retry recving from the
        loop {
            if let Some(new_hemi) = self.recv_hemisphere() {
                self.set_new_hemisphere(new_hemi);
                break;
            }
        }
    }

    /// This releases any resources that exist for only one frame, such
    /// as damage regions for a window. These per-frame data will be added
    /// in ways, propogated to vkcomp, and then released with this once the
    /// frame has completed
    pub fn release_consumables(&mut self) {
        self.a_hemi.as_mut().unwrap().reset_consumables();
    }

    /// Has the current hemisphere been changed
    ///
    /// Ways will use this to know if it should flip
    /// hemispheres and wake up vkcomp
    pub fn is_changed(&mut self) -> bool {
        match self.a_hemi.as_mut() {
            Some(h) => h.is_changed(),
            // If the hemisphere doesn't exist, we have sent ours
            // and are waiting for the other side, so say we
            // are changed so evman will keep calling recv
            None => true,
        }
    }

    fn mark_changed(&mut self) {
        self.a_hemi.as_mut().unwrap().mark_changed();
    }

    pub fn get_barsize(&self) -> f32 {
        self.get_resolution().1 as f32 * 0.02
    }

    // ------------------------------
    // For the sake of abstraction, the atmosphere will be the
    // point of contact for modifying global state. We will
    // record any changes to replay and pass the data down
    // to the hemisphere
    // ------------------------------

    /// Create a new window for the id
    ///
    /// This wraps a couple actions into one helper
    /// since there are multiple
    pub fn create_new_window(&mut self, id: WindowId, _owner: ClientId) {
        self.add_wm_task(wm::task::Task::create_window(id));
    }

    /// Reserve a new client id
    ///
    /// Should be done the first time we interact with
    /// a new client
    pub fn reserve_client_id(&mut self, id: ClientId) {
        self.set_client_in_use(id, true);
        self.set_windows_for_client(id, Vec::new());

        // For the priv maps we are activating and deactivating
        // the entries so we can use the iterator trait
        let ClientId(raw_id) = id;
        // Add a new priv entry
        // This is kept separately for ways
        self.a_client_priv
            .set(raw_id, ClientPriv::SEAT, &ClientPriv::seat(None));
    }

    /// Mark the specified id as in-use
    ///
    /// Ids are used as indexes for most of the vecs
    /// in the hemisphere, and we need to mark this as
    /// no longer available
    pub fn reserve_window_id(&mut self, client: ClientId, id: WindowId) {
        // first initialize all our properties
        self.set_window_in_use(id, true);
        self.set_owner(id, client);
        self.set_toplevel(id, false);
        self.set_window_pos(id, 0.0, 0.0);
        self.set_window_size(id, 0.0, 0.0);
        self.set_surface_pos(id, 0.0, 0.0);
        self.set_surface_size(id, 0.0, 0.0);
        self.set_skiplist_next(id, None);
        self.set_skiplist_prev(id, None);
        self.set_skiplist_skip(id, None);
        self.set_top_child(id, None);
        self.set_parent_window(id, None);
        self.set_root_window(id, None);

        // Add a new priv entry
        // This is kept separately for ways
        let WindowId(raw_id) = id;
        self.a_window_priv
            .set(raw_id, Priv::SURFACE, &Priv::surface(None));

        // We also need to notify the wm proc that we are creating
        // a window. There might be surface updates before we make it
        // visibile, and wm needs to track it.
        self.create_new_window(id, client);

        // TODO: optimize me
        // This is a bit too expensive atm
        let mut windows = self.get_windows_for_client(client);
        windows.push(id);
        self.set_windows_for_client(client, windows);
    }

    pub fn free_client_id(&mut self, id: ClientId) {
        // Free all windows belonging to this client
        let windows = self.get_windows_for_client(id);
        for win in windows.iter() {
            self.free_window_id(id, *win);
        }

        self.set_client_in_use(id, false);
        // For the priv maps we are activating and deactivating
        // the entries so we can use the iterator trait
        let ClientId(raw_id) = id;
        self.a_client_priv.deactivate(raw_id);
    }

    /// Mark the id as available
    pub fn free_window_id(&mut self, client: ClientId, id: WindowId) {
        // we also need to remove this surface from focus
        self.skiplist_remove_win_focus(id);
        self.skiplist_remove_surf_focus(id);
        // remove this id from the heirarchy
        self.skiplist_remove_window(id);

        // remove this window from the clients list
        // TODO: This is a bit too expensive atm
        let mut windows = self.get_windows_for_client(client);
        windows.retain(|&wid| wid != id);
        self.set_windows_for_client(client, windows);

        // free window id
        self.set_window_in_use(id, false);

        let WindowId(raw_id) = id;
        // Clear the private wayland rc data
        self.a_window_priv
            .set(raw_id, Priv::SURFACE, &Priv::surface(None));
        // We keep our own list of what ids are loaned out. Clear
        // this id in our list.
        // This needs to be last since calling `set` on a propmap will
        // activate the id.
        self.a_window_priv.deactivate(raw_id);
    }

    /// convert a global location to a surface local coordinates.
    /// Returns None if the location given is not over the surface
    pub fn global_coords_to_surf(&self, id: WindowId, x: f64, y: f64) -> Option<(f64, f64)> {
        // get the surface-local position
        let (wx, wy) = self.get_surface_pos(id);
        let (ww, wh) = self.get_surface_size(id);

        // offset into the surface
        let (sx, sy) = (x - wx as f64, y - wy as f64);

        // if the cursor is out of the valid bounds for the surface
        // offset, the cursor is not over this surface
        if sx < 0.0 || sy < 0.0 || sx >= ww as f64 || sy >= wh as f64 {
            return None;
        }
        return Some((sx, sy));
    }

    /// Adds a one-time task to the queue
    pub fn add_wm_task(&mut self, task: wm::task::Task) {
        self.a_hemi.as_mut().unwrap().add_wm_task(task);
    }

    /// pulls a one-time task off the queue
    pub fn get_next_wm_task(&mut self) -> Option<wm::task::Task> {
        self.a_hemi.as_mut().unwrap().wm_task_pop()
    }

    /// Set the damage for this surface
    /// This will be added once a frame, and then cleared before the next.
    pub fn set_surface_damage(&mut self, id: WindowId, damage: th::Damage) {
        self.a_hemi.as_mut().unwrap().set_surface_damage(id, damage)
    }
    /// For efficiency, this takes the damage so that we can avoid
    /// copying it
    pub fn take_surface_damage(&mut self, id: WindowId) -> Option<th::Damage> {
        self.a_hemi.as_mut().unwrap().take_surface_damage(id)
    }

    /// Set the damage for this window's buffer
    /// This is the same as set_surface_damage, but operates on buffer coordinates.
    /// It is the preferred method.
    pub fn set_buffer_damage(&mut self, id: WindowId, damage: th::Damage) {
        self.a_hemi.as_mut().unwrap().set_buffer_damage(id, damage)
    }
    /// For efficiency, this takes the damage so that we can avoid
    /// copying it
    pub fn take_buffer_damage(&mut self, id: WindowId) -> Option<th::Damage> {
        self.a_hemi.as_mut().unwrap().take_buffer_damage(id)
    }

    /// Add an offset to the cursor patch
    ///
    /// This increments the cursor position, which will later
    /// get replayed into the hemisphere
    pub fn add_cursor_pos(&mut self, dx: f64, dy: f64) {
        let pos = self.get_cursor_pos();
        self.set_cursor_pos(pos.0 + dx, pos.1 + dy);

        // Now update the grabbed window if it exists
        let grabbed = match self.get_grabbed() {
            Some(g) => g,
            None => return,
        };

        // Need to update both the surface and window positions
        let mut gpos = self.get_window_pos(grabbed);
        gpos.0 += dx as f32;
        gpos.1 += dy as f32;
        self.set_window_pos(grabbed, gpos.0, gpos.1);

        let mut gpos = self.get_surface_pos(grabbed);
        gpos.0 += dx as f32;
        gpos.1 += dy as f32;
        self.set_surface_pos(grabbed, gpos.0, gpos.1);
    }

    // -- subsystem specific handlers --

    /// These are getters for the private wayland structures
    /// that do not get shared across hemispheres
    pub fn add_surface(&mut self, id: WindowId, surf: Rc<RefCell<Surface>>) {
        let WindowId(raw_id) = id;
        self.a_window_priv
            .set(raw_id, Priv::SURFACE, &Priv::surface(Some(surf)));
    }

    /// Grab our Surface struct for this id
    pub fn get_surface_from_id(&self, id: WindowId) -> Option<Rc<RefCell<Surface>>> {
        let WindowId(raw_id) = id;
        match self.a_window_priv.get(raw_id, Priv::SURFACE) {
            Some(Priv::surface(Some(s))) => Some(s.clone()),
            _ => None,
        }
    }

    /// Grab the wayland protocol object wl_surface for this id
    pub fn get_wl_surface_from_id(&self, id: WindowId) -> Option<wl_surface::WlSurface> {
        let WindowId(raw_id) = id;
        match self.a_window_priv.get(raw_id, Priv::WL_SURFACE) {
            Some(Priv::wl_surface(s)) => Some(s.clone()),
            _ => None,
        }
    }
    pub fn set_wl_surface(&mut self, id: WindowId, surf: wl_surface::WlSurface) {
        let WindowId(raw_id) = id;
        self.a_window_priv
            .set(raw_id, Priv::WL_SURFACE, &Priv::wl_surface(surf));
    }

    pub fn add_seat(&mut self, id: ClientId, seat: Rc<RefCell<Seat>>) {
        let ClientId(raw_id) = id;
        self.a_client_priv
            .set(raw_id, ClientPriv::SEAT, &ClientPriv::seat(Some(seat)));
    }

    pub fn get_seat_from_window_id(&self, id: WindowId) -> Option<Rc<RefCell<Seat>>> {
        // get the client id
        let owner = self.get_owner(id);
        self.get_seat_from_client_id(owner)
    }
    pub fn get_seat_from_client_id(&self, id: ClientId) -> Option<Rc<RefCell<Seat>>> {
        let ClientId(raw_id) = id;
        match self.a_client_priv.get(raw_id, ClientPriv::SEAT) {
            Some(ClientPriv::seat(Some(s))) => Some(s.clone()),
            _ => None,
        }
    }

    /// Signal any registered frame callbacks
    /// TODO: actually do optimizations
    ///
    /// Wayland uses these callbacks to tell apps when they should
    /// redraw themselves. If they aren't on screen we don't send
    /// the callback so it doesn't use the power.
    pub fn signal_frame_callbacks(&mut self) {
        // get each valid id in the mapping
        for id in self.a_window_priv.active_id_iter() {
            // get the refcell for the surface for this id
            if let Some(Priv::surface(Some(cell))) =
                self.a_window_priv.get(id as PropertyId, Priv::SURFACE)
            {
                let mut surf = cell.borrow_mut();
                if surf.s_frame_callbacks.len() > 0 {
                    // frame callbacks are signaled in the order that they
                    // were submitted in
                    if let Some(callback) = surf.s_frame_callbacks.pop_front() {
                        log::debug!("Firing frame callback {:?}", callback);
                        // frame callbacks return the current time
                        // in milliseconds.
                        callback.done(
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .expect("Error getting system time")
                                .as_millis() as u32,
                        );
                    }
                }
            }
        }
    }
}

/// One hemisphere of the bicameral atmosphere
///
/// The atmosphere is the global state, but it needs to be
/// simultaneously accessed by two threads. We have two
/// hemispheres, each of which is a entity component set
/// that holds the current state of the desktop(s).
///
/// It's like rcu done through double buffering. At the
/// end of each frame both threads synchronize and switch
/// hemispheres.
///
/// Each subsystem (ways and vkcomp) will possess one
/// hemisphere. ways will update its hemisphere and
/// vkcomp will construct a frame from its hemisphere
///
/// Following Abrash's advice of "know your data" I am
/// using a vector instead of a hashmap for the main table.
/// The "keys" (aka window ids) are offsets into the vec.
/// This is done since there are normally < 15 windows
/// open on any given desktop, and this is the largest
/// table so we are going for compactness. The offsets
/// still provide O(1) lookup time, with the downside
/// that we have to scan the vec to find a new entry,
/// and potentially resize the vec to fit a new one.
#[allow(dead_code)]
pub struct Hemisphere {
    /// Will be true if there is new data in this hemisphere,
    /// false if this hemi can be safely ignored
    h_has_changed: bool,
    /// The property database for our ECS
    h_global_props: PropertyMap<GlobalProperty>,
    h_client_props: PropertyMap<ClientProperty>,
    h_window_props: PropertyMap<WindowProperty>,
    /// A list of tasks to be completed by vkcomp this frame
    /// - does not need to be patched
    ///
    /// Tasks are one time events. Anything related to state should
    /// be added elsewhere. A task is a transfer of ownership from
    /// ways to vkcommp
    h_wm_tasks: VecDeque<wm::task::Task>,
    h_surf_damages: PropertyList<th::Damage>,
    h_damages: PropertyList<th::Damage>,
}

impl Hemisphere {
    fn new() -> Hemisphere {
        Hemisphere {
            h_has_changed: true,
            h_global_props: PropertyMap::new(),
            h_client_props: PropertyMap::new(),
            h_window_props: PropertyMap::new(),
            // These are added to a hemisphere by one side,
            // and are consumed by the other
            // They are not patched
            h_wm_tasks: VecDeque::new(),
            h_surf_damages: PropertyList::new(),
            h_damages: PropertyList::new(),
        }
    }

    /// Apply a patch to this hemisphere
    /// This is used to commit a changeset
    ///
    /// Changes are accrued in the patch list. Before
    /// flipping hemispheres we will apply the patch
    /// list to the current hemisphere, and then again
    /// to the new one to keep things up to date.
    fn set_global_prop(&mut self, id: PropertyId, prop: &GlobalProperty) {
        self.mark_changed();
        // for global properties just always pass the id as 0
        // since we don't care about window/client indexing
        self.h_global_props.set(0, id, prop);
    }

    fn get_global_prop(&self, id: PropertyId) -> Option<&GlobalProperty> {
        self.h_global_props.get(0, id)
    }

    fn set_client_prop(&mut self, client: ClientId, id: PropertyId, prop: &ClientProperty) {
        self.mark_changed();
        // for global properties just always pass the id as 0
        // since we don't care about window/client indexing
        let ClientId(raw_client) = client;
        self.h_client_props.set(raw_client, id, prop);
    }

    fn get_client_prop(&self, client: ClientId, id: PropertyId) -> Option<&ClientProperty> {
        let ClientId(raw_client) = client;
        self.h_client_props.get(raw_client, id)
    }

    fn set_window_prop(&mut self, win: WindowId, id: PropertyId, prop: &WindowProperty) {
        self.mark_changed();
        // for global properties just always pass the id as 0
        // since we don't care about window/client indexing
        let WindowId(raw_win) = win;
        self.h_window_props.set(raw_win, id, prop);
    }

    fn get_window_prop(&self, win: WindowId, id: PropertyId) -> Option<&WindowProperty> {
        let WindowId(raw_win) = win;
        self.h_window_props.get(raw_win, id)
    }

    /// This should be called after all patches are applied
    /// and signifies that we have brought this hemisphere
    /// up to date (minus the cursor, which this applies)
    fn commit(&mut self) {
        // clear the changed flag
        self.h_has_changed = false;
    }

    fn reset_consumables(&mut self) {
        self.h_surf_damages.clear();
        self.h_damages.clear();
    }

    fn is_changed(&self) -> bool {
        self.h_has_changed
    }

    fn mark_changed(&mut self) {
        self.h_has_changed = true;
    }

    // ----------------
    // modifiers
    // ----------------

    fn add_wm_task(&mut self, task: wm::task::Task) {
        self.mark_changed();
        self.h_wm_tasks.push_back(task);
    }

    pub fn wm_task_pop(&mut self) -> Option<wm::task::Task> {
        self.mark_changed();
        self.h_wm_tasks.pop_front()
    }

    fn set_surface_damage(&mut self, id: WindowId, damage: th::Damage) {
        self.mark_changed();
        self.h_surf_damages.update_or_create(id.into(), damage)
    }
    fn take_surface_damage(&mut self, id: WindowId) -> Option<th::Damage> {
        if self.h_surf_damages.id_exists(id.into()) {
            return self.h_surf_damages[id.into()].take();
        }
        return None;
    }

    fn set_buffer_damage(&mut self, id: WindowId, damage: th::Damage) {
        self.mark_changed();
        self.h_damages.update_or_create(id.into(), damage)
    }
    fn take_buffer_damage(&mut self, id: WindowId) -> Option<th::Damage> {
        if self.h_damages.id_exists(id.into()) {
            return self.h_damages[id.into()].take();
        }
        return None;
    }
}
