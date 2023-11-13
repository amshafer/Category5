use crate as ll;
use std::sync::{Arc, Mutex};

#[test]
fn basic_test() {
    // Create the ECS holder
    let mut inst = ll::Instance::new();
    // Make a new entity
    let entity = inst.add_entity();

    // Now add our component. This will be a string, but
    // we don't have to specify that for now
    let mut c = inst.add_component();

    // Before querying the value, we first need to set a valid value
    // for this component. Afterwards, we can get it and check that
    // it is unchanged.
    c.set(&entity, "Hola Lluvia");
    let data_ref = c.get(&entity).unwrap();
    assert_eq!(*data_ref, "Hola Lluvia");
}

#[test]
fn basic_non_sparse_test() {
    // Create the ECS holder
    let mut inst = ll::Instance::new();
    // Make a new entity
    let entity = inst.add_entity();

    // Now add our component. This will be a string, but
    // we don't have to specify that for now
    let mut c = inst.add_non_sparse_component(|| "");

    // Before querying the value, we first need to set a valid value
    // for this component. Afterwards, we can get it and check that
    // it is unchanged.
    c.set(&entity, "Hola Lluvia");
    let data_ref = c.get(&entity).unwrap();
    assert_eq!(*data_ref, "Hola Lluvia");
}

struct TestData {
    e: bool,
    e1: bool,
}
struct Empty(&'static str, Arc<Mutex<TestData>>);

impl Drop for Empty {
    fn drop(&mut self) {
        println!("Dropping {}", self.0);
        match self.0 {
            "e" => self.1.lock().unwrap().e = false,
            "e1" => self.1.lock().unwrap().e1 = false,
            _ => panic!("Unrecognized string"),
        }
    }
}

// Test that we can add an Entity into a component table as data
//
// This is done by adding e in e1's data. We then add a custom struct
// which will record if that element has been dropped yet in TestData
// and test the values afterwards
#[test]
fn entity_in_component_data() {
    let mut inst = ll::Instance::new();
    let mut c = inst.add_component();
    let mut c1 = inst.add_component();

    let container = Arc::new(Mutex::new(TestData { e: true, e1: true }));
    {
        let e1 = inst.add_entity();
        c1.set(&e1, Empty("e1", container.clone()));

        {
            let e = inst.add_entity();
            c1.set(&e, Empty("e", container.clone()));
            let e_id = e.get_raw_id();

            c.set(&e1, e);

            let data_ref = c.get(&e1).unwrap();
            assert_eq!(data_ref.ecs_id, e_id);
        }

        // Assert the data is still valid
        let data = container.lock().unwrap();
        assert!(data.e && data.e1);
    }
    // Assert the data is not valid since we dropped e1
    let data = container.lock().unwrap();
    assert!(!data.e && !data.e1);
}

#[test]
fn snapshot_test() {
    let mut inst = ll::Instance::new();
    let mut c = inst.add_component();
    let e1 = inst.add_entity();
    let e2 = inst.add_entity();
    let e3 = inst.add_entity();

    c.set(&e1, "e1");
    c.set(&e2, "e2");
    c.set(&e3, "e3");

    let mut snap = c.snapshot();

    snap.set(&e1, "e4");
    snap.take(&e2);
    snap.set(&e3, "e5");

    assert_eq!(*c.get(&e1).unwrap(), "e1");
    assert_eq!(*c.get(&e2).unwrap(), "e2");
    assert_eq!(*c.get(&e3).unwrap(), "e3");

    assert_eq!(*snap.get(&e1).unwrap(), "e4");
    assert!(snap.get(&e2).is_none());
    assert_eq!(*snap.get(&e3).unwrap(), "e5");

    snap.commit();

    assert_eq!(*c.get(&e1).unwrap(), "e4");
    assert!(c.get(&e2).is_none());
    assert_eq!(*c.get(&e3).unwrap(), "e5");

    // test resetting a snapshot
    snap.reset();
    snap.set(&e1, "e6");
    snap.set(&e2, "e7");
    snap.set(&e3, "e8");
    snap.commit();

    assert_eq!(*c.get(&e1).unwrap(), "e6");
    assert_eq!(*c.get(&e2).unwrap(), "e7");
    assert_eq!(*c.get(&e3).unwrap(), "e8");

    // test layered snapshots
    snap.reset();
    snap.set(&e1, "e9");
    snap.set(&e2, "e10");
    snap.set(&e3, "e11");

    let mut child_snap = snap.snapshot();
    child_snap.set(&e1, "e12");
    assert_eq!(*snap.get(&e1).unwrap(), "e9");
    assert_eq!(*child_snap.get(&e1).unwrap(), "e12");
    assert_eq!(*child_snap.get(&e2).unwrap(), "e10");
    assert_eq!(*child_snap.get(&e3).unwrap(), "e11");

    snap.commit();
    assert_eq!(*c.get(&e1).unwrap(), "e9");
    assert_eq!(*c.get(&e2).unwrap(), "e10");
    assert_eq!(*c.get(&e3).unwrap(), "e11");

    child_snap.commit();
    assert_eq!(*c.get(&e1).unwrap(), "e12");
}

#[test]
#[should_panic]
fn snapshot_child_fail_test() {
    let mut inst = ll::Instance::new();
    let c = inst.add_component();
    let snap: ll::Snapshot<usize> = c.snapshot();
    let mut child_snap = snap.snapshot();

    child_snap.commit();
}

#[test]
#[should_panic]
fn snapshot_reset_fail_test() {
    let mut inst = ll::Instance::new();
    let c = inst.add_component();
    let e1 = inst.add_entity();
    let mut snap: ll::Snapshot<usize> = c.snapshot();

    snap.commit();
    snap.set(&e1, 0);
}
