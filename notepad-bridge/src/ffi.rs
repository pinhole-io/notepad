//! C ABI for cross-extension bridge calls via a single shared library.

use std::ffi::{c_char, c_int};

use superglue::hooks::HookRegistry;
use superglue::tools::ToolRegistry;

use crate::{
    alloc_notepad, attach_offload_hook, attach_tools, compact_tool_messages_handle, release_notepad,
};
use notepad::Notepad;
use notepad::OffloadConfig;
use notepad::offload_config_from_json;
use superglue::openai::ChatMessage;
use tokio::sync::RwLock as AsyncRwLock;

fn runtime() -> &'static tokio::runtime::Runtime {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Runtime::new().expect("bridge runtime"))
}

/// Register an existing `Arc<RwLock<Notepad>>` (passed as raw arc pointer).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn notepad_bridge_register_arc(np_arc: *const AsyncRwLock<Notepad>) -> u64 {
    if np_arc.is_null() {
        return 0;
    }
    let arc = unsafe { std::sync::Arc::from_raw(np_arc) };
    let handle = alloc_notepad(std::sync::Arc::clone(&arc));
    std::mem::forget(arc);
    handle
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn notepad_bridge_release(handle: u64) {
    release_notepad(handle);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn notepad_bridge_attach_tools(
    registry: *const ToolRegistry,
    handle: u64,
) -> c_int {
    if registry.is_null() || handle == 0 {
        return -1;
    }
    match runtime().block_on(unsafe { attach_tools(&*registry, handle) }) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn notepad_bridge_attach_offload_hook(
    hooks: *const HookRegistry,
    handle: u64,
    config_json: *const c_char,
) -> c_int {
    if hooks.is_null() || handle == 0 {
        return -1;
    }
    let cfg = if config_json.is_null() {
        OffloadConfig::default()
    } else {
        let s = unsafe { std::ffi::CStr::from_ptr(config_json) };
        let s = match s.to_str() {
            Ok(v) => v,
            Err(_) => return -1,
        };
        let value: serde_json::Value = match serde_json::from_str(s) {
            Ok(v) => v,
            Err(_) => return -1,
        };
        match offload_config_from_json(&value) {
            Ok(c) => c,
            Err(_) => return -1,
        }
    };
    match runtime().block_on(unsafe { attach_offload_hook(&*hooks, handle, cfg) }) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn notepad_bridge_compact_tool_messages(
    messages_json: *const c_char,
    out_buf: *mut c_char,
    out_len: usize,
    handle: u64,
    config_json: *const c_char,
) -> c_int {
    if messages_json.is_null() || out_buf.is_null() || out_len == 0 || handle == 0 {
        return -1;
    }
    let messages_str = match unsafe { std::ffi::CStr::from_ptr(messages_json) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let mut messages: Vec<ChatMessage> = match serde_json::from_str(messages_str) {
        Ok(v) => v,
        Err(_) => return -1,
    };
    let cfg = if config_json.is_null() {
        OffloadConfig::default()
    } else {
        let s = match unsafe { std::ffi::CStr::from_ptr(config_json) }.to_str() {
            Ok(v) => v,
            Err(_) => return -1,
        };
        let value: serde_json::Value = match serde_json::from_str(s) {
            Ok(v) => v,
            Err(_) => return -1,
        };
        match offload_config_from_json(&value) {
            Ok(c) => c,
            Err(_) => return -1,
        }
    };
    if runtime()
        .block_on(compact_tool_messages_handle(&mut messages, handle, &cfg))
        .is_err()
    {
        return -1;
    }
    let out = match serde_json::to_string(&messages) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let bytes = out.as_bytes();
    if bytes.len() + 1 > out_len {
        return -2;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_buf as *mut u8, bytes.len());
        *out_buf.add(bytes.len()) = 0;
    }
    0
}
