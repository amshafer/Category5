extern crate dakota;
use dakota::{Dakota, GlobalEvent, OutputEvent, PlatformEvent};

extern crate utils;
use std::env;
use std::fs::File;
use std::io::BufReader;

fn add_output(
    dakota: &mut Dakota,
    virtual_outputs: &mut Vec<dakota::VirtualOutput>,
    outputs: &mut Vec<dakota::Output>,
    scenes: &mut Vec<dakota::Scene>,
    xml_file: &str,
) {
    // Now create an output to display things on
    // The virtual output is a virtual region in which a scene can be
    // positioned and displayed. One or more Outputs will be created
    // to present a region of this virtual space.
    virtual_outputs.push(
        dakota
            .create_virtual_output()
            .expect("Failed to create Dakota Virtual Output Surface"),
    );
    let idx = virtual_outputs.len() - 1;
    // The primary output is the default surface which Dakota will
    // present the application content on. This is normally a toplevel
    // desktop window, but could be any presentation surface.
    outputs.push(
        dakota
            .create_output(&virtual_outputs[idx])
            .expect("Failed to create Dakota Output"),
    );
    // For this example we default to the VirtualOutput and Output
    // being the same size.
    let resolution = outputs[idx].get_resolution();
    virtual_outputs[idx].set_size(resolution);

    // Now we can create our "scene", which will describe the layout
    // of our application content
    scenes.push(
        outputs[idx]
            .create_scene(&virtual_outputs[idx])
            .expect("Could not create scene"),
    );
    // For convenience we load our scene contents from an XML file
    scenes[idx]
        .load_xml_reader(BufReader::new(
            File::open(xml_file).expect("could not open file"),
        ))
        .expect("Could not parse XML dakota file");
    // Now refresh our scene to recalculate the layout of the contents
    // that we just loaded in
    scenes[idx]
        .recompile(&virtual_outputs[idx])
        .expect("Refreshing Dakota Scene");
}

// This is a simple Dakota program which accepts a command line argument of an
// XML file which will be loaded and presented.
fn main() {
    println!("Starting dakota");
    let args: Vec<String> = env::args().collect();
    assert!(args.len() >= 2);
    println!("Loading scene {}", args[1]);

    let mut window_count = 1;
    if args.len() >= 3 {
        window_count = args[2].parse::<usize>().expect("Invalid window count");
    }

    // Create our Dakota instance
    let mut dakota = Dakota::new().expect("Could not create dakota instance");

    let mut virtual_outputs = Vec::new();
    let mut outputs = Vec::new();
    let mut scenes = Vec::new();

    for _ in 0..window_count {
        add_output(
            &mut dakota,
            &mut virtual_outputs,
            &mut outputs,
            &mut scenes,
            &args[1],
        );
    }

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
        for (i, virtual_output) in virtual_outputs.iter_mut().enumerate() {
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
                                &mut scenes[i],
                                position,
                                (xrel.unwrap_or(0).into(), yrel.unwrap_or(0).into()),
                            )
                            .expect("Error while handling scrolling");
                        // Tell our Output to present the new contents
                        outputs[i].request_redraw();
                    }
                    _ => {}
                }
            }
        }

        // Process any events which dakota encountered on this output
        // while dispatching. These events are specific to the toplevel
        // desktop window being driven by Dakota, such as resizing or
        // closing.
        let mut dead_outputs = Vec::with_capacity(0);
        for i in 0..outputs.len() {
            while let Some(event) = outputs[i].pop_event() {
                println!("Dakota got event: {:?}", event);

                match event {
                    // This event lets us know that we should redraw the scene. This
                    // event fires depending on window system hints or if a resize
                    // has just taken place.
                    OutputEvent::Redraw => outputs[i]
                        .redraw(&virtual_outputs[i], &mut scenes[i])
                        .expect("Failed to redraw output"),
                    // If the window has been resized then we need to recompile our
                    // scene using the new output parameters. A redraw event will be
                    // signaled after this so we only need to recompute for now.
                    OutputEvent::Resized => {
                        // First handle the resize on this output
                        outputs[i].handle_resize().expect("Failed to resize output");

                        // Update our VirtualOutput with the newly resized dimensions
                        let resolution = outputs[i].get_resolution();
                        virtual_outputs[i].set_size(resolution);

                        // Because our virtual surface this scene is being applied on
                        // was changed we need to recompile it. This triggers layout
                        // of the Scene's elements.
                        scenes[i]
                            .recompile(&virtual_outputs[i])
                            .expect("Failed to handle resize of scene")
                    }
                    // Exit gracefully if this output has terminated
                    OutputEvent::Destroyed => dead_outputs.push(i),
                }
            }
        }

        for i in 0..dead_outputs.len() {
            let idx = dead_outputs[i];
            scenes.remove(idx);
            outputs.remove(idx);
            virtual_outputs.remove(idx);
        }
    }
}
