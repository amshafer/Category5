//! Super simple ECS infrastructure for basic usages.
//!
//! This frameworks is designed for problems that are well
//! suited for large collections of items with wild lifetimes,
//! that have to be handed to multiple components of a program.
//!
//! There are three parts:
//! * The `ECSInstance` - This is the instance of this collection
//! of tagged data. This will survive for as long as the last in-use
//! struct lives. This tracks which Ids are valid
//! * The entity id, `ECSId` - A small struct that tracks the lifetime
//! of an entity in the system. It holds a RefCell to the ECSInstance,
//! and will mark itself as freed when it goes out of use.
//! * The `ECSTable` - This is a dictionary of data that is indexed
//! by ECSIds. You index it like an array, and it gives you a reference
//! to whatever you put in it. Implements Index and IndexMut, and
//! is fully generic, with the only trait restriction being that
//! the type it holds implements Default.
//!
//! ECSIds are really just Rcs that track the lifetime of the entity,
//! so they do not implement Copy, but instead use Clone.
//!
//! Basic usage looks like this:
//! ```
//! // Create a new instance of a entity component system
//! let inst = ECSInstance::new();
//! // Create a table full of strings
//! let table: ECSTable<String> = ECSTable::new(inst.clone());
//!
//! // Now we can create a new entity
//! let first_id = inst.mint_new_id();
//! // use this id to access its data in one of our collections
//! table[&first_id] = String::from("Hello ECS!");
//!
//! assert!(table[&first_id] == "Hello ECS!".to_owned());
//! ```
// Austin Shafer - 2022
use std::cell::RefCell;
use std::ops::{Drop, Index, IndexMut};
use std::rc::Rc;

#[derive(Debug)]
struct ECSInstanceInternal {
    /// The number of ids we have allocated out of the list above. This
    /// allows us to optimize the case of a full id list: We don't have to
    /// scan the entire list if it's full
    eci_total_num_ids: usize,
    /// This is a list of active ids in the system.
    eci_valid_ids: Vec<bool>,
}

/// An Entity component system.
///
/// This tracks the validity of various ids. It does not hold any data itself.
#[derive(Debug)]
pub struct ECSInstance {
    ecs_internal: Rc<RefCell<ECSInstanceInternal>>,
}

impl Clone for ECSInstance {
    fn clone(&self) -> Self {
        Self {
            ecs_internal: self.ecs_internal.clone(),
        }
    }
}

impl ECSInstance {
    pub fn new() -> Self {
        Self {
            ecs_internal: Rc::new(RefCell::new(ECSInstanceInternal {
                eci_valid_ids: Vec::new(),
                eci_total_num_ids: 0,
            })),
        }
    }

    /// Get the total number of entities that have been allocated.
    ///
    /// This returns the number of "live" ids
    pub fn num_entities(&self) -> usize {
        self.ecs_internal.borrow().eci_total_num_ids
    }

    pub fn mint_new_id(&mut self) -> ECSId {
        let new_self = Self {
            ecs_internal: self.ecs_internal.clone(),
        };
        let mut internal = self.ecs_internal.borrow_mut();

        // Find the first free id that is not in use or make one
        let first_valid_id = {
            let mut index = None;

            // first check the array from back to front
            // Don't do this if the array is full, just skip to extending the
            // array if that is the case
            if internal.eci_total_num_ids != internal.eci_valid_ids.len() {
                for i in internal.eci_valid_ids.len()..0 {
                    if !internal.eci_valid_ids[i] {
                        index = Some(i);
                    }
                }
            }

            // if that didn't work then add one to the back
            if index.is_none() {
                internal.eci_valid_ids.push(true);
                index = Some(internal.eci_valid_ids.len() - 1);
            }

            index.unwrap()
        };
        // Mark this new id as active
        internal.eci_valid_ids[first_valid_id] = true;
        internal.eci_total_num_ids += 1;

        return Rc::new(ECSIdInternal {
            ecs_id: first_valid_id,
            ecs_inst: new_self,
        });
    }

    fn invalidate_id(&mut self, id: usize) {
        let mut internal = self.ecs_internal.borrow_mut();
        assert!(internal.eci_valid_ids[id]);
        internal.eci_valid_ids[id] = false;
        internal.eci_total_num_ids -= 1;
    }

    fn assert_id_is_valid(&self, id: &ECSId) {
        assert!(self.ecs_internal.borrow().eci_valid_ids[id.ecs_id]);
    }
}

#[derive(Debug)]
pub struct ECSIdInternal {
    ecs_id: usize,
    ecs_inst: ECSInstance,
}

impl Drop for ECSIdInternal {
    fn drop(&mut self) {
        self.ecs_inst.invalidate_id(self.ecs_id);
    }
}

/// An Entity name
///
/// This gives an entity an identity, it holds a reference
/// to the instance it was allocated from, and is used to
/// index into a table to get data belonging to this
/// entity. An Id needs to be requested from the instance.
///
/// ```
/// let a = inst.mint_new_id();
/// // Use clone to duplicate the id to be used
/// // from multiple lifetimes.
/// let a_dup = a.clone();
/// ```
pub type ECSId = Rc<ECSIdInternal>;

/// A collection of data associated with entities
///
/// This is essentially a big vector that is indexed by ECSId.
/// This means a lookup time of O(1), but is potentially memory
/// hungry. If you create two ids, the table will have to fill
/// in the missing ids with something (It will use T::default()).
///
/// Multiple tables can be created inside of one instance, you
/// should create a new table for each type of data you would
/// like to track for an entity.
#[derive(Debug)]
pub struct ECSTable<T: Default> {
    pub ect_inst: ECSInstance,
    /// This is a component set, it will be indexed by ECSId
    pub ect_data: Vec<T>,
}

impl<T: Default> ECSTable<T> {
    pub fn new(inst: ECSInstance) -> Self {
        Self {
            ect_inst: inst,
            ect_data: Vec::new(),
        }
    }

    fn ensure_space_for_id(&mut self, ecs_id: &ECSId) {
        let id = ecs_id.ecs_id;
        self.ect_inst.assert_id_is_valid(ecs_id);

        // First handle any resizing that needs to occur
        if id >= self.ect_data.len() {
            self.ect_data.resize_with(id + 1, || T::default());
        }
    }

    pub fn iter<'a>(&'a self) -> ECSTableIterator<'a, T> {
        ECSTableIterator {
            eti_table: self,
            eti_cur: 0,
        }
    }
}

impl<T: Default> Index<&ECSId> for ECSTable<T> {
    type Output = T;

    #[inline]
    fn index(&self, id: &ECSId) -> &T {
        assert!(id.ecs_id < self.ect_data.len());
        &self.ect_data[id.ecs_id]
    }
}

impl<T: Default> IndexMut<&ECSId> for ECSTable<T> {
    #[inline]
    fn index_mut(&mut self, id: &ECSId) -> &mut T {
        self.ensure_space_for_id(id);
        &mut self.ect_data[id.ecs_id]
    }
}

pub struct ECSTableIterator<'a, T: Default> {
    eti_table: &'a ECSTable<T>,
    eti_cur: usize,
}

impl<'a, T: Default> Iterator for ECSTableIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.eti_cur >= self.eti_table.ect_data.len() {
            return None;
        }
        let ret = &self.eti_table.ect_data[self.eti_cur];
        self.eti_cur += 1;

        return Some(ret);
    }
}
