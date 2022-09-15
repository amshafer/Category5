//! Lluvia - A stripped down Entity Component System that allows for no-nonsense
//! data storage in finite time.
//!
//! This library lets you quickly throw together large collections of objects
//! with varying lifetimes into one ECS. You specify the `Components`, create
//! any number of reference counted `Entity` objects, and when an `Entity` goes
//! out of scope its data will be automatically dropped as well. You can even
//! store `Entity` objects as components of other entities, and everything will
//! get dropped at once when the root `Entity` goes out of scope.
//!
//! What sets this ECS apart is that it is very fast, very small in
//! scope, and has a very small footprint. The implementation is ~500 lines,
//! it has zero dependencies, and almost all operations run in O(1) time.
//! There is no archetyping, there is no rayon integration, there is no
//! advanced iterator pattern, and there is no multi-threaded access. Emphasis
//! is placed on minimizing complexity and avoiding scanning or re-organizing
//! data, as Lluvia was designed to be the data engine for low-latency graphics
//! programs.
//!
//! Lluvia begins with creating an `Instance` object. This will track the
//! validity of `Entity` objects in the system, and will hold references
//! to data tables used for storage.
//!
//! The `Instance` can then be used to add `Component` tables, and access
//! them using a `Session` object. The `Session` allows for getting and
//! setting components for each `Entity`.
//!
//! Basic usage looks like:
//! ```
//! use lluvia as ll;
//! // Create the ECS holder
//! let mut inst = ll::Instance::new();
//! // Make a new entity
//! let entity = inst.add_entity();
//!
//! // Now add our component. This will be a string, but
//! // we don't have to specify that for now
//! let c = inst.add_component();
//!
//! // Get a session to access data for component c. This
//! // allows access to the per-entity data for this component and
//! // lets us perform queries.
//! let mut sesh = inst.open_session(c).unwrap();
//!
//! // Before querying the value, we first need to set a valid value
//! // for this component. Afterwards, we can get it and check that
//! // it is unchanged.
//! sesh.set(&entity, "Hola Lluvia");
//! let data_ref = sesh.get(&entity).unwrap();
//! assert_eq!(*data_ref, "Hola Lluvia");
//! ```
// Austin Shafer - 2022

use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

#[cfg(test)]
mod tests;

pub struct EntityInternal {
    ecs_inst: Instance,
    ecs_id: usize,
}

impl fmt::Debug for EntityInternal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntityInternal")
            .field("ecs_id", &self.ecs_id)
            .finish()
    }
}

impl EntityInternal {
    /// Gets the raw index offset for this entity
    pub fn get_raw_id(&self) -> usize {
        self.ecs_id
    }
}

impl Drop for EntityInternal {
    fn drop(&mut self) {
        self.ecs_inst.invalidate_id(self.ecs_id);
    }
}

/// An abstract Entity
///
/// This gives an entity an identity, it holds a reference
/// to the instance it was allocated from, and is used to
/// index into a table to get data belonging to this
/// entity. An Id needs to be requested from the instance.
///
/// ```
/// use lluvia as ll;
/// let mut inst = ll::Instance::new();
/// let id = inst.add_entity();
/// // Use clone to duplicate the id to be used
/// // from multiple lifetimes.
/// let id_dup = id.clone();
/// ```
pub type Entity = Rc<EntityInternal>;

#[derive(Copy, Clone)]
pub struct Component<T: 'static> {
    c_index: usize,
    _c_phantom: PhantomData<T>,
}

/// A component table wrapper trait
///
/// This lets us do some type-agnostic operations on a table from
/// the Instance without having to do a full downcast call. This
/// prevents the Instance from having to keep track of type state.
trait ComponentTable {
    /// Set an entity value to None and throw away the value
    fn clear_entity(&self, id: usize);

    /// This function allows us to transform into an Any object, so
    /// that we can perform downcasting to Table<T>.
    fn as_any(&self) -> &dyn Any;

    fn as_mut_any(&mut self) -> &mut dyn Any;
}

/// A table containing a series of optional values.
///
/// This is indexed by the Entity.ecs_id field.
#[derive(Debug)]
pub struct TableInternal<T: 'static> {
    t_entity: Vec<Option<T>>,
}

#[derive(Debug)]
pub struct Table<T: 'static> {
    t_internal: Rc<RefCell<TableInternal<T>>>,
}

impl<T: 'static> Clone for Table<T> {
    fn clone(&self) -> Self {
        Self {
            t_internal: self.t_internal.clone(),
        }
    }
}

impl<T: 'static> ComponentTable for Table<T> {
    fn clear_entity(&self, id: usize) {
        let _val = {
            // Take the data and don't drop it until we have dropped our RefMut
            let mut internal = self.t_internal.borrow_mut();
            if id >= internal.t_entity.len() {
                return;
            }
            internal.t_entity[id].take()
        };
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl<T: 'static> Table<T> {
    pub fn new() -> Self {
        Self {
            t_internal: Rc::new(RefCell::new(TableInternal {
                t_entity: Vec::new(),
            })),
        }
    }

    /// Get the number of valid ids in the system.
    ///
    /// This is the same as the parent ECSInstance's num_entities.
    pub fn num_entities(&self) -> usize {
        self.t_internal.borrow().t_entity.len()
    }

    fn has_space_for_id(&self, entity: &Entity) -> bool {
        entity.ecs_id < self.t_internal.borrow().t_entity.len()
    }

    /// Helper to grow our internal array to fit entity
    fn ensure_space_for_id(&mut self, entity: &Entity) {
        let mut internal = self.t_internal.borrow_mut();

        // First handle any resizing that needs to occur
        if entity.ecs_id >= internal.t_entity.len() {
            let new_size = (entity.ecs_id + 1).next_power_of_two();
            internal.t_entity.resize_with(new_size, || None);
        }
    }
}

pub struct InstanceInternal {
    /// The number of ids we have allocated out of the list above. This
    /// allows us to optimize the case of a full id list: We don't have to
    /// scan the entire list if it's full
    i_total_num_ids: usize,
    /// This is a list of active ids in the system.
    i_valid_ids: Vec<bool>,
}

pub struct ComponentList {
    /// This is the component table list.
    ///
    /// It is a series of RefCells so that individual sessions can access
    /// different component sets mutably at the same time.
    cl_components: Vec<Box<dyn ComponentTable>>,
}

/// An Entity component system.
///
/// This tracks the validity of various ids. It also holds a series of reference
/// counted internal component tables where all of the data in the system is stored.
///
/// We keep the instance id tracking and the component set data with separate
/// interior mutability so that we can do a fun little dance to handle the case
/// of storing an Entity as data inside a component.
///   * Invalidate the id in our tracking set
///   * Dispatch a call into each table
///   * The table will take the data out of its entry, without dropping it (to
///     avoid retriggering the process)
///   * The table releases its internal borrows, so all mutable borrows have ended
///   * The data is dropped
pub struct Instance {
    i_internal: Rc<RefCell<InstanceInternal>>,
    i_component_set: Rc<RefCell<ComponentList>>,
}

impl Clone for Instance {
    fn clone(&self) -> Self {
        Self {
            i_internal: self.i_internal.clone(),
            i_component_set: self.i_component_set.clone(),
        }
    }
}

impl Instance {
    /// Create a new global Entity Component System
    pub fn new() -> Self {
        Self {
            i_internal: Rc::new(RefCell::new(InstanceInternal {
                i_valid_ids: Vec::new(),
                i_total_num_ids: 0,
            })),
            i_component_set: Rc::new(RefCell::new(ComponentList {
                cl_components: Vec::new(),
            })),
        }
    }

    /// Get the total number of entities that have been allocated.
    ///
    /// This returns the number of "live" ids
    pub fn num_entities(&self) -> usize {
        self.i_internal.borrow().i_total_num_ids
    }

    /// Get the largest entity value
    ///
    /// This is essentially the capacity of the entity array
    pub fn capacity(&self) -> usize {
        self.i_internal.borrow().i_valid_ids.len()
    }

    /// Allocate a new component table
    ///
    /// Components are essentially the data in this system. Each entity may have a piece
    /// of data stored for each component. Components have a generic data type for the
    /// data they store, and Entities are not required to have a populated value.
    pub fn add_component<T: 'static>(&mut self) -> Component<T> {
        let mut cl = self.i_component_set.borrow_mut();

        let component_id = cl.cl_components.len();
        let new_table: Table<T> = Table::new();
        cl.cl_components.push(Box::new(new_table));

        Component {
            c_index: component_id,
            _c_phantom: PhantomData,
        }
    }

    /// Add a new entity to the system
    ///
    /// This will mint an Id for the new entity, tracked by the `Entity` struct. This
    /// is a reference counted id that will free all populated component values when the
    /// entity is dropped.
    ///
    /// This function allocates a new internal id for the entity, and returns its tracking
    /// structure. There is non-zero time spent to find an old, free id value to recycle.
    pub fn add_entity(&mut self) -> Entity {
        let new_self = self.clone();
        let mut internal = self.i_internal.borrow_mut();

        // Find the first free id that is not in use or make one
        let first_valid_id = {
            let mut index = None;

            // first check the array from back to front
            // Don't do this if the array is full, just skip to extending the
            // array if that is the case
            if internal.i_total_num_ids != internal.i_valid_ids.len() {
                for (i, is_valid) in internal.i_valid_ids.iter().enumerate().rev() {
                    if !*is_valid {
                        index = Some(i);
                        break;
                    }
                }
            }

            // if that didn't work then add one to the back
            if index.is_none() {
                internal.i_valid_ids.push(true);
                index = Some(internal.i_valid_ids.len() - 1);
            }

            index.unwrap()
        };
        // Mark this new id as active
        internal.i_valid_ids[first_valid_id] = true;
        internal.i_total_num_ids += 1;

        return Rc::new(EntityInternal {
            ecs_id: first_valid_id,
            ecs_inst: new_self,
        });
    }

    /// Invalidate an Entity and free all of its component values
    fn invalidate_id(&mut self, id: usize) {
        // First remove this id from the valid list
        {
            let mut internal = self.i_internal.borrow_mut();
            assert!(internal.i_valid_ids[id]);
            internal.i_valid_ids[id] = false;
            internal.i_total_num_ids -= 1;
        }

        // Now that we have dropped our ref for the id tracking we can
        // tell each table to free the entity
        {
            let cl = self.i_component_set.borrow();
            for table in cl.cl_components.iter() {
                table.clear_entity(id);
            }
        }
    }

    fn id_is_valid(&self, id: &Entity) -> bool {
        let internal = self.i_internal.borrow();

        id.ecs_id < internal.i_valid_ids.len() && internal.i_valid_ids[id.ecs_id]
    }

    fn component_is_valid<T: 'static>(&self, component: &Component<T>) -> bool {
        let cl = self.i_component_set.borrow();

        component.c_index < cl.cl_components.len()
    }

    /// Open a Session for the specified component
    ///
    /// A session will provide query access for the values of this component
    /// for each entity. We will branch off a session and return it, allowing the
    /// user to interact with data.
    ///
    /// If `component` is invalid, return None.
    pub fn open_session<T: 'static>(&self, component: Component<T>) -> Option<Session<T>> {
        // validate component
        if !self.component_is_valid(&component) {
            return None;
        }

        let cl = self.i_component_set.borrow();

        // Ensure that this component table is of the right type
        if let Some(table) = cl.cl_components[component.c_index]
            .as_any()
            .downcast_ref::<Table<T>>()
        {
            let new_inst = self.clone();
            return Some(Session {
                s_inst: new_inst,
                _s_phantom: PhantomData,
                s_table: table.clone(),
                s_component: component,
            });
        }
        None
    }
}

/// A Session providing access to a component
///
/// The Session abstraction allows for getting a separate reference
/// to the data table holding the populated values of entities and
/// components. This is where all the querying information can happen and
/// must be opened from the Instance.
///
/// Sessions really wrap RefCells used internally to hold data, and therefore the
/// same runtime validation rules apply. There can be many outstanding read-only
/// references in usage but only one mutable reference at a time.
///
/// Additionally, you must keep in mind that Entities will also adhere to these
/// rules when dropped, as they must free their data held in the internal component
/// data tables. You should not have long-outsanding references floating around
/// that will cause a panic when an Entity is dropped and the Session is already
/// borrowed.
pub struct Session<T: 'static> {
    s_inst: Instance,
    _s_phantom: PhantomData<T>,
    s_component: Component<T>,
    s_table: Table<T>,
}

impl<T: 'static> fmt::Debug for Session<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("s_component", &self.s_component.c_index)
            .finish()
    }
}

impl<T: 'static> Session<T> {
    /// Get a reference to data corresponding to the (component, entity) pair
    ///
    /// This provides read-only access to the component value for an Entity. This
    /// is the primary way through which data should be queried from the system.
    ///
    /// This will return a Ref to the underlying component, if it exists. The
    /// ref holds open the internal refcell that this ECS instance uses, so be
    /// careful not to drop any Entitys or request any other components
    /// while this ref is in scope.
    ///
    /// If this entity has not had a value set, None will be returned.
    pub fn get(&self, entity: &Entity) -> Option<Ref<T>> {
        if !self.s_inst.id_is_valid(entity) || !self.s_table.has_space_for_id(entity) {
            return None;
        }

        let table_internal = self.s_table.t_internal.borrow();
        if table_internal.t_entity[entity.ecs_id].is_none() {
            return None;
        }

        return Some(Ref::map(table_internal, |i| {
            i.t_entity[entity.ecs_id].as_ref().unwrap()
        }));
    }

    /// Get a mutable reference to data corresponding to the (component, entity) pair
    ///
    /// This is the same as the `get` method, but for mutable references. Keep in
    /// mind that only one RefMut from the Component corresponding to this
    /// Session can be active, and trying to get a second mutable reference will
    /// panic.
    ///
    /// The `set` method must be called before this can be used, or else the
    /// value of the entity for this property cannot be determined and None
    /// will be returned.
    pub fn get_mut(&mut self, entity: &Entity) -> Option<RefMut<T>> {
        if !self.s_inst.id_is_valid(entity) || !self.s_table.has_space_for_id(entity) {
            return None;
        }

        let table_internal = self.s_table.t_internal.borrow_mut();
        if table_internal.t_entity[entity.ecs_id].is_none() {
            return None;
        }

        return Some(RefMut::map(table_internal, |i| {
            i.t_entity[entity.ecs_id].as_mut().unwrap()
        }));
    }

    /// Set the value of an entity for the component corresponding to this session
    ///
    /// This is the first thing that should be called when populating a value for
    /// the entity. This will set the initial value, which can then be modified
    /// with `get_mut`
    pub fn set(&mut self, entity: &Entity, val: T) {
        assert!(self.s_inst.id_is_valid(entity));

        // First grow our internal storage if necessary
        self.s_table.ensure_space_for_id(entity);

        let mut table_internal = self.s_table.t_internal.borrow_mut();
        table_internal.t_entity[entity.ecs_id] = Some(val);
    }

    /// Take a value out of the component table
    ///
    /// This is the opposite of `set`. It will unset the value of the component for this
    /// entity and will return the value that was stored there. The component entry will
    /// be undefined after this.
    pub fn take(&mut self, entity: &Entity) -> Option<T> {
        if !self.s_inst.id_is_valid(entity) {
            return None;
        }

        // First grow our internal storage if necessary
        self.s_table.ensure_space_for_id(entity);

        let mut table_internal = self.s_table.t_internal.borrow_mut();
        table_internal.t_entity[entity.ecs_id].take()
    }

    /// Create an iterator over all values in this component table
    ///
    /// This will return Option values for each entry in the internal
    /// component array. None values can be returned by the iterator,
    /// as it allows for you to use `.enumerate()` to mirror the
    /// component table into other resources.
    pub fn iter<'a>(&'a self) -> SessionIterator<'a, T> {
        SessionIterator {
            si_session: self,
            si_cur: 0,
        }
    }
}

pub struct SessionIterator<'a, T: 'static> {
    si_session: &'a Session<T>,
    si_cur: usize,
}

impl<'a, T: 'static> Iterator for SessionIterator<'a, T> {
    type Item = Option<Ref<'a, T>>;

    fn next(&mut self) -> Option<Self::Item> {
        let table_internal = self.si_session.s_table.t_internal.borrow();
        let cur = self.si_cur;
        self.si_cur += 1;

        if cur >= table_internal.t_entity.len() {
            return None;
        }
        if table_internal.t_entity[cur].is_none() {
            return Some(None);
        }

        let ret = Ref::map(table_internal, |t| t.t_entity[cur].as_ref().unwrap());

        return Some(Some(ret));
    }
}
