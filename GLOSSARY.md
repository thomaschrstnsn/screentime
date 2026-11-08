# Screentime

Observing and adjusting the screen time of monitored users on machines governed by Timekpr-nExT.

## Language

**Monitored user**:
A local account on a host whose screen time Timekpr-nExT limits and the collector reports on.
_Avoid_: Kid, child, target user

**Observation**:
A snapshot of a monitored user's time figures (left today, spent today/week/month, balance) at a point in time.
_Avoid_: Reading, sample, event

**Time left today**:
The remaining screen time a monitored user has before Timekpr-nExT locks them out for the current day.
_Avoid_: Remaining time, allowance, daily limit

**Time adjustment**:
A one-off increase or decrease of a monitored user's time left today, leaving their configured limits unchanged.
_Avoid_: Extension, bonus, override, limit change

**Adjuster**:
A person admitted by the screentime Cloudflare Access policy, who may make time adjustments.
_Avoid_: Admin, parent, operator
