{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11"; # >= 25.11 for services.timekpr

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      lib = nixpkgs.lib;

      # Builds one workspace member. Each crate builds from the whole workspace
      # (one Cargo.lock), so both get the same source.
      crate =
        pname: extra:
        pkgs.rustPlatform.buildRustPackage (
          {
            inherit pname;
            version = (lib.importTOML ./Cargo.toml).workspace.package.version;
            src = lib.fileset.toSource {
              root = ./.;
              fileset = lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./src # includes src/web/static: index.html is embedded via include_str!
              ];
            };
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = [ "-p" pname ];
            cargoTestFlags = [ "-p" pname ];
            meta.mainProgram = pname;
          }
          // extra
        );

      screentime-collector = crate "screentime-collector" { };
      screentime-web = crate "screentime-web" {
        meta = {
          description = "screentime-web";
          license = lib.licenses.mit;
          mainProgram = "screentime-web";
        };
      };
    in
    {
      packages.${system} = {
        inherit screentime-collector screentime-web;
        test-vm = self.nixosConfigurations.test-vm.config.system.build.vm;
      };

      nixosModules.collector = import ./module.nix self.packages;
      # Kept so existing `nixosModules.default` imports keep working.
      nixosModules.default = self.nixosModules.collector;

      # Manual integration test VM, driven by the scripts in ./test-vm.
      nixosConfigurations.test-vm = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix"
          self.nixosModules.collector
          ./test-vm/configuration.nix
        ];
      };
    };
}
