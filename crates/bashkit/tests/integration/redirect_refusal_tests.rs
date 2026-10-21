//! An output redirect the filesystem refuses (an embedder's write policy, a
//! read-only mount) is an ordinary failed redirection: the command fails with
//! status 1, the reason goes to fd 2 as the redirects before it left it, and
//! the script decides what happens next.

use std::sync::Arc;

use bashkit::{Bash, FileSystem, InMemoryFs, ReadOnlyFs};

fn read_only_shell() -> Bash {
    let fs: Arc<dyn FileSystem> = Arc::new(ReadOnlyFs::new(Arc::new(InMemoryFs::new())));
    Bash::builder().fs(fs).build()
}

#[tokio::test]
async fn refused_write_is_handled_by_the_script() {
    let mut bash = read_only_shell();
    let result = bash
        .exec("echo x > /etc/passwd || echo \"handled $?\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "handled 1\n");
    assert_eq!(result.exit_code, 0);
    assert!(result.stderr.contains("/etc/passwd: "), "{}", result.stderr);
}

#[tokio::test]
async fn refusal_honors_an_earlier_stderr_redirect() {
    let mut bash = read_only_shell();
    let result = bash
        .exec("echo x 2>/dev/null > /etc/passwd || echo handled\n{ echo y >> /etc/passwd; } 2>/dev/null; echo \"group $?\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "handled\ngroup 1\n");
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn refusal_after_the_stderr_redirect_still_reports() {
    // `>/etc/passwd` opens first, before `2>/dev/null` (bash prints it too).
    let mut bash = read_only_shell();
    let result = bash
        .exec("echo x > /etc/passwd 2>/dev/null || echo handled")
        .await
        .unwrap();
    assert_eq!(result.stdout, "handled\n");
    assert!(result.stderr.contains("/etc/passwd: "), "{}", result.stderr);
}
