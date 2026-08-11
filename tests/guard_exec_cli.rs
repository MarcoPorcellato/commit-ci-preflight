// Copyright 2026 Marco Porcellato
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_commit-ci-preflight")
}

fn compile_fixture(root: &Path) -> PathBuf {
    fs::create_dir_all(root).expect("create fixture build root");
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("guard_exec_fixture.rs");
    let executable = root.join(if cfg!(windows) {
        "guard-exec-fixture.exe"
    } else {
        "guard-exec-fixture"
    });
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    let status = Command::new(rustc)
        .arg(&source)
        .arg("--edition=2024")
        .arg("-o")
        .arg(&executable)
        .status()
        .expect("run rustc for fixture");
    assert!(status.success(), "fixture compilation failed");
    executable
}

fn guard_command(fixture: &Path, role: &str, sentinel: &str, timeout: &str) -> Command {
    let mut command = Command::new(binary());
    command
        .args([
            "guard",
            "exec",
            "--admission-timeout-seconds",
            "10",
            "--timeout-seconds",
            timeout,
            "--",
        ])
        .arg(fixture)
        .env("CCP_GUARD_EXEC_CHILD_ROLE", role)
        .env("CCP_GUARD_EXEC_SENTINEL", sentinel)
        .current_dir(std::env::temp_dir());
    if let Some(root) = std::env::var_os("CCP_TEST_ROOT") {
        command.env("XDG_CACHE_HOME", root);
    }
    command
}

fn run_guard_exec(
    fixture: &Path,
    role: &str,
    sentinel: &str,
    exit_code: Option<&str>,
    timeout: &str,
    extra_args: &[&str],
) -> Output {
    let mut command = guard_command(fixture, role, sentinel, timeout);
    command.args(extra_args);
    if let Some(code) = exit_code {
        command.env("CCP_GUARD_EXEC_EXIT_CODE", code);
    }
    command.output().expect("guard exec")
}

#[test]
fn guard_exec_portable_end_to_end_contract() {
    let base = std::env::var_os("CCP_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = base.join(format!(
        "commit-ci-preflight-guard-exec-fixture-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let fixture = compile_fixture(&root);

    let literal = run_guard_exec(
        &fixture,
        "literal",
        "sentinel-1",
        None,
        "10",
        &["$HOME", "*", "spaced arg"],
    );
    assert!(
        literal.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&literal.stderr)
    );
    let stdout = String::from_utf8(literal.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(literal.stderr).expect("stderr utf8");
    assert!(stdout.contains("stdout:sentinel-1"));
    assert!(stderr.contains("stderr:sentinel-1"));
    assert!(stdout.contains("\"$HOME\""));
    assert!(stdout.contains("\"*\""));
    assert!(stdout.contains("\"spaced arg\""));
    assert!(!stdout.contains("stderr:sentinel-1"));
    assert!(!stderr.contains("stdout:sentinel-1"));

    let nonzero = run_guard_exec(&fixture, "exit", "sentinel-2", Some("255"), "10", &[]);
    assert_eq!(nonzero.status.code(), Some(255));
    assert!(String::from_utf8_lossy(&nonzero.stdout).contains("stdout:sentinel-2"));

    let timeout = run_guard_exec(&fixture, "sleep", "sentinel-3", None, "1", &[]);
    assert_eq!(timeout.status.code(), Some(124));

    verify_guard_exec_serializes_children(&fixture, &root);

    #[cfg(unix)]
    verify_user_cancellation(&fixture, &root);

    fs::remove_dir_all(root).expect("remove owned fixture root");
}

fn verify_guard_exec_serializes_children(fixture: &Path, root: &Path) {
    use std::thread;
    use std::time::{Duration, Instant};

    let ready = root.join("holder.ready");
    let release = root.join("holder.release");
    let queued_started = root.join("queued.started");

    let mut holder = guard_command(fixture, "hold", "sentinel-queue-1", "10")
        .env("CCP_GUARD_EXEC_READY_FILE", &ready)
        .env("CCP_GUARD_EXEC_RELEASE_FILE", &release)
        .spawn()
        .expect("spawn slot holder");

    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    assert!(ready.exists(), "slot holder did not start");

    let mut queued = guard_command(fixture, "mark", "sentinel-queue-2", "10")
        .env("CCP_GUARD_EXEC_STARTED_FILE", &queued_started)
        .spawn()
        .expect("spawn queued guard");
    thread::sleep(Duration::from_millis(250));
    assert!(
        !queued_started.exists(),
        "queued child started before the active guard released its slot"
    );

    fs::write(&release, b"release\n").expect("release slot holder");
    assert!(holder.wait().expect("wait for slot holder").success());
    assert!(queued.wait().expect("wait for queued guard").success());
    assert!(queued_started.exists(), "queued child never started");
}

#[cfg(unix)]
fn verify_user_cancellation(fixture: &Path, root: &Path) {
    use std::thread;
    use std::time::{Duration, Instant};

    use nix::errno::Errno;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let pid_file = root.join("child.pid");
    let mut child = guard_command(fixture, "sleep-with-pid", "sentinel-4", "30")
        .env("CCP_GUARD_EXEC_PID_FILE", &pid_file)
        .spawn()
        .expect("spawn guarded child");

    let deadline = Instant::now() + Duration::from_secs(10);
    while !pid_file.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    let guarded_pid: i32 = fs::read_to_string(&pid_file)
        .expect("guarded fixture pid")
        .parse()
        .expect("numeric guarded fixture pid");

    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).expect("signal guard exec");
    let status = child.wait().expect("wait for guard exec cancellation");
    assert_eq!(status.code(), Some(130));
    assert_eq!(kill(Pid::from_raw(guarded_pid), None), Err(Errno::ESRCH));
}
