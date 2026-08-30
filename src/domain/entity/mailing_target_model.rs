use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "mailing_target_model", rename_all = "snake_case")]
pub enum MailingTargetModel {
    MailingContact,
    Party,
}

impl std::fmt::Display for MailingTargetModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MailingContact => write!(f, "mailing_contact"),
            Self::Party => write!(f, "party"),
        }
    }
}

impl FromStr for MailingTargetModel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mailing_contact" => Ok(Self::MailingContact),
            "party" => Ok(Self::Party),
            _ => Err(format!("Unknown MailingTargetModel variant: {}", s)),
        }
    }
}

impl Default for MailingTargetModel {
    fn default() -> Self {
        Self::MailingContact
    }
}
