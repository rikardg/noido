use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use engine_hayro::HayroEngine;
use noido_engine::Properties;
use noido_render::read_properties;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

#[test]
fn properties_arrive_and_wake_the_app() {
    let bytes = Arc::new(std::fs::read(format!("{FIXTURES}/properties.pdf")).unwrap());
    let (wake, woken) = mpsc::channel();
    let reply = read_properties(
        Arc::new(HayroEngine),
        bytes,
        Box::new(move || {
            let _ = wake.send(());
        }),
    );
    let properties = reply.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(properties.title.as_deref(), Some("Info Title"));
    assert_eq!(properties.fonts.len(), 4);
    woken
        .recv_timeout(Duration::from_secs(5))
        .expect("the reply did not wake the app");
}

#[test]
fn a_document_that_does_not_open_has_no_properties() {
    let reply = read_properties(
        Arc::new(HayroEngine),
        Arc::new(b"not a pdf".to_vec()),
        Box::new(|| {}),
    );
    assert_eq!(
        reply.recv_timeout(Duration::from_secs(5)).unwrap(),
        Properties::default()
    );
}
