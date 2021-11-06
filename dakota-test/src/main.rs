extern crate dakota;
use dakota::{Dakota, DakotaError};

use std::env;
use std::fs::File;
use std::io::BufReader;

fn main() {
    println!("Starting dakota");
    let args: Vec<String> = env::args().collect();
    assert!(args.len() > 1);
    println!("Loading scene {}", args[1]);

    let f = File::open(&args[1]).expect("could not open file");
    let reader = BufReader::new(f);

    let mut dak = Dakota::new().expect("Could not create dakota instance");
    dak.load_xml_reader(reader)
        .expect("Could not parse XML dakota file");
    dak.refresh_full().unwrap();

    loop {
        // Pass errors through to a big panic below
        // Continue normally if everything is Ok or if out of date
        // and the window needs redrawn
        let err = match dak.dispatch(|| {}) {
            // Dispatch was successful. If Dakota says the window was
            // closed then we can exit here.
            Ok(should_exit) => {
                if should_exit {
                    break;
                }
                continue;
            }
            // If things were not successful there can be two reasons:
            // 1. there was a legitimate failure and we should bail
            // 2. the window's drawable is out of date. The window has
            // been resized and we need to redraw. Dakota will handle the
            // redrawing for us, but we still get notified it happened so
            // the app can update anything it wants before re-dispatching.
            Err(e) => match e.downcast::<DakotaError>() {
                Ok(e) => match e {
                    DakotaError::OUT_OF_DATE => continue,
                    e => dakota::Error::from(e),
                },
                Err(e) => e,
            },
        };
        panic!("Error while dispatching dakota for drawing {:?}", err)
    }
}
