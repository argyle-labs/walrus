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

// ── helpers ──────────────────────────────────────────────────────────────────

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
