// Global atmosphere
//
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate wayland_server as ws;
use ws::protocol::wl_surface;

mod property;
use property::{PropertyId,Property};
mod property_map;
use property_map::PropertyMap;
mod skiplist;

use crate::category5::ways::{
    surface::*,
    seat::Seat,
    xdg_shell::xdg_toplevel::ResizeEdge,
};
use crate::category5::vkcomp::wm;
use super::{WindowId,ClientId};
use crate::category5::utils::{
    timing::*, logging::LogLevel,
};
use crate::log;

use std::rc::Rc;
use std::vec::Vec;
use std::cell::RefCell;
use std::collections::{HashMap,VecDeque};
use std::sync::mpsc::{Sender,Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

// Global data not tied to a client or window
//
// See `Property` for implementation comments
#[derive(Copy, Clone, Debug)]
enum GlobalProperty {
    // !! IF YOU CHANGE THIS UPDATE property_count BELOW!!

    cursor_pos(f64, f64),
    resolution(u32, u32),
    grabbed(Option<WindowId>),
    resizing(Option<WindowId>),
    // the window the user is currently interacting with
    // This tells us which one to start looking at for the skiplist
    focus(Option<WindowId>),
}

// Declare constants for the property ids. This prevents us
// from having to make an instance of the enum that we would
// have to call get_property_id on
impl GlobalProperty {
    const CURSOR_POS: PropertyId = 0;
    const RESOLUTION: PropertyId = 1;
    const GRABBED: PropertyId = 2;
    const RESIZING: PropertyId = 3;
    const FOCUS: PropertyId = 4;
    // MUST be the last one
    const VARIANT_LEN: PropertyId = 5;
}

impl Property for GlobalProperty {
    // Get a unique Id
    fn get_property_id(&self) -> PropertyId {
        match self {
            Self::cursor_pos(_, _) => Self::CURSOR_POS,
            Self::resolution(_, _) => Self::RESOLUTION,
            Self::grabbed(_) => Self::GRABBED,
            Self::resizing(_) => Self::RESIZING,
            Self::focus(_) => Self::FOCUS,
        }
    }

    fn variant_len() -> u32 {
        return Self::VARIANT_LEN as u32;
    }
}

// These are indexed by ClientId
#[derive(Clone, Debug)]
enum ClientProperty {
    // is this id in use?
    in_use(bool),
    // window ids belonging to this client
    windows(Vec<WindowId>),
}

impl ClientProperty {
    const IN_USE: PropertyId = 0;
    const WINDOWS: PropertyId = 1;
    const VARIANT_LEN: PropertyId = 2;
}

impl Property for ClientProperty {
    // Get a unique Id
    fn get_property_id(&self) -> PropertyId {
        match self {
            Self::in_use(_) => Self::IN_USE,
            Self::windows(_)=> Self::WINDOWS,
        }
    }

    fn variant_len() -> u32 {
        return Self::VARIANT_LEN as u32;
    }
}

// These are indexed by WindowId
#[derive(Clone, Debug)]
enum WindowProperty {
    // is this id in use?
    in_use(bool),
    // The client that created this window
    owner(ClientId),
    // does this window have the toplevel role
    // this controls if SSD are drawn
    toplevel(bool),
    // (x, y, width, height)
    // If this window is a subsurface, then x and y will
    // be offsets from the base of the parent window
    window_dimensions(f32, f32, f32, f32),
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
}

impl WindowProperty {
    const IN_USE: PropertyId = 0;
    const OWNER: PropertyId = 1;
    const TOPLEVEL: PropertyId = 2;
    const WINDOW_DIMENSIONS: PropertyId = 3;
    const SKIPLIST_NEXT: PropertyId = 4;
    const SKIPLIST_PREV: PropertyId = 5;
    const SKIPLIST_SKIP: PropertyId = 6;
    const TOP_CHILD: PropertyId = 7;
    const PARENT_WINDOW: PropertyId = 8;
    const VARIANT_LEN: PropertyId = 9;
}

impl Property for WindowProperty {
    // Get a unique Id
    fn get_property_id(&self) -> PropertyId {
        match self {
            Self::in_use(_) => Self::IN_USE,
            Self::owner(_) => Self::OWNER,
            Self::toplevel(_)=> Self::TOPLEVEL,
            Self::window_dimensions(_,_,_,_)=> Self::WINDOW_DIMENSIONS,
            Self::skiplist_next(_)=> Self::SKIPLIST_NEXT,
            Self::skiplist_prev(_)=> Self::SKIPLIST_PREV,
            Self::skiplist_skip(_)=> Self::SKIPLIST_SKIP,
            Self::top_child(_)=> Self::TOP_CHILD,
            Self::parent_window(_)=> Self::PARENT_WINDOW,
        }
    }

    fn variant_len() -> u32 {
        return Self::VARIANT_LEN as u32;
    }
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

// Global state tracking
//
// Don't make fun of my naming convention pls. We need a
// place for all wayland code to stash meta information.
// This is such a place, but it should not hold anything
// exceptionally protocol-specific for sync reasons.
//
// Although this is referenced everywhere, both sides
// will have their own version of it. a_hemisphere is
// the shared part.
//
// Keep in mind this only holds any shared data, data
// exclusive to subsystems will be held by said subsystem
#[allow(dead_code)]
pub struct Atmosphere {
    // transfer channel
    a_tx: Sender<Box<Hemisphere>>,
    // receive channel
    a_rx: Receiver<Box<Hemisphere>>,
    // The current hemisphere
    //
    // While this is held we will retain ownership of the
    // current hemisphere's mutex, and be able to service
    // requests on this hemisphere
    //
    // To switch hemispheres we send this through the
    // channel to the other subsystem.
    a_hemi: Option<Box<Hemisphere>>,

    // -- private subsystem specific resources --

    // These keep track of what ids we have handed out to the
    // property maps. We need to track this here since we are
    // patching the prop maps and can't rely on them
    a_client_id_map: Vec<bool>,
    a_window_id_map: Vec<bool>,

    // -- ways --
    
    a_window_priv: PropertyMap<Priv>,
    a_client_priv: PropertyMap<ClientPriv>,
    // A hashmap of patches based on the window id and the
    // property name
    //
    // This needs to be a hashmap since we want to quickly
    // update information. Searching would take too long.
    //
    // ways performs patch replay:
    // Changes will be accrued in a batch here during hemisphere
    // construction, and will then be applied before flipping.
    // when receiving the other hemisphere, first replay all
    // patches before constructing a new changeset.
    a_window_patches: HashMap<(WindowId, PropertyId), WindowProperty>,
    a_client_patches: HashMap<(ClientId, PropertyId), ClientProperty>,
    a_global_patches: HashMap<PropertyId, GlobalProperty>,
}

impl Atmosphere {
    // Create a new atmosphere to be shared within a subsystem
    //
    // We pass in the hemispheres and lock(s) since they will have to
    // also be passed to the other subsystem.
    // One subsystem must be setup as index 0 and the other
    // as index 1
    pub fn new(tx: Sender<Box<Hemisphere>>,
               rx: Receiver<Box<Hemisphere>>)
               -> Atmosphere
    {
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

        return atmos;
    }

    // Gets the next available id in a vec of bools
    // generic id getter
    fn get_next_id(v: &mut Vec<bool>) -> u32 {
        for (i, in_use) in v.iter_mut().enumerate() {
            if !*in_use {
                *in_use = true;
                return i as u32;
            }
        }

        v.push(true);
        return (v.len() - 1) as u32;
    }

    // Get the next id
    //
    // Find a free id if one is available, if not then
    // add a new one
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

    // Add a patch to be replayed on a flip
    //
    // All changes to the current hemisphere will get
    // batched up into a set of patches. This is needed to
    // keep both hemispheres in sync.
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

    fn get_global_prop(&self, prop_id: PropertyId)
                       -> Option<&GlobalProperty>
    {
        // check if there is an existing patch to grab
        if let Some(v) = self.a_global_patches.get(&prop_id) {
            return Some(v);
        }
        return self.a_hemi.as_ref().unwrap()
            .get_global_prop(prop_id);
    }

    fn set_client_prop(&mut self, client: ClientId, value: &ClientProperty) {
        self.mark_changed();
        let prop_id = value.get_property_id();
        // check if there is an existing patch to overwrite
        if let Some(v) = self.a_client_patches.get_mut(&(client, prop_id)) {
            // if so, just update it
            *v = value.clone();
        } else {
            self.a_client_patches.insert((client, prop_id), value.clone());
        }
    }

    fn get_client_prop(&self, client: ClientId, prop_id: PropertyId)
                       -> Option<&ClientProperty>
    {
        // check if there is an existing patch to grab
        if let Some(v) = self.a_client_patches.get(&(client, prop_id)) {
            return Some(v);
        }
        return self.a_hemi.as_ref().unwrap()
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

    fn get_window_prop(&self, id: WindowId, prop_id: PropertyId)
                       -> Option<&WindowProperty>
    {
        // check if there is an existing patch to grab
        if let Some(v) = self.a_window_patches.get(&(id, prop_id)) {
            return Some(v);
        }
        return self.a_hemi.as_ref().unwrap()
            .get_window_prop(id, prop_id);
    }

    // Commit all our patches into the hemisphere
    //
    // We are batching all the changes into patches. We
    // then need to apply those patches to the current
    // hemisphere before we send it to update it. We also
    // need to replay the patches on the new hemisphere to
    // update it will all the info it's missing
    fn replay(&mut self, hemi: &mut Hemisphere) {
        log!(LogLevel::info, "replaying on hemisphere");
        for (prop_id, prop) in self.a_global_patches.iter() {
            log!(LogLevel::info, "   replaying {:?}", prop);
            hemi.set_global_prop(*prop_id, prop);
        }
        for ((id, prop_id), prop) in self.a_client_patches.iter() {
            log!(LogLevel::info, "   replaying {:?}", prop);
            hemi.set_client_prop(*id, *prop_id, prop);
        }
        for ((id, prop_id), prop) in self.a_window_patches.iter() {
            log!(LogLevel::info, "   replaying {:?}", prop);
            hemi.set_window_prop(*id, *prop_id, prop);
        }

        // Apply any remaining constant state like cursor
        // positions
        hemi.commit();
    }

    // Must be called before recv_hemisphere
    pub fn send_hemisphere(&mut self) {
        // first grab our own hemi
        if let Some(mut h) = self.a_hemi.take() {
            log!(LogLevel::info, "sending hemisphere");
            // second, we need to apply our changes to
            // our own hemisphere before we send it
            self.replay(&mut h);

            // actually flip hemispheres
            self.a_tx.send(h)
                .expect("Could not send hemisphere");
        }
    }

    // Must be called after send_hemisphere
    // returns true if we were able to get the other hemisphere
    // if returns false, this needs to be called again
    pub fn recv_hemisphere(&mut self) -> bool {
        log!(LogLevel::info, "trying to recv hemisphere");
        let mut new_hemi = match self.a_rx.recv() {
            Ok(h) => h,
            Err(_) => return false,
        };
        log!(LogLevel::info, "recieved hemisphere");

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

        return true;
    }

    // Exchange hemispheres between the two subsystems
    //
    // This is in charge of sending and receiving hemisphere
    // boxes over the channels. It also organizes the replays
    // and clears the patches
    pub fn flip_hemispheres(&mut self) {
        self.send_hemisphere();
        self.recv_hemisphere();
    }

    // Has the current hemisphere been changed
    //
    // Ways will use this to know if it should flip
    // hemispheres and wake up vkcomp
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
        self.a_hemi.as_mut().map(|h| h.mark_changed());
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

    // Create a new window for the id
    //
    // This wraps a couple actions into one helper
    // since there are multiple 
    pub fn create_new_window(&mut self,
                             id: WindowId,
                             owner: ClientId,
                             is_toplevel: bool)
    {
        self.add_wm_task(
            wm::task::Task::create_window(id)
        );

        let barsize = self.get_barsize();

        self.set_owner(id, owner);
        self.set_window_prop(id, &WindowProperty::in_use(true));
        self.set_window_prop(id, &WindowProperty::toplevel(is_toplevel));
        self.set_window_prop(id, &WindowProperty::window_dimensions(
            0.0, 0.0 + barsize, // (x, y)
            640.0, 480.0 // (width, height)
        ));

        // make this the new toplevel window
        self.focus_on(Some(id));
    }

    // Reserve a new client id
    //
    // Should be done the first time we interact with
    // a new client
    pub fn reserve_client_id(&mut self, id: ClientId) {
        self.set_client_prop(id, &ClientProperty::in_use(true));

        // For the priv maps we are activating and deactivating
        // the entries so we can use the iterator trait
        let ClientId(raw_id) = id;
        self.a_client_priv.activate(raw_id);
        // Add a new priv entry
        // This is kept separately for ways
        self.a_client_priv.set(raw_id,
                               ClientPriv::SEAT,
                               &ClientPriv::seat(None));
    }

    // Mark the specified id as in-use
    //
    // Ids are used as indexes for most of the vecs
    // in the hemisphere, and we need to mark this as
    // no longer available
    pub fn reserve_window_id(&mut self, client: ClientId, id: WindowId) {
        let WindowId(raw_id) = id;
        // Add a new priv entry
        // For the priv maps we are activating and deactivating
        // the entries so we can use the iterator trait
        self.a_window_priv.activate(raw_id);
        // Add a new priv entry
        // This is kept separately for ways
        self.a_window_priv.set(raw_id,
                               Priv::SURFACE,
                               &Priv::surface(None));

        // This is a bit too expensive atm
        let mut windows = self.get_windows_for_client(client);
        windows.push(id);
        self.set_client_prop(client, &ClientProperty::windows(windows));
    }

    pub fn free_client_id(&mut self, id: ClientId) {
        // Free all windows belonging to this client
        let windows = self.get_windows_for_client(id);
        for win in windows.iter() {
            self.free_window_id(id, *win);
        }

        self.set_client_prop(id, &ClientProperty::in_use(false));
        // For the priv maps we are activating and deactivating
        // the entries so we can use the iterator trait
        let ClientId(raw_id) = id;
        self.a_client_priv.deactivate(raw_id);
    }

    // Mark the id as available
    pub fn free_window_id(&mut self, client: ClientId, id: WindowId) {
        // remove this id from the heirarchy
        self.skiplist_remove_window(id);

        // remove this window from the clients list
        // TODO: This is a bit too expensive atm
        let mut windows = self.get_windows_for_client(client);
        windows.retain(|&wid| wid != id);
        self.set_client_prop(client, &ClientProperty::windows(windows));

        // free window id
        self.set_window_prop(id, &WindowProperty::in_use(false));
        // Clear the private wayland rc data
        let WindowId(raw_id) = id;
        self.a_window_priv.set(raw_id, Priv::SURFACE,
                               &Priv::surface(None));
    }

    // Get the windows ids belonging this client
    pub fn get_windows_for_client(&self, id: ClientId) -> Vec<WindowId> {
        match self.get_client_prop(id, ClientProperty::WINDOWS) {
            Some(ClientProperty::windows(v)) => v.clone(),
            None => Vec::new(),
            _ => panic!("property not found"),
        }
    }

    // this is one of the few updates from vkcomp
    pub fn set_resolution(&mut self, x: u32, y: u32) {
        self.set_global_prop(&GlobalProperty::resolution(x, y));
    }

    // Get the screen resolution as set by vkcomp
    pub fn get_resolution(&self) -> (u32, u32) {
        match self.get_global_prop(GlobalProperty::RESOLUTION) {
            Some(GlobalProperty::resolution(x, y)) => (*x, *y),
            _ => panic!("Could not find value for property"),
        }
    }

    // Get the screen resolution as set by vkcomp
    pub fn is_toplevel(&self, id: WindowId) -> bool {
        match self.get_window_prop(id, WindowProperty::TOPLEVEL) {
            Some(WindowProperty::toplevel(b)) => *b,
            None => false,
            _ => panic!("Could not find value for property"),
        }
    }

    // Get the owning client of a surface
    pub fn get_owner(&self, id: WindowId) -> ClientId {
        match self.get_window_prop(id, WindowProperty::OWNER) {
            Some(WindowProperty::owner(o)) => *o,
            _ => panic!("Could not find value for property"),
        }
    }

    // Set the owning client of a surface
    pub fn set_owner(&mut self, id: WindowId, owner: ClientId) {
        self.set_window_prop(id, &WindowProperty::owner(owner));
    }

    // Grab the window by the specified id
    // it will get moved around as the cursor does
    pub fn grab(&mut self, id: WindowId) {
        self.set_global_prop(&GlobalProperty::grabbed(Some(id)));
    }

    pub fn ungrab(&mut self) {
        self.set_global_prop(&GlobalProperty::grabbed(None));
    }

    pub fn get_grabbed(&self) -> Option<WindowId> {
        match self.get_global_prop(GlobalProperty::GRABBED) {
            Some(GlobalProperty::grabbed(id)) => *id,
            None => None,
            _ => panic!("Could not find value for property"),
        }
    }

    // Get and set the window that is currently being resized
    // This is used by both input and xdg shell
    pub fn set_resizing(&mut self, id: Option<WindowId>) {
        self.set_global_prop(&GlobalProperty::resizing(id));
    }

    pub fn get_resizing(&self) -> Option<WindowId> {
        match self.get_global_prop(GlobalProperty::RESIZING) {
            Some(GlobalProperty::resizing(id)) => *id,
            None => None,
            _ => panic!("Could not find value for property"),
        }
    }

    // Find if there is a toplevel window under (x,y)
    //
    // This is used first to find if the cursor intersects
    // with a window. If it does, point_is_on_titlebar is
    // used to check for a grab or relay input event.
    pub fn find_window_at_point(&self, x: f32, y: f32)
                                -> Option<WindowId>
    {
        let barsize = self.get_barsize();

        for win in self.visible_windows() {
            let pos = self.get_window_dimensions(win);

            // If this window contains (x, y) then return it
            if x > pos.0 && y > (pos.1 - barsize)
                && x < (pos.0 + pos.2)
                && y < (pos.1 + pos.3)
            {
                return Some(win);
            }
        }
        return None;
    }

    
    // Is the current point over the titlebar of the window
    //
    // Id should have first been found with find_window_at_point
    pub fn point_is_on_titlebar(&self, id: WindowId, x: f32, y: f32)
                          -> bool
    {
        let barsize = self.get_barsize();
        let pos = self.get_window_dimensions(id);

        // If this window contains (x, y) then return it
        if x > pos.0 && y > (pos.1 - barsize)
            && x < (pos.0 + pos.2)
            && y < pos.1
        {
            return true;
        }
        return false;
    }

    // convert a global location to a surface local coordinates.
    // Returns None if the location given is not over the surface
    pub fn global_coords_to_surf(&self, id: WindowId,  x: f64, y: f64)
                                 -> Option<(f64, f64)>
    {
        // get the surface-local position
        let (wx, wy, ww, wh) = self.get_window_dimensions(id);

        // offset into the surface
        let (sx, sy) = (x - wx as f64, y - wy as f64);

        // if the cursor is out of the valid bounds for the surface
        // offset, the cursor is not over this surface
        if sx < 0.0 || sy < 0.0 || sx >= ww as f64 || sy >= wh as f64 {
            return None;
        }
        return Some((sx, sy));
    }

    // calculates if a position is over the part of a window that
    // procs a resize
    pub fn point_is_on_window_edge(&self, id: WindowId, x: f32, y: f32)
                                   -> ResizeEdge
    {
        let barsize = self.get_barsize();
        let (wx, wy, ww, wh) = self.get_window_dimensions(id);
        let prox = 3.0; // TODO find a better val for this??

        // is (x,y) inside each dimension of the window
        let x_contained = x > wx && x < wx + ww;
        let y_contained = y > wy && y < wy + barsize + wh;

        // closures for helping us with overlap calculations
        // v is val to check, a is axis location
        let near_edge = |p, a| {
            p > (a - prox) && p < (a + prox)
        };
        // same thing but for corners
        // v is the point and c is the corner
        let near_corner = |vx, vy, cx, cy| {
            near_edge(vx, cx) && near_edge(vy, cy)
        };

        // first check if we are over a corner
        if near_corner(x, y, wx, wy) {
             ResizeEdge::TopLeft
        } else if near_corner(x, y, wx + ww, wy) {
            ResizeEdge::TopRight
        } else if near_corner(x, y, wx, wy + wh) {
            ResizeEdge::BottomLeft
        } else if near_corner(x, y, wx + ww, wy + wh) {
            ResizeEdge::BottomRight
        } else if near_edge(x, wx) && y_contained {
            ResizeEdge::Left
        } else if near_edge(x, wx + ww) && y_contained {
            ResizeEdge::Right
        } else if near_edge(y, wy) && x_contained {
            ResizeEdge::Top
        } else if near_edge(y, wy + wh) && x_contained {
            ResizeEdge::Bottom
        } else {
            ResizeEdge::None
        }
    }

    // Adds a one-time task to the queue
    pub fn add_wm_task(&mut self, task: wm::task::Task) {
        self.a_hemi.as_mut().map(|h| h.add_wm_task(task));
    }

    // pulls a one-time task off the queue
    pub fn get_next_wm_task(&mut self) -> Option<wm::task::Task> {
        self.a_hemi.as_mut().unwrap().wm_task_pop()
    }

    // Add an offset to the cursor patch
    //
    // This increments the cursor position, which will later
    // get replayed into the hemisphere
    pub fn add_cursor_pos(&mut self, dx: f64, dy: f64) {
        let pos = self.get_cursor_pos();
        self.set_global_prop(&GlobalProperty::cursor_pos(
            pos.0 + dx, pos.1 + dy,
        ));

        // Now update the grabbed window if it exists
        let grabbed = match self.get_grabbed() {
            Some(g) => g,
            None => return,
        };

        let mut gpos = self.get_window_dimensions(grabbed);
        gpos.0 += dx as f32;
        gpos.1 += dy as f32;

        self.set_window_dimensions(grabbed, gpos.0, gpos.1,
                                   gpos.2, gpos.3);
    }

    pub fn set_cursor_pos(&mut self, x: f64, y: f64) {
        self.set_global_prop(&GlobalProperty::cursor_pos(x, y));
    }

    pub fn get_cursor_pos(&self) -> (f64, f64) {
        match self.get_global_prop(GlobalProperty::CURSOR_POS)
        {
            Some(GlobalProperty::cursor_pos(x, y)) => (*x, *y),
            _ => panic!("Could not find value for property"),
        }
    }

    pub fn is_in_use(&self, id: WindowId) -> bool {
        match self.get_window_prop(id, WindowProperty::IN_USE)
        {
            Some(WindowProperty::in_use(b)) => *b,
            None => false,
            _ => panic!("Could not find value for property"),
        }
    }

    // Get the window dimensions
    // grab them from the patchmap first, and fetch them from the
    // hemisphere if they aren't currently patched.
    pub fn get_window_dimensions(&self, id: WindowId)
                                 -> (f32, f32, f32, f32)
    {
        match self.get_window_prop(id, WindowProperty::WINDOW_DIMENSIONS)
        {
            Some(WindowProperty::window_dimensions(x, y, w, h)) => (*x, *y, *w, *h),
            _ => panic!("Could not find value for property"),
        }
    }

    // Set the dimensions of the window
    //
    // This includes the base coordinate, plus the width and height
    pub fn set_window_dimensions(&mut self,
                                 id: WindowId,
                                 x: f32,
                                 y: f32,
                                 width: f32,
                                 height: f32)
    {
        self.set_window_prop(id,
                             &WindowProperty::window_dimensions(
                                 x, y, width, height,
                             ));
    }

    // Set the window size.
    // This is a helper for calling get and set on the window dims
     pub fn set_window_size(&mut self,
                            id: WindowId,
                            width: f32,
                            height: f32)
    {
        let mut pos = self.get_window_dimensions(id);
        pos.2 = width;
        pos.3 = height;
        self.set_window_dimensions(id,
                                   pos.0, pos.1,
                                   pos.2, pos.3);
    }

    // -- subsystem specific handlers --

    // These are getters for the private wayland structures
    // that do not get shared across hemispheres
    pub fn add_surface(&mut self, id: WindowId,
                       surf: Rc<RefCell<Surface>>)
    {
        let WindowId(raw_id) = id;
        self.a_window_priv.set(raw_id,
                               Priv::SURFACE,
                               &Priv::surface(Some(surf)));
    }

    // Grab our Surface struct for this id
    pub fn get_surface_from_id(&self, id: WindowId)
                               -> Option<Rc<RefCell<Surface>>>
    {
        let WindowId(raw_id) = id;
        match self.a_window_priv.get(raw_id, Priv::SURFACE) {
            Some(Priv::surface(Some(s))) => Some(s.clone()),
            _ => None,
        }
    }

    // Grab the wayland protocol object wl_surface for this id
    pub fn get_wl_surface_from_id(&self, id: WindowId)
                                  -> Option<wl_surface::WlSurface>
    {
        let WindowId(raw_id) = id;
        match self.a_window_priv.get(raw_id, Priv::WL_SURFACE) {
            Some(Priv::wl_surface(s)) => Some(s.clone()),
            _ => None,
        }
    }
    pub fn set_wl_surface(&mut self,
                          id: WindowId,
                          surf: wl_surface::WlSurface)
    {
        let WindowId(raw_id) = id;
        self.a_window_priv.set(raw_id,
                               Priv::WL_SURFACE,
                               &Priv::wl_surface(surf));
    }

    pub fn add_seat(&mut self, id: ClientId,
                    seat: Rc<RefCell<Seat>>)
    {
        let ClientId(raw_id) = id;
        self.a_client_priv.set(raw_id,
                               ClientPriv::SEAT,
                               &ClientPriv::seat(Some(seat)));
    }

    pub fn get_seat_from_window_id(&self, id: WindowId)
                                   -> Option<Rc<RefCell<Seat>>>
    {
        // get the client id
        let owner = self.get_owner(id);
        self.get_seat_from_client_id(owner)
    }
    pub fn get_seat_from_client_id(&self, id: ClientId)
                                   -> Option<Rc<RefCell<Seat>>>
    {
        let ClientId(raw_id) = id;
        match self.a_client_priv.get(raw_id, ClientPriv::SEAT) {
            Some(ClientPriv::seat(Some(s))) => Some(s.clone()),
            _ => None,
        }
    }

    // Signal any registered frame callbacks
    // TODO: actually do optimizations
    //
    // Wayland uses these callbacks to tell apps when they should
    // redraw themselves. If they aren't on screen we don't send
    // the callback so it doesn't use the power.
    pub fn signal_frame_callbacks(&mut self) {
        // get each valid id in the mapping
        for id in self.a_window_priv.active_id_iter() {
            // get the refcell for the surface for this id
            if let Some(Priv::surface(Some(cell))) = self.a_window_priv
                .get(id, Priv::SURFACE)
            {
                let surf = cell.borrow_mut();
                if let Some(callback) = surf.s_frame_callback.as_ref() {
                    // frame callbacks return the current time
                    // in milliseconds.
                    callback.done(SystemTime::now()
                                  .duration_since(UNIX_EPOCH)
                                  .expect("Error getting system time")
                                  .as_millis() as u32);
                }
            }
        }
    }
}

// One hemisphere of the bicameral atmosphere
//
// The atmosphere is the global state, but it needs to be
// simultaneously accessed by two threads. We have two
// hemispheres, each of which is a entity component set
// that holds the current state of the desktop(s).
//
// It's like rcu done through double buffering. At the
// end of each frame both threads synchronize and switch
// hemispheres.
//
// Each subsystem (ways and vkcomp) will possess one
// hemisphere. ways will update its hemisphere and
// vkcomp will construct a frame from its hemisphere
//
// Following Abrash's advice of "know your data" I am
// using a vector instead of a hashmap for the main table.
// The "keys" (aka window ids) are offsets into the vec.
// This is done since there are normally < 15 windows
// open on any given desktop, and this is the largest
// table so we are going for compactness. The offsets
// still provide O(1) lookup time, with the downside
// that we have to scan the vec to find a new entry,
// and potentially resize the vec to fit a new one.
#[allow(dead_code)]
pub struct Hemisphere {
    // Will be true if there is new data in this hemisphere,
    // false if this hemi can be safely ignored
    h_has_changed: bool,
    // The property database for our ECS
    h_global_props: PropertyMap<GlobalProperty>,
    h_client_props: PropertyMap<ClientProperty>,
    h_window_props: PropertyMap<WindowProperty>,
    // A list of tasks to be completed by vkcomp this frame
    // - does not need to be patched
    //
    // Tasks are one time events. Anything related to state should
    // be added elsewhere. A task is a transfer of ownership from
    // ways to vkcommp
    h_wm_tasks: VecDeque<wm::task::Task>,
}


impl Hemisphere {
    fn new() -> Hemisphere {
        Hemisphere {
            h_has_changed: true,
            h_global_props: PropertyMap::new(),
            h_client_props: PropertyMap::new(),
            h_window_props: PropertyMap::new(),
            h_wm_tasks: VecDeque::new(),
        }
    }

    // Apply a patch to this hemisphere
    // This is used to commit a changeset
    //
    // Changes are accrued in the patch list. Before
    // flipping hemispheres we will apply the patch
    // list to the current hemisphere, and then again
    // to the new one to keep things up to date.
    fn set_global_prop(&mut self,
                       id: PropertyId,
                       prop: &GlobalProperty)
    {
        self.mark_changed();
        // for global properties just always pass the id as 0
        // since we don't care about window/client indexing
        self.h_global_props.set(0, id, prop);
    }

    fn get_global_prop(&self, id: PropertyId)
                       -> Option<&GlobalProperty>
    {
        self.h_global_props.get(0, id)
    }

    fn set_client_prop(&mut self,
                       client: ClientId,
                       id: PropertyId,
                       prop: &ClientProperty)
    {
        self.mark_changed();
        // for global properties just always pass the id as 0
        // since we don't care about window/client indexing
        let ClientId(raw_client) = client;
        self.h_client_props.set(raw_client, id, prop);
    }

    fn get_client_prop(&self, client: ClientId, id: PropertyId)
                       -> Option<&ClientProperty>
    {
        let ClientId(raw_client) = client;
        self.h_client_props.get(raw_client, id)
    }

    fn set_window_prop(&mut self,
                       win: WindowId,
                       id: PropertyId,
                       prop: &WindowProperty)
    {
        self.mark_changed();
        // for global properties just always pass the id as 0
        // since we don't care about window/client indexing
        let WindowId(raw_win) = win;
        self.h_window_props.set(raw_win, id, prop);
    }

    fn get_window_prop(&self, win: WindowId, id: PropertyId)
                       -> Option<&WindowProperty>
    {
        let WindowId(raw_win) = win;
        self.h_window_props.get(raw_win, id)
    }

    // This should be called after all patches are applied
    // and signifies that we have brought this hemisphere
    // up to date (minus the cursor, which this applies)
    fn commit(&mut self) {
        // clear the changed flag
        self.h_has_changed = false;
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
}
