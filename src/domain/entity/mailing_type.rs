use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "mailing_type", rename_all = "snake_case")]
pub enum MailingType {
    Mail,
    Sms,
}

impl std::fmt::Display for MailingType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mail => write!(f, "mail"),
            Self::Sms => write!(f, "sms"),
        }
    }
}

impl FromStr for MailingType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mail" => Ok(Self::Mail),
            "sms" => Ok(Self::Sms),
            _ => Err(format!("Unknown MailingType variant: {}", s)),
        }
    }
}

impl Default for MailingType {
    fn default() -> Self {
        Self::Mail
    }
}
