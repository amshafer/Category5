/// Generated getters/setters for properties
///
/// Austin Shafer - 2022
extern crate paste;
use paste::paste;
extern crate lluvia as ll;

use crate::{dom, DakotaId, DakotaObjectType, Scene};

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
            // Use the paste crate to append get_ to the front of our name
            pub fn $name(&self) -> ll::Component<$val> {
                self.[<d_ $sesh>].clone()
            }
        }
    };
}

// Define a rule for each entry in Dakota

impl Scene {
    // Provide hints for this resource.
    //
    // Allows for specifying things like will the resource be updated
    // at any point in the future.
    define_element_property!(resource_hints, resource_hints, dom::Hints);
    // Resource Color
    //
    // If set the elements assigned this resource will be filled with the
    // color specified in this component.
    define_element_property!(resource_color, resource_color, dom::Color);
    // Resource assigned to an Element
    // If the DakotaId is of type Element, then we can assign another
    // DakotaId that represents a Resource which will define what Dakota
    // should draw inside of the Element.
    define_element_property!(resource, resources, DakotaId);
    // Get the Dakota object type for this DakotaId
    //
    // This sets the "role" of the object.
    define_element_property!(object_type, node_types, DakotaObjectType);
    // Element Offset
    //
    // This specifies the Offset of an Element relative to its parent.
    //
    // Specifying an offset is always honored, and Dakota will not
    // move this element to any other location. No auto layout will take
    // place.
    //
    // If no offset is specified but a size is present then Dakota will
    // "tile" this element inside of the parent. Children in the parent
    // will be tiled from left to right, top to bottom.
    define_element_property!(offset, offsets, dom::RelativeOffset);
    // Element Size
    //
    // Specifies the Size of an Element. This will always be honored.
    //
    // If no size is specified but a sized resource (such as an image) is
    // assigned then the Resources size will be used.
    //
    // This Element will be grown/shrunk to the size of its children if:
    // - no size was set by the user
    // - no image resource is assigned
    // - element does not have any positioned content
    //
    // If none of the above apply, the Element defaults to the size of its
    // parent.
    //
    // If the dimensions of an Element exceed that of the parent then the
    // parent will have scrolling activated, but the child Element will be
    // clipped to the parent's dimensions during drawing.
    define_element_property!(width, widths, dom::Value);
    define_element_property!(height, heights, dom::Value);
    // Default Text block
    //
    // This is the default text drawing element. The text provided will be
    // laid out and drawn on top of this Element.
    define_element_property!(text, texts, dom::Text);
    // Text font assigned to any child Text
    //
    // Blanket specifier of the font to use for any text assigned. This
    // Font must be defined.
    define_element_property!(text_font, text_font, DakotaId);
    // Aligned Content
    //
    // This allows a child to have a specified alignment during layout. One
    // common usage is to center content inside of another element.
    define_element_property!(content, contents, dom::Content);
    // Bounding edges on which scrolling is allowed.
    //
    // This defaults to allowing only vertical scrolling.
    define_element_property!(bounds, bounds, dom::Edges);
    // Child Elements
    //
    // Child Elements may be assigned, and will be contained within the parents
    // dimensions.
    define_element_property!(children, children, Vec<DakotaId>);
    // Mark this subsurface as unbounded
    //
    // This excepts it from being clipped inside of the parent during drawing.
    define_element_property!(unbounded_subsurface, unbounded_subsurf, bool);
}
