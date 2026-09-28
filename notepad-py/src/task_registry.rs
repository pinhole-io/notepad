//! Python bindings for life-wide task registry.

use std::path::PathBuf;

use notepad::{LifeTaskCreate, LifeTaskFilter, LifeTaskId, LifeTaskRegistry, LifeTaskStatus};
use pyo3::IntoPyObjectExt;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyList;
use time::OffsetDateTime;
use uuid::Uuid;

fn parse_task_id(task_id: &str) -> PyResult<LifeTaskId> {
    let uuid: Uuid = task_id
        .parse()
        .map_err(|_| PyValueError::new_err(format!("invalid task id: {task_id}")))?;
    Ok(LifeTaskId(uuid))
}

fn parse_status(raw: Option<&str>) -> PyResult<Option<LifeTaskStatus>> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some("open") => Ok(Some(LifeTaskStatus::Open)),
        Some("in_progress" | "active") => Ok(Some(LifeTaskStatus::InProgress)),
        Some("done" | "completed") => Ok(Some(LifeTaskStatus::Done)),
        Some("cancelled" | "canceled") => Ok(Some(LifeTaskStatus::Cancelled)),
        Some(other) => Err(PyValueError::new_err(format!(
            "invalid task status: {other}"
        ))),
    }
}

fn parse_due_at(raw: Option<&str>) -> PyResult<Option<OffsetDateTime>> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(value) => OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
            .map(Some)
            .map_err(|_| {
                PyValueError::new_err(format!(
                    "invalid due_at: {value} (use RFC3339, e.g. 2026-07-25T17:00:00Z)"
                ))
            }),
    }
}

fn task_to_py(py: Python<'_>, task: &notepad::LifeTask) -> PyResult<Py<PyAny>> {
    let json_mod = py.import("json")?;
    let json_str =
        serde_json::to_string(task).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    json_mod.call_method1("loads", (json_str,))?.into_py_any(py)
}

#[pyclass(name = "TaskRegistry")]
pub struct PyTaskRegistry {
    inner: LifeTaskRegistry,
    path: PathBuf,
}

#[pymethods]
impl PyTaskRegistry {
    #[new]
    fn new(path: String) -> PyResult<Self> {
        let path = PathBuf::from(path);
        let inner =
            LifeTaskRegistry::load(&path).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(Self { inner, path })
    }

    #[getter]
    fn path(&self) -> String {
        self.path.display().to_string()
    }

    fn save(&self) -> PyResult<()> {
        self.inner
            .save(&self.path)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    fn reload(&mut self) -> PyResult<()> {
        self.inner
            .reload_from_disk(&self.path)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    #[pyo3(signature = (status=None, tag=None, due_before=None, limit=None))]
    fn list(
        &self,
        py: Python<'_>,
        status: Option<&str>,
        tag: Option<&str>,
        due_before: Option<&str>,
        limit: Option<usize>,
    ) -> PyResult<Py<PyList>> {
        let filter = LifeTaskFilter {
            status: parse_status(status)?,
            tag: tag.map(str::to_string),
            due_before: parse_due_at(due_before)?,
            limit,
            include_deleted: false,
        };
        let list = PyList::empty(py);
        for task in self.inner.list(&filter) {
            list.append(task_to_py(py, task)?)?;
        }
        Ok(list.into())
    }

    fn get(&self, py: Python<'_>, task_id: &str) -> PyResult<Option<Py<PyAny>>> {
        let id = parse_task_id(task_id)?;
        Ok(self.inner.get(id).map(|t| task_to_py(py, t)).transpose()?)
    }

    #[pyo3(signature = (title, description=None, tags=None, status=None, due_at=None))]
    fn create(
        &mut self,
        title: &str,
        description: Option<&str>,
        tags: Option<Vec<String>>,
        status: Option<&str>,
        due_at: Option<&str>,
    ) -> PyResult<String> {
        let input = LifeTaskCreate {
            title: title.to_string(),
            description: description.map(str::to_string),
            tags: tags.unwrap_or_default(),
            status: parse_status(status)?,
            due_at: parse_due_at(due_at)?,
        };
        let id = self
            .inner
            .create(input)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(id.to_string())
    }

    #[pyo3(signature = (task_id, title=None, description=None, tags=None, due_at=None, clear_description=false, clear_due_at=false))]
    fn update(
        &mut self,
        task_id: &str,
        title: Option<&str>,
        description: Option<&str>,
        tags: Option<Vec<String>>,
        due_at: Option<&str>,
        clear_description: bool,
        clear_due_at: bool,
    ) -> PyResult<()> {
        let id = parse_task_id(task_id)?;
        let description_update = if clear_description {
            Some(None)
        } else if description.is_some() {
            Some(
                description
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
            )
        } else {
            None
        };
        let due_at_update = if clear_due_at {
            Some(None)
        } else if let Some(raw) = due_at {
            if raw.trim().is_empty() {
                Some(None)
            } else {
                Some(parse_due_at(Some(raw))?)
            }
        } else {
            None
        };
        self.inner
            .update(
                id,
                title.map(str::to_string),
                description_update,
                tags,
                due_at_update,
            )
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    fn set_status(&mut self, task_id: &str, status: &str) -> PyResult<()> {
        let id = parse_task_id(task_id)?;
        let status = parse_status(Some(status))?
            .ok_or_else(|| PyValueError::new_err("status is required"))?;
        self.inner
            .set_status(id, status)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    fn add_note(&mut self, task_id: &str, content: &str) -> PyResult<String> {
        let id = parse_task_id(task_id)?;
        let note_id = self
            .inner
            .add_note(id, content.to_string())
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        Ok(note_id.to_string())
    }

    fn delete(&mut self, task_id: &str) -> PyResult<()> {
        let id = parse_task_id(task_id)?;
        self.inner
            .delete(id)
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "TaskRegistry(path={:?}, tasks={})",
            self.path,
            self.inner.tasks().len()
        )
    }
}

pub fn register_task_registry(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyTaskRegistry>()?;
    Ok(())
}
