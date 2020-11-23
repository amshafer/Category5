// Trait definitions for property types
//
// Austin Shafer - 2020

/// Represents what index an enum variant is
pub type PropertyId = usize;

/// A property represents an enumerator that is used to index
/// into a PropertyMap
///
/// Represents updating one property in the ECS
///
/// Our atmosphere is really just a lock-free Entity
/// component set. We need a way to snapshot the
/// changes accummulated in a hemisphere during a frame
/// so that we can replay them on the other hemisphere
/// to keep things consistent. This encapsulates uppdating
/// one property.
///
/// These will be collected in a hashmap for replay
///    map<(window id, property id), Patch>
///
/// PropertyMap needs to be able to find the offset where a
/// particular property is stored, and it needs to use this
/// trait to find the appropriate info.
pub trait Property {
    /// This gets an array offset for a enumerator value
    ///
    /// Conceptually this is the number of the variant in
    /// order (so the first declared variant returns 0 here)
    fn get_property_id(&self) -> PropertyId;

    /// WARNING:
    /// This NEEDS to be the number of variants in Property
    fn variant_len() -> u32;
}
