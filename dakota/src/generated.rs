/// Generated getters/setters for properties
///
/// Austin Shafer - 2022
extern crate paste;
use paste::paste;
extern crate lluvia as ll;

use crate::utils::{anyhow, Result};
use crate::{Dakota, DakotaId, DakotaObjectType};

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
                pub fn [<get_ $name>](&self, el: &DakotaId) -> Result<ll::TableRef<$val>> {
                    self.[<d_ $sesh>].get(el).ok_or(anyhow!("Element did not have "))
                }
                pub fn [<get_mut $name>](&self, el: &DakotaId) -> Result<ll::TableRefMut<$val>> {
                    self.[<d_ $sesh>].get_mut(el).ok_or(anyhow!("Element did not have "))
                }
                pub fn [<set_ $name>](&self, el: &DakotaId, data: $val) {
                    self.[<d_ $sesh>].set(el, data)
                }
            }
        }
    };
}

define_element_property!(object_type, node_types, DakotaObjectType);
