//! Entry point for `ospc`, the short alias of `openspectation`.
//!
//! Same binary, same flags; the name is only shorter to type.

use std::process::ExitCode;

fn main() -> ExitCode {
    openspectation::run()
}
