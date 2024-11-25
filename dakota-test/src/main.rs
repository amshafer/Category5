extern crate dakota;
use dakota::{Dakota, GlobalEvent, OutputEvent, PlatformEvent};

extern crate utils;
use std::env;
use std::fs::File;
use std::io::BufReader;

// This is a simple Dakota program which accepts a command line argument of an
// XML file which will be loaded and presented.
fn main() {
    println!("Starting dakota");
    let args: Vec<String> = env::args().collect();
    assert!(args.len() > 1);
    println!("Loading scene {}", args[1]);

    // Create our Dakota instance
    let mut dakota = Dakota::new().expect("Could not create dakota instance");

    // Now create an output to display things on
    // The virtual output is a virtual region in which a scene can be
    // positioned and displayed. One or more Outputs will be created
    // to present a region of this virtual space.
    let mut virtual_output = dakota
        .create_virtual_output()
        .expect("Failed to create Dakota Virtual Output Surface");
    // The primary output is the default surface which Dakota will
    // present the application content on. This is normally a toplevel
    // desktop window, but could be any presentation surface.
    let mut output = dakota
        .create_output(&virtual_output)
        .expect("Failed to create Dakota Output");
    // For this example we default to the VirtualOutput and Output
    // being the same size.
    let resolution = output.get_resolution();
    virtual_output.set_size(resolution);

    // Now we can create our "scene", which will describe the layout
    // of our application content
    let mut scene = output
        .create_scene(&virtual_output)
        .expect("Could not create scene");
    // For convenience we load our scene contents from an XML file
    scene
        .load_xml_reader(BufReader::new(
            File::open(&args[1]).expect("could not open file"),
        ))
        .expect("Could not parse XML dakota file");
    // Now refresh our scene to recalculate the layout of the contents
    // that we just loaded in
    scene
        .recompile(&virtual_output)
        .expect("Refreshing Dakota Scene");

    loop {
        // Dispatch Dakota's main event loop. Here we will block waiting
        // for events and allow the
        dakota.dispatch(None).unwrap();

        // Process any global events first. These events show global
        // changes in state or give updates from Dakota's main polling
        // loop.
        for event in dakota.drain_events() {
            println!("Dakota got event: {:?}", event);
            match event {
                GlobalEvent::Quit => return,
                _ => {}
            }
        }

        // Next process any events on our virtual output. This primarily includes
        // input events which get delivered to our virtual space
        while let Some(event) = virtual_output.pop_event() {
            println!("Dakota got event: {:?}", event);

            match event {
                PlatformEvent::InputScroll {
                    position,
                    xrel,
                    yrel,
                    ..
                } => {
                    // Use the default input scrolling handler which will scroll
                    // any available regions
                    virtual_output
                        .handle_scrolling(
                            &mut scene,
                            position,
                            (xrel.unwrap_or(0).into(), yrel.unwrap_or(0).into()),
                        )
                        .expect("Error while handling scrolling");
                    // Tell our Output to present the new contents
                    output.request_redraw();
                }
                _ => {}
            }
        }

        // Process any events which dakota encountered on this output
        // while dispatching. These events are specific to the toplevel
        // desktop window being driven by Dakota, such as resizing or
        // closing.
        while let Some(event) = output.pop_event() {
            println!("Dakota got event: {:?}", event);

            match event {
                // This event lets us know that we should redraw the scene. This
                // event fires depending on window system hints or if a resize
                // has just taken place.
                OutputEvent::Redraw => output
                    .redraw(&virtual_output, &mut scene)
                    .expect("Failed to redraw output"),
                // If the window has been resized then we need to recompile our
                // scene using the new output parameters. A redraw event will be
                // signaled after this so we only need to recompute for now.
                OutputEvent::Resized => {
                    // First handle the resize on this output
                    output.handle_resize().expect("Failed to resize output");

                    // Update our VirtualOutput with the newly resized dimensions
                    let resolution = output.get_resolution();
                    virtual_output.set_size(resolution);

                    // Because our virtual surface this scene is being applied on
                    // was changed we need to recompile it. This triggers layout
                    // of the Scene's elements.
                    scene
                        .recompile(&virtual_output)
                        .expect("Failed to handle resize of scene")
                }
                // Exit gracefully if this output has terminated
                OutputEvent::Destroyed => return,
            }
        }
    }
}
