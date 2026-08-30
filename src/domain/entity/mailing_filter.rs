use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::MailingTargetModel;
use super::AuditMetadata;

/// Strongly-typed ID for MailingFilter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailingFilterId(pub Uuid);

impl MailingFilterId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MailingFilterId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MailingFilterId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MailingFilterId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MailingFilterId> for Uuid {
    fn from(id: MailingFilterId) -> Self { id.0 }
}

impl AsRef<Uuid> for MailingFilterId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MailingFilterId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MailingFilter {
    pub id: Uuid,
    pub name: String,
    pub target_model: MailingTargetModel,
    pub domain: serde_json::Value,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl MailingFilter {
    /// Create a builder for MailingFilter
    pub fn builder() -> MailingFilterBuilder {
        <MailingFilterBuilder as Default>::default()
    }

    /// Create a new MailingFilter with required fields
    pub fn new(name: String, target_model: MailingTargetModel, domain: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            target_model,
            domain,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MailingFilterId {
        MailingFilterId(self.id)
    }

    /// Get when this entity was created
    pub fn created_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.created_at.as_ref()
    }

    /// Get when this entity was last updated
    pub fn updated_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.updated_at.as_ref()
    }

    /// Check if this entity is soft deleted
    pub fn is_deleted(&self) -> bool {
        self.metadata.deleted_at.is_some()
    }

    /// Check if this entity is active (not deleted)
    pub fn is_active(&self) -> bool {
        self.metadata.deleted_at.is_none()
    }

    /// Get when this entity was deleted
    pub fn deleted_at(&self) -> Option<&DateTime<Utc>> {
        self.metadata.deleted_at.as_ref()
    }

    /// Get who created this entity
    pub fn created_by(&self) -> Option<&Uuid> {
        self.metadata.created_by.as_ref()
    }

    /// Get who last updated this entity
    pub fn updated_by(&self) -> Option<&Uuid> {
        self.metadata.updated_by.as_ref()
    }

    /// Get who deleted this entity
    pub fn deleted_by(&self) -> Option<&Uuid> {
        self.metadata.deleted_by.as_ref()
    }


    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.name = v; }
                }
                "target_model" => {
                    if let Ok(v) = serde_json::from_value(value) { self.target_model = v; }
                }
                "domain" => {
                    if let Ok(v) = serde_json::from_value(value) { self.domain = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for MailingFilter {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "MailingFilter"
    }
}

impl backbone_core::PersistentEntity for MailingFilter {
    fn entity_id(&self) -> String {
        self.id.to_string()
    }
    fn set_entity_id(&mut self, id: String) {
        if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
            self.id = uuid;
        }
    }
    fn created_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.created_at
    }
    fn set_created_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.created_at = Some(ts);
    }
    fn updated_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.updated_at
    }
    fn set_updated_at(&mut self, ts: chrono::DateTime<chrono::Utc>) {
        self.metadata.updated_at = Some(ts);
    }
    fn deleted_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.metadata.deleted_at
    }
    fn set_deleted_at(&mut self, ts: Option<chrono::DateTime<chrono::Utc>>) {
        self.metadata.deleted_at = ts;
    }
}

impl backbone_orm::EntityRepoMeta for MailingFilter {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("target_model".to_string(), "mailing_target_model".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["name"]
    }
}

/// Builder for MailingFilter entity
///
/// Provides a fluent API for constructing MailingFilter instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MailingFilterBuilder {
    name: Option<String>,
    target_model: Option<MailingTargetModel>,
    domain: Option<serde_json::Value>,
}

impl MailingFilterBuilder {
    /// Set the name field (required)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the target_model field (default: `MailingTargetModel::default()`)
    pub fn target_model(mut self, value: MailingTargetModel) -> Self {
        self.target_model = Some(value);
        self
    }

    /// Set the domain field (default: `serde_json::json!({})`)
    pub fn domain(mut self, value: serde_json::Value) -> Self {
        self.domain = Some(value);
        self
    }

    /// Build the MailingFilter entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<MailingFilter, String> {
        let name = self.name.ok_or_else(|| "name is required".to_string())?;

        Ok(MailingFilter {
            id: Uuid::new_v4(),
            name,
            target_model: self.target_model.unwrap_or_default(),
            domain: self.domain.unwrap_or(serde_json::json!({})),
            metadata: AuditMetadata::default(),
        })
    }
}
