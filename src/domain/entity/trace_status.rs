use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "trace_status", rename_all = "snake_case")]
pub enum TraceStatus {
    Outgoing,
    Process,
    Pending,
    Sent,
    Open,
    Reply,
    Bounce,
    Error,
    Cancel,
}

impl std::fmt::Display for TraceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Outgoing => write!(f, "outgoing"),
            Self::Process => write!(f, "process"),
            Self::Pending => write!(f, "pending"),
            Self::Sent => write!(f, "sent"),
            Self::Open => write!(f, "open"),
            Self::Reply => write!(f, "reply"),
            Self::Bounce => write!(f, "bounce"),
            Self::Error => write!(f, "error"),
            Self::Cancel => write!(f, "cancel"),
        }
    }
}

impl FromStr for TraceStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "outgoing" => Ok(Self::Outgoing),
            "process" => Ok(Self::Process),
            "pending" => Ok(Self::Pending),
            "sent" => Ok(Self::Sent),
            "open" => Ok(Self::Open),
            "reply" => Ok(Self::Reply),
            "bounce" => Ok(Self::Bounce),
            "error" => Ok(Self::Error),
            "cancel" => Ok(Self::Cancel),
            _ => Err(format!("Unknown TraceStatus variant: {}", s)),
        }
    }
}

impl Default for TraceStatus {
    fn default() -> Self {
        Self::Outgoing
    }
}
