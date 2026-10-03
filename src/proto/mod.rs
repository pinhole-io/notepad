//! Generated protobuf types — canonical public API surface for all language bindings.
//! Source of truth: `proto/notepad.proto`.
include!(concat!(env!("OUT_DIR"), "/notepad.rs"));

mod conv;

#[cfg(feature = "superglue")]
pub use conv::{offload_config_from_json, offload_config_to_json};
