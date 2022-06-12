extern crate dakota;
use dakota::event::Event;
use dakota::Dakota;

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
    let dom = dak
        .load_xml_reader(reader)
        .expect("Could not parse XML dakota file");
    dak.refresh_full(&dom).unwrap();

    loop {
        dak.dispatch(&dom, None).unwrap();

        for event in dak.get_events().iter() {
            // Exit if the window is closed, else do nothing
            match event {
                Event::WindowClosed { .. } => return,
                _ => (),
            }
        }
    }
}
