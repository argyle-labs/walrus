//! Detection + remediation for a macOS workstation. Each check returns an
//! optional [`Finding`]; each repair id maps to a concrete action. Everything is
//! synchronous (short `brew`/`which` commands); the core proxy runs it on a
//! blocking pool.
//!
//! walrus only provisions macOS — every check no-ops off macOS so the plugin's
//! `diagnose` stays well-formed on any build/CI host.

use std::path::PathBuf;
use std::process::Command;

use plugin_toolkit::contract::diagnostics::{
    DiagnoseArgs, Finding, RepairArgs, RepairOutcome, RepairSpec, Severity,
};
use plugin_toolkit::serde_json;

/// Homebrew formulae (the setup.sh base toolchain).
const FORMULAE: &[&str] = &[
    "nvm",
    "direnv",
    "openjdk",
    "libpq",
    "git",
    "fastfetch",
    "eza",
    "bat",
];
/// Homebrew casks.
const CASKS: &[&str] = &["1password-cli", "google-cloud-sdk", "iterm2"];

// ── diagnose ─────────────────────────────────────────────────────────────────

/// Run every check and return the findings as JSON (`Vec<Finding>`).
pub fn diagnose(args_json: &str) -> Result<String, String> {
    let _: DiagnoseArgs = if args_json.trim().is_empty() {
        DiagnoseArgs::default()
    } else {
        serde_json::from_str(args_json).unwrap_or_default()
    };
    let findings: Vec<Finding> = [
        check_homebrew(),
        check_formulae(),
        check_casks(),
        check_node(),
        check_claude_code(),
        check_disk_cleanup(),
        check_spotlight_nfs(),
        check_launchd_audit(),
        check_power_sleep(),
    ]
    .into_iter()
    .flatten()
    .collect();
    serde_json::to_string(&findings).map_err(|e| format!("encode findings: {e}"))
}

fn finding(
    id: &str,
    severity: Severity,
    title: &str,
    detail: String,
    repair: Option<RepairSpec>,
) -> Finding {
    Finding {
        id: id.to_string(),
        provider: crate::PROVIDER.to_string(),
        severity,
        title: title.to_string(),
        detail,
        repair,
    }
}

fn repair_spec(id: &str, description: &str, automatic: bool, privileged: bool) -> RepairSpec {
    RepairSpec {
        id: id.to_string(),
        description: description.to_string(),
        automatic,
        privileged,
        delegate: None,
    }
}

// ── checks ───────────────────────────────────────────────────────────────────

fn check_homebrew() -> Option<Finding> {
    if !is_macos() {
        return None;
    }
    if which("brew").is_some() {
        return Some(finding(
            "homebrew",
            Severity::Ok,
            "Homebrew installed",
            "brew is on PATH".to_string(),
            None,
        ));
    }
    Some(finding(
        "homebrew",
        Severity::Warn,
        "Homebrew not installed",
        "the package manager for everything below is missing".to_string(),
        // Non-automatic: the installer is an interactive curl|bash; we print it.
        Some(repair_spec(
            "homebrew",
            "Install Homebrew (prints the official install command)",
            false,
            false,
        )),
    ))
}

fn check_formulae() -> Option<Finding> {
    if !is_macos() || which("brew").is_none() {
        return None;
    }
    let have = brew_list(&["--formula", "-1"]);
    let miss: Vec<&str> = FORMULAE
        .iter()
        .copied()
        .filter(|f| !have.contains(&f.to_string()))
        .collect();
    if miss.is_empty() {
        return Some(finding(
            "formulae",
            Severity::Ok,
            "Brew formulae present",
            format!("{} installed", FORMULAE.join(", ")),
            None,
        ));
    }
    Some(finding(
        "formulae",
        Severity::Warn,
        "Brew formulae missing",
        format!("missing: {}", miss.join(", ")),
        Some(repair_spec(
            "formulae",
            "brew install the missing formulae",
            true,
            false,
        )),
    ))
}

fn check_casks() -> Option<Finding> {
    if !is_macos() || which("brew").is_none() {
        return None;
    }
    let have = brew_list(&["--cask", "-1"]);
    let miss: Vec<&str> = CASKS
        .iter()
        .copied()
        .filter(|c| !have.contains(&c.to_string()))
        .collect();
    if miss.is_empty() {
        return Some(finding(
            "casks",
            Severity::Ok,
            "Brew casks present",
            format!("{} installed", CASKS.join(", ")),
            None,
        ));
    }
    Some(finding(
        "casks",
        Severity::Info,
        "Brew casks missing",
        format!("missing: {}", miss.join(", ")),
        Some(repair_spec(
            "casks",
            "brew install --cask the missing casks",
            true,
            false,
        )),
    ))
}

fn check_node() -> Option<Finding> {
    if !is_macos() {
        return None;
    }
    if which("node").is_some() {
        return Some(finding(
            "node",
            Severity::Ok,
            "Node present",
            "node is on PATH".to_string(),
            None,
        ));
    }
    Some(finding(
        "node",
        Severity::Warn,
        "Node not installed",
        "install nvm (formula) then a Node LTS".to_string(),
        // Non-automatic: nvm must be sourced in the shell; we print guidance.
        Some(repair_spec(
            "node",
            "Install Node LTS via nvm (prints the command)",
            false,
            false,
        )),
    ))
}

fn check_claude_code() -> Option<Finding> {
    if !is_macos() {
        return None;
    }
    if which("claude").is_some() {
        return Some(finding(
            "claude-code",
            Severity::Ok,
            "Claude Code installed",
            "claude is on PATH".to_string(),
            None,
        ));
    }
    Some(finding(
        "claude-code",
        Severity::Info,
        "Claude Code not installed",
        "the @anthropic-ai/claude-code CLI is missing".to_string(),
        Some(repair_spec(
            "claude-code",
            "npm install -g @anthropic-ai/claude-code",
            true,
            false,
        )),
    ))
}

// ── maintenance checks ─────────────────────────────────────────────────────────

/// Regenerable cache/build-artifact locations, relative to `$HOME`. Every one of
/// these is rebuilt on demand by the owning tool (browsers, package managers,
/// compilers, updaters), so reclaiming them is non-destructive.
const CRUFT_PATHS: &[&str] = &[
    "Library/Caches/Mozilla.sccache",
    "Library/Caches/Yarn",
    "Library/Caches/ms-playwright",
    "Library/Caches/node-gyp",
    "Library/Developer/Xcode/DerivedData",
    "Library/Developer/CoreSimulator/Caches",
    ".npm/_npx",
    ".cargo/registry/src",
    ".cargo/registry/cache",
];
/// Warn when reclaimable cruft exceeds this many bytes (default 5 GiB).
const CRUFT_WARN_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// High-churn write paths (relative to `$HOME`) that, when they live on a
/// network-exported volume, cause the macOS Spotlight indexer to loop on the
/// constant writes — which has produced a watchdog reset. A
/// `.metadata_never_index` marker in the directory tells Spotlight to skip it.
const NEVER_INDEX_PATHS: &[&str] = &[".orca"];

fn check_disk_cleanup() -> Option<Finding> {
    if !is_macos() {
        return None;
    }
    let home = home_dir()?;
    let total: u64 = CRUFT_PATHS
        .iter()
        .map(|rel| dir_size(&home.join(rel)))
        .sum();
    if total < CRUFT_WARN_BYTES {
        return Some(finding(
            "disk-cleanup",
            Severity::Ok,
            "Reclaimable cruft below threshold",
            format!(
                "{} of regenerable caches (< {} threshold)",
                human_bytes(total),
                human_bytes(CRUFT_WARN_BYTES)
            ),
            None,
        ));
    }
    Some(finding(
        "disk-cleanup",
        Severity::Warn,
        "Reclaimable disk cruft over threshold",
        format!(
            "{} of regenerable caches (sccache/Yarn/playwright/DerivedData/npx/cargo); \
             threshold {}",
            human_bytes(total),
            human_bytes(CRUFT_WARN_BYTES)
        ),
        Some(repair_spec(
            "disk-cleanup",
            "Reclaim regenerable caches (brew cleanup -s, npm cache clean, remove staged cruft dirs)",
            true,
            false,
        )),
    ))
}

fn check_spotlight_nfs() -> Option<Finding> {
    if !is_macos() {
        return None;
    }
    let home = home_dir()?;
    let missing: Vec<&str> = NEVER_INDEX_PATHS
        .iter()
        .copied()
        .filter(|rel| {
            let dir = home.join(rel);
            dir.is_dir() && !dir.join(".metadata_never_index").exists()
        })
        .collect();
    if missing.is_empty() {
        return Some(finding(
            "spotlight-nfs",
            Severity::Ok,
            "High-churn paths excluded from Spotlight",
            "every monitored write path carries a .metadata_never_index marker".to_string(),
            None,
        ));
    }
    Some(finding(
        "spotlight-nfs",
        Severity::Warn,
        "High-churn path lacks a Spotlight exclusion marker",
        format!(
            "missing .metadata_never_index in: {}. On a network-exported volume the \
             Spotlight indexer can loop on constant writes here and trigger a watchdog reset.",
            missing.join(", ")
        ),
        Some(repair_spec(
            "spotlight-nfs",
            "Create a .metadata_never_index marker in each high-churn path",
            true,
            false,
        )),
    ))
}

fn check_launchd_audit() -> Option<Finding> {
    if !is_macos() {
        return None;
    }
    let home = home_dir()?;
    let agents = list_dir(&home.join("Library/LaunchAgents"), "plist");
    let items = login_items();
    let mut parts = Vec::new();
    if !agents.is_empty() {
        parts.push(format!("user LaunchAgents: {}", agents.join(", ")));
    }
    if !items.is_empty() {
        parts.push(format!("login items: {}", items.join(", ")));
    }
    if parts.is_empty() {
        return Some(finding(
            "launchd-audit",
            Severity::Ok,
            "No user launch agents or login items",
            "nothing user-scoped is set to auto-start".to_string(),
            None,
        ));
    }
    // Informational only: whether any given agent/item is disable-worthy is the
    // operator's call, so there is no automatic repair.
    Some(finding(
        "launchd-audit",
        Severity::Info,
        "Auto-start agents and login items present",
        format!(
            "review for disable-worthy entries — {}. Disable an agent with \
             `launchctl bootout gui/$UID <label>` and remove login items in System Settings.",
            parts.join("; ")
        ),
        None,
    ))
}

fn check_power_sleep() -> Option<Finding> {
    if !is_macos() {
        return None;
    }
    let out = match run("pmset", &["-g", "custom"]) {
        Ok(o) => o,
        Err(_) => return None,
    };
    let get = |key: &str| -> Option<i64> {
        out.lines()
            .map(str::trim)
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<i64>().ok())
    };
    // A node meant to stay reachable wants sleep disabled and powernap off.
    let sleep = get("sleep");
    let powernap = get("powernap");
    let mut tunables = Vec::new();
    if let Some(s) = sleep.filter(|&s| s != 0) {
        tunables.push(format!(
            "sleep={s} (a mesh node wants 0; `sudo pmset -a sleep 0`)"
        ));
    }
    if powernap.is_some_and(|p| p != 0) {
        tunables.push("powernap=1 (`sudo pmset -a powernap 0`)".to_string());
    }
    if tunables.is_empty() {
        return Some(finding(
            "power-sleep",
            Severity::Ok,
            "Power settings suitable for an always-on node",
            "sleep and powernap are already tuned".to_string(),
            None,
        ));
    }
    // Informational: `pmset -a` needs sudo, so we surface the tunables rather
    // than mutating power policy automatically.
    Some(finding(
        "power-sleep",
        Severity::Info,
        "Power settings worth tuning",
        format!("consider: {}", tunables.join("; ")),
        None,
    ))
}

// ── repair ───────────────────────────────────────────────────────────────────

/// Run one repair by id and return a [`RepairOutcome`] as JSON.
pub fn repair(args_json: &str) -> Result<String, String> {
    let args: RepairArgs =
        serde_json::from_str(args_json).map_err(|e| format!("invalid repair args: {e}"))?;
    let (ok, message) = match args.repair_id.as_str() {
        "homebrew" => repair_homebrew(),
        "formulae" => repair_formulae(),
        "casks" => repair_casks(),
        "node" => repair_node(),
        "claude-code" => repair_claude_code(),
        "disk-cleanup" => repair_disk_cleanup(),
        "spotlight-nfs" => repair_spotlight_nfs(),
        other => (false, format!("walrus has no repair '{other}'")),
    };
    let outcome = RepairOutcome {
        id: args.repair_id,
        provider: crate::PROVIDER.to_string(),
        ok,
        message,
    };
    serde_json::to_string(&outcome).map_err(|e| format!("encode outcome: {e}"))
}

fn repair_homebrew() -> (bool, String) {
    if which("brew").is_some() {
        return (true, "Homebrew already installed".to_string());
    }
    (
        false,
        "run: /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\""
            .to_string(),
    )
}

fn repair_formulae() -> (bool, String) {
    if which("brew").is_none() {
        return (
            false,
            "install Homebrew first (repair 'homebrew')".to_string(),
        );
    }
    let have = brew_list(&["--formula", "-1"]);
    let miss: Vec<&str> = FORMULAE
        .iter()
        .copied()
        .filter(|f| !have.contains(&f.to_string()))
        .collect();
    if miss.is_empty() {
        return (true, "all formulae already installed".to_string());
    }
    let mut args = vec!["install"];
    args.extend_from_slice(&miss);
    match run("brew", &args) {
        Ok(_) => (true, format!("installed: {}", miss.join(", "))),
        Err(e) => (
            false,
            format!(
                "brew install failed ({e}); run: brew install {}",
                miss.join(" ")
            ),
        ),
    }
}

fn repair_casks() -> (bool, String) {
    if which("brew").is_none() {
        return (
            false,
            "install Homebrew first (repair 'homebrew')".to_string(),
        );
    }
    let have = brew_list(&["--cask", "-1"]);
    let miss: Vec<&str> = CASKS
        .iter()
        .copied()
        .filter(|c| !have.contains(&c.to_string()))
        .collect();
    if miss.is_empty() {
        return (true, "all casks already installed".to_string());
    }
    let mut args = vec!["install", "--cask"];
    args.extend_from_slice(&miss);
    match run("brew", &args) {
        Ok(_) => (true, format!("installed: {}", miss.join(", "))),
        Err(e) => (
            false,
            format!(
                "brew install --cask failed ({e}); run: brew install --cask {}",
                miss.join(" ")
            ),
        ),
    }
}

fn repair_node() -> (bool, String) {
    if which("node").is_some() {
        return (true, "node already installed".to_string());
    }
    (
        false,
        "run in your shell: brew install nvm && source $(brew --prefix)/opt/nvm/nvm.sh && nvm install --lts"
            .to_string(),
    )
}

fn repair_claude_code() -> (bool, String) {
    if which("claude").is_some() {
        return (true, "Claude Code already installed".to_string());
    }
    if which("npm").is_none() {
        return (false, "install Node first (repair 'node')".to_string());
    }
    match run("npm", &["install", "-g", "@anthropic-ai/claude-code"]) {
        Ok(_) => (true, "installed @anthropic-ai/claude-code".to_string()),
        Err(e) => (
            false,
            format!("npm install failed ({e}); run: npm install -g @anthropic-ai/claude-code"),
        ),
    }
}

fn repair_disk_cleanup() -> (bool, String) {
    let home = match home_dir() {
        Some(h) => h,
        None => return (false, "cannot resolve $HOME".to_string()),
    };
    let mut freed: u64 = 0;
    let mut done = Vec::new();

    // Tool-native cache purges first (they know what is safe to drop).
    if which("brew").is_some() {
        run("brew", &["cleanup", "-s"]).ok();
        done.push("brew cleanup -s".to_string());
    }
    if which("npm").is_some() {
        run("npm", &["cache", "clean", "--force"]).ok();
        done.push("npm cache clean".to_string());
    }

    // Then remove the regenerable cruft directories.
    for rel in CRUFT_PATHS {
        let dir = home.join(rel);
        if !dir.is_dir() {
            continue;
        }
        let sz = dir_size(&dir);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {
                freed += sz;
                done.push(format!("removed {rel} ({})", human_bytes(sz)));
            }
            Err(e) => done.push(format!("skip {rel}: {e}")),
        }
    }

    (
        true,
        format!("reclaimed ~{} — {}", human_bytes(freed), done.join(", ")),
    )
}

fn repair_spotlight_nfs() -> (bool, String) {
    let home = match home_dir() {
        Some(h) => h,
        None => return (false, "cannot resolve $HOME".to_string()),
    };
    let mut created = Vec::new();
    for rel in NEVER_INDEX_PATHS {
        let dir = home.join(rel);
        if !dir.is_dir() {
            continue;
        }
        let marker = dir.join(".metadata_never_index");
        if marker.exists() {
            continue;
        }
        match std::fs::File::create(&marker) {
            Ok(_) => created.push(rel.to_string()),
            Err(e) => return (false, format!("failed to create marker in {rel}: {e}")),
        }
    }
    if created.is_empty() {
        (true, "all high-churn paths already excluded".to_string())
    } else {
        (
            true,
            format!(
                "created .metadata_never_index in: {}. Rebuild the index with \
                 `sudo mdutil -E <volume>` if churn persists.",
                created.join(", ")
            ),
        )
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Recursive on-disk size of `dir` in bytes (0 if it does not exist).
fn dir_size(dir: &std::path::Path) -> u64 {
    let mut total = 0;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// File names in `dir` with the given extension (empty if the dir is absent).
fn list_dir(dir: &std::path::Path, ext: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some(ext))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

/// Names of the current user's login items (empty on any failure).
fn login_items() -> Vec<String> {
    run(
        "osascript",
        &[
            "-e",
            "tell application \"System Events\" to get the name of every login item",
        ],
    )
    .map(|o| {
        o.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn is_macos() -> bool {
    std::env::consts::OS == "macos"
}

fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(bin))
        .find(|p| p.is_file())
}

/// `brew list <args>` → the set of names (one per line), or empty on failure.
fn brew_list(args: &[&str]) -> Vec<String> {
    let mut a = vec!["list"];
    a.extend_from_slice(args);
    run("brew", &a)
        .map(|o| {
            o.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn run(bin: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(bin)
        .args(args)
        .output()
        .map_err(|e| format!("spawn {bin}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr)
            .trim()
            .lines()
            .next()
            .unwrap_or("command failed")
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnose_emits_valid_json_array() {
        let out = diagnose("{}").expect("diagnose ok");
        let findings: Vec<Finding> = serde_json::from_str(&out).expect("valid findings json");
        for f in &findings {
            assert_eq!(f.provider, crate::PROVIDER);
        }
    }

    #[test]
    fn repair_unknown_id_reports_not_ok() {
        let out = repair(r#"{"provider":"walrus","repair_id":"nope"}"#).expect("encodes");
        let o: RepairOutcome = serde_json::from_str(&out).unwrap();
        assert!(!o.ok);
        assert!(o.message.contains("no repair"));
    }
}
