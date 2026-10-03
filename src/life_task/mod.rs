//! Life-wide personal tasks: registry, render, and agent tools.

mod registry;
mod render;
mod types;

#[cfg(feature = "superglue")]
pub(crate) mod tools;

pub use registry::{LifeTaskFilter, LifeTaskRegistry};
pub use render::{life_task_status_icon, render_life_task_text};
pub use types::{
    LifeTask, LifeTaskCreate, LifeTaskId, LifeTaskNote, LifeTaskNoteId, LifeTaskStatus,
};

#[cfg(feature = "superglue")]
pub use tools::{
    LIFE_TASK_TOOL_NAME, LifeTaskToolState, life_task_registry_state, register_life_task_tools,
};
