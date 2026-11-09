# NixOS module for the collector, exported by the flake as nixosModules.default.
# The flake passes in its own packages; `package` defaults to the one for the
# host system.
packages:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.screentime-collector;
  user = "stcol";
in
{
  options.services.screentime-collector = {
    enable = lib.mkEnableOption "the screentime collector, reporting Timekpr-nExT observations to NATS";

    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "screentime-collector.packages.\${system}.default";
      description = "The screentime-collector package to run.";
    };

    natsUrl = lib.mkOption {
      type = lib.types.str;
      example = "nats://localhost:4222";
      description = "URL of the NATS server to publish observations to and serve time adjustments from.";
    };

    monitoredUsers = lib.mkOption {
      type = lib.types.nonEmptyListOf lib.types.str;
      example = [ "junior" ];
      description = "Local accounts whose screen time Timekpr-nExT limits and the collector reports on.";
    };

    hostname = lib.mkOption {
      type = lib.types.str;
      default = config.networking.hostName;
      defaultText = lib.literalExpression "config.networking.hostName";
      description = "Hostname the collector reports under, used in NATS subjects.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = config.services.timekpr.enable;
        message = "services.screentime-collector requires services.timekpr.enable = true.";
      }
    ];

    users.users.${user} = {
      isSystemUser = true;
      group = user;
    };
    users.groups.${user} = { };

    # adminUsers puts the user in the timekpr group, which the D-Bus policy requires.
    services.timekpr.adminUsers = [ user ];

    systemd.services.screentime-collector = {
      description = "Screentime collector";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      # Never add After=timekpr.service: nixpkgs orders timekpr after
      # multi-user.target, so that would be an ordering cycle and systemd would
      # drop the collector at boot. Restart covers timekpr not being ready yet.
      after = [ "network-online.target" ];
      environment.RUST_LOG = lib.mkDefault "info";
      serviceConfig = {
        ExecStart = lib.escapeShellArgs [
          (lib.getExe cfg.package)
          "-n"
          cfg.natsUrl
          "-u"
          (lib.concatStringsSep "," cfg.monitoredUsers)
          "--hostname"
          cfg.hostname
        ];
        User = user;
        Group = user;
        Restart = "on-failure";
        RestartSec = "5s";

        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectHome = true;
        ProtectSystem = "full";
        LimitNPROC = 512;
        LimitNOFILE = 1048576;
      };
    };
  };
}
