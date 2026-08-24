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

use std::fmt;
use std::io::{self, Read};

#[cfg(target_os = "macos")]
use nix::{errno::Errno, sys::signal::kill, unistd::Pid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionIdentity {
    pub parent_pid: u32,
    pub parent_start: String,
    pub boot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionObservation {
    Live,
    LostParent,
    Reparented,
    Rebooted,
    Ambiguous,
    Unsupported,
}

pub trait SessionInspector {
    fn observe(&self, identity: &AgentSessionIdentity) -> SessionObservation;
}

pub trait CapabilitySource {
    fn capability_32(&self) -> Result<[u8; 32], AgentSessionError>;
}

#[derive(Debug)]
pub enum AgentSessionError {
    InvalidParentPid,
    InvalidPlatformOutput(&'static str),
    Io(io::Error),
    ProcessNotFound,
    Unsupported,
}

impl fmt::Display for AgentSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParentPid => formatter.write_str("parent PID must be non-zero"),
            Self::InvalidPlatformOutput(field) => {
                write!(formatter, "platform did not provide valid {field} evidence")
            }
            Self::Io(_) => formatter.write_str("platform session evidence could not be read"),
            Self::ProcessNotFound => formatter.write_str("parent process is not present"),
            Self::Unsupported => formatter.write_str("platform session evidence is unsupported"),
        }
    }
}

impl std::error::Error for AgentSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct SystemSessionInspector;

impl SystemSessionInspector {
    pub fn capture(parent_pid: u32) -> Result<AgentSessionIdentity, AgentSessionError> {
        capture_with_probe(parent_pid, &SystemPlatformProbe)
    }
}

impl SessionInspector for SystemSessionInspector {
    fn observe(&self, identity: &AgentSessionIdentity) -> SessionObservation {
        observe_with_probe(identity, &SystemPlatformProbe)
    }
}

trait PlatformProbe {
    fn boot_id(&self) -> Result<String, AgentSessionError>;
    fn current_parent_pid(&self) -> Result<u32, AgentSessionError>;
    fn process_snapshot(
        &self,
        parent_pid: u32,
    ) -> Result<Option<ProcessSnapshot>, AgentSessionError>;
}

struct SystemPlatformProbe;

impl PlatformProbe for SystemPlatformProbe {
    fn boot_id(&self) -> Result<String, AgentSessionError> {
        system_boot_id()
    }

    fn current_parent_pid(&self) -> Result<u32, AgentSessionError> {
        current_parent_pid()
    }

    fn process_snapshot(
        &self,
        parent_pid: u32,
    ) -> Result<Option<ProcessSnapshot>, AgentSessionError> {
        process_snapshot(parent_pid)
    }
}

fn capture_with_probe(
    parent_pid: u32,
    probe: &impl PlatformProbe,
) -> Result<AgentSessionIdentity, AgentSessionError> {
    if parent_pid == 0 {
        return Err(AgentSessionError::InvalidParentPid);
    }
    let parent_before = probe.current_parent_pid()?;
    if parent_before != parent_pid || parent_before == 1 {
        return Err(AgentSessionError::InvalidPlatformOutput("parent process"));
    }
    let boot_before = probe.boot_id()?;
    let snapshot = probe
        .process_snapshot(parent_pid)?
        .ok_or(AgentSessionError::ProcessNotFound)?;
    if snapshot.pid != parent_pid {
        return Err(AgentSessionError::InvalidPlatformOutput("parent process"));
    }
    let parent_after = probe.current_parent_pid()?;
    let boot_after = probe.boot_id()?;
    if parent_before != parent_after || boot_before != boot_after {
        return Err(AgentSessionError::InvalidPlatformOutput(
            "stable session evidence",
        ));
    }
    Ok(AgentSessionIdentity {
        parent_pid,
        parent_start: snapshot.start,
        boot_id: boot_before,
    })
}

fn observe_with_probe(
    identity: &AgentSessionIdentity,
    probe: &impl PlatformProbe,
) -> SessionObservation {
    if identity.parent_pid == 0 || identity.parent_start.is_empty() || identity.boot_id.is_empty() {
        return SessionObservation::Ambiguous;
    }
    let boot_before = match probe.boot_id() {
        Ok(boot_id) => boot_id,
        Err(_) => return SessionObservation::Unsupported,
    };
    if boot_before != identity.boot_id {
        return SessionObservation::Rebooted;
    }
    let parent_before = match probe.current_parent_pid() {
        Ok(parent_pid) => parent_pid,
        Err(_) => return SessionObservation::Unsupported,
    };
    let snapshot = match probe.process_snapshot(identity.parent_pid) {
        Ok(snapshot) => snapshot,
        Err(_) => return SessionObservation::Unsupported,
    };
    let parent_after = match probe.current_parent_pid() {
        Ok(parent_pid) => parent_pid,
        Err(_) => return SessionObservation::Unsupported,
    };
    let boot_after = match probe.boot_id() {
        Ok(boot_id) => boot_id,
        Err(_) => return SessionObservation::Unsupported,
    };
    if boot_before != boot_after || parent_before != parent_after {
        return SessionObservation::Ambiguous;
    }
    if boot_before != identity.boot_id {
        return SessionObservation::Rebooted;
    }
    classify_observation(identity, &boot_before, parent_before, snapshot.as_ref())
}

#[derive(Debug, Default)]
pub struct SystemCapabilitySource;

impl CapabilitySource for SystemCapabilitySource {
    fn capability_32(&self) -> Result<[u8; 32], AgentSessionError> {
        #[cfg(unix)]
        {
            let mut capability = [0u8; 32];
            std::fs::File::open("/dev/urandom")
                .map_err(AgentSessionError::Io)?
                .read_exact(&mut capability)
                .map_err(AgentSessionError::Io)?;
            Ok(capability)
        }
        #[cfg(not(unix))]
        {
            Err(AgentSessionError::Unsupported)
        }
    }
}

pub fn digest_capability(capability: &[u8; 32]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"commit-ci-preflight-agent-session-capability-v1\0");
    hasher.update(capability);
    let digest = hasher.finalize();
    let mut value = String::with_capacity("sha256:".len() + digest.len() * 2);
    value.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
const AGENT_SESSION_RECORD_SCHEMA_VERSION: &str = "1.0";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AgentSessionRecord {
    schema_version: String,
    parent_pid: u32,
    parent_start: String,
    boot_id: String,
    capability_digest: String,
}

#[cfg(test)]
impl AgentSessionRecord {
    pub(crate) fn from_identity_and_capability(
        identity: &AgentSessionIdentity,
        capability: &[u8; 32],
    ) -> Self {
        Self {
            schema_version: AGENT_SESSION_RECORD_SCHEMA_VERSION.to_owned(),
            parent_pid: identity.parent_pid,
            parent_start: identity.parent_start.clone(),
            boot_id: identity.boot_id.clone(),
            capability_digest: digest_capability(capability),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessSnapshot {
    pid: u32,
    start: String,
}

fn classify_observation(
    identity: &AgentSessionIdentity,
    observed_boot_id: &str,
    current_parent_pid: u32,
    snapshot: Option<&ProcessSnapshot>,
) -> SessionObservation {
    if identity.parent_pid == 0 || identity.parent_start.is_empty() || identity.boot_id.is_empty() {
        return SessionObservation::Ambiguous;
    }
    if identity.boot_id != observed_boot_id {
        return SessionObservation::Rebooted;
    }
    let Some(snapshot) = snapshot else {
        return if current_parent_pid == identity.parent_pid {
            SessionObservation::Ambiguous
        } else {
            SessionObservation::LostParent
        };
    };
    if snapshot.pid != identity.parent_pid || snapshot.start != identity.parent_start {
        return SessionObservation::Ambiguous;
    }
    if current_parent_pid != identity.parent_pid {
        return SessionObservation::Reparented;
    }
    SessionObservation::Live
}

fn process_snapshot(parent_pid: u32) -> Result<Option<ProcessSnapshot>, AgentSessionError> {
    #[cfg(target_os = "macos")]
    {
        let pid = i32::try_from(parent_pid)
            .map_err(|_| AgentSessionError::InvalidPlatformOutput("parent PID"))?;
        let mut info = unsafe { std::mem::zeroed::<nix::libc::proc_bsdinfo>() };
        let expected_size = i32::try_from(std::mem::size_of_val(&info))
            .map_err(|_| AgentSessionError::InvalidPlatformOutput("process snapshot"))?;
        let actual_size = unsafe {
            nix::libc::proc_pidinfo(
                pid,
                nix::libc::PROC_PIDTBSDINFO,
                0,
                std::ptr::addr_of_mut!(info).cast(),
                expected_size,
            )
        };
        if actual_size == expected_size {
            let observed_pid = info.pbi_pid;
            if observed_pid != parent_pid {
                return Err(AgentSessionError::InvalidPlatformOutput("process PID"));
            }
            return Ok(Some(ProcessSnapshot {
                pid: observed_pid,
                start: format!("{}.{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
            }));
        }
        if actual_size != 0 {
            return Err(AgentSessionError::InvalidPlatformOutput("process snapshot"));
        }
        match kill(Pid::from_raw(pid), None) {
            Err(Errno::ESRCH) => Ok(None),
            Ok(()) | Err(_) => Err(AgentSessionError::Unsupported),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = parent_pid;
        Err(AgentSessionError::Unsupported)
    }
}

fn system_boot_id() -> Result<String, AgentSessionError> {
    #[cfg(target_os = "macos")]
    {
        let mut boot_time = unsafe { std::mem::zeroed::<nix::libc::timeval>() };
        let mut length = std::mem::size_of_val(&boot_time);
        let mut mib = [nix::libc::CTL_KERN, nix::libc::KERN_BOOTTIME];
        let result = unsafe {
            nix::libc::sysctl(
                mib.as_mut_ptr(),
                u32::try_from(mib.len()).expect("fixed MIB length"),
                std::ptr::addr_of_mut!(boot_time).cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 || length != std::mem::size_of_val(&boot_time) {
            return Err(AgentSessionError::Unsupported);
        }
        Ok(format!("macos:{}.{}", boot_time.tv_sec, boot_time.tv_usec))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(AgentSessionError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
fn current_parent_pid() -> Result<u32, AgentSessionError> {
    let parent_pid = unsafe { nix::libc::getppid() };
    u32::try_from(parent_pid).map_err(|_| AgentSessionError::Unsupported)
}

#[cfg(not(target_os = "macos"))]
fn current_parent_pid() -> Result<u32, AgentSessionError> {
    Err(AgentSessionError::Unsupported)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    fn identity() -> AgentSessionIdentity {
        AgentSessionIdentity {
            parent_pid: 42,
            parent_start: "Wed Aug 21 10:00:00 2026".to_owned(),
            boot_id: "boot-1".to_owned(),
        }
    }

    fn snapshot(_parent_pid: u32, start: &str) -> ProcessSnapshot {
        ProcessSnapshot {
            pid: 42,
            start: start.to_owned(),
        }
    }

    struct FakePlatformProbe {
        boot_ids: RefCell<VecDeque<Result<String, AgentSessionError>>>,
        parent_pids: RefCell<VecDeque<Result<u32, AgentSessionError>>>,
        snapshots: RefCell<VecDeque<Result<Option<ProcessSnapshot>, AgentSessionError>>>,
    }

    impl FakePlatformProbe {
        fn new(
            boot_ids: Vec<Result<String, AgentSessionError>>,
            parent_pids: Vec<Result<u32, AgentSessionError>>,
            snapshots: Vec<Result<Option<ProcessSnapshot>, AgentSessionError>>,
        ) -> Self {
            Self {
                boot_ids: RefCell::new(boot_ids.into()),
                parent_pids: RefCell::new(parent_pids.into()),
                snapshots: RefCell::new(snapshots.into()),
            }
        }
    }

    impl PlatformProbe for FakePlatformProbe {
        fn boot_id(&self) -> Result<String, AgentSessionError> {
            self.boot_ids
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(AgentSessionError::Unsupported))
        }

        fn current_parent_pid(&self) -> Result<u32, AgentSessionError> {
            self.parent_pids
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(AgentSessionError::Unsupported))
        }

        fn process_snapshot(
            &self,
            _parent_pid: u32,
        ) -> Result<Option<ProcessSnapshot>, AgentSessionError> {
            self.snapshots
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(AgentSessionError::Unsupported))
        }
    }

    #[test]
    fn classifies_injected_session_states_without_host_processes() {
        let identity = identity();
        assert_eq!(
            classify_observation(
                &identity,
                "boot-1",
                identity.parent_pid,
                Some(&snapshot(2, &identity.parent_start))
            ),
            SessionObservation::Live
        );
        assert_eq!(
            classify_observation(&identity, "boot-1", 9, None),
            SessionObservation::LostParent
        );
        assert_eq!(
            classify_observation(
                &identity,
                "boot-1",
                1,
                Some(&snapshot(2, &identity.parent_start))
            ),
            SessionObservation::Reparented
        );
        assert_eq!(
            classify_observation(
                &identity,
                "boot-2",
                identity.parent_pid,
                Some(&snapshot(2, &identity.parent_start))
            ),
            SessionObservation::Rebooted
        );
        assert_eq!(
            classify_observation(
                &identity,
                "boot-1",
                identity.parent_pid,
                Some(&snapshot(2, "other-start"))
            ),
            SessionObservation::Ambiguous
        );
    }

    #[test]
    fn malformed_identity_is_ambiguous() {
        let mut identity = identity();
        identity.parent_start.clear();
        assert_eq!(
            classify_observation(
                &identity,
                "boot-1",
                identity.parent_pid,
                Some(&snapshot(2, ""))
            ),
            SessionObservation::Ambiguous
        );
    }

    #[test]
    fn observe_rejects_structurally_malformed_identity_before_probing() {
        let mut empty_boot = identity();
        empty_boot.boot_id.clear();
        let probe = FakePlatformProbe::new(vec![Ok("boot-1".to_owned())], vec![], vec![]);
        assert_eq!(
            observe_with_probe(&empty_boot, &probe),
            SessionObservation::Ambiguous
        );

        let mut zero_pid = identity();
        zero_pid.parent_pid = 0;
        let probe = FakePlatformProbe::new(vec![], vec![], vec![]);
        assert_eq!(
            observe_with_probe(&zero_pid, &probe),
            SessionObservation::Ambiguous
        );

        let mut empty_start = identity();
        empty_start.parent_start.clear();
        let probe = FakePlatformProbe::new(vec![], vec![], vec![]);
        assert_eq!(
            observe_with_probe(&empty_start, &probe),
            SessionObservation::Ambiguous
        );
    }

    #[test]
    fn probe_drift_and_probe_errors_are_never_live() {
        let identity = identity();
        let parent_drift = FakePlatformProbe::new(
            vec![Ok("boot-1".to_owned()), Ok("boot-1".to_owned())],
            vec![Ok(42), Ok(1)],
            vec![Ok(Some(snapshot(2, &identity.parent_start)))],
        );
        assert_eq!(
            observe_with_probe(&identity, &parent_drift),
            SessionObservation::Ambiguous
        );

        let permission_error = FakePlatformProbe::new(
            vec![Ok("boot-1".to_owned())],
            vec![Ok(42)],
            vec![Err(AgentSessionError::Unsupported)],
        );
        assert_eq!(
            observe_with_probe(&identity, &permission_error),
            SessionObservation::Unsupported
        );

        let boot_drift = FakePlatformProbe::new(
            vec![Ok("boot-1".to_owned()), Ok("boot-2".to_owned())],
            vec![Ok(42), Ok(42)],
            vec![Ok(Some(snapshot(2, &identity.parent_start)))],
        );
        assert_eq!(
            observe_with_probe(&identity, &boot_drift),
            SessionObservation::Ambiguous
        );

        let boot_error =
            FakePlatformProbe::new(vec![Err(AgentSessionError::Unsupported)], vec![], vec![]);
        assert_eq!(
            observe_with_probe(&identity, &boot_error),
            SessionObservation::Unsupported
        );
    }

    #[test]
    fn capture_accepts_a_stable_parent_identity() {
        let probe = FakePlatformProbe::new(
            vec![Ok("boot-1".to_owned()), Ok("boot-1".to_owned())],
            vec![Ok(42), Ok(42)],
            vec![Ok(Some(snapshot(1, "start-1")))],
        );

        assert_eq!(
            capture_with_probe(42, &probe).expect("stable identity"),
            AgentSessionIdentity {
                parent_pid: 42,
                parent_start: "start-1".to_owned(),
                boot_id: "boot-1".to_owned(),
            }
        );
    }

    #[test]
    fn capability_digest_is_stable_and_does_not_include_raw_bytes() {
        assert_eq!(
            digest_capability(&[0; 32]),
            "sha256:6a99e1eb72d896ab541f7846f287968783ebf0d2faaa7324b3c11b36f4ab060e"
        );
    }

    #[test]
    fn session_record_serializes_only_the_capability_digest() {
        let record = AgentSessionRecord::from_identity_and_capability(&identity(), &[0; 32]);
        let serialized = serde_json::to_value(record).expect("session record JSON");

        assert_eq!(
            serialized.get("capability_digest"),
            Some(&serde_json::Value::String(digest_capability(&[0; 32])))
        );
        assert!(serialized.get("capability").is_none());
        assert_eq!(serialized.as_object().expect("record object").len(), 5);
    }

    #[cfg(target_os = "macos")]
    #[ignore = "requires stable host process and boot evidence outside a sandbox"]
    #[test]
    fn macos_inspector_captures_and_rechecks_the_current_parent() {
        let parent_pid = current_parent_pid().expect("current parent PID");
        let identity = SystemSessionInspector::capture(parent_pid).expect("stable identity");

        assert_eq!(identity.parent_pid, parent_pid);
        assert!(!identity.parent_start.is_empty());
        assert!(identity.boot_id.starts_with("macos:"));
        assert_eq!(
            SystemSessionInspector.observe(&identity),
            SessionObservation::Live
        );
    }
}
