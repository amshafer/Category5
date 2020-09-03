// Global atmosphere
//
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate wayland_server as ws;

use crate::category5::ways::{
    surface::*,
    seat::Seat,
};
use crate::category5::vkcomp::wm;
use super::WindowId;
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

// Different shared property ids in the ECS
//
// We use this to show which property will be
// updated by an action. All hashmaps are indexed
// using the window id, and therefore another
// method is needed to identify the property to update
#[derive(PartialEq, Eq, Hash, Copy, Clone)]
enum Property {
    RESERVE_WINDOW_ID,
    FREE_WINDOW_ID,
    FOCUS_ON_ID,
    ADD_NEW_TOPLEVEL,
    SET_WINDOW_DIMENSIONS,
}

// Represents updating one property in the ECS
//
// Our atmosphere is really just a lock-free Entity
// component set. We need a way to snapshot the
// changes accummulated in a hemisphere during a frame
// so that we can replay them on the other hemisphere
// to keep things consistent. This encapsulates uppdating
// one property.
//
// These will be collected in a hashmap for replay
//    map<(window id, property id), Patch>
#[allow(dead_code)]
#[derive(Copy, Clone, Debug)]
enum Patch {
    // reserve a window id, old or new
    reserve_window_id(WindowId),
    free_window_id(WindowId),
    // bring a window to the top
    focus_on_id(WindowId),
    // add a new toplevel role window
    add_new_toplevel(WindowId),
    // set (x, y, width, height)
    set_window_dimensions((f32, f32, f32, f32)),
}

// This is a magic value used as the id parameter in the
// (WindowId, Property) pair for FOCUS_ON_ID
// We need this because we won't have a window
// id to look up the focused window patch with
const FOCUS_STUB_ID: WindowId = 0;

// private data used by ways only
//
// This holds all of the protocol resources
// that ways needs. An array of these are used (indexed
// by the window id) to tie an id to a set of protocol
// objects. i.e. find the surface/seat/etc for this id.
pub struct Priv {
    // a surface to have its callbacks called
    p_surf: Option<Rc<RefCell<Surface>>>,
    // a collection of input resources
    p_seat: Option<Rc<RefCell<Seat>>>,
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

    // The next id to hand out
    // each index is marked true if that id is handed out
    a_id_map: Vec<bool>,

    // -- ways --
    
    a_ways_priv: Vec<Option<Priv>>,
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
    a_patches: HashMap<(WindowId, Property), Patch>,
    // The cursor is the number one thing we will have to
    // patch. There's no point having the overhead of a_patches
    // when it is only 2 floats, so just add it here.
    a_cursor_patch: Option<(f64, f64)>,
    // Same idea as the cursor patch but for the grabbed window
    // The outer option tells us if we have an update. The inner
    // is the value of the hemi.h_grabbed
    a_grab_patch: Option<Option<WindowId>>,
    a_resolution_patch: Option<(u32, u32)>,
}

impl Atmosphere {
    // Create a new atmosphere to be shared within a subsystem
    //
    // We pass in the hemispheres since they will have to
    // also be passed to the other subsystem.
    // One subsystem must be setup as index 0 and the other
    // as index 1
    pub fn new(tx: Sender<Box<Hemisphere>>,
               rx: Receiver<Box<Hemisphere>>)
               -> Atmosphere
    {
        Atmosphere {
            a_tx: tx,
            a_rx: rx,
            a_hemi: Some(Box::new(Hemisphere::new())),
            a_id_map: Vec::new(),
            // TODO: only do this for ways
            a_ways_priv: Vec::new(),
            a_patches: HashMap::new(),
            a_cursor_patch: None,
            a_grab_patch: None,
            a_resolution_patch: None,
        }
    }

    // Get the next id
    //
    // Find a free id if one is available, if not then
    // add a new one
    pub fn mint_client_id(&mut self) -> WindowId {
        for (i, in_use) in self.a_id_map.iter_mut().enumerate() {
            if !*in_use {
                *in_use = true;
                self.reserve_window_id(i as u32);
                return i as u32;
            }
        }

        // grow the mapping
        self.reserve_window_id(self.a_id_map.len() as u32);
        // this should come separately so we don't mess with the len
        // used in the line above
        self.a_id_map.push(true);
        return (self.a_id_map.len() - 1) as u32;
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
        for ((window_id, prop), patch) in self.a_patches.iter() {
            log!(LogLevel::info, "   replaying {:?}", patch);
            hemi.apply_patch(*window_id, *prop, patch);
        }
        if let Some(grab) = self.a_grab_patch {
            hemi.grab(grab);
        }
        if let Some(res) = self.a_resolution_patch {
            hemi.set_resolution(res.0, res.1);
        }
        if let Some(cursor) = self.a_cursor_patch {
            hemi.set_cursor_pos(cursor.0, cursor.1);
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

        // Clear the patches
        self.a_resolution_patch = None;
        self.a_grab_patch = None;
        self.a_cursor_patch = None;
        self.a_patches.clear();

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

    // Add a patch to be replayed on a flip
    //
    // All changes to the current hemisphere will get
    // batched up into a set of patches. This is needed to
    // keep both hemispheres in sync.
    fn add_patch(&mut self,
                 id: WindowId,
                 prop: Property,
                 patch: &Patch)
    {
        self.a_hemi.as_mut().map(|h| h.mark_changed());
        self.a_patches.insert((id, prop), *patch);
    }

    // ------------------------------
    // For the sake of abstraction, the atmosphere will be the
    // point of contact for modifying global state. We will
    // record any changes to replay and pass the data down
    // to the hemisphere
    // ------------------------------

    // TODO: make atmosphere in charge of ids
    //
    // This wraps a couple actions into one helper
    // since there are multiple 
    pub fn create_new_window(&mut self, id: WindowId) {
        self.add_wm_task(
            wm::task::Task::create_window(id)
        );

        self.add_patch(
            id,
            Property::SET_WINDOW_DIMENSIONS,
            &Patch::set_window_dimensions(
                (0.0, 0.0, // (x, y)
                 640.0, 480.0) // (width, height)
            )
        );

        // make this the new toplevel window
        self.add_patch(id,
                       Property::ADD_NEW_TOPLEVEL,
                       &Patch::add_new_toplevel(id));
    }

    // Mark the specified id as in-use
    //
    // Ids are used as indexes for most of the vecs
    // in the hemisphere, and we need to mark this as
    // no longer available
    pub fn reserve_window_id(&mut self, id: WindowId) {
        self.add_patch(id,
                       Property::RESERVE_WINDOW_ID,
                       &Patch::reserve_window_id(id));

        // Add a new priv entry
        let private = Some(Priv {
            p_surf: None,
            p_seat: None,
        });
        if (id as usize) < self.a_ways_priv.len() {
            assert!(!self.a_ways_priv[id as usize].is_none());
            self.a_ways_priv[id as usize] = private;
        } else {
            // otherwise make a new one
            assert!(id as usize == self.a_ways_priv.len());
            self.a_ways_priv.push(private);
        }
    }

    // Mark the id as available
    pub fn free_window_id(&mut self, id: WindowId) {
        assert!(!self.a_ways_priv[id as usize].is_none());
        self.a_ways_priv[id as usize] = None;
        self.add_patch(
            id,
            Property::FREE_WINDOW_ID,
            &Patch::free_window_id(id),
        );
    }

    // Get the window order from [0..n windows)
    //
    // TODO: Make this a more efficient tree
    pub fn get_window_order(&self, id: WindowId) -> u32 {
        // check if this window has been brought into focus
        if let Some(patch) = self.a_patches.get(&(FOCUS_STUB_ID,
                                                  Property::FOCUS_ON_ID))
        {
            match patch {
                Patch::focus_on_id(focus) => {
                    if *focus == id { return 0; }
                },
                _ => (),
            }
        }

        self.a_hemi.as_ref().unwrap().get_window_order(id)
    }

    // Get the window currently in use
    pub fn get_window_in_focus(&self) -> Option<WindowId> {
        if let Some(focus) = self.a_patches.get(&(FOCUS_STUB_ID,
                                                  Property::FOCUS_ON_ID))
        {
            match focus {
                Patch::focus_on_id(id) => return Some(*id),
                _ => (),
            }
        }

        self.a_hemi.as_ref().unwrap().get_window_in_focus()
    }

    // Set the window currently in focus
    //
    // CONCERN: Theoretically if multiple of these could
    // be submitted in one hemi flip then it could perform
    // harmful swapping that could remove one or more windows
    // from being drawn
    pub fn focus_on(&mut self, id: WindowId) {
        log!(LogLevel::info, "adding patch to focus on window {}", id);
        self.add_patch(
            FOCUS_STUB_ID, // see comment for this var
            Property::FOCUS_ON_ID,
            &Patch::focus_on_id(id),
        );
    }

    // this is one of the few updates from vkcomp
    pub fn set_resolution(&mut self, x: u32, y: u32) {
        self.a_hemi.as_mut().map(|h| h.mark_changed());
        self.a_resolution_patch = Some((x, y));
    }

    // Get the screen resolution as set by vkcomp
    pub fn get_resolution(&self) -> (u32, u32) {
        self.a_hemi.as_ref().unwrap().get_resolution()
    }

    // This is the thickness of the titlebar
    // It is based on the resolution, but hidden behind this
    // function so it can easily be changed
    pub fn get_barsize(&self) -> f32 {
        self.a_hemi.as_ref().unwrap().get_barsize()
    }

    // Find if there is a toplevel window under (x,y)
    //
    // This is used first to find if the cursor intersects
    // with a window. If it does, point_is_on_titlebar is
    // used to check for a grab or relay input event.
    pub fn find_window_at_point(&self, x: f32, y: f32)
                                -> Option<WindowId>
    {
        self.a_hemi.as_ref().unwrap().find_window_at_point(x, y)
    }

    // Is the current point over the titlebar of the window
    //
    // Id should have first been found with find_window_at_point
    pub fn point_is_on_titlebar(&self, id: WindowId, x: f32, y: f32)
                                -> bool
    {
        self.a_hemi.as_ref().unwrap().point_is_on_titlebar(id, x, y)
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
        self.a_hemi.as_mut().map(|h| h.mark_changed());
        if let Some(mut cursor) = self.a_cursor_patch.as_mut() {
            cursor.0 += dx;
            cursor.1 += dy;
        } else {
            let cursor = self.a_hemi.as_mut().unwrap()
                .get_cursor_pos();
            self.a_cursor_patch = Some((cursor.0 + dx, cursor.1 + dy));
        }

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

    // gets the id of the currently grabbed window
    pub fn get_grabbed(&self) -> Option<WindowId> {
        // check if we have cached it
        match self.a_grab_patch {
            Some(grabbed) => grabbed,
            // or just grab it
            None => self.a_hemi.as_ref().unwrap().get_grabbed(),
        }
    }

    pub fn get_cursor_pos(&self) -> (f64, f64) {
        self.a_hemi.as_ref().unwrap().get_cursor_pos()
    }

    // Get the window dimensions
    // grab them from the patchmap first, and fetch them from the
    // hemisphere if they aren't currently patched.
    pub fn get_window_dimensions(&self, id: WindowId)
                                 -> (f32, f32, f32, f32)
    {
        if let Some(Patch::set_window_dimensions(patch))
            = self.a_patches.get(&(id, Property::SET_WINDOW_DIMENSIONS))
        {
            return *patch;
        } else {
            return self.a_hemi.as_ref().unwrap()
                .get_window_dimensions(id);
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
        self.add_patch(id,
                       Property::SET_WINDOW_DIMENSIONS,
                       &Patch::set_window_dimensions((x, y, width, height)));
    }

    // Grab the window by the specified id
    // it will get moved around as the cursor does
    pub fn grab(&mut self, id: WindowId) {
        self.a_hemi.as_mut().map(|h| h.mark_changed());
        self.a_grab_patch = Some(Some(id));
    }

    pub fn ungrab(&mut self) {
        self.a_hemi.as_mut().map(|h| h.mark_changed());
        self.a_grab_patch = Some(None);
    }


    // -- subsystem specific handlers --

    pub fn add_surface(&mut self, id: WindowId,
                       surf: Rc<RefCell<Surface>>)
    {
        if let Some(private) = self.a_ways_priv[id as usize].as_mut() {
            private.p_surf = Some(surf);
        }
    }

    pub fn add_seat(&mut self, id: WindowId,
                    seat: Rc<RefCell<Seat>>)
    {
        if let Some(private) = self.a_ways_priv[id as usize].as_mut() {
            private.p_seat = Some(seat);
        }
    }

    pub fn get_seat_from_id(&mut self, id: WindowId)
                            -> Option<Rc<RefCell<Seat>>>
    {
        if let Some(private) = self.a_ways_priv[id as usize].as_mut() {
            return private.p_seat.clone();
        }
        return None;
    }

    // Signal any registered frame callbacks
    // TODO: actually do optimizations
    //
    // Wayland uses these callbacks to tell apps when they should
    // redraw themselves. If they aren't on screen we don't send
    // the callback so it doesn't use the power.
    pub fn signal_frame_callbacks(&mut self) {
        for private in self.a_ways_priv
            .iter()
            .filter(|p| p.is_some())
        {
            if let Some(cell) = private.as_ref().unwrap().p_surf.as_ref() {
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
    // software cursor position
    h_cursor_x: f64,
    h_cursor_y: f64,
    // A window that has been grabbed by the user and is being
    // drug around by the mouse.
    //
    // Any movements to the cursor will also move this window
    // automagically.
    h_grabbed: Option<WindowId>,
    // A list of surfaces which have been handed out to clients
    // Recorded here so we can perform interesting DE interactions
    // These are indexed by window id, and marked true if they are
    // in use. (aka h_windows[0] == false means this id is available)
    h_windows: Vec<bool>,
    // a list of the window ids from front to back
    // index 0 is the current focus
    h_window_heir: Vec<WindowId>,
    // The position of each window's top left corner, and it's width
    // Indexed by WindowId
    // (base_x, base_y, width, height)
    h_window_dimensions: Vec<Option<(f32, f32, f32, f32)>>,
    // A list of tasks to be completed by vkcomp this frame
    // - does not need to be patched
    //
    // Tasks are one time events. Anything related to state should
    // be added elsewhere. A task is a transfer of ownership from
    // ways to vkcommp
    h_wm_tasks: VecDeque<wm::task::Task>,
    // The resolution of the screen
    // TODO: multimonitor support
    h_resolution: (u32, u32),
}


impl Hemisphere {
    fn new() -> Hemisphere {
        Hemisphere {
            h_has_changed: true,
            h_cursor_x: 0.0,
            h_cursor_y: 0.0,
            h_grabbed: None,
            h_windows: Vec::new(),
            h_window_heir: Vec::new(),
            h_window_dimensions: Vec::new(),
            h_wm_tasks: VecDeque::new(),
            h_resolution: (0, 0),
        }
    }

    // Apply a patch to this hemisphere
    // This is used to commit a changeset
    //
    // Changes are accrued in the patch list. Before
    // flipping hemispheres we will apply the patch
    // list to the current hemisphere, and then again
    // to the new one to keep things up to date.
    fn apply_patch(&mut self,
                   win: WindowId,
                   _prop: Property,
                   patch: &Patch)
    {
        match patch {
            Patch::reserve_window_id(id) =>
                self.reserve_window_id(*id),
            Patch::free_window_id(id) =>
                self.free_window_id(*id),
            Patch::focus_on_id(id) =>
                self.focus_on(*id),
            Patch::add_new_toplevel(id) =>
                self.add_new_toplevel(*id),
            Patch::set_window_dimensions(dims) =>
                self.set_window_dimensions(
                    win, dims.0, dims.1, dims.2, dims.3
                ),
        };
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

    fn clear_changed(&mut self) {
        self.h_has_changed = false;
    }

    // ----------------
    // modifiers
    // ----------------

    fn add_wm_task(&mut self, task: wm::task::Task) {
        self.mark_changed();
        self.h_wm_tasks.push_back(task);
    }

    // Returns the lowest allocated client id
    // does not handle any heirarchy or other stuff
    fn reserve_window_id(&mut self, wid: WindowId) {
        self.mark_changed();
        let id = wid as usize;

        // Check if we can reuse the id
        if id < self.h_windows.len() {
            assert!(!self.h_windows[id]);
            self.h_windows[id] = true;
        } else {
            // otherwise make a new one
            assert!(id == self.h_windows.len());
            self.h_windows.push(true);
        }
    }

    // Make this window the top level
    pub fn focus_on(&mut self, id: WindowId) {
        log!(LogLevel::info, "focusing on window {}", id);
        assert!((id as usize) < self.h_windows.len());
        self.mark_changed();

        // find the index of this window
        let idx = self.h_window_heir.iter()
            .enumerate()
            .find(|(_, &win)| win == id)
            // there can only be one
            .unwrap()
            // get the index, which is the first in the tuple
            .0;

        // if it is already the top window then bail
        if idx == 0 {
            return;
        }

        self.h_window_heir.swap(0, idx);
    }

    // Marks id as free and removes all references to it
    fn free_window_id(&mut self, id: WindowId) {
        assert!((id as usize) < self.h_windows.len());
        self.mark_changed();

        self.h_windows[id as usize] = false;
        // remove this id from the heirarchy
        self.h_window_heir.retain(|&wid| wid != id);
    }

    pub fn wm_task_pop(&mut self) -> Option<wm::task::Task> {
        self.mark_changed();
        self.h_wm_tasks.pop_front()
    }

    pub fn add_cursor_pos(&mut self, dx: f64, dy: f64) {
        self.mark_changed();
        self.h_cursor_x += dx;
        self.h_cursor_y += dy;
    }

    pub fn set_cursor_pos(&mut self, dx: f64, dy: f64) {
        self.mark_changed();
        self.h_cursor_x = dx;
        self.h_cursor_y = dy;
    }

    pub fn grab(&mut self, id: Option<WindowId>) {
        self.h_grabbed = id;
    }

    pub fn add_new_toplevel(&mut self, id: WindowId) {
        self.h_window_heir.insert(0, id);
    }

    pub fn set_window_dimensions(&mut self,
                                 id: WindowId,
                                 x: f32,
                                 y: f32,
                                 width: f32,
                                 height: f32)
    {
        if (id as usize) >= self.h_window_dimensions.len() {
            self.h_window_dimensions.resize(id as usize + 1, None);
        }

        self.h_window_dimensions[id as usize] =
            Some((x, y, width, height));
    }

    // this is one of the few updates from vkcomp
    pub fn set_resolution(&mut self, x: u32, y: u32) {
        self.h_resolution = (x, y);
    }

    // ----------------
    // accessors
    // ----------------

    // Get the window currently in use
    // The window heir is sorted, so the first one will
    // be the top level
    pub fn get_window_in_focus(&self) -> Option<WindowId> {
        if self.h_window_heir.len() > 0 {
            return Some(self.h_window_heir[0]);
        }
        return None;
    }

    pub fn get_resolution(&self) -> (u32, u32) {
        self.h_resolution
    }

    fn get_grabbed(&self) -> Option<WindowId> {
        self.h_grabbed
    }

    pub fn get_window_order(&self, id: WindowId) -> u32 {
        for (i, win) in self.h_window_heir.iter().enumerate() {
            if *win == id {
                return i as u32;
            }
        }
        panic!("Could not find window with id {}", id);
    }

    // Used to find what window is under the cursor
    // returns None if the point is not over a window
    pub fn find_window_at_point(&self, x: f32, y: f32)
                                -> Option<WindowId>
    {
        let barsize = self.get_barsize();

        for win in self.h_window_heir.iter() {
            let pos = self.h_window_dimensions[*win as usize].unwrap();

            // If this window contains (x, y) then return it
            if x > pos.0 && y > pos.1
                && x < (pos.0 + pos.2)
                && y < (pos.1 + pos.3 + barsize)
            {
                return Some(*win);
            }
        }
        return None;
    }

    // Used to find if the point is over this windows titlebar
    // returns true if the point is
    pub fn point_is_on_titlebar(&self, id: WindowId, x: f32, y: f32)
                          -> bool
    {
        let barsize = self.get_barsize();

        let pos = self.h_window_dimensions[id as usize].unwrap();

        // If this window contains (x, y) then return it
        if x > pos.0 && y > pos.1
            && x < (pos.0 + pos.2)
            && y < pos.1 + barsize
        {
            return true;
        }
        return false;
    }

    pub fn get_cursor_pos(&self) -> (f64, f64) {
        (self.h_cursor_x, self.h_cursor_y)
    }

    pub fn get_barsize(&self) -> f32 {
        self.h_resolution.1 as f32 * 0.02
    }

    pub fn get_window_dimensions(&self, id: WindowId)
                                 -> (f32, f32, f32, f32)
    {
        assert!((id as usize) < self.h_window_dimensions.len());

        self.h_window_dimensions[id as usize].unwrap()
    }
}
