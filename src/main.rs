//! Dynamic (subprocess) entrypoint for the walrus plugin.
//!
//! The toolkit's `serve_tool_plugin!` emits `fn main`, serving this plugin over
//! the orca socket. Hybrid arm: an (empty) `walrus.` tool surface plus the
//! `diagnostics` domain backend. `target_compat` is empty — walrus provisions
//! whatever local macOS box it runs on, so there's no external service version
//! to gate against. The backend descriptor comes from
//! [`walrus::registration::backends_json`] and `walrus.__diag.*` ops route through
//! [`walrus::registration::backend_dispatch`].

plugin_toolkit::serve_tool_plugin! {
    name: "walrus",
    target_compat: "",
    backends: walrus::registration::backends_json(),
    backend_dispatch: walrus::registration::backend_dispatch,
}
