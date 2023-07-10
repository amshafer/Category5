use crate as ll;
use std::cell::RefCell;
use std::rc::Rc;

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
struct Empty(&'static str, Rc<RefCell<TestData>>);

impl Drop for Empty {
    fn drop(&mut self) {
        println!("Dropping {}", self.0);
        match self.0 {
            "e" => self.1.borrow_mut().e = false,
            "e1" => self.1.borrow_mut().e1 = false,
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

    let container = Rc::new(RefCell::new(TestData { e: true, e1: true }));
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
        let data = container.borrow();
        assert!(data.e && data.e1);
    }
    // Assert the data is not valid since we dropped e1
    let data = container.borrow();
    assert!(!data.e && !data.e1);
}
