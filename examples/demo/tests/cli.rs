//! End-to-end tests of the demo binary: everything `#[derive(Cli)]` wires up.

use core::time::Duration;
use std::process::{Command, Output, Stdio};
use std::time::Instant;

fn demo(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_demo"))
        .args(args)
        .env_remove("DEMO_VERSION_FILE")
        .env_remove("BPAF_KIT_ECHO")
        .env_remove("BPAF_KIT_DRY_RUN")
        .env_remove("RUST_LOG")
        .env("NO_COLOR", "1")
        .output()
        .expect("demo runs")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn version_short_and_verbose() {
    let out = demo(&["--version"]);
    assert!(out.status.success());
    let s = text(&out.stdout);
    assert!(
        s.starts_with(&format!("demo {}", env!("CARGO_PKG_VERSION"))),
        "{s}"
    );
    assert_eq!(s.lines().count(), 1);

    let s = text(&demo(&["--version", "--verbose"]).stdout);
    assert!(s.contains("\ncommit:     "), "{s}");
    assert!(s.contains("\ntarget:     "), "{s}");
    assert!(s.ends_with("info from:  compiled in\n"), "{s}");
}

#[test]
fn version_override_file() {
    let path = std::env::temp_dir().join(format!("demo-version-{}.env", std::process::id()));
    std::fs::write(
        &path,
        "version=9.9.9\nrev=0123456789abcdef\ndirty=true\ndate=2030-01-02\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_demo"))
        .arg("--version")
        .env("DEMO_VERSION_FILE", &path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        text(&out.stdout),
        "demo 9.9.9 (012345678-dirty 2030-01-02)\n"
    );
}

#[test]
fn manpage() {
    let out = demo(&["--man"]);
    assert!(out.status.success());
    let s = text(&out.stdout);
    assert!(s.contains(".TH demo 1 "), "{s}");
    assert!(s.contains("\\fBservices\\fP"), "{s}");
}

#[test]
fn exec_echoes_and_streams() {
    let out = demo(&["exec", "--", "sh", "-c", "echo out; echo err >&2"]);
    assert!(out.status.success());
    assert_eq!(text(&out.stdout), "out\n");
    assert_eq!(text(&out.stderr), "+ sh -c 'echo out; echo err >&2'\nerr\n");
}

#[test]
fn no_echo_and_env_var() {
    let out = demo(&["--no-echo", "exec", "--", "true"]);
    assert_eq!(text(&out.stderr), "");
    let out = Command::new(env!("CARGO_BIN_EXE_demo"))
        .args(["exec", "--", "true"])
        .env("BPAF_KIT_ECHO", "0")
        .output()
        .unwrap();
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn dry_run_only_echoes() {
    let marker = std::env::temp_dir().join(format!("demo-dry-{}", std::process::id()));
    let m = marker.to_str().unwrap();
    let out = demo(&["--dry-run", "exec", "--", "touch", m]);
    assert!(out.status.success());
    assert_eq!(text(&out.stderr), format!("+ touch {m}\n"));
    assert!(!marker.exists());
}

#[test]
fn capture_reports_stderr_tail_with_code() {
    let out = demo(&[
        "exec",
        "--capture",
        "--",
        "sh",
        "-c",
        "echo nope >&2; exit 4",
    ]);
    assert_eq!(out.status.code(), Some(1));
    let s = text(&out.stderr);
    assert!(
        s.ends_with("demo: error[cmd_exit]: `sh` failed (exit status: 4):\nnope\n"),
        "{s}"
    );
}

#[test]
fn verbose_flag_enables_info_logs() {
    let quiet = text(&demo(&["exec", "--", "true"]).stderr);
    assert!(!quiet.contains("info: running"), "{quiet}");
    let loud = text(&demo(&["-v", "exec", "--", "true"]).stderr);
    assert!(loud.contains("info: running program=\"true\""), "{loud}");
}

#[test]
fn json_logs() {
    let s = text(&demo(&["-v", "--log-json", "exec", "--", "true"]).stderr);
    let line = s.lines().find(|l| l.starts_with('{')).unwrap_or_default();
    assert!(line.contains("\"level\":\"INFO\""), "{s}");
}

#[test]
fn enum_suggestion() {
    let out = demo(&["services", "--format", "boxd"]);
    assert!(!out.status.success());
    let s = text(&out.stderr);
    assert!(
        s.contains("unknown style 'boxd', did you mean 'boxed'?"),
        "{s}"
    );
}

#[test]
fn plain_table_when_piped() {
    let s = text(&demo(&["services"]).stdout);
    assert!(s.lines().next().unwrap().contains("service"), "{s}");
    assert!(!s.contains('│'), "{s}");
}

#[cfg(unix)]
#[test]
fn ctrl_c_exits_130() {
    let child = Command::new(env!("CARGO_BIN_EXE_demo"))
        .args(["--no-echo", "exec", "--", "sleep", "5"])
        // `sleep` outlives demo; don't let it hold the test's stdout.
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Give it time to install the handler and start the child.
    std::thread::sleep(Duration::from_millis(300));
    let start = Instant::now();
    let killed = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(130));
    assert!(start.elapsed() < Duration::from_secs(3));
    // The cleanup registered with `interrupt::on_interrupt` ran.
    assert_eq!(text(&out.stderr), "demo: interrupted, cleaning up\n");
}
