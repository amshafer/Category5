// Austin Shafer - 2020

use super::property::{PropertyId,Property};

/// This is a map that handles keeping track of offsets into an
/// array for a set of WindowIds
///
/// We are storing different properties in the same way an os
/// stores fds. Each id has a set of available properties which
/// can be get/set
///
/// This approach was chosen because of the relatively small number
/// of windows that will be active at any point in time. This prevents
/// generating large hashmaps to hold data, which could be expensive.
pub struct PropertyMap<T: Clone + Property> {
    // CONST number of variants in enumerator T
    pm_variants: u32,
    // 2D property system
    // The first vec represents a map of which window
    // ids are active in the map. It is indexed by WindowId
    //
    // The second vec has an entry for each variant in T. This
    // way we can save/restore a value for each property.
    pm_map: Vec<Option<Vec<Option<T>>>>,
}

impl<T: Clone + Property> PropertyMap<T> {
    /// Create a new map based on an enumerator T.
    pub fn new() -> Self {
        PropertyMap {
            pm_variants: T::variant_len(),
            pm_map: Vec::new(),
        }
    }

    /// Gets the next available id in this map,
    /// but does not reserve it.
    pub fn get_first_free_id(&self) -> u32 {
        for (i, t) in self.pm_map.iter().enumerate() {
            if !t.is_none() {
                return i as u32;
            }
        }
        return self.pm_map.len() as u32;
    }

    /// Mark an id as active and make a new
    /// entry for it in the map.
    pub fn activate(&mut self, id: u32) {
        // First handle any resizing that needs to occur
        if id as usize >= self.pm_map.len() {
            self.pm_map.resize(id as usize + 1, None);
        }

        assert!(self.pm_map[id as usize].is_none());
        let mut v = Vec::new();
        v.resize(self.pm_variants as usize, None);
        self.pm_map[id as usize] = Some(v);
    }

    pub fn deactivate(&mut self, id: u32) {
        self.pm_map[id as usize] = None;
    }

    /// Get the value of a property.
    /// `id` is the WindowId we want to get the property for
    /// `prop_id` is the unique value of the variant
    /// we want to retrieve.
    pub fn get(&self, id: u32, prop_id: PropertyId) -> Option<&T> {
        // make sure this id exists
        assert!(!self.pm_map[id as usize].is_none());
        let win = self.pm_map[id as usize].as_ref().unwrap();
        // make sure this property exists
        assert!(!win[prop_id].is_none());

        return win[prop_id].as_ref();
    }

    /// Get the value of a property.
    /// `id` is the WindowId we want to get the property for
    /// `prop_id` is the unique value of the variant
    /// we want to set.
    /// T is the new data
    pub fn set(&mut self, id: u32, prop_id: PropertyId, value: &T) {
        // make sure this id exists
        assert!(!self.pm_map[id as usize].is_none());
        let win = self.pm_map[id as usize].as_mut().unwrap();

        win[prop_id] = Some(value.clone());
    }

    /// Clears an entry in the mapping
    /// This will cause `get` to return None
    pub fn clear(&mut self, id: u32, prop_id: PropertyId) {
        let win = self.pm_map[id as usize].as_mut().unwrap();
        win[prop_id] = None;
    }
}
