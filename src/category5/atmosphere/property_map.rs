// Austin Shafer - 2020

use super::property::{Property, PropertyId};
use super::property_list::PropertyList;

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
    pm_map: PropertyList<Vec<Option<T>>>,
}

impl<T: Clone + Property> PropertyMap<T> {
    /// Create a new map based on an enumerator T.
    pub fn new() -> Self {
        PropertyMap {
            pm_variants: T::variant_len(),
            pm_map: PropertyList::new(),
        }
    }

    /// Get the value of a property.
    /// `id` is the WindowId we want to get the property for
    /// `prop_id` is the unique value of the variant
    /// we want to retrieve.
    pub fn get(&self, id: PropertyId, prop_id: PropertyId) -> Option<&T> {
        if self.pm_map.id_exists(id) {
            let win = self.pm_map[id].as_ref().unwrap();
            return win[prop_id].as_ref();
        } else {
            return None;
        }
    }

    pub fn deactivate(&mut self, id: PropertyId) {
        self.pm_map.deactivate(id);
    }

    /// Get the value of a property.
    /// `id` is the WindowId we want to get the property for
    /// `prop_id` is the unique value of the variant
    /// we want to set.
    /// T is the new data
    pub fn set(&mut self, id: PropertyId, prop_id: PropertyId, value: &T) {
        if !self.pm_map.id_exists(id) {
            // Create the internal vec as all None
            let v = std::iter::repeat(None)
                .take(self.pm_variants as usize)
                .collect();
            self.pm_map.activate(id, v);
        }
        let win = self.pm_map[id].as_mut().unwrap();
        win[prop_id] = Some(value.clone());
    }

    /// Clears an entry in the mapping
    /// This will cause `get` to return None
    pub fn clear(&mut self, id: PropertyId, prop_id: PropertyId) {
        if self.pm_map.id_exists(id) {
            let win = self.pm_map[id].as_mut().unwrap();
            win[prop_id] = None;
        }
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
