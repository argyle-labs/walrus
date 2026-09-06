//! Dynamic (subprocess) entrypoint for the walrus plugin.
//!
//! walrus contributes a single `diagnostics` domain backend (no `walrus.` tool
//! surface). The typed [`Plugin`] builder advertises the backend and routes
//! `diagnose`/`repair` ops through [`walrus::checks::WalrusDiagnostics`]; the
//! toolkit's `diagnostics::dispatch_op` handles op routing + arg codec.

plugin_toolkit::instrument::bootstrap!();

use plugin_toolkit::plugin::Plugin;

fn main() -> plugin_toolkit::anyhow::Result<()> {
    Plugin::named("walrus")
        .version(env!("CARGO_PKG_VERSION"))
        .diagnostics(walrus::checks::WalrusDiagnostics)
        .serve()
}
