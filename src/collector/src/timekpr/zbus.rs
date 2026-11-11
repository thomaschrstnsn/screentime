use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

use super::{Adjusted, Timekpr, TimekprError};
use crate::dbus::timekpr::user::admin::AdminProxy;
use crate::observation::Observation;

/// [`Timekpr`] over the `com.timekpr.server.user.admin` D-Bus interface.
///
/// Cheap to clone: clones share the underlying bus connection.
#[derive(Clone)]
pub struct ZbusTimekpr {
    proxy: AdminProxy<'static>,
}

impl ZbusTimekpr {
    pub async fn new(connection: &zbus::Connection) -> zbus::Result<Self> {
        let proxy =
            AdminProxy::new(connection, "com.timekpr.server", "/com/timekpr/server").await?;
        Ok(Self { proxy })
    }
}

impl Timekpr for ZbusTimekpr {
    async fn observe(&self, user: &str) -> Result<Observation, TimekprError> {
        let (code, message, properties) = self
            .proxy
            .get_user_information(user, "F")
            .await
            .map_err(dbus_error)?;
        check(code, message, GET_USER_INFORMATION_NOT_TRACKED)?;
        parse_observation(&properties)
    }

    async fn adjust(&self, user: &str, delta_seconds: i32) -> Result<Adjusted, TimekprError> {
        // Refresh first: it rolls a stale control file over to today, which would
        // otherwise wipe the adjustment on the next read.
        let before = self.observe(user).await?;

        let operation = if delta_seconds < 0 { "-" } else { "+" };
        let (code, message) = self
            .proxy
            .set_time_left(user, operation, delta_seconds.unsigned_abs() as i32)
            .await
            .map_err(dbus_error)?;
        check(code, message, SET_TIME_LEFT_NOT_TRACKED)?;

        let observation = self.observe(user).await?;
        Ok(Adjusted {
            left_day_before: before.left_day,
            observation,
        })
    }
}

fn dbus_error(e: zbus::Error) -> TimekprError {
    TimekprError::Internal(format!("D-Bus call failed: {e}"))
}

/// The code each method returns when the user's config or control file is missing (the
/// user was never tracked). Timekpr v0.5.10: getUserInformation passes on the config
/// loader's -1 (`configprocessor.py` `loadAndCheckUser*`), setTimeLeft returns 1. Both
/// also use that code for an unexpected exception, which can't be told apart.
const GET_USER_INFORMATION_NOT_TRACKED: i32 = -1;
const SET_TIME_LEFT_NOT_TRACKED: i32 = 1;
/// An invalid operation or value, or a failure computing or setting it.
const REJECTED: i32 = 28;

/// Maps Timekpr's `(code, message)` result onto an error.
fn check(code: i32, message: String, not_tracked: i32) -> Result<(), TimekprError> {
    match code {
        0 => Ok(()),
        c if c == not_tracked => Err(TimekprError::NotTracked),
        REJECTED => Err(TimekprError::Rejected(message)),
        _ => Err(TimekprError::Internal(format!(
            "Timekpr returned code {code}: {message}"
        ))),
    }
}

fn parse_observation(
    properties: &HashMap<String, OwnedValue>,
) -> Result<Observation, TimekprError> {
    let get = |key: &str| -> Result<i32, TimekprError> {
        let value = properties.get(key).ok_or_else(|| {
            TimekprError::Internal(format!("missing key {key} in user information"))
        })?;
        value
            .downcast_ref()
            .map_err(|e| TimekprError::Internal(format!("bad value for {key}: {e}")))
    };

    Ok(Observation {
        left_day: get("TIME_LEFT_DAY")?,
        spent_balance: get("TIME_SPENT_BALANCE")?,
        spent_month: get("TIME_SPENT_MONTH")?,
        spent_week: get("TIME_SPENT_WEEK")?,
        spent_day: get("TIME_SPENT_DAY")?,
    })
}
