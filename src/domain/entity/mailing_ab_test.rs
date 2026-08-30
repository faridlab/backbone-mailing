use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::AbWinnerSelection;
use super::AuditMetadata;

/// Strongly-typed ID for MailingAbTest
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailingAbTestId(pub Uuid);

impl MailingAbTestId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MailingAbTestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MailingAbTestId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MailingAbTestId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MailingAbTestId> for Uuid {
    fn from(id: MailingAbTestId) -> Self { id.0 }
}

impl AsRef<Uuid> for MailingAbTestId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MailingAbTestId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MailingAbTest {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub winner_selection: AbWinnerSelection,
    pub promote_at: Option<DateTime<Utc>>,
    pub completed: bool,
    pub winner_mailing_id: Option<Uuid>,
    pub sampling_seed: String,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl MailingAbTest {
    /// Create a builder for MailingAbTest
    pub fn builder() -> MailingAbTestBuilder {
        <MailingAbTestBuilder as Default>::default()
    }

    /// Create a new MailingAbTest with required fields
    pub fn new(campaign_id: Uuid, winner_selection: AbWinnerSelection, completed: bool, sampling_seed: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            campaign_id,
            winner_selection,
            promote_at: None,
            completed,
            winner_mailing_id: None,
            sampling_seed,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MailingAbTestId {
        MailingAbTestId(self.id)
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
    // Fluent Setters (with_* for optional fields)
    // ==========================================================

    /// Set the promote_at field (chainable)
    pub fn with_promote_at(mut self, value: DateTime<Utc>) -> Self {
        self.promote_at = Some(value);
        self
    }

    /// Set the winner_mailing_id field (chainable)
    pub fn with_winner_mailing_id(mut self, value: Uuid) -> Self {
        self.winner_mailing_id = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "campaign_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.campaign_id = v; }
                }
                "winner_selection" => {
                    if let Ok(v) = serde_json::from_value(value) { self.winner_selection = v; }
                }
                "promote_at" => {
                    if let Ok(v) = serde_json::from_value(value) { self.promote_at = v; }
                }
                "completed" => {
                    if let Ok(v) = serde_json::from_value(value) { self.completed = v; }
                }
                "winner_mailing_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.winner_mailing_id = v; }
                }
                "sampling_seed" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sampling_seed = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for MailingAbTest {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "MailingAbTest"
    }
}

impl backbone_core::PersistentEntity for MailingAbTest {
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

impl backbone_orm::EntityRepoMeta for MailingAbTest {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("campaign_id".to_string(), "uuid".to_string());
        m.insert("winner_mailing_id".to_string(), "uuid".to_string());
        m.insert("winner_selection".to_string(), "ab_winner_selection".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["sampling_seed"]
    }
}

/// Builder for MailingAbTest entity
///
/// Provides a fluent API for constructing MailingAbTest instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MailingAbTestBuilder {
    campaign_id: Option<Uuid>,
    winner_selection: Option<AbWinnerSelection>,
    promote_at: Option<DateTime<Utc>>,
    completed: Option<bool>,
    winner_mailing_id: Option<Uuid>,
    sampling_seed: Option<String>,
}

impl MailingAbTestBuilder {
    /// Set the campaign_id field (required)
    pub fn campaign_id(mut self, value: Uuid) -> Self {
        self.campaign_id = Some(value);
        self
    }

    /// Set the winner_selection field (default: `AbWinnerSelection::default()`)
    pub fn winner_selection(mut self, value: AbWinnerSelection) -> Self {
        self.winner_selection = Some(value);
        self
    }

    /// Set the promote_at field (optional)
    pub fn promote_at(mut self, value: DateTime<Utc>) -> Self {
        self.promote_at = Some(value);
        self
    }

    /// Set the completed field (default: `false`)
    pub fn completed(mut self, value: bool) -> Self {
        self.completed = Some(value);
        self
    }

    /// Set the winner_mailing_id field (optional)
    pub fn winner_mailing_id(mut self, value: Uuid) -> Self {
        self.winner_mailing_id = Some(value);
        self
    }

    /// Set the sampling_seed field (required)
    pub fn sampling_seed(mut self, value: String) -> Self {
        self.sampling_seed = Some(value);
        self
    }

    /// Build the MailingAbTest entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<MailingAbTest, String> {
        let campaign_id = self.campaign_id.ok_or_else(|| "campaign_id is required".to_string())?;
        let sampling_seed = self.sampling_seed.ok_or_else(|| "sampling_seed is required".to_string())?;

        Ok(MailingAbTest {
            id: Uuid::new_v4(),
            campaign_id,
            winner_selection: self.winner_selection.unwrap_or_default(),
            promote_at: self.promote_at,
            completed: self.completed.unwrap_or(false),
            winner_mailing_id: self.winner_mailing_id,
            sampling_seed,
            metadata: AuditMetadata::default(),
        })
    }
}
