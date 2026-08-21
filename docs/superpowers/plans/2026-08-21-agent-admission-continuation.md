# Agent Admission Continuation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an opt-in, vendor-neutral CCP lifecycle that releases lost agent tickets, returns a ready capability to a live activity, and requires explicit claim before guarded execution.

**Architecture:** `AdmissionCoordinator` remains the sole FIFO and slot authority. Agent state adds session/boot liveness and a ready-capability digest; a grant-aware `guard exec` consumes the capability atomically before existing supervision begins.

**Tech Stack:** Rust 2024; existing `fs2`, `serde`, `serde_json`, `sha2`, `clap`, and `process-wrap`; deterministic Rust tests. No new dependency, daemon, network, model, Docker, or OrbStack.

**Spec:** `docs/superpowers/specs/2026-08-20-agent-admission-continuation-design.md`

## Global Constraints

- Preserve legacy `run`, `benchmark`, and `guard exec` behavior.
- Keep Rust as the only coordinator; do not add Python or a second scheduler.
- Never persist guarded argv, repository paths, prompts, users, logs, or source content.
- Never start a child before a valid unexpired capability is atomically consumed.
- Unknown, malformed, foreign, contradictory, or ambiguous state remains fail-closed.
- Agent tickets self-release on verified parent loss, PID-1 reparenting, cancellation, or reboot identity change.
- Unsupported liveness means unsupported/blocked, never PID-only proof.
- Do not change receipts or GitHub verification; do not run heavy local work for these tests.

---

## File map

| File | Change |
|---|---|
| `src/agent_session.rs` | New platform-liveness, boot identity, entropy, and deterministic test seams. |
| `src/admission.rs` | Agent-state record, lifecycle, FIFO integration, recovery, claim, and status v3. |
| `src/lib.rs` | Expose `agent_session`. |
| `src/main.rs` | `admission agent` commands and atomic grant-aware `guard exec`. |
| `tests/agent_admission_cli.rs` | New black-box lifecycle and no-preclaim execution tests. |
| `tests/agent_integration_contract.rs` | Public guidance, disclosure, and privacy scans. |
| `README.md`, `docs/PRODUCT_ROADMAP.md`, `docs/COORDINATION_RUNBOOK.md` | Scope exception and operator contract. |
| `docs/agent-integrations/**`, `examples/agent/CCP_ACTIVITY_HANDOFF.md` | Codex, Claude, and terminal instructions. |

### Task 1: Authorize the narrow safety exception

**Files:** Modify `docs/PRODUCT_ROADMAP.md`, `docs/COORDINATION_RUNBOOK.md`, `README.md`, `tests/agent_integration_contract.rs`.

**Consumes:** The approved design spec.

**Produces:** A documented exception for orphan prevention, with no runtime change.

- [x] **Step 1: Add the failing public-guidance assertions**

```rust
let runbook = read(root, "docs/COORDINATION_RUNBOOK.md");
for required in ["explicit claim", "no hidden execution", "legacy `guard exec`"] {
    assert!(runbook.contains(required), "runbook is missing {required}");
}
```

- [x] **Step 2: Verify failure**

Run: `cargo test --locked --test agent_integration_contract`  
Expected: FAIL because the terms are absent.

- [x] **Step 3: Amend docs minimally**

State that orphan prevention is a narrow owner-approved safety exception; agent mode is opt-in, never bypasses unknown ownership, never revives a terminated chat, and never auto-executes a command. Preserve the legacy synchronous path.

- [x] **Step 4: Verify pass and commit**

Run: `cargo test --locked --test agent_integration_contract`  
Expected: PASS.

Commit deferred: source changes are locally verified; commit, push, PR, and merge remain outside the current authorization.

### Task 2: Build deterministic session and capability primitives

**Files:** Create `src/agent_session.rs`; modify `src/lib.rs`; test `src/agent_session.rs`.

**Consumes:** Existing `sha2`; no new crate.

**Produces:**

```rust
pub struct AgentSessionIdentity { pub parent_pid: u32, pub parent_start: String, pub boot_id: String }
pub enum SessionObservation { Live, LostParent, Reparented, Rebooted, Ambiguous, Unsupported }
pub trait SessionInspector { fn observe(&self, identity: &AgentSessionIdentity) -> SessionObservation; }
pub trait CapabilitySource { fn capability_32(&self) -> Result<[u8; 32], AgentSessionError>; }
pub fn digest_capability(bytes: &[u8; 32]) -> String;
```

- [x] **Step 1: Write failing unit tests**

Cover same PID/start/boot = `Live`; PID 1 = `Reparented`; missing parent = `LostParent`; changed start = `Ambiguous`; changed boot = `Rebooted`; unavailable platform = `Unsupported`. Assert a fixed 32-byte test capability serializes only as its SHA-256 digest.

- [x] **Step 2: Verify failure**

Run: `cargo test --locked agent_session::tests`  
Expected: FAIL because the module does not exist.

- [x] **Step 3: Implement the minimal seam**

Production macOS must obtain parent/start/boot evidence and OS entropy without a new crate. If it cannot prove identity, return `Unsupported` or `Ambiguous`. Test fakes inject all observations and capability bytes.

- [x] **Step 4: Verify pass and commit**

Run: `cargo test --locked agent_session::tests`  
Expected: PASS.

Commit deferred: source changes are locally verified; commit, push, PR, and merge remain outside the current authorization.

### Task 3: Add durable agent-ticket state

**Files:** Modify `src/admission.rs`; test `src/admission.rs`.

**Consumes:** Task 2 types.

**Produces:** `AgentTicketState`, `AgentWaitOutcome`, `AgentGrant`, and `submit_agent`, `wait_agent`, `cancel_agent`, `claim_agent_for_guard` on `AdmissionCoordinator`. `claim_agent_for_guard` returns the already-acquired `AdmissionGuard` required by guarded execution.

- [ ] **Step 1: Write failing coordinator tests**

Test five FIFO clients; only the head receives `Ready`; ready expiry advances the next ticket; cancel removes only its ticket; parent loss/PID 1/reboot self-release; malformed state blocks; legacy ticket behavior remains unchanged.

```rust
assert!(matches!(coordinator.wait_agent(&first, &live), AgentWaitOutcome::Ready(_)));
assert!(matches!(coordinator.wait_agent(&second, &live), AgentWaitOutcome::Queued));
```

- [ ] **Step 2: Verify failure**

Run: `cargo test --locked admission::tests`  
Expected: FAIL because agent lifecycle methods do not exist.

- [ ] **Step 3: Implement a separate state record**

Accept one CCP-owned agent-state directory in `validate_layout`. Key each record by existing opaque ticket ID. Store only ticket ID, lifecycle, session identity, heartbeat, expiry, capability digest, and consumed marker. Use `DurableFileSystem` create-new/atomic-replace; keep legacy ticket/lease JSON parse-compatible.

- [ ] **Step 4: Implement transitions under queue lock**

```rust
pub enum AgentWaitOutcome { Queued, Ready(AgentGrant), Cancelled, Expired, ParentLost, Rebooted, Unsupported }
pub struct AgentGrant { pub ticket_id: String, pub capability: [u8; 32], pub expires_at_unix_seconds: u64 }
```

`claim_agent_for_guard` checks FIFO head, session, digest, expiry, and unused state under the queue lock, atomically marks it `claimed`, activates its lease, and returns an `AdmissionGuard` that already owns the authoritative slot. It leaves unrelated files untouched on failure.

- [ ] **Step 5: Verify pass and commit**

Run: `cargo test --locked admission::tests`  
Expected: PASS, including existing legacy tests.

Run: `git add src/admission.rs && git commit -m "feat: add durable agent admission tickets"`

### Task 4: Expose `admission agent` and status v3

**Files:** Modify `src/main.rs`, `src/admission.rs`; create `tests/agent_admission_cli.rs`.

**Consumes:** Task 3 methods.

**Produces:** `submit`, `wait`, `cancel` CLI commands and status schema `3.0` with agent counts.

- [ ] **Step 1: Write failing black-box tests**

Require `admission agent submit --parent-pid <pid> --json`, `wait --ticket <id> --json`, and `cancel --ticket <id> --json`. Reject zero/malformed values with usage exit 2. Assert JSON excludes cwd, argv, paths, and token data except the single ready capability returned to its caller.

- [ ] **Step 2: Verify failure**

Run: `cargo test --locked --test agent_admission_cli`  
Expected: FAIL because `agent` is absent from Clap.

- [ ] **Step 3: Implement bounded dispatch**

Add `AdmissionCommand::Agent { action: AgentCommand }` and route it from `run_admission_command`. Set `ADMISSION_STATUS_SCHEMA_VERSION` to `3.0`, retain schema-2 fields, add `agent_queued_count` and `agent_ready_count`, and retain the process-visibility note.

- [ ] **Step 4: Add five-client CLI FIFO test**

Use an owned temporary coordinator root, hold one legacy slot, submit/wait five clients, release in order, and assert no child marker exists.

- [ ] **Step 5: Verify pass and commit**

Run: `cargo test --locked --test agent_admission_cli`  
Expected: PASS.

Run: `git add src/main.rs src/admission.rs tests/agent_admission_cli.rs && git commit -m "feat: expose agent admission lifecycle"`

### Task 5: Claim atomically in `guard exec`

**Files:** Modify `src/main.rs`, `tests/fixtures/guard_exec_fixture.rs`, `tests/guard_exec_cli.rs`, `tests/agent_admission_cli.rs`.

**Consumes:** `AgentGrant` from Task 3.

**Produces:** `guard exec --agent-ticket <id> --agent-capability <hex>`.

- [ ] **Step 1: Write failing grant tests**

Using the marker fixture, assert no marker before grant-aware guard execution. Wrong, expired, replayed, or session-mismatched capability must exit non-zero and start no child.

```rust
assert!(!started_marker.exists());
assert!(!replayed_started_marker.exists());
```

- [ ] **Step 2: Verify failure**

Run: `cargo test --locked --test agent_admission_cli -- grant`  
Expected: FAIL because guard does not accept a grant.

- [ ] **Step 3: Implement atomic bridge**

Add mutually required agent ticket/capability fields to `GuardExecArgs`. The agent path calls `claim_agent_for_guard` and constructs `GuardExecSession` from its returned, already-held `AdmissionGuard`; it must not call `GuardExecSession::acquire` a second time. Then run existing resource preflight, watchdog, and supervision unchanged. If preflight fails, release the claimed guard and terminalize the consumed ticket without spawning a child.

- [ ] **Step 4: Protect legacy mode**

With no agent fields, retain the exact existing `GuardExecSession::acquire` path. A half-specified grant is a Clap usage error. `run` and `benchmark` cannot consume a grant.

- [ ] **Step 5: Verify pass and commit**

Run: `cargo test --locked --test agent_admission_cli`  
Expected: PASS.

Run: `git add src/main.rs tests/fixtures/guard_exec_fixture.rs tests/guard_exec_cli.rs tests/agent_admission_cli.rs && git commit -m "feat: require agent grant for guarded execution"`

### Task 6: Add neutral harness guidance and a bounded pilot

**Files:** Modify `docs/agent-integrations/HARNESS_INTEGRATION.md`, `docs/agent-integrations/harnesses/codex-app.md`, `docs/agent-integrations/harnesses/codex-cli.md`, `docs/agent-integrations/harnesses/claude-code.md`, `examples/agent/CCP_ACTIVITY_HANDOFF.md`, `docs/COORDINATION_RUNBOOK.md`, `tests/agent_integration_contract.rs`.

**Consumes:** Tasks 2–5.

**Produces:** Copy/paste-neutral sequence and an honest pilot protocol.

- [ ] **Step 1: Write failing contract assertions**

Require `submit`, `wait`, `ready`, `explicit claim`, `parent/session sentinel`, and `terminated chat cannot be revived`. Assert Codex and Claude pages never claim external automatic wake-up.

- [ ] **Step 2: Verify failure**

Run: `cargo test --locked --test agent_integration_contract`  
Expected: FAIL because lifecycle guidance is absent.

- [ ] **Step 3: Write the actual guidance**

Codex uses `/goal` and same-task wait completion; Claude and terminal use the same neutral CLI. Notifications are informative only. Examples contain no private paths, raw command payloads, capability tokens, logs, or code-intelligence vendor names.

- [ ] **Step 4: Add pilot record template**

Record opaque ticket, exact SHA only when relevant to later guarded work, state transitions, deliberate parent-loss result, post-run admission, and child-start truth. Mark it `NOT_RUN` until real execution; it is not a receipt.

- [ ] **Step 5: Verify pass and commit**

Run: `cargo test --locked --test agent_integration_contract`  
Expected: PASS.

Run: `git add docs/agent-integrations examples/agent docs/COORDINATION_RUNBOOK.md tests/agent_integration_contract.rs && git commit -m "docs: add agent admission handoff guidance"`

### Task 7: Full non-heavy verification and review

**Files:** Review all Task 1–6 files.

**Produces:** Evidence-backed local handoff; no merge, release, or global activation.

- [ ] **Step 1: Run static checks**

Run: `cargo fmt --all -- --check`  
Run: `RUSTFLAGS=-Dwarnings cargo check --locked`  
Expected: PASS.

- [ ] **Step 2: Run deterministic suite**

Run: `cargo test --locked --all-targets --all-features`  
Expected: PASS. Record ignored native tests as `NOT_RUN` unless admission and resource preconditions are separately authorized.

- [ ] **Step 3: Verify dependencies and public boundaries**

Run: `git diff -- Cargo.toml Cargo.lock`  
Expected: no change.

Scan staged public docs for `/Users/`, `/private/tmp`, `GitNexus`, `Serena`, `Bearer `, `ghp_`, and `token=`. Any match blocks publication.

- [ ] **Step 4: Review exact scope**

Run: `git diff --check`  
Run: `git diff --stat`  
Expected: only planned Rust, tests, and docs; no receipt, cache, target output, binary, or credential.

- [ ] **Step 5: Request review before publishing**

Report base/head, completed tranches, test output, dependency result, pilot state, platform limits, and that no heavy qualification or GitHub check is implied.
