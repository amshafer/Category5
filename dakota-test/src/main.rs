extern crate dakota;
use dakota::Dakota;
use dakota::Event;

extern crate utils;
use std::env;
use std::fs::File;
use std::io::BufReader;
use utils::timing::StopWatch;

#[cfg(feature = "renderdoc")]
extern crate renderdoc;
#[cfg(feature = "renderdoc")]
use renderdoc::RenderDoc;

// This is a simple Dakota program which accepts a command line argument of an
// XML file which will be loaded and presented.
fn main() {
    println!("Starting dakota");
    let args: Vec<String> = env::args().collect();
    assert!(args.len() > 1);
    println!("Loading scene {}", args[1]);

    #[cfg(feature = "renderdoc")]
    let mut doc: RenderDoc<renderdoc::V141> = RenderDoc::new().unwrap();
    #[cfg(feature = "renderdoc")]
    let mut renderdoc_recording = false;

    // Create our Dakota instance
    let dakota = Dakota::new().expect("Could not create dakota instance");

    // Now create an output to display things on
    // The virtual output is a virtual region in which a scene can be
    // positioned and displayed. One or more Outputs will be created
    // to present a region of this virtual space.
    let mut virtual_output = dakota.create_virtual_output();
    // The primary output is the default surface which Dakota will
    // present the application content on. This is normally a toplevel
    // desktop window, but could be any presentation surface.
    let mut output = dakota.create_output(&virtual_output);
    let resolution = virtual_output.get_resolution();
    output.set_virtual_geometry(0, 0, resolution.0, resolution.1);

    // Now we can create our "scene", which will describe the layout
    // of our application content
    let mut scene = Scene::new().expect("Could not create scene");
    // For convenience we load our scene contents from an XML file
    scene
        .load_xml_reader(BufReader::new(
            File::open(&args[1]).expect("could not open file"),
        ))
        .expect("Could not parse XML dakota file");
    // Now refresh our scene to recalculate the layout of the contents
    // that we just loaded in
    scene.recompile().expect("Refreshing Dakota Scene");

    loop {
        #[cfg(feature = "renderdoc")]
        if renderdoc_recording {
            doc.start_frame_capture(std::ptr::null(), std::ptr::null());
        }

        // Dispatch Dakota's main event loop. Here we will block waiting
        // for events and allow the
        dakota.dispatch(&mut scene, None).unwrap();

        #[cfg(feature = "renderdoc")]
        if renderdoc_recording {
            doc.end_frame_capture(std::ptr::null(), std::ptr::null());
        }

        // Process any global events first. These events show global
        // changes in state or give updates from Dakota's main polling
        // loop.
        for event in dakota.drain_events() {
            println!("Dakota got event: {:?}", event);
        }

        // Next process any events on our virtual output. This primarily includes
        // input events which get delivered to our virtual space
        for event in virtual_output.drain_events() {
            println!("Dakota got event: {:?}", event);
            match event {
                // Enable renderdoc tracing. This debugging shortcut lets us set
                // up initial state and then begin recording.
                #[cfg(feature = "renderdoc")]
                Event::InputKeyDown { key, modifiers: _ } => {
                    if key == dakota::input::Keycode::LCtrl {
                        renderdoc_recording = true;
                    }
                }
            }
        }

        // Process any events which dakota encountered on this output
        // while dispatching. These events are specific to the toplevel
        // desktop window being driven by Dakota, such as resizing or
        // closing.
        for event in output.drain_events() {
            println!("Dakota got event: {:?}", event);

            match event {
                // This event lets us know that we should redraw the scene. This
                // event fires depending on window system hints or if a resize
                // has just taken place.
                Event::Redraw => output.redraw().expect("Failed to redraw output"),
                // If the window has been resized then we need to recompile our
                // scene using the new output parameters. A redraw event will be
                // signaled after this so we only need to recompute for now.
                Event::Resized => scene
                    .recompile(&output)
                    .expect("Failed to handle resize of scene"),
                // Exit gracefully if this output has terminated
                Event::Destroyed { .. } => return,
                _ => {}
            }
        }
    }
}
