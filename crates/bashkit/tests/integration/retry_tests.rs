//! `retry` runs its command through the interpreter: each attempt is an
//! ordinary command in the same shell, and the status is the last attempt's.

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Fails until its third call, recording that every call saw the same
/// arguments and stdin.
struct Flaky {
    calls: Arc<AtomicU32>,
}

#[async_trait]
impl Builtin for Flaky {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        assert_eq!(ctx.args, ["two words", "$(echo not-run)", ""]);
        assert_eq!(ctx.stdin.map(|stdin| &**stdin), Some("input\n"));
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(ExecResult::with_code(
            format!("call {call}\n"),
            if call < 3 { 7 } else { 0 },
        ))
    }
}

async fn run(script: &str) -> ExecResult {
    Bash::new().exec(script).await.unwrap()
}

#[tokio::test]
async fn retry_runs_until_success_in_the_same_shell() {
    let r = run("n=0; flaky() { n=$((n+1)); [ $n -ge 3 ]; }\n\
         retry -n 5 -d 0 -q -- flaky; echo \"status=$? n=$n\"")
    .await;
    assert_eq!(r.stdout, "status=0 n=3\n");
    assert_eq!(r.stderr, "");
}

#[tokio::test]
async fn retry_returns_the_last_status_with_every_attempts_output() {
    let r = run("retry -n 2 -d 0 -- sh -c 'echo attempt; exit 7'; echo \"status=$?\"").await;
    assert_eq!(r.stdout, "attempt\nattempt\nstatus=7\n");
    assert_eq!(r.stderr, "retry: attempt 1/2 failed (7); retrying in 0s\n");
}

#[tokio::test]
async fn retry_passes_exact_arguments_and_stdin_to_each_attempt() {
    let calls = Arc::new(AtomicU32::new(0));
    let mut bash = Bash::builder()
        .builtin(
            "flaky",
            Box::new(Flaky {
                calls: calls.clone(),
            }),
        )
        .build();
    let r = bash
        .exec("printf 'input\\n' | retry -n 5 -d 0 -q -- flaky 'two words' '$(echo not-run)' ''")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "call 1\ncall 2\ncall 3\n");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_diagnostics_follow_quiet_and_verbose() {
    let quiet = run("retry -n 3 -d 0 -q -- false").await;
    assert_eq!(quiet.exit_code, 1);
    assert_eq!(quiet.stderr, "");

    let verbose = run("retry -n 2 -d 0 -v -- false").await;
    assert_eq!(verbose.exit_code, 1);
    assert_eq!(
        verbose.stderr,
        "retry: attempt 1/2\n\
         retry: attempt 1/2 failed (1); retrying in 0s\n\
         retry: attempt 2/2\n"
    );
}

#[tokio::test]
async fn retry_redirects_apply_to_the_whole_run() {
    let r = run(
        "retry -n 2 -d 0 -- sh -c 'echo out; echo err >&2; exit 3' > /tmp/out 2> /tmp/err\n\
         echo \"status=$?\"; cat /tmp/out; echo --; cat /tmp/err",
    )
    .await;
    assert_eq!(
        r.stdout,
        "status=3\nout\nout\n--\nerr\nretry: attempt 1/2 failed (3); retrying in 0s\nerr\n"
    );
    assert_eq!(r.stderr, "");
}

#[tokio::test]
async fn retry_stops_on_control_flow() {
    let r = run("for i in 1 2; do retry -n 3 -d 0 -- break; echo after; done; echo done").await;
    assert_eq!(r.stdout, "done\n");
}

#[tokio::test(start_paused = true)]
async fn retry_waits_between_failures_with_backoff() {
    let started = tokio::time::Instant::now();
    let r = run("retry -n 3 -d 2 --backoff -- false").await;
    assert_eq!(r.exit_code, 1);
    assert_eq!(
        r.stderr,
        "retry: attempt 1/3 failed (1); retrying in 2s\n\
         retry: attempt 2/3 failed (1); retrying in 4s\n"
    );
    assert!(started.elapsed() >= Duration::from_secs(6));
}

#[tokio::test]
async fn retry_rejects_invalid_usage() {
    for args in [
        "",
        "--",
        "-n 0 -- true",
        "-n 10001 -- true",
        "-d -1 -- true",
        "-d inf -- true",
    ] {
        let r = run(&format!("retry {args}")).await;
        assert_eq!(r.exit_code, 1, "retry {args}");
        assert!(r.stdout.is_empty(), "retry {args}: {}", r.stdout);
    }
}
