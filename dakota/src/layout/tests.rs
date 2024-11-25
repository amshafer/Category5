/// Dakota Layout engine tests
use crate as dak;
use dak::{dom, DakotaId};

/// Common initialization
///
/// This sets up Dakota but also initializes the dom and window
/// structs.
fn setup_dakota() -> (
    dak::Dakota,
    dak::VirtualOutput,
    dak::Output,
    dak::Scene,
    DakotaId,
) {
    let mut dak = dak::Dakota::new().expect("Could not create Dakota");
    // Set up our output
    let mut virtual_output = dak
        .create_virtual_output()
        .expect("Failed to create Dakota Virtual Output Surface");
    let mut output = dak
        .create_output(&virtual_output)
        .expect("Failed to create Dakota Output");

    // Now set up our scene
    let mut scene = output
        .create_scene(&virtual_output)
        .expect("Could not create scene");

    let root = scene.create_element().unwrap();
    scene.set_dakota_dom(dom::DakotaDOM {
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
    });

    output.set_resolution(&mut scene, 640, 480).unwrap();
    virtual_output.set_size((640, 480));

    (dak, virtual_output, output, scene, root)
}

/// Test root node colored and inheriting the window size
#[test]
fn basic() {
    let (_, virtual_output, _, mut scene, root) = setup_dakota();

    // Color the entire scene gray
    let gray = scene.create_resource().unwrap();
    scene
        .resource_color()
        .set(&gray, dom::Color::new(0.5, 0.5, 0.5, 1.0));
    scene.resource().set(&root, gray);
    scene
        .recompile(&virtual_output)
        .expect("Refreshing Dakota Scene");

    let node = scene.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 0);
}

/// Test a child inheriting the root node size
#[test]
fn unsized_child() {
    let (_, virtual_output, _, mut scene, root) = setup_dakota();

    let child = scene.create_element().unwrap();
    scene.add_child_to_element(&root, child.clone());

    scene
        .recompile(&virtual_output)
        .expect("Refreshing Dakota Scene");

    let node = scene.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 1);

    let child_node = scene.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(640, 480));
    assert!(child_node.l_children.len() == 0);
}

/// Test a child with a static size
#[test]
fn sized_child() {
    let (_, virtual_output, _, mut scene, root) = setup_dakota();

    let child = scene.create_element().unwrap();
    scene.add_child_to_element(&root, child.clone());
    scene.width().set(&child, dom::Value::Constant(128));
    scene.height().set(&child, dom::Value::Constant(128));

    scene
        .recompile(&virtual_output)
        .expect("Refreshing Dakota Scene");

    let node = scene.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 1);

    let child_node = scene.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(128, 128));
    assert!(child_node.l_children.len() == 0);
}

/// Test tiling of two children:
///  * only height specified constant
///  * dynamic sized child
#[test]
fn relatively_sized_child() {
    let (_, virtual_output, _, mut scene, root) = setup_dakota();

    let child = scene.create_element().unwrap();
    scene.add_child_to_element(&root, child.clone());
    scene.height().set(&child, dom::Value::Constant(128));

    let child2 = scene.create_element().unwrap();
    scene.add_child_to_element(&root, child2.clone());
    scene.width().set(&child2, dom::Value::Relative(0.5));
    scene.height().set(&child2, dom::Value::Relative(0.5));

    scene
        .recompile(&virtual_output)
        .expect("Refreshing Dakota Scene");

    let node = scene.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 2);

    let child_node = scene.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(640, 128));
    assert!(child_node.l_children.len() == 0);

    let child2_node = scene.d_layout_nodes.get(&child2).unwrap();
    assert!(child2_node.l_offset == dom::Offset::new(0, 128));
    assert!(child2_node.l_size == dom::Size::new(320, 240));
    assert!(child2_node.l_children.len() == 0);
}

/// Test dynamically sized and centered child content
#[test]
fn centered_content() {
    let (_, virtual_output, _, mut scene, root) = setup_dakota();

    let child = scene.create_element().unwrap();
    scene.content().set(&root, dom::Content::new(child.clone()));
    scene.width().set(&child, dom::Value::Relative(0.5));
    scene.height().set(&child, dom::Value::Relative(0.5));

    scene
        .recompile(&virtual_output)
        .expect("Refreshing Dakota Scene");

    let node = scene.d_layout_nodes.get(&root).unwrap();
    assert!(node.l_offset == dom::Offset::new(0, 0));
    assert!(node.l_size == dom::Size::new(640, 480));
    assert!(node.l_children.len() == 1);

    let child_node = scene.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(160, 120));
    assert!(child_node.l_size == dom::Size::new(320, 240));
}

/// Test tiling of two children:
///  * only width specified inheriting hight from assigned image resource
///  * dynamic sized child, assigned a color resource
#[test]
fn resource_from_bits() {
    let (_, virtual_output, _, mut scene, root) = setup_dakota();

    let child = scene.create_element().unwrap();
    scene.add_child_to_element(&root, child.clone());
    scene.width().set(&child, dom::Value::Constant(128));

    let pixels: Vec<u8> = std::iter::repeat(128).take(4 * 64 * 64).collect();
    let img = scene.create_resource().unwrap();
    scene
        .define_resource_from_bits(
            &img,
            pixels.as_slice(),
            64, // width
            64, // height
            0,  // stride
            dom::Format::ARGB8888,
        )
        .unwrap();
    scene.resource().set(&child, img);

    let child2 = scene.create_element().unwrap();
    scene.add_child_to_element(&root, child2.clone());
    scene.width().set(&child2, dom::Value::Relative(0.5));
    scene.height().set(&child2, dom::Value::Relative(0.5));

    let gray = scene.create_resource().unwrap();
    scene
        .resource_color()
        .set(&gray, dom::Color::new(0.2, 0.2, 0.2, 1.0));
    scene.resource().set(&child2, gray);

    scene
        .recompile(&virtual_output)
        .expect("Refreshing Dakota Scene");

    let child_node = scene.d_layout_nodes.get(&child).unwrap();
    assert!(child_node.l_offset == dom::Offset::new(0, 0));
    assert!(child_node.l_size == dom::Size::new(128, 64));
    assert!(child_node.l_children.len() == 0);

    let child2_node = scene.d_layout_nodes.get(&child2).unwrap();
    assert!(child2_node.l_offset == dom::Offset::new(128, 0));
    assert!(child2_node.l_size == dom::Size::new(320, 240));
    assert!(child2_node.l_children.len() == 0);
}
