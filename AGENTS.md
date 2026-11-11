## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on `thomaschrstnsn/screentime`, using the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Layout

A Cargo workspace with one crate per component: `src/collector` (`screentime-collector`) and `src/web` (`screentime-web`), plus `src/contract` (`screentime-contract`), the NATS subjects, stream names, message types and JSON examples both depend on. The flake, the NixOS modules (`collector-module.nix` for the collector, `web-module.nix` for web), `checks/`, `test-vm/` and docs live at the root. Run `cargo test --workspace`, `nix build .#screentime-collector .#screentime-web` when changing the build, and `nix flake check` when changing a NixOS module.
