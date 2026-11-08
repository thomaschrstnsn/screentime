# What Timekpr-nExT's `setTimeLeft` does

Research for issue #3 (child of #1, "time adjustments from the web UI").

**Version examined:** Timekpr-nExT **0.5.10**. That is the version nixpkgs packages
at nixpkgs rev `55bfb4d7` (`pkgs/by-name/ti/timekpr/package.nix`:
`version = "0.5.10"`, built with `fetchgit { url = "https://git.launchpad.net/timekpr-next"; tag = "v${version}"; }`).
All line numbers below come from that tag
(`https://git.launchpad.net/timekpr-next/tree/<path>?h=v0.5.10`), read from the
nix store copy of the source.

Vocabulary follows `GLOSSARY.md`: monitored user, observation, time left today,
time adjustment.

## TL;DR

- `setTimeLeft(user: s, op: s, seconds: i) -> (result: i, message: s)` on
  interface `com.timekpr.server.user.admin`. `op` must be `"+"`, `"-"` or `"="`, and
  `seconds` is a count of seconds.
- It changes only **`TIME_SPENT_BALANCE`**, the day's spent counter that includes
  bonuses, in the monitored user's control file (`<workdir>/<user>.time`). The
  daily limit, `TIME_SPENT_DAY/WEEK/MONTH` and the configuration are left alone,
  so it does what the glossary calls a **time adjustment**.
- `+` lowers the balance, which adds time. `-` raises it, which removes time.
  `=` sets `balance = todays_limit - seconds`. The balance is clamped to
  ±86400. A negative balance is allowed, so time left today can go **above the
  daily limit**.
- Before `+` or `-` is applied, the base balance is capped at today's limit:
  `min(balance, limit)`. Removing more time than is left therefore gives
  0 left today and builds no debt that a later `+` would have to pay off.
- **No active session is needed.** But for a monitored user who is logged out and
  whose control file is still dated a previous day, the adjustment is
  **silently wiped** by the next day-rollover reset. See "Day rollover" below.
- The caller must be root or in the `timekpr` group. The D-Bus policy enforces
  this, and the NixOS `services.timekpr.adminUsers` option fills that group.
- A logged-in monitored user gets a desktop notification: "Time allowance has
  changed, please note new time left!"

## Call chain

1. **CLI** `timekpra --settimeleft USER OP TIME`
   (`client/admin/adminprocessor.py:298-305`, `processSetTimeLeft` at `:653-680`).
   `TIME` goes through `getSecondsFromTimeStr`, which accepts plain seconds or
   strings such as `1h`, `1d2h3m4s` (`common/utils/misc.py:346-369`). The result
   is passed as an int to the D-Bus call (`client/interface/dbus/administration.py:393-409`).
2. **D-Bus method** `setTimeLeft`, `in_signature="ssi"`, `out_signature="is"`
   (`server/interface/dbus/daemon.py:878-904`):
   - builds a `timekprUserConfigurationProcessor` and calls `checkAndSetTimeLeft(op, seconds)`;
   - then, **if the user is in the in-memory user list** (has a session),
     calls `adjustTimeSpentFromControl(pSilent=False, pPreserveSpent=(op != "="))`.
3. **Validation and write** in `checkAndSetTimeLeft`
   (`server/config/configprocessor.py:677-741`).
4. **In-memory reload** in `adjustTimeSpentFromControl`
   (`server/user/userdata.py:338-419`).

## Operations and units

From `configprocessor.py:697-726`:

```python
elif pOperation not in ("+", "-", "="):      # -> TK_MSG_USER_ADMIN_CHK_TIMELIMIT_OPERATION_INVALID
...
timeLimit = self._timekprUserConfig.getUserLimitsPerWeekdays()[datetime.date(datetime.now()).isoweekday()-1]
if pOperation == "+":
    setLimit = min(max(min(balance, timeLimit) - pTimeLeft, -TK_LIMIT_PER_DAY), TK_LIMIT_PER_DAY)
elif pOperation == "-":
    setLimit = min(max(min(balance, timeLimit) + pTimeLeft, -TK_LIMIT_PER_DAY), TK_LIMIT_PER_DAY)
elif pOperation == "=":
    setLimit = min(max(timeLimit - pTimeLeft, -TK_LIMIT_PER_DAY), TK_LIMIT_PER_DAY)
self._timekprUserControl.setUserTimeSpentBalance(setLimit)
...
self._timekprUserControl.saveControl()
```

- `TK_LIMIT_PER_DAY = 86400` (`common/constants/constants.py:263`).
- `pTimeLeft` is in **seconds** (int32 on the bus). The only check is
  `int(pTimeLeft) > 0` inside a `try` that ignores the comparison's result
  (`:703-709`), so **0 and negative values are accepted**. `+ -600` behaves like
  `- 600`.
- The source comment says these operations are "technically reversed" because
  the stored value is time *spent*, not time left (`:719-720`).
- The upstream help text describes them as "set time left for the user at the
  current moment of time: + (add time), - (subtract time), = (set exact time
  available)" (`common/constants/messages.py:45`).

## What it changes

Only `TIME_SPENT_BALANCE`, which the control file describes as "total time
balance spent for this day" (`common/utils/config.py:1472-1474`).
`TIME_SPENT_DAY` ("total time spent for this day"), `TIME_SPENT_WEEK`,
`TIME_SPENT_MONTH`, `LAST_CHECKED` and the PlayTime counters are re-saved
unchanged (`saveControl`, `config.py:1513-1545`). Configured limits are not
touched.

So after a time adjustment, `TIME_SPENT_BALANCE` and `TIME_SPENT_DAY` drift
apart: the balance includes the adjustment and the day counter does not. Daily
time left is computed from the balance:

- saved: `TIME_LEFT_DAY = max(min(intervals_left_today, todays_limit - TIME_SPENT_BALANCE, 86400), 0)`
  (`configprocessor.py:93-135`, line 132);
- in memory: `LEFTD = LIMITD - SPENTBD` (`userdata.py:408`), then
  `recalculateTimeLeft` also takes the minimum with week and month time left
  (`userdata.py:~150-170`).

Week and month counters are untouched, so `+` does **not** lift a weekly or
monthly cap. PlayTime has its own separate `setPlayTimeLeft` method
(`daemon.py:~1070-1090`).

## Removing more than is left / adding beyond the limit

- **Over-removal:** the balance can rise as high as +86400. Time left is then
  negative internally and reported as `max(..., 0)`, which is 0. Because `+` and
  `-` start from `min(balance, limit)`, the next `+N` gives exactly N seconds left.
  No debt carries over. No error is returned.
- **Adding past the daily limit:** allowed. The balance goes negative, down to
  −86400, so `limit - balance` exceeds the limit. Effective time left today is
  still capped by the **remaining allowed-hours intervals** today, by the
  week/month limits (in-memory value only), and by 86400. Adding time cannot
  extend past the allowed hours.
- **`=`** sets `balance = limit - N`, which gives N seconds of time left today,
  subject to the same interval, week and month caps. `=` ignores time already
  spent.

## Session requirements

None. The method loads the user's config file (`<configdir>/timekpr.<user>.conf`)
and control file (`<workdir>/<user>.time`) from disk
(`configprocessor.py:31-65`). It fails only if one of them is missing:
`TK_MSG_CONFIG_LOADER_USERCONFIG_NOTFOUND` / `..._USERCONTROL_NOTFOUND`
(code 1, `messages.py:165-166`). Those files exist once Timekpr has tracked
the user at least once. On NixOS the dirs are `/var/lib/timekpr/config` and
`/var/lib/timekpr/work` (nixpkgs `nixos/modules/services/security/timekpr.nix`).

### Day rollover: the logged-out pitfall

`setTimeLeft` never updates `LAST_CHECKED`. Rollover is detected by comparing
`LAST_CHECKED`'s date with today (`config.py:1580-1594`):

- If the user is **logged out**, the next `getUserInformation(user, "S"|"F")` call
  runs `calculateAdjustedDatesForUserControl` (`configprocessor.py:244-248`, `:67-91`).
  When `LAST_CHECKED` is a previous day, that call **zeroes `TIME_SPENT_BALANCE`**
  and `TIME_SPENT_DAY`, sets `LAST_CHECKED=now` and saves.
- When the user **logs in**, the daemon calls `adjustTimeSpentFromControl()`
  (`daemon.py:236-248`). If the day changed, it sets the balance to the
  current hour's spent time and drops the file value (`userdata.py:394-400`).

So a time adjustment made for a monitored user who has not been seen today, by
either a session or a `getUserInformation` S/F call, is **lost**. The `+`/`-`
base is also computed from the stale previous-day balance.

In practice this case is narrower than it sounds. The collector already calls
`getUserInformation(user, "F")` on every observation (`src/main.rs:79`), and
that call rolls the file over to today. Once that has happened today, a
logged-out adjustment persists, and at login `dayChanged` is false, so the
balance is kept. A robust web UI should still read `getUserInformation(user, "F")`
**immediately before** calling `setTimeLeft`.

## Immediacy, restart, rollover

- **Logged-in user:** the D-Bus call writes the file and then reloads it into
  memory in the same main-loop callback. With `pPreserveSpent=True` (for `+`/`-`),
  seconds spent since the last periodic save (`TK_SAVE_INTERVAL = 30` s,
  `constants.py:287`) are added back on top: `timeSpentBeforeReload`
  (`userdata.py:368-375`, `:400`). For `=` they are dropped. The next
  `getUserInformation` reflects the change at once in `TIME_SPENT_BALANCE`,
  `TIME_LEFT_DAY` (saved) and `ACTUAL_TIME_SPENT_BALANCE`/`ACTUAL_TIME_LEFT_DAY`
  (in-memory, `daemon.py:511-543`).
- **Logged-out user:** the file is written at once, and `TIME_SPENT_BALANCE` /
  `TIME_LEFT_DAY` from `getUserInformation` show it, subject to the rollover
  pitfall above.
- **Restart:** the value lives in the control file, so it survives a daemon
  restart. On reload, `dayChanged` is false on the same day.
- **Day rollover:** the balance resets to 0 at the start of a new day. An
  adjustment applies to **today only**.

## Return values and errors

`(int result, str message)`. On success the result is `(0, "")`.

| Condition | Message key | Code |
|---|---|---|
| User config missing | `TK_MSG_CONFIG_LOADER_USERCONFIG_NOTFOUND` | 1 |
| Control file missing | `TK_MSG_CONFIG_LOADER_USERCONTROL_NOTFOUND` | 1 |
| `op` not in `+ - =` | `TK_MSG_USER_ADMIN_CHK_TIMELIMIT_OPERATION_INVALID` | 28 |
| `seconds` not an int (unreachable over D-Bus `i`) | `TK_MSG_USER_ADMIN_CHK_TIMELIMIT_INVALID` | 28 |
| Exception while computing/setting (see bug below) | `TK_MSG_USER_ADMIN_CHK_TIMELIMIT_INVALID_SET` | 28 |
| Any other exception | `TK_MSG_CONFIG_LOADER_SAVECONTROL_UNEXPECTED_ERROR` | 1 |

The codes come from `messages.py`, where an `i` counter starts at 10 on line 55
and these three share one value, 28. The messages are translated strings.
Callers should treat any non-zero result as failure and show `message`.
Timekpr 0.5.10's `timekpra` now exits non-zero on such errors (debian/changelog).

### Bug: weekday limit indexing

`timeLimit = getUserLimitsPerWeekdays()[isoweekday()-1]` (`configprocessor.py:718`)
indexes the limit list by weekday number. That list is positional and matches
`ALLOWED_WEEKDAYS` ("number of values must match number of values for
ALLOWED_WEEKDAYS", `resource/server/timekpr.USER.conf:19`). When not every day
of the week is allowed, for example `ALLOWED_WEEKDAYS = 1;2;3;4;5`:

- on a day past the list's end, the lookup raises `IndexError`, and the call returns `..._TIMELIMIT_INVALID_SET` (28);
- if the allowed days are not contiguous from Monday, it uses the **wrong day's limit**.

`calculateTimeAvailableFromSavedConfiguration` does the lookup correctly with
`allowedWeekDays.index(currDay)` (`configprocessor.py:129-130`). The default and
typical config has all 7 days allowed, and then the bug does not show.

## Privileges

D-Bus policy `resource/server/dbus/timekpr.conf`: the default context **denies**
`com.timekpr.server.user.admin`. That interface is allowed for `group="timekpr"`
and `user="root"`. The method itself does no further checks. The NixOS module
creates the `timekpr` group (gid 2000) with `members = cfg.adminUsers` and
installs the package's D-Bus policy (`services.dbus.packages`). A collector
service that calls `setTimeLeft` therefore needs to run as root or as a member
of `timekpr`, the same requirement as the `getUserInformation` it already calls.

## Notifications

When the user has a session, `adjustTimeSpentFromControl(pSilent=False)` emits the
`timeLeftChangedNotification` D-Bus signal (`userdata.py:414-417`;
`common/utils/notifications.py:244-248`). The user's client shows
"Time allowance has changed, please note new time left!"
(`client/interface/dbus/notifications.py:340-342`, `messages.py:257`) when
limit notifications and the tray icon are enabled
(`client/interface/dbus/daemon.py:278-284`). A logged-out user gets no notification.

## Implications for the web UI (input for map #1)

- A time adjustment maps directly onto `+N` / `-N` seconds. `=` is available
  for "set time left today to N".
- Removals clamp to 0 left with no error. Additions can exceed the daily limit
  but are capped by allowed hours and week/month limits, so the UI should show
  the resulting `TIME_LEFT_DAY` after the call rather than predict it.
- No session is needed, but call `getUserInformation(user, "F")` just before
  `setTimeLeft` so the control file has rolled over to today.
- Watch for the weekday-index bug on monitored users who do not have all 7
  days allowed.
- The collector process needs `timekpr` group membership or root.
