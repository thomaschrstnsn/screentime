# Headless NixOS VM with a timekpr-tracked monitored user

Research for issue #5 (child of map #1, "time adjustments from the web UI").
Question: how do we build an ephemeral, headless NixOS QEMU VM from a flake in
this repo, where Timekpr-nExT actively tracks a monitored user, so the
collector can read and adjust that user's time?

Sources read (pinned versions):

- nixpkgs `55bfb4d7437bd4e09ab7cba58a43bd68eb310044` (26.11 pre-release, the
  host's `nixpkgs` registry entry), referred to below as `nixpkgs:`.
- Timekpr-nExT `v0.5.10` (`https://git.launchpad.net/timekpr-next`, the tag
  nixpkgs packages), referred to as `timekpr:`.
- systemd `v261.3` (the nixpkgs systemd), referred to as `systemd:`.
- `~/dotfiles` (production config for host `cyrus`).

## TL;DR

- `services.timekpr` comes from **nixpkgs** (added in 25.11), not dotfiles.
  Its only options are `enable`, `package` and `adminUsers`.
- Timekpr **discovers** users only through logind (`ListUsers`), and **counts
  time** only for a session that is `active`, not idle, and whose type is
  `x11`, `wayland` or `mir`. `tty` sessions are explicitly excluded by
  default. So a user with no session is never tracked: no time accrues.
- `getUserInformation` / `setTimeLeft` work on the **saved files** in
  `/var/lib/timekpr/{config,work}`, so they work without a session, **but only
  after those files exist**. Timekpr creates them the first time it sees the
  user in logind. Before that, both calls return an error (and today's
  collector panics on that error).
- Minimal headless session, keeping the stock timekpr config: a systemd
  service running as the monitored user with `PAMName=` and
  `Environment=XDG_SESSION_TYPE=wayland`. pam_systemd registers it with logind
  as a seatless `wayland` session. Seatless means always `active`, and a
  graphical type means the idle hint is only set explicitly, so it defaults to
  false. No compositor, GPU or graphics needed. (Evaluated, not booted yet:
  the spike must confirm it.)
- Fallback if that fails: override `/etc/timekpr/timekpr.conf` to track `tty`
  and use an SSH or serial autologin session.
- Expose `nixosConfigurations.test-vm.config.system.build.vm` as
  `packages.x86_64-linux.test-vm`. It has `meta.mainProgram`, so
  `nix run .#test-vm` works. Use `virtualisation.diskImage = null` for an
  ephemeral tmpfs root, `forwardPorts` for 4222 and 2222->22, and open the
  guest firewall.
- NATS: `services.nats = { enable = true; jetstream = true; }` (port 4222). The
  module has no `openFirewall`.

## 1. Where does `services.timekpr` come from?

From **nixpkgs**: `nixpkgs:nixos/modules/services/security/timekpr.nix`.
Release note: `nixpkgs:nixos/doc/manual/release-notes/rl-2511.section.md:182`.
Manual: `nixpkgs:nixos/doc/manual/configuration/user-mgmt.chapter.md:158`.
`~/dotfiles` only *uses* it (`machines/cyrus/configuration.nix:153`), and
nothing there defines it.

Options: `services.timekpr.enable`, `.package` (default `pkgs.timekpr`,
0.5.10), `.adminUsers` (list of users added to group `timekpr`, gid 2000).

What the module does:

- Adds the package to `services.dbus.packages`, which installs the policy
  `timekpr:resource/server/dbus/timekpr.conf`. Under that policy, only `root`
  and group `timekpr` may call `com.timekpr.server.user.admin`. That is why
  `stcol` must be in `adminUsers`.
- Adds the package to `systemd.packages`, plus `wantedBy = multi-user.target`.
- Turns on `security.polkit.enable`.
- Adds tmpfiles rules for `/var/lib/timekpr/{config,work}`.
- Sets `environment.etc."timekpr".source = "${pkg}/etc/timekpr"`. The main
  config is therefore a **read-only store copy of upstream defaults**. Changing
  it means overriding that `etc` entry with `lib.mkForce`, pointing at a
  patched directory.

The nixpkgs VM test (`nixpkgs:nixos/tests/timekpr.nix`) only checks that the
unit starts. No upstream test exercises a tracked user.

## 2. Does Timekpr track, or answer for, a user with no logind session?

### Discovery: logind only

`timekpr:server/interface/dbus/daemon.py` `checkUsers()` calls
`_timekprLoginManager.getUserList()`, which is logind `ListUsers`
(`timekpr:server/interface/dbus/logind/manager.py:105`). A user who is not in
logind (no session and not lingering) never gets a `timekprUser` object, and
users that leave logind are removed again (`removableUsers`). A newly seen
user is accepted only if all of the following hold:

- uid >= `UID_MIN - 1` (from `login.defs`);
- a shell that is not `nologin`/`false`;
- the name is not in `TIMEKPR_USERS_EXCL` (`testtimekpr;gdm;...`), so **don't
  name the test user `testtimekpr`**.

The check is in `timekpr:server/config/userhelper.py:55`. `isNormalUser = true`
in NixOS satisfies it.

### Accounting: only active, non-idle, controlled session types

`timekpr:server/interface/dbus/logind/user.py:118` `isUserActive()` counts the
user as active only when a session has all of these:

- `State == "active"`;
- `IdleHint == False`;
- `LockedHint != True`;
- `Type` in `TIMEKPR_SESSION_TYPES_CTRL` and not in `TIMEKPR_SESSION_TYPES_EXCL`.

The defaults are in `timekpr:resource/server/timekpr.conf`:

```
TIMEKPR_SESSION_TYPES_CTRL = x11;wayland;mir
TIMEKPR_SESSION_TYPES_EXCL = tty;unspecified
```

So a lingering user, or an SSH, getty or serial login (all type `tty`), is
seen but **never accrues time** with the stock config.

### getUserInformation / setTimeLeft without a session

- `getUserInformation(user, "F")` (`daemon.py:635`) loads the saved user config
  and control files via `getSavedUserInformation`
  (`timekpr:server/config/configprocessor.py:169`). It returns
  `TIME_LEFT_DAY`, `TIME_SPENT_BALANCE/DAY/WEEK/MONTH`, and so on, which are
  the keys the collector reads. It adds the `ACTUAL_*` realtime keys only if
  the user is in memory.
- `setTimeLeft(user, op, secs)` (`daemon.py:879`) writes through
  `checkAndSetTimeLeft` to the control file. It pushes the change to memory
  only if the user is currently tracked.
- Both use `loadUserConfiguration(True)` / `loadUserControl(True)`. Here
  `True` means validate only, and they **fail if
  `/var/lib/timekpr/config/timekpr.<user>.conf` or
  `/var/lib/timekpr/work/<user>.time` don't exist** (`configprocessor.py:31-65`,
  `timekpr:common/utils/config.py:817,1397`). Timekpr creates those files when
  it first sets the user up from logind (`adjustLimitsFromConfig` /
  `adjustTimeSpentFromControl` in `checkUsers`).

Consequence: the user must have had a logind session at least once, or the
files must be pre-seeded, before the collector can read or adjust them. In
the current `src/main.rs`, a non-zero result gives an empty map, so
`TimeObservation::try_from` fails and `.expect` **panics**. In the VM the
collector unit needs `Restart=on-failure` (as production has) or must start
after the session exists.

### Minimal headless session

The session must look `active`, non-idle and typed `x11`/`wayland`/`mir`, with
no graphics stack. The relevant facts:

- **Seatless sessions are always active**:
  `systemd:src/login/logind-session.c:1114`
  `session_is_active(): if (!s->seat) return true;`.
- **Graphical types have an explicit idle hint only**:
  `systemd:src/login/logind-session.c:1160`. For `SESSION_TYPE_IS_GRAPHICAL`
  it returns `s->idle_hint`, which is false unless a client calls
  `SetIdleHint(true)`.
- **For comparison, tty sessions** go idle when the tty atime is older than
  `IdleActionSec` (default 30 min, `systemd:src/login/logind-core.c:72`). An
  idle getty autologin would stop accruing after 30 minutes.
- **pam_systemd takes the type from the PAM env** `XDG_SESSION_TYPE`
  (`systemd:src/login/pam_systemd.c:1791`, `getenv_harder`). The exception is
  ssh, which is always forced to `tty` (`pam_systemd.c:955`).
- **systemd copies a unit's environment into the PAM env** before
  `pam_open_session` when `PAMName=` is set
  (`systemd:src/core/exec-invoke.c:1343`, `pam_putenv`).

So this is the recommended mechanism. It needs no change to the timekpr
config:

```nix
security.pam.services.timekpr-session.startSession = true; # adds pam_systemd
systemd.services.monitored-session = {
  wantedBy = [ "multi-user.target" ];
  after = [ "systemd-user-sessions.service" "systemd-logind.service" ];
  environment = { XDG_SESSION_TYPE = "wayland"; XDG_SESSION_CLASS = "user"; };
  serviceConfig = {
    User = "conrad";
    PAMName = "timekpr-session";
    ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
    Restart = "always"; # timekpr terminates the session when time runs out
  };
};
```

To check it in the spike, run `loginctl list-sessions` and
`loginctl show-session <id> -p Type -p State -p IdleHint`. Expect
`wayland / active / no`. Then check `journalctl -u timekpr` for
`user "conrad" set up for tracking`. `TIME_SPENT_DAY` should grow about 3 s per
3 s poll. Stopping the unit ends the session, so it doubles as a
"log out / log in" toggle for tests.

**Fallback** if logind or timekpr object to a seatless `wayland` session:

1. Override `environment.etc."timekpr".source = lib.mkForce <dir>` with a
   `timekpr.conf` where `TIMEKPR_SESSION_TYPES_CTRL = tty;x11;wayland;mir` and
   `TIMEKPR_SESSION_TYPES_EXCL = unspecified`.
2. Hold any session: `ssh -p 2222 conrad@localhost sleep infinity`, or
   `services.getty.autologinUser = "conrad"` on the serial console. SSH
   sessions are seatless and so always active. Their leader has no ctty, so
   `get_process_ctty_atime` fails and the idle hint stays false. A getty
   session on a VT goes idle after `IdleActionSec`, so set
   `services.logind.settings.Login.IdleActionSec = "infinity"` if you use
   that.

Not needed: a lingering user (`users.users.conrad.linger = true`). It makes
the user appear in `ListUsers`, so the files get created, but no time accrues.
It is a cheap way to pre-create the files, but the PAM session already covers
that.

## 3. Building and running

`nixpkgs:nixos/modules/virtualisation/qemu-vm.nix`:

- `system.build.vm` is a derivation with
  `meta.mainProgram = "run-${config.system.name}-vm"` (line 1515). So
  `packages.x86_64-linux.test-vm = self.nixosConfigurations.test-vm.config.system.build.vm`
  gives you `nix run .#test-vm`. No `apps` entry is needed. `nixos-rebuild
  build-vm` does the same thing but needs a `--flake` target and leaves a
  `result` symlink, so `nix run` is the better loop.
- `mountHostNixStore` defaults to true (line 917): the guest mounts the host
  `/nix/store` over virtiofs, so no disk image is built. A rebuild costs only
  the changed derivations.
- `virtualisation.diskImage = null` gives a tmpfs root and no persistence
  (lines 473-485). Without it, a `./test-vm.qcow2` appears in the cwd and keeps
  state, including `/var/lib/timekpr`, between runs.
- `virtualisation.graphics = false` runs `-nographic`, with the serial console
  in the terminal (line 553).
- `virtualisation.forwardPorts = [ { from = "host"; host.port = 4222; guest.port = 4222; } { from = "host"; host.port = 2222; guest.port = 22; } ]`
  becomes slirp `hostfwd=tcp::4222-:4222,hostfwd=tcp::2222-:22` (line 649 and
  1298; confirmed by evaluation). The guest firewall is on by default, so add
  `networking.firewall.allowedTCPPorts = [ 4222 ]`. openssh opens 22 itself.
- `virtualisation.sharedDirectories.<tag> = { source; target; writable?; }`
  (line 584). The run script starts `virtiofsd` unprivileged by itself
  (line 329), so no root is needed.
- Defaults: `memorySize = 1024` MiB, `cores = 1`.

### Getting the working-tree collector in

| Option | How | Loop cost | Notes |
|---|---|---|---|
| A. Nix-built package (recommended default) | `rustPlatform.buildRustPackage { src = lib.cleanSource ./.; cargoLock.lockFile = ./Cargo.lock; }` in the flake, used in the VM's `ExecStart` | Full cargo build per change (no incremental cache) and a VM restart | Reproducible and the same as production. A flake only sees **git-tracked** files, so new files need `git add`. |
| B. Shared host build dir | `cargo build --release` on the host; `sharedDirectories.target = { source = "$PWD/target/release"; target = "/mnt/stcol"; }`; `ExecStart=/mnt/stcol/screentime-collector` | Incremental cargo build plus `systemctl restart` in the guest, no VM rebuild | The binary is dynamically linked against host glibc. That is fine because both use the same nixpkgs store paths over the shared store, but the host build must use the flake's nixpkgs (devShell). `source` is evaluated in the run script, so `$PWD` works. |

Use A for `nix run .#test-vm`. Add B, or an `ExecStart` that prefers
`/mnt/stcol` when it exists, if the edit loop feels slow.

## 4. NATS in the VM

`nixpkgs:nixos/modules/services/networking/nats.nix`. Options include
`enable`, `jetstream` (enable option), `port` (default 4222), `dataDir`
(default `/var/lib/nats`), `serverName`, freeform `settings`, and
`validateConfig`. With `jetstream = true` the generated settings are
`{ port = 4222; jetstream.store_dir = "/var/lib/nats"; server_name = "nats"; }`
(confirmed by evaluation). No listen host is set, so NATS binds all
interfaces. There is **no `openFirewall`**.

## 5. Boot time and resources

These are estimates, not measured. The spike should time them.

- **RAM and CPU:** the defaults (1 GiB, 1 core) are plenty for timekprd
  (Python), nats-server, the collector, sshd and a `sleep`. Use
  `cores = 2` for snappier boots.
- **Boot:** with KVM, direct kernel boot, the host store over virtiofs and a
  tmpfs root, there is no image build and no bootloader. A minimal NixOS guest
  like this typically reaches `multi-user.target` in roughly 5-15 s. Timekpr
  then picks up the session within one poll (`TIMEKPR_POLLTIME = 3`).
- **First build:** the timekpr Python package and the collector (if not
  cached). After that, config-only changes rebuild just the toplevel.
- **KVM:** the run script uses KVM when `/dev/kvm` is accessible. Without it,
  TCG is much slower.

## Sketch: flake outputs

```nix
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable"; # >= 25.11 for services.timekpr
  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      lib = nixpkgs.lib;
      screentime-collector = pkgs.rustPlatform.buildRustPackage {
        pname = "screentime-collector";
        version = "dev";
        src = lib.cleanSource ./.;
        cargoLock.lockFile = ./Cargo.lock;
      };
    in {
      packages.${system} = {
        default = screentime-collector;
        test-vm = self.nixosConfigurations.test-vm.config.system.build.vm;
      };

      nixosConfigurations.test-vm = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix"
          ({ pkgs, ... }: {
            system.stateVersion = "25.11";
            networking.hostName = "test-vm";

            virtualisation = {
              graphics = false;
              diskImage = null;      # ephemeral
              memorySize = 1024;
              cores = 2;
              forwardPorts = [
                { from = "host"; host.port = 4222; guest.port = 4222; }
                { from = "host"; host.port = 2222; guest.port = 22; }
              ];
            };
            networking.firewall.allowedTCPPorts = [ 4222 ];

            services.openssh = { enable = true; settings.PermitRootLogin = "yes"; };
            users.users.root.initialPassword = "root";   # test VM only
            services.getty.autologinUser = "root";       # serial console convenience

            users.users.conrad = { isNormalUser = true; initialPassword = "conrad"; };
            users.users.stcol = { isSystemUser = true; group = "stcol"; };
            users.groups.stcol = { };

            services.timekpr = { enable = true; adminUsers = [ "stcol" ]; };
            services.nats = { enable = true; jetstream = true; };

            # headless "graphical" session so timekpr tracks conrad (see section 2)
            security.pam.services.timekpr-session.startSession = true;
            systemd.services.monitored-session = {
              wantedBy = [ "multi-user.target" ];
              after = [ "systemd-user-sessions.service" "systemd-logind.service" ];
              environment = { XDG_SESSION_TYPE = "wayland"; XDG_SESSION_CLASS = "user"; };
              serviceConfig = {
                User = "conrad";
                PAMName = "timekpr-session";
                ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
                Restart = "always";
              };
            };

            systemd.services.screentime-collector = {
              wantedBy = [ "multi-user.target" ];
              after = [ "nats.service" "timekpr.service" "monitored-session.service" ];
              serviceConfig = {
                ExecStart = "${screentime-collector}/bin/screentime-collector -n nats://localhost:4222 -u conrad --hostname test-vm";
                User = "stcol";
                Group = "stcol";
                Restart = "on-failure";   # needed: panics until timekpr has created conrad's files
                RestartSec = "5s";
              };
            };
          })
        ];
      };
    };
}
```

Usage: `nix run .#test-vm`. The host then reaches NATS at
`nats://localhost:4222` (`nats sub 'time.obs.>'`) and SSH via
`ssh -p 2222 root@localhost`. Exit with `poweroff` in the guest or `Ctrl-a x`.

Feasibility check done: an equivalent module set (timekpr, nats+jetstream,
the PAM session service, forwardPorts, `diskImage = null`) evaluates cleanly
against the pinned nixpkgs, and `system.build.vm` instantiates. **Nothing was
built or booted.** That is the spike ticket.

## Open risks for the spike

1. Whether logind accepts a seatless `wayland` session from a service, and
   whether timekpr then logs "set up for tracking" and accrues time. The
   fallback is described in section 2.
2. The collector panics until the files exist. That is fine with
   `Restart=on-failure`, but the web-UI work may want to handle
   `result != 0` gracefully instead.
3. When time runs out, timekpr's default lockout terminates the session.
   `Restart=always` brings it back, and timekpr kills it again, which can
   loop. Give the test user a generous limit, or use this deliberately to test
   lockout.
