use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::MailingState;
use super::MailingScheduleType;
use super::MailingType;
use super::MailingTargetModel;
use super::AuditMetadata;

use crate::domain::state_machine::{mailing_stateStateMachine, mailing_stateState, StateMachineError};

/// Strongly-typed ID for Mailing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailingId(pub Uuid);

impl MailingId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MailingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MailingId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MailingId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MailingId> for Uuid {
    fn from(id: MailingId) -> Self { id.0 }
}

impl AsRef<Uuid> for MailingId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MailingId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Mailing {
    pub id: Uuid,
    pub subject: String,
    pub preview: Option<String>,
    pub body_html: String,
    pub email_from: String,
    pub reply_to: Option<String>,
    pub keep_archives: bool,
    pub(crate) state: MailingState,
    pub schedule_type: MailingScheduleType,
    pub schedule_date: Option<DateTime<Utc>>,
    pub sent_date: Option<DateTime<Utc>>,
    pub mailing_type: MailingType,
    pub target_model: MailingTargetModel,
    pub mailing_domain: serde_json::Value,
    pub use_exclusion_list: bool,
    pub campaign_id: Option<Uuid>,
    pub medium_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub ab_testing_enabled: bool,
    pub ab_testing_pc: i32,
    pub ab_test_id: Option<Uuid>,
    pub kpi_mail_required: bool,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl Mailing {
    /// Create a builder for Mailing
    pub fn builder() -> MailingBuilder {
        <MailingBuilder as Default>::default()
    }

    /// Create a new Mailing with required fields
    pub fn new(subject: String, body_html: String, email_from: String, keep_archives: bool, state: MailingState, schedule_type: MailingScheduleType, mailing_type: MailingType, target_model: MailingTargetModel, mailing_domain: serde_json::Value, use_exclusion_list: bool, ab_testing_enabled: bool, ab_testing_pc: i32, kpi_mail_required: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            subject,
            preview: None,
            body_html,
            email_from,
            reply_to: None,
            keep_archives,
            state,
            schedule_type,
            schedule_date: None,
            sent_date: None,
            mailing_type,
            target_model,
            mailing_domain,
            use_exclusion_list,
            campaign_id: None,
            medium_id: None,
            source_id: None,
            user_id: None,
            ab_testing_enabled,
            ab_testing_pc,
            ab_test_id: None,
            kpi_mail_required,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MailingId {
        MailingId(self.id)
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

    /// Set the preview field (chainable)
    pub fn with_preview(mut self, value: String) -> Self {
        self.preview = Some(value);
        self
    }

    /// Set the reply_to field (chainable)
    pub fn with_reply_to(mut self, value: String) -> Self {
        self.reply_to = Some(value);
        self
    }

    /// Set the schedule_date field (chainable)
    pub fn with_schedule_date(mut self, value: DateTime<Utc>) -> Self {
        self.schedule_date = Some(value);
        self
    }

    /// Set the sent_date field (chainable)
    pub fn with_sent_date(mut self, value: DateTime<Utc>) -> Self {
        self.sent_date = Some(value);
        self
    }

    /// Set the campaign_id field (chainable)
    pub fn with_campaign_id(mut self, value: Uuid) -> Self {
        self.campaign_id = Some(value);
        self
    }

    /// Set the medium_id field (chainable)
    pub fn with_medium_id(mut self, value: Uuid) -> Self {
        self.medium_id = Some(value);
        self
    }

    /// Set the source_id field (chainable)
    pub fn with_source_id(mut self, value: Uuid) -> Self {
        self.source_id = Some(value);
        self
    }

    /// Set the user_id field (chainable)
    pub fn with_user_id(mut self, value: Uuid) -> Self {
        self.user_id = Some(value);
        self
    }

    /// Set the ab_test_id field (chainable)
    pub fn with_ab_test_id(mut self, value: Uuid) -> Self {
        self.ab_test_id = Some(value);
        self
    }

    // ==========================================================
    // State Machine
    // ==========================================================

    /// Transition to a new state via the state state machine.
    ///
    /// Returns `Err` if the transition is not permitted from the current state.
    /// Use this method instead of assigning `self.state` directly.
    pub fn transition_to(&mut self, new_state: mailing_stateState) -> Result<(), StateMachineError> {
        let current = self.state.to_string().parse::<mailing_stateState>()?;
        let mut sm = mailing_stateStateMachine::from_state(current);
        sm.transition_to_state(new_state)?;
        self.state = new_state.to_string().parse::<MailingState>()
            .map_err(|e| StateMachineError::InvalidState(e.to_string()))?;
        Ok(())
    }

    // ==========================================================
    // Partial Update
    // ==========================================================

    /// Apply partial updates from a map of field name to JSON value
    pub fn apply_patch(&mut self, fields: std::collections::HashMap<String, serde_json::Value>) {
        for (key, value) in fields {
            match key.as_str() {
                "subject" => {
                    if let Ok(v) = serde_json::from_value(value) { self.subject = v; }
                }
                "preview" => {
                    if let Ok(v) = serde_json::from_value(value) { self.preview = v; }
                }
                "body_html" => {
                    if let Ok(v) = serde_json::from_value(value) { self.body_html = v; }
                }
                "email_from" => {
                    if let Ok(v) = serde_json::from_value(value) { self.email_from = v; }
                }
                "reply_to" => {
                    if let Ok(v) = serde_json::from_value(value) { self.reply_to = v; }
                }
                "keep_archives" => {
                    if let Ok(v) = serde_json::from_value(value) { self.keep_archives = v; }
                }
                "schedule_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.schedule_type = v; }
                }
                "schedule_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.schedule_date = v; }
                }
                "sent_date" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sent_date = v; }
                }
                "mailing_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.mailing_type = v; }
                }
                "target_model" => {
                    if let Ok(v) = serde_json::from_value(value) { self.target_model = v; }
                }
                "mailing_domain" => {
                    if let Ok(v) = serde_json::from_value(value) { self.mailing_domain = v; }
                }
                "use_exclusion_list" => {
                    if let Ok(v) = serde_json::from_value(value) { self.use_exclusion_list = v; }
                }
                "campaign_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.campaign_id = v; }
                }
                "medium_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.medium_id = v; }
                }
                "source_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.source_id = v; }
                }
                "user_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.user_id = v; }
                }
                "ab_testing_enabled" => {
                    if let Ok(v) = serde_json::from_value(value) { self.ab_testing_enabled = v; }
                }
                "ab_testing_pc" => {
                    if let Ok(v) = serde_json::from_value(value) { self.ab_testing_pc = v; }
                }
                "ab_test_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.ab_test_id = v; }
                }
                "kpi_mail_required" => {
                    if let Ok(v) = serde_json::from_value(value) { self.kpi_mail_required = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for Mailing {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "Mailing"
    }
}

impl backbone_core::PersistentEntity for Mailing {
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

impl backbone_orm::EntityRepoMeta for Mailing {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("campaign_id".to_string(), "uuid".to_string());
        m.insert("medium_id".to_string(), "uuid".to_string());
        m.insert("source_id".to_string(), "uuid".to_string());
        m.insert("user_id".to_string(), "uuid".to_string());
        m.insert("ab_test_id".to_string(), "uuid".to_string());
        m.insert("state".to_string(), "mailing_state".to_string());
        m.insert("schedule_type".to_string(), "mailing_schedule_type".to_string());
        m.insert("mailing_type".to_string(), "mailing_type".to_string());
        m.insert("target_model".to_string(), "mailing_target_model".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["subject", "body_html", "email_from"]
    }
}

/// Builder for Mailing entity
///
/// Provides a fluent API for constructing Mailing instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MailingBuilder {
    subject: Option<String>,
    preview: Option<String>,
    body_html: Option<String>,
    email_from: Option<String>,
    reply_to: Option<String>,
    keep_archives: Option<bool>,
    state: Option<MailingState>,
    schedule_type: Option<MailingScheduleType>,
    schedule_date: Option<DateTime<Utc>>,
    sent_date: Option<DateTime<Utc>>,
    mailing_type: Option<MailingType>,
    target_model: Option<MailingTargetModel>,
    mailing_domain: Option<serde_json::Value>,
    use_exclusion_list: Option<bool>,
    campaign_id: Option<Uuid>,
    medium_id: Option<Uuid>,
    source_id: Option<Uuid>,
    user_id: Option<Uuid>,
    ab_testing_enabled: Option<bool>,
    ab_testing_pc: Option<i32>,
    ab_test_id: Option<Uuid>,
    kpi_mail_required: Option<bool>,
}

impl MailingBuilder {
    /// Set the subject field (required)
    pub fn subject(mut self, value: String) -> Self {
        self.subject = Some(value);
        self
    }

    /// Set the preview field (optional)
    pub fn preview(mut self, value: String) -> Self {
        self.preview = Some(value);
        self
    }

    /// Set the body_html field (required)
    pub fn body_html(mut self, value: String) -> Self {
        self.body_html = Some(value);
        self
    }

    /// Set the email_from field (required)
    pub fn email_from(mut self, value: String) -> Self {
        self.email_from = Some(value);
        self
    }

    /// Set the reply_to field (optional)
    pub fn reply_to(mut self, value: String) -> Self {
        self.reply_to = Some(value);
        self
    }

    /// Set the keep_archives field (default: `true`)
    pub fn keep_archives(mut self, value: bool) -> Self {
        self.keep_archives = Some(value);
        self
    }

    /// Set the state field (default: `MailingState::default()`)
    pub fn state(mut self, value: MailingState) -> Self {
        self.state = Some(value);
        self
    }

    /// Set the schedule_type field (default: `MailingScheduleType::default()`)
    pub fn schedule_type(mut self, value: MailingScheduleType) -> Self {
        self.schedule_type = Some(value);
        self
    }

    /// Set the schedule_date field (optional)
    pub fn schedule_date(mut self, value: DateTime<Utc>) -> Self {
        self.schedule_date = Some(value);
        self
    }

    /// Set the sent_date field (optional)
    pub fn sent_date(mut self, value: DateTime<Utc>) -> Self {
        self.sent_date = Some(value);
        self
    }

    /// Set the mailing_type field (default: `MailingType::default()`)
    pub fn mailing_type(mut self, value: MailingType) -> Self {
        self.mailing_type = Some(value);
        self
    }

    /// Set the target_model field (default: `MailingTargetModel::default()`)
    pub fn target_model(mut self, value: MailingTargetModel) -> Self {
        self.target_model = Some(value);
        self
    }

    /// Set the mailing_domain field (default: `serde_json::json!({})`)
    pub fn mailing_domain(mut self, value: serde_json::Value) -> Self {
        self.mailing_domain = Some(value);
        self
    }

    /// Set the use_exclusion_list field (default: `true`)
    pub fn use_exclusion_list(mut self, value: bool) -> Self {
        self.use_exclusion_list = Some(value);
        self
    }

    /// Set the campaign_id field (optional)
    pub fn campaign_id(mut self, value: Uuid) -> Self {
        self.campaign_id = Some(value);
        self
    }

    /// Set the medium_id field (optional)
    pub fn medium_id(mut self, value: Uuid) -> Self {
        self.medium_id = Some(value);
        self
    }

    /// Set the source_id field (optional)
    pub fn source_id(mut self, value: Uuid) -> Self {
        self.source_id = Some(value);
        self
    }

    /// Set the user_id field (optional)
    pub fn user_id(mut self, value: Uuid) -> Self {
        self.user_id = Some(value);
        self
    }

    /// Set the ab_testing_enabled field (default: `false`)
    pub fn ab_testing_enabled(mut self, value: bool) -> Self {
        self.ab_testing_enabled = Some(value);
        self
    }

    /// Set the ab_testing_pc field (default: `10`)
    pub fn ab_testing_pc(mut self, value: i32) -> Self {
        self.ab_testing_pc = Some(value);
        self
    }

    /// Set the ab_test_id field (optional)
    pub fn ab_test_id(mut self, value: Uuid) -> Self {
        self.ab_test_id = Some(value);
        self
    }

    /// Set the kpi_mail_required field (default: `false`)
    pub fn kpi_mail_required(mut self, value: bool) -> Self {
        self.kpi_mail_required = Some(value);
        self
    }

    /// Build the Mailing entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<Mailing, String> {
        let subject = self.subject.ok_or_else(|| "subject is required".to_string())?;
        let body_html = self.body_html.ok_or_else(|| "body_html is required".to_string())?;
        let email_from = self.email_from.ok_or_else(|| "email_from is required".to_string())?;

        Ok(Mailing {
            id: Uuid::new_v4(),
            subject,
            preview: self.preview,
            body_html,
            email_from,
            reply_to: self.reply_to,
            keep_archives: self.keep_archives.unwrap_or(true),
            state: self.state.unwrap_or_default(),
            schedule_type: self.schedule_type.unwrap_or_default(),
            schedule_date: self.schedule_date,
            sent_date: self.sent_date,
            mailing_type: self.mailing_type.unwrap_or_default(),
            target_model: self.target_model.unwrap_or_default(),
            mailing_domain: self.mailing_domain.unwrap_or(serde_json::json!({})),
            use_exclusion_list: self.use_exclusion_list.unwrap_or(true),
            campaign_id: self.campaign_id,
            medium_id: self.medium_id,
            source_id: self.source_id,
            user_id: self.user_id,
            ab_testing_enabled: self.ab_testing_enabled.unwrap_or(false),
            ab_testing_pc: self.ab_testing_pc.unwrap_or(10),
            ab_test_id: self.ab_test_id,
            kpi_mail_required: self.kpi_mail_required.unwrap_or(false),
            metadata: AuditMetadata::default(),
        })
    }
}
