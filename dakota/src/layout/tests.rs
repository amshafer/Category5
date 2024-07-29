/// Dakota Layout engine tests
use crate as dak;
use dak::{dom, DakotaId};

/// Common initialization
///
/// This sets up Dakota but also initializes the dom and window
/// structs.
fn setup_dakota() -> (dak::Dakota, DakotaId, DakotaId) {
    let mut dak = dak::Dakota::new().expect("Could not create Dakota");

    let root = dak.create_element().unwrap();
    let dom = dak.create_dakota_dom().unwrap();
    dak.dakota_dom().set(
        &dom,
        dom::DakotaDOM {
            version: "0.0.1".to_string(),
            window: dom::Window {
                title: "Dakota unit test".to_string(),
                size: Some((640, 480)),
                events: dom::WindowEvents {
                    resize: None,
                    redraw_complete: None,
                    closed: None,
                },
            },
            root_element: root.clone(),
        },
    );

    (dak, dom, root)
}

/// Test root node colored and inheriting the window size
#[test]
fn basic() {
    let (mut dak, dom, root) = setup_dakota();

    // Color the entire scene gray
    let gray = dak.create_resource().unwrap();
    dak.resource_color()
        .set(&gray, dom::Color::new(0.5, 0.5, 0.5, 1.0));
    dak.resource().set(&root, gray);

    dak.refresh_full(&dom).unwrap();

    let node = dak.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 0);
}

/// Test a child inheriting the root node size
#[test]
fn unsized_child() {
    let (mut dak, dom, root) = setup_dakota();

    let child = dak.create_element().unwrap();
    dak.add_child_to_element(&root, child.clone());

    dak.refresh_full(&dom).unwrap();

    let node = dak.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 1);

    let child_node = dak.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(640, 480));
    assert!(child_node.l_children.len() == 0);
}

/// Test a child with a static size
#[test]
fn sized_child() {
    let (mut dak, dom, root) = setup_dakota();

    let child = dak.create_element().unwrap();
    dak.add_child_to_element(&root, child.clone());
    dak.width().set(&child, dom::Value::Constant(128));
    dak.height().set(&child, dom::Value::Constant(128));

    dak.refresh_full(&dom).unwrap();

    let node = dak.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 1);

    let child_node = dak.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(128, 128));
    assert!(child_node.l_children.len() == 0);
}

/// Test tiling of two children:
///  * only height specified constant
///  * dynamic sized child
#[test]
fn relatively_sized_child() {
    let (mut dak, dom, root) = setup_dakota();

    let child = dak.create_element().unwrap();
    dak.add_child_to_element(&root, child.clone());
    dak.height().set(&child, dom::Value::Constant(128));

    let child2 = dak.create_element().unwrap();
    dak.add_child_to_element(&root, child2.clone());
    dak.width().set(&child2, dom::Value::Relative(0.5));
    dak.height().set(&child2, dom::Value::Relative(0.5));

    dak.refresh_full(&dom).unwrap();

    let node = dak.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 2);

    let child_node = dak.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(640, 128));
    assert!(child_node.l_children.len() == 0);

    let child2_node = dak.d_layout_nodes.get(&child2).unwrap();
    assert!(child2_node.l_offset == dom::Offset::new(0, 128));
    assert!(child2_node.l_size == dom::Size::new(320, 240));
    assert!(child2_node.l_children.len() == 0);
}

/// Test dynamically sized and centered child content
#[test]
fn centered_content() {
    let (mut dak, dom, root) = setup_dakota();

    let child = dak.create_element().unwrap();
    dak.content().set(&root, dom::Content::new(child.clone()));
    dak.width().set(&child, dom::Value::Relative(0.5));
    dak.height().set(&child, dom::Value::Relative(0.5));

    dak.refresh_full(&dom).unwrap();

    let node = dak.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 1);

    let child_node = dak.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(160, 120));
    assert!(child_node.l_size == dom::Size::new(320, 240));
}

/// Test tiling of two children:
///  * only width specified inheriting hight from assigned image resource
///  * dynamic sized child, assigned a color resource
#[test]
fn resource_from_bits() {
    let (mut dak, dom, root) = setup_dakota();

    let child = dak.create_element().unwrap();
    dak.add_child_to_element(&root, child.clone());
    dak.width().set(&child, dom::Value::Constant(128));

    let pixels: Vec<u8> = std::iter::repeat(128).take(4 * 64 * 64).collect();
    let img = dak.create_resource().unwrap();
    dak.define_resource_from_bits(
        &img,
        pixels.as_slice(),
        64, // width
        64, // height
        0,  // stride
        dom::Format::ARGB8888,
    )
    .unwrap();
    dak.resource().set(&child, img);

    let child2 = dak.create_element().unwrap();
    dak.add_child_to_element(&root, child2.clone());
    dak.width().set(&child2, dom::Value::Relative(0.5));
    dak.height().set(&child2, dom::Value::Relative(0.5));

    let gray = dak.create_resource().unwrap();
    dak.resource_color()
        .set(&gray, dom::Color::new(0.2, 0.2, 0.2, 1.0));
    dak.resource().set(&child2, gray);

    dak.refresh_full(&dom).unwrap();

    let child_node = dak.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(128, 64));
    assert!(child_node.l_children.len() == 0);

    let child2_node = dak.d_layout_nodes.get(&child2).unwrap();
    assert!(child2_node.l_offset == dom::Offset::new(128, 0));
    assert!(child2_node.l_size == dom::Size::new(320, 240));
    assert!(child2_node.l_children.len() == 0);
}
