use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "mailing_state", rename_all = "snake_case")]
pub enum MailingState {
    Draft,
    InQueue,
    Sending,
    Done,
}

impl std::fmt::Display for MailingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::InQueue => write!(f, "in_queue"),
            Self::Sending => write!(f, "sending"),
            Self::Done => write!(f, "done"),
        }
    }
}

impl FromStr for MailingState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "draft" => Ok(Self::Draft),
            "in_queue" => Ok(Self::InQueue),
            "sending" => Ok(Self::Sending),
            "done" => Ok(Self::Done),
            _ => Err(format!("Unknown MailingState variant: {}", s)),
        }
    }
}

impl Default for MailingState {
    fn default() -> Self {
        Self::Draft
    }
}
