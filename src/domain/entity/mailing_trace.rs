use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use super::TraceType;
use super::TraceStatus;
use super::TraceFailureType;
use super::AuditMetadata;

use crate::domain::state_machine::{trace_statusStateMachine, trace_statusState, StateMachineError};

/// Strongly-typed ID for MailingTrace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailingTraceId(pub Uuid);

impl MailingTraceId {
    pub fn new(id: Uuid) -> Self { Self(id) }
    pub fn generate() -> Self { Self(Uuid::new_v4()) }
    pub fn into_inner(self) -> Uuid { self.0 }
}

impl std::fmt::Display for MailingTraceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MailingTraceId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

impl From<Uuid> for MailingTraceId {
    fn from(id: Uuid) -> Self { Self(id) }
}

impl From<MailingTraceId> for Uuid {
    fn from(id: MailingTraceId) -> Self { id.0 }
}

impl AsRef<Uuid> for MailingTraceId {
    fn as_ref(&self) -> &Uuid { &self.0 }
}

impl std::ops::Deref for MailingTraceId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MailingTrace {
    pub id: Uuid,
    pub trace_type: TraceType,
    pub is_test_trace: bool,
    pub mailing_id: Uuid,
    pub campaign_id: Option<Uuid>,
    pub recipient_model: String,
    pub recipient_id: Uuid,
    pub recipient_email: String,
    pub mail_id: Option<Uuid>,
    pub message_id: Option<String>,
    pub(crate) trace_status: TraceStatus,
    pub failure_type: Option<TraceFailureType>,
    pub failure_reason: Option<String>,
    pub sent_datetime: Option<DateTime<Utc>>,
    pub open_datetime: Option<DateTime<Utc>>,
    pub reply_datetime: Option<DateTime<Utc>>,
    pub links_click_datetime: Option<DateTime<Utc>>,
    #[serde(default)]
    #[sqlx(json)]
    pub metadata: AuditMetadata,
}

impl MailingTrace {
    /// Create a builder for MailingTrace
    pub fn builder() -> MailingTraceBuilder {
        <MailingTraceBuilder as Default>::default()
    }

    /// Create a new MailingTrace with required fields
    pub fn new(trace_type: TraceType, is_test_trace: bool, mailing_id: Uuid, recipient_model: String, recipient_id: Uuid, recipient_email: String, trace_status: TraceStatus) -> Self {
        Self {
            id: Uuid::new_v4(),
            trace_type,
            is_test_trace,
            mailing_id,
            campaign_id: None,
            recipient_model,
            recipient_id,
            recipient_email,
            mail_id: None,
            message_id: None,
            trace_status,
            failure_type: None,
            failure_reason: None,
            sent_datetime: None,
            open_datetime: None,
            reply_datetime: None,
            links_click_datetime: None,
            metadata: AuditMetadata::default(),
        }
    }

    /// Get the entity's unique identifier
    pub fn id(&self) -> &Uuid {
        &self.id
    }

    /// Get a strongly-typed ID for this entity
    pub fn typed_id(&self) -> MailingTraceId {
        MailingTraceId(self.id)
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

    /// Set the campaign_id field (chainable)
    pub fn with_campaign_id(mut self, value: Uuid) -> Self {
        self.campaign_id = Some(value);
        self
    }

    /// Set the mail_id field (chainable)
    pub fn with_mail_id(mut self, value: Uuid) -> Self {
        self.mail_id = Some(value);
        self
    }

    /// Set the message_id field (chainable)
    pub fn with_message_id(mut self, value: String) -> Self {
        self.message_id = Some(value);
        self
    }

    /// Set the failure_type field (chainable)
    pub fn with_failure_type(mut self, value: TraceFailureType) -> Self {
        self.failure_type = Some(value);
        self
    }

    /// Set the failure_reason field (chainable)
    pub fn with_failure_reason(mut self, value: String) -> Self {
        self.failure_reason = Some(value);
        self
    }

    /// Set the sent_datetime field (chainable)
    pub fn with_sent_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.sent_datetime = Some(value);
        self
    }

    /// Set the open_datetime field (chainable)
    pub fn with_open_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.open_datetime = Some(value);
        self
    }

    /// Set the reply_datetime field (chainable)
    pub fn with_reply_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.reply_datetime = Some(value);
        self
    }

    /// Set the links_click_datetime field (chainable)
    pub fn with_links_click_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.links_click_datetime = Some(value);
        self
    }

    // ==========================================================
    // State Machine
    // ==========================================================

    /// Transition to a new state via the trace_status state machine.
    ///
    /// Returns `Err` if the transition is not permitted from the current state.
    /// Use this method instead of assigning `self.trace_status` directly.
    pub fn transition_to(&mut self, new_state: trace_statusState) -> Result<(), StateMachineError> {
        let current = self.trace_status.to_string().parse::<trace_statusState>()?;
        let mut sm = trace_statusStateMachine::from_state(current);
        sm.transition_to_state(new_state)?;
        self.trace_status = new_state.to_string().parse::<TraceStatus>()
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
                "trace_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.trace_type = v; }
                }
                "is_test_trace" => {
                    if let Ok(v) = serde_json::from_value(value) { self.is_test_trace = v; }
                }
                "mailing_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.mailing_id = v; }
                }
                "campaign_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.campaign_id = v; }
                }
                "recipient_model" => {
                    if let Ok(v) = serde_json::from_value(value) { self.recipient_model = v; }
                }
                "recipient_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.recipient_id = v; }
                }
                "recipient_email" => {
                    if let Ok(v) = serde_json::from_value(value) { self.recipient_email = v; }
                }
                "mail_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.mail_id = v; }
                }
                "message_id" => {
                    if let Ok(v) = serde_json::from_value(value) { self.message_id = v; }
                }
                "failure_type" => {
                    if let Ok(v) = serde_json::from_value(value) { self.failure_type = v; }
                }
                "failure_reason" => {
                    if let Ok(v) = serde_json::from_value(value) { self.failure_reason = v; }
                }
                "sent_datetime" => {
                    if let Ok(v) = serde_json::from_value(value) { self.sent_datetime = v; }
                }
                "open_datetime" => {
                    if let Ok(v) = serde_json::from_value(value) { self.open_datetime = v; }
                }
                "reply_datetime" => {
                    if let Ok(v) = serde_json::from_value(value) { self.reply_datetime = v; }
                }
                "links_click_datetime" => {
                    if let Ok(v) = serde_json::from_value(value) { self.links_click_datetime = v; }
                }
                _ => {} // ignore unknown fields
            }
        }
    }

    // <<< CUSTOM METHODS START >>>
    // <<< CUSTOM METHODS END >>>
}

impl super::Entity for MailingTrace {
    type Id = Uuid;

    fn entity_id(&self) -> &Self::Id {
        &self.id
    }

    fn entity_type() -> &'static str {
        "MailingTrace"
    }
}

impl backbone_core::PersistentEntity for MailingTrace {
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

impl backbone_orm::EntityRepoMeta for MailingTrace {
    fn column_types() -> std::collections::HashMap<String, String> {
        let mut m = std::collections::HashMap::new();
        m.insert("id".to_string(), "uuid".to_string());
        m.insert("mailing_id".to_string(), "uuid".to_string());
        m.insert("campaign_id".to_string(), "uuid".to_string());
        m.insert("recipient_id".to_string(), "uuid".to_string());
        m.insert("mail_id".to_string(), "uuid".to_string());
        m.insert("trace_type".to_string(), "trace_type".to_string());
        m.insert("trace_status".to_string(), "trace_status".to_string());
        m.insert("failure_type".to_string(), "trace_failure_type".to_string());
        m
    }
    fn search_fields() -> &'static [&'static str] {
        &["recipient_model", "recipient_email"]
    }
}

/// Builder for MailingTrace entity
///
/// Provides a fluent API for constructing MailingTrace instances.
/// System fields (id, metadata, timestamps) are auto-initialized.
#[derive(Debug, Clone, Default)]
pub struct MailingTraceBuilder {
    trace_type: Option<TraceType>,
    is_test_trace: Option<bool>,
    mailing_id: Option<Uuid>,
    campaign_id: Option<Uuid>,
    recipient_model: Option<String>,
    recipient_id: Option<Uuid>,
    recipient_email: Option<String>,
    mail_id: Option<Uuid>,
    message_id: Option<String>,
    trace_status: Option<TraceStatus>,
    failure_type: Option<TraceFailureType>,
    failure_reason: Option<String>,
    sent_datetime: Option<DateTime<Utc>>,
    open_datetime: Option<DateTime<Utc>>,
    reply_datetime: Option<DateTime<Utc>>,
    links_click_datetime: Option<DateTime<Utc>>,
}

impl MailingTraceBuilder {
    /// Set the trace_type field (default: `TraceType::default()`)
    pub fn trace_type(mut self, value: TraceType) -> Self {
        self.trace_type = Some(value);
        self
    }

    /// Set the is_test_trace field (default: `false`)
    pub fn is_test_trace(mut self, value: bool) -> Self {
        self.is_test_trace = Some(value);
        self
    }

    /// Set the mailing_id field (required)
    pub fn mailing_id(mut self, value: Uuid) -> Self {
        self.mailing_id = Some(value);
        self
    }

    /// Set the campaign_id field (optional)
    pub fn campaign_id(mut self, value: Uuid) -> Self {
        self.campaign_id = Some(value);
        self
    }

    /// Set the recipient_model field (required)
    pub fn recipient_model(mut self, value: String) -> Self {
        self.recipient_model = Some(value);
        self
    }

    /// Set the recipient_id field (required)
    pub fn recipient_id(mut self, value: Uuid) -> Self {
        self.recipient_id = Some(value);
        self
    }

    /// Set the recipient_email field (required)
    pub fn recipient_email(mut self, value: String) -> Self {
        self.recipient_email = Some(value);
        self
    }

    /// Set the mail_id field (optional)
    pub fn mail_id(mut self, value: Uuid) -> Self {
        self.mail_id = Some(value);
        self
    }

    /// Set the message_id field (optional)
    pub fn message_id(mut self, value: String) -> Self {
        self.message_id = Some(value);
        self
    }

    /// Set the trace_status field (default: `TraceStatus::default()`)
    pub fn trace_status(mut self, value: TraceStatus) -> Self {
        self.trace_status = Some(value);
        self
    }

    /// Set the failure_type field (optional)
    pub fn failure_type(mut self, value: TraceFailureType) -> Self {
        self.failure_type = Some(value);
        self
    }

    /// Set the failure_reason field (optional)
    pub fn failure_reason(mut self, value: String) -> Self {
        self.failure_reason = Some(value);
        self
    }

    /// Set the sent_datetime field (optional)
    pub fn sent_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.sent_datetime = Some(value);
        self
    }

    /// Set the open_datetime field (optional)
    pub fn open_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.open_datetime = Some(value);
        self
    }

    /// Set the reply_datetime field (optional)
    pub fn reply_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.reply_datetime = Some(value);
        self
    }

    /// Set the links_click_datetime field (optional)
    pub fn links_click_datetime(mut self, value: DateTime<Utc>) -> Self {
        self.links_click_datetime = Some(value);
        self
    }

    /// Build the MailingTrace entity
    ///
    /// Returns Err if any required field without a default is missing.
    pub fn build(self) -> Result<MailingTrace, String> {
        let mailing_id = self.mailing_id.ok_or_else(|| "mailing_id is required".to_string())?;
        let recipient_model = self.recipient_model.ok_or_else(|| "recipient_model is required".to_string())?;
        let recipient_id = self.recipient_id.ok_or_else(|| "recipient_id is required".to_string())?;
        let recipient_email = self.recipient_email.ok_or_else(|| "recipient_email is required".to_string())?;

        Ok(MailingTrace {
            id: Uuid::new_v4(),
            trace_type: self.trace_type.unwrap_or_default(),
            is_test_trace: self.is_test_trace.unwrap_or(false),
            mailing_id,
            campaign_id: self.campaign_id,
            recipient_model,
            recipient_id,
            recipient_email,
            mail_id: self.mail_id,
            message_id: self.message_id,
            trace_status: self.trace_status.unwrap_or_default(),
            failure_type: self.failure_type,
            failure_reason: self.failure_reason,
            sent_datetime: self.sent_datetime,
            open_datetime: self.open_datetime,
            reply_datetime: self.reply_datetime,
            links_click_datetime: self.links_click_datetime,
            metadata: AuditMetadata::default(),
        })
    }
}
