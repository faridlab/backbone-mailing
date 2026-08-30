use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for MailingSubscription
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailingSubscriptionId(pub Uuid);

impl MailingSubscriptionId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MailingSubscriptionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MailingSubscriptionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MailingSubscriptionId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MailingSubscriptionId> for Uuid {
    fn from(id: MailingSubscriptionId) -> Self { id.0 }
}

impl AsRef<Uuid> for MailingSubscriptionId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MailingSubscriptionId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MailingSubscription {
    pub id: Uuid,
    pub contact_id: Uuid,
    pub mailing_audience_id: Uuid,
    pub opt_out: bool,
    pub opt_out_datetime: Option<DateTime<Utc>>,
    pub opt_out_reason_id: Option<Uuid>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl MailingSubscription {
    /// Create a builder for MailingSubscription
    pub fn builder() -> MailingSubscriptionBuilder {
        <MailingSubscriptionBuilder as Default>::default()
    }

    /// Create a new MailingSubscription with required fields
    pub fn new(contact_id: Uuid, mailing_audience_id: Uuid, opt_out: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            contact_id,
            mailing_audience_id,
            opt_out,
            opt_out_datetime: None,
            opt_out_reason_id: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MailingSubscriptionId {
        MailingSubscriptionId(self.id)
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

    /// Set the opt_out_datetime field (chainable)
    pub fn with_opt_out_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.opt_out_datetime = Some(value);
        self
    }

    /// Set the opt_out_reason_id field (chainable)
    pub fn with_opt_out_reason_id(mut self, value: Uuid) -> Self {
        self.opt_out_reason_id = Some(value);
        self
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "contact_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.contact_id = v; }
                }
                "mailing_audience_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.mailing_audience_id = v; }
                }
                "opt_out" => {
                    if let Ok(v) = serde_json::from_value(value) { self.opt_out = v; }
                }
                "opt_out_datetime" => {
                    if let Ok(v) = serde_json::from_value(value) { self.opt_out_datetime = v; }
                }
                "opt_out_reason_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.opt_out_reason_id = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for MailingSubscription {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "MailingSubscription"
    }
}

impl backbone_core::PersistentEntity for MailingSubscription {
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

impl backbone_orm::EntityRepoMeta for MailingSubscription {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("contact_id".to_string(), "uuid".to_string());
        m.insert("mailing_audience_id".to_string(), "uuid".to_string());
        m.insert("opt_out_reason_id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &[]
    }
}

/// Builder for MailingSubscription entity
///
/// Provides a fluent API for constructing MailingSubscription instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MailingSubscriptionBuilder {
    contact_id: Option<Uuid>,
    mailing_audience_id: Option<Uuid>,
    opt_out: Option<bool>,
    opt_out_datetime: Option<DateTime<Utc>>,
    opt_out_reason_id: Option<Uuid>,
}

impl MailingSubscriptionBuilder {
    /// Set the contact_id field (required)
    pub fn contact_id(mut self, value: Uuid) -> Self {
        self.contact_id = Some(value);
        self
    }

    /// Set the mailing_audience_id field (required)
    pub fn mailing_audience_id(mut self, value: Uuid) -> Self {
        self.mailing_audience_id = Some(value);
        self
    }

    /// Set the opt_out field (default: `false`)
    pub fn opt_out(mut self, value: bool) -> Self {
        self.opt_out = Some(value);
        self
    }

    /// Set the opt_out_datetime field (optional)
    pub fn opt_out_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.opt_out_datetime = Some(value);
        self
    }

    /// Set the opt_out_reason_id field (optional)
    pub fn opt_out_reason_id(mut self, value: Uuid) -> Self {
        self.opt_out_reason_id = Some(value);
        self
    }

    /// Build the MailingSubscription entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<MailingSubscription, String> {
        let contact_id = self.contact_id.ok_or_else(|| "contact_id is required".to_string())?;
        let mailing_audience_id = self.mailing_audience_id.ok_or_else(|| "mailing_audience_id is required".to_string())?;

        Ok(MailingSubscription {
            id: Uuid::new_v4(),
            contact_id,
            mailing_audience_id,
            opt_out: self.opt_out.unwrap_or(false),
            opt_out_datetime: self.opt_out_datetime,
            opt_out_reason_id: self.opt_out_reason_id,
            metadata: AuditMetadata::default(),
        })
    }
}
