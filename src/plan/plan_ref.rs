//! Stable short identifiers for persisted plans.

use super::registry::PlanRegistry;
use crate::plan::{Plan, PlanId};
use uuid::Uuid;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
pub const PUBLIC_ID_LENGTH: usize = 8;

/// Build a stable, human-readable identifier from the canonical plan UUID.
#[must_use]
pub fn public_id_for_plan_id(id: PlanId) -> String {
    public_id_for_plan_id_with_attempt(id, 0)
}

/// Build a deterministic alternate identifier when a very rare collision occurs.
#[must_use]
pub fn public_id_for_plan_id_with_attempt(id: PlanId, attempt: u32) -> String {
    let mut bytes = [0u8; 5];
    bytes.copy_from_slice(&id.0.as_bytes()[..5]);
    let salt = attempt.to_le_bytes();
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte ^= salt[index % salt.len()];
    }

    let mut output = String::with_capacity(PUBLIC_ID_LENGTH);
    let value = u64::from_be_bytes([0, 0, 0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]]);
    for shift in (0..PUBLIC_ID_LENGTH).rev() {
        let index = ((value >> (shift * 5)) & 0x1f) as usize;
        output.push(ALPHABET[index] as char);
    }
    output
}

/// Resolve a public plan id or a legacy canonical UUID.
pub fn resolve_plan_id(registry: &PlanRegistry, raw: &str) -> Result<PlanId, String> {
    let trimmed = raw.trim();
    if let Ok(uuid) = trimmed.parse::<Uuid>() {
        let id = PlanId(uuid);
        if registry.get(id).is_some() {
            return Ok(id);
        }
    }

    registry
        .plans()
        .iter()
        .find(|plan| plan.public_id.eq_ignore_ascii_case(trimmed))
        .map(|plan| plan.id)
        .ok_or_else(|| format!("plan not found: {trimmed}"))
}

/// Return the public id used in agent-facing plan output.
#[must_use]
pub fn public_id(plan: &Plan) -> &str {
    &plan.public_id
}

/// True when `value` is one public plan id.
#[must_use]
pub fn is_public_plan_id(value: &str) -> bool {
    let mut count = 0usize;
    for ch in value.chars() {
        if !ch.is_ascii() || !ALPHABET.contains(&(ch.to_ascii_uppercase() as u8)) {
            return false;
        }
        count += 1;
        if count > PUBLIC_ID_LENGTH {
            return false;
        }
    }
    count == PUBLIC_ID_LENGTH
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlanDraft, PlanDraftTask};

    fn draft() -> PlanDraft {
        PlanDraft {
            title: "Plan".into(),
            overview: "Overview".into(),
            key_files: vec![],
            context: vec![],
            tasks: vec![PlanDraftTask {
                title: "Task".into(),
                description: None,
                files: vec![],
            }],
            verification: vec![],
            published_output: String::new(),
            notes: vec![],
        }
    }

    #[test]
    fn public_id_is_stable_and_short() {
        let id = PlanId(Uuid::from_u128(1));
        assert_eq!(public_id_for_plan_id(id), public_id_for_plan_id(id));
        assert_eq!(public_id_for_plan_id(id).len(), PUBLIC_ID_LENGTH);
        assert!(is_public_plan_id(&public_id_for_plan_id(id)));
        assert!(!is_public_plan_id("PLANNING"));
        assert!(!is_public_plan_id("K3M9P2Q"));
    }

    #[test]
    fn resolves_public_id_and_uuid() {
        let mut registry = PlanRegistry::new();
        let id = registry.commit(draft(), None);
        let public_id = registry.get(id).unwrap().public_id.clone();

        assert_eq!(resolve_plan_id(&registry, &public_id), Ok(id));
        assert_eq!(resolve_plan_id(&registry, &id.to_string()), Ok(id));
    }
}
