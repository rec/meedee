//! Opaque instrument dump requests and capture.
mod dump_models;
mod dumps;
pub use dumps::{dump, dump_with, DumpOptions, DumpReport};

/// Explicit supported model names. Remote scopes and manual instructions are
/// documented in doc/sysex-dumps.md; a listed name does not imply remote support.
pub fn models() -> Vec<&'static str> {
    dump_models::MODELS.to_vec()
}
