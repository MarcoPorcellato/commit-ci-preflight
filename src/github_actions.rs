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

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

use saphyr::{LoadableYamlNode, MappingOwned, YamlOwned};
use saphyr_parser::{Event, Parser};
use schemars::{JsonSchema, schema_for};
use serde::Serialize;

pub const COMPATIBILITY_SCHEMA_VERSION: &str = "1.0";
pub const MAX_WORKFLOW_BYTES: usize = 1_048_576;
pub const MAX_JOBS: usize = 64;
pub const MAX_STEPS_PER_JOB: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeatureDisposition {
    Translated,
    ManualReview,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CompatibilityFindingV1 {
    pub path: String,
    pub feature: String,
    pub disposition: FeatureDisposition,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ProposedCheckV1 {
    pub id: String,
    pub source_path: String,
    pub shell: String,
    pub command: String,
    pub working_directory: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MigrationReadiness {
    ReadyForConfigAuthoring,
    ManualReviewRequired,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct CompatibilitySummaryV1 {
    pub translated: usize,
    pub manual_review: usize,
    pub unsupported: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GithubActionsCompatibilityReportV1 {
    pub schema_version: String,
    pub workflow_name: Option<String>,
    pub readiness: MigrationReadiness,
    pub summary: CompatibilitySummaryV1,
    pub environment_names: Vec<String>,
    pub proposed_checks: Vec<ProposedCheckV1>,
    pub findings: Vec<CompatibilityFindingV1>,
    pub executable_config_emitted: bool,
}

impl GithubActionsCompatibilityReportV1 {
    pub fn json_bytes(&self) -> Result<Vec<u8>, GithubActionsError> {
        serde_json::to_vec(self).map_err(GithubActionsError::Serialize)
    }
}

pub fn compatibility_report_schema_json() -> Result<String, GithubActionsError> {
    let schema = schema_for!(GithubActionsCompatibilityReportV1);
    let mut output =
        serde_json::to_string_pretty(&schema).map_err(GithubActionsError::Serialize)?;
    output.push('\n');
    Ok(output)
}

pub fn analyze_workflow_file(
    path: &Path,
) -> Result<GithubActionsCompatibilityReportV1, GithubActionsError> {
    let metadata = fs::metadata(path).map_err(|source| GithubActionsError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if size > MAX_WORKFLOW_BYTES {
        return Err(GithubActionsError::WorkflowTooLarge {
            actual: size,
            maximum: MAX_WORKFLOW_BYTES,
        });
    }
    let input = fs::read_to_string(path).map_err(|source| GithubActionsError::Io {
        path: path.display().to_string(),
        source,
    })?;
    analyze_workflow(&input)
}

pub fn analyze_workflow(
    input: &str,
) -> Result<GithubActionsCompatibilityReportV1, GithubActionsError> {
    if input.len() > MAX_WORKFLOW_BYTES {
        return Err(GithubActionsError::WorkflowTooLarge {
            actual: input.len(),
            maximum: MAX_WORKFLOW_BYTES,
        });
    }
    reject_yaml_indirection(input)?;
    let documents = YamlOwned::load_from_str(input)
        .map_err(|error| GithubActionsError::Yaml(error.to_string()))?;
    if documents.len() != 1 {
        return Err(GithubActionsError::DocumentCount(documents.len()));
    }
    let root = documents[0].as_mapping().ok_or_else(|| {
        GithubActionsError::Structure("workflow root must be a mapping".to_owned())
    })?;
    reject_unsafe_yaml(&documents[0], "$")?;

    let mut analyzer = Analyzer::default();
    analyzer.workflow_name = optional_string(root, "name", "name", &mut analyzer.findings);
    analyzer.inspect_top_level(root)?;
    analyzer.finish()
}

fn reject_yaml_indirection(input: &str) -> Result<(), GithubActionsError> {
    enum Context {
        Mapping {
            expecting_key: bool,
            keys: BTreeSet<String>,
        },
        Sequence,
    }

    fn complete_parent_value(stack: &mut [Context]) {
        if let Some(Context::Mapping { expecting_key, .. }) = stack.last_mut() {
            *expecting_key = true;
        }
    }

    let mut stack = Vec::new();
    for parsed in Parser::new_from_str(input).keep_tags(true) {
        let (event, _) = parsed.map_err(|error| GithubActionsError::Yaml(error.to_string()))?;
        match event {
            Event::Alias(_) => {
                return Err(GithubActionsError::Structure(
                    "workflow contains a YAML alias; anchors, aliases, and tags are unsupported"
                        .to_owned(),
                ));
            }
            Event::Scalar(value, _, anchor, tag) => {
                reject_anchor_or_tag(anchor, tag.is_some())?;
                if let Some(Context::Mapping {
                    expecting_key,
                    keys,
                }) = stack.last_mut()
                {
                    if *expecting_key {
                        if !keys.insert(value.into_owned()) {
                            return Err(GithubActionsError::Structure(
                                "workflow contains a duplicate mapping key".to_owned(),
                            ));
                        }
                        *expecting_key = false;
                    } else {
                        *expecting_key = true;
                    }
                }
            }
            Event::MappingStart(anchor, tag) => {
                reject_anchor_or_tag(anchor, tag.is_some())?;
                if matches!(
                    stack.last(),
                    Some(Context::Mapping {
                        expecting_key: true,
                        ..
                    })
                ) {
                    return Err(GithubActionsError::Structure(
                        "complex YAML mapping keys are unsupported".to_owned(),
                    ));
                }
                stack.push(Context::Mapping {
                    expecting_key: true,
                    keys: BTreeSet::new(),
                });
            }
            Event::SequenceStart(anchor, tag) => {
                reject_anchor_or_tag(anchor, tag.is_some())?;
                if matches!(
                    stack.last(),
                    Some(Context::Mapping {
                        expecting_key: true,
                        ..
                    })
                ) {
                    return Err(GithubActionsError::Structure(
                        "complex YAML sequence keys are unsupported".to_owned(),
                    ));
                }
                stack.push(Context::Sequence);
            }
            Event::MappingEnd | Event::SequenceEnd => {
                stack.pop();
                complete_parent_value(&mut stack);
            }
            Event::Nothing
            | Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart(_)
            | Event::DocumentEnd => {}
        }
    }
    Ok(())
}

fn reject_anchor_or_tag(anchor: usize, has_tag: bool) -> Result<(), GithubActionsError> {
    if anchor != 0 || has_tag {
        Err(GithubActionsError::Structure(
            "workflow contains a YAML anchor or tag; anchors, aliases, and tags are unsupported"
                .to_owned(),
        ))
    } else {
        Ok(())
    }
}

#[derive(Default)]
struct Analyzer {
    workflow_name: Option<String>,
    findings: Vec<CompatibilityFindingV1>,
    proposed_checks: Vec<ProposedCheckV1>,
    environment_names: BTreeSet<String>,
}

impl Analyzer {
    fn inspect_top_level(&mut self, root: &MappingOwned) -> Result<(), GithubActionsError> {
        self.report_unknown_keys(
            root,
            "",
            &["name", "on", "env", "defaults", "permissions", "jobs"],
        );
        if mapping_get(root, "on").is_some() {
            self.record(
                "on",
                "trigger",
                FeatureDisposition::ManualReview,
                "Trigger semantics are reported but are not reproduced locally.",
            );
        }
        if mapping_get(root, "permissions").is_some() {
            self.record(
                "permissions",
                "permissions",
                FeatureDisposition::Unsupported,
                "Workflow permission semantics are not imported or approximated.",
            );
        }
        if let Some(env) = mapping_get(root, "env") {
            self.inspect_environment(env, "env")?;
        }
        if mapping_get(root, "defaults").is_some() {
            self.record(
                "defaults",
                "workflow_defaults",
                FeatureDisposition::ManualReview,
                "Workflow-level defaults require explicit review per job.",
            );
        }
        let jobs = required_mapping(root, "jobs", "jobs")?;
        if jobs.is_empty() || jobs.len() > MAX_JOBS {
            return Err(GithubActionsError::Structure(format!(
                "jobs must contain 1 to {MAX_JOBS} entries"
            )));
        }
        for (job_key, job) in jobs {
            let job_id = string_key(job_key, "jobs")?;
            self.inspect_job(job_id, job)?;
        }
        Ok(())
    }

    fn inspect_job(&mut self, job_id: &str, node: &YamlOwned) -> Result<(), GithubActionsError> {
        let path = format!("jobs.{job_id}");
        let job = node
            .as_mapping()
            .ok_or_else(|| GithubActionsError::Structure(format!("{path} must be a mapping")))?;
        self.report_unknown_keys(
            job,
            &path,
            &[
                "name",
                "runs-on",
                "steps",
                "env",
                "defaults",
                "strategy",
                "services",
                "needs",
                "if",
                "permissions",
                "timeout-minutes",
                "continue-on-error",
                "container",
                "uses",
                "with",
                "secrets",
                "outputs",
                "concurrency",
                "environment",
            ],
        );
        if mapping_get(job, "uses").is_some() {
            self.record(
                format!("{path}.uses"),
                "reusable_workflow",
                FeatureDisposition::Unsupported,
                "Reusable workflows are not loaded or executed.",
            );
            if mapping_get(job, "with").is_some() {
                self.record(
                    format!("{path}.with"),
                    "reusable_workflow_inputs",
                    FeatureDisposition::Unsupported,
                    "Reusable workflow inputs are not imported or evaluated.",
                );
            }
            if mapping_get(job, "secrets").is_some() {
                self.record(
                    format!("{path}.secrets"),
                    "secrets",
                    FeatureDisposition::Unsupported,
                    "Secret values and secret inheritance are never imported.",
                );
            }
            return Ok(());
        }
        self.inspect_runner(job, &path);
        for key in [
            "permissions",
            "if",
            "continue-on-error",
            "outputs",
            "concurrency",
            "environment",
        ] {
            if mapping_get(job, key).is_some() {
                self.record(
                    format!("{path}.{key}"),
                    key,
                    FeatureDisposition::Unsupported,
                    "This job behavior is outside compatibility contract v1.",
                );
            }
        }
        for key in [
            "strategy",
            "services",
            "needs",
            "container",
            "timeout-minutes",
        ] {
            if let Some(value) = mapping_get(job, key) {
                self.record(
                    format!("{path}.{key}"),
                    key,
                    FeatureDisposition::ManualReview,
                    "The feature is reported but not translated into executable configuration.",
                );
                if node_contains_expression(value) {
                    self.record(
                        format!("{path}.{key}"),
                        "expression",
                        FeatureDisposition::Unsupported,
                        "Nested GitHub expressions are not evaluated or interpolated.",
                    );
                }
            }
        }
        if mapping_get(job, "secrets").is_some() {
            self.record(
                format!("{path}.secrets"),
                "secrets",
                FeatureDisposition::Unsupported,
                "Secret values and secret inheritance are never imported.",
            );
        }
        if let Some(env) = mapping_get(job, "env") {
            self.inspect_environment(env, &format!("{path}.env"))?;
        }
        let defaults = run_defaults(mapping_get(job, "defaults"), &path, &mut self.findings)?;
        let steps = mapping_get(job, "steps")
            .and_then(YamlOwned::as_vec)
            .ok_or_else(|| {
                GithubActionsError::Structure(format!("{path}.steps must be a sequence"))
            })?;
        if steps.is_empty() || steps.len() > MAX_STEPS_PER_JOB {
            return Err(GithubActionsError::Structure(format!(
                "{path}.steps must contain 1 to {MAX_STEPS_PER_JOB} entries"
            )));
        }
        for (index, step) in steps.iter().enumerate() {
            self.inspect_step(job_id, index, step, &defaults)?;
        }
        Ok(())
    }

    fn inspect_runner(&mut self, job: &MappingOwned, path: &str) {
        let runner_path = format!("{path}.runs-on");
        match mapping_get(job, "runs-on").and_then(YamlOwned::as_str) {
            Some(label) if contains_expression(label) => self.record(
                runner_path,
                "runner_expression",
                FeatureDisposition::Unsupported,
                "Runner expressions are not evaluated.",
            ),
            Some(label) if label.starts_with("ubuntu-") => self.record(
                runner_path,
                "runner_label",
                FeatureDisposition::ManualReview,
                "The Linux runner label must be mapped to an operator-selected pinned image.",
            ),
            Some(_) => self.record(
                runner_path,
                "runner_label",
                FeatureDisposition::Unsupported,
                "Compatibility contract v1 does not translate this runner platform.",
            ),
            None => self.record(
                runner_path,
                "runner_label",
                FeatureDisposition::Unsupported,
                "A literal runs-on string is required.",
            ),
        }
    }

    fn inspect_step(
        &mut self,
        job_id: &str,
        index: usize,
        step: &YamlOwned,
        defaults: &RunDefaults,
    ) -> Result<(), GithubActionsError> {
        let path = format!("jobs.{job_id}.steps[{index}]");
        let step = step
            .as_mapping()
            .ok_or_else(|| GithubActionsError::Structure(format!("{path} must be a mapping")))?;
        self.report_unknown_keys(
            step,
            &path,
            &[
                "name",
                "id",
                "uses",
                "run",
                "shell",
                "working-directory",
                "env",
                "with",
                "if",
                "continue-on-error",
                "timeout-minutes",
            ],
        );
        let uses = mapping_get(step, "uses");
        let run = mapping_get(step, "run");
        if uses.is_some() && run.is_some() {
            self.record(
                path,
                "step_shape",
                FeatureDisposition::Unsupported,
                "A step cannot contain both uses and run.",
            );
            return Ok(());
        }
        if let Some(action) = uses {
            self.inspect_action_step(&path, action, mapping_get(step, "with"));
            return Ok(());
        }
        if let Some(command) = run {
            self.inspect_run_step(job_id, index, &path, command, step, defaults)?;
            return Ok(());
        }
        self.record(
            path,
            "step_shape",
            FeatureDisposition::Unsupported,
            "A step must contain either uses or run.",
        );
        Ok(())
    }

    fn inspect_action_step(&mut self, path: &str, action: &YamlOwned, with: Option<&YamlOwned>) {
        let action_path = format!("{path}.uses");
        let Some(reference) = action.as_str() else {
            self.record(
                action_path,
                "action_reference",
                FeatureDisposition::Unsupported,
                "Action references must be literal strings.",
            );
            return;
        };
        if contains_expression(reference) {
            self.record(
                action_path,
                "action_expression",
                FeatureDisposition::Unsupported,
                "Action expressions are not evaluated.",
            );
        } else if let Some(revision) = reference.strip_prefix("actions/checkout@") {
            if is_full_commit_sha(revision) {
                self.record(
                    action_path,
                    "checkout",
                    FeatureDisposition::Translated,
                    "Pinned checkout is recognized as source metadata; the action is not executed.",
                );
            } else {
                self.record(
                    action_path,
                    "checkout",
                    FeatureDisposition::ManualReview,
                    "Checkout must be pinned to a full lowercase commit SHA before translation.",
                );
            }
        } else if is_setup_action(reference) {
            self.record(
                action_path,
                "setup_metadata",
                FeatureDisposition::ManualReview,
                "Setup metadata is reported; the action is never downloaded or executed.",
            );
        } else {
            self.record(
                action_path,
                "marketplace_or_local_action",
                FeatureDisposition::Unsupported,
                "Marketplace, Docker, and local actions are never downloaded or executed.",
            );
        }
        if let Some(with) = with {
            self.record(
                format!("{path}.with"),
                "action_inputs",
                FeatureDisposition::ManualReview,
                "Action inputs are reported as present but their values are not imported.",
            );
            if node_contains_expression(with) {
                self.record(
                    format!("{path}.with"),
                    "action_input_expression",
                    FeatureDisposition::Unsupported,
                    "Action input expressions and secret references are not evaluated.",
                );
            }
        }
    }

    fn inspect_run_step(
        &mut self,
        job_id: &str,
        index: usize,
        path: &str,
        command: &YamlOwned,
        step: &MappingOwned,
        defaults: &RunDefaults,
    ) -> Result<(), GithubActionsError> {
        let Some(command) = command.as_str() else {
            self.record(
                format!("{path}.run"),
                "run_command",
                FeatureDisposition::Unsupported,
                "Run commands must be literal strings.",
            );
            return Ok(());
        };
        let mut supported = true;
        if contains_expression(command) {
            supported = false;
            self.record(
                format!("{path}.run"),
                "expression",
                FeatureDisposition::Unsupported,
                "GitHub expressions are not evaluated or interpolated.",
            );
        }
        for key in ["if", "continue-on-error", "timeout-minutes"] {
            if mapping_get(step, key).is_some() {
                supported = false;
                self.record(
                    format!("{path}.{key}"),
                    key,
                    FeatureDisposition::Unsupported,
                    "This step behavior is outside compatibility contract v1.",
                );
            }
        }
        let shell = mapping_get(step, "shell")
            .and_then(YamlOwned::as_str)
            .or(defaults.shell.as_deref());
        let Some(shell) = shell else {
            self.record(
                format!("{path}.shell"),
                "implicit_shell",
                FeatureDisposition::ManualReview,
                "An explicit sh or bash shell is required for a proposed check.",
            );
            return Ok(());
        };
        if !matches!(shell, "sh" | "bash") {
            supported = false;
            self.record(
                format!("{path}.shell"),
                "shell",
                FeatureDisposition::Unsupported,
                "Only the literal sh and bash shells are recognized in contract v1.",
            );
        }
        let working_directory = mapping_get(step, "working-directory")
            .and_then(YamlOwned::as_str)
            .or(defaults.working_directory.as_deref())
            .unwrap_or(".");
        if contains_expression(working_directory) || !is_safe_relative_path(working_directory) {
            supported = false;
            self.record(
                format!("{path}.working-directory"),
                "working_directory",
                FeatureDisposition::Unsupported,
                "Working directory must be a literal normalized repository-relative path.",
            );
        }
        if let Some(env) = mapping_get(step, "env") {
            self.inspect_environment(env, &format!("{path}.env"))?;
        }
        if supported {
            self.proposed_checks.push(ProposedCheckV1 {
                id: format!("{job_id}-step-{:03}", index + 1),
                source_path: path.to_owned(),
                shell: shell.to_owned(),
                command: command.to_owned(),
                working_directory: working_directory.to_owned(),
            });
            self.record(
                format!("{path}.run"),
                "plain_run",
                FeatureDisposition::Translated,
                "A non-executable check proposal was created; no command was run.",
            );
        }
        Ok(())
    }

    fn inspect_environment(
        &mut self,
        node: &YamlOwned,
        path: &str,
    ) -> Result<(), GithubActionsError> {
        let mapping = node
            .as_mapping()
            .ok_or_else(|| GithubActionsError::Structure(format!("{path} must be a mapping")))?;
        for (key, value) in mapping {
            let name = string_key(key, path)?;
            if !is_environment_name(name) {
                self.record(
                    format!("{path}.{name}"),
                    "environment_name",
                    FeatureDisposition::Unsupported,
                    "Environment names must use portable NAME syntax.",
                );
                continue;
            }
            self.environment_names.insert(name.to_owned());
            self.record(
                format!("{path}.{name}"),
                "environment_name",
                FeatureDisposition::Translated,
                "Only the variable name is imported; its value is never serialized.",
            );
            if value.as_str().is_some_and(contains_expression) {
                self.record(
                    format!("{path}.{name}"),
                    "environment_expression",
                    FeatureDisposition::Unsupported,
                    "Environment expressions and secret references are not evaluated.",
                );
            } else {
                self.record(
                    format!("{path}.{name}"),
                    "environment_value",
                    FeatureDisposition::ManualReview,
                    "The workflow value is intentionally omitted; the operator must supply it explicitly.",
                );
            }
        }
        Ok(())
    }

    fn report_unknown_keys(&mut self, mapping: &MappingOwned, path: &str, known: &[&str]) {
        for key in mapping.keys() {
            match key.as_str() {
                Some(name) if known.contains(&name) => {}
                Some(name) => self.record(
                    dotted(path, name),
                    "unknown_key",
                    FeatureDisposition::Unsupported,
                    "Unknown workflow keys are fail-closed.",
                ),
                None => self.record(
                    dotted(path, "<non-string-key>"),
                    "non_string_key",
                    FeatureDisposition::Unsupported,
                    "Workflow mapping keys must be strings.",
                ),
            }
        }
    }

    fn record(
        &mut self,
        path: impl Into<String>,
        feature: impl Into<String>,
        disposition: FeatureDisposition,
        detail: impl Into<String>,
    ) {
        self.findings.push(CompatibilityFindingV1 {
            path: path.into(),
            feature: feature.into(),
            disposition,
            detail: detail.into(),
        });
    }

    fn finish(mut self) -> Result<GithubActionsCompatibilityReportV1, GithubActionsError> {
        self.findings.sort_by(|left, right| {
            (&left.path, &left.feature, left.disposition).cmp(&(
                &right.path,
                &right.feature,
                right.disposition,
            ))
        });
        self.proposed_checks
            .sort_by(|left, right| left.id.cmp(&right.id));
        let summary = CompatibilitySummaryV1 {
            translated: self
                .findings
                .iter()
                .filter(|finding| finding.disposition == FeatureDisposition::Translated)
                .count(),
            manual_review: self
                .findings
                .iter()
                .filter(|finding| finding.disposition == FeatureDisposition::ManualReview)
                .count(),
            unsupported: self
                .findings
                .iter()
                .filter(|finding| finding.disposition == FeatureDisposition::Unsupported)
                .count(),
        };
        let readiness = if summary.unsupported > 0 {
            MigrationReadiness::Blocked
        } else if summary.manual_review > 0 {
            MigrationReadiness::ManualReviewRequired
        } else {
            MigrationReadiness::ReadyForConfigAuthoring
        };
        Ok(GithubActionsCompatibilityReportV1 {
            schema_version: COMPATIBILITY_SCHEMA_VERSION.to_owned(),
            workflow_name: self.workflow_name,
            readiness,
            summary,
            environment_names: self.environment_names.into_iter().collect(),
            proposed_checks: self.proposed_checks,
            findings: self.findings,
            executable_config_emitted: false,
        })
    }
}

#[derive(Default)]
struct RunDefaults {
    shell: Option<String>,
    working_directory: Option<String>,
}

fn run_defaults(
    node: Option<&YamlOwned>,
    job_path: &str,
    findings: &mut Vec<CompatibilityFindingV1>,
) -> Result<RunDefaults, GithubActionsError> {
    let Some(node) = node else {
        return Ok(RunDefaults::default());
    };
    let defaults = node.as_mapping().ok_or_else(|| {
        GithubActionsError::Structure(format!("{job_path}.defaults must be a mapping"))
    })?;
    let Some(run) = mapping_get(defaults, "run") else {
        return Ok(RunDefaults::default());
    };
    let run = run.as_mapping().ok_or_else(|| {
        GithubActionsError::Structure(format!("{job_path}.defaults.run must be a mapping"))
    })?;
    for key in run.keys() {
        if key
            .as_str()
            .is_none_or(|name| !matches!(name, "shell" | "working-directory"))
        {
            findings.push(CompatibilityFindingV1 {
                path: format!("{job_path}.defaults.run.<unknown>"),
                feature: "unknown_key".to_owned(),
                disposition: FeatureDisposition::Unsupported,
                detail: "Unknown run defaults are fail-closed.".to_owned(),
            });
        }
    }
    Ok(RunDefaults {
        shell: mapping_get(run, "shell")
            .and_then(YamlOwned::as_str)
            .map(str::to_owned),
        working_directory: mapping_get(run, "working-directory")
            .and_then(YamlOwned::as_str)
            .map(str::to_owned),
    })
}

fn optional_string(
    mapping: &MappingOwned,
    key: &str,
    path: &str,
    findings: &mut Vec<CompatibilityFindingV1>,
) -> Option<String> {
    let value = mapping_get(mapping, key)?;
    match value.as_str() {
        Some(value) => Some(value.to_owned()),
        None => {
            findings.push(CompatibilityFindingV1 {
                path: path.to_owned(),
                feature: "scalar".to_owned(),
                disposition: FeatureDisposition::Unsupported,
                detail: "This field must be a literal string.".to_owned(),
            });
            None
        }
    }
}

fn required_mapping<'a>(
    mapping: &'a MappingOwned,
    key: &str,
    path: &str,
) -> Result<&'a MappingOwned, GithubActionsError> {
    mapping_get(mapping, key)
        .and_then(YamlOwned::as_mapping)
        .ok_or_else(|| GithubActionsError::Structure(format!("{path} must be a mapping")))
}

fn string_key<'a>(key: &'a YamlOwned, path: &str) -> Result<&'a str, GithubActionsError> {
    key.as_str().ok_or_else(|| {
        GithubActionsError::Structure(format!("{path} contains a non-string mapping key"))
    })
}

fn mapping_get<'a>(mapping: &'a MappingOwned, key: &str) -> Option<&'a YamlOwned> {
    mapping
        .iter()
        .find_map(|(candidate, value)| (candidate.as_str() == Some(key)).then_some(value))
}

fn reject_unsafe_yaml(node: &YamlOwned, path: &str) -> Result<(), GithubActionsError> {
    match node {
        YamlOwned::Alias(_) => Err(GithubActionsError::Structure(format!(
            "{path} contains an alias; aliases are unsupported"
        ))),
        YamlOwned::Tagged(_, _) => Err(GithubActionsError::Structure(format!(
            "{path} contains a tag; tags are unsupported"
        ))),
        YamlOwned::BadValue | YamlOwned::Representation(_, _, _) => Err(
            GithubActionsError::Structure(format!("{path} contains an unresolved scalar")),
        ),
        YamlOwned::Sequence(sequence) => {
            for (index, child) in sequence.iter().enumerate() {
                reject_unsafe_yaml(child, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        YamlOwned::Mapping(mapping) => {
            for (key, value) in mapping {
                reject_unsafe_yaml(key, path)?;
                reject_unsafe_yaml(value, path)?;
            }
            Ok(())
        }
        YamlOwned::Value(_) => Ok(()),
    }
}

fn contains_expression(value: &str) -> bool {
    value.contains("${{")
}

fn node_contains_expression(node: &YamlOwned) -> bool {
    match node {
        YamlOwned::Value(_) => node.as_str().is_some_and(contains_expression),
        YamlOwned::Sequence(sequence) => sequence.iter().any(node_contains_expression),
        YamlOwned::Mapping(mapping) => mapping
            .iter()
            .any(|(key, value)| node_contains_expression(key) || node_contains_expression(value)),
        YamlOwned::Representation(value, _, _) => contains_expression(value),
        YamlOwned::Tagged(_, node) => node_contains_expression(node),
        YamlOwned::Alias(_) | YamlOwned::BadValue => false,
    }
}

fn is_full_commit_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_setup_action(value: &str) -> bool {
    [
        "actions/setup-node@",
        "actions/setup-python@",
        "actions/setup-java@",
        "actions/setup-go@",
        "dotnet/setup-dotnet@",
        "dtolnay/rust-toolchain@",
    ]
    .iter()
    .any(|prefix| value.starts_with(prefix))
}

fn is_environment_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_safe_relative_path(value: &str) -> bool {
    if value == "." {
        return true;
    }
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('~')
        && !value.contains('\\')
        && !value.contains(":/")
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

fn dotted(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}.{name}")
    }
}

#[derive(Debug)]
pub enum GithubActionsError {
    Io {
        path: String,
        source: std::io::Error,
    },
    WorkflowTooLarge {
        actual: usize,
        maximum: usize,
    },
    Yaml(String),
    DocumentCount(usize),
    Structure(String),
    Serialize(serde_json::Error),
}

impl fmt::Display for GithubActionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, .. } => write!(formatter, "cannot read workflow {path}"),
            Self::WorkflowTooLarge { actual, maximum } => write!(
                formatter,
                "workflow is {actual} bytes; maximum is {maximum} bytes"
            ),
            Self::Yaml(error) => write!(formatter, "invalid workflow YAML: {error}"),
            Self::DocumentCount(count) => {
                write!(
                    formatter,
                    "workflow must contain one YAML document, found {count}"
                )
            }
            Self::Structure(message) => write!(formatter, "invalid workflow structure: {message}"),
            Self::Serialize(_) => formatter.write_str("cannot serialize compatibility report"),
        }
    }
}

impl std::error::Error for GithubActionsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Serialize(source) => Some(source),
            Self::WorkflowTooLarge { .. }
            | Self::Yaml(_)
            | Self::DocumentCount(_)
            | Self::Structure(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FeatureDisposition, MigrationReadiness, analyze_workflow};

    #[test]
    fn rejects_multiple_documents_and_non_mapping_roots() {
        assert!(analyze_workflow("---\nname: one\n---\nname: two\n").is_err());
        assert!(analyze_workflow("- one\n- two\n").is_err());
    }

    #[test]
    fn expressions_and_marketplace_actions_fail_closed() {
        let report = analyze_workflow(
            "name: Unsafe\non: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: vendor/action@v1\n      - run: echo '${{ secrets.TOKEN }}'\n        shell: bash\n",
        )
        .expect("compatibility report");
        assert_eq!(report.readiness, MigrationReadiness::Blocked);
        assert!(report.findings.iter().any(|finding| {
            finding.disposition == FeatureDisposition::Unsupported
                && finding.feature == "marketplace_or_local_action"
        }));
        assert!(report.proposed_checks.is_empty());
    }

    #[test]
    fn aliases_and_tags_are_rejected_before_analysis() {
        let alias = "name: Alias\non: push\nenv: &shared\n  SAFE: yes\njobs:\n  test:\n    runs-on: ubuntu-latest\n    env: *shared\n    steps:\n      - run: true\n        shell: sh\n";
        assert!(analyze_workflow(alias).is_err());
        let tagged = "name: Tagged\non: push\njobs: !custom {}\n";
        assert!(analyze_workflow(tagged).is_err());
    }

    #[test]
    fn duplicate_mapping_keys_are_rejected_before_last_value_wins() {
        let duplicate = "name: First\nname: Second\non: push\njobs: {}\n";
        assert!(analyze_workflow(duplicate).is_err());
    }
}
