extern crate dakota;
use dakota::dom::DakotaDOM;
use dakota::xml::*;
use dakota::Dakota;

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
    dak.load_xml_reader(reader);
    dak.refresh_full().unwrap();

    loop {
        dak.dispatch().unwrap();
    }
}
