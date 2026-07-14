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

Brew repairs are non-privileged (no `sudo`) and automatic; the two that need an
interactive shell (`homebrew`, `node`) print the exact command instead of
running it. Every check no-ops off macOS, so `diagnose` stays well-formed on any
CI/build host.

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
