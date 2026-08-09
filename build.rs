//! Build script.
//!
//! `rust-i18n` bakes the contents of `locales/` into the binary at compile time,
//! so Cargo has to be told to rebuild whenever a translation file changes.

fn main() {
    println!("cargo:rerun-if-changed=locales");
}
