//! This repo's copy of the time adjustment contract with screentime-web.
//!
//! Unknown fields are ignored everywhere (no `deny_unknown_fields`), so either side can add
//! an optional field without a lockstep deploy. A missing required field is still an error.
//! The JSON in `examples/` is shared with screentime-web, and both repos test against it.

use serde::{Deserialize, Serialize};

use crate::observation::Observation;

/// Body of a request on `time.adjust.<host>.<user>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub request_id: String,
    pub adjuster: String,
    /// Positive adds to time left today, negative removes.
    pub delta_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Reply {
    Ok {
        left_day_before: i32,
        observation: Observation,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnknownUser,
    NotTracked,
    TimekprRejected,
    Internal,
}

/// Published on `time.adjusted.<host>.<user>` after a successful time adjustment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjustedEvent {
    pub request_id: String,
    pub adjuster: String,
    pub host: String,
    pub user: String,
    pub delta_seconds: i32,
    pub left_day_before: i32,
    pub left_day_after: i32,
    pub at: jiff::Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const REQUEST: &str = include_str!("examples/request.json");
    const REPLY_OK: &str = include_str!("examples/reply_ok.json");
    const REPLY_ERROR: &str = include_str!("examples/reply_error.json");
    const EVENT: &str = include_str!("examples/event.json");

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn request_example_parses() {
        let request: Request = serde_json::from_str(REQUEST).unwrap();
        assert_eq!(
            request,
            Request {
                request_id: "7d1f0c1e-3b5a-4c8e-9f2a-1b2c3d4e5f60".into(),
                adjuster: "me@example.com".into(),
                delta_seconds: 900,
            }
        );
    }

    #[test]
    fn request_ignores_unknown_fields() {
        let request: Request = serde_json::from_str(
            r#"{"request_id":"r","adjuster":"a","delta_seconds":-60,"reason":"homework"}"#,
        )
        .unwrap();
        assert_eq!(request.delta_seconds, -60);
    }

    #[test]
    fn request_rejects_missing_fields() {
        assert!(serde_json::from_str::<Request>(r#"{"request_id":"r","adjuster":"a"}"#).is_err());
    }

    #[test]
    fn ok_reply_example_round_trips() {
        let reply: Reply = serde_json::from_str(REPLY_OK).unwrap();
        assert_eq!(
            reply,
            Reply::Ok {
                left_day_before: 3600,
                observation: Observation {
                    left_day: 4500,
                    spent_balance: -900,
                    spent_month: 18000,
                    spent_week: 7200,
                    spent_day: 0,
                },
            }
        );
        assert_eq!(serde_json::to_value(&reply).unwrap(), json(REPLY_OK));
    }

    #[test]
    fn error_reply_example_round_trips() {
        let reply: Reply = serde_json::from_str(REPLY_ERROR).unwrap();
        assert!(matches!(
            reply,
            Reply::Error {
                code: ErrorCode::TimekprRejected,
                ..
            }
        ));
        assert_eq!(serde_json::to_value(&reply).unwrap(), json(REPLY_ERROR));
    }

    #[test]
    fn every_error_code_has_its_contract_name() {
        let codes = [
            (ErrorCode::InvalidRequest, "invalid_request"),
            (ErrorCode::UnknownUser, "unknown_user"),
            (ErrorCode::NotTracked, "not_tracked"),
            (ErrorCode::TimekprRejected, "timekpr_rejected"),
            (ErrorCode::Internal, "internal"),
        ];
        for (code, name) in codes {
            assert_eq!(serde_json::to_value(code).unwrap(), Value::from(name));
        }
    }

    #[test]
    fn event_example_round_trips() {
        let event: AdjustedEvent = serde_json::from_str(EVENT).unwrap();
        assert_eq!(
            event,
            AdjustedEvent {
                request_id: "7d1f0c1e-3b5a-4c8e-9f2a-1b2c3d4e5f60".into(),
                adjuster: "me@example.com".into(),
                host: "test-vm".into(),
                user: "junior".into(),
                delta_seconds: 900,
                left_day_before: 3600,
                left_day_after: 4500,
                at: "2026-11-09T12:00:00Z".parse().unwrap(),
            }
        );
        assert_eq!(serde_json::to_value(&event).unwrap(), json(EVENT));
    }
}
