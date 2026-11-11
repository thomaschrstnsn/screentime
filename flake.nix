{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11"; # >= 25.11 for services.timekpr

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      lib = nixpkgs.lib;

      screentime-collector = pkgs.rustPlatform.buildRustPackage {
        pname = "screentime-collector";
        version = (lib.importTOML ./src/collector/Cargo.toml).package.version;
        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [
            ./Cargo.toml
            ./Cargo.lock
            ./src
          ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [ "-p" "screentime-collector" ];
        cargoTestFlags = [ "-p" "screentime-collector" ];
        meta.mainProgram = "screentime-collector";
      };
    in
    {
      packages.${system} = {
        default = screentime-collector;
        inherit screentime-collector;
        test-vm = self.nixosConfigurations.test-vm.config.system.build.vm;
      };

      nixosModules.default = import ./module.nix self.packages;

      # Manual integration test VM, driven by the scripts in ./test-vm.
      nixosConfigurations.test-vm = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix"
          self.nixosModules.default
          ./test-vm/configuration.nix
        ];
      };
    };
}
