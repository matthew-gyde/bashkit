//! Retry argument parsing. Execution belongs to the interpreter so attempts
//! share command dispatch, workspace state, and execution limits.

use async_trait::async_trait;

use super::limits::RETRY_MAX_ATTEMPTS as MAX_RETRY_ATTEMPTS;
use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Retry builtin. The interpreter handles execution through its special dispatch.
///
/// Usage: retry [OPTIONS] -- command [args...]
///
/// Options:
///   -n NUM       Max retry attempts (default: 3)
///   -d SECONDS   Delay between retries (default: 1)
///   --backoff    Enable exponential backoff
///   -q           Quiet mode (suppress retry messages)
///   -v           Verbose mode (show detailed retry info)
pub struct Retry;

pub(crate) struct RetryConfig {
    pub max_attempts: u32,
    pub delay_secs: f64,
    pub backoff: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub command: Vec<String>,
}

pub(crate) fn parse_retry_args(args: &[String]) -> std::result::Result<RetryConfig, String> {
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
            if !delay_secs.is_finite()
                || !(0.0..=super::limits::SLEEP_MAX_SECONDS).contains(&delay_secs)
            {
                return Err("retry: delay must be finite and between 0 and 60 seconds".to_string());
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
        return Err("retry: missing command after --".to_string());
    }

    Ok(RetryConfig {
        max_attempts,
        delay_secs,
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
            return Ok(ExecResult::err(
                "retry: usage: retry [OPTIONS] -- command [args...]\n".to_string(),
                1,
            ));
        }

        if let Err(error) = parse_retry_args(ctx.args) {
            return Ok(ExecResult::err(format!("{error}\n"), 1));
        }
        Ok(ExecResult::err(
            "retry: requires interpreter command dispatch\n",
            1,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> std::result::Result<RetryConfig, String> {
        parse_retry_args(
            &args
                .iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn accepts_bounded_attempts_delay_and_exact_command_arguments() {
        let config = parse(&[
            "-n",
            "5",
            "-d",
            "0.05",
            "--backoff",
            "-q",
            "-v",
            "--",
            "printf",
            "%s",
            "a b",
        ])
        .unwrap();
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.delay_secs, 0.05);
        assert!(config.backoff && config.quiet && config.verbose);
        assert_eq!(config.command, ["printf", "%s", "a b"]);
    }

    #[test]
    fn rejects_missing_command_invalid_counts_and_non_finite_delays() {
        for args in [
            vec![],
            vec!["--"],
            vec!["-n"],
            vec!["-n", "0", "--", "true"],
            vec!["-n", "10001", "--", "true"],
            vec!["-n", "abc", "--", "true"],
            vec!["-d"],
            vec!["-d", "-1", "--", "true"],
            vec!["-d", "NaN", "--", "true"],
            vec!["-d", "inf", "--", "true"],
            vec!["-d", "61", "--", "true"],
            vec!["--unknown", "--", "true"],
        ] {
            assert!(parse(&args).is_err(), "{}", args.join(" "));
        }
    }
}
