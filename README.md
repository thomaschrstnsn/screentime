# screentime-collector

A Rust application that collects screen time usage data from [Timekpr-nExT](https://launchpad.net/timekpr-next) via DBus and publishes it to a NATS server.

## Overview

`screentime-collector` periodically queries the Timekpr-nExT DBus interface to retrieve user session information, such as:
- Time left for the day
- Time spent today
- Time spent this week
- Time spent this month
- Time balance

This information is then serialized as JSON and published to a configured NATS subject.

## Usage

```bash
cargo run -- --users <USERNAME> --hostname <HOSTNAME> [--nats-url <NATS_URL>]
```

### Arguments

- `-u`, `--users`: Comma-separated list of usernames to monitor (Required).
- `--hostname`: The hostname of the machine running the collector (Required). Used in the NATS subject.
- `-n`, `--nats-url`: URL of the NATS server. Defaults to `nats://localhost:4222`.

### Example

```bash
cargo run -- --users kid1,kid2 --hostname my-pc --nats-url nats://nats.example.com:4222
```

## Data Format

The application publishes JSON data to the subject `time.obs.<hostname>.<user>`.

Example payload:

```json
{
  "left_day": 3600,
  "spent_balance": 0,
  "spent_month": 18000,
  "spent_week": 7200,
  "spent_day": 3600
}
```

## Requirements

- Rust (latest stable)
- Timekpr-nExT installed and running
- A NATS server

## Integration testing

[`test-vm/`](test-vm/README.md) has a NixOS VM running Timekpr-nExT, NATS and the collector built from the working tree, plus scripts to start, stop and delete it, move its clock, and change a monitored user's time left today:

```bash
test-vm/start
test-vm/timekpr add 10m
test-vm/nats sub 'time.obs.>'
```

The collector itself also builds with `nix build`.
