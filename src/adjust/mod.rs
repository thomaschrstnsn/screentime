//! Time adjustments served on `time.adjust.<host>.<user>`.
//!
//! The contract with screentime-web lives in [`contract`]; [`handle`] turns one request
//! into a reply and, on success, a `time.adjusted` event.

pub mod contract;

use self::contract::{AdjustedEvent, ErrorCode, Reply, Request};
use crate::timekpr::{Timekpr, TimekprError};

/// The largest time adjustment, either way, in one request: 8 h.
pub const MAX_DELTA_SECONDS: i32 = 8 * 60 * 60;

/// Handles one time adjustment request for `user`, the last token of the subject.
pub async fn handle(
    users: &[String],
    timekpr: &impl Timekpr,
    host: &str,
    user: &str,
    request: &[u8],
    now: jiff::Timestamp,
) -> (Reply, Option<AdjustedEvent>) {
    if !users.iter().any(|u| u == user) {
        return error(
            ErrorCode::UnknownUser,
            format!("{user} is not a monitored user on {host}"),
        );
    }
    let request: Request = match serde_json::from_slice(request) {
        Ok(request) => request,
        Err(e) => return error(ErrorCode::InvalidRequest, format!("invalid request: {e}")),
    };
    let delta_seconds = match i32::try_from(request.delta_seconds) {
        Ok(d) if d != 0 && d.abs() <= MAX_DELTA_SECONDS => d,
        _ => {
            return error(
                ErrorCode::InvalidRequest,
                format!(
                    "delta_seconds must be non-zero and at most {MAX_DELTA_SECONDS} either way, got {}",
                    request.delta_seconds
                ),
            );
        }
    };

    match timekpr.adjust(user, delta_seconds).await {
        Ok(adjusted) => {
            let event = AdjustedEvent {
                request_id: request.request_id,
                adjuster: request.adjuster,
                host: host.to_string(),
                user: user.to_string(),
                delta_seconds,
                left_day_before: adjusted.left_day_before,
                left_day_after: adjusted.observation.left_day,
                at: now,
            };
            let reply = Reply::Ok {
                left_day_before: adjusted.left_day_before,
                observation: adjusted.observation,
            };
            (reply, Some(event))
        }
        Err(e) => {
            let code = match e {
                TimekprError::NotTracked => ErrorCode::NotTracked,
                TimekprError::Rejected(_) => ErrorCode::TimekprRejected,
                TimekprError::Internal(_) => ErrorCode::Internal,
            };
            error(code, e.to_string())
        }
    }
}

fn error(code: ErrorCode, message: String) -> (Reply, Option<AdjustedEvent>) {
    (Reply::Error { code, message }, None)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::observation::Observation;
    use crate::timekpr::Adjusted;

    /// An in-memory Timekpr that records adjustments and answers with a canned result.
    struct FakeTimekpr {
        result: Result<Adjusted, TimekprError>,
        calls: Mutex<Vec<(String, i32)>>,
    }

    impl FakeTimekpr {
        fn answering(result: Result<Adjusted, TimekprError>) -> Self {
            Self {
                result,
                calls: Mutex::new(vec![]),
            }
        }

        fn calls(&self) -> Vec<(String, i32)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Timekpr for FakeTimekpr {
        async fn observe(&self, _user: &str) -> Result<Observation, TimekprError> {
            unimplemented!("handle never observes on its own")
        }

        async fn adjust(&self, user: &str, delta_seconds: i32) -> Result<Adjusted, TimekprError> {
            self.calls
                .lock()
                .unwrap()
                .push((user.into(), delta_seconds));
            self.result.clone()
        }
    }

    fn observation(left_day: i32) -> Observation {
        Observation {
            left_day,
            spent_balance: -900,
            spent_month: 18000,
            spent_week: 7200,
            spent_day: 0,
        }
    }

    fn succeeding() -> FakeTimekpr {
        FakeTimekpr::answering(Ok(Adjusted {
            left_day_before: 3600,
            observation: observation(4500),
        }))
    }

    fn noon() -> jiff::Timestamp {
        "2026-11-09T12:00:00Z".parse().unwrap()
    }

    async fn run(
        timekpr: &FakeTimekpr,
        user: &str,
        request: &str,
    ) -> (Reply, Option<AdjustedEvent>) {
        let users = vec!["junior".to_string(), "senior".to_string()];
        handle(&users, timekpr, "test-vm", user, request.as_bytes(), noon()).await
    }

    fn error_code(reply: &Reply) -> ErrorCode {
        match reply {
            Reply::Error { code, .. } => *code,
            Reply::Ok { .. } => panic!("expected an error reply, got {reply:?}"),
        }
    }

    const VALID: &str = r#"{"request_id":"r-1","adjuster":"me@example.com","delta_seconds":900}"#;

    #[tokio::test]
    async fn user_not_in_users_is_unknown_without_asking_timekpr() {
        let timekpr = succeeding();

        let (reply, event) = run(&timekpr, "ghost", VALID).await;

        assert_eq!(error_code(&reply), ErrorCode::UnknownUser);
        assert_eq!(event, None);
        assert_eq!(timekpr.calls(), vec![]);
    }

    async fn assert_invalid(request: &str) {
        let timekpr = succeeding();

        let (reply, event) = run(&timekpr, "junior", request).await;

        assert_eq!(
            error_code(&reply),
            ErrorCode::InvalidRequest,
            "for {request}"
        );
        assert_eq!(event, None);
        assert_eq!(timekpr.calls(), vec![], "for {request}");
    }

    #[tokio::test]
    async fn malformed_json_is_invalid() {
        assert_invalid("not json").await;
    }

    #[tokio::test]
    async fn missing_field_is_invalid() {
        assert_invalid(r#"{"request_id":"r-1","delta_seconds":900}"#).await;
    }

    #[tokio::test]
    async fn zero_delta_is_invalid() {
        assert_invalid(r#"{"request_id":"r-1","adjuster":"a","delta_seconds":0}"#).await;
    }

    #[tokio::test]
    async fn more_than_eight_hours_either_way_is_invalid() {
        assert_invalid(r#"{"request_id":"r-1","adjuster":"a","delta_seconds":28801}"#).await;
        assert_invalid(r#"{"request_id":"r-1","adjuster":"a","delta_seconds":-28801}"#).await;
        assert_invalid(r#"{"request_id":"r-1","adjuster":"a","delta_seconds":9999999999}"#).await;
    }

    #[tokio::test]
    async fn success_replies_with_fresh_observation_and_emits_event() {
        let timekpr = succeeding();

        let (reply, event) = run(&timekpr, "junior", VALID).await;

        assert_eq!(timekpr.calls(), vec![("junior".into(), 900)]);
        assert_eq!(
            reply,
            Reply::Ok {
                left_day_before: 3600,
                observation: observation(4500),
            }
        );
        assert_eq!(
            event,
            Some(AdjustedEvent {
                request_id: "r-1".into(),
                adjuster: "me@example.com".into(),
                host: "test-vm".into(),
                user: "junior".into(),
                delta_seconds: 900,
                left_day_before: 3600,
                left_day_after: 4500,
                at: noon(),
            })
        );
    }

    async fn reply_when_timekpr_fails(error: TimekprError) -> Reply {
        let timekpr = FakeTimekpr::answering(Err(error));
        let (reply, event) = run(&timekpr, "junior", VALID).await;
        assert_eq!(event, None);
        reply
    }

    #[tokio::test]
    async fn timekpr_not_tracking_the_user_is_not_tracked() {
        let reply = reply_when_timekpr_fails(TimekprError::NotTracked).await;
        assert_eq!(error_code(&reply), ErrorCode::NotTracked);
    }

    #[tokio::test]
    async fn timekpr_rejection_carries_its_message() {
        let reply =
            reply_when_timekpr_fails(TimekprError::Rejected("can be one of these: - + =".into()))
                .await;
        let Reply::Error { code, message } = reply else {
            panic!("expected an error reply");
        };
        assert_eq!(code, ErrorCode::TimekprRejected);
        assert!(message.contains("can be one of these: - + ="), "{message}");
    }

    #[tokio::test]
    async fn other_timekpr_failures_are_internal() {
        let reply = reply_when_timekpr_fails(TimekprError::Internal("bus gone".into())).await;
        assert_eq!(error_code(&reply), ErrorCode::Internal);
    }

    #[tokio::test]
    async fn exactly_eight_hours_either_way_is_passed_on() {
        let timekpr = succeeding();
        run(
            &timekpr,
            "junior",
            r#"{"request_id":"r","adjuster":"a","delta_seconds":28800}"#,
        )
        .await;
        run(
            &timekpr,
            "junior",
            r#"{"request_id":"r","adjuster":"a","delta_seconds":-28800}"#,
        )
        .await;

        assert_eq!(
            timekpr.calls(),
            vec![("junior".into(), 28800), ("junior".into(), -28800)]
        );
    }
}
