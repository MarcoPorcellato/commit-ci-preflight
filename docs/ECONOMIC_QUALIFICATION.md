# Economic qualification and measured savings

Commit CI Preflight saves GitHub Actions money only when it replaces billable
hosted work. Standard GitHub-hosted runners are free for public repositories,
and self-hosted runners and standard hosted Dependabot work do not consume
billable Actions minutes. Private repositories receive an included allowance;
usage beyond it is billed.

The current official references are:

- [GitHub Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions);
- [job execution time and per-job rounding](https://docs.github.com/en/actions/how-tos/monitor-workflows/view-job-execution-time);
- [included product usage](https://docs.github.com/en/billing/reference/product-usage-included).

At the observation date, GitHub documented 3,000 included Actions minutes for
GitHub Pro, `$0.006` per minute for a standard two-core Linux runner, and
rounding of every private hosted job up to the next whole minute. Pricing and
allowances can change; recheck them before applying these examples.

## What counts as savings

Keep four quantities separate:

1. **Remote compute avoided**: hosted runner minutes not executed.
2. **Included quota preserved**: avoided minutes that would have consumed the
   monthly allowance.
3. **GitHub charges avoided**: avoided minutes after the allowance was
   exhausted, multiplied by the applicable runner rate.
4. **Net savings**: GitHub charges avoided minus electricity, hardware
   amortization, operator time, maintenance, and local failures.

The first three are measured or bounded-estimated below, as labeled. Net
savings are not yet certified. CCP should not be run solely to claim savings
when local operating cost is unknown or greater than the avoided GitHub charge.

## Results at a glance

| Case | Measured comparison | GitHub compute avoided | Claim |
|---|---:|---:|---|
| This public repository | Standard public hosted CI versus local replacement | `$0` billable savings | Keep hosted CI for ordinary PRs |
| Matryca-Knowledge | 30 receipt-gate events versus a two-job hosted matrix | 28 rounded minutes; about `$0.168` | Measured billed-phase saving |
| Matryca-Brain | 22 CCP-guarded self-hosted attempts scaled from 194 executed hosted PR validations | about 635.1 minutes; about `$3.81` | Bounded counterfactual estimate |

The machine-readable inputs and hand-checked expected results are in
[`economic-case-studies-2026-08.json`](evidence/economic-case-studies-2026-08.json).

## Case study: Matryca-Knowledge

The private repository qualified exact commit
`2dae025a3dde112b7b35a24ddd9b0514d1f1ee7c` locally on Python 3.11 and 3.13.
Each runtime completed 279 tests, 83% aggregate coverage, and all 19 declared
checks. The independently verified receipt IDs were:

- Python 3.11: `sha256:a9465bf170eb110bdd4ed29b3f977bd4dc3bc21a617ad01ca54d0c357d85350d`;
- Python 3.13: `sha256:cee6416805b7b78ad23d385fa8415cd64f4f2615dd9c8df04b016fa6ee58273a`.

The August observation recorded 111 Linux minutes, `$0.666` gross compute,
`$0.096` of allowance or discount, and `$0.570` billed. The old path normally
created two hosted jobs per event; the receipt gate retains one short hosted
job. Because GitHub rounds each job separately, the ordinary event moves from
at least two rounded minutes to one.

Across 30 observed receipt-gate runs, the gate used 538 runner-seconds. Its
successful median was 15 seconds and it consumed approximately 32 rounded
runner-minutes, compared with at least 60 for the former two-job matrix:

```text
avoided_minutes = 60 - 32 = 28
quota_reduction = 28 / 60 = 46.7%
avoided_billed_compute = 28 * $0.006 = $0.168
```

This proves a small but real GitHub-side saving during a billed phase. It does
not prove that operating the Mac solely for this saving is economical. The
local qualification also served provenance and reproducibility needs.

## Case study: Matryca-Brain

“Matryca-Brain” is the approved public name for a private-repository case
study. The underlying private repository and raw billing records are not
published.

### Observed August usage

The GitHub billing dashboard and Actions API were inspected on 2026-08-30:

| Observation | Value |
|---|---:|
| Total Actions runs | 358 |
| PR Validation runs | 314 |
| Dependency-update runs | 22 |
| CCP-guarded self-hosted runs | 22 |
| Linux runner minutes | 5,600 |
| Gross Linux compute | `$33.60` |
| Gross storage | `$0.12` |
| Gross total | `$33.72` |
| Included allowance or discounts | `$18.34` |
| Billed total | `$15.38` |

Before the cutover there were 286 PR Validation runs across 207 distinct
heads: 139 succeeded, 43 failed, 12 were cancelled, and 92 were skipped. The
194 non-skipped executions are the comparable hosted-workflow population.

The 22 local/self-hosted attempts were protected by CCP and covered 19 distinct
commits. Seventeen succeeded, one failed, and four were cancelled. Failed and
cancelled attempts remain in the denominator: hiding them would overstate the
economics. The operator reports that the local ten-stage Ready mirror preserved
the number and quality of tests during the move offline. This is the
comparability input for the estimate, not a claim that a local container is a
GitHub-hosted runner.

### Savings calculation

Dependency-update runs are reported for completeness but excluded from this
comparison because they are not the PR Validation workload replaced by CCP;
GitHub also documents standard hosted Dependabot work as free. No other paid
workflow family was present in the observed run inventory. The observed hosted
average includes failed and cancelled PR runs, so it is more conservative than
using successful full-suite runs only:

```text
observed_minutes_per_executed_pr = 5,600 / 194
                                 = 28.866 minutes

estimated_avoided_minutes = 28.866 * 22
                          = 635.1 minutes

estimated_avoided_github_compute = 635.1 * $0.006
                                  = $3.81
```

From 2026-08-20 through 2026-08-30 the billing dashboard recorded `$0.00` of
new gross usage for Matryca-Brain. That is a useful operational observation,
not permission to extrapolate ten quiet days into a fabricated monthly saving.
The `$3.81` figure is instead tied to the number of comparable CCP attempts.

The repository's observed `$15.38` billed total is historical spend, not an
amount wholly saved by CCP. The study claims approximately `$3.81` of avoided
GitHub Linux compute under the documented comparison. Net savings remain
unknown until local operating costs are priced.

## Reusable decision rule

For a private repository, measure rather than assume:

```text
avoided_hosted_cost
  = replaced_rounded_job_minutes * applicable_runner_rate

net_savings
  = avoided_hosted_cost
  - retained_remote_gate_cost
  - local_energy_cost
  - hardware_amortization
  - operator_and_maintenance_cost
```

Adopt CCP for economic reasons only when the same required checks remain
covered, the receipt gate is materially cheaper than the replaced hosted jobs,
and the conservative net result is positive. Otherwise use ordinary hosted CI,
especially for public repositories where standard runners already cost zero.

## Evidence limits

- Billing-dashboard values are an observed current-cycle snapshot, not a final
  tax invoice.
- Private run URLs and raw logs are intentionally omitted.
- Receipt integrity proves the declared checks for an exact commit; it does not
  prove producer identity or universal GitHub Actions parity.
- Per-job rounding means workflow wall time cannot be substituted for billable
  runner-minutes.
- Future pricing, included quota, runner type, workflow shape, or local costs
  require a fresh calculation.
