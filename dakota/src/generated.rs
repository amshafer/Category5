/// Generated getters/setters for properties
///
/// Austin Shafer - 2022
extern crate paste;
use paste::paste;
extern crate lluvia as ll;

use crate::{dom, Dakota, DakotaId, DakotaObjectType};

// ------------------------------------------------
// Now implement some getters/setters
// ------------------------------------------------

macro_rules! define_element_property {
    // of the form: define_element_property(name, session_name, type)
    //
    // Where:
    //   name - the name of the element property to be used by consumers
    //   session_name - the internal dakota session to use
    //   type - the data's return type
    ($name:ident, $sesh:ident, $val:ty) => {
        paste! {
            impl<'a> Dakota<'a> {
                // Use the paste crate to append get_ to the front of our name
                pub fn [<get_ $name>](&self, el: &DakotaId) -> Option<ll::TableRef<$val>> {
                    self.[<d_ $sesh>].get(el)
                }
                pub fn [<get_mut $name>](&mut self, el: &DakotaId) -> Option<ll::TableRefMut<$val>> {
                    self.[<d_ $sesh>].get_mut(el)
                }
                pub fn [<set_ $name>](&mut self, el: &DakotaId, data: $val) {
                    self.[<d_ $sesh>].set(el, data)
                }
            }
        }
    };
}

// Define a rule for each entry in Dakota

define_element_property!(object_type, node_types, DakotaObjectType);
define_element_property!(resource_definition, resource_definitions, dom::Resource);
define_element_property!(resource, resources, DakotaId);
define_element_property!(offset, offsets, dom::RelativeOffset);
define_element_property!(size, sizes, dom::RelativeSize);
define_element_property!(text, texts, dom::Text);
define_element_property!(content, contents, dom::Content);
define_element_property!(bounds, bounds, dom::Edges);
define_element_property!(children, children, Vec<DakotaId>);
define_element_property!(dakota_dom, dom, dom::DakotaDOM);
