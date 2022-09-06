## Lluvia - A stripped down Entity Component System that allows for no-nonsense data storage in finite time.

This library lets you quickly throw together large collections of objects
with varying lifetimes into one ECS. You specify the `Components`, create
any number of reference counted `Entity` objects, and when an `Entity` goes
out of scope its data will be automatically dropped as well. You can even
store `Entity` objects as components of other entities, and everything will
get dropped at once when the root `Entity` goes out of scope.

What sets this ECS apart is that it is very fast, very small in
scope, and has a very small footprint. The implementation is ~500 lines,
it has zero dependencies, and almost all operations run in O(1) time.
There is no archetyping, there is no rayon integration, there is no
advanced iterator pattern, and there is no multi-threaded access. Emphasis
is placed on minimizing complexity and avoiding scanning or re-organizing
data, as Lluvia was designed to be the data engine for low-latency graphics
programs.

Lluvia begins with creating an `Instance` object. This will track the
validity of `Entity` objects in the system, and will hold references
to data tables used for storage.

The `Instance` can then be used to add `Component` tables, and access
them using a `Session` object. The `Session` allows for getting and
setting components for each `Entity`.

Basic usage looks like:
```
use lluvia as ll;
// Create the ECS holder
let mut inst = ll::Instance::new();
// Make a new entity
let entity = inst.add_entity();

// Now add our component. This will be a string, but
// we don't have to specify that for now
let c = inst.add_component();

// Get a session to access data for component c. This
// allows access to the per-entity data for this component and
// lets us perform queries.
let mut sesh = inst.open_session(c).unwrap();

// Before querying the value, we first need to set a valid value
// for this component. Afterwards, we can get it and check that
// it is unchanged.
sesh.set(&entity, "Hola Lluvia");
let data_ref = sesh.get(&entity).unwrap();
assert_eq!(*data_ref, "Hola Lluvia");
```
