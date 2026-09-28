//! Opaque handle table for notepad / synthesizer native pointers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use notepad::{
    LifeTaskCreate, LifeTaskFilter, LifeTaskId, LifeTaskRegistry, LifeTaskStatus, Notepad,
    SynthesisConfig,
};
use notepad_bridge::release_notepad;
use superglue::chat::ChatOptions;
use superglue::http::HttpClient;
use tokio::sync::RwLock;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static HANDLES: OnceLock<Mutex<HashMap<u64, KHandle>>> = OnceLock::new();

fn table() -> &'static Mutex<HashMap<u64, KHandle>> {
    HANDLES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct NotepadState {
    pub inner: Arc<RwLock<Notepad>>,
    pub bridge_handle: u64,
}

pub struct SynthesizerState {
    pub notepad: Arc<RwLock<Notepad>>,
    pub http: HttpClient,
    pub chat_opts: ChatOptions,
    pub config: SynthesisConfig,
}

pub struct TaskRegistryState {
    pub inner: LifeTaskRegistry,
    pub path: PathBuf,
}

pub enum KHandle {
    Notepad(NotepadState),
    Synthesizer(SynthesizerState),
    TaskRegistry(TaskRegistryState),
}

pub fn alloc(h: KHandle) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut t = table().lock().expect("handles");
    t.insert(id, h);
    id
}

pub fn get_notepad(id: u64) -> Result<Arc<RwLock<Notepad>>, String> {
    let t = table().lock().expect("handles");
    match t.get(&id) {
        Some(KHandle::Notepad(s)) => Ok(Arc::clone(&s.inner)),
        _ => Err(format!("invalid notepad handle: {id}")),
    }
}

pub fn get_notepad_bridge_handle(id: u64) -> Result<u64, String> {
    let t = table().lock().expect("handles");
    match t.get(&id) {
        Some(KHandle::Notepad(s)) => Ok(s.bridge_handle),
        _ => Err(format!("invalid notepad handle: {id}")),
    }
}

pub fn get_synthesizer(id: u64) -> Result<SynthesizerState, String> {
    let t = table().lock().expect("handles");
    match t.get(&id) {
        Some(KHandle::Synthesizer(s)) => Ok(SynthesizerState {
            notepad: Arc::clone(&s.notepad),
            http: s.http.clone(),
            chat_opts: s.chat_opts.clone(),
            config: s.config.clone(),
        }),
        _ => Err(format!("invalid synthesizer handle: {id}")),
    }
}

pub fn get_task_registry(id: u64) -> Result<TaskRegistryState, String> {
    let t = table().lock().expect("handles");
    match t.get(&id) {
        Some(KHandle::TaskRegistry(s)) => Ok(TaskRegistryState {
            inner: s.inner.clone(),
            path: s.path.clone(),
        }),
        _ => Err(format!("invalid task registry handle: {id}")),
    }
}

pub fn with_task_registry_mut<F, T>(id: u64, f: F) -> Result<T, String>
where
    F: FnOnce(&mut TaskRegistryState) -> Result<T, String>,
{
    let mut t = table().lock().expect("handles");
    match t.get_mut(&id) {
        Some(KHandle::TaskRegistry(state)) => f(state),
        _ => Err(format!("invalid task registry handle: {id}")),
    }
}

pub fn remove(id: u64) -> bool {
    let mut t = table().lock().expect("handles");
    match t.remove(&id) {
        Some(KHandle::Notepad(s)) => {
            release_notepad(s.bridge_handle);
            true
        }
        Some(KHandle::Synthesizer(_)) => true,
        Some(KHandle::TaskRegistry(_)) => true,
        None => false,
    }
}
