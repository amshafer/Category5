use crate as ll;

#[test]
fn basic_test() {
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
}

// Test that we can add an Entity into a component table as data
#[test]
fn entity_in_component_data() {
    let mut inst = ll::Instance::new();
    let c = inst.add_component();
    let mut sesh = inst.open_session(c).unwrap();

    {
        let e = inst.add_entity();
        let e_id = e.get_raw_id();
        let e1 = inst.add_entity();
        sesh.set(&e1, e);
        let data_ref = sesh.get(&e1).unwrap();
        assert_eq!(data_ref.ecs_id, e_id);
    }
}
