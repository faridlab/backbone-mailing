use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "trace_failure_type", rename_all = "snake_case")]
pub enum TraceFailureType {
    Unknown,
    MailBounce,
    MailSpam,
    MailEmailInvalid,
    MailEmailMissing,
    MailFromInvalid,
    MailFromMissing,
    MailSmtp,
    MailBl,
    MailDup,
    MailOptout,
}

impl std::fmt::Display for TraceFailureType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown"),
            Self::MailBounce => write!(f, "mail_bounce"),
            Self::MailSpam => write!(f, "mail_spam"),
            Self::MailEmailInvalid => write!(f, "mail_email_invalid"),
            Self::MailEmailMissing => write!(f, "mail_email_missing"),
            Self::MailFromInvalid => write!(f, "mail_from_invalid"),
            Self::MailFromMissing => write!(f, "mail_from_missing"),
            Self::MailSmtp => write!(f, "mail_smtp"),
            Self::MailBl => write!(f, "mail_bl"),
            Self::MailDup => write!(f, "mail_dup"),
            Self::MailOptout => write!(f, "mail_optout"),
        }
    }
}

impl FromStr for TraceFailureType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "unknown" => Ok(Self::Unknown),
            "mail_bounce" => Ok(Self::MailBounce),
            "mail_spam" => Ok(Self::MailSpam),
            "mail_email_invalid" => Ok(Self::MailEmailInvalid),
            "mail_email_missing" => Ok(Self::MailEmailMissing),
            "mail_from_invalid" => Ok(Self::MailFromInvalid),
            "mail_from_missing" => Ok(Self::MailFromMissing),
            "mail_smtp" => Ok(Self::MailSmtp),
            "mail_bl" => Ok(Self::MailBl),
            "mail_dup" => Ok(Self::MailDup),
            "mail_optout" => Ok(Self::MailOptout),
            _ => Err(format!("Unknown TraceFailureType variant: {}", s)),
        }
    }
}

impl Default for TraceFailureType {
    fn default() -> Self {
        Self::Unknown
    }
}
