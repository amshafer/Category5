extern crate dakota;
use dakota::dom::DakotaDOM;
use dakota::xml::*;
use dakota::{Dakota, DakotaError};

use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;

fn main() {
    println!("Starting dakota");
    let args: Vec<String> = env::args().collect();
    assert!(args.len() > 1);
    println!("Loading scene {}", args[1]);

    let f = File::open(&args[1]).expect("could not open file");
    let mut reader = BufReader::new(f);

    let mut dak = Dakota::new().expect("Could not create dakota instance");
    dak.load_xml_reader(reader)
        .expect("Could not parse XML dakota file");
    dak.refresh_full().unwrap();

    loop {
        // Pass errors through to a big panic below
        // Continue normally if everything is Ok or if out of date
        // and the window needs redrawn
        let err = match dak.dispatch(|| {}) {
            Ok(()) => continue,
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
