# NixOS test VM: Timekpr-nExT tracking a monitored user, NATS with JetStream,
# and the collector from this flake's NixOS module, built from this working
# tree. See ./README.md.
{ pkgs, ... }:
let
  monitoredUser = "junior";
in
{
  system.stateVersion = "25.11";
  networking.hostName = "test-vm";

  virtualisation = {
    graphics = false;
    memorySize = 1024;
    cores = 2;
    # The disk image path comes from NIX_DISK_IMAGE, set by ./start.
    forwardPorts = [
      {
        from = "host";
        host.address = "127.0.0.1";
        host.port = 14222;
        guest.port = 4222;
      }
      {
        from = "host";
        host.address = "127.0.0.1";
        host.port = 12222;
        guest.port = 22;
      }
    ];
  };
  networking.firewall.allowedTCPPorts = [ 4222 ];

  # The guest clock is moved by ./clock, so nothing may pull it back.
  services.timesyncd.enable = false;

  # Root logs in over SSH with the key ./start passes in as an SMBIOS
  # credential, so no key lives in the repo or the store.
  services.openssh = {
    enable = true;
    settings.PermitRootLogin = "prohibit-password";
  };
  systemd.services.root-authorized-keys = {
    wantedBy = [ "multi-user.target" ];
    before = [ "sshd.service" ];
    serviceConfig = {
      Type = "oneshot";
      ImportCredential = "test-vm.authorized-keys";
    };
    script = ''
      install -D -m 600 "$CREDENTIALS_DIRECTORY/test-vm.authorized-keys" /root/.ssh/authorized_keys
    '';
  };
  # Serial console convenience for `./start --foreground`.
  users.users.root.initialPassword = "root";
  services.getty.autologinUser = "root";

  users.users.${monitoredUser} = {
    isNormalUser = true;
    initialPassword = monitoredUser;
  };
  services.timekpr.enable = true;
  services.nats = {
    enable = true;
    jetstream = true;
  };

  # Timekpr only tracks users with an active x11/wayland/mir logind session.
  # This fakes one: no compositor, just a PAM session tagged as wayland.
  # A seatless session is always active and never idle.
  security.pam.services.timekpr-session.startSession = true;
  systemd.services.monitored-session = {
    wantedBy = [ "multi-user.target" ];
    after = [
      "systemd-user-sessions.service"
      "systemd-logind.service"
    ];
    environment = {
      XDG_SESSION_TYPE = "wayland";
      XDG_SESSION_CLASS = "user";
    };
    serviceConfig = {
      User = monitoredUser;
      PAMName = "timekpr-session";
      ExecStart = "${pkgs.coreutils}/bin/sleep infinity";
      Restart = "always"; # timekpr terminates the session when time runs out
    };
  };

  environment.systemPackages = [ pkgs.natscli ];

  services.screentime-collector = {
    enable = true;
    natsUrl = "nats://localhost:4222";
    monitoredUsers = [ monitoredUser ];
  };
  # On top of the module's ordering, so the first run finds NATS and a session.
  systemd.services.screentime-collector.after = [
    "nats.service"
    "monitored-session.service"
  ];
}
