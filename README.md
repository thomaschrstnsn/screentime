# screentime

Observing and adjusting the screen time of monitored users on machines governed by [Timekpr-nExT](https://launchpad.net/timekpr-next), over NATS.

| Crate | What it does |
|---|---|
| [`src/collector`](src/collector/README.md) | Runs on each monitored host: publishes Timekpr-nExT observations to NATS and serves time adjustments. |
| [`src/web`](src/web/README.md) | Web dashboard: shows observations live and lets adjusters make time adjustments. |

The domain language is in [`GLOSSARY.md`](GLOSSARY.md).

## Building

```bash
cargo test --workspace
nix build .#screentime-collector .#screentime-web
```

The flake also exports `nixosModules.collector` (also as `nixosModules.default`), `nixosModules.web`, a NixOS VM test of the web module (`nix flake check`) and a manual integration test VM, `packages.test-vm`, driven by the scripts in [`test-vm/`](test-vm/README.md).
