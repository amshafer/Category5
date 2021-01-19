// Austin Shafer - 2020

use super::property::{Property, PropertyId};

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
        let mut v = Vec::with_capacity(self.pm_variants as usize);
        v.resize(self.pm_variants as usize, None);
        self.pm_map[id as usize] = Some(v);
    }

    pub fn deactivate(&mut self, id: u32) {
        assert!(!self.pm_map[id as usize].is_none());
        self.pm_map[id as usize] = None;
    }

    fn ensure_active(&mut self, id: u32) {
        if id as usize >= self.pm_map.len() || self.pm_map[id as usize].is_none() {
            self.activate(id);
        }
    }

    /// Get the value of a property.
    /// `id` is the WindowId we want to get the property for
    /// `prop_id` is the unique value of the variant
    /// we want to retrieve.
    pub fn get(&self, id: u32, prop_id: PropertyId) -> Option<&T> {
        // make sure this id exists
        if id as usize >= self.pm_map.len() || self.pm_map[id as usize].is_none() {
            return None;
        }

        let win = self.pm_map[id as usize].as_ref().unwrap();
        return win[prop_id].as_ref();
    }

    /// Get the value of a property.
    /// `id` is the WindowId we want to get the property for
    /// `prop_id` is the unique value of the variant
    /// we want to set.
    /// T is the new data
    pub fn set(&mut self, id: u32, prop_id: PropertyId, value: &T) {
        // make sure this id exists
        self.ensure_active(id);
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

// The lifetimes here are kind of weird. We need to make a
// PropertymapIterator with the same lifetime as the PropertyMap
// since it will hold a reference to it. This means our IntoIterator
// declaration is going to happen on a reference not the owned value
impl<'a, T: Clone + Property> PropertyMap<T> {
    /// return an iterator of valid ids.
    ///
    /// This will be all ids that are have been `activate`d
    pub fn active_id_iter(&'a self) -> PropertyMapIterator<'a, T> {
        self.into_iter()
    }
}

// Iterator for valid ids in a property map
// Because we hold a reference to the PropertyMap, we do some
// ugly lifetimes to ensure it lives this long
pub struct PropertyMapIterator<'a, T: Clone + Property> {
    pmi_pm: &'a PropertyMap<T>,
    // the last index we returned
    pmi_index: usize,
    // the maximum we should walk before stopping
    pmi_max: usize,
}

// Non-consuming iterator over a Propertymap, this is why
// our trait is for a lifetime bound reference.
//
// See the YARIT iterator tutorials webpage for more
//
// This ties into active_id_iter, which is where we specify
// the lifetimes as we instantiate this.
impl<'a, T: Clone + Property> IntoIterator for &'a PropertyMap<T> {
    type Item = u32;
    type IntoIter = PropertyMapIterator<'a, T>;

    // note that into_iter() is consuming self
    fn into_iter(self) -> Self::IntoIter {
        PropertyMapIterator {
            pmi_pm: &self,
            pmi_index: 0,
            pmi_max: self.pm_map.len(),
        }
    }
}

// We will inevitably need to get a list of valid ids that are
// present in a map. This iterator does that. It effectively tells
// us what ids we can use for queries against this map
impl<'a, T: Clone + Property> Iterator for PropertyMapIterator<'a, T> {
    // Our item type is a WindowId
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        // iterate until we find a non-null id
        while self.pmi_index < self.pmi_max {
            if self.pmi_pm.pm_map[self.pmi_index].is_some() {
                self.pmi_index += 1;
                return Some(self.pmi_index as u32 - 1);
            } else {
                self.pmi_index += 1;
            }
        }
        None
    }
}
