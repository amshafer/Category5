extern crate dakota;
use dakota::dom::DakotaDOM;
use dakota::Dakota;

fn main() {
    println!("Starting dakota");

    let dak = Dakota::new().expect("Could not create dakota instance");
}
