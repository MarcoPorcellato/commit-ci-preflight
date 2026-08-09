# Dependency register

This application commits `Cargo.lock`. Every direct dependency is introduced in
the pull request that first needs it and is reviewed for purpose, license,
features, minimum Rust version, and transitive impact.

## Direct dependencies

| Crate | Version introduced | Purpose | Enabled features | License |
|---|---:|---|---|---|
| `clap` | 4.6.6 | Typed CLI parsing and stable help/usage errors | `derive`, `error-context`, `help`, `std`, `usage`; default features disabled | MIT OR Apache-2.0 |
| `ctrlc` | 3.5.2 | Convert Ctrl+C and termination signals into cooperative cancellation | `termination` | MIT OR Apache-2.0 |
| `process-wrap` | 9.1.0 | Cross-platform process containment through Unix process groups and Windows Job Objects | `std`, `process-group`, `job-object`; default features disabled | MIT OR Apache-2.0 |
| `nix` (Unix only) | 0.31.1 | Explicit group signals and post-cleanup existence checks | `process`, `signal`; default features disabled | MIT |
| `serde` | 1.0.229 | Typed receipt serialization and strict deserialization | `derive` | MIT OR Apache-2.0 |
| `serde_json` | 1.0.151 | JSON values and deterministic compact encoding | default `std` | MIT OR Apache-2.0 |
| `sha2` | 0.11.0 | Pure-Rust SHA-256 receipt integrity digest | no default features | MIT OR Apache-2.0 |
| `toml` | 1.1.4 | Strict TOML v1 configuration deserialization | `parse`, `serde`, `std`; default features disabled | MIT OR Apache-2.0 |
| `schemars` | 1.2.2 | Generate the pinned JSON Schema from Rust receipt types | default `derive`, `std` | MIT |

## Selection notes

- These crates are independently maintained and broadly used Rust ecosystem
  components. They are not copied or vendored into this repository.
- Hex encoding remains a small internal function, avoiding another dependency.
- Receipt errors use the standard library rather than an error-derive crate.
- No dependency performs network access or telemetry. Process creation occurs
  only through the reviewed supervisor boundary; environment inheritance is
  allowlisted and explicit argv never passes through an implicit shell.
- `process-wrap` replaced the deprecated `command-group` line. Its MSRV of
  Rust 1.87 sets the repository MSRV. The wrapper owns the platform-specific
  containment primitive while CCP independently verifies Unix group cleanup.
- `ctrlc` installs one process-global handler for cooperative CLI cancellation;
  it does not perform command execution.
- The locked `cargo metadata` inventory found no missing license declarations
  and only permissive Apache-2.0, MIT, Unlicense, Unicode-3.0, and Zlib
  combinations.
- Dependency advisories remain a separate remote/default-branch signal until a
  pinned local advisory scanner is added under the implementation plan.

Versions in this document describe the first accepted lock state. `Cargo.lock`
is authoritative for exact resolved versions.
