//! # Atmosphere: an entity-component set
//!
//! Atmosphere is our entity component set used to communicate between the
//! different subsystems. It assigns a numerical id to a resource which
//! can be used to get or set the value of different properties. For
//! example, `ways` will update the position property of a window. During
//! the next frame, vkcomp will read this updated position and use it to
//! draw that window in a new location.

// Austin Shafer - 2020
extern crate wayland_server as ws;
use crate::category5::ws::Resource;
use ws::protocol::{wl_buffer, wl_callback, wl_shm, wl_surface};
extern crate paste;
use paste::paste;

extern crate dakota as dak;
extern crate lluvia as ll;

mod skiplist;

use crate::category5::input::Input;
use crate::category5::vkcomp::{release_info::GenericReleaseInfo, wm};
use crate::category5::ways::{seat::Seat, shm::ShmBuffer, surface::*, wl_region::Region};
use utils::log;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::vec::Vec;

/// ECS refcounted id for each client
pub type ClientId = ll::Entity;
/// ECS refcounted id for each surface
///
/// This is actually a DakotaId, meaning that all properties for this
/// are tracked by dakota elements.
pub type SurfaceId = dak::DakotaId;
/// ECS refcounted buffer id
///
/// This id will represent each wl_buffer created, and allows us a way
/// to attach arbitrary state to them. This id is stored in the wl_buffer
/// object.
pub type BufferId = dak::DakotaId;

/// Shadow buffer state
///
/// A shadow buffer is an internally owned Scene Resource which contains
/// a copy of the surface contents. This is used primarily for shm buffers,
/// where we have a local copy that we update based on buffer damage and
/// release the attached buffer immediately.
struct ShadowBuffer {}

/// Global state tracking
///
/// Our atmosphere holds all of the ECS data in one place, and is essentially
/// a database of Category5's internsal state.
///
/// Keep in mind this only holds any shared data, data
/// exclusive to subsystems will be held by said subsystem
pub struct Atmosphere {
    pub a_cursor_pos: (f64, f64),
    /// The offset of the cursor image
    pub a_cursor_hotspot: (i32, i32),
    pub a_resolution: (u32, u32),
    pub a_grabbed: Option<SurfaceId>,
    pub a_resizing: Option<SurfaceId>,
    /// the window the user is currently interacting with
    /// This tells us which one to start looking at for the skiplist
    ///
    /// Not to be confused with `surf_focus`, this refers to the *application*
    /// that is currently in focus. It is used to track the "root" window that
    /// was created by xdg/wl_shell.
    pub a_win_focus: Option<SurfaceId>,
    /// This is the current surface that is in focus, not respective of application.
    /// It is possible that this is the same as `win_focus`.
    ///
    /// This is the wl_surface that the user has entered, and it is highly likely
    /// that this is a subsurface. Therefore `win_focus` will be the "root" application
    /// toplevel window, and `surf_focus` may be a subsurface of that window tree.
    pub a_surf_focus: Option<SurfaceId>,
    /// The surface that the pointer is currently over.
    /// note that this may be different than the application focus, this separate tracking
    /// is used to scrolling "unfocused" windows.
    pub a_pointer_focus: Option<SurfaceId>,
    /// Current surface in use for a cursor, if any
    pub a_cursor_surface: Option<SurfaceId>,
    /// Is recording traces with Renderdoc enabled?
    /// This is used for debugging. input will trigger this, which tells vkcomp
    /// to record frames.
    pub a_renderdoc_recording: bool,
    /// The name of the DRM node in use. This will be filled in by vkcomp
    /// and populated from VK_EXT_physical_device_drm
    pub a_drm_dev: (i64, i64),

    pub a_changed: bool,

    /// Tasks to be handled by vkcomp before rendering the next frame
    pub a_wm_tasks: VecDeque<wm::task::Task>,

    // -------------------------------------------------------
    /// Client id tracking
    ///
    /// This is an ECS for tying a bunch of data to a ClientId
    pub a_client_ecs: ll::Instance,
    // Indexed by ClientId -------------------------------------------------------
    /// window ids belonging to this client
    pub a_windows_for_client: ll::Component<Vec<SurfaceId>>,
    /// a collection of input resources
    pub a_seat: ll::Component<Arc<Mutex<Seat>>>,

    // -------------------------------------------------------
    /// Surface id tracking
    ///
    /// This is an ECS tying a bunch of data to a surface.
    /// SurfaceIds are really just DakotaIds, we use these tables to tack on our own
    /// information about each surface.
    pub a_surface_ecs: ll::Instance,
    // Indexed by SurfaceId -------------------------------------------------------
    // is this id in use?
    pub a_window_in_use: ll::Component<bool>,
    /// The client that created this window
    pub a_owner: ll::Component<ClientId>,
    /// does this window have the toplevel role
    /// this controls if SSD are drawn
    pub a_toplevel: ll::Component<bool>,
    /// the position of the visible portion of the window
    pub a_window_pos: ll::Component<(f32, f32)>,
    /// size of the visible portion : `ll::Component<non-CSD>` of the window
    /// window manager uses this
    pub a_window_size: ll::Component<(f32, f32)>,
    /// If this window is a subsurface, then x and y will
    /// be offsets from the base of the parent window
    pub a_surface_pos: ll::Component<(f32, f32)>,
    /// the size of the surface
    /// aka the size of the last buffer attached
    /// vkcomp uses this
    pub a_surface_size: ll::Component<(f32, f32)>,
    /// This window's position in the desktop order
    ///
    /// The next window behind this one
    pub a_skiplist_next: ll::Component<SurfaceId>,
    /// The window in front of this
    pub a_skiplist_prev: ll::Component<SurfaceId>,
    /// The next *visible* window
    pub a_skiplist_skip: ll::Component<SurfaceId>,
    /// The toplevel child surface
    /// because surfaces can be arbitrarily nested,
    /// surfaces may be added to this list instead
    /// of the main global ordering.
    ///
    /// The start of the subsurface skiplist
    pub a_top_child: ll::Component<SurfaceId>,
    /// If this is a subsurface of another window
    /// aka not a toplevel
    pub a_parent_window: ll::Component<SurfaceId>,
    /// The wl_subsurface.set_sync property. This tells use if we should
    /// commit when the parent does or whenever this surface is
    /// committed.
    /// Will be None if this is not a subsurface.
    pub a_subsurface_sync: ll::Component<bool>,
    /// This is the root of the window tree that this window
    /// is a part of. When this surface is in focus, this will
    /// be the value of the `win_focus` global prop.
    pub a_root_window: ll::Component<SurfaceId>,
    /// a surface to have its callbacks called
    pub a_surface: ll::Component<Arc<Mutex<Surface>>>,
    /// The protocol object for this surface
    /// We need to store this here because some places
    /// : ll::Component<`keyboard_enter`> will want to query for it to deliver
    /// events while the above surface is borrowed
    pub a_wl_surface: ll::Component<wl_surface::WlSurface>,
    /// Accumulated damage local to this surface
    pub a_surface_damage: ll::Component<dak::Damage>,
    /// Damage to the buffer from wayland events
    pub a_buffer_damage: ll::Component<dak::Damage>,
    /// Frame throttling callbacks
    ///
    /// These will be signaled on the next draw point so the
    /// surface can commit new contents
    pub a_frame_callbacks: ll::Component<Vec<wl_callback::WlCallback>>,
    /// The opaque region.
    /// vkcomp can optimize displaying this region
    pub a_opaque_region: ll::Component<Arc<Mutex<Region>>>,
    /// The input region.
    /// Input events will only be delivered if this region is in focus
    pub a_input_region: ll::Component<Arc<Mutex<Region>>>,
    /// Scene resources per surface. This is the same as dakota.resource(), and
    /// is the resource currently bound to this surface (i.e. dakota element)
    pub a_surf_resource: ll::Component<BufferId>,

    // -------------------------------------------------------
    // Resource id tracking
    //
    // These are indexed by BufferIds which represent Scene resource objects.
    // These will be attached to SurfaceIds to assign window content.
    /// Shadow Resource (local copy of buffer)
    a_shadow_buffer: ll::Component<ShadowBuffer>,
}

// Implement getters/setters for our global properties
macro_rules! define_global_getters {
    ($name:ident, $val:ty) => {
        paste! {
            pub fn [<get_ $name>](&self) -> $val {
                self.[< a_ $name>].clone()
            }
            pub fn [<set_ $name>](&mut self, val: $val) {
                self.mark_changed();
                self.[<a_ $name>] = val;
            }
        }
    };
}

impl Atmosphere {
    define_global_getters!(cursor_pos, (f64, f64));
    define_global_getters!(cursor_hotspot, (i32, i32));
    define_global_getters!(resolution, (u32, u32));
    define_global_getters!(grabbed, Option<SurfaceId>);
    define_global_getters!(resizing, Option<SurfaceId>);
    define_global_getters!(win_focus, Option<SurfaceId>);
    define_global_getters!(surf_focus, Option<SurfaceId>);
    define_global_getters!(pointer_focus, Option<SurfaceId>);
    define_global_getters!(cursor_surface, Option<SurfaceId>);
    define_global_getters!(renderdoc_recording, bool);
    define_global_getters!(drm_dev, (i64, i64));
}

impl Atmosphere {
    /// Create a new atmosphere to be shared within a subsystem
    ///
    /// We pass in the hemispheres and lock(s) since they will have to
    /// also be passed to the other subsystem.
    /// One subsystem must be setup as index 0 and the other
    /// as index 1
    pub fn new(scene: &dak::Scene) -> Atmosphere {
        let mut surf_ecs = scene.get_ecs_instance();
        let mut resource_ecs = scene.get_resource_ecs_instance();
        let mut client_ecs = ll::Instance::new();

        Atmosphere {
            a_cursor_pos: (0.0, 0.0),
            a_cursor_hotspot: (0, 0),
            a_resolution: (0, 0),
            a_grabbed: None,
            a_resizing: None,
            a_win_focus: None,
            a_surf_focus: None,
            a_pointer_focus: None,
            a_cursor_surface: None,
            a_renderdoc_recording: false,
            a_changed: false,
            a_drm_dev: (0, 0),
            a_wm_tasks: VecDeque::new(),
            // ---------------------
            a_windows_for_client: client_ecs.add_component(),
            a_seat: client_ecs.add_component(),
            a_client_ecs: client_ecs,
            // ---------------------
            a_window_in_use: surf_ecs.add_component(),
            a_owner: surf_ecs.add_component(),
            a_toplevel: surf_ecs.add_component(),
            a_window_pos: surf_ecs.add_component(),
            a_window_size: surf_ecs.add_component(),
            a_surface_pos: surf_ecs.add_component(),
            a_surface_size: surf_ecs.add_component(),
            a_skiplist_next: surf_ecs.add_component(),
            a_skiplist_prev: surf_ecs.add_component(),
            a_skiplist_skip: surf_ecs.add_component(),
            a_top_child: surf_ecs.add_component(),
            a_parent_window: surf_ecs.add_component(),
            a_subsurface_sync: surf_ecs.add_component(),
            a_root_window: surf_ecs.add_component(),
            a_surface: surf_ecs.add_component(),
            a_wl_surface: surf_ecs.add_component(),
            a_surface_damage: surf_ecs.add_component(),
            a_buffer_damage: surf_ecs.add_component(),
            a_frame_callbacks: surf_ecs.add_component(),
            a_opaque_region: surf_ecs.add_component(),
            a_input_region: surf_ecs.add_component(),
            a_surf_resource: scene.resource(),
            // ---------------------
            a_shadow_buffer: resource_ecs.add_component(),
            a_surface_ecs: surf_ecs,
        }
    }

    /// Has the current hemisphere been changed
    ///
    /// Ways will use this to know if it should flip
    /// hemispheres and wake up vkcomp
    pub fn is_changed(&self) -> bool {
        self.a_changed
            || self.a_windows_for_client.is_modified()
            || self.a_seat.is_modified()
            || self.a_window_in_use.is_modified()
            || self.a_owner.is_modified()
            || self.a_toplevel.is_modified()
            || self.a_window_pos.is_modified()
            || self.a_window_size.is_modified()
            || self.a_surface_pos.is_modified()
            || self.a_surface_size.is_modified()
            || self.a_skiplist_next.is_modified()
            || self.a_skiplist_prev.is_modified()
            || self.a_skiplist_skip.is_modified()
            || self.a_top_child.is_modified()
            || self.a_parent_window.is_modified()
            || self.a_subsurface_sync.is_modified()
            || self.a_root_window.is_modified()
            || self.a_surface.is_modified()
            || self.a_wl_surface.is_modified()
            || self.a_surface_damage.is_modified()
            || self.a_buffer_damage.is_modified()
            || self.a_surf_resource.is_modified()
            || self.a_shadow_buffer.is_modified()
    }
    pub fn clear_changed(&mut self) {
        self.a_changed = false;
        self.a_windows_for_client.clear_modified();
        self.a_seat.clear_modified();
        self.a_window_in_use.clear_modified();
        self.a_owner.clear_modified();
        self.a_toplevel.clear_modified();
        self.a_window_pos.clear_modified();
        self.a_window_size.clear_modified();
        self.a_surface_pos.clear_modified();
        self.a_surface_size.clear_modified();
        self.a_skiplist_next.clear_modified();
        self.a_skiplist_prev.clear_modified();
        self.a_skiplist_skip.clear_modified();
        self.a_top_child.clear_modified();
        self.a_parent_window.clear_modified();
        self.a_subsurface_sync.clear_modified();
        self.a_root_window.clear_modified();
        self.a_surface.clear_modified();
        self.a_wl_surface.clear_modified();
        self.a_surface_damage.clear_modified();
        self.a_buffer_damage.clear_modified();
        self.a_surf_resource.clear_modified();
        self.a_shadow_buffer.clear_modified();
    }
    pub fn mark_changed(&mut self) {
        self.a_changed = true;
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

    /// Reserve a new client id
    ///
    /// Should be done the first time we interact with
    /// a new client
    pub fn mint_client_id(&mut self) -> ClientId {
        let id = self.a_client_ecs.add_entity();
        self.a_windows_for_client.set(&id, Vec::new());

        return id;
    }

    /// Mark the specified id as in-use
    ///
    /// Ids are used as indexes for most of the vecs
    /// in the hemisphere, and we need to mark this as
    /// no longer available
    pub fn mint_window_id(&mut self, scene: &mut dak::Scene, client: &ClientId) -> SurfaceId {
        let id = scene.create_element().unwrap();

        // first initialize all our properties
        self.a_owner.set(&id, client.clone());
        self.a_toplevel.set(&id, false);
        self.a_window_pos.set(&id, (0.0, 0.0));
        self.a_surface_pos.set(&id, (0.0, 0.0));
        self.a_surface_size.set(&id, (0.0, 0.0));

        // TODO: optimize me
        // This is a bit too expensive atm
        let mut windows = self.a_windows_for_client.get_mut(client).unwrap();
        windows.push(id.clone());

        return id;
    }

    /// Create a new BufferId
    ///
    /// This is really a Scene Resource id type.
    pub fn mint_buffer_id(&mut self, scene: &mut dak::Scene) -> BufferId {
        scene.create_resource().unwrap()
    }

    /// Recalculate the pointer focus
    ///
    /// This will find the input at the current cursor position and update the
    /// pointer focus to that SurfaceId.
    pub fn recalculate_pointer_focus(&mut self) {
        let (cx, cy) = self.get_cursor_pos();

        // Get the window the pointer is over
        let focus = self.find_window_with_input_at_point(cx as f32, cy as f32);
        // If the pointer is over top of a different window, change the
        // pointer focus and send the leave/enter events
        if focus.clone().map(|e| e.get_raw_id()) != self.get_pointer_focus().map(|e| e.get_raw_id())
        {
            if let Some(id) = self.get_pointer_focus() {
                Input::pointer_leave(self, &id);
            }
            if let Some(id) = focus.as_ref() {
                Input::pointer_enter(self, id);
            }
            self.set_pointer_focus(focus.clone());
        }
    }

    /// Mark the id as available and clean up any resources which reference
    /// this id (such as surface lists and current focus
    pub fn free_window_id(&mut self, client: &ClientId, id: &SurfaceId) {
        log::debug!("Ways before removing id {:?}", id);
        self.print_surface_tree();

        // we also need to remove this surface from focus
        self.skiplist_remove_win_focus(id);
        self.skiplist_remove_surf_focus(id);
        // remove this id from the heirarchy
        self.skiplist_remove_window(id);
        // TODO: generate RemoveWindow event?

        // remove this window from the clients list
        // TODO: This is a bit too expensive atm
        {
            let mut windows = self.a_windows_for_client.get_mut(client).unwrap();
            windows.retain(|wid| wid == id);
        }

        // If this window was in focus recalculate focus now
        if let Some(pointer_focus) = self.get_pointer_focus() {
            if id == &pointer_focus {
                self.set_pointer_focus(None);
                self.recalculate_pointer_focus();
            }
        }

        log::debug!("Ways after removing id {:?}", id);
        self.print_surface_tree();
    }

    /// convert a global location to a surface local coordinates.
    /// Returns None if the location given is not over the surface
    pub fn global_coords_to_surf(&self, id: &SurfaceId, x: f64, y: f64) -> Option<(f64, f64)> {
        let (x, y) = self.get_adjusted_desktop_coord(x as f32, y as f32);
        let (x, y) = (x as f64, y as f64);
        // get the surface-local position
        let (mut wx, mut wy) = *self.a_surface_pos.get(id).unwrap();
        let (ww, wh) = *self.a_surface_size.get(id).unwrap();

        // Add any parent surface's positions to our surface offset to account
        // for this surface being a subsurf
        let mut parent = self.a_parent_window.get_clone(id);
        while parent.is_some() {
            let p = parent.take().unwrap();
            let (px, py) = *self.a_surface_pos.get(&p).unwrap();
            wx += px;
            wy += py;

            parent = self.a_parent_window.get_clone(&p);
        }

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
        self.mark_changed();
        self.a_wm_tasks.push_back(task);
    }

    /// pulls a one-time task off the queue
    pub fn get_next_wm_task(&mut self) -> Option<wm::task::Task> {
        self.mark_changed();
        self.a_wm_tasks.pop_front()
    }

    /// Handles an update from dmabuf task
    ///
    /// Translates the task update structure into lower
    /// level calls to import a dmabuf and update a image.
    /// Creates a new image if one doesn't exist yet.
    pub fn create_dmabuf_resource(
        &mut self,
        scene: &mut dak::Scene,
        resource: &dak::DakotaId,
        buffer: wl_buffer::WlBuffer,
        dmabuf: &dak::Dmabuf,
    ) -> dak::Result<()> {
        // Create a new resource from this dmabuf
        scene.define_resource_from_dmabuf(
            resource,
            dmabuf,
            Some(Box::new(GenericReleaseInfo {
                wl_buffer: buffer.clone(),
            })),
        )?;

        Ok(())
    }

    /// Get or create a shadow buffer for this surface
    fn get_shadow_resource(&mut self, scene: &mut dak::Scene, surf: &SurfaceId) -> BufferId {
        if let Some(id) = self.a_surf_resource.get_clone(surf) {
            if self.a_shadow_buffer.get(&id).is_some() {
                return id;
            }
        }

        let id = self.mint_buffer_id(scene);
        self.a_shadow_buffer.set(&id, ShadowBuffer {});
        return id;
    }

    /// Handle update from memimage task
    ///
    /// Copies the shm buffer into the app's image.
    /// Creates a new image if one doesn't exist yet.
    pub fn update_shm_resource(
        &mut self,
        scene: &mut dak::Scene,
        surf: &SurfaceId,
        shm_buffer: &ShmBuffer,
        buffer: &wl_buffer::WlBuffer,
    ) -> dak::Result<()> {
        // Get the shadow resource if it exists. If not, create it.
        // We do this by checking if the surface is currently assigned a resource
        // which has had its shadow state set.
        let shadow = self.get_shadow_resource(scene, surf);

        let pixels = shm_buffer.get_mem_image();
        if let Err(e) = match scene.is_resource_defined(&shadow) {
            // If the shadow resource is defined, then copy the damaged regions
            // of this new buffer into the shadow copy.
            true => scene.update_resource_from_bits(
                &shadow,
                &pixels,
                shm_buffer.sb_width as u32,
                shm_buffer.sb_height as u32,
                0,
                dak::dom::Format::ARGB8888,
                self.a_buffer_damage.take(&surf),
            ),
            // If the shadow resource is not defined, define it now using the
            // buffers contents
            false => scene.define_resource_from_bits(
                &shadow,
                &pixels,
                shm_buffer.sb_width as u32,
                shm_buffer.sb_height as u32,
                0,
                dak::dom::Format::ARGB8888,
            ),
        } {
            buffer.post_error(
                wl_shm::Error::InvalidFd as u32,
                format!("Error Importing Shm Buffer: {:?}", e),
            );
            return Err(e.context("Failed to import Shm Buffer"));
        }

        // Release the new buffer immediately so the app can reuse it
        buffer.release();
        // Now we can (re)bind it to this surface
        self.a_surf_resource.set(&surf, shadow);

        Ok(())
    }

    /// Set the damage for this surface
    /// This will be added once a frame, and then cleared before the next.
    pub fn set_surface_damage(&mut self, id: &SurfaceId, damage: dak::Damage) {
        self.a_surface_damage.set(id, damage)
    }
    /// For efficiency, this takes the damage so that we can avoid
    /// copying it
    pub fn take_surface_damage(&mut self, id: &SurfaceId) -> Option<dak::Damage> {
        self.a_surface_damage.take(id)
    }

    /// Set the damage for this window's buffer
    /// This is the same as set_surface_damage, but operates on buffer coordinates.
    /// It is the preferred method.
    pub fn set_buffer_damage(&mut self, id: &SurfaceId, damage: dak::Damage) {
        self.a_buffer_damage.set(id, damage)
    }
    /// For efficiency, this takes the damage so that we can avoid
    /// copying it
    pub fn take_buffer_damage(&mut self, id: &SurfaceId) -> Option<dak::Damage> {
        self.a_buffer_damage.take(id)
    }

    /// Update the cursor image
    pub fn set_cursor(&mut self, id: Option<SurfaceId>) {
        self.set_cursor_surface(id.clone());
        self.add_wm_task(wm::task::Task::set_cursor { id: id });
    }

    /// Add an offset to the cursor patch
    ///
    /// This increments the cursor position, which will later
    /// get replayed into the hemisphere
    pub fn add_cursor_pos(&mut self, dx: f64, dy: f64) {
        let pos = self.get_cursor_pos();
        self.set_cursor_pos((pos.0 + dx, pos.1 + dy));

        // Now update the grabbed window if it exists
        let grabbed = match self.get_grabbed() {
            Some(g) => g,
            None => return,
        };

        // Need to update both the surface and window positions
        let mut gpos = *self.a_window_pos.get(&grabbed).unwrap();
        gpos.0 += dx as f32;
        gpos.1 += dy as f32;
        self.a_window_pos.set(&grabbed, (gpos.0, gpos.1));

        let mut gpos = *self.a_surface_pos.get(&grabbed).unwrap();
        gpos.0 += dx as f32;
        gpos.1 += dy as f32;
        self.a_surface_pos.set(&grabbed, (gpos.0, gpos.1));
    }

    // -- subsystem specific handlers --

    /// These are getters for the private wayland structures
    /// that do not get shared across hemispheres
    pub fn add_surface(&mut self, id: &SurfaceId, surf: Arc<Mutex<Surface>>) {
        self.a_surface.set(id, surf)
    }

    /// Grab our Surface struct for this id
    pub fn get_surface_from_id(&self, id: &SurfaceId) -> Option<Arc<Mutex<Surface>>> {
        self.a_surface.get_clone(id)
    }

    /// Grab the wayland protocol object wl_surface for this id
    pub fn get_wl_surface_from_id(&self, id: &SurfaceId) -> Option<wl_surface::WlSurface> {
        self.a_wl_surface.get_clone(id)
    }
    pub fn set_wl_surface(&mut self, id: &SurfaceId, surf: wl_surface::WlSurface) {
        self.a_wl_surface.set(id, surf);
    }

    pub fn add_seat(&mut self, id: &ClientId, seat: Arc<Mutex<Seat>>) {
        self.a_seat.set(id, seat);
    }

    pub fn get_seat_from_surface_id(&self, id: &SurfaceId) -> Option<Arc<Mutex<Seat>>> {
        // get the client id
        let owner = self.a_owner.get_clone(id).unwrap();
        self.get_seat_from_client_id(&owner)
    }
    pub fn get_seat_from_client_id(&self, id: &ClientId) -> Option<Arc<Mutex<Seat>>> {
        self.a_seat.get_clone(id).clone()
    }

    /// Signal any registered frame callbacks
    /// TODO: actually do optimizations
    ///
    /// Wayland uses these callbacks to tell apps when they should
    /// redraw themselves. If they aren't on screen we don't send
    /// the callback so it doesn't use the power.
    pub fn send_frame_callbacks_for_surf(&mut self, id: &SurfaceId) {
        log::debug!("Sending frame callbacks for Surf {:?}", id);
        // get each valid id in the mapping
        // get the refcell for the surface for this id
        if let Some(mut cbs) = self.a_frame_callbacks.get_mut(id) {
            for callback in cbs.drain(0..) {
                // frame callbacks are signaled in the order that they
                // were submitted in
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
