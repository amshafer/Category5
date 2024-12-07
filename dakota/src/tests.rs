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
fn check_pixels(dak: &mut dak::Dakota, testname: &str, threshold: u32) {
    let filename = [testname, ".ppm"].join("");
    dak.dump_framebuffer(&filename);
    let goldfile = ["golds/", &filename].join("");

    let mut cmd = std::process::Command::new("perceptualdiff");
    if threshold > 0 {
        // require n pixels of difference for failure
        cmd.arg("--threshold").arg(threshold.to_string());
    }

    let result = cmd
        .arg(filename)
        .arg(goldfile)
        .status()
        .expect("perceptualdiff error, probable mismatch");
    assert!(result.success());
}

/// Test one of the scenes
///
/// This will render one frame with Dakota of the specified test
/// scene from dakota-test
fn test_file(testname: &str, threshold: u32) {
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
    check_pixels(&mut dak, testname, threshold);
}

#[test]
fn scene1() {
    test_file("scene1", 0)
}

#[test]
fn color() {
    test_file("color", 0)
}

#[test]
fn events() {
    test_file("events", 0)
}

#[test]
fn relative() {
    test_file("relative", 0)
}

#[test]
fn scene2() {
    test_file("scene2", 0)
}

#[test]
fn text() {
    // exception for hidpi laptop screen on linux
    test_file("text", 0)
}

#[test]
fn tiling() {
    test_file("tiling", 0)
}
