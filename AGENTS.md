## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on `thomaschrstnsn/screentime`, using the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Uses the five default triage labels (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

## Layout

A Cargo workspace with one crate per component: `src/collector` (`screentime-collector`) and `src/web` (`screentime-web`). The flake, `module.nix`, `test-vm/` and docs live at the root. Run `cargo test --workspace`, and `nix build .#screentime-collector .#screentime-web` when changing the build.
