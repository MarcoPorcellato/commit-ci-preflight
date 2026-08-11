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

use std::env;
use std::fs;
use std::io::{self, Write};
use std::thread;
use std::time::Duration;

fn main() {
    let role = env::var("CCP_GUARD_EXEC_CHILD_ROLE").unwrap_or_default();
    let sentinel = env::var("CCP_GUARD_EXEC_SENTINEL").unwrap_or_default();
    let argv: Vec<_> = env::args_os().skip(1).collect();

    match role.as_str() {
        "literal" => {
            let _ = writeln!(io::stdout(), "stdout:{sentinel}:{argv:?}");
            let _ = writeln!(io::stderr(), "stderr:{sentinel}:{argv:?}");
        }
        "exit" => {
            let code = env::var("CCP_GUARD_EXEC_EXIT_CODE")
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            let _ = writeln!(io::stdout(), "stdout:{sentinel}");
            let _ = writeln!(io::stderr(), "stderr:{sentinel}");
            std::process::exit(code);
        }
        "sleep-with-pid" => {
            let path = env::var_os("CCP_GUARD_EXEC_PID_FILE").expect("pid file path");
            fs::write(path, std::process::id().to_string()).expect("write pid file");
            thread::sleep(Duration::from_secs(30));
        }
        "hold" => {
            let ready = env::var_os("CCP_GUARD_EXEC_READY_FILE").expect("ready file path");
            let release = env::var_os("CCP_GUARD_EXEC_RELEASE_FILE").expect("release file path");
            fs::write(ready, b"ready\n").expect("write ready file");
            while !std::path::Path::new(&release).exists() {
                thread::sleep(Duration::from_millis(25));
            }
        }
        "mark" => {
            let path = env::var_os("CCP_GUARD_EXEC_STARTED_FILE").expect("started file path");
            fs::write(path, b"started\n").expect("write started file");
        }
        "sleep" => thread::sleep(Duration::from_secs(10)),
        _ => std::process::exit(70),
    }
}
