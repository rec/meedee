//! Device-free CoreMIDI implementation, linked only by the explicit fixture test.
#![cfg(target_os = "macos")]

unsafe extern "C" {
    fn md_fixture_enable();
    fn md_fixture_fail_send_after(endpoint: u32, accepted: i32);
    fn md_fixture_fail_disposal(endpoint: u32);
    fn md_fixture_endpoint_exists(endpoint: u32) -> bool;
    fn md_fixture_emit(endpoint: u32, data: *const u8, size: usize) -> i32;
}

pub fn enable() {
    // SAFETY: enables only the test process's fake MIDI endpoint registry.
    unsafe { md_fixture_enable() };
}

pub fn fail_disposal(endpoint: u32) {
    // SAFETY: an integer ID into the test registry, never an OS resource.
    unsafe { md_fixture_fail_disposal(endpoint) };
}

pub fn endpoint_exists(endpoint: u32) -> bool {
    // SAFETY: this queries only the test registry and retains no argument.
    unsafe { md_fixture_endpoint_exists(endpoint) }
}

pub fn emit(endpoint: u32, bytes: &[u8]) -> i32 {
    // SAFETY: borrowed bytes remain live for this synchronous fake delivery.
    unsafe { md_fixture_emit(endpoint, bytes.as_ptr(), bytes.len()) }
}

pub fn fail_send_after(endpoint: u32, accepted: i32) {
    // SAFETY: changes only the test endpoint's deterministic send fault counter.
    unsafe { md_fixture_fail_send_after(endpoint, accepted) };
}
