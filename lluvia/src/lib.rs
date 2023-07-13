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
//! The `Instance` can then be used to add `Component` tables. The `Component` allows for getting
//! and setting components for each `Entity`.
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
//! let mut c = inst.add_component();
//!
//! // Before querying the value, we first need to set a valid value
//! // for this component. Afterwards, we can get it and check that
//! // it is unchanged.
//! c.set(&entity, "Hola Lluvia");
//! let data_ref = c.get(&entity).unwrap();
//! assert_eq!(*data_ref, "Hola Lluvia");
//! ```
// Austin Shafer - 2022

use std::any::Any;
use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::{atomic::AtomicBool, Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(test)]
mod tests;

#[derive(Debug)]
enum TableRefEntityType {
    /// A reference tracked entity
    Entity(Entity),
    /// A offset into the table. This is only to be used for the iterator
    /// implementations.
    Offset(usize),
}

/// The storage backend for a particular table
///
/// Lluvia lets you choose between a couple different memory layouts
/// for components, which can help if you know the performance characteristics
/// of your data. This trait allows uniform access to these storage types.
pub trait Container<T: 'static> {
    fn index(&self, index: usize) -> Option<&T>;
    fn index_mut(&mut self, index: usize) -> Option<&mut T>;
    fn set(&mut self, index: usize, val: T);
    fn take(&mut self, index: usize) -> Option<T>;
    fn get_next_id(&self, index: usize) -> Option<usize>;
    fn clear(&mut self);
}

/// Our basic vector storage
///
/// This is the default container type meant for general purpose
/// usage. This storage is presented as a congruent array, but in
/// reality it is a series of blocks allocated when their data is
/// filled in. This essentially makes it a sparse vector, allowing
/// lluvia to support high client counts in frequently unused tables
/// without wasting space.
pub struct VecContainer<T: 'static> {
    v_block_size: usize,
    v_blocks: Vec<Option<VCBlock<T>>>,
}

struct VCBlock<T: 'static> {
    v_vec: Vec<Option<T>>,
}

/// Arbitrarily chosen size of the blocks in Lluvia's sparse block allocator.
const DEFAULT_LLUVIA_BLOCK_SIZE: usize = 32;

impl<T: 'static> VecContainer<T> {
    /// Helper that turns a global index into a block + offset index pair
    fn get_indices(&self, index: usize) -> (usize, usize) {
        (index / self.v_block_size, index % self.v_block_size)
    }

    /// Makes a final, returnable index from our (block, offset) pair
    fn make_index(&self, block: usize, index: usize) -> usize {
        block * self.v_block_size + index
    }

    /// Ensure that we have a block allocated for this index. Dynamic allocation
    /// is done here.
    fn ensure_space_for_id(&mut self, index: usize) {
        let (bi, i) = self.get_indices(index);

        if bi >= self.v_blocks.len() {
            self.v_blocks.resize_with(bi + 1, || None);
        }

        if self.v_blocks[bi].is_none() {
            // set up a new empty block
            let mut new_vec = Vec::new();
            for _ in 0..self.v_block_size {
                new_vec.push(None);
            }

            assert!(i < new_vec.len());
            self.v_blocks[bi] = Some(VCBlock { v_vec: new_vec });
        }
    }
}

impl<T: 'static> Container<T> for VecContainer<T> {
    fn index(&self, index: usize) -> Option<&T> {
        let (bi, i) = self.get_indices(index);
        // If the block index is too large or the block doesn't exist or the
        // index is too large for the block return None
        if bi >= self.v_blocks.len()
            || self.v_blocks[bi].is_none()
            || i >= self.v_blocks[bi].as_ref().unwrap().v_vec.len()
        {
            return None;
        }
        self.v_blocks[bi].as_ref().unwrap().v_vec[i].as_ref()
    }
    fn index_mut(&mut self, index: usize) -> Option<&mut T> {
        self.ensure_space_for_id(index);

        let (bi, i) = self.get_indices(index);
        assert!(bi < self.v_blocks.len());
        self.v_blocks[bi].as_mut().unwrap().v_vec[i].as_mut()
    }
    fn set(&mut self, index: usize, val: T) {
        self.ensure_space_for_id(index);

        let (bi, i) = self.get_indices(index);
        assert!(bi < self.v_blocks.len());
        self.v_blocks[bi].as_mut().unwrap().v_vec[i] = Some(val);
    }
    fn take(&mut self, index: usize) -> Option<T> {
        self.ensure_space_for_id(index);

        let (bi, i) = self.get_indices(index);
        if bi >= self.v_blocks.len() {
            return None;
        }
        self.v_blocks[bi].as_mut().unwrap().v_vec[i].take()
    }
    fn get_next_id(&self, index: usize) -> Option<usize> {
        let (bi, block_offset) = self.get_indices(index);
        if bi >= self.v_blocks.len() {
            return None;
        }

        let mut offset = Some(block_offset + 1);
        // Test all remaining blocks, starting with the current one
        for block_index in bi..self.v_blocks.len() {
            if let Some(block) = self.v_blocks[block_index].as_ref() {
                // if this is the first block, then start from our index's
                // offset. if not, start at the beginning
                let start_index = match offset.take() {
                    Some(off) => off,
                    None => 0,
                };
                // Now crawl this block and see if we find a valid index
                for i in (start_index)..block.v_vec.len() {
                    if block.v_vec[i].is_some() {
                        return Some(self.make_index(block_index, i));
                    }
                }
            }
        }

        None
    }
    fn clear(&mut self) {
        for b in self.v_blocks.iter_mut() {
            if let Some(block) = b {
                for item in block.v_vec.iter_mut() {
                    *item = None;
                }
            }
        }
    }
}

/// A continuous non-space slice container
///
/// This container is a continuously allocated Vec that is guaranteed to
/// be contiguous. The internal storage is `Vec<T>`. This means all values
/// stored can be accessed as a total slice, without being wrapped in any
/// data types of any sort. The down side is more memory usage since it is
/// not sparse and having to provide a default value.
pub struct SliceContainer<T: 'static> {
    v_callback: Box<dyn Fn() -> T>,
    v_vec: Vec<T>,
}

impl<T: 'static> SliceContainer<T> {
    fn ensure_space_for_id(&mut self, index: usize) {
        // Kind of a pain here, we have to pass a reference to
        // the closure and have to deref it out of its dyn box first
        if index >= self.v_vec.len() {
            self.v_vec.resize_with(index + 1, &*self.v_callback);
        }
    }

    /// Get the slice of the backing array
    fn as_slice<'a>(&'a self) -> &'a [T] {
        self.v_vec.as_slice()
    }
}

impl<T: 'static> Container<T> for SliceContainer<T> {
    fn index(&self, index: usize) -> Option<&T> {
        if index >= self.v_vec.len() {
            return None;
        }
        Some(&self.v_vec[index])
    }
    fn index_mut(&mut self, index: usize) -> Option<&mut T> {
        self.ensure_space_for_id(index);
        Some(&mut self.v_vec[index])
    }
    fn set(&mut self, index: usize, val: T) {
        self.ensure_space_for_id(index);
        self.v_vec[index] = val;
    }
    /// The slice container doesn't have a concept of "set" vs "unset",
    /// it's just defined value vs default value provided from a callback.
    /// This will always return Some()
    fn take(&mut self, index: usize) -> Option<T> {
        self.ensure_space_for_id(index);
        let mut tmp = (self.v_callback)();
        std::mem::swap(&mut self.v_vec[index], &mut tmp);
        Some(tmp)
    }
    fn get_next_id(&self, index: usize) -> Option<usize> {
        if index + 1 >= self.v_vec.len() {
            return None;
        }

        Some(index + 1)
    }
    fn clear(&mut self) {
        for item in self.v_vec.iter_mut() {
            *item = (self.v_callback)();
        }
    }
}

#[derive(Debug)]
pub struct TableRef<'a, T: 'static, C: Container<T> + 'static> {
    /// The lock guard returned from the table
    tr_guard: RwLockReadGuard<'a, TableInternal<T, C>>,
    /// The entity we are operating on
    tr_entity: TableRefEntityType,
}

impl<'a, T: 'static, C: Container<T> + 'static> Deref for TableRef<'a, T, C> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.tr_guard
            .t_entity
            .index(match &self.tr_entity {
                TableRefEntityType::Entity(entity) => entity.ecs_id,
                TableRefEntityType::Offset(off) => *off,
            })
            .as_ref()
            .unwrap()
    }
}

#[derive(Debug)]
pub struct TableRefMut<'a, T: 'static, C: Container<T> + 'static> {
    /// The lock guard returned from the table
    tr_guard: RwLockWriteGuard<'a, TableInternal<T, C>>,
    /// The entity we are operating on
    tr_entity: Entity,
}

impl<'a, T: 'static, C: Container<T> + 'static> Deref for TableRefMut<'a, T, C> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.tr_guard
            .t_entity
            .index(self.tr_entity.ecs_id)
            .as_ref()
            .unwrap()
    }
}
impl<'a, T: 'static, C: Container<T> + 'static> DerefMut for TableRefMut<'a, T, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.tr_guard
            .t_entity
            .index_mut(self.tr_entity.ecs_id)
            .unwrap()
    }
}

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
pub type Entity = Arc<EntityInternal>;

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
pub struct TableInternal<T: 'static, C: Container<T> + 'static> {
    t_entity: C,
    _t_phantom: PhantomData<T>,
}

#[derive(Debug)]
pub struct Table<T: 'static, C: Container<T> + 'static> {
    t_internal: Arc<RwLock<TableInternal<T, C>>>,
}

impl<T: 'static, C: Container<T> + 'static> Clone for Table<T, C> {
    fn clone(&self) -> Self {
        Self {
            t_internal: self.t_internal.clone(),
        }
    }
}

impl<T: 'static, C: Container<T> + 'static> ComponentTable for Table<T, C> {
    fn clear_entity(&self, id: usize) {
        let _val = {
            // Take the data and don't drop it until we have dropped our RefMut
            self.t_internal.write().unwrap().t_entity.take(id)
        };
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_mut_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl<T: 'static, C: Container<T> + 'static> Table<T, C> {
    pub fn new(container: C) -> Self {
        Self {
            t_internal: Arc::new(RwLock::new(TableInternal {
                t_entity: container,
                _t_phantom: PhantomData,
            })),
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
#[derive(Clone)]
pub struct Instance {
    i_internal: Arc<RwLock<InstanceInternal>>,
    i_component_set: Arc<RwLock<ComponentList>>,
}

impl Instance {
    /// Create a new global Entity Component System
    pub fn new() -> Self {
        Self {
            i_internal: Arc::new(RwLock::new(InstanceInternal {
                i_valid_ids: Vec::new(),
                i_total_num_ids: 0,
            })),
            i_component_set: Arc::new(RwLock::new(ComponentList {
                cl_components: Vec::new(),
            })),
        }
    }

    /// Get the total number of entities that have been allocated.
    ///
    /// This returns the number of "live" ids
    pub fn num_entities(&self) -> usize {
        self.i_internal.read().unwrap().i_total_num_ids
    }

    /// Get the largest entity value
    ///
    /// This is essentially the capacity of the entity array
    pub fn capacity(&self) -> usize {
        self.i_internal.read().unwrap().i_valid_ids.len()
    }

    /// Allocate a new component table
    ///
    /// Components are essentially the data in this system. Each entity may have a piece
    /// of data stored for each component. Components have a generic data type for the
    /// data they store, and Entities are not required to have a populated value.
    ///
    /// This uses the default storage container which supports sparse memory usage.
    pub fn add_component<T: 'static>(&mut self) -> Component<T> {
        self.add_raw_component(VecContainer {
            v_block_size: DEFAULT_LLUVIA_BLOCK_SIZE,
            v_blocks: Vec::new(),
        })
    }

    /// Allocate a new component table with contiguous storage
    ///
    /// This is the same as `add_component`, but will use a different storage backend
    /// which stores all data in one continuous array (not sparse). This can be useful
    /// if you need to fill in a array that you want to hand off as a slice to some other
    /// library, and don't want to copy between lluvia and another storage type.
    ///
    /// To use this you must provide a callback which will be used to fill in default
    /// values in the backing array. This is necessary since the backing storage is
    /// of type `&[T]`, and there needs to be a valid `T` value placed in every cell
    /// even if it has no associated entity.
    pub fn add_non_sparse_component<T: 'static, F>(&mut self, callback: F) -> NonSparseComponent<T>
    where
        F: Fn() -> T + 'static,
    {
        self.add_raw_component(SliceContainer {
            v_vec: Vec::new(),
            v_callback: Box::new(callback),
        })
    }

    /// Add a component of the given containe type. This is an internal helper.
    fn add_raw_component<T: 'static, C: Container<T> + 'static>(
        &mut self,
        container: C,
    ) -> RawComponent<T, C> {
        let mut cl = self.i_component_set.write().unwrap();

        let component_id = cl.cl_components.len();
        let new_table = Table::new(container);
        cl.cl_components.push(Box::new(new_table));

        let table = cl.cl_components[component_id]
            .as_any()
            .downcast_ref::<Table<T, C>>()
            .unwrap();

        let new_inst = self.clone();
        return RawComponent {
            c_inst: new_inst,
            _c_phantom: PhantomData,
            c_table: table.clone(),
            c_component_id: component_id,
            c_modified: Arc::new(AtomicBool::new(false)),
        };
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
        let mut internal = self.i_internal.write().unwrap();

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

        return Arc::new(EntityInternal {
            ecs_id: first_valid_id,
            ecs_inst: new_self,
        });
    }

    /// Invalidate an Entity and free all of its component values
    fn invalidate_id(&mut self, id: usize) {
        // First remove this id from the valid list
        {
            let mut internal = self.i_internal.write().unwrap();
            assert!(internal.i_valid_ids[id]);
            internal.i_valid_ids[id] = false;
            internal.i_total_num_ids -= 1;
        }

        // Now that we have dropped our ref for the id tracking we can
        // tell each table to free the entity
        {
            let cl = self.i_component_set.read().unwrap();
            for table in cl.cl_components.iter() {
                table.clear_entity(id);
            }
        }
    }

    fn id_is_valid(&self, id: &Entity) -> bool {
        let internal = self.i_internal.read().unwrap();

        id.ecs_id < internal.i_valid_ids.len() && internal.i_valid_ids[id.ecs_id]
    }
}

/// A Component holding values for each Entity
///
/// Each Component in the system is really a key-value store for each
/// Entity: values of type `T` can be retrieved by fetching the entry
/// for an Entity. Components have an internal Table which holds an
/// array of values indexed by the Entity's id.
pub struct RawComponent<T: 'static, C: Container<T> + 'static> {
    /// Instance this component belongs to
    c_inst: Instance,
    _c_phantom: PhantomData<T>,
    /// The index into the component data list inside c_inst
    c_component_id: usize,
    /// The storage table holding per-entity values
    c_table: Table<T, C>,
    /// Marked true when this component table has outstanding changes
    /// not processed by the user.
    c_modified: Arc<AtomicBool>,
}

/// General Purpose Component
///
/// This is the default component type. See `RawComponent` for an overview
/// of the component object, this type defines a component whose internal
/// memory tracking is "sparse". This is the best all-purpose component type
/// as it will not overallocate when only a few entities have assigned values
/// of this component.
pub type Component<T> = RawComponent<T, VecContainer<T>>;

/// Component with non-sparse data allocation
///
/// This Component type uses a contiguous array internally to track data. This
/// is useful for when you need to be able to pass a slice of `T` objects to
/// other libraries without copying. This is not well suited for scenarios where
/// you have a lot of entities and only a few of them have values.
pub type NonSparseComponent<T> = RawComponent<T, SliceContainer<T>>;

impl<T: 'static, C: Container<T> + 'static> fmt::Debug for RawComponent<T, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("c_component", &self.c_component_id)
            .finish()
    }
}

impl<T: 'static, C: Container<T> + 'static> Clone for RawComponent<T, C> {
    fn clone(&self) -> Self {
        Self {
            c_inst: self.c_inst.clone(),
            _c_phantom: PhantomData,
            c_component_id: self.c_component_id,
            c_table: self.c_table.clone(),
            c_modified: self.c_modified.clone(),
        }
    }
}

impl<T: 'static, C: Container<T> + 'static> RawComponent<T, C> {
    /// Get if changes have taken place in this Component
    ///
    /// This can be used to check if a property has been updated for some number
    /// of entities, if you need to take action based on that.
    pub fn is_modified(&self) -> bool {
        self.c_modified.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Reset the modified tracker
    pub fn clear_modified(&mut self) {
        self.c_modified
            .store(false, std::sync::atomic::Ordering::Release);
    }

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
    pub fn get(&self, entity: &Entity) -> Option<TableRef<T, C>> {
        if !self.c_inst.id_is_valid(entity) {
            return None;
        }

        let table_internal = self.c_table.t_internal.read().unwrap();
        if table_internal.t_entity.index(entity.ecs_id).is_none() {
            return None;
        }

        return Some(TableRef {
            tr_guard: table_internal,
            tr_entity: TableRefEntityType::Entity(entity.clone()),
        });
    }

    /// Get a mutable reference to data corresponding to the (component, entity) pair
    ///
    /// This is the same as the `get` method, but for mutable references. Keep in
    /// mind that only one TableRefMut from the Component corresponding to this
    /// Session can be active, and trying to get a second mutable reference will
    /// panic.
    ///
    /// The `set` method must be called before this can be used, or else the
    /// value of the entity for this property cannot be determined and None
    /// will be returned.
    pub fn get_mut(&mut self, entity: &Entity) -> Option<TableRefMut<T, C>> {
        if !self.c_inst.id_is_valid(entity) {
            return None;
        }

        let table_internal = self.c_table.t_internal.write().unwrap();
        if table_internal.t_entity.index(entity.ecs_id).is_none() {
            return None;
        }

        self.c_modified
            .store(true, std::sync::atomic::Ordering::Release);

        return Some(TableRefMut {
            tr_guard: table_internal,
            tr_entity: entity.clone(),
        });
    }

    /// Set the value of an entity for the component corresponding to this session
    ///
    /// This is the first thing that should be called when populating a value for
    /// the entity. This will set the initial value, which can then be modified
    /// with `get_mut`
    pub fn set(&mut self, entity: &Entity, val: T) {
        assert!(self.c_inst.id_is_valid(entity));

        self.c_modified
            .store(true, std::sync::atomic::Ordering::Release);
        let mut table_internal = self.c_table.t_internal.write().unwrap();
        table_internal.t_entity.set(entity.ecs_id, val);
    }

    /// Take a value out of the component table
    ///
    /// This is the opposite of `set`. It will unset the value of the component for this
    /// entity and will return the value that was stored there. The component entry will
    /// be undefined after this.
    pub fn take(&mut self, entity: &Entity) -> Option<T> {
        if !self.c_inst.id_is_valid(entity) {
            return None;
        }

        self.c_modified
            .store(true, std::sync::atomic::Ordering::Release);
        let mut table_internal = self.c_table.t_internal.write().unwrap();
        table_internal.t_entity.take(entity.ecs_id)
    }

    /// Drop all values in the table
    ///
    /// This will drop all values in this component table, and in the case of
    /// non-sparse allocations will replace it with the default value.
    pub fn clear(&mut self) {
        self.c_modified
            .store(true, std::sync::atomic::Ordering::Release);
        let mut table_internal = self.c_table.t_internal.write().unwrap();
        table_internal.t_entity.clear();
    }

    /// Create an iterator over all values in this component table
    ///
    /// This will return Option values for each entry in the internal
    /// component array. None values can be returned by the iterator,
    /// as it allows for you to use `.enumerate()` to mirror the
    /// component table into other resources.
    pub fn iter<'a>(&'a self) -> ComponentIterator<'a, T, C> {
        ComponentIterator {
            si_session: self,
            si_cur: -1,
            si_next: Some(0),
        }
    }
}

/// Helper struct for a slice
///
/// This is a rwlock guard for the sliced data
pub struct SliceRef<'a, T: 'static> {
    /// The lock guard returned from the table
    sr_guard: RwLockReadGuard<'a, TableInternal<T, SliceContainer<T>>>,
}

impl<'a, T: 'static> SliceRef<'a, T> {
    /// Get the backing slice where all data is stored
    ///
    /// This returns the raw data itself
    pub fn data(&'a self) -> &'a [T] {
        self.sr_guard.t_entity.as_slice()
    }
}

impl<T: 'static> RawComponent<T, SliceContainer<T>> {
    /// Get the backing slice where all data is stored
    ///
    /// This is useful if you want to pass the raw data array to
    /// another library, such as ECS objects being passed to Vulkan
    pub fn get_data_slice<'a>(&'a self) -> SliceRef<'a, T> {
        SliceRef {
            sr_guard: self.c_table.t_internal.read().unwrap(),
        }
    }
}

pub struct ComponentIterator<'a, T: 'static, C: Container<T> + 'static> {
    si_session: &'a RawComponent<T, C>,
    si_cur: isize,
    si_next: Option<usize>,
}

impl<'a, T: 'static, C: Container<T> + 'static> Iterator for ComponentIterator<'a, T, C> {
    type Item = Option<TableRef<'a, T, C>>;

    fn next(&mut self) -> Option<Self::Item> {
        let table_internal = self.si_session.c_table.t_internal.read().unwrap();
        // Now update our current to our next pointer. If it is None, then
        // we don't have any more valid indices
        if self.si_next.is_none() {
            return None;
        }
        self.si_cur = self.si_next.unwrap() as isize;
        self.si_next = table_internal.t_entity.get_next_id(self.si_next.unwrap());

        // Double check that this ref is good before returning it.
        //
        // Since we start at the zero offset, there's a chance it isn't
        // defined and we can't pass a TableRef that will panic back to
        // the caller. This is gross
        if self.si_cur >= 0 {
            match table_internal.t_entity.index(self.si_cur as usize).as_ref() {
                // Now we can create a ref to this id
                Some(_) => Some(Some(TableRef {
                    tr_guard: table_internal,
                    tr_entity: TableRefEntityType::Offset(self.si_cur as usize),
                })),
                None => Some(None),
            }
        } else {
            Some(None)
        }
    }
}
