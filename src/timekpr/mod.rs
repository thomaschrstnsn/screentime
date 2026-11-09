//! Timekpr-nExT, seen as the two things the collector does with it: observe a monitored
//! user and make a time adjustment.
//!
//! [`ZbusTimekpr`] is the real adapter over D-Bus; tests use an in-memory fake.

mod zbus;

use std::fmt;
use std::future::Future;

use crate::observation::Observation;

pub use self::zbus::ZbusTimekpr;

pub trait Timekpr {
    fn observe(&self, user: &str)
    -> impl Future<Output = Result<Observation, TimekprError>> + Send;

    /// Adds (`delta_seconds > 0`) to or removes from time left today, and reports the
    /// time left before and a fresh observation after.
    fn adjust(
        &self,
        user: &str,
        delta_seconds: i32,
    ) -> impl Future<Output = Result<Adjusted, TimekprError>> + Send;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adjusted {
    pub left_day_before: i32,
    pub observation: Observation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimekprError {
    /// Timekpr has no config or control file for the user (return code 1).
    NotTracked,
    /// Timekpr refused or failed to compute the change (return code 28), with its message.
    Rejected(String),
    /// Anything else: D-Bus errors, other return codes, unparseable replies.
    Internal(String),
}

impl TimekprError {
    /// A short name for logs, the same for every instance of a kind.
    pub fn kind(&self) -> &'static str {
        match self {
            TimekprError::NotTracked => "not_tracked",
            TimekprError::Rejected(_) => "rejected",
            TimekprError::Internal(_) => "internal",
        }
    }
}

impl fmt::Display for TimekprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimekprError::NotTracked => write!(f, "user is not tracked by Timekpr"),
            TimekprError::Rejected(msg) => write!(f, "Timekpr rejected the call: {msg}"),
            TimekprError::Internal(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TimekprError {}
