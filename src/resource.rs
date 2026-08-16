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

use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Serialize;

use crate::process::{
    CancellationReason, CancellationToken, CleanupStatus, GenerationGuard, ProcessError,
    ProcessRequest, ProcessTermination, RunIdentity, SupervisorPort,
};

pub const RESOURCE_SCHEMA_VERSION: &str = "1.0";
pub const MACOS_POLICY_VERSION: &str = "macos-v4";
pub const WATCHDOG_SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
pub const RESOURCE_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
pub const RESOURCE_CAPTURE_BYTES: usize = 65_536;
pub const MIN_PRESTART_AVAILABLE_PERCENT: u8 = 20;
pub const MIN_PRESTART_FREE_BYTES: u64 = 3 * 1024 * 1024 * 1024;
pub const MAX_PRESTART_SWAP_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const HARD_AVAILABLE_PERCENT: u8 = 3;
pub const HARD_RECLAIMABLE_BYTES: u64 = 512 * 1024 * 1024;
pub const HARD_SWAP_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub const HARD_COMPRESSOR_PERCENT: u8 = 70;
pub const HARD_COMPRESSOR_COMPANION_AVAILABLE_PERCENT: u8 = 8;
pub const SOFT_AVAILABLE_PERCENT: u8 = 10;
pub const SOFT_RECLAIMABLE_BYTES: u64 = 1536 * 1024 * 1024;
pub const SOFT_COMPRESSOR_PERCENT: u8 = 55;
pub const SOFT_SWAP_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const SOFT_SWAP_GROWTH_BYTES: u64 = 1024 * 1024 * 1024;
pub const SOFT_REQUIRED_SIGNALS: u8 = 2;
pub const SOFT_CONSECUTIVE_SAMPLES: u8 = 15;
pub const SOFT_TREND_WINDOW_SAMPLES: usize = 16;

const MEMORY_PRESSURE: &str = "/usr/bin/memory_pressure";
const VM_STAT: &str = "/usr/bin/vm_stat";
const SYSCTL: &str = "/usr/sbin/sysctl";
const MAX_PERCENT: u8 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceCapability {
    SupportedEnforced,
    UnsupportedNotEnforced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceDecision {
    Admit,
    Deny,
    Clear,
    SoftPressure,
    HardPressure,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePlatform {
    MacOs,
    Unsupported,
}

impl ResourcePlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unsupported
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Unsupported => std::env::consts::OS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceStatusV1 {
    pub schema_version: String,
    pub policy_version: String,
    pub platform: String,
    pub capability: ResourceCapability,
    pub decision: ResourceDecision,
    pub available_percent: Option<u8>,
    pub reclaimable_uncompressed_bytes: Option<u64>,
    pub compressor_occupied_bytes: Option<u64>,
    pub total_memory_bytes: Option<u64>,
    pub swap_used_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub consecutive_soft_samples: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub available_percent: u8,
    pub reclaimable_uncompressed_bytes: u64,
    pub compressor_occupied_bytes: u64,
    pub total_memory_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceObservationSummary {
    pub baseline: ResourceSnapshot,
    pub last_snapshot: ResourceSnapshot,
    pub sample_count: u64,
    pub minimum_available_percent: u8,
    pub minimum_reclaimable_uncompressed_bytes: u64,
    pub maximum_compressor_occupied_bytes: u64,
    pub maximum_swap_used_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ResourceObservation {
    summary: Arc<Mutex<ResourceObservationSummary>>,
}

impl ResourceObservation {
    pub fn new(baseline: ResourceSnapshot) -> Self {
        Self {
            summary: Arc::new(Mutex::new(ResourceObservationSummary {
                sample_count: 1,
                minimum_available_percent: baseline.available_percent,
                minimum_reclaimable_uncompressed_bytes: baseline.reclaimable_uncompressed_bytes,
                maximum_compressor_occupied_bytes: baseline.compressor_occupied_bytes,
                maximum_swap_used_bytes: baseline.swap_used_bytes,
                last_snapshot: baseline.clone(),
                baseline,
            })),
        }
    }

    pub fn record(&self, snapshot: &ResourceSnapshot) {
        if let Ok(mut summary) = self.summary.lock() {
            summary.sample_count = summary.sample_count.saturating_add(1);
            summary.minimum_available_percent = summary
                .minimum_available_percent
                .min(snapshot.available_percent);
            summary.minimum_reclaimable_uncompressed_bytes = summary
                .minimum_reclaimable_uncompressed_bytes
                .min(snapshot.reclaimable_uncompressed_bytes);
            summary.maximum_compressor_occupied_bytes = summary
                .maximum_compressor_occupied_bytes
                .max(snapshot.compressor_occupied_bytes);
            summary.maximum_swap_used_bytes = summary
                .maximum_swap_used_bytes
                .max(snapshot.swap_used_bytes);
            summary.last_snapshot = snapshot.clone();
        }
    }

    pub fn summary(&self) -> Option<ResourceObservationSummary> {
        self.summary.lock().ok().map(|summary| summary.clone())
    }
}

impl ResourceSnapshot {
    pub fn validate(&self) -> Result<(), ResourceProbeError> {
        if self.available_percent > MAX_PERCENT
            || self.total_memory_bytes == 0
            || self.reclaimable_uncompressed_bytes > self.total_memory_bytes
            || self.compressor_occupied_bytes > self.total_memory_bytes
            || self.swap_used_bytes > self.swap_total_bytes
        {
            return Err(ResourceProbeError::ContradictorySnapshot);
        }
        Ok(())
    }

    fn compressor_percent_at_least(&self, percent: u8) -> bool {
        ratio_at_least(
            self.compressor_occupied_bytes,
            self.total_memory_bytes,
            percent,
        )
    }

    fn has_hard_compound_compression(&self) -> bool {
        self.compressor_percent_at_least(HARD_COMPRESSOR_PERCENT)
            && (self.available_percent <= HARD_COMPRESSOR_COMPANION_AVAILABLE_PERCENT
                || self.reclaimable_uncompressed_bytes < SOFT_RECLAIMABLE_BYTES
                || self.swap_used_bytes >= SOFT_SWAP_BYTES)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreStartDecision {
    Admit,
    Deny,
}

pub fn evaluate_pre_start(
    snapshot: &ResourceSnapshot,
) -> Result<PreStartDecision, ResourceProbeError> {
    snapshot.validate()?;
    let proportional_swap_limit = snapshot.total_memory_bytes.saturating_mul(30) / 100;
    let swap_limit = MAX_PRESTART_SWAP_BYTES.min(proportional_swap_limit);
    Ok(
        if snapshot.available_percent >= MIN_PRESTART_AVAILABLE_PERCENT
            && snapshot.reclaimable_uncompressed_bytes >= MIN_PRESTART_FREE_BYTES
            && snapshot.swap_used_bytes <= swap_limit
            && !snapshot.has_hard_compound_compression()
        {
            PreStartDecision::Admit
        } else {
            PreStartDecision::Deny
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogTripReason {
    HardPressure,
    SoftPressure,
    ProbeFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogDecision {
    Continue,
    Tripped(WatchdogTripReason),
}

#[derive(Debug, Default)]
pub struct WatchdogState {
    consecutive_soft_samples: u8,
    first_trip: Option<WatchdogTripReason>,
    swap_window: VecDeque<u64>,
}

impl WatchdogState {
    pub fn observe(
        &mut self,
        snapshot: &ResourceSnapshot,
    ) -> Result<WatchdogDecision, ResourceProbeError> {
        snapshot.validate()?;
        if let Some(reason) = self.first_trip {
            return Ok(WatchdogDecision::Tripped(reason));
        }
        self.swap_window.push_back(snapshot.swap_used_bytes);
        if self.swap_window.len() > SOFT_TREND_WINDOW_SAMPLES {
            self.swap_window.pop_front();
        }
        let swap_growth = self.swap_window.len() == SOFT_TREND_WINDOW_SAMPLES
            && snapshot.swap_used_bytes.saturating_sub(
                *self
                    .swap_window
                    .front()
                    .expect("bounded trend window is non-empty"),
            ) >= SOFT_SWAP_GROWTH_BYTES;
        if snapshot.available_percent <= HARD_AVAILABLE_PERCENT
            || snapshot.reclaimable_uncompressed_bytes < HARD_RECLAIMABLE_BYTES
            || snapshot.swap_used_bytes >= HARD_SWAP_BYTES
            || snapshot.has_hard_compound_compression()
        {
            self.first_trip = Some(WatchdogTripReason::HardPressure);
            return Ok(WatchdogDecision::Tripped(WatchdogTripReason::HardPressure));
        }
        let soft_signals = u8::from(snapshot.available_percent < SOFT_AVAILABLE_PERCENT)
            + u8::from(snapshot.reclaimable_uncompressed_bytes < SOFT_RECLAIMABLE_BYTES)
            + u8::from(snapshot.compressor_percent_at_least(SOFT_COMPRESSOR_PERCENT))
            + u8::from(snapshot.swap_used_bytes >= SOFT_SWAP_BYTES)
            + u8::from(swap_growth);
        if soft_signals >= SOFT_REQUIRED_SIGNALS {
            self.consecutive_soft_samples = self.consecutive_soft_samples.saturating_add(1);
            if self.consecutive_soft_samples >= SOFT_CONSECUTIVE_SAMPLES {
                self.first_trip = Some(WatchdogTripReason::SoftPressure);
                return Ok(WatchdogDecision::Tripped(WatchdogTripReason::SoftPressure));
            }
        } else {
            self.consecutive_soft_samples = 0;
        }
        Ok(WatchdogDecision::Continue)
    }

    pub fn consecutive_soft_samples(&self) -> u8 {
        self.consecutive_soft_samples
    }

    pub fn first_trip(&self) -> Option<WatchdogTripReason> {
        self.first_trip
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCommand {
    MemoryPressure,
    VmStat,
    MemSize,
    SwapUsage,
}

impl ResourceCommand {
    fn program(self) -> &'static str {
        match self {
            Self::MemoryPressure => MEMORY_PRESSURE,
            Self::VmStat => VM_STAT,
            Self::MemSize | Self::SwapUsage => SYSCTL,
        }
    }

    fn argv(self) -> &'static [&'static str] {
        match self {
            Self::MemoryPressure => &["-Q"],
            Self::VmStat => &[],
            Self::MemSize => &["-n", "hw.memsize"],
            Self::SwapUsage => &["vm.swapusage"],
        }
    }
}

pub trait ResourceCommandRunner: Send + Sync {
    fn run(&self, command: ResourceCommand) -> Result<Vec<u8>, ResourceProbeError>;
}

pub struct ResourceProbe<R> {
    runner: R,
}

impl<R: ResourceCommandRunner> ResourceProbe<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    pub fn sample(&self) -> Result<ResourceSnapshot, ResourceProbeError> {
        let memory_pressure = self.runner.run(ResourceCommand::MemoryPressure)?;
        let vm_stat = self.runner.run(ResourceCommand::VmStat)?;
        let memsize = self.runner.run(ResourceCommand::MemSize)?;
        let swap_usage = self.runner.run(ResourceCommand::SwapUsage)?;
        let available_percent = parse_memory_pressure(&memory_pressure)?;
        let (page_size, free_pages, speculative_pages, inactive_pages, compressor_pages) =
            parse_vm_stat(&vm_stat)?;
        let total_memory_bytes = parse_memsize(&memsize)?;
        let (swap_total_bytes, swap_used_bytes) = parse_swap_usage(&swap_usage)?;
        let reclaimable_pages = free_pages
            .checked_add(speculative_pages)
            .and_then(|pages| pages.checked_add(inactive_pages))
            .ok_or(ResourceProbeError::Overflow)?;
        let reclaimable_uncompressed_bytes = pages_to_bytes(reclaimable_pages, page_size)?;
        let compressor_occupied_bytes = pages_to_bytes(compressor_pages, page_size)?;
        let snapshot = ResourceSnapshot {
            available_percent,
            reclaimable_uncompressed_bytes,
            compressor_occupied_bytes,
            total_memory_bytes,
            swap_used_bytes,
            swap_total_bytes,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

fn pages_to_bytes(pages: u64, page_size: u64) -> Result<u64, ResourceProbeError> {
    pages
        .checked_mul(page_size)
        .ok_or(ResourceProbeError::Overflow)
}

pub struct SupervisorResourceRunner {
    supervisor: Arc<dyn SupervisorPort>,
    current_dir: PathBuf,
    cancellation: CancellationToken,
}

impl SupervisorResourceRunner {
    pub fn new(
        supervisor: Arc<dyn SupervisorPort>,
        current_dir: PathBuf,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            supervisor,
            current_dir,
            cancellation,
        }
    }
}

impl ResourceCommandRunner for SupervisorResourceRunner {
    fn run(&self, command: ResourceCommand) -> Result<Vec<u8>, ResourceProbeError> {
        let identity = RunIdentity {
            project: "commit-ci-preflight.resource-guard".to_owned(),
            commit: None,
            config_digest: "resource-guard-v1".to_owned(),
            generation: "resource-guard-v1".to_owned(),
        };
        let generation = GenerationGuard::new(identity.clone());
        let request = ProcessRequest {
            identity,
            program: command.program().into(),
            argv: command.argv().iter().map(|value| (*value).into()).collect(),
            current_dir: self.current_dir.clone(),
            environment: Default::default(),
            timeout: RESOURCE_COMMAND_TIMEOUT,
            max_capture_bytes: RESOURCE_CAPTURE_BYTES,
        };
        let result = self
            .supervisor
            .execute(&request, &self.cancellation, &generation)
            .map_err(ResourceProbeError::Process)?;
        if result.termination == ProcessTermination::Cancelled
            && self.cancellation.reason() == Some(CancellationReason::User)
        {
            return Err(ResourceProbeError::Cancelled);
        }
        if result.termination != ProcessTermination::Completed {
            return Err(ResourceProbeError::CommandFailed);
        }
        if result.cleanup != CleanupStatus::Verified
            || result.exit.map(|exit| exit.success) != Some(true)
            || result.stdout.truncated
            || result.stderr.truncated
        {
            return Err(ResourceProbeError::CommandFailed);
        }
        Ok(result.stdout.bytes)
    }
}

pub struct ResourceWatchdog {
    stop: Arc<AtomicBool>,
    trip: Arc<Mutex<Option<WatchdogTripReason>>>,
    join: Option<JoinHandle<()>>,
}

impl ResourceWatchdog {
    pub fn start<R: ResourceCommandRunner + 'static>(
        probe: ResourceProbe<R>,
        cancellation: CancellationToken,
    ) -> Self {
        Self::start_with_interval(probe, cancellation, WATCHDOG_SAMPLE_INTERVAL)
    }

    pub fn start_observed<R: ResourceCommandRunner + 'static>(
        probe: ResourceProbe<R>,
        cancellation: CancellationToken,
        observation: ResourceObservation,
    ) -> Self {
        Self::start_with_interval_and_observation(
            probe,
            cancellation,
            WATCHDOG_SAMPLE_INTERVAL,
            Some(observation),
        )
    }

    pub fn start_with_interval<R: ResourceCommandRunner + 'static>(
        probe: ResourceProbe<R>,
        cancellation: CancellationToken,
        interval: Duration,
    ) -> Self {
        Self::start_with_interval_and_observation(probe, cancellation, interval, None)
    }

    fn start_with_interval_and_observation<R: ResourceCommandRunner + 'static>(
        probe: ResourceProbe<R>,
        cancellation: CancellationToken,
        interval: Duration,
        observation: Option<ResourceObservation>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let trip = Arc::new(Mutex::new(None));
        let thread_stop = Arc::clone(&stop);
        let thread_trip = Arc::clone(&trip);
        let join = thread::spawn(move || {
            let mut state = WatchdogState::default();
            while !thread_stop.load(Ordering::Acquire) {
                thread::sleep(interval);
                if thread_stop.load(Ordering::Acquire) {
                    break;
                }
                if cancellation.reason() == Some(CancellationReason::User) {
                    break;
                }
                let decision = match probe.sample() {
                    Ok(snapshot) => {
                        if let Some(observation) = &observation {
                            observation.record(&snapshot);
                        }
                        state.observe(&snapshot)
                    }
                    Err(ResourceProbeError::Cancelled) => Ok(WatchdogDecision::Continue),
                    Err(_) => Ok(WatchdogDecision::Tripped(WatchdogTripReason::ProbeFailure)),
                };
                match decision {
                    Ok(WatchdogDecision::Continue) => {}
                    Ok(WatchdogDecision::Tripped(reason)) => {
                        if let Ok(mut current) = thread_trip.lock() {
                            if current.is_none() {
                                *current = Some(reason);
                            }
                        }
                        cancellation.cancel_resource_pressure();
                        break;
                    }
                    Err(_) => unreachable!("watchdog decision only fails closed through probe"),
                }
            }
        });
        Self {
            stop,
            trip,
            join: Some(join),
        }
    }

    pub fn stop_and_join(mut self) -> Result<Option<WatchdogTripReason>, ResourceProbeError> {
        self.stop.store(true, Ordering::Release);
        if self
            .join
            .take()
            .expect("watchdog join handle is present")
            .join()
            .is_err()
        {
            return Err(ResourceProbeError::WatchdogPanicked);
        }
        self.trip
            .lock()
            .map(|trip| *trip)
            .map_err(|_| ResourceProbeError::WatchdogStatePoisoned)
    }
}

pub fn unsupported_status() -> ResourceStatusV1 {
    status_with_capability(ResourceCapability::UnsupportedNotEnforced)
}

pub fn unknown_status() -> ResourceStatusV1 {
    status_with_capability(ResourceCapability::SupportedEnforced)
}

fn status_with_capability(capability: ResourceCapability) -> ResourceStatusV1 {
    ResourceStatusV1 {
        schema_version: RESOURCE_SCHEMA_VERSION.to_owned(),
        policy_version: MACOS_POLICY_VERSION.to_owned(),
        platform: ResourcePlatform::current().as_str().to_owned(),
        capability,
        decision: ResourceDecision::Unknown,
        available_percent: None,
        reclaimable_uncompressed_bytes: None,
        compressor_occupied_bytes: None,
        total_memory_bytes: None,
        swap_used_bytes: None,
        swap_total_bytes: None,
        consecutive_soft_samples: 0,
    }
}

pub fn status_from_snapshot(
    snapshot: &ResourceSnapshot,
) -> Result<ResourceStatusV1, ResourceProbeError> {
    snapshot.validate()?;
    let decision = match evaluate_pre_start(snapshot)? {
        PreStartDecision::Admit => ResourceDecision::Admit,
        PreStartDecision::Deny => ResourceDecision::Deny,
    };
    Ok(ResourceStatusV1 {
        schema_version: RESOURCE_SCHEMA_VERSION.to_owned(),
        policy_version: MACOS_POLICY_VERSION.to_owned(),
        platform: "macos".to_owned(),
        capability: ResourceCapability::SupportedEnforced,
        decision,
        available_percent: Some(snapshot.available_percent),
        reclaimable_uncompressed_bytes: Some(snapshot.reclaimable_uncompressed_bytes),
        compressor_occupied_bytes: Some(snapshot.compressor_occupied_bytes),
        total_memory_bytes: Some(snapshot.total_memory_bytes),
        swap_used_bytes: Some(snapshot.swap_used_bytes),
        swap_total_bytes: Some(snapshot.swap_total_bytes),
        consecutive_soft_samples: 0,
    })
}

fn ratio_at_least(value: u64, total: u64, percent: u8) -> bool {
    u128::from(value) * 100 >= u128::from(total) * u128::from(percent)
}

fn parse_memory_pressure(input: &[u8]) -> Result<u8, ResourceProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ResourceProbeError::MalformedOutput)?;
    let mut found = None;
    for line in text.lines() {
        let Some(value) = line.strip_prefix("System-wide memory free percentage:") else {
            continue;
        };
        if found.is_some() {
            return Err(ResourceProbeError::MalformedOutput);
        }
        let value = value.trim();
        let percent = value
            .strip_suffix('%')
            .ok_or(ResourceProbeError::MalformedOutput)?
            .parse::<u8>()
            .map_err(|_| ResourceProbeError::MalformedOutput)?;
        if percent > MAX_PERCENT {
            return Err(ResourceProbeError::MalformedOutput);
        }
        found = Some(percent);
    }
    found.ok_or(ResourceProbeError::MalformedOutput)
}

fn parse_vm_stat(input: &[u8]) -> Result<(u64, u64, u64, u64, u64), ResourceProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ResourceProbeError::MalformedOutput)?;
    let header = text
        .lines()
        .next()
        .ok_or(ResourceProbeError::MalformedOutput)?;
    let page_size = header
        .strip_prefix("Mach Virtual Memory Statistics: (page size of ")
        .or_else(|| header.strip_prefix("Mach Virtual Memory Statistics: (page size: "))
        .and_then(|value| value.strip_suffix(" bytes)"))
        .ok_or(ResourceProbeError::MalformedOutput)?
        .parse::<u64>()
        .map_err(|_| ResourceProbeError::MalformedOutput)?;
    if page_size == 0 {
        return Err(ResourceProbeError::MalformedOutput);
    }
    let mut free = None;
    let mut speculative = None;
    let mut inactive = None;
    let mut compressor = None;
    for line in text.lines().skip(1) {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let slot = match key {
            "Pages free" => &mut free,
            "Pages speculative" => &mut speculative,
            "Pages inactive" => &mut inactive,
            "Pages occupied by compressor" => &mut compressor,
            _ => continue,
        };
        if slot.is_some() {
            return Err(ResourceProbeError::MalformedOutput);
        }
        *slot = Some(parse_page_count(value.trim())?);
    }
    Ok((
        page_size,
        free.ok_or(ResourceProbeError::MalformedOutput)?,
        speculative.ok_or(ResourceProbeError::MalformedOutput)?,
        inactive.ok_or(ResourceProbeError::MalformedOutput)?,
        compressor.ok_or(ResourceProbeError::MalformedOutput)?,
    ))
}

fn parse_page_count(value: &str) -> Result<u64, ResourceProbeError> {
    let digits = value
        .strip_suffix('.')
        .ok_or(ResourceProbeError::MalformedOutput)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ResourceProbeError::MalformedOutput);
    }
    digits
        .parse()
        .map_err(|_| ResourceProbeError::MalformedOutput)
}

fn parse_memsize(input: &[u8]) -> Result<u64, ResourceProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ResourceProbeError::MalformedOutput)?;
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ResourceProbeError::MalformedOutput);
    }
    let value = text
        .parse()
        .map_err(|_| ResourceProbeError::MalformedOutput)?;
    if value == 0 {
        return Err(ResourceProbeError::MalformedOutput);
    }
    Ok(value)
}

fn parse_swap_usage(input: &[u8]) -> Result<(u64, u64), ResourceProbeError> {
    let text = std::str::from_utf8(input).map_err(|_| ResourceProbeError::MalformedOutput)?;
    let rest = text
        .strip_prefix("vm.swapusage:")
        .filter(|rest| {
            rest.as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_whitespace())
        })
        .ok_or(ResourceProbeError::MalformedOutput)?;
    let tokens: Vec<_> = rest.split_whitespace().collect();
    if tokens.len() != 9 && tokens.len() != 10 {
        return Err(ResourceProbeError::MalformedOutput);
    }
    if tokens[0] != "total"
        || tokens[1] != "="
        || tokens[3] != "used"
        || tokens[4] != "="
        || tokens[6] != "free"
        || tokens[7] != "="
        || (tokens.len() == 10 && tokens[9] != "(encrypted)")
    {
        return Err(ResourceProbeError::MalformedOutput);
    }
    let total = parse_size(tokens[2])?;
    let used = parse_size(tokens[5])?;
    let free = parse_size(tokens[8])?;
    let sum = used.checked_add(free).ok_or(ResourceProbeError::Overflow)?;
    if sum.abs_diff(total) > 1 {
        return Err(ResourceProbeError::ContradictorySnapshot);
    }
    Ok((total, used))
}

fn parse_size(value: &str) -> Result<u64, ResourceProbeError> {
    let (number, unit) = value.split_at(value.len().saturating_sub(1));
    let multiplier = match unit {
        "B" => 1_u128,
        "K" => 1_u128 << 10,
        "M" => 1_u128 << 20,
        "G" => 1_u128 << 30,
        "T" => 1_u128 << 40,
        _ => return Err(ResourceProbeError::MalformedOutput),
    };
    let (whole, fraction) = number
        .split_once('.')
        .ok_or(ResourceProbeError::MalformedOutput)?;
    if whole.is_empty()
        || fraction.is_empty()
        || fraction.len() > 2
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ResourceProbeError::MalformedOutput);
    }
    let scale = 10_u128.pow(fraction.len() as u32);
    let numerator = u128::from(
        whole
            .parse::<u64>()
            .map_err(|_| ResourceProbeError::MalformedOutput)?,
    )
    .checked_mul(scale)
    .and_then(|value| value.checked_add(u128::from(fraction.parse::<u64>().ok()?)))
    .ok_or(ResourceProbeError::Overflow)?;
    let bytes = numerator
        .checked_mul(multiplier)
        .ok_or(ResourceProbeError::Overflow)?
        / scale;
    u64::try_from(bytes).map_err(|_| ResourceProbeError::Overflow)
}
#[derive(Debug)]
pub enum ResourceProbeError {
    MalformedOutput,
    ContradictorySnapshot,
    Overflow,
    Cancelled,
    CommandFailed,
    Process(ProcessError),
    WatchdogPanicked,
    WatchdogStatePoisoned,
}

#[derive(Debug)]
pub enum ResourceGuardError {
    PreStartDenied,
    Probe(ResourceProbeError),
    Watchdog(ResourceProbeError),
    WatchdogTripped(WatchdogTripReason),
}

impl ResourceGuardError {
    pub fn exit_code(&self) -> i32 {
        6
    }
}

impl fmt::Display for ResourceGuardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PreStartDenied => formatter.write_str("host resource admission denied"),
            Self::Probe(error) => write!(formatter, "host resource probe failed: {error}"),
            Self::Watchdog(error) => write!(formatter, "host resource watchdog failed: {error}"),
            Self::WatchdogTripped(reason) => {
                write!(formatter, "host resource watchdog tripped: {reason:?}")
            }
        }
    }
}

impl std::error::Error for ResourceGuardError {}

impl fmt::Display for ResourceProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedOutput => formatter.write_str("resource probe output was malformed"),
            Self::ContradictorySnapshot => {
                formatter.write_str("resource probe values were contradictory")
            }
            Self::Overflow => formatter.write_str("resource probe value overflowed"),
            Self::Cancelled => formatter.write_str("resource probe was cancelled"),
            Self::CommandFailed => {
                formatter.write_str("resource probe command failed or was uncertain")
            }
            Self::Process(error) => write!(formatter, "resource probe process failed: {error}"),
            Self::WatchdogPanicked => formatter.write_str("resource watchdog thread panicked"),
            Self::WatchdogStatePoisoned => {
                formatter.write_str("resource watchdog state was poisoned")
            }
        }
    }
}

impl std::error::Error for ResourceProbeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{self, Sender};
    use std::thread;

    fn snapshot() -> ResourceSnapshot {
        ResourceSnapshot {
            available_percent: 50,
            reclaimable_uncompressed_bytes: 4 * 1024 * 1024 * 1024,
            compressor_occupied_bytes: 10 * 1024 * 1024 * 1024,
            total_memory_bytes: 36 * 1024 * 1024 * 1024,
            swap_used_bytes: 1024 * 1024 * 1024,
            swap_total_bytes: 36 * 1024 * 1024 * 1024,
        }
    }

    fn percent_ceiling(total: u64, percent: u64) -> u64 {
        (total * percent).div_ceil(100)
    }

    #[test]
    fn observation_tracks_only_bounded_extrema() {
        let baseline = snapshot();
        let observation = ResourceObservation::new(baseline.clone());
        let mut first = baseline.clone();
        first.available_percent = 42;
        first.reclaimable_uncompressed_bytes -= 100;
        first.compressor_occupied_bytes += 200;
        first.swap_used_bytes += 300;
        observation.record(&first);
        let mut second = baseline.clone();
        second.available_percent = 47;
        second.reclaimable_uncompressed_bytes -= 50;
        second.compressor_occupied_bytes += 100;
        second.swap_used_bytes += 150;
        observation.record(&second);

        assert_eq!(
            observation.summary().expect("observation summary"),
            ResourceObservationSummary {
                baseline,
                last_snapshot: second,
                sample_count: 3,
                minimum_available_percent: 42,
                minimum_reclaimable_uncompressed_bytes: first.reclaimable_uncompressed_bytes,
                maximum_compressor_occupied_bytes: first.compressor_occupied_bytes,
                maximum_swap_used_bytes: first.swap_used_bytes,
            }
        );
    }

    #[test]
    fn policy_boundaries_are_explicit() {
        assert_eq!(MACOS_POLICY_VERSION, "macos-v4");
        assert_eq!(MIN_PRESTART_AVAILABLE_PERCENT, 20);
        assert_eq!(MAX_PRESTART_SWAP_BYTES, 8 * 1024 * 1024 * 1024);
        assert_eq!(SOFT_CONSECUTIVE_SAMPLES, 15);
        assert_eq!(SOFT_REQUIRED_SIGNALS, 2);
        assert_eq!(SOFT_COMPRESSOR_PERCENT, 55);
        assert_eq!(HARD_COMPRESSOR_PERCENT, 70);
        assert_eq!(HARD_SWAP_BYTES, 8 * 1024 * 1024 * 1024);
        assert_eq!(
            evaluate_pre_start(&snapshot()).expect("admit"),
            PreStartDecision::Admit
        );
        let mut denied = snapshot();
        denied.available_percent = MIN_PRESTART_AVAILABLE_PERCENT;
        denied.reclaimable_uncompressed_bytes = MIN_PRESTART_FREE_BYTES;
        denied.swap_used_bytes = MAX_PRESTART_SWAP_BYTES;
        assert_eq!(
            evaluate_pre_start(&denied).expect("boundary"),
            PreStartDecision::Admit
        );
        denied.available_percent = MIN_PRESTART_AVAILABLE_PERCENT - 1;
        assert_eq!(
            evaluate_pre_start(&denied).expect("deny"),
            PreStartDecision::Deny
        );

        denied = snapshot();
        denied.swap_total_bytes = 12 * 1024 * 1024 * 1024;
        denied.swap_used_bytes = MAX_PRESTART_SWAP_BYTES;
        assert_eq!(
            evaluate_pre_start(&denied).expect("8 GiB swap boundary admit"),
            PreStartDecision::Admit
        );
        denied.swap_used_bytes = MAX_PRESTART_SWAP_BYTES + 1;
        assert_eq!(
            evaluate_pre_start(&denied).expect("above 8 GiB swap deny"),
            PreStartDecision::Deny
        );
        denied = snapshot();
        denied.total_memory_bytes = 16 * 1024 * 1024 * 1024;
        denied.compressor_occupied_bytes = denied.total_memory_bytes / 10;
        denied.swap_total_bytes = denied.total_memory_bytes;
        denied.swap_used_bytes = 30 * denied.total_memory_bytes / 100;
        assert_eq!(
            evaluate_pre_start(&denied).expect("30 percent small host boundary admit"),
            PreStartDecision::Admit
        );
        denied.swap_used_bytes += 1;
        assert_eq!(
            evaluate_pre_start(&denied).expect("above proportional swap deny"),
            PreStartDecision::Deny
        );
        denied = snapshot();
        denied.reclaimable_uncompressed_bytes = MIN_PRESTART_FREE_BYTES - 1;
        assert_eq!(
            evaluate_pre_start(&denied).expect("free deny"),
            PreStartDecision::Deny
        );
    }

    #[test]
    fn pre_start_compression_is_advisory_without_a_companion_signal() {
        let mut compressed = snapshot();
        compressed.compressor_occupied_bytes = percent_ceiling(compressed.total_memory_bytes, 90);
        compressed.swap_used_bytes = 0;
        assert_eq!(
            evaluate_pre_start(&compressed).expect("healthy compressed host"),
            PreStartDecision::Admit
        );

        let observed = ResourceSnapshot {
            available_percent: 40,
            reclaimable_uncompressed_bytes: 7_594_246_144,
            compressor_occupied_bytes: 16_000_000_000,
            total_memory_bytes: 38_654_705_664,
            swap_used_bytes: 0,
            swap_total_bytes: 6_442_450_944,
        };
        assert_eq!(
            evaluate_pre_start(&observed).expect("observed false-positive sample"),
            PreStartDecision::Admit
        );
    }

    #[test]
    fn pre_start_rejects_hard_compound_compression() {
        let mut compound = snapshot();
        compound.compressor_occupied_bytes = percent_ceiling(
            compound.total_memory_bytes,
            u64::from(HARD_COMPRESSOR_PERCENT),
        );
        compound.swap_used_bytes = SOFT_SWAP_BYTES;
        assert_eq!(
            evaluate_pre_start(&compound).expect("compound compressor pressure"),
            PreStartDecision::Deny
        );
    }

    #[test]
    fn compressor_alone_never_cancels_a_healthy_run() {
        let mut state = WatchdogState::default();
        let mut compressed = snapshot();
        compressed.compressor_occupied_bytes = percent_ceiling(compressed.total_memory_bytes, 80);
        for _ in 0..(SOFT_CONSECUTIVE_SAMPLES * 2) {
            assert_eq!(
                state
                    .observe(&compressed)
                    .expect("healthy companion signals"),
                WatchdogDecision::Continue
            );
        }
        assert_eq!(state.consecutive_soft_samples(), 0);
    }

    #[test]
    fn observed_false_positive_fixture_remains_admitted_in_progress() {
        let mut state = WatchdogState::default();
        let observed = ResourceSnapshot {
            available_percent: 42,
            reclaimable_uncompressed_bytes: 8_126_005_248,
            compressor_occupied_bytes: 16_777_625_600,
            total_memory_bytes: 38_654_705_664,
            swap_used_bytes: 0,
            swap_total_bytes: 23_622_320_128,
        };
        for _ in 0..(SOFT_CONSECUTIVE_SAMPLES * 2) {
            assert_eq!(
                state.observe(&observed).expect("measured snapshot"),
                WatchdogDecision::Continue
            );
        }
    }

    #[test]
    fn hard_pressure_requires_a_critical_signal_or_compound_compression() {
        for pressure in [
            |value: &mut ResourceSnapshot| value.available_percent = HARD_AVAILABLE_PERCENT,
            |value: &mut ResourceSnapshot| {
                value.reclaimable_uncompressed_bytes = HARD_RECLAIMABLE_BYTES - 1
            },
            |value: &mut ResourceSnapshot| value.swap_used_bytes = HARD_SWAP_BYTES,
            |value: &mut ResourceSnapshot| {
                value.compressor_occupied_bytes =
                    percent_ceiling(value.total_memory_bytes, u64::from(HARD_COMPRESSOR_PERCENT));
                value.available_percent = HARD_COMPRESSOR_COMPANION_AVAILABLE_PERCENT;
            },
        ] {
            let mut state = WatchdogState::default();
            let mut sample = snapshot();
            pressure(&mut sample);
            assert_eq!(
                state.observe(&sample).expect("hard boundary"),
                WatchdogDecision::Tripped(WatchdogTripReason::HardPressure)
            );
        }

        let mut state = WatchdogState::default();
        let mut hard = snapshot();
        hard.swap_used_bytes = HARD_SWAP_BYTES;
        assert_eq!(
            state.observe(&hard).expect("hard"),
            WatchdogDecision::Tripped(WatchdogTripReason::HardPressure)
        );
        assert_eq!(
            state.observe(&snapshot()).expect("first trip preserved"),
            WatchdogDecision::Tripped(WatchdogTripReason::HardPressure)
        );
    }

    #[test]
    fn soft_pressure_requires_two_signals_for_thirty_seconds_and_resets() {
        let mut state = WatchdogState::default();
        let mut soft = snapshot();
        soft.available_percent = SOFT_AVAILABLE_PERCENT - 1;
        soft.reclaimable_uncompressed_bytes = SOFT_RECLAIMABLE_BYTES - 1;
        for _ in 0..(SOFT_CONSECUTIVE_SAMPLES - 1) {
            assert_eq!(
                state.observe(&soft).expect("sustained compound pressure"),
                WatchdogDecision::Continue
            );
        }
        assert_eq!(
            state.consecutive_soft_samples(),
            SOFT_CONSECUTIVE_SAMPLES - 1
        );
        assert_eq!(
            state.observe(&snapshot()).expect("healthy reset"),
            WatchdogDecision::Continue
        );
        assert_eq!(state.consecutive_soft_samples(), 0);
        for _ in 0..(SOFT_CONSECUTIVE_SAMPLES - 1) {
            assert_eq!(
                state.observe(&soft).expect("compound pressure after reset"),
                WatchdogDecision::Continue
            );
        }
        assert_eq!(
            state.observe(&soft).expect("thirtieth second"),
            WatchdogDecision::Tripped(WatchdogTripReason::SoftPressure)
        );
    }

    #[test]
    fn rapid_swap_growth_combines_with_compression_without_crossing_absolute_swap_limit() {
        let mut state = WatchdogState::default();
        let mut sample = snapshot();
        sample.swap_used_bytes = 0;
        sample.compressor_occupied_bytes = percent_ceiling(
            sample.total_memory_bytes,
            u64::from(SOFT_COMPRESSOR_PERCENT),
        );
        for index in 0..(SOFT_TREND_WINDOW_SAMPLES + usize::from(SOFT_CONSECUTIVE_SAMPLES) - 1) {
            sample.swap_used_bytes = (index as u64) * 100 * 1024 * 1024;
            let decision = state.observe(&sample).expect("bounded swap trend");
            if index + 1 < SOFT_TREND_WINDOW_SAMPLES + usize::from(SOFT_CONSECUTIVE_SAMPLES) - 1 {
                assert_eq!(decision, WatchdogDecision::Continue);
            } else {
                assert_eq!(
                    decision,
                    WatchdogDecision::Tripped(WatchdogTripReason::SoftPressure)
                );
            }
        }
    }

    #[test]
    fn strict_parsers_reject_malformed_and_contradictory_data() {
        assert!(parse_memory_pressure(b"other output\n").is_err());
        assert!(parse_memory_pressure(b"System-wide memory free percentage: 101%\n").is_err());
        assert!(parse_memory_pressure(
            b"System-wide memory free percentage: 50%\nSystem-wide memory free percentage: 50%\n"
        )
        .is_err());
        assert!(parse_memory_pressure(b"\xff\n").is_err());
        assert!(
            parse_vm_stat(
                b"Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 1.\n"
            )
            .is_err()
        );
        assert!(parse_vm_stat(
            b"Mach Virtual Memory Statistics: (page size: 4096 bytes)\nPages free: 1\nPages free: 1.\nPages speculative: 1.\nPages occupied by compressor: 1.\n"
        )
        .is_err());
        assert!(parse_vm_stat(
            b"Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 1.\nPages speculative: 1.\nPages occupied by compressor: 1.\n"
        )
        .is_err());
        assert!(parse_vm_stat(b"bad\n").is_err());
        assert!(parse_swap_usage(b"total = 1.00G used = 0.00G free = 1.00G\n").is_err());
        assert!(parse_memsize(b"36G\n").is_err());
        assert!(parse_memsize(b"0\n").is_err());
        assert!(
            parse_swap_usage(b"vm.swapusage: total = 1.00G used = 2.00G free = 0.00G\n").is_err()
        );
        assert!(parse_swap_usage(b"vm.swapusage: total = 1G used = 0.00G free = 1.00G\n").is_err());
        assert!(parse_size("1.000G").is_err());
        assert!(parse_size("1.00X").is_err());
        assert!(parse_size("1.00G trailing").is_err());
    }

    #[test]
    fn vm_stat_accepts_only_the_two_explicit_official_header_forms() {
        assert_eq!(
            parse_vm_stat(
                b"Mach Virtual Memory Statistics: (page size: 4096 bytes)\nPages free: 1.\nPages speculative: 2.\nPages inactive: 4.\nPages occupied by compressor: 3.\n"
            )
            .expect("legacy official header"),
            (4096, 1, 2, 4, 3)
        );
        assert_eq!(
            parse_vm_stat(
                b"Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 1.\nPages speculative: 2.\nPages inactive: 4.\nPages occupied by compressor: 3.\n"
            )
            .expect("current official header"),
            (16384, 1, 2, 4, 3)
        );
        assert!(parse_vm_stat(
            b"Mach Virtual Memory Statistics: (page size arbitrary 16384 bytes)\nPages free: 1.\nPages speculative: 2.\nPages inactive: 4.\nPages occupied by compressor: 3.\n"
        )
        .is_err());
    }

    fn official_output(command: ResourceCommand, available: u8) -> Vec<u8> {
        match command {
            ResourceCommand::MemoryPressure => {
                format!("System-wide memory free percentage: {available}%\n").into_bytes()
            }
            ResourceCommand::VmStat => b"Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 27854.\nPages speculative: 4617.\nPages inactive: 1053105.\nPages occupied by compressor: 524288.\n".to_vec(),
            ResourceCommand::MemSize => b"38654705664\n".to_vec(),
            ResourceCommand::SwapUsage => b"vm.swapusage: total = 3072.00M  used = 1776.19M  free = 1295.81M  (encrypted)\n".to_vec(),
        }
    }

    struct StaticRunner {
        available: u8,
    }

    impl ResourceCommandRunner for StaticRunner {
        fn run(&self, command: ResourceCommand) -> Result<Vec<u8>, ResourceProbeError> {
            Ok(official_output(command, self.available))
        }
    }

    struct FailingRunner {
        called: Sender<()>,
    }

    impl ResourceCommandRunner for FailingRunner {
        fn run(&self, _command: ResourceCommand) -> Result<Vec<u8>, ResourceProbeError> {
            let _ = self.called.send(());
            Err(ResourceProbeError::CommandFailed)
        }
    }

    #[test]
    fn injected_probe_runner_is_strict_and_bounded() {
        let snapshot = ResourceProbe::new(StaticRunner { available: 50 })
            .sample()
            .expect("valid official output");
        assert_eq!(snapshot.available_percent, 50);
        assert!(
            ResourceProbe::new(StaticRunner { available: 24 })
                .sample()
                .is_ok()
        );
    }

    #[test]
    fn captured_macos_sample_parses_and_is_admitted() {
        let snapshot = ResourceProbe::new(StaticRunner { available: 81 })
            .sample()
            .expect("captured macOS sample");
        assert_eq!(snapshot.available_percent, 81);
        assert_eq!(
            snapshot.reclaimable_uncompressed_bytes,
            (27_854 + 4_617 + 1_053_105) * 16_384
        );
        assert_eq!(snapshot.swap_total_bytes, 3 * 1024 * 1024 * 1024);
        assert_eq!(
            evaluate_pre_start(&snapshot).expect("captured sample policy"),
            PreStartDecision::Admit
        );
    }

    #[test]
    fn watchdog_probe_failure_trips_and_cancels() {
        let (called, received) = mpsc::channel();
        let cancellation = CancellationToken::default();
        let watchdog = ResourceWatchdog::start_with_interval(
            ResourceProbe::new(FailingRunner { called }),
            cancellation.clone(),
            Duration::from_millis(1),
        );
        received.recv().expect("watchdog sample");
        assert_eq!(
            watchdog.stop_and_join().expect("watchdog joins"),
            Some(WatchdogTripReason::ProbeFailure)
        );
        assert_eq!(
            cancellation.reason(),
            Some(CancellationReason::ResourcePressure)
        );
    }

    #[test]
    fn watchdog_does_not_replace_user_cancellation() {
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        let watchdog = ResourceWatchdog::start_with_interval(
            ResourceProbe::new(StaticRunner { available: 1 }),
            cancellation.clone(),
            Duration::from_millis(1),
        );
        assert_eq!(watchdog.stop_and_join().expect("watchdog joins"), None);
        assert_eq!(cancellation.reason(), Some(CancellationReason::User));
        thread::yield_now();
    }

    #[test]
    fn unsupported_status_is_explicit_and_private() {
        let status = unsupported_status();
        let json = serde_json::to_string(&status).expect("status JSON");
        assert_eq!(
            status.capability,
            ResourceCapability::UnsupportedNotEnforced
        );
        assert!(json.contains("unsupported_not_enforced"));
        assert!(!json.contains("/usr/"));
    }

    #[test]
    fn supported_status_json_is_bounded_and_private() {
        let status = status_from_snapshot(&snapshot()).expect("status");
        let json = serde_json::to_string(&status).expect("status JSON");
        assert_eq!(status.capability, ResourceCapability::SupportedEnforced);
        assert!(json.contains("available_percent"));
        assert!(!json.contains("commit-ci-preflight"));
        assert!(!json.contains("/Users/"));
        assert!(!json.contains("memory_pressure"));
    }
}
