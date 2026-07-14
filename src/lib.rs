//! walrus — macOS workstation provisioning diagnostics/repair plugin for orca.
//!
//! A backend-only plugin contributing one `diagnostics` domain backend (no
//! `walrus.` tool surface). Running on the macOS box it provisions, it checks the
//! Homebrew toolchain (formulae + casks), Node, and Claude Code, and emits typed
//! [`plugin_toolkit::contract::diagnostics::Finding`]s — each with an optional
//! repair the operator can run via `orca diagnostics repair`.
//!
//! Detection + remediation lives in [`checks`]; [`registration`] wires it to the
//! diagnostics domain. Served over the orca socket from the `walrus` binary.

pub mod checks;
pub mod registration;

/// Registry name this plugin uses across the diagnostics domain.
pub const PROVIDER: &str = "walrus";
