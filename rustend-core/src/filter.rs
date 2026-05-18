use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CreatedAtFilter {
    Gt(DateTime<Utc>),
    Gte(DateTime<Utc>),
    Lt(DateTime<Utc>),
    Lte(DateTime<Utc>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterCondition {
    pub path:     String,
    pub operator: FilterOperator,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FilterOperator {
    Exists,
    IsNull,
    Eq(Value),
    Ne(Value),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    Contains(Value),
    StartsWith(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_operator_roundtrip() {
        let op = FilterOperator::Gt(serde_json::json!("2024-01-01"));
        let json = serde_json::to_string(&op).unwrap();
        let back: FilterOperator = serde_json::from_str(&json).unwrap();
        assert_eq!(op, back);
    }

    #[test]
    fn created_at_filter_roundtrip() {
        let f = CreatedAtFilter::Gte(
            DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );
        let json = serde_json::to_string(&f).unwrap();
        let back: CreatedAtFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }
}
