# NixOS module for web, exported by the flake as nixosModules.web.
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
  cfg = config.services.screentime-web;
  user = "stweb";
  # IPv6 addresses need brackets in a socket address.
  bindHost =
    if lib.hasInfix ":" cfg.listenAddress then "[${cfg.listenAddress}]" else cfg.listenAddress;
in
{
  options.services.screentime-web = {
    enable = lib.mkEnableOption "screentime-web, the dashboard showing observations from NATS and making time adjustments";

    package = lib.mkOption {
      type = lib.types.package;
      default = packages.${pkgs.stdenv.hostPlatform.system}.screentime-web;
      defaultText = lib.literalExpression "screentime.packages.\${system}.screentime-web";
      description = "The screentime-web package to run.";
    };

    natsUrl = lib.mkOption {
      type = lib.types.str;
      example = "nats://localhost:4222";
      description = "URL of the NATS server to read observations from and send time adjustments to.";
    };

    listenAddress = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      example = "0.0.0.0";
      description = "Address the HTTP server listens on.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 6767; # the binary defaults to 3000; production serves on 6767
      description = "Port the HTTP server listens on.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Whether to open `port` in the firewall.";
    };

    cloudflareAccess = lib.mkOption {
      type = lib.types.nullOr (
        lib.types.submodule {
          options = {
            team = lib.mkOption {
              type = lib.types.str;
              description = "Cloudflare Access team name (`CF_ACCESS_TEAM`).";
            };
            aud = lib.mkOption {
              type = lib.types.str;
              description = "Audience tag of the Cloudflare Access application (`CF_ACCESS_AUD`).";
            };
          };
        }
      );
      default = null;
      description = ''
        Cloudflare Access application to validate requests against. When null,
        every request is anonymous and nobody can make time adjustments.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    warnings =
      lib.optional
        (
          cfg.openFirewall
          && lib.elem cfg.listenAddress [
            "127.0.0.1"
            "::1"
            "localhost"
          ]
        )
        "services.screentime-web.openFirewall opens port ${toString cfg.port}, but web only listens on ${cfg.listenAddress}.";

    users.users.${user} = {
      isSystemUser = true;
      group = user;
    };
    users.groups.${user} = { };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];

    systemd.services.screentime-web = {
      description = "Screentime web";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      environment = {
        RUST_LOG = lib.mkDefault "info";
      }
      // lib.optionalAttrs (cfg.cloudflareAccess != null) {
        CF_ACCESS_TEAM = cfg.cloudflareAccess.team;
        CF_ACCESS_AUD = cfg.cloudflareAccess.aud;
      };
      serviceConfig = {
        ExecStart = lib.escapeShellArgs [
          (lib.getExe cfg.package)
          "--nats-url"
          cfg.natsUrl
          "--bind"
          "${bindHost}:${toString cfg.port}"
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
