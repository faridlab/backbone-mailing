use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "ab_winner_selection", rename_all = "snake_case")]
pub enum AbWinnerSelection {
    Manual,
    OpenedRatio,
    ClicksRatio,
    RepliedRatio,
    SaleInvoicedAmount,
}

impl std::fmt::Display for AbWinnerSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manual => write!(f, "manual"),
            Self::OpenedRatio => write!(f, "opened_ratio"),
            Self::ClicksRatio => write!(f, "clicks_ratio"),
            Self::RepliedRatio => write!(f, "replied_ratio"),
            Self::SaleInvoicedAmount => write!(f, "sale_invoiced_amount"),
        }
    }
}

impl FromStr for AbWinnerSelection {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "manual" => Ok(Self::Manual),
            "opened_ratio" => Ok(Self::OpenedRatio),
            "clicks_ratio" => Ok(Self::ClicksRatio),
            "replied_ratio" => Ok(Self::RepliedRatio),
            "sale_invoiced_amount" => Ok(Self::SaleInvoicedAmount),
            _ => Err(format!("Unknown AbWinnerSelection variant: {}", s)),
        }
    }
}

impl Default for AbWinnerSelection {
    fn default() -> Self {
        Self::OpenedRatio
    }
}
