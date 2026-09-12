use std::time::Duration;

use super::{ControlFlow, ExecResult, Interpreter};
use crate::StreamData;
use crate::builtins::retry::{RetryConfig, parse_retry_args};
use crate::error::Result;
use crate::parser::{Redirect, SimpleCommand, Word};

impl Interpreter {
    pub(super) async fn execute_retry(
        &mut self,
        args: &[String],
        stdin: Option<StreamData>,
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        if args == ["--help"] {
            return self.apply_redirections(ExecResult::ok(
                "Usage: retry [-n ATTEMPTS] [-d SECONDS] [--backoff] [-q] [-v] -- command [args...]\n\
                 Run a command until it succeeds, returning its last exit status.\n\
                 Defaults: 3 attempts, 1 second between failures.\n\
                 Limits: 10000 attempts, 60 seconds per delay (including backoff), 8 nested calls.\n\
                 -q suppresses retry diagnostics; -v reports each attempt.\n"
            ), redirects).await;
        }
        let config = match parse_retry_args(args) {
            Ok(config) => config,
            Err(error) => {
                return self
                    .apply_redirections(ExecResult::err(format!("{error}\n"), 1), redirects)
                    .await;
            }
        };
        // Retry adds an interpreter dispatch frame around each function frame.
        // Bound mixed retry/function nesting before the smaller WASI stack can
        // trap, while retaining any stricter caller-configured depth limit.
        let retry_limits = self
            .limits
            .clone()
            .max_function_depth(self.limits.max_function_depth.min(8));
        self.counters.push_function(&retry_limits)?;
        let result = self.run_retry(config, stdin).await;
        self.counters.pop_function();
        self.apply_redirections(result?, redirects).await
    }

    async fn run_retry(
        &mut self,
        config: RetryConfig,
        stdin: Option<StreamData>,
    ) -> Result<ExecResult> {
        let name = &config.command[0];
        let args = &config.command[1..];
        // Operands have already been expanded; dispatch exact argv without eval.
        let command = SimpleCommand {
            name: Word::quoted_literal(name),
            args: Vec::new(),
            assignments: Vec::new(),
            redirects: Vec::new(),
            span: Default::default(),
        };
        let budget = self.execution_budget.clone();
        let mut retained = Vec::new();
        let mut result = ExecResult::default();
        let mut delay = config.delay_secs;
        for attempt in 1..=config.max_attempts {
            self.check_cancelled()?;
            budget.consume_work(1)?;
            self.counters.tick_command(&self.limits)?;
            self.counters
                .check_session_limits(&self.session_limits)
                .map_err(|error| crate::Error::Execution(error.to_string()))?;
            let before = result.stdout.len() + result.stderr.len();
            if config.verbose && !config.quiet {
                result.stderr_truncated |= result.stderr.append_capped(
                    &format!("retry: attempt {attempt}/{}\n", config.max_attempts).into(),
                    self.limits.max_stderr_bytes,
                );
            }
            let current = self
                .dispatch_command(name, &command, args.to_vec(), stdin.clone())
                .await?;
            result.stdout_truncated |= current.stdout_truncated
                | result
                    .stdout
                    .append_capped(&current.stdout, self.limits.max_stdout_bytes);
            result.stderr_truncated |= current.stderr_truncated
                | result
                    .stderr
                    .append_capped(&current.stderr, self.limits.max_stderr_bytes);
            result.exit_code = current.exit_code;
            result.control_flow = current.control_flow;
            self.last_exit_code = result.exit_code;
            let finished = result.exit_code == 0
                || result.control_flow != ControlFlow::None
                || attempt == config.max_attempts;
            if !finished && !config.quiet {
                result.stderr_truncated |= result.stderr.append_capped(
                    &format!(
                        "retry: attempt {attempt}/{} failed ({}); retrying in {delay}s\n",
                        config.max_attempts, result.exit_code
                    )
                    .into(),
                    self.limits.max_stderr_bytes,
                );
            }
            let added = (result.stdout.len() + result.stderr.len()).saturating_sub(before);
            budget.consume_input(added)?;
            retained.push(budget.lease_bytes(added)?);
            if finished {
                break;
            }
            let mut remaining = Duration::from_secs_f64(delay);
            while !remaining.is_zero() {
                self.check_cancelled()?;
                budget.check()?;
                let slice = remaining.min(Duration::from_millis(50));
                crate::time_compat::sleep(slice).await;
                remaining = remaining.saturating_sub(slice);
            }
            if config.backoff {
                delay = (delay * 2.0).min(60.0);
            }
        }
        Ok(result)
    }
}
