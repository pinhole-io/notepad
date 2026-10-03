use super::types::{Artifact, ArtifactId, derive_artifact_description};
use crate::error::ArtifactError;

/// In-memory session store for markdown artifacts.
#[derive(Debug, Clone, Default)]
pub struct ArtifactStore {
    artifacts: Vec<Artifact>,
}

impl ArtifactStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            artifacts: Vec::new(),
        }
    }

    pub fn create(
        &mut self,
        title: String,
        content: String,
        description: Option<String>,
    ) -> Artifact {
        let description = description
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| derive_artifact_description(&content, &title));
        let artifact = Artifact::new(title, content, description);
        self.artifacts.push(artifact.clone());
        artifact
    }

    pub fn update(
        &mut self,
        id: ArtifactId,
        title: Option<String>,
        content: String,
        description: Option<String>,
    ) -> Result<Artifact, ArtifactError> {
        let artifact = self
            .artifacts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| ArtifactError::NotFound(id.to_string()))?;
        if let Some(title) = title {
            artifact.title = title;
        }
        artifact.content = content.clone();
        artifact.description = description
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| derive_artifact_description(&content, &artifact.title));
        artifact.version = artifact.version.saturating_add(1);
        artifact.updated_at = time::OffsetDateTime::now_utc();
        Ok(artifact.clone())
    }

    #[must_use]
    pub fn get(&self, id: ArtifactId) -> Option<&Artifact> {
        self.artifacts.iter().find(|a| a.id == id)
    }

    pub fn remove(&mut self, id: ArtifactId) -> Result<Artifact, ArtifactError> {
        let pos = self
            .artifacts
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| ArtifactError::NotFound(id.to_string()))?;
        Ok(self.artifacts.remove(pos))
    }

    /// Newest first.
    #[must_use]
    pub fn list(&self) -> Vec<&Artifact> {
        self.artifacts.iter().rev().collect()
    }

    #[must_use]
    pub fn artifacts(&self) -> &[Artifact] {
        &self.artifacts
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    pub fn clear(&mut self) {
        self.artifacts.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_removes_all_artifacts() {
        let mut store = ArtifactStore::new();
        store.create("Doc".into(), "# Hello".into(), None);
        assert_eq!(store.len(), 1);
        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn create_update_and_list() {
        let mut store = ArtifactStore::new();
        let created = store.create("Doc".into(), "# Hello".into(), None);
        assert_eq!(created.version, 1);

        let updated = store
            .update(created.id, None, "# Hello world".into(), None)
            .unwrap();
        assert_eq!(updated.version, 2);
        assert_eq!(updated.content, "# Hello world");

        let listed = store.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
    }
}
