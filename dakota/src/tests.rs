/// Dakota tests
///
/// Austin Shafer - 2024
use crate as dak;
use std::fs::File;
use std::io::BufReader;

/// our generic pixel result checker
///
/// In this case we simply hash the raw pixel dump and compare
/// it against the known gold image for the test.
///
/// We can't directly check the pixel values, or hash the results.
/// Hashing might use different algorithms on other rust versions
/// meaning it may mismatch for no reason on a machine. We use
/// the perceptualdiff program to compare with the gold image as
/// different vendors may subltly round float values differently,
/// causing a mismatch. Perceptualdiff compares the two images
/// adjusting for perceivable errors, returning 0 if there are none.
fn check_pixels(dak: &mut dak::Dakota, testname: &str) {
    let filename = [testname, ".ppm"].join("");
    dak.dump_framebuffer(&filename);
    let goldfile = ["golds/", &filename].join("");

    assert!(std::process::Command::new("perceptualdiff")
        .arg(filename)
        .arg(goldfile)
        .status()
        .expect("perceptualdiff error, probable mismatch")
        .success());
}

/// Test one of the scenes
///
/// This will render one frame with Dakota of the specified test
/// scene from dakota-test
fn test_file(testname: &str) {
    let mut dak = dak::Dakota::new().expect("Could not create Dakota");

    let filename = ["../dakota-test/data/", testname, ".xml"].join("");
    let f = File::open(&filename).expect("could not open file");
    let reader = BufReader::new(f);

    let dom = dak
        .load_xml_reader(reader)
        .expect("Could not parse XML dakota file");
    dak.refresh_full(&dom).expect("Refreshing Dakota");
    dak.set_resolution(&dom, 640, 480).unwrap();
    dak.dispatch(&dom, None).expect("Dakota rendering failed");

    // ------------ check output -------------
    check_pixels(&mut dak, testname);
}

#[test]
fn scene1() {
    test_file("scene1")
}

#[test]
fn color() {
    test_file("color")
}

#[test]
fn events() {
    test_file("events")
}

#[test]
fn relative() {
    test_file("relative")
}

#[test]
fn scene2() {
    test_file("scene2")
}

#[test]
fn text() {
    test_file("text")
}

#[test]
fn tiling() {
    test_file("tiling")
}
