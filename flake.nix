# Throwaway spike for "Spike: real setTimeLeft round-trip in the test VM" (#7).
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
              diskImage = null;
              memorySize = 1024;
              cores = 2;
              forwardPorts = [
                { from = "host"; host.port = 14222; guest.port = 4222; }
                { from = "host"; host.port = 12222; guest.port = 22; }
              ];
            };
            networking.firewall.allowedTCPPorts = [ 4222 ];

            services.openssh = { enable = true; settings.PermitRootLogin = "yes"; };
            users.users.root.initialPassword = "root";
            users.users.root.openssh.authorizedKeys.keys = [ "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJYxVntkgwzUJ64qrSvZIDrxlxc/rGKjNZJJEWbP0Fdi thomas@atlas" ];
            services.getty.autologinUser = "root";

            users.users.conrad = { isNormalUser = true; initialPassword = "conrad"; };
            users.users.stcol = { isSystemUser = true; group = "stcol"; };
            users.groups.stcol = { };

            services.timekpr = { enable = true; adminUsers = [ "stcol" ]; };
            services.nats = { enable = true; jetstream = true; };

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

            environment.systemPackages = [ pkgs.natscli ];

            systemd.services.screentime-collector = {
              wantedBy = [ "multi-user.target" ];
              after = [ "nats.service" "timekpr.service" "monitored-session.service" ];
              environment.RUST_LOG = "info";
              serviceConfig = {
                ExecStart = "${screentime-collector}/bin/screentime-collector -n nats://localhost:4222 -u conrad --hostname test-vm";
                User = "stcol";
                Group = "stcol";
                Restart = "on-failure";
                RestartSec = "5s";
              };
            };
          })
        ];
      };
    };
}
