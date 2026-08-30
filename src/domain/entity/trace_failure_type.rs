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
    SmsBlacklist,
    SmsNumberMissing,
    SmsNumberFormat,
    SmsCountryNotSupported,
    SmsRegistrationNeeded,
    SmsCredit,
    SmsServer,
    SmsAcc,
    SmsDuplicate,
    SmsOptout,
    SmsExpired,
    SmsInvalidDestination,
    SmsNotAllowed,
    SmsNotDelivered,
    SmsRejected,
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
            Self::SmsBlacklist => write!(f, "sms_blacklist"),
            Self::SmsNumberMissing => write!(f, "sms_number_missing"),
            Self::SmsNumberFormat => write!(f, "sms_number_format"),
            Self::SmsCountryNotSupported => write!(f, "sms_country_not_supported"),
            Self::SmsRegistrationNeeded => write!(f, "sms_registration_needed"),
            Self::SmsCredit => write!(f, "sms_credit"),
            Self::SmsServer => write!(f, "sms_server"),
            Self::SmsAcc => write!(f, "sms_acc"),
            Self::SmsDuplicate => write!(f, "sms_duplicate"),
            Self::SmsOptout => write!(f, "sms_optout"),
            Self::SmsExpired => write!(f, "sms_expired"),
            Self::SmsInvalidDestination => write!(f, "sms_invalid_destination"),
            Self::SmsNotAllowed => write!(f, "sms_not_allowed"),
            Self::SmsNotDelivered => write!(f, "sms_not_delivered"),
            Self::SmsRejected => write!(f, "sms_rejected"),
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
            "sms_blacklist" => Ok(Self::SmsBlacklist),
            "sms_number_missing" => Ok(Self::SmsNumberMissing),
            "sms_number_format" => Ok(Self::SmsNumberFormat),
            "sms_country_not_supported" => Ok(Self::SmsCountryNotSupported),
            "sms_registration_needed" => Ok(Self::SmsRegistrationNeeded),
            "sms_credit" => Ok(Self::SmsCredit),
            "sms_server" => Ok(Self::SmsServer),
            "sms_acc" => Ok(Self::SmsAcc),
            "sms_duplicate" => Ok(Self::SmsDuplicate),
            "sms_optout" => Ok(Self::SmsOptout),
            "sms_expired" => Ok(Self::SmsExpired),
            "sms_invalid_destination" => Ok(Self::SmsInvalidDestination),
            "sms_not_allowed" => Ok(Self::SmsNotAllowed),
            "sms_not_delivered" => Ok(Self::SmsNotDelivered),
            "sms_rejected" => Ok(Self::SmsRejected),
            _ => Err(format!("Unknown TraceFailureType variant: {}", s)),
        }
    }
}

impl Default for TraceFailureType {
    fn default() -> Self {
        Self::Unknown
    }
}
