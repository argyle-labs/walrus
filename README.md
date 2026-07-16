# walrus

Opinionated, reproducible **macOS workstation** provisioning — the Homebrew
toolchain (formulae + casks), Node, and Claude Code — as an **orca plugin**.
The macOS counterpart to [raccoon](https://github.com/argyle-labs/raccoon)
(Linux desktop).

> Working name — `walrus` may be renamed before first release.

## As an orca diagnostics plugin

walrus is a Rust subprocess plugin registering a provider in orca's `diagnostics`
capability domain. Running on the Mac it provisions, it emits typed `Finding`s
(with optional repairs) over orca's MCP / CLI / REST:

```bash
orca diagnostics diagnose                                  # what's installed / missing
orca diagnostics repair --provider walrus --repair-id formulae
```

Checks (each a typed `Finding` + optional `Repair`):

| Check | Repair |
|-------|--------|
| **homebrew** | prints the official install command (interactive; non-automatic) |
| **formulae** | `brew install` the missing base formulae (nvm, direnv, openjdk, libpq, git, fastfetch, eza, bat) |
| **casks** | `brew install --cask` the missing casks (1password-cli, google-cloud-sdk, iterm2) |
| **node** | prints the nvm + Node LTS command (needs a shell to source nvm) |
| **claude-code** | `npm install -g @anthropic-ai/claude-code` |
| **disk-cleanup** | reclaims regenerable caches (`brew cleanup -s`, `npm cache clean`, then removes sccache/Yarn/playwright/DerivedData/npx/cargo cache dirs) — Warn over a 5 GiB threshold |
| **spotlight-nfs** | creates a `.metadata_never_index` marker in high-churn write paths so Spotlight skips them (prevents the watchdog reset described below) |
| **launchd-audit** | lists user LaunchAgents + login items to review (informational; no auto-repair) |
| **power-sleep** | flags `pmset` values worth tuning for an always-on node (informational) |

Brew repairs are non-privileged (no `sudo`) and automatic; the two that need an
interactive shell (`homebrew`, `node`) print the exact command instead of
running it. `disk-cleanup` and `spotlight-nfs` are automatic and non-privileged;
`launchd-audit` and `power-sleep` are informational (the disable/tune actions
require operator judgment or `sudo`, so they surface the exact command rather
than running it). Every check no-ops off macOS, so `diagnose` stays well-formed
on any CI/build host.

## Maintenance runbook

### Spotlight vs. high-churn write paths (the watchdog root cause)

A long-running daemon that writes constantly to a directory can make the macOS
Spotlight indexer (`mds`/`mds_stores`) loop indefinitely trying to re-index the
churning files. On a machine that also **network-exports that directory to other
hosts**, the indexing load compounds until the system watchdog forces a reset.

The fix is to tell Spotlight to skip the directory by dropping an empty
`.metadata_never_index` marker at its root, then rebuilding the index:

```bash
touch ~/<high-churn-dir>/.metadata_never_index
sudo mdutil -E /                         # rebuild the volume index once
```

The **spotlight-nfs** check detects a monitored high-churn path missing this
marker and its repair creates it — the highest-value check here, because it
prevents the watchdog recurrence on every node.

### Aggressive disk reclaim

The **disk-cleanup** repair reclaims space from regenerable locations only —
nothing here holds unique state:

| Category | Reclaimed by |
|----------|--------------|
| Homebrew download/bottle cache | `brew cleanup -s` |
| npm / npx package cache | `npm cache clean --force`, remove `~/.npm/_npx` |
| Compiler caches | remove `~/Library/Caches/Mozilla.sccache` |
| JS toolchain caches | remove `~/Library/Caches/{Yarn,ms-playwright,node-gyp}` |
| Xcode build artifacts | remove `~/Library/Developer/Xcode/DerivedData`, simulator caches |
| Rust registry unpack/cache | remove `~/.cargo/registry/{src,cache}` (keep `index`) |

Because `rm -rf` is commonly blocked by shell guards, the manual procedure is to
`mv` a target into a staging directory, confirm the size, then delete the stage
with `find <stage> -depth -delete`. **Never** remove `target/` or `node_modules/`
inside an active repo without confirming they are regenerable, and never delete a
checkout with unpushed commits or a dirty working tree.

## Build

```bash
cargo build --release                                       # host
cargo zigbuild --release --target aarch64-apple-darwin      # cross for Apple Silicon
# install the resulting binary via orca's plugin install path
```

## Pre-orca seed

On a brand-new Mac, orca isn't installed yet — the dotfiles `setup.sh` is the
thin seed that installs Homebrew + orca, after which everything above is done
over orca. walrus is the steady-state / repeatable path.

## License

MIT — see [LICENSE](LICENSE).
