//! Command discovery must describe the same builtins the shell dispatches.

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, BuiltinRegistry, ExecResult};
use std::sync::Arc;

struct DiagnosticCommand;

#[async_trait]
impl Builtin for DiagnosticCommand {
    fn llm_hint(&self) -> Option<&'static str> {
        Some("diagnostic-command: accepts quoted \"text\"; no subprocesses")
    }

    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok("diagnostic-ok\n"))
    }
}

#[tokio::test]
async fn discovery_agrees_for_special_and_host_builtins() {
    let registry = BuiltinRegistry::new();
    let mut bash = Bash::builder().builtin_registry(registry.clone()).build();
    registry.insert("audit-host", Arc::new(DiagnosticCommand));
    for name in ["bash", "sh", "command", "exec", "getopts", "audit-host"] {
        let result = bash
            .exec(&format!("command -v {name}; type -t {name}"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "{name}: {}", result.stderr);
        assert_eq!(result.stdout, format!("{name}\nbuiltin\n"));
    }
    assert_eq!(
        bash.exec("bash -c 'echo nested'; sh -c 'echo nested'")
            .await
            .unwrap()
            .stdout,
        "nested\nnested\n"
    );
    assert_eq!(
        bash.exec("audit-host").await.unwrap().stdout,
        "diagnostic-ok\n"
    );
    registry.remove("audit-host");
    let result = bash.exec("type audit-host").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.contains("not found"));
}

#[tokio::test]
async fn discovery_help_tracks_live_inventory_and_custom_hints() {
    let registry = BuiltinRegistry::new();
    let mut bash = Bash::builder()
        .builtin("audit-custom", Box::new(DiagnosticCommand))
        .builtin_registry(registry.clone())
        .build();
    registry.insert("audit-host", Arc::new(DiagnosticCommand));
    let inventory = bash.builtin_names();
    let listing = bash.exec("help --list -s").await.unwrap();
    assert_eq!(listing.stdout.lines().collect::<Vec<_>>(), inventory);
    for name in ["audit-custom", "audit-host", "bash", "sh", "timeout"] {
        let result = bash.exec(&format!("help --json {name}")).await.unwrap();
        assert_eq!(result.exit_code, 0, "{name}: {}", result.stderr);
        let value: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
        assert_eq!(value["name"], name);
        if name.starts_with("audit-") {
            assert!(
                value["description"]
                    .as_str()
                    .unwrap()
                    .contains("no subprocesses")
            );
        }
    }
    registry.remove("audit-host");
    assert_eq!(bash.exec("help audit-host").await.unwrap().exit_code, 1);
    assert!(
        !bash
            .exec("help --list -s")
            .await
            .unwrap()
            .stdout
            .lines()
            .any(|name| name == "audit-host")
    );
}

#[tokio::test]
async fn discovery_missing_command_diagnostic_is_bounded_utf8() {
    let mut bash = Bash::new();
    let name = "雪".repeat(2000);
    let result = bash.exec(&format!("type '{name}'")).await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.len() <= 1024);
}

#[tokio::test]
async fn missing_command_honors_output_redirections() {
    let mut bash = Bash::new();
    let result = bash
        .exec("audit_missing_command > /tmp/missing.out 2> /tmp/missing.err")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 127);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    let captured = bash
        .exec("test -f /tmp/missing.out; cat /tmp/missing.out /tmp/missing.err")
        .await
        .unwrap();
    assert_eq!(captured.exit_code, 0);
    assert!(
        captured
            .stdout
            .contains("audit_missing_command: command not found")
    );
    assert!(captured.stderr.is_empty());
    let merged = bash.exec("audit_missing_command 2>&1").await.unwrap();
    assert_eq!(merged.exit_code, 127);
    assert!(
        merged
            .stdout
            .contains("audit_missing_command: command not found")
    );
    assert!(merged.stderr.is_empty());
}
