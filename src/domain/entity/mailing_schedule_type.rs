use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "mailing_schedule_type", rename_all = "snake_case")]
pub enum MailingScheduleType {
    Immediate,
    Scheduled,
}

impl std::fmt::Display for MailingScheduleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Immediate => write!(f, "immediate"),
            Self::Scheduled => write!(f, "scheduled"),
        }
    }
}

impl FromStr for MailingScheduleType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "immediate" => Ok(Self::Immediate),
            "scheduled" => Ok(Self::Scheduled),
            _ => Err(format!("Unknown MailingScheduleType variant: {}", s)),
        }
    }
}

impl Default for MailingScheduleType {
    fn default() -> Self {
        Self::Immediate
    }
}
