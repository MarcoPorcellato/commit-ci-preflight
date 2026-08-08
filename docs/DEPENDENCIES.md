# Dependency register

This application commits `Cargo.lock`. Every direct dependency is introduced in
the pull request that first needs it and is reviewed for purpose, license,
features, minimum Rust version, and transitive impact.

## Direct dependencies

| Crate | Version introduced | Purpose | Enabled features | License |
|---|---:|---|---|---|
| `serde` | 1.0.229 | Typed receipt serialization and strict deserialization | `derive` | MIT OR Apache-2.0 |
| `serde_json` | 1.0.151 | JSON values and deterministic compact encoding | default `std` | MIT OR Apache-2.0 |
| `sha2` | 0.11.0 | Pure-Rust SHA-256 receipt integrity digest | no default features | MIT OR Apache-2.0 |
| `schemars` | 1.2.2 | Generate the pinned JSON Schema from Rust receipt types | default `derive`, `std` | MIT |

## Selection notes

- These crates are independently maintained and broadly used Rust ecosystem
  components. They are not copied or vendored into this repository.
- Hex encoding remains a small internal function, avoiding another dependency.
- Receipt errors use the standard library rather than an error-derive crate.
- No dependency performs network access, telemetry, process execution, or
  filesystem discovery at application runtime.
- The initial `cargo metadata --locked` inventory found only permissive
  transitive licenses: Apache-2.0, MIT, Unlicense, and Unicode-3.0 combinations.
- Dependency advisories remain a separate remote/default-branch signal until a
  pinned local advisory scanner is added under the implementation plan.

Versions in this document describe the first accepted lock state. `Cargo.lock`
is authoritative for exact resolved versions.

