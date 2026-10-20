//! `help` lists and describes the commands that can actually run, including
//! the ones an embedder registers.

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, BuiltinRegistry, ExecResult};
use std::sync::Arc;

/// A command whose hint needs JSON escaping.
struct Hinted;

#[async_trait]
impl Builtin for Hinted {
    fn llm_hint(&self) -> Option<&'static str> {
        Some("hinted: reads \"quoted\" text\\paths; no network")
    }

    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok("hinted\n"))
    }
}

/// A command without a hint.
struct Plain;

#[async_trait]
impl Builtin for Plain {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok("plain\n"))
    }
}

fn json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap_or_else(|e| panic!("{e}: {stdout:?}"))
}

#[tokio::test]
async fn help_lists_the_live_inventory() {
    let registry = BuiltinRegistry::new();
    let mut bash = Bash::builder()
        .builtin("hinted", Box::new(Hinted))
        .builtin_registry(registry.clone())
        .build();
    registry.insert("plain", Arc::new(Plain));

    let listing = bash.exec("help --list -s").await.unwrap();
    assert_eq!(listing.exit_code, 0);
    assert_eq!(
        listing.stdout.lines().collect::<Vec<_>>(),
        bash.builtin_names()
    );

    registry.remove("plain");
    let listing = bash.exec("help --list -s").await.unwrap();
    assert!(!listing.stdout.lines().any(|name| name == "plain"));
    let result = bash.exec("help plain").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("no help topics match"));
}

#[tokio::test]
async fn help_describes_embedder_commands_by_their_hint() {
    let registry = BuiltinRegistry::new();
    let mut bash = Bash::builder()
        .builtin("hinted", Box::new(Hinted))
        .builtin_registry(registry.clone())
        .build();
    registry.insert("plain", Arc::new(Plain));

    let hinted = bash.exec("help --json hinted").await.unwrap();
    assert_eq!(hinted.exit_code, 0, "{}", hinted.stderr);
    let value = json(&hinted.stdout);
    assert_eq!(value["name"], "hinted");
    assert_eq!(value["category"], "custom");
    assert_eq!(
        value["description"],
        "hinted: reads \"quoted\" text\\paths; no network"
    );

    let plain = bash.exec("help --json plain").await.unwrap();
    assert_eq!(plain.exit_code, 0, "{}", plain.stderr);
    assert_eq!(
        json(&plain.stdout)["description"],
        "No description available"
    );

    let long = bash.exec("help hinted").await.unwrap();
    assert!(long.stdout.contains("Usage: hinted\n"), "{}", long.stdout);
    assert!(
        long.stdout.contains("Category: custom\n"),
        "{}",
        long.stdout
    );
}

#[tokio::test]
async fn help_describes_an_override_by_the_command_that_runs() {
    // `ln` has a table entry; a registered `ln` replaces it, so its own hint
    // (or none) describes it, not bashkit's usage line.
    let mut bash = Bash::builder().builtin("ln", Box::new(Hinted)).build();
    let value = json(&bash.exec("help --json ln").await.unwrap().stdout);
    assert_eq!(value["category"], "custom");
    assert_eq!(value["usage"], "ln");
    assert_eq!(
        value["description"],
        "hinted: reads \"quoted\" text\\paths; no network"
    );

    // Without an override the table entry stands.
    let mut bash = Bash::new();
    let value = json(&bash.exec("help --json ln").await.unwrap().stdout);
    assert_eq!(value["category"], "files");
    assert_eq!(value["description"], "Create links");
}

#[tokio::test]
async fn help_covers_bundled_commands_without_a_table_entry() {
    let mut bash = Bash::new();
    // `rev` runs but has no curated entry.
    assert!(bash.builtin_names().iter().any(|name| name == "rev"));
    let result = bash.exec("help --json rev").await.unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    let value = json(&result.stdout);
    assert_eq!(value["name"], "rev");
    assert_eq!(value["category"], "other");
}
