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

#![cfg(unix)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use commit_ci_preflight::process::{
    CancellationToken, GenerationGuard, ProcessRequest, ProcessSupervisor, ProcessTermination,
    RunIdentity, SupervisorPort,
};
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;

const ROLE_ENV: &str = "CCP_PROCESS_TREE_FIXTURE_ROLE";
const PID_FILE_ENV: &str = "CCP_PROCESS_TREE_FIXTURE_PID_FILE";

#[test]
fn process_tree_fixture() {
    let Ok(role) = std::env::var(ROLE_ENV) else {
        return;
    };
    if role == "descendant" {
        thread::sleep(Duration::from_secs(30));
        return;
    }

    let executable = std::env::current_exe().expect("test executable");
    let pid_file = PathBuf::from(std::env::var_os(PID_FILE_ENV).expect("pid file"));
    let mut descendant = Command::new(executable)
        .args(["--exact", "process_tree_fixture"])
        .env(ROLE_ENV, "descendant")
        .env(PID_FILE_ENV, &pid_file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn descendant");
    fs::write(&pid_file, descendant.id().to_string()).expect("write descendant pid");
    thread::sleep(Duration::from_secs(30));
    descendant.wait().expect("wait for descendant");
}

#[test]
fn timeout_removes_the_real_process_group_and_descendant() {
    let pid_file = std::env::temp_dir().join(format!(
        "ccp-process-tree-{}-descendant.pid",
        std::process::id()
    ));
    let _ = fs::remove_file(&pid_file);

    let executable = std::env::current_exe().expect("test executable");
    let identity = RunIdentity {
        project: "fixture/process-tree".to_owned(),
        commit: None,
        config_digest: "a".repeat(64),
        generation: "process-tree-v1".to_owned(),
    };
    let guard = GenerationGuard::new(identity.clone());
    let request = ProcessRequest {
        identity,
        program: executable.into_os_string(),
        argv: vec![
            OsString::from("--exact"),
            OsString::from("process_tree_fixture"),
        ],
        current_dir: std::env::current_dir().expect("current directory"),
        environment: BTreeMap::from([
            (OsString::from(ROLE_ENV), OsString::from("root")),
            (
                OsString::from(PID_FILE_ENV),
                pid_file.clone().into_os_string(),
            ),
        ]),
        timeout: Duration::from_millis(750),
        max_capture_bytes: 4096,
    };

    let result = ProcessSupervisor::standard()
        .execute(&request, &CancellationToken::default(), &guard)
        .expect("timeout must clean the complete process group");
    assert_eq!(result.termination, ProcessTermination::TimedOut);

    let descendant_pid: i32 = fs::read_to_string(&pid_file)
        .expect("descendant pid file")
        .parse()
        .expect("numeric descendant pid");
    assert_eq!(kill(Pid::from_raw(descendant_pid), None), Err(Errno::ESRCH));
    fs::remove_file(pid_file).expect("remove owned fixture pid file");
}
