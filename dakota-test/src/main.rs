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

fn main() {
    println!("Starting dakota");
    let args: Vec<String> = env::args().collect();
    assert!(args.len() > 1);
    println!("Loading scene {}", args[1]);

    #[cfg(feature = "renderdoc")]
    let mut doc: RenderDoc<renderdoc::V141> = RenderDoc::new().unwrap();
    #[cfg(feature = "renderdoc")]
    let mut renderdoc_recording = false;

    let f = File::open(&args[1]).expect("could not open file");
    let reader = BufReader::new(f);

    let mut dak = Dakota::new().expect("Could not create dakota instance");
    let dom = dak
        .load_xml_reader(reader)
        .expect("Could not parse XML dakota file");
    dak.refresh_full(&dom).expect("Refreshing Dakota");
    let mut stop = StopWatch::new();

    loop {
        #[cfg(feature = "renderdoc")]
        if renderdoc_recording {
            doc.start_frame_capture(std::ptr::null(), std::ptr::null());
        }

        stop.start();
        dak.dispatch(&dom, None).unwrap();
        stop.end();

        #[cfg(feature = "renderdoc")]
        if renderdoc_recording {
            doc.end_frame_capture(std::ptr::null(), std::ptr::null());
        }

        for event in dak.drain_events() {
            println!("Dakota got event: {:?}", event);
            // Exit if the window is closed, else do nothing
            match event {
                Event::WindowClosed { .. } => return,
                #[cfg(feature = "renderdoc")]
                Event::InputKeyDown { key, modifiers: _ } => {
                    if key == dakota::input::Keycode::LCtrl {
                        renderdoc_recording = true;
                    }
                }
                _ => {}
            }
        }
    }
}
