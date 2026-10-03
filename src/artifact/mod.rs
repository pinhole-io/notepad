//! Session artifacts: store and agent tools.

mod store;
mod types;

#[cfg(feature = "superglue")]
pub(crate) mod tools;

pub use store::ArtifactStore;
pub use types::{Artifact, ArtifactId};

#[cfg(feature = "superglue")]
pub use tools::{
    ARTIFACT_TOOL_NAME, ArtifactHook, ArtifactToolState, artifact_tool_state,
    register_artifact_tools,
};
