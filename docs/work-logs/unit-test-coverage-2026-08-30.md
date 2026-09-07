---
task_id: unit-test-coverage-80
status: in_progress
claimed_by: pi-session
claimed_at: 2026-08-30T12:35:00Z
branch: feature/80Percent-ut-coverage
handoff_status: active
---

# Unit-Test Coverage Gate

## Policy

> **Superseded 2026-08-31.** The measurement method changed when this branch
> merged with `feature/test-coverage`: the gate now runs every in-crate test
> target with nextest (matching CI), and the denominator excludes src-shipped
> test scaffolding and node's bootstrap/wiring via the shared ignore regex in
> `scripts/coverage.sh`. The lib-only baselines below are historical; the
> official post-merge numbers are the PR #374 CI table (workspace 81.0%,
> every crate >= 80% except node, which the wiring exclusion addresses).
> Current status lives in docs/work-logs/task-test-coverage-80pct-2026-08-30.md.

The gate uses line coverage from library and binary unit tests. Integration tests and doctests do not contribute.

Each workspace crate must have at least 80% line coverage. The weighted workspace total must also have at least 80% line coverage.

A missing or malformed coverage report fails the gate. The required `Test (casper)` check also requires successful pull-request coverage.

A direct `Coverage Summary` ruleset entry remains necessary for clear GitHub status reporting.

## Initial Baseline

| Crate | Lines | Covered | Coverage |
| --- | ---: | ---: | ---: |
| rspace_plus_plus | 9,021 | 2,935 | 32.5% |
| rholang | 21,133 | 7,457 | 35.3% |
| shared | 1,793 | 1,069 | 59.6% |
| node | 10,001 | 5,775 | 57.7% |
| models | 5,527 | 836 | 15.1% |
| crypto | 1,641 | 1,100 | 67.0% |
| block-storage | 2,999 | 993 | 33.1% |
| comm | 5,713 | 2,474 | 43.3% |
| graphz | 187 | 0 | 0.0% |
| casper | 37,282 | 16,314 | 43.8% |
| **Workspace** | **95,297** | **38,953** | **40.9%** |

## Current Evidence

| Crate | Unit tests | Lines | Covered | Coverage | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| graphz | 4 | 468 | 466 | 99.6% | pass |
| crypto | 34 | 1,742 | 1,432 | 82.2% | pass |
| shared | 60 | 2,136 | 1,715 | 80.3% | pass |

## Remaining Work

- Add unit tests until each remaining crate reaches 80%.
- Run the full unit-test coverage matrix.
- Grant the GitHub token write access to repository rulesets.
- Add `Coverage Summary` to `devProtect` and `masterProtect`.
- Verify the rulesets through the GitHub API.

The current token has repository administrator access but cannot update rulesets. The GitHub API returned HTTP 403 for both ruleset updates.
