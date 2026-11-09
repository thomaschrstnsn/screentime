# What a monitored user sees when an admin changes time left today

Research for issue #16. Question: when an admin calls Timekpr-nExT's `setTimeLeft`
(`+`/`-`) over D-Bus for a user with an active session, what does the Timekpr
client show that user?

## Answer

Timekpr already tells the user. The server emits a dedicated D-Bus signal,
`timeLeftChangedNotification`, as soon as `setTimeLeft` succeeds. The client turns
it into a desktop notification titled "Timekpr-nExT notification" with the fixed
text **"Time allowance has changed, please note new time left!"**. The text does
not give the amount, does not say whether time went up or down, and does not show
the new total. Within about one poll interval (3 s by default), the tray or
indicator label then shows the new time left. If the change moves the user
across one of their client-side warning thresholds, they also get the usual
"You have X hours Y minutes Z seconds left" notification.

Three conditions suppress the change notification:

- The user is not logged in. The server sends signals only to users in its
  in-memory session list.
- The admin has set **Hide icon and notifications** (`HIDE_TRAY_ICON`) for the
  user.
- The user has unticked **Show limit changes** in their own client settings
  (`SHOW_LIMIT_NOTIFICATION` in `~/.config/timekpr/timekpr.conf`, default
  `True`).

**Implication:** neither screentime-web nor the collector needs to send its own
"your time changed" notification. Timekpr does this by default. Its notification
has no detail, so a richer message (the amount, the reason, who granted it) is
the only thing we could add.

## Evidence

All paths are relative to the Timekpr-nExT checkout (`~/src/timekpr-next`,
commit `a3e6b2b`).

### Server: `setTimeLeft` triggers an immediate notification

- `server/interface/dbus/daemon.py:1440` `setTimeLeft(pUserName, pOperation, pTimeLeft)`
  writes the control file through `checkAndSetTimeLeft`. Then, at
  `daemon.py:1458-1462`:

  ```python
  if pUserName in self._timekprUserList:
      # inform the user immediately
      self._timekprUserList[pUserName].adjustTimeSpentFromControl(
          pSilent=False, pPreserveSpent=(pOperation != "=")
      )
  ```

  `_timekprUserList` holds only users the daemon found as logged-in sessions
  (they are added at `daemon.py:289` and removed when they leave at
  `daemon.py:308-317`). A user who is not logged in gets the change silently and
  sees the new figure at their next login.
- `server/user/userdata.py:338` `adjustTimeSpentFromControl` reloads the control
  file, recalculates `LEFTD`, and, because `pSilent=False`, calls
  `timeLeftChangedNotification(cons.TK_PRIO_IMPORTANT_INFO)` (`userdata.py:414-417`).
  It also updates the control-file mtime it has cached (`userdata.py:412`). As a
  result, the external-change check in `saveSpent` (`userdata.py:670-673`) does
  not fire a second notification for the same write.
- `common/utils/notifications.py:242-247` declares `timeLeftChangedNotification`
  as a D-Bus signal with signature `s` that carries only the priority. The signal
  carries no time value.

### Server: the new figure follows on the next poll

- The worker loop calls `getTimeLeft()` for every tracked user on each poll
  (`daemon.py:345`) and sleeps `TIMEKPR_POLLTIME` between polls (`daemon.py:179-183`;
  default `TK_POLLTIME = 3`, `common/constants/constants.py:285`).
- `getTimeLeft` passes the values to `processTimeLeft` (`userdata.py:633`). That
  function always emits the `timeLeft` signal with the current figures
  (`notifications.py:124`). It also emits `timeLeftNotification` when the
  server's coarse level changes (`notifications.py:127-139`). The server has
  three levels: at least the final-notification time left, at least 0, and below
  0 (`notifications.py:52-56`). So a `-` that pushes the user below the final
  warning time, or a `+` that lifts them back above it, also produces a server-side
  time-left notification.

### Client: how the signals are shown

- `client/interface/dbus/daemon.py:174-178` subscribes to `timeLeftChangedNotification`.
- `client/interface/dbus/daemon.py:278-284`:

  ```python
  def receiveTimeLeftChangedNotification(self, pPriority):
      if self._timekprClientConfig.getClientShowLimitNotifications() and self._timekprClientIndicator.getTrayIconEnabled():
          self._timekprClientIndicator.notifyUser(cons.TK_MSG_CODE_TIMELEFTCHANGED, None, pPriority)
  ```

  This handler is where both suppression settings apply.
- `client/interface/dbus/notifications.py:340-342` maps `TK_MSG_CODE_TIMELEFTCHANGED`
  to the message `TK_MSG_NOTIFICATION_ALLOWANCE_CHANGED`. That message is defined at
  `common/constants/messages.py:247` as "Time allowance has changed, please note
  new time left!". The notification title is `TK_MSG_NOTIFICATION_TITLE`,
  "Timekpr-nExT notification" (`messages.py:244`).
- `client/interface/dbus/notifications.py:364-455` `notifyUser` sends the message through the freedesktop
  `Notify` call. `IMPORTANT_INFO` is not in `(CRITICAL, IMPORTANT)`, so it uses
  the normal notification timeout and warning sound, if sound is enabled
  (`notifications.py:384-402`). The icon is `dialog-information`
  (`constants.py:328`). If the user has enabled speech notifications, the
  message is also read aloud (`notifications.py:452-455`). A notification shown
  shortly after an earlier one replaces it instead of stacking
  (`notifications.py:404-408`). Several quick adjustments therefore collapse into
  one visible popup.
- On the next `timeLeft` signal, `receiveTimeLeft` (`client/interface/dbus/daemon.py:232-248`)
  updates the indicator label through `formatTimeLeft`
  (`client/interface/ui/notificationarea.py:99-176`). If the new value falls into
  a different user-configured warning level (`_determinePriority`,
  `notificationarea.py:81-97`), the client also shows the standard "You have …
  left" notification (`notificationarea.py:143-155`) and may change the tray
  icon colour (`notificationarea.py:163-170`).

### Settings that suppress or enable the notification

| Setting | Owner | Where | Effect |
|---|---|---|---|
| `SHOW_LIMIT_NOTIFICATION` | monitored user (client) | `common/utils/config.py:1651` (default `True`); GUI checkbox "Show limit changes", `resource/client/forms/client.glade:1641-1649`; file `~/.config/timekpr/timekpr.conf` (`config.py:1608`) | `False` suppresses this notification and the "configuration changed" one |
| `HIDE_TRAY_ICON` | admin (per-user config) | sent to the client as `HIDEI` (`userdata.py:613`, `notifications.py:110`), applied in `client/interface/dbus/daemon.py:219-227` | hides the icon; `getTrayIconEnabled()` is then false, so the change notification is suppressed. README: "Hides the Timekpr-nExT client icon and almost all notifications. Only critical notifications are shown." (`README.md:570-572`) |
| `USE_SPEECH_NOTIFICATIONS` | monitored user | `config.py:1657` (default `False`) | also reads the message aloud |
| notification sound / timeouts | monitored user | client config | change how the popup is presented, not whether it appears |

No server-side or admin D-Bus option enables or disables this notification
specifically. The admin can only hide all non-critical notifications with
`HIDE_TRAY_ICON`.

## Caveats

- The local checkout identifies itself as `TK_VERSION = "0.5.9"`
  (`common/constants/constants.py:17`; latest commit 2025-11-24). The project
  targets 0.5.10. I did not check whether 0.5.10 changes this path. It has been
  stable across 0.5.x, but it is worth checking once against the deployed
  version.
- The monitored user owns the client setting and can turn the notification off
  themselves. Nothing guarantees they will see it.
- No client running (for example, a non-graphical session, or the client not
  autostarted) means no notification at all. The server still applies the change.
- This is a source reading. It has not been observed in the test VM.
