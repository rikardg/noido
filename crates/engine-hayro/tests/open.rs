//! What a reader is told when a PDF can't be opened.

use engine_hayro::HayroEngine;
use hayro::hayro_syntax::Pdf;
use noido_engine::{PdfBytes, PdfEngine};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// The message `HayroEngine::open` gives for the fixture `name`.
fn open_message(name: &str) -> String {
    match HayroEngine.open(PdfBytes::from(fixture(name))) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("{name} opened"),
    }
}

#[test]
fn a_non_pdf_is_not_a_readable_pdf() {
    assert_eq!(open_message("not-a-pdf.txt"), "not a readable PDF");
}

#[test]
fn a_password_protected_pdf_says_it_is_password_protected() {
    assert_eq!(
        open_message("encrypted.pdf"),
        "this PDF is password-protected"
    );
}

#[test]
fn an_encryption_noido_cannot_read_says_so() {
    assert_eq!(
        open_message("encrypted-pubkey.pdf"),
        "this PDF is encrypted in a way noido cannot read"
    );
}

#[test]
fn damaged_encryption_says_so() {
    assert_eq!(
        open_message("encrypted-damaged.pdf"),
        "this PDF's encryption is damaged"
    );
}

#[test]
fn the_encrypted_fixture_opens_with_its_password() {
    // Not merely a broken file: the password it was made with opens it, so
    // the message above is about the one it is missing, not a damage.
    let pdf = Pdf::new_with_password(fixture("encrypted.pdf"), "noido")
        .expect("the fixture's own password opens it");
    assert_eq!(pdf.pages().len(), 1);
    // The page's content is encrypted with its own object's key too, so a
    // wrong derivation shows here rather than in a page of garbage.
    assert_eq!(
        pdf.pages()[0].page_stream(),
        Some(&b"0 0 0 rg 100 100 100 100 re f"[..])
    );
}
