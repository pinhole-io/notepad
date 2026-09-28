use std::path::PathBuf;

use jni::JNIEnv;
use jni::objects::JClass;
use jni::sys::{jlong, jstring};
use notepad::{LifeTaskCreate, LifeTaskFilter, LifeTaskId, LifeTaskRegistry, LifeTaskStatus};
use serde::Deserialize;
use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::handle::{KHandle, TaskRegistryState, alloc, remove, with_task_registry_mut};
use crate::jni_base::{jstr_from_str, jstr_to_string, jthrow};

fn parse_task_id(raw: &str) -> Result<LifeTaskId, String> {
    let uuid: Uuid = raw.parse().map_err(|_| format!("invalid task id: {raw}"))?;
    Ok(LifeTaskId(uuid))
}

fn parse_status(raw: Option<String>) -> Result<Option<LifeTaskStatus>, String> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some("open") => Ok(Some(LifeTaskStatus::Open)),
        Some("in_progress" | "active") => Ok(Some(LifeTaskStatus::InProgress)),
        Some("done" | "completed") => Ok(Some(LifeTaskStatus::Done)),
        Some("cancelled" | "canceled") => Ok(Some(LifeTaskStatus::Cancelled)),
        Some(other) => Err(format!("invalid task status: {other}")),
    }
}

fn parse_due_at(raw: Option<String>) -> Result<Option<OffsetDateTime>, String> {
    match raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(value) => OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .map(Some)
            .map_err(|_| format!("invalid due_at: {value}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListReq {
    status: Option<String>,
    tag: Option<String>,
    due_before: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateReq {
    title: String,
    description: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    status: Option<String>,
    due_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateReq {
    task_id: String,
    title: Option<String>,
    description: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    clear_description: bool,
    due_at: Option<String>,
    #[serde(default)]
    clear_due_at: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetStatusReq {
    task_id: String,
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddNoteReq {
    task_id: String,
    content: String,
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryCreate(
    mut env: JNIEnv,
    _class: JClass,
    path: jstring,
) -> jlong {
    let path_str = match jstr_to_string(&mut env, path) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return 0;
        }
    };
    let path_buf = PathBuf::from(&path_str);
    let inner = match LifeTaskRegistry::load(&path_buf) {
        Ok(reg) => reg,
        Err(e) => {
            jthrow(&mut env, &e.to_string());
            return 0;
        }
    };
    alloc(KHandle::TaskRegistry(TaskRegistryState {
        inner,
        path: path_buf,
    })) as jlong
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryDestroy(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle != 0 {
        remove(handle as u64);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistrySave(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let result = with_task_registry_mut(handle as u64, |state| {
        state.inner.save(&state.path).map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        jthrow(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryReload(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    let result = with_task_registry_mut(handle as u64, |state| {
        state
            .inner
            .reload_from_disk(&state.path)
            .map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        jthrow(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryList(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    req_json: jstring,
) -> jstring {
    let req_raw = match jstr_to_string(&mut env, req_json) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return std::ptr::null_mut();
        }
    };
    let req: ListReq = match serde_json::from_str(&req_raw) {
        Ok(v) => v,
        Err(e) => {
            jthrow(&mut env, &e.to_string());
            return std::ptr::null_mut();
        }
    };
    let result = with_task_registry_mut(handle as u64, |state| {
        let filter = LifeTaskFilter {
            status: parse_status(req.status)?,
            tag: req.tag,
            due_before: parse_due_at(req.due_before)?,
            limit: req.limit,
            include_deleted: false,
        };
        let tasks: Vec<Value> = state
            .inner
            .list(&filter)
            .iter()
            .map(|task| serde_json::to_value(task).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        serde_json::to_string(&tasks).map_err(|e| e.to_string())
    });
    match result {
        Ok(json) => jstr_from_str(&mut env, &json),
        Err(e) => {
            jthrow(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryGet(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    task_id: jstring,
) -> jstring {
    let task_id = match jstr_to_string(&mut env, task_id) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return std::ptr::null_mut();
        }
    };
    let result = with_task_registry_mut(handle as u64, |state| {
        let id = parse_task_id(&task_id)?;
        let task = state
            .inner
            .get(id)
            .ok_or_else(|| format!("task not found: {task_id}"))?;
        serde_json::to_string(task).map_err(|e| e.to_string())
    });
    match result {
        Ok(json) => jstr_from_str(&mut env, &json),
        Err(e) => {
            jthrow(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryCreateTask(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    req_json: jstring,
) -> jstring {
    let req_raw = match jstr_to_string(&mut env, req_json) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return std::ptr::null_mut();
        }
    };
    let req: CreateReq = match serde_json::from_str(&req_raw) {
        Ok(v) => v,
        Err(e) => {
            jthrow(&mut env, &e.to_string());
            return std::ptr::null_mut();
        }
    };
    let result = with_task_registry_mut(handle as u64, |state| {
        let input = LifeTaskCreate {
            title: req.title,
            description: req.description,
            tags: req.tags,
            status: parse_status(req.status)?,
            due_at: parse_due_at(req.due_at)?,
        };
        let id = state.inner.create(input).map_err(|e| e.to_string())?;
        Ok(json!({ "taskId": id.to_string() }).to_string())
    });
    match result {
        Ok(json) => jstr_from_str(&mut env, &json),
        Err(e) => {
            jthrow(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryUpdate(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    req_json: jstring,
) {
    let req_raw = match jstr_to_string(&mut env, req_json) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return;
        }
    };
    let req: UpdateReq = match serde_json::from_str(&req_raw) {
        Ok(v) => v,
        Err(e) => {
            jthrow(&mut env, &e.to_string());
            return;
        }
    };
    let result = with_task_registry_mut(handle as u64, |state| {
        let id = parse_task_id(&req.task_id)?;
        let description = if req.clear_description {
            Some(None)
        } else if req.description.is_some() {
            Some(
                req.description
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            )
        } else {
            None
        };
        let due_at = if req.clear_due_at {
            Some(None)
        } else if let Some(raw) = req.due_at {
            if raw.trim().is_empty() {
                Some(None)
            } else {
                Some(parse_due_at(Some(raw))?)
            }
        } else {
            None
        };
        state
            .inner
            .update(id, req.title, description, req.tags, due_at)
            .map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        jthrow(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistrySetStatus(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    req_json: jstring,
) {
    let req_raw = match jstr_to_string(&mut env, req_json) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return;
        }
    };
    let req: SetStatusReq = match serde_json::from_str(&req_raw) {
        Ok(v) => v,
        Err(e) => {
            jthrow(&mut env, &e.to_string());
            return;
        }
    };
    let result = with_task_registry_mut(handle as u64, |state| {
        let id = parse_task_id(&req.task_id)?;
        let status =
            parse_status(Some(req.status))?.ok_or_else(|| "status is required".to_string())?;
        state
            .inner
            .set_status(id, status)
            .map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        jthrow(&mut env, &e);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryAddNote(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    req_json: jstring,
) -> jstring {
    let req_raw = match jstr_to_string(&mut env, req_json) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return std::ptr::null_mut();
        }
    };
    let req: AddNoteReq = match serde_json::from_str(&req_raw) {
        Ok(v) => v,
        Err(e) => {
            jthrow(&mut env, &e.to_string());
            return std::ptr::null_mut();
        }
    };
    let result = with_task_registry_mut(handle as u64, |state| {
        let id = parse_task_id(&req.task_id)?;
        let note_id = state
            .inner
            .add_note(id, req.content)
            .map_err(|e| e.to_string())?;
        Ok(json!({ "noteId": note_id.to_string() }).to_string())
    });
    match result {
        Ok(json) => jstr_from_str(&mut env, &json),
        Err(e) => {
            jthrow(&mut env, &e);
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_com_notepad_kt_NotepadNativeJni_taskRegistryDelete(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    task_id: jstring,
) {
    let task_id = match jstr_to_string(&mut env, task_id) {
        Ok(s) => s,
        Err(e) => {
            jthrow(&mut env, &e);
            return;
        }
    };
    let result = with_task_registry_mut(handle as u64, |state| {
        let id = parse_task_id(&task_id)?;
        state.inner.delete(id).map_err(|e| e.to_string())
    });
    if let Err(e) = result {
        jthrow(&mut env, &e);
    }
}
