//! Session working memory for agent runs.
//!
//! Store arbitrary payloads during a tool loop, keep the active LLM context small,
//! then synthesize a final answer from everything collected.
//!
//! # Scratchpad (agent-controlled)
//!
//! Register [`register_notepad_tools`] so the model can use the consolidated `notepad`
//! tool with `action=store|list|read|remove`. Optional automatic offload is available
//! via [`register_offload_hook`] when [`OffloadConfig::auto_offload`] is enabled.

mod artifact;
mod artifact_store;
mod content_quality;
mod entry;
mod error;
mod life_task;
mod life_task_registry;
mod life_task_render;
mod notepad;
mod plan;
mod plan_note_ref;
mod plan_ref;
mod plan_registry;
mod plan_render;
mod plan_task_ref;
mod proto;
mod proto_conv;
mod render;

#[cfg(feature = "superglue")]
mod tools;

#[cfg(feature = "superglue")]
mod life_task_tools;

#[cfg(feature = "superglue")]
mod artifact_tools;

#[cfg(feature = "superglue")]
mod plan_tools;

#[cfg(feature = "superglue")]
mod superglue;

#[cfg(feature = "synthesize")]
mod synthesize;

pub use artifact::{Artifact, ArtifactId};
pub use artifact_store::ArtifactStore;
pub use content_quality::is_low_signal_web_text;
pub use entry::{Entry, EntryId, StoredPayload};
pub use error::{ArtifactError, LifeTaskError, NotepadError, PlanError};
pub use life_task::{
    LifeTask, LifeTaskCreate, LifeTaskId, LifeTaskNote, LifeTaskNoteId, LifeTaskStatus,
};
pub use life_task_registry::{LifeTaskFilter, LifeTaskRegistry};
pub use life_task_render::{life_task_status_icon, render_life_task_text};
pub use notepad::Notepad;
pub use plan::{
    KeyFile, NoteId, Plan, PlanDraft, PlanDraftTask, PlanId, PlanNote, PlanProgress, PlanStatus,
    PlanTask, TaskId, TaskStatus,
};
pub use plan_note_ref::{note_ref, note_ref_for_id, parse_note_ref, resolve_note_id};
pub use plan_ref::{PUBLIC_ID_LENGTH, public_id, public_id_for_plan_id, resolve_plan_id};
pub use plan_registry::{PlanFilter, PlanRegistry};
pub use plan_render::{render_plan_text, task_status_icon};
pub use plan_task_ref::{
    parse_task_ref, resolve_task_id, task_ref, task_ref_for_id, task_ref_for_task,
};
pub use render::{RenderFormat, RenderOpts};

#[cfg(feature = "superglue")]
pub use proto_conv::{offload_config_from_json, offload_config_to_json};

#[cfg(feature = "superglue")]
pub use superglue::{
    NAVIGATION_TOOLS, OffloadConfig, OffloadHook, SCRATCHPAD_TOOLS, compact_tool_messages,
    default_never_offload_tools, is_notepad_stub, register_offload_hook, synthesis_user_prompt,
};

#[cfg(feature = "superglue")]
pub use tools::register_notepad_tools;

#[cfg(feature = "superglue")]
pub use life_task_tools::{LifeTaskToolState, life_task_registry_state, register_life_task_tools};

#[cfg(feature = "superglue")]
pub use artifact_tools::{
    ArtifactHook, ArtifactToolState, artifact_tool_state, register_artifact_tools,
};

#[cfg(feature = "superglue")]
pub use plan_tools::{
    PlanCommitHook, PlanMutateApproveFn, PlanToolState, plan_registry_state,
    plan_registry_state_with_approve, register_plan_mutation_tools, register_plan_readonly_tools,
};

#[cfg(feature = "synthesize")]
pub use synthesize::{ExtractedFact, NotepadSynthesizer, SynthesisConfig, SynthesisOutcome};

#[cfg(feature = "synthesize")]
pub use error::SynthesisError;
