# Test VM

A headless NixOS VM for manual integration tests. Timekpr-nExT tracks a monitored user, the collector built from this working tree reports on them, and NATS runs with JetStream. It is a small copy of the production setup on *cyrus*, and you control both the clock and the monitored user's time left today.

Requires Linux on x86_64 with KVM and Nix with flakes enabled. Nothing has to be installed beyond that.

```sh
test-vm/start            # build from the working tree and boot (~10 s after the first build)
test-vm/timekpr          # what timekpr thinks of junior right now
test-vm/nats sub 'time.>'  # watch observations arrive on the host
test-vm/stop
```

## What's inside

| | |
|---|---|
| Monitored user | `junior`, with a fake wayland session so timekpr counts their time from boot |
| Collector | `screentime-collector -u junior,ghost --hostname test-vm`, running as `stcol` (in group `timekpr`), publishing to `time.obs.test-vm.junior` every 5 s and serving time adjustments on `time.adjust.test-vm.*`. `ghost` is unknown to Timekpr, so it is skipped with a warning. |
| NATS | JetStream enabled, no auth, guest port 4222 forwarded to **`nats://127.0.0.1:14222`**. The collector creates the `OBSERVATIONS` and `ADJUSTMENTS` streams at startup. |
| SSH | root, guest port 22 forwarded to **`127.0.0.1:12222`**, key login only |
| Clock | UTC, with NTP off so clock changes stick |

The NixOS config is [`configuration.nix`](configuration.nix), wired into the repo's `flake.nix` as `nixosConfigurations.test-vm` and `packages.test-vm`.

## Scripts

All scripts work from any directory.

### Lifecycle

| Script | Does |
|---|---|
| `start [--foreground]` | Builds the VM, collector included, from the working tree, then boots it in the background and waits for SSH. `--foreground` attaches the serial console to your terminal instead (root is auto-logged in; quit with `Ctrl-a x`). |
| `stop` | Shuts down cleanly over SSH, and kills QEMU if that hasn't finished after 30 s. The disk is kept. |
| `delete` | Stops the VM and removes `.state/` (disk, SSH key, logs). The next `start` boots a fresh VM. |
| `status` | Running or not, the state of the relevant services, and the guest clock. |
| `ssh [command…]` | A root shell, or runs one command, e.g. `test-vm/ssh journalctl -fu screentime-collector`. |
| `nats <args…>` | The NATS CLI on the host, pointed at the VM, e.g. `test-vm/nats sub 'time.>'` or `test-vm/nats req …`. |

**Trying a code change:** the VM runs whatever collector was in the working tree at `start`. Run `test-vm/stop && test-vm/start` to pick up changes. The disk survives, so timekpr's limits and time spent carry over.

**The disk persists** across `stop`/`start` in `test-vm/.state/disk.qcow2` (gitignored), so timekpr state is kept. `delete` starts over.

### Time left today: `timekpr`

These calls go through Timekpr's DBus admin API as `stcol`, the same user and permissions as the collector.

| Command | Does |
|---|---|
| `timekpr` | Time left today, spent today, balance, spent week/month, daily limits. |
| `timekpr add <duration>` | Time adjustment: adds to time left today (`setTimeLeft +`). |
| `timekpr remove <duration>` | Time adjustment: removes from time left today, stopping at 0 (`setTimeLeft -`). |
| `timekpr set <duration>` | Sets time left today outright (`setTimeLeft =`). |
| `timekpr limit <duration>` | Sets the daily limit for every allowed weekday. |

Durations are seconds or `h`/`m`/`s` combinations: `600`, `10m`, `1h30m`. Set `TEST_VM_USER` to target another user.

**Set a limit first.** A fresh VM has timekpr's default of 24 h on every day, so time left today is really just the time until midnight. Adding time then changes the balance but not time left today. Run `test-vm/timekpr limit 1h` to see adjustments take effect. The limit persists on the disk.

### The clock: `clock`

| Command | Does |
|---|---|
| `clock` | Guest and host time. |
| `clock set <date>` | Any GNU `date` string: `"2026-11-09 08:00"`, `"tomorrow 10:00"`, `"next monday 09:00"`. |
| `clock advance <duration>` | Jumps forward, e.g. `2h`. |
| `clock midnight [seconds]` | Jumps to just before midnight (30 s by default), so you can watch the day roll over. |
| `clock reset` | Back to the host's time. |

Timekpr notices a clock jump within about 3 s, treats it like a wake from sleep, and rolls over the day, week or month as needed. Moving the clock **backwards** (including `reset`) doesn't restore the earlier day: timekpr sees another day change and starts it fresh. Use `delete` for a truly clean slate. A reboot also resets the clock to the host's time.

## Recipes

**Watch a time adjustment reach the collector:**

```sh
test-vm/timekpr limit 1h
test-vm/nats sub 'time.obs.>' &
test-vm/timekpr add 10m        # within 5 s, left_day goes up by 600
```

**Watch the day roll over:**

```sh
test-vm/clock midnight 20
test-vm/timekpr                # 20 s left: capped by midnight
sleep 25; test-vm/timekpr      # a new day with the full limit
```

**A logged-out monitored user:** `test-vm/ssh systemctl stop monitored-session` ends junior's session, and `test-vm/ssh systemctl start monitored-session` brings it back. Timekpr stops counting time. Logind keeps junior's user manager running for a while (`loginctl list-sessions` shows a `manager` session), and as long as it does, timekpr holds junior in memory and handles day changes itself. Once `loginctl list-users` no longer shows junior, follow with `test-vm/ssh systemctl restart timekpr` to reach the fully logged-out case. There, timekpr answers DBus calls from its saved files, and a `setTimeLeft` on a new day is wiped by the next refresh unless `getUserInformation` ran first (every `timekpr` command does).

**Point screentime-web at the VM:** use `nats://127.0.0.1:14222`.

## Troubleshooting

- **`start` says the VM exited during boot:** read `test-vm/.state/console.log`. The usual causes are a missing `/dev/kvm` or ports 12222/14222 already in use on the host.
- **The collector restarts a few times right after the first boot.** That's expected: it fails until timekpr has seen junior's session once. `Restart=on-failure` takes care of it.
- **The collector crashes for every user** when it's asked to report on a user timekpr has never seen: timekpr answers with an empty result, and the collector panics while parsing it.
- **The collector unit isn't ordered after `timekpr.service`** on purpose. nixpkgs orders timekpr after `multi-user.target`, so that ordering would create a cycle and systemd would drop the collector at boot.
- **Nix doesn't see a new file:** flakes only see git-tracked files. With jj that happens automatically; with plain git, `git add` the file.
