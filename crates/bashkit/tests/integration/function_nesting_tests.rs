//! A function or `source` nesting overrun (TM-DOS-020) ends like bash's
//! `FUNCNEST` overrun: the current command line is abandoned with a
//! diagnostic and status 1, output already produced is kept, and the
//! script resumes at its next line. Spec cases in `funcnest.test.sh`
//! compare the `FUNCNEST` form with real bash.

use bashkit::{Bash, ExecutionLimits};

fn shell(depth: usize) -> Bash {
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_function_depth(depth)
                .max_commands(1_000),
        )
        .build()
}

#[tokio::test]
async fn overrun_keeps_output_and_resumes_at_the_next_line() {
    let mut bash = shell(4);
    let result = bash
        .exec("echo before\nf() { f; }\nf; echo same-line\necho \"after $?\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "before\nafter 1\n");
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stderr,
        "bash: line 2: f: maximum function nesting level exceeded (4)\n"
    );
}

#[tokio::test]
async fn overrun_on_the_last_line_is_the_script_status() {
    let mut bash = shell(4);
    let result = bash
        .exec("echo before-limit; f(){ f; }; f; echo after-limit")
        .await
        .unwrap();
    assert_eq!(result.stdout, "before-limit\n");
    assert_eq!(result.exit_code, 1);
    assert!(
        result
            .stderr
            .contains("f: maximum function nesting level exceeded (4)"),
        "{}",
        result.stderr
    );
}

#[tokio::test]
async fn overrun_unwinds_every_caller_once() {
    // Each level would call itself twice: the first overrun abandons the
    // whole line, so recursion stops instead of fanning out.
    let mut bash = shell(4);
    let result = bash
        .exec("n=0\nf() { n=$((n+1)); f; f; }\nf\necho \"calls=$n status=$?\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "calls=4 status=1\n");
    assert_eq!(result.stderr.matches("nesting level exceeded").count(), 1);
}

#[tokio::test]
async fn overrun_releases_the_depth_for_later_calls() {
    let mut bash = shell(4);
    bash.exec("f() { f; }; f").await.unwrap();
    let result = bash
        .exec("g() { if [ $1 -gt 1 ]; then g $(($1 - 1)); else echo bottom; fi; }; g 4")
        .await
        .unwrap();
    assert_eq!(result.stdout, "bottom\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn source_overrun_abandons_the_line_and_keeps_positionals() {
    let mut bash = shell(4);
    let result = bash
        .exec(
            "set -- a b\necho 'n=$((n+1)); source /s x' > /s\nsource /s x\necho \"after $? n=$n args=$*\"",
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "after 1 n=4 args=a b\n");
    assert_eq!(
        result.stderr,
        "/s: line 1: /s: maximum source nesting level exceeded (4)\n"
    );
}

#[tokio::test]
async fn funcnest_below_the_limit_applies_first() {
    let mut bash = shell(8);
    let result = bash
        .exec("FUNCNEST=3; f() { f; }; m=$( { f; } 2>&1 ); echo \"${m#*line *: }\"")
        .await
        .unwrap();
    assert_eq!(
        result.stdout,
        "f: maximum function nesting level exceeded (3)\n"
    );
}

#[tokio::test]
async fn funcnest_above_the_limit_cannot_raise_it() {
    let mut bash = shell(4);
    let result = bash
        .exec("FUNCNEST=50; f() { f; }; m=$( { f; } 2>&1 ); echo \"${m#*line *: }\"")
        .await
        .unwrap();
    assert_eq!(
        result.stdout,
        "f: maximum function nesting level exceeded (4)\n"
    );
}
