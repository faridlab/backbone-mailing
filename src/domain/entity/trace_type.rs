use serde::{Deserialize, Serialize};
use sqlx::Type;
use std::str::FromStr;
#[cfg(feature = "openapi")]
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "openapi", derive(ToSchema))]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "trace_type", rename_all = "snake_case")]
pub enum TraceType {
    Mail,
}

impl std::fmt::Display for TraceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mail => write!(f, "mail"),
        }
    }
}

impl FromStr for TraceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mail" => Ok(Self::Mail),
            _ => Err(format!("Unknown TraceType variant: {}", s)),
        }
    }
}

impl Default for TraceType {
    fn default() -> Self {
        Self::Mail
    }
}
