use serde::{Deserialize, Serialize};

/// A snapshot of a monitored user's time figures, as published on `time.obs.<host>.<user>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub left_day: i32,
    pub spent_balance: i32,
    pub spent_month: i32,
    pub spent_week: i32,
    pub spent_day: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_with_lowercase_keys() {
        let observation = Observation {
            left_day: 3600,
            spent_balance: 0,
            spent_month: 18000,
            spent_week: 7200,
            spent_day: 3600,
        };

        assert_eq!(
            serde_json::to_value(&observation).unwrap(),
            json!({
                "left_day": 3600,
                "spent_balance": 0,
                "spent_month": 18000,
                "spent_week": 7200,
                "spent_day": 3600,
            })
        );
    }
}
