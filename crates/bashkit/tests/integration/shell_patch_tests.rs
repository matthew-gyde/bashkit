//! Regression coverage for the shell fixes carried by the Gyde fork.

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, BuiltinRegistry, ExecResult};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

struct Recoverable {
    calls: Arc<AtomicU32>,
}

#[async_trait]
impl Builtin for Recoverable {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        assert_eq!(ctx.args, ["two words", "$(printf injected)", ""]);
        assert_eq!(ctx.stdin.map(|stdin| &**stdin), Some("input\n"));
        let attempt = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let mut result = ExecResult::ok(format!("attempt {attempt}\n"));
        result.exit_code = if attempt < 3 { 7 } else { 0 };
        Ok(result)
    }
}

#[tokio::test]
async fn retry_preserves_arguments_stdin_and_redirected_output() {
    let calls = Arc::new(AtomicU32::new(0));
    let registry = BuiltinRegistry::new();
    registry.insert(
        "recoverable",
        Arc::new(Recoverable {
            calls: calls.clone(),
        }),
    );
    let mut bash = Bash::builder().builtin_registry(registry).build();
    let result = bash
        .exec("printf 'input\\n' | retry -n 5 -d 0 -q -- recoverable 'two words' '$(printf injected)' '' > /tmp/attempts; cat /tmp/attempts")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "attempt 1\nattempt 2\nattempt 3\n");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_shares_function_state_and_returns_last_failure() {
    let mut bash = Bash::new();
    let result = bash
        .exec("attempt=0; recoverable() { attempt=$((attempt+1)); test $attempt -eq 3; }; retry -n 4 -d 0 -q -- recoverable; printf '%s' $attempt")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "3");
    let failed = bash
        .exec("command retry -n 2 -d 0 -q -- sh -c 'printf failed; exit 7'")
        .await
        .unwrap();
    assert_eq!(failed.exit_code, 7);
    assert_eq!(failed.stdout, "failedfailed");
    for args in [
        "--",
        "-n 0 -- true",
        "-n 10001 -- true",
        "-d NaN -- true",
        "-d inf -- true",
        "-d -1 -- true",
        "-d 61 -- true",
    ] {
        let result = bash.exec(&format!("retry {args}")).await.unwrap();
        assert_ne!(result.exit_code, 0, "retry {args}");
    }
}

#[tokio::test]
async fn command_discovers_all_operands_and_honors_redirections() {
    let calls = Arc::new(AtomicU32::new(0));
    let registry = BuiltinRegistry::new();
    registry.insert("recoverable", Arc::new(Recoverable { calls }));
    let mut bash = Bash::builder().builtin_registry(registry).build();
    let result = bash
        .exec("command -v printf recoverable cat")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "printf\nrecoverable\ncat\n");
    let mixed = bash
        .exec("command -v printf missing-command recoverable > /tmp/discovered")
        .await
        .unwrap();
    assert_eq!(mixed.exit_code, 0);
    assert_eq!(mixed.stdout, "");
    assert_eq!(
        bash.exec("command -v missing-command another-missing-command")
            .await
            .unwrap()
            .exit_code,
        1
    );
    assert_eq!(
        bash.exec("cat /tmp/discovered").await.unwrap().stdout,
        "printf\nrecoverable\n"
    );
    let bypass = bash
        .exec("printf() { echo function; }; command printf builtin")
        .await
        .unwrap();
    assert_eq!(bypass.stdout, "builtin");
    assert_eq!(bash.exec("command -Z printf").await.unwrap().exit_code, 2);
}

#[tokio::test]
async fn stat_formats_modification_timestamps_from_filesystem_metadata() {
    let mut bash = Bash::new();
    assert_eq!(bash.exec("touch /tmp/stamped").await.unwrap().exit_code, 0);
    let modified = bash
        .fs()
        .stat(Path::new("/tmp/stamped"))
        .await
        .unwrap()
        .modified;
    let epoch = modified
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let result = bash.exec("stat -c '%Y' /tmp/stamped").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, format!("{epoch}\n"));
    let formatted = bash.exec("stat -c '%y' /tmp/stamped").await.unwrap();
    let parsed =
        chrono::DateTime::parse_from_str(formatted.stdout.trim(), "%Y-%m-%d %H:%M:%S%.f %z")
            .unwrap();
    assert_eq!(parsed.timestamp(), epoch as i64);
}
