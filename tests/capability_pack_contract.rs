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

use commit_ci_preflight::capability_pack::{
    CAPABILITY_PACK_SCHEMA_VERSION, CapabilityInputKindV1, CapabilityPackBindingV1,
    CapabilityPackEnvelopeV1, CapabilityPackError, CapabilityPackManifestV1,
    CapabilityRuntimeFeatureV1, MAX_CAPABILITY_PACK_BYTES,
};
const PINNED_SCHEMA: &str = include_str!("../schema/capability-pack-v1.schema.json");
use commit_ci_preflight::config::{ConfigError, RuntimeKind};
use commit_ci_preflight::process::{
    CancellationToken, CapturedStream, CleanupStatus, ExitOutcome, GenerationGuard, ProcessError,
    ProcessRequest, ProcessResult, ProcessTermination, RunIdentity, SupervisorPort,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, atomic::{AtomicU64, Ordering}};
use std::time::Duration;

const VALID: &str = include_str!("fixtures/capability-pack-v1/valid-minimal.toml");
const UNKNOWN_FIELD: &str = include_str!("fixtures/capability-pack-v1/unknown-field.toml");
const UNKNOWN_VERSION: &str = include_str!("fixtures/capability-pack-v1/unknown-version.toml");
const INVALID_IMAGE: &str = include_str!("fixtures/capability-pack-v1/invalid-image.toml");
const INVALID_LICENSE: &str = include_str!("fixtures/capability-pack-v1/invalid-license.toml");
const INVALID_PROVENANCE: &str =
    include_str!("fixtures/capability-pack-v1/invalid-provenance.toml");
const INVALID_PATH: &str = include_str!("fixtures/capability-pack-v1/invalid-path.toml");
const SHELL_ENTRYPOINT: &str = include_str!("fixtures/capability-pack-v1/shell-entrypoint.toml");
const DEPENDENCY_CYCLE: &str = include_str!("fixtures/capability-pack-v1/dependency-cycle.toml");
const REORDERED_VALID: &str =
    include_str!("fixtures/capability-pack-v1/valid-minimal-reordered.toml");
const PINNED_CANONICAL: &[u8] =
    include_bytes!("fixtures/capability-pack-v1/valid-minimal.canonical.json");
const PINNED_EXPANSION: &[u8] =
    include_bytes!("fixtures/capability-pack-v1/valid-minimal.strict-clippy.expansion.json");
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct ScriptedSupervisor {
    requests: Mutex<Vec<ProcessRequest>>,
    responses: Mutex<VecDeque<Result<ProcessResult, ProcessError>>>,
}

impl Default for ScriptedSupervisor {
    fn default() -> Self {
        Self::with_responses([])
    }
}

impl ScriptedSupervisor {
    fn with_success_stdout(stdout: &[u8]) -> Self {
        Self::with_responses([scripted_success(stdout)])
    }

    fn with_result(result: Result<ProcessResult, ProcessError>) -> Self {
        Self::with_responses([result])
    }

    fn with_responses(
        responses: impl IntoIterator<Item = Result<ProcessResult, ProcessError>>,
    ) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into_iter().collect()),
        }
    }

    fn call_count(&self) -> usize {
        self.requests.lock().expect("scripted request lock").len()
    }

    fn only_request(&self) -> ProcessRequest {
        let requests = self.requests.lock().expect("scripted request lock");
        assert_eq!(requests.len(), 1);
        requests[0].clone()
    }
}

impl SupervisorPort for ScriptedSupervisor {
    fn execute(
        &self,
        request: &ProcessRequest,
        _cancellation: &CancellationToken,
        _generation: &GenerationGuard,
    ) -> Result<ProcessResult, ProcessError> {
        self.requests
            .lock()
            .expect("scripted request lock")
            .push(request.clone());
        let response = self.responses
            .lock()
            .expect("scripted response lock")
            .pop_front()
            .unwrap_or(Err(ProcessError::Invariant("unexpected Git request")));
        response.map(|mut result| {
            result.identity = request.identity.clone();
            result
        })
    }
}

fn scripted_result(
    stdout: &[u8],
    stderr: &[u8],
    termination: ProcessTermination,
    exit: Option<ExitOutcome>,
    stdout_truncated: bool,
    stderr_truncated: bool,
) -> ProcessResult {
    ProcessResult {
        identity: RunIdentity {
            project: "scripted-supervisor".to_owned(),
            commit: None,
            config_digest: "sha256:scripted".to_owned(),
            generation: "test".to_owned(),
        },
        termination,
        cleanup: CleanupStatus::Verified,
        exit,
        stdout: CapturedStream::from_captured(stdout.to_vec(), stdout_truncated),
        stderr: CapturedStream::from_captured(stderr.to_vec(), stderr_truncated),
        elapsed_millis: 0,
    }
}

fn scripted_success(stdout: &[u8]) -> Result<ProcessResult, ProcessError> {
    Ok(scripted_result(
        stdout,
        b"",
        ProcessTermination::Completed,
        Some(ExitOutcome {
            success: true,
            code: Some(0),
        }),
        false,
        false,
    ))
}

fn scripted_nonzero(stderr: &[u8]) -> Result<ProcessResult, ProcessError> {
    Ok(scripted_result(
        b"",
        stderr,
        ProcessTermination::Completed,
        Some(ExitOutcome {
            success: false,
            code: Some(1),
        }),
        false,
        false,
    ))
}

fn scripted_timeout() -> Result<ProcessResult, ProcessError> {
    Ok(scripted_result(
        b"",
        b"",
        ProcessTermination::TimedOut,
        None,
        false,
        false,
    ))
}

fn scripted_truncated_stdout() -> Result<ProcessResult, ProcessError> {
    Ok(scripted_result(
        b"commit\n",
        b"",
        ProcessTermination::Completed,
        Some(ExitOutcome {
            success: true,
            code: Some(0),
        }),
        true,
        false,
    ))
}

fn scripted_output_error() -> ProcessError {
    ProcessError::Output(io::Error::other("scripted read failure"))
}

fn scripted_cleanup_error() -> ProcessError {
    ProcessError::CleanupUncertain {
        stage: "scripted cleanup",
        source: io::Error::other("scripted cleanup failure"),
    }
}

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

const M2_BASE_COMMIT: &str = "2e6286cc23584d5e82842aacf106c3bb5e7462df";
const HISTORICAL_CAPTURE_BYTES: usize = 65_536;

#[derive(Debug, PartialEq, Eq)]
enum HistoricalManifestError {
    InvalidRoot,
    InvalidObject,
    UnpinnedObject,
    InvalidPath,
    DeclaredSizeTooLarge,
    Process,
    Identity,
    Termination,
    Cleanup,
    Exit,
    Truncated,
    Utf8,
    BaseNotCommit,
    PathNotBlob,
    Length,
}

struct HistoricalGitReader<'a, R: SupervisorPort> {
    root: &'a Path,
    supervisor: &'a R,
}

impl<'a, R: SupervisorPort> HistoricalGitReader<'a, R> {
    fn new(root: &'a Path, supervisor: &'a R) -> Self {
        Self { root, supervisor }
    }

    fn object_type(&self, object: &str) -> Result<String, HistoricalManifestError> {
        if !is_lowercase_hex_object_id(object) {
            return Err(HistoricalManifestError::InvalidObject);
        }
        if object != M2_BASE_COMMIT {
            return Err(HistoricalManifestError::UnpinnedObject);
        }
        let object_type = self.raw_object_type(object)?;
        if object_type != "commit" {
            return Err(HistoricalManifestError::BaseNotCommit);
        }
        Ok(object_type)
    }

    fn blob(&self, path: &str, declared_bytes: u64) -> Result<Vec<u8>, HistoricalManifestError> {
        if !is_safe_historical_path(path) {
            return Err(HistoricalManifestError::InvalidPath);
        }
        let declared_bytes = usize::try_from(declared_bytes)
            .map_err(|_| HistoricalManifestError::DeclaredSizeTooLarge)?;
        if declared_bytes > HISTORICAL_CAPTURE_BYTES {
            return Err(HistoricalManifestError::DeclaredSizeTooLarge);
        }

        self.object_type(M2_BASE_COMMIT)?;
        let revision = format!("{M2_BASE_COMMIT}:{path}");
        if self.raw_object_type(&revision)? != "blob" {
            return Err(HistoricalManifestError::PathNotBlob);
        }
        let bytes = self.execute_git(vec![
            "cat-file".into(),
            "blob".into(),
            revision.into(),
        ])?;
        if bytes.len() != declared_bytes {
            return Err(HistoricalManifestError::Length);
        }
        Ok(bytes)
    }

    fn raw_object_type(&self, object: &str) -> Result<String, HistoricalManifestError> {
        let bytes = self.execute_git(vec!["cat-file".into(), "-t".into(), object.into()])?;
        let object_type = std::str::from_utf8(&bytes)
            .map_err(|_| HistoricalManifestError::Utf8)?
            .strip_suffix('\n')
            .ok_or(HistoricalManifestError::Utf8)?;
        if object_type.contains('\n') || !matches!(object_type, "commit" | "blob" | "tree") {
            return Err(HistoricalManifestError::Utf8);
        }
        Ok(object_type.to_owned())
    }

    fn execute_git(&self, mut command: Vec<OsString>) -> Result<Vec<u8>, HistoricalManifestError> {
        if !self.root.is_absolute() {
            return Err(HistoricalManifestError::InvalidRoot);
        }
        let mut argv = vec![
            "--no-replace-objects".into(),
            "--no-lazy-fetch".into(),
            "-C".into(),
            self.root.as_os_str().to_os_string(),
        ];
        argv.append(&mut command);
        let request = historical_request(self.root, argv);
        let cancellation = CancellationToken::default();
        let generation = GenerationGuard::new(request.identity.clone());
        let result = self
            .supervisor
            .execute(&request, &cancellation, &generation)
            .map_err(|_| HistoricalManifestError::Process)?;
        validate_historical_process_result(&request, result)
    }
}

fn historical_git_environment() -> BTreeMap<OsString, OsString> {
    BTreeMap::from([
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_NO_REPLACE_OBJECTS".into(), "1".into()),
        ("GIT_NO_LAZY_FETCH".into(), "1".into()),
        ("GIT_OPTIONAL_LOCKS".into(), "0".into()),
        ("GIT_LITERAL_PATHSPECS".into(), "1".into()),
    ])
}

fn historical_request(root: &Path, argv: Vec<OsString>) -> ProcessRequest {
    ProcessRequest {
        identity: RunIdentity {
            project: "m2-historical-contract".to_owned(),
            commit: Some(M2_BASE_COMMIT.to_owned()),
            config_digest: "sha256:m2-historical-contract".to_owned(),
            generation: "test".to_owned(),
        },
        program: "git".into(),
        argv,
        current_dir: root.to_path_buf(),
        environment: historical_git_environment(),
        timeout: Duration::from_secs(2),
        max_capture_bytes: HISTORICAL_CAPTURE_BYTES,
    }
}

fn validate_historical_process_result(
    request: &ProcessRequest,
    result: ProcessResult,
) -> Result<Vec<u8>, HistoricalManifestError> {
    if result.identity != request.identity {
        return Err(HistoricalManifestError::Identity);
    }
    if result.termination != ProcessTermination::Completed {
        return Err(HistoricalManifestError::Termination);
    }
    if result.cleanup != CleanupStatus::Verified {
        return Err(HistoricalManifestError::Cleanup);
    }
    if !result.exit.is_some_and(|exit| exit.success) {
        return Err(HistoricalManifestError::Exit);
    }
    if result.stdout.truncated || result.stderr.truncated {
        return Err(HistoricalManifestError::Truncated);
    }
    Ok(result.stdout.bytes)
}

fn is_lowercase_hex_object_id(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn is_safe_historical_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\0')
        && !value.contains(':')
        && !value.contains('\\')
        && !Path::new(value).is_absolute()
        && Path::new(value)
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)))
}

#[test]
fn historical_reader_rejects_invalid_input_without_calling_git() {
    let supervisor = ScriptedSupervisor::default();
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);

    assert!(reader.object_type("not-a-commit").is_err());
    assert!(reader.blob("../CHANGELOG.md", 1).is_err());
    assert!(reader.blob("CHANGELOG.md:other", 1).is_err());
    assert_eq!(supervisor.call_count(), 0);
}

#[test]
fn historical_reader_rejects_oversized_declaration_without_calling_git() {
    let supervisor = ScriptedSupervisor::default();
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);

    assert!(reader
        .blob("CHANGELOG.md", (HISTORICAL_CAPTURE_BYTES + 1) as u64)
        .is_err());
    assert_eq!(supervisor.call_count(), 0);
}

#[test]
fn historical_reader_builds_hardened_git_request() {
    let supervisor = ScriptedSupervisor::with_success_stdout(b"commit\n");
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);

    assert_eq!(reader.object_type(M2_BASE_COMMIT).unwrap(), "commit");
    let request = supervisor.only_request();
    assert_eq!(request.program, "git");
    assert_eq!(request.timeout, Duration::from_secs(2));
    assert_eq!(request.max_capture_bytes, HISTORICAL_CAPTURE_BYTES);
    assert_eq!(request.environment, historical_git_environment());
    assert_eq!(
        request.argv,
        vec![
            "--no-replace-objects".into(),
            "--no-lazy-fetch".into(),
            "-C".into(),
            repo_root().as_os_str().to_os_string(),
            "cat-file".into(),
            "-t".into(),
            M2_BASE_COMMIT.into(),
        ]
    );
}

#[test]
fn historical_reader_rejects_non_commit_base() {
    let supervisor = ScriptedSupervisor::with_success_stdout(b"tree\n");
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);
    assert!(reader.object_type(M2_BASE_COMMIT).is_err());
}

#[test]
fn historical_reader_rejects_missing_or_non_blob_path() {
    let supervisor = ScriptedSupervisor::with_responses([
        scripted_success(b"commit\n"),
        scripted_nonzero(b"missing\n"),
    ]);
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);
    assert!(reader.blob("CHANGELOG.md", 1).is_err());
}

#[test]
fn historical_reader_rejects_timeout_and_truncated_capture() {
    let timeout_supervisor = ScriptedSupervisor::with_result(scripted_timeout());
    let timeout_reader = HistoricalGitReader::new(repo_root(), &timeout_supervisor);
    assert!(timeout_reader.object_type(M2_BASE_COMMIT).is_err());

    let truncated_supervisor = ScriptedSupervisor::with_result(scripted_truncated_stdout());
    let truncated_reader = HistoricalGitReader::new(repo_root(), &truncated_supervisor);
    assert!(truncated_reader.object_type(M2_BASE_COMMIT).is_err());
}

#[test]
fn historical_reader_rejects_supervisor_and_cleanup_errors() {
    let output_supervisor = ScriptedSupervisor::with_result(Err(scripted_output_error()));
    let output_reader = HistoricalGitReader::new(repo_root(), &output_supervisor);
    assert!(output_reader.object_type(M2_BASE_COMMIT).is_err());

    let cleanup_supervisor = ScriptedSupervisor::with_result(Err(scripted_cleanup_error()));
    let cleanup_reader = HistoricalGitReader::new(repo_root(), &cleanup_supervisor);
    assert!(cleanup_reader.object_type(M2_BASE_COMMIT).is_err());
}

#[test]
fn historical_reader_rejects_blob_length_mismatch() {
    let supervisor = ScriptedSupervisor::with_responses([
        scripted_success(b"commit\n"),
        scripted_success(b"blob\n"),
        scripted_success(b"short"),
    ]);
    let reader = HistoricalGitReader::new(repo_root(), &supervisor);
    assert!(reader.blob("CHANGELOG.md", 6).is_err());
}

#[test]
fn generated_capability_pack_schema_matches_pinned_bytes() {
    assert_eq!(
        commit_ci_preflight::capability_pack::capability_pack_schema_json()
            .expect("capability pack schema"),
        PINNED_SCHEMA
    );
}

#[test]
fn m2_manifest_matches_exact_file_bytes() {
    const EXPECTED_PATHS: [&str; 18] = [
        "CHANGELOG.md",
        "docs/CAPABILITY_PACKS.md",
        "schema/capability-pack-v1.schema.json",
        "src/capability_pack.rs",
        "src/lib.rs",
        "tests/capability_pack_contract.rs",
        "tests/fixtures/capability-pack-v1/dependency-cycle.toml",
        "tests/fixtures/capability-pack-v1/invalid-image.toml",
        "tests/fixtures/capability-pack-v1/invalid-license.toml",
        "tests/fixtures/capability-pack-v1/invalid-path.toml",
        "tests/fixtures/capability-pack-v1/invalid-provenance.toml",
        "tests/fixtures/capability-pack-v1/shell-entrypoint.toml",
        "tests/fixtures/capability-pack-v1/unknown-field.toml",
        "tests/fixtures/capability-pack-v1/unknown-version.toml",
        "tests/fixtures/capability-pack-v1/valid-minimal.canonical.json",
        "tests/fixtures/capability-pack-v1/valid-minimal-reordered.toml",
        "tests/fixtures/capability-pack-v1/valid-minimal.strict-clippy.expansion.json",
        "tests/fixtures/capability-pack-v1/valid-minimal.toml",
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = root.join(
        "docs/superpowers/programmes/2026-08-30-capability-packs-clean-architecture/m2-manifest.json",
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("read M2 manifest"))
            .expect("parse M2 manifest");
    let object = manifest.as_object().expect("M2 manifest object");
    assert_eq!(
        object
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["base_commit", "files", "schema_version"])
    );
    assert_eq!(manifest["schema_version"], "1.0");
    assert_eq!(
        manifest["base_commit"],
        "2e6286cc23584d5e82842aacf106c3bb5e7462df"
    );
    let entries = manifest["files"].as_array().expect("M2 manifest files");
    let paths = entries
        .iter()
        .map(|entry| {
            let entry = entry.as_object().expect("M2 manifest file entry");
            assert_eq!(
                entry
                    .keys()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>(),
                std::collections::BTreeSet::from(["bytes", "path", "sha256"])
            );
            entry["path"].as_str().expect("M2 manifest path")
        })
        .collect::<Vec<_>>();
    assert_eq!(paths, EXPECTED_PATHS);

    for entry in entries {
        let relative = entry["path"].as_str().expect("M2 manifest path");
        let bytes = std::fs::read(root.join(relative)).expect("read manifested file");
        assert_eq!(
            entry["bytes"].as_u64(),
            Some(bytes.len() as u64),
            "{relative}"
        );
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            entry["sha256"].as_str(),
            Some(format!("sha256:{digest}").as_str()),
            "{relative}"
        );
    }
}

fn valid_binding() -> CapabilityPackBindingV1 {
    CapabilityPackBindingV1 {
        project: "example/project".to_owned(),
        profile_id: "strict-clippy".to_owned(),
        receipt: commit_ci_preflight::config::ReceiptConfig {
            output: ".ccp/receipt.json".to_owned(),
            freshness_seconds: 300,
        },
    }
}
fn binding_for(profile_id: &str) -> CapabilityPackBindingV1 {
    CapabilityPackBindingV1 {
        profile_id: profile_id.to_owned(),
        ..valid_binding()
    }
}
fn binding_for_project(project: &str) -> CapabilityPackBindingV1 {
    CapabilityPackBindingV1 {
        project: project.to_owned(),
        ..valid_binding()
    }
}
fn unique_test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ccp-{label}-{}-{}",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}
fn valid_manifest_with_argv(argv: Vec<String>) -> String {
    let original = "argv = [\"cargo\", \"clippy\", \"--locked\", \"--offline\", \"--all-targets\", \"--all-features\", \"--\", \"-D\", \"warnings\"]";
    let replacement = format!(
        "argv = {}",
        serde_json::to_string(&argv).expect("argv JSON")
    );
    assert!(VALID.contains(original));
    VALID.replacen(original, &replacement, 1)
}

fn validate_fixture(source: &str) -> Result<CapabilityPackEnvelopeV1, CapabilityPackError> {
    CapabilityPackManifestV1::parse(source)?.validate()
}

fn valid_manifest() -> CapabilityPackManifestV1 {
    CapabilityPackManifestV1::parse(VALID).expect("valid input model")
}

fn assert_invalid_field(
    mutate: impl FnOnce(&mut CapabilityPackManifestV1),
    expected: &'static str,
) {
    let mut manifest = valid_manifest();
    mutate(&mut manifest);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(field)) if field == expected
    ));
}

fn assert_invalid_freshness(created: Option<&str>, max_age_seconds: Option<u64>) {
    let mut manifest = valid_manifest();
    let input = manifest.profiles[0]
        .inputs
        .first_mut()
        .expect("valid fixture database input");
    input.snapshot_created_at_utc = created.map(str::to_owned);
    input.max_age_seconds = max_age_seconds;
    let expected = if created.is_some() && max_age_seconds.is_some() {
        "profiles.inputs.snapshot_created_at_utc"
    } else {
        "profiles.inputs.freshness"
    };
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(field)) if field == expected
    ));
}

#[test]
fn strict_manifest_parser_accepts_only_schema_1_0_toml() {
    let manifest = CapabilityPackManifestV1::parse(VALID).expect("valid manifest");
    assert_eq!(manifest.schema_version, CAPABILITY_PACK_SCHEMA_VERSION);
    assert_eq!(manifest.pack_id, "ccp.rust-minimal");
    assert!(CapabilityPackManifestV1::parse(UNKNOWN_FIELD).is_err());
    assert!(matches!(
        CapabilityPackManifestV1::parse(UNKNOWN_VERSION)
            .and_then(CapabilityPackManifestV1::validate),
        Err(CapabilityPackError::UnsupportedSchemaVersion(version)) if version == "2.0"
    ));
}

#[test]
fn manifest_parser_rejects_more_than_one_mebibyte_before_toml_decode() {
    let oversized = "x".repeat(MAX_CAPABILITY_PACK_BYTES + 1);
    assert!(matches!(
        CapabilityPackManifestV1::parse(&oversized),
        Err(CapabilityPackError::ManifestTooLarge { actual, maximum })
            if actual == MAX_CAPABILITY_PACK_BYTES + 1 && maximum == MAX_CAPABILITY_PACK_BYTES
    ));
}

#[test]
fn validator_rejects_untrusted_metadata_and_unsafe_execution_shape() {
    assert!(matches!(
        validate_fixture(INVALID_LICENSE),
        Err(CapabilityPackError::InvalidField("license"))
    ));
    assert!(matches!(
        validate_fixture(INVALID_PROVENANCE),
        Err(CapabilityPackError::InvalidField("profiles.tools.digest"))
    ));
    assert!(
        matches!(validate_fixture(SHELL_ENTRYPOINT), Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy")
    );
    assert!(matches!(
        validate_fixture(INVALID_IMAGE),
        Err(CapabilityPackError::Config(ConfigError::InvalidField(
            "runtime.image"
        )))
    ));
    assert!(matches!(
        validate_fixture(INVALID_PATH),
        Err(CapabilityPackError::Config(ConfigError::InvalidField(
            "cache.mount_path"
        )))
    ));
    assert!(matches!(
        validate_fixture(DEPENDENCY_CYCLE),
        Err(CapabilityPackError::Config(ConfigError::DependencyCycle(_)))
    ));
}

#[test]
fn validator_rejects_shells_invoked_through_env() {
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].argv = vec![
        "/usr/bin/env".to_owned(),
        "-i".to_owned(),
        "bash".to_owned(),
        "-c".to_owned(),
        "cargo clippy".to_owned(),
    ];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy"
    ));
}

#[test]
fn validator_rejects_shell_entrypoint_bypasses() {
    for argv in [
        vec![
            "env".to_owned(),
            "X=1".to_owned(),
            "bash".to_owned(),
            "-c".to_owned(),
            "cargo clippy".to_owned(),
        ],
        vec![
            "env".to_owned(),
            "-S".to_owned(),
            "bash -c cargo clippy".to_owned(),
        ],
        vec![
            "C:\\Windows\\System32\\cmd.exe".to_owned(),
            "/c".to_owned(),
            "cargo clippy".to_owned(),
        ],
    ] {
        let mut manifest = valid_manifest();
        manifest.profiles[0].checks[0].argv = argv;
        assert!(matches!(
            manifest.validate(),
            Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy"
        ));
    }
}

#[test]
fn validator_rejects_shells_after_env_option_operands() {
    for argv in [
        vec!["env", "-u", "FOO", "bash", "-c", "cargo clippy"],
        vec!["env", "-C", "/tmp", "bash", "-c", "cargo clippy"],
        vec!["env", "--unset", "FOO", "bash", "-c", "cargo clippy"],
        vec!["env", "--chdir", "/tmp", "bash", "-c", "cargo clippy"],
    ] {
        let mut manifest = valid_manifest();
        manifest.profiles[0].checks[0].argv = argv.into_iter().map(str::to_owned).collect();
        assert!(matches!(
            manifest.validate(),
            Err(CapabilityPackError::ShellEntrypoint(id)) if id == "clippy"
        ));
    }
}

#[test]
fn normalized_pack_is_order_independent_and_matches_pinned_canonical_bytes() {
    let first = validate_fixture(VALID).expect("first pack");
    let reordered = validate_fixture(REORDERED_VALID).expect("reordered pack");
    assert_eq!(first.pack_digest, reordered.pack_digest);
    assert_eq!(first.inspection(), reordered.inspection());
    assert_eq!(
        first.canonical_bytes().expect("canonical bytes"),
        PINNED_CANONICAL
    );
}

#[test]
fn freshness_metadata_is_required_as_an_atomic_pair() {
    assert_invalid_freshness(None, Some(86_400));
    assert_invalid_freshness(Some("2026-08-30T00:00:00Z"), None);
    assert_invalid_freshness(Some("2026-02-30T00:00:00Z"), Some(86_400));
}

#[test]
fn validator_enforces_pack_and_profile_collection_bounds() {
    assert_invalid_field(|m| m.profiles.clear(), "profiles");
    assert_invalid_field(|m| m.upstream_sources.clear(), "upstream_sources");
    assert_invalid_field(|m| m.profiles[0].tools.clear(), "profiles.tools");
    assert_invalid_field(
        |m| m.profiles[0].supported_hosts.clear(),
        "profiles.supported_hosts",
    );
    assert_invalid_field(
        |m| m.profiles[0].target_platforms.clear(),
        "profiles.target_platforms",
    );
    assert_invalid_field(
        |m| m.profiles[0].required_runtime_features.clear(),
        "profiles.required_runtime_features",
    );
    assert_invalid_field(
        |m| m.profiles[0].known_blind_spots.clear(),
        "profiles.known_blind_spots",
    );

    let mut manifest = valid_manifest();
    let profile = manifest.profiles[0].clone();
    manifest.profiles = (0..33)
        .map(|index| {
            let mut value = profile.clone();
            value.id = format!("profile-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles",
            actual: 33,
            maximum: 32
        })
    ));
    let mut manifest = valid_manifest();
    let source = manifest.upstream_sources[0].clone();
    manifest.upstream_sources = (0..17)
        .map(|index| {
            let mut value = source.clone();
            value.id = format!("source-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "upstream_sources",
            actual: 17,
            maximum: 16
        })
    ));
    let mut manifest = valid_manifest();
    let tool = manifest.profiles[0].tools[0].clone();
    manifest.profiles[0].tools = (0..33)
        .map(|index| {
            let mut value = tool.clone();
            value.id = format!("tool-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.tools",
            actual: 33,
            maximum: 32
        })
    ));
    let mut manifest = valid_manifest();
    let input = manifest.profiles[0].inputs[0].clone();
    manifest.profiles[0].inputs = (0..65)
        .map(|index| {
            let mut value = input.clone();
            value.id = format!("input-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.inputs",
            actual: 65,
            maximum: 64
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].supported_hosts = vec![manifest.profiles[0].supported_hosts[0]; 9];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.supported_hosts",
            actual: 9,
            maximum: 8
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].target_platforms = vec![manifest.profiles[0].target_platforms[0]; 9];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.target_platforms",
            actual: 9,
            maximum: 8
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].required_runtime_features =
        vec![CapabilityRuntimeFeatureV1::NoNetwork; 17];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.required_runtime_features",
            actual: 17,
            maximum: 16
        })
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].known_blind_spots = (0..33).map(|i| format!("blind-{i}")).collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::TooManyItems {
            field: "profiles.known_blind_spots",
            actual: 33,
            maximum: 32
        })
    ));
}

#[test]
fn validator_reuses_embedded_config_collection_checks() {
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks.clear();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::NoChecks))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].required = false;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::NoRequiredChecks))
    ));
    let mut manifest = valid_manifest();
    let check = manifest.profiles[0].checks[0].clone();
    manifest.profiles[0].checks = (0..129)
        .map(|index| {
            let mut value = check.clone();
            value.id = format!("check-{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::TooManyItems {
            field: "checks",
            actual: 129,
            maximum: 128
        }))
    ));
    let mut manifest = valid_manifest();
    let cache = manifest.profiles[0].caches[0].clone();
    manifest.profiles[0].caches = (0..33)
        .map(|index| {
            let mut value = cache.clone();
            value.id = format!("cache-{index}");
            value.mount_path = format!(".cache/{index}");
            value
        })
        .collect();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::TooManyItems {
            field: "caches",
            actual: 33,
            maximum: 32
        }))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].argv = vec!["command".to_owned(); 65];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::InvalidField(
            "check.argv"
        )))
    ));
}

#[test]
fn validator_rejects_malformed_metadata_and_duplicates() {
    assert_invalid_field(|m| m.description = "x".repeat(4097), "description");
    assert_invalid_field(|m| m.pack_id = "bad id".to_owned(), "pack_id");
    assert_invalid_field(|m| m.profiles[0].id = "bad id".to_owned(), "profiles.id");
    assert_invalid_field(
        |m| m.upstream_sources[0].id = "bad id".to_owned(),
        "upstream_sources.id",
    );
    assert_invalid_field(
        |m| m.profiles[0].tools[0].id = "bad id".to_owned(),
        "profiles.tools.id",
    );
    assert_invalid_field(
        |m| m.profiles[0].inputs[0].id = "bad id".to_owned(),
        "profiles.inputs.id",
    );
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].clone();
    manifest.profiles.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "profiles.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.upstream_sources[0].clone();
    manifest.upstream_sources.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "upstream_sources.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].tools[0].clone();
    manifest.profiles[0].tools.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "profiles.tools.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].inputs[0].clone();
    manifest.profiles[0].inputs.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateId {
            field: "profiles.inputs.id",
            ..
        })
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].checks[0].clone();
    manifest.profiles[0].checks.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateId {
            field: "check.id",
            ..
        }))
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].caches[0].clone();
    manifest.profiles[0].caches.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateId {
            field: "cache.id",
            ..
        }))
    ));
}

#[test]
fn validator_rejects_duplicate_values_and_runtime_feature_contract_breaks() {
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].supported_hosts[0];
    manifest.profiles[0].supported_hosts.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.supported_hosts"
        ))
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].target_platforms[0];
    manifest.profiles[0].target_platforms.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.target_platforms"
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0]
        .required_runtime_features
        .push(CapabilityRuntimeFeatureV1::NoNetwork);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.required_runtime_features"
        ))
    ));
    let mut manifest = valid_manifest();
    let value = manifest.profiles[0].known_blind_spots[0].clone();
    manifest.profiles[0].known_blind_spots.push(value);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::DuplicateValue(
            "profiles.known_blind_spots"
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].depends_on = vec!["clippy".to_owned(), "clippy".to_owned()];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateValue(
            "check.depends_on"
        )))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].artifacts = vec!["artifact".to_owned(), "artifact".to_owned()];
    manifest.profiles[0]
        .required_runtime_features
        .push(CapabilityRuntimeFeatureV1::BoundedArtifacts);
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(ConfigError::DuplicateValue(
            "check.artifacts"
        )))
    ));
    assert_invalid_field(
        |m| {
            m.profiles[0].required_runtime_features.pop();
        },
        "profiles.required_runtime_features",
    );
    let mut manifest = valid_manifest();
    manifest.profiles[0].checks[0].artifacts = vec!["artifact".to_owned()];
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(
            "profiles.required_runtime_features"
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].caches.clear();
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(
            "profiles.required_runtime_features"
        ))
    ));
    assert_invalid_field(
        |m| m.profiles[0].runtime.kind = RuntimeKind::Host,
        "profiles.runtime.kind",
    );
    assert_invalid_field(
        |m| m.profiles[0].runtime.network = true,
        "profiles.runtime.network",
    );
}

#[test]
fn validator_enforces_value_syntax_and_input_freshness() {
    for value in ["v1.0.0", "1.0", "01.0.0", "1.0.0-alpha"] {
        assert_invalid_field(|m| m.pack_version = value.to_owned(), "pack_version");
    }
    assert_invalid_field(
        |m| m.profiles[0].tools[0].version.clear(),
        "profiles.tools.version",
    );
    assert_invalid_field(
        |m| m.profiles[0].tools[0].version = "bad\u{0000}".to_owned(),
        "profiles.tools.version",
    );
    assert_invalid_field(
        |m| m.profiles[0].tools[0].version = "x".repeat(4097),
        "profiles.tools.version",
    );
    for value in [
        "http://example.com",
        "https://user@example.com",
        "https://example.com/space here",
        "https://example.com/\u{0000}",
    ] {
        assert_invalid_field(
            |m| m.upstream_sources[0].url = value.to_owned(),
            "upstream_sources.url",
        );
    }
    assert_invalid_field(
        |m| m.profiles[0].tools[0].url = "http://example.com".to_owned(),
        "profiles.tools.url",
    );
    assert_invalid_field(
        |m| m.profiles[0].inputs[0].url = "http://example.com".to_owned(),
        "profiles.inputs.url",
    );
    for value in ["NOASSERTION", "NONE", "Apache-2.0 OR MIT"] {
        assert_invalid_field(|m| m.license = value.to_owned(), "license");
    }
    assert_invalid_field(|m| m.license = "x".repeat(129), "license");
    assert_invalid_field(
        |m| m.profiles[0].tools[0].license = "Apache-2.0 OR MIT".to_owned(),
        "profiles.tools.license",
    );
    assert_invalid_field(|m| m.description.clear(), "description");
    assert_invalid_field(
        |m| m.profiles[0].description.clear(),
        "profiles.description",
    );
    assert_invalid_field(
        |m| m.profiles[0].pass_semantics.clear(),
        "profiles.pass_semantics",
    );
    assert_invalid_freshness(None, Some(86_400));
    assert_invalid_field(
        |m| m.profiles[0].inputs[0].max_age_seconds = Some(0),
        "profiles.inputs.max_age_seconds",
    );
    let mut manifest = valid_manifest();
    manifest.profiles[0].inputs[0].kind = CapabilityInputKindV1::Rules;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::InvalidField(
            "profiles.inputs.freshness"
        ))
    ));
}

#[test]
fn parser_requires_explicit_storage_and_schema_13_runtime_policy() {
    let without_storage = VALID.replacen("[profiles.storage]\nmin_free_bytes = 1048576\nreceipt_journal_reserve_bytes = 4096\nmax_cache_growth_bytes = 1048576\n\n", "", 1);
    assert!(matches!(
        CapabilityPackManifestV1::parse(&without_storage),
        Err(CapabilityPackError::Parse(_))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].runtime.pull_policy = None;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(
            ConfigError::MissingRuntimeCapabilityPolicy
        ))
    ));
    let mut manifest = valid_manifest();
    manifest.profiles[0].runtime.swap_mode = None;
    assert!(matches!(
        manifest.validate(),
        Err(CapabilityPackError::Config(
            ConfigError::MissingRuntimeCapabilityPolicy
        ))
    ));
}

#[test]
fn canonical_bytes_rejects_tampered_digest() {
    let mut envelope = validate_fixture(VALID).expect("valid envelope");
    envelope.pack_digest = "sha256:bad".to_owned();
    assert!(matches!(
        envelope.canonical_bytes(),
        Err(CapabilityPackError::PackDigestMismatch)
    ));
}

#[test]
fn one_explicit_profile_expands_to_one_existing_plan_envelope() {
    let pack = validate_fixture(VALID).expect("pack");
    let expansion = pack.expand(valid_binding()).expect("expansion");
    assert_eq!(expansion.pack_digest, pack.pack_digest);
    assert_eq!(expansion.profile_id, "strict-clippy");
    assert_eq!(expansion.execution_plan.plan.project, "example/project");
    assert_eq!(expansion.execution_plan.plan.schema_version, "1.3");
    assert_eq!(
        expansion.canonical_bytes().expect("canonical expansion"),
        PINNED_EXPANSION
    );
}

#[test]
fn inspection_and_expansion_never_execute_declared_argv() {
    let root = unique_test_root("pack-inertness");
    std::fs::create_dir_all(&root).expect("create owned test root");
    let marker = root.join("must-not-exist");
    let source = valid_manifest_with_argv(vec![
        "/usr/bin/touch".to_owned(),
        marker.display().to_string(),
    ]);
    let pack = CapabilityPackManifestV1::parse(&source)
        .and_then(CapabilityPackManifestV1::validate)
        .expect("inert pack");
    let _inspection = pack.inspection();
    let _expansion = pack.expand(valid_binding()).expect("inert expansion");
    assert!(!marker.exists());
    std::fs::remove_dir(&root).expect("remove empty owned test root");
}

#[test]
fn expansion_rejects_unknown_profile_and_invalid_repository_binding() {
    let pack = validate_fixture(VALID).expect("pack");
    assert!(
        matches!(pack.expand(binding_for("missing")), Err(CapabilityPackError::UnknownProfile(id)) if id == "missing")
    );
    assert!(matches!(
        pack.expand(binding_for_project("not-a-repository")),
        Err(CapabilityPackError::Config(ConfigError::InvalidField(
            "project"
        )))
    ));
}

#[test]
fn expansion_canonical_bytes_rejects_malformed_pack_digest() {
    let pack = validate_fixture(VALID).expect("pack");
    let mut expansion = pack.expand(valid_binding()).expect("expansion");
    for digest in [
        "sha256:".to_owned(),
        "sha256:ABC".to_owned(),
        "sha256:".to_owned() + &("0".repeat(63) + "g"),
    ] {
        expansion.pack_digest = digest;
        assert!(matches!(
            expansion.canonical_bytes(),
            Err(CapabilityPackError::InvalidField("pack_digest"))
        ));
    }
}

#[test]
fn validator_rejects_malformed_https_authorities() {
    for value in [
        "https://?query",
        "https:///path",
        "https://:443",
        "https://example.com:",
        "https://example.com:not-a-port",
        "https://example.com:65536",
        "https://example.com:80:81",
        "https://bad_host.example",
        "https://999.999.999.999",
        "https://[::1",
        "https://[]",
        "https://[::1]suffix",
        "https://[::1]:",
        "https://[::1]:65536",
        "https://example.com/#fragment",
    ] {
        assert_invalid_field(
            |manifest| manifest.upstream_sources[0].url = value.to_owned(),
            "upstream_sources.url",
        );
    }
    let mut manifest = valid_manifest();
    manifest.upstream_sources[0].url = "https://[2001:db8::1]:443/path?query".to_owned();
    manifest
        .validate()
        .expect("valid bracketed IPv6 authority with port");
    let mut manifest = valid_manifest();
    manifest.upstream_sources[0].url = "https://[::1]".to_owned();
    manifest
        .validate()
        .expect("valid bracketed IPv6 authority without port");
}

#[test]
fn envelope_identity_seal_rejects_public_pack_mutation_before_expand() {
    let mut pack = validate_fixture(VALID).expect("pack");
    pack.pack.pack_id = "other-pack".to_owned();
    assert!(matches!(
        pack.expand(valid_binding()),
        Err(CapabilityPackError::PackDigestMismatch)
    ));
}

#[test]
fn expansion_identity_seal_rejects_replacement_with_another_valid_plan() {
    let pack = validate_fixture(VALID).expect("pack");
    let mut expansion = pack.expand(valid_binding()).expect("expansion");
    let replacement = pack
        .expand(binding_for_project("other/project"))
        .expect("replacement expansion");
    expansion.execution_plan = replacement.execution_plan;
    assert!(matches!(
        expansion.canonical_bytes(),
        Err(CapabilityPackError::PackDigestMismatch)
    ));
}

#[test]
fn envelope_debug_redacts_fixed_environment_literals() {
    let mut manifest = valid_manifest();
    manifest.profiles[0].environment.fixed.insert(
        "CAPABILITY_PACK_TEST_TOKEN".to_owned(),
        "fixed-environment-literal-must-not-appear".to_owned(),
    );
    let pack = manifest.validate().expect("pack");
    assert!(!format!("{pack:?}").contains("fixed-environment-literal-must-not-appear"));
}

#[test]
fn expansion_debug_redacts_fixed_environment_literals() {
    let mut manifest = valid_manifest();
    manifest.profiles[0].environment.fixed.insert(
        "CAPABILITY_PACK_EXPANSION_TEST_TOKEN".to_owned(),
        "expansion-fixed-environment-literal-must-not-appear".to_owned(),
    );
    let expansion = manifest
        .validate()
        .and_then(|pack| pack.expand(valid_binding()))
        .expect("expansion");
    assert!(
        !format!("{expansion:?}").contains("expansion-fixed-environment-literal-must-not-appear")
    );
}

#[test]
fn load_rejects_non_regular_paths_and_bounds_reads() {
    let root = unique_test_root("pack-load");
    std::fs::create_dir_all(&root).expect("create owned test root");
    assert!(matches!(
        CapabilityPackManifestV1::load(&root),
        Err(CapabilityPackError::Io { path, source })
            if path == root && source.kind() == std::io::ErrorKind::InvalidInput
    ));
    let oversized = root.join("oversized.toml");
    std::fs::write(&oversized, vec![b'x'; MAX_CAPABILITY_PACK_BYTES + 1])
        .expect("write oversized manifest");
    assert!(matches!(
        CapabilityPackManifestV1::load(&oversized),
        Err(CapabilityPackError::ManifestTooLarge { actual, maximum })
            if actual == MAX_CAPABILITY_PACK_BYTES + 1 && maximum == MAX_CAPABILITY_PACK_BYTES
    ));
    std::fs::remove_file(&oversized).expect("remove oversized manifest");
    std::fs::remove_dir(&root).expect("remove empty owned test root");
}
