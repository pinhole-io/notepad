//! Shared handle table for notepad instances across native binding modules.

pub mod ffi;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use notepad::Notepad;
use notepad::{
    OffloadConfig, compact_tool_messages, register_notepad_tools, register_offload_hook,
};
use superglue::hooks::HookRegistry;
use superglue::openai::ChatMessage;
use superglue::tools::{ToolInvokeError, ToolRegistry};
use tokio::sync::RwLock as AsyncRwLock;

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn handles() -> &'static RwLock<HashMap<u64, Arc<AsyncRwLock<Notepad>>>> {
    static HANDLES: OnceLock<RwLock<HashMap<u64, Arc<AsyncRwLock<Notepad>>>>> = OnceLock::new();
    HANDLES.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn alloc_notepad(notepad: Arc<AsyncRwLock<Notepad>>) -> u64 {
    let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    handles()
        .write()
        .expect("handle table lock")
        .insert(id, notepad);
    id
}

pub fn release_notepad(handle: u64) {
    handles()
        .write()
        .expect("handle table lock")
        .remove(&handle);
}

pub fn get_notepad(handle: u64) -> Option<Arc<AsyncRwLock<Notepad>>> {
    handles()
        .read()
        .expect("handle table lock")
        .get(&handle)
        .cloned()
}

pub async fn attach_tools(registry: &ToolRegistry, handle: u64) -> Result<(), ToolInvokeError> {
    let notepad = get_notepad(handle).ok_or_else(|| {
        ToolInvokeError::handler(format!("invalid notepad handle: {handle}"), None)
    })?;
    register_notepad_tools(registry, notepad).await
}

pub async fn attach_offload_hook(
    hooks: &HookRegistry,
    handle: u64,
    config: OffloadConfig,
) -> Result<(), String> {
    let notepad = get_notepad(handle).ok_or_else(|| format!("invalid notepad handle: {handle}"))?;
    register_offload_hook(hooks, notepad, config).await;
    Ok(())
}

pub async fn compact_tool_messages_handle(
    messages: &mut [ChatMessage],
    handle: u64,
    config: &OffloadConfig,
) -> Result<(), String> {
    let notepad = get_notepad(handle).ok_or_else(|| format!("invalid notepad handle: {handle}"))?;
    let mut np = notepad.write().await;
    compact_tool_messages(messages, &mut np, config);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn alloc_and_release() {
        let np = Arc::new(AsyncRwLock::new(Notepad::new()));
        let h = alloc_notepad(Arc::clone(&np));
        assert!(get_notepad(h).is_some());
        release_notepad(h);
        assert!(get_notepad(h).is_none());
    }

    #[tokio::test]
    async fn attach_tools_invalid_handle() {
        let registry = ToolRegistry::new();
        let err = attach_tools(&registry, 9999).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("invalid notepad handle"));
    }
}
