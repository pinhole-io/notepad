//! Plans: durable multi-step work with tasks, notes, and agent tools.

mod note_ref;
mod plan_ref;
mod registry;
mod render;
mod task_ref;
mod types;

#[cfg(feature = "superglue")]
pub(crate) mod tools;

pub use note_ref::{note_ref, note_ref_for_id, parse_note_ref, resolve_note_id};
pub use plan_ref::{PUBLIC_ID_LENGTH, public_id, public_id_for_plan_id, resolve_plan_id};
pub use registry::{PlanFilter, PlanRegistry};
pub use render::{render_plan_text, task_status_icon};
pub use task_ref::{parse_task_ref, resolve_task_id, task_ref, task_ref_for_id, task_ref_for_task};
pub use types::{
    KeyFile, NoteId, Plan, PlanDraft, PlanDraftTask, PlanId, PlanNote, PlanProgress, PlanStatus,
    PlanTask, TaskAnchor, TaskId, TaskStatus,
};

#[cfg(feature = "superglue")]
pub use tools::{
    PLAN_TOOL_NAME, PlanCommitHook, PlanMutateApproveFn, PlanToolState, plan_registry_state,
    plan_registry_state_with_approve, register_plan_mutation_tools, register_plan_readonly_tools,
};
