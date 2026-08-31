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
    CrmLead,
    CrmDeal,
    SellingCustomer,
}

impl std::fmt::Display for MailingTargetModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MailingContact => write!(f, "mailing_contact"),
            Self::Party => write!(f, "party"),
            Self::CrmLead => write!(f, "crm_lead"),
            Self::CrmDeal => write!(f, "crm_deal"),
            Self::SellingCustomer => write!(f, "selling_customer"),
        }
    }
}

impl FromStr for MailingTargetModel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mailing_contact" => Ok(Self::MailingContact),
            "party" => Ok(Self::Party),
            "crm_lead" => Ok(Self::CrmLead),
            "crm_deal" => Ok(Self::CrmDeal),
            "selling_customer" => Ok(Self::SellingCustomer),
            _ => Err(format!("Unknown MailingTargetModel variant: {}", s)),
        }
    }
}

impl Default for MailingTargetModel {
    fn default() -> Self {
        Self::MailingContact
    }
}
