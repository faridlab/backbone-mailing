use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use super::AuditMetadata;

/// Strongly-typed ID for MailingContact
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailingContactId(pub Uuid);

impl MailingContactId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MailingContactId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MailingContactId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MailingContactId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MailingContactId> for Uuid {
    fn from(id: MailingContactId) -> Self { id.0 }
}

impl AsRef<Uuid> for MailingContactId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MailingContactId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MailingContact {
    pub id: Uuid,
    pub name: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub email: String,
    pub company_name: Option<String>,
    pub country_code: Option<String>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl MailingContact {
    /// Create a builder for MailingContact
    pub fn builder() -> MailingContactBuilder {
        <MailingContactBuilder as Default>::default()
    }

    /// Create a new MailingContact with required fields
    pub fn new(email: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: None,
            first_name: None,
            last_name: None,
            email,
            company_name: None,
            country_code: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MailingContactId {
        MailingContactId(self.id)
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

    /// Set the name field (chainable)
    pub fn with_name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the first_name field (chainable)
    pub fn with_first_name(mut self, value: String) -> Self {
        self.first_name = Some(value);
        self
    }

    /// Set the last_name field (chainable)
    pub fn with_last_name(mut self, value: String) -> Self {
        self.last_name = Some(value);
        self
    }

    /// Set the company_name field (chainable)
    pub fn with_company_name(mut self, value: String) -> Self {
        self.company_name = Some(value);
        self
    }

    /// Set the country_code field (chainable)
    pub fn with_country_code(mut self, value: String) -> Self {
        self.country_code = Some(value);
        self
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
                "first_name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.first_name = v; }
                }
                "last_name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.last_name = v; }
                }
                "email" => {
                    if let Ok(v) = serde_json::from_value(value) { self.email = v; }
                }
                "company_name" => {
                    if let Ok(v) = serde_json::from_value(value) { self.company_name = v; }
                }
                "country_code" => {
                    if let Ok(v) = serde_json::from_value(value) { self.country_code = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for MailingContact {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "MailingContact"
    }
}

impl backbone_core::PersistentEntity for MailingContact {
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

impl backbone_orm::EntityRepoMeta for MailingContact {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["email"]
    }
}

/// Builder for MailingContact entity
///
/// Provides a fluent API for constructing MailingContact instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MailingContactBuilder {
    name: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    company_name: Option<String>,
    country_code: Option<String>,
}

impl MailingContactBuilder {
    /// Set the name field (optional)
    pub fn name(mut self, value: String) -> Self {
        self.name = Some(value);
        self
    }

    /// Set the first_name field (optional)
    pub fn first_name(mut self, value: String) -> Self {
        self.first_name = Some(value);
        self
    }

    /// Set the last_name field (optional)
    pub fn last_name(mut self, value: String) -> Self {
        self.last_name = Some(value);
        self
    }

    /// Set the email field (required)
    pub fn email(mut self, value: String) -> Self {
        self.email = Some(value);
        self
    }

    /// Set the company_name field (optional)
    pub fn company_name(mut self, value: String) -> Self {
        self.company_name = Some(value);
        self
    }

    /// Set the country_code field (optional)
    pub fn country_code(mut self, value: String) -> Self {
        self.country_code = Some(value);
        self
    }

    /// Build the MailingContact entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<MailingContact, String> {
        let email = self.email.ok_or_else(|| "email is required".to_string())?;

        Ok(MailingContact {
            id: Uuid::new_v4(),
            name: self.name,
            first_name: self.first_name,
            last_name: self.last_name,
            email,
            company_name: self.company_name,
            country_code: self.country_code,
            metadata: AuditMetadata::default(),
        })
    }
}
