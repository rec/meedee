//! Private C ABI. Raw handles never leave the native client thread.

use std::ffi::{c_char, c_void};

pub(super) type ErrorCallback =
    unsafe extern "C" fn(*mut c_void, *const c_char, usize, *const c_void);

#[repr(C)]
pub(super) struct InputConfig {
    pub api: i32,
    pub flags: u32,
    pub port: *const c_void,
    pub name: *const c_char,
    pub context: *mut c_void,
    pub bytes: unsafe extern "C" fn(*mut c_void, i64, *const u8, usize),
    pub words: unsafe extern "C" fn(*mut c_void, i64, *const u32, usize),
    pub error_context: *mut c_void,
    pub error: ErrorCallback,
}

unsafe extern "C" {
    pub fn md_thread_init() -> i32;
    pub fn md_thread_shutdown();
    pub fn md_available(mask: *mut u32) -> i32;
    pub fn md_observer_new(
        api: i32,
        context: *mut c_void,
        error: ErrorCallback,
        out: *mut *mut c_void,
    ) -> i32;
    pub fn md_enumerate(
        handle: *mut c_void,
        input: u32,
        context: *mut c_void,
        visit: unsafe extern "C" fn(*mut c_void, *const c_void),
    ) -> i32;
    pub fn md_port_clone(input: u32, port: *const c_void, out: *mut *mut c_void) -> i32;
    pub fn md_port_info(
        input: u32,
        port: *const c_void,
        id: *mut u64,
        name: *mut *const c_char,
        size: *mut usize,
    ) -> i32;
    pub fn md_port_free(input: u32, port: *mut c_void) -> i32;
    pub fn md_observer_free(handle: *mut c_void) -> i32;
    pub fn md_input_new(config: *const InputConfig, out: *mut *mut c_void) -> i32;
    pub fn md_input_free(handle: *mut c_void) -> i32;
    pub fn md_output_new(
        api: i32,
        port: *const c_void,
        name: *const c_char,
        context: *mut c_void,
        error: ErrorCallback,
        out: *mut *mut c_void,
    ) -> i32;
    pub fn md_output_free(handle: *mut c_void) -> i32;
    pub fn md_send_bytes(handle: *mut c_void, bytes: *const u8, size: usize) -> i32;
    pub fn md_send_words(handle: *mut c_void, words: *const u32, size: usize) -> i32;
}
