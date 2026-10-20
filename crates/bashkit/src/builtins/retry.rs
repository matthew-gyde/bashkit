//! retry builtin - run a command until it succeeds
//!
//! Non-standard builtin.
//!
//! Decisions:
//! - The command runs through an [`ExecutionPlan::Driver`], so each attempt is
//!   an ordinary command in this shell: functions, builtins and scripts all
//!   work, attempts share shell state, and every attempt counts against the
//!   execution limits like any other command.
//! - The status is the last attempt's: 0 as soon as one succeeds, otherwise
//!   the final failure's. A `break`, `continue`, `return` or `exit` from the
//!   command ends the retries and propagates.
//! - Diagnostics go to stderr, after the failed attempt's own output: one line
//!   per retried failure (none with `-q`), plus one per attempt with `-v`.
//! - THREAT[TM-DOS]: `-n` is capped at [`MAX_RETRY_ATTEMPTS`]; each wait is
//!   capped at [`MAX_SLEEP_SECONDS`] like `sleep`, backoff included, and never
//!   outlasts the execution deadline; collected output stops at the shell's
//!   stdout/stderr limits. Non-finite and negative delays are rejected.

use async_trait::async_trait;
use std::time::Duration;

use super::limits::RETRY_MAX_ATTEMPTS as MAX_RETRY_ATTEMPTS;
use super::limits::SLEEP_MAX_SECONDS as MAX_SLEEP_SECONDS;
use super::{Builtin, Context, ExecutionDeadline, ExecutionPlan, PlanDriver, PlanStep, SubCommand};
use crate::ExecutionCapability;
use crate::error::Result;
use crate::interpreter::{ControlFlow, ExecResult};
use crate::limits::ExecutionLimits;
use crate::stream::StreamData;

/// Retry builtin - runs a command until it succeeds or the attempts run out.
///
/// Usage: retry [OPTIONS] -- command [args...]
///
/// Options:
///   -n NUM       Max attempts (default: 3)
///   -d SECONDS   Delay between attempts (default: 1)
///   --backoff    Double the delay after each failure
///   -q           Quiet mode (suppress retry messages)
///   -v           Verbose mode (announce each attempt)
pub struct Retry;

const USAGE: &str = "retry: usage: retry [OPTIONS] -- command [args...]\n";

struct RetryConfig {
    max_attempts: u32,
    delay_secs: f64,
    backoff: bool,
    quiet: bool,
    verbose: bool,
    command: Vec<String>,
}

fn parse_retry_args(args: &[String]) -> std::result::Result<RetryConfig, String> {
    let mut max_attempts: u32 = 3;
    let mut delay_secs: f64 = 1.0;
    let mut backoff = false;
    let mut quiet = false;
    let mut verbose = false;
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if p.flag("--") {
            break;
        } else if let Some(val) = p.flag_value("-n", "retry")? {
            max_attempts = val
                .parse()
                .map_err(|_| format!("retry: invalid number '{}'", val))?;
            if max_attempts == 0 {
                return Err("retry: -n must be at least 1".to_string());
            }
            if max_attempts > MAX_RETRY_ATTEMPTS {
                return Err(format!("retry: -n must be at most {MAX_RETRY_ATTEMPTS}"));
            }
        } else if let Some(val) = p.flag_value("-d", "retry")? {
            delay_secs = val
                .parse()
                .map_err(|_| format!("retry: invalid delay '{}'", val))?;
            // `inf` and `nan` parse as f64 but are not durations.
            if !delay_secs.is_finite() || delay_secs < 0.0 {
                return Err(format!("retry: invalid delay '{}'", val));
            }
        } else if p.flag("--backoff") {
            backoff = true;
        } else if p.flag("-q") {
            quiet = true;
        } else if p.flag("-v") {
            verbose = true;
        } else if let Some(arg) = p.current() {
            return Err(format!("retry: unknown option '{}'", arg));
        } else {
            p.advance();
        }
    }

    let command: Vec<String> = p.rest().to_vec();
    if command.is_empty() {
        return Err(USAGE.trim_end().to_string());
    }

    Ok(RetryConfig {
        max_attempts,
        delay_secs: delay_secs.min(MAX_SLEEP_SECONDS),
        backoff,
        quiet,
        verbose,
        command,
    })
}

#[async_trait]
impl Builtin for Retry {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(USAGE.to_string(), 1));
        }
        match parse_retry_args(ctx.args) {
            Err(e) => Ok(ExecResult::err(format!("{e}\n"), 1)),
            // The interpreter runs valid invocations through execution_plan();
            // a context without one has no shell to run the command in.
            Ok(_) => Ok(ExecResult::err(
                "retry: commands can only run inside the shell\n".to_string(),
                1,
            )),
        }
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        let Ok(config) = parse_retry_args(ctx.args) else {
            return Ok(None); // execute() reports the error
        };
        let defaults = ExecutionLimits::default();
        let limits = ctx.shell.as_ref().map_or(&defaults, |shell| shell.limits());
        let mut command = config.command.clone();
        let name = command.remove(0);
        Ok(Some(ExecutionPlan::Driver(Box::new(RetryRun {
            command: SubCommand {
                name,
                args: command,
                stdin: ctx.stdin.cloned(),
                assignments: Vec::new(),
            },
            delay: config.delay_secs,
            config,
            attempt: 0,
            wait: None,
            queued: None,
            output: ExecResult::default(),
            stdout_cap: limits.max_stdout_bytes,
            stderr_cap: limits.max_stderr_bytes,
            deadline: ctx.execution_extension::<ExecutionDeadline>(),
        }))))
    }
}

/// Plan driver: one `Run` per attempt, with a wait between failures.
struct RetryRun {
    config: RetryConfig,
    command: SubCommand,
    /// Attempts started so far.
    attempt: u32,
    /// Seconds to wait before the next attempt.
    delay: f64,
    /// A wait owed before the next attempt (the failure was announced first).
    wait: Option<Duration>,
    /// The attempt to run after an `Emit`.
    queued: Option<PlanStep>,
    /// Every attempt's output and the retry diagnostics, for callers that
    /// capture instead of stream.
    output: ExecResult,
    stdout_cap: usize,
    stderr_cap: usize,
    deadline: Option<ExecutionCapability<ExecutionDeadline>>,
}

/// Append `more` to `buf` without growing it past `cap`; true when bytes
/// were dropped.
fn append_within(buf: &mut StreamData, more: &StreamData, cap: usize) -> bool {
    let room = cap.saturating_sub(buf.len());
    if more.len() <= room {
        buf.append(more);
        false
    } else {
        buf.append(&more.prefix(room));
        true
    }
}

impl RetryRun {
    fn collect(&mut self, stdout: &StreamData, stderr: &StreamData) {
        self.output.stdout_truncated |=
            append_within(&mut self.output.stdout, stdout, self.stdout_cap);
        self.output.stderr_truncated |=
            append_within(&mut self.output.stderr, stderr, self.stderr_cap);
    }

    /// Start the next attempt, announcing it first with `-v` (and any
    /// `notice` already due).
    fn start_attempt(&mut self, mut notice: String) -> PlanStep {
        self.attempt += 1;
        if self.config.verbose && !self.config.quiet {
            notice.push_str(&format!(
                "retry: attempt {}/{}\n",
                self.attempt, self.config.max_attempts
            ));
        }
        let run = PlanStep::Run {
            command: self.command.clone(),
            cwd: None,
        };
        if notice.is_empty() {
            return run;
        }
        self.queued = Some(run);
        self.emit(notice)
    }

    fn emit(&mut self, notice: String) -> PlanStep {
        let notice = StreamData::from(notice);
        self.collect(&StreamData::new(), &notice);
        PlanStep::Emit {
            stdout: StreamData::new(),
            stderr: notice,
        }
    }

    /// The wait owed after a failure: the current delay, never past the
    /// execution deadline. Advances the backoff.
    fn next_wait(&mut self) -> Duration {
        let mut wait = Duration::from_secs_f64(self.delay);
        if let Some(remaining) = self
            .deadline
            .as_ref()
            .and_then(|deadline| deadline.try_with(ExecutionDeadline::remaining).ok())
        {
            wait = wait.min(remaining);
        }
        if self.config.backoff {
            self.delay = (self.delay * 2.0).min(MAX_SLEEP_SECONDS);
        }
        wait
    }
}

#[async_trait]
impl PlanDriver for RetryRun {
    async fn next(&mut self, last: Option<ExecResult>) -> Result<PlanStep> {
        let Some(result) = last else {
            if let Some(wait) = self.wait.take()
                && !wait.is_zero()
            {
                crate::time_compat::sleep(wait).await;
            }
            if let Some(step) = self.queued.take() {
                return Ok(step);
            }
            return Ok(self.start_attempt(String::new()));
        };

        self.collect(&result.stdout, &result.stderr);
        self.output.stdout_truncated |= result.stdout_truncated;
        self.output.stderr_truncated |= result.stderr_truncated;
        let finished = result.exit_code == 0
            || result.control_flow != ControlFlow::None
            || self.attempt >= self.config.max_attempts;
        if finished {
            let mut output = std::mem::take(&mut self.output);
            output.exit_code = result.exit_code;
            output.control_flow = result.control_flow;
            return Ok(PlanStep::Done(output));
        }

        let wait = self.next_wait();
        if self.config.quiet {
            if !wait.is_zero() {
                crate::time_compat::sleep(wait).await;
            }
            return Ok(self.start_attempt(String::new()));
        }
        // Announce the failure before waiting, then run after the wait.
        self.wait = Some(wait);
        Ok(self.emit(format!(
            "retry: attempt {}/{} failed ({}); retrying in {}s\n",
            self.attempt,
            self.config.max_attempts,
            result.exit_code,
            wait.as_secs_f64()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_retry(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Retry.execute(ctx).await.unwrap()
    }

    fn parse(args: &[&str]) -> std::result::Result<RetryConfig, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse_retry_args(&args)
    }

    #[tokio::test]
    async fn test_no_args() {
        let result = run_retry(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("usage"));
    }

    #[test]
    fn test_defaults_with_separator() {
        let config = parse(&["--", "echo", "hello"]).unwrap();
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.delay_secs, 1.0);
        assert!(!config.backoff && !config.quiet && !config.verbose);
        assert_eq!(config.command, ["echo", "hello"]);
    }

    #[test]
    fn test_custom_attempts_delay_and_flags() {
        let config = parse(&[
            "-n",
            "5",
            "-d",
            "2.5",
            "--backoff",
            "-q",
            "-v",
            "--",
            "curl",
            "http://x",
        ])
        .unwrap();
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.delay_secs, 2.5);
        assert!(config.backoff && config.quiet && config.verbose);
        assert_eq!(config.command, ["curl", "http://x"]);
    }

    #[test]
    fn test_delay_capped_like_sleep() {
        assert_eq!(
            parse(&["-d", "600", "--", "true"]).unwrap().delay_secs,
            60.0
        );
    }

    #[tokio::test]
    async fn test_missing_command() {
        for args in [&["--"][..], &["-n", "2"][..]] {
            let result = run_retry(args).await;
            let shown = args.join(" ");
            assert_eq!(result.exit_code, 1, "{shown}");
            assert!(result.stderr.contains("usage"), "{shown}");
        }
    }

    #[tokio::test]
    async fn test_invalid_delay() {
        for delay in ["-1", "inf", "NaN", "abc"] {
            let result = run_retry(&["-d", delay, "--", "true"]).await;
            assert_eq!(result.exit_code, 1, "{delay}");
            assert!(result.stderr.contains("invalid delay"), "{delay}");
        }
    }

    #[tokio::test]
    async fn test_invalid_n() {
        let result = run_retry(&["-n", "abc", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid number"));
    }

    #[tokio::test]
    async fn test_missing_n_arg() {
        let result = run_retry(&["-n"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("-n requires an argument"));
    }

    #[tokio::test]
    async fn test_zero_attempts() {
        let result = run_retry(&["-n", "0", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("must be at least 1"));
    }

    #[tokio::test]
    async fn test_attempts_upper_bound() {
        let result = run_retry(&["-n", "10001", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("must be at most 10000"));
    }

    #[tokio::test]
    async fn test_unknown_option() {
        let result = run_retry(&["--foo", "--", "cmd"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("unknown option"));
    }

    #[tokio::test]
    async fn test_execute_without_shell_refuses() {
        let result = run_retry(&["-n", "2", "--", "true"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }
}
