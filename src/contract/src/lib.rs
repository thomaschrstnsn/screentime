//! The NATS contract between the collector and web: subjects, stream names and message bodies.
//!
//! Unknown fields are ignored everywhere (no `deny_unknown_fields`), so either side can add
//! an optional field without a lockstep deploy. A missing required field is still an error.
//! [`examples`] holds a JSON example of each message, which the tests here parse.

use serde::{Deserialize, Serialize};

/// NATS subjects, each `time.<kind>.<host>.<user>`. Pass `*` for a wildcard token.
pub mod subject {
    /// Where the collector publishes each [`Observation`](crate::Observation).
    pub fn observation(host: &str, user: &str) -> String {
        format!("time.obs.{host}.{user}")
    }

    /// Where web sends a time adjustment [`Request`](crate::Request).
    pub fn adjust(host: &str, user: &str) -> String {
        format!("time.adjust.{host}.{user}")
    }

    /// Where the collector publishes each [`AdjustedEvent`](crate::AdjustedEvent).
    pub fn adjusted(host: &str, user: &str) -> String {
        format!("time.adjusted.{host}.{user}")
    }
}

/// The JetStream stream holding the latest observation of every monitored user. The
/// collector creates it; web reads it.
pub const OBSERVATIONS_STREAM: &str = "OBSERVATIONS";

/// The JetStream stream recording `time.adjusted` events, created by the collector.
pub const ADJUSTMENTS_STREAM: &str = "ADJUSTMENTS";

/// The largest time adjustment, either way, in one [`Request`]: 8 h.
pub const MAX_DELTA_SECONDS: i32 = 8 * 60 * 60;

/// The JSON example of each message, as sent on the wire.
pub mod examples {
    /// A [`Request`](crate::Request) from web.
    pub const REQUEST: &str = include_str!("../examples/request.json");
    /// A [`Reply::Ok`](crate::Reply::Ok).
    pub const REPLY_OK: &str = include_str!("../examples/reply_ok.json");
    /// A [`Reply::Error`](crate::Reply::Error).
    pub const REPLY_ERROR: &str = include_str!("../examples/reply_error.json");
    /// An [`AdjustedEvent`](crate::AdjustedEvent).
    pub const EVENT: &str = include_str!("../examples/event.json");
}

/// A snapshot of a monitored user's time figures, as published on `time.obs.<host>.<user>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub left_day: i32,
    pub spent_balance: i32,
    pub spent_month: i32,
    pub spent_week: i32,
    pub spent_day: i32,
}

/// Body of a request on `time.adjust.<host>.<user>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub request_id: String,
    pub adjuster: String,
    /// Positive adds to time left today, negative removes.
    pub delta_seconds: i64,
}

/// The collector's reply to a [`Request`].
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
    /// A code this build doesn't know, so a collector can add codes before web learns them.
    /// Only for reading: serializing it is an error, so the collector can't send it.
    #[serde(other, skip_serializing)]
    Other,
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
    use super::examples::*;
    use super::*;
    use serde_json::Value;

    fn json(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn subjects_name_the_host_then_the_user() {
        assert_eq!(
            subject::observation("test-vm", "junior"),
            "time.obs.test-vm.junior"
        );
        assert_eq!(
            subject::adjust("test-vm", "junior"),
            "time.adjust.test-vm.junior"
        );
        assert_eq!(
            subject::adjusted("test-vm", "junior"),
            "time.adjusted.test-vm.junior"
        );
    }

    #[test]
    fn observation_serializes_with_snake_case_keys() {
        let observation = Observation {
            left_day: 3600,
            spent_balance: 0,
            spent_month: 18000,
            spent_week: 7200,
            spent_day: 3600,
        };
        assert_eq!(
            serde_json::to_value(&observation).unwrap(),
            serde_json::json!({
                "left_day": 3600,
                "spent_balance": 0,
                "spent_month": 18000,
                "spent_week": 7200,
                "spent_day": 3600,
            })
        );
    }

    #[test]
    fn request_example_round_trips() {
        let request: Request = serde_json::from_str(REQUEST).unwrap();
        assert_eq!(
            request,
            Request {
                request_id: "7d1f0c1e-3b5a-4c8e-9f2a-1b2c3d4e5f60".into(),
                adjuster: "me@example.com".into(),
                delta_seconds: 900,
            }
        );
        assert_eq!(serde_json::to_value(&request).unwrap(), json(REQUEST));
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
        assert_eq!(
            reply,
            Reply::Error {
                code: ErrorCode::TimekprRejected,
                message: "Timekpr rejected the call: operation can be one of these: - + =".into(),
            }
        );
        assert_eq!(serde_json::to_value(&reply).unwrap(), json(REPLY_ERROR));
    }

    #[test]
    fn reply_ignores_unknown_fields() {
        let reply: Reply = serde_json::from_str(
            r#"{"status":"error","code":"internal","message":"boom","hint":"retry"}"#,
        )
        .unwrap();
        assert!(matches!(reply, Reply::Error { .. }));
    }

    #[test]
    fn reply_rejects_missing_fields() {
        assert!(serde_json::from_str::<Reply>(r#"{"status":"ok","left_day_before":1}"#).is_err());
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
    fn an_error_code_added_later_parses_as_other() {
        let reply: Reply = serde_json::from_str(
            r#"{"status":"error","code":"rate_limited","message":"slow down"}"#,
        )
        .unwrap();
        assert_eq!(
            reply,
            Reply::Error {
                code: ErrorCode::Other,
                message: "slow down".into(),
            }
        );
    }

    #[test]
    fn other_is_never_sent() {
        assert!(serde_json::to_value(ErrorCode::Other).is_err());
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
