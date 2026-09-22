# Time-to-feedback evaluation

## What this measures

This method compares the feedback delay of equivalent hosted and local CCP
work. It is not a benchmark of arbitrary machines, a claim that local work is
always faster, or a calculation of net financial savings.

For each comparable event, record these six measures separately:

| Measure | Definition |
|---|---|
| Hosted job duration | Hosted runner start to terminal job result. |
| Hosted queue delay | Workflow dispatch to hosted runner start. |
| Hosted end-to-end feedback | Push or workflow dispatch to required hosted result. |
| Local CCP execution | Guarded local workload start to terminal workload result. |
| Local end-to-end feedback | Deliberate local start to independent local verification. |
| Local preparation overhead | Snapshot, dependency/cache preparation, evidence publication, and manual work. |

The local end-to-end measure includes neither a claim about GitHub availability
nor a replacement for required remote review. Record retained remote gates
separately in the worksheet.

## Required comparable-event data

Use the same commit range, equivalent test scope, runner SKU, repository
visibility, test matrix, cache state, and date range for both sides of a
comparison. Give each hosted/local pair the same opaque scope identifier; the
evaluator rejects a mismatch.

Record failures, cancellations, and incomplete events. They remain visible in
the outcome counts but do not enter time medians. A report with fewer than ten
comparable successful events is `exploratory`; it may describe an observation
but must not be presented as a generally measured outcome.

Every published case study must state the machine, hosted runner SKU, cache
state, test matrix, date range, median, range, exclusions, and local
preparation overhead. Do not discard inconvenient events or compare a reduced
local suite with a larger hosted suite.

## Worksheet format

The offline evaluator reads UTF-8 JSON with schema version
`economic-evaluation-v1`. The fixture directory
contains synthetic method tests, not customer or adopter evidence. One valid
[synthetic worksheet](../tests/fixtures/economic-evaluation-v1/valid-ten-events.json)
is included for inspection.

```json
{
  "schema_version": "economic-evaluation-v1",
  "runner_rate_microusd_per_minute": 6000,
  "events": [
    {
      "outcome": "success",
      "hosted_scope_id": "opaque-scope-01",
      "local_scope_id": "opaque-scope-01",
      "hosted_dispatched_at_seconds": 1000,
      "hosted_runner_started_at_seconds": 1010,
      "hosted_completed_at_seconds": 1180,
      "hosted_job_seconds": [61, 61],
      "retained_hosted_job_seconds": [60, 60],
      "local_started_at_seconds": 2000,
      "local_completed_at_seconds": 2060,
      "local_verified_at_seconds": 2072,
      "local_preparation_seconds": 5
    }
  ]
}
```

Times are integer seconds in one monotonic clock domain per event. The
evaluator requires each success event to satisfy:

```text
hosted_dispatched <= hosted_runner_started <= hosted_completed
local_started <= local_completed <= local_verified
```

Each job duration is rounded up separately to whole minutes. The avoided
rounded minutes for one success event are the original hosted-job total minus
the retained hosted-gate total. An event that avoids zero or negative rounded
minutes is rejected rather than reported as a saving. Store the current runner
rate in integer micro-USD per minute: `6000` represents `$0.006` per minute.
A zero rate is valid for a standard public hosted runner, but it produces zero
avoided GitHub charges.

## Running the offline evaluator

From a reviewed source checkout, run:

```console
cargo run --locked --example evaluate_economic_case_study -- path/to/worksheet.json
```

The example reads only the supplied worksheet and writes an aggregate JSON
report. It does not call GitHub, inspect receipts, read billing dashboards,
access Git history, or send network traffic. It rejects extra arguments and
does not print input event identifiers, timestamps, paths, commands, tokens,
or repository names.

## Reading the report

`classification` is `measured` with at least ten comparable successful events;
otherwise it is `exploratory`. The report includes medians and ranges for
hosted and local end-to-end feedback, plus median queue, execution, and
preparation values. It separately includes the number of comparable successes,
excluded events, all outcome counts, avoided rounded hosted minutes, and
avoided GitHub charge in micro-USD.

For an even-sized sample, the evaluator uses the arithmetic midpoint of the two
central integer-second values, rounded down to the nearest whole second.

The report deliberately does not calculate a universal speed-up percentage.
Use its medians and ranges to describe the observed context, including local
preparation cost and retained remote gates.

## Cost is not time

Lower feedback delay does not prove a billed saving. A private repository may
preserve its included quota or avoid charges only when the local workflow
actually suppresses the equivalent billed hosted jobs. A public repository on
standard GitHub-hosted runners has a zero runner price, so moving ordinary
checks locally does not create a monetary saving.

For the four distinct economic quantities and the existing bounded case
studies, see [economic qualification](ECONOMIC_QUALIFICATION.md). Net savings
remain unknown until electricity, hardware amortization, failures, maintenance,
and operator time are measured conservatively.

## Privacy and evidence limits

Use opaque scope identifiers. Keep raw billing exports, full timestamps, local
paths, commands, receipts, personal data, and proprietary logs outside public
worksheets. A passing evaluator report proves only that its supplied aggregate
inputs satisfy this method. It does not prove producer identity, hosted/local
parity, a future bill, a future queue delay, or a user's net savings.
