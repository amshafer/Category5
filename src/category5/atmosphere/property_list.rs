/// A property list tracks one datatype for each id in an ECS
/// It is meant to be combined into PropertyMap
///
/// Austin Shafer - 2021
use super::property::PropertyId;
use std::iter::Iterator;
use std::ops::{Index, IndexMut};

pub struct PropertyList<T> {
    /// This is indexed by a property id such as WindowId/ClientId
    /// It holds None if that property isn't active, and Some if
    /// the id is active and a value has been added.
    pl_list: Vec<Option<T>>,
}

impl<T> PropertyList<T> {
    pub fn new() -> Self {
        Self {
            pl_list: Vec::new(),
        }
    }

    pub fn activate(&mut self, id: PropertyId, data: T) {
        // First handle any resizing that needs to occur
        if id >= self.pl_list.len() {
            self.pl_list.resize_with(id + 1, || None);
        }

        assert!(self.pl_list[id].is_none());
        self.pl_list[id] = Some(data);
    }

    pub fn deactivate(&mut self, id: PropertyId) {
        assert!(!self.pl_list[id].is_none());
        self.pl_list[id] = None;
    }

    pub fn id_exists(&self, id: PropertyId) -> bool {
        id < self.pl_list.len() && self.pl_list[id].is_some()
    }

    /// Gets the next available id in this map,
    /// but does not reserve it.
    pub fn get_first_free_id(&self) -> PropertyId {
        for (i, t) in self.pl_list.iter().enumerate() {
            if !t.is_none() {
                return i;
            }
        }

        return self.pl_list.len();
    }

    pub fn get_mut<'a>(&mut self, id: PropertyId) -> Option<&mut T> {
        if self.id_exists(id) {
            return self[id].as_mut();
        }
        return None;
    }

    pub fn update_or_create(&mut self, id: PropertyId, data: T) {
        if !self.id_exists(id) {
            self.activate(id, data);
        } else {
            self[id] = Some(data);
        }
    }

    pub fn delete(&mut self, id: PropertyId) {
        if self.id_exists(id) {
            self[id] = None;
        }
    }

    pub fn len(&self) -> usize {
        self.pl_list.len()
    }
}

impl<T> Index<usize> for PropertyList<T> {
    type Output = Option<T>;

    fn index(&self, i: usize) -> &Self::Output {
        &self.pl_list[i]
    }
}

impl<T> IndexMut<usize> for PropertyList<T> {
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        &mut self.pl_list[i]
    }
}
