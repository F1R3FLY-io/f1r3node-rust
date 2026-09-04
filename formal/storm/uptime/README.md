# Uptime stochastic verification

This directory quantifies long-horizon shard service reliability without
reimplementing Casper, replay, or cost-accounting semantics. The protocol
proofs expose a certified service interface; Storm models environmental
failure, repair, queue pressure, and finality-lag recovery around that
interface.

## Artifact map

| Artifact | Purpose |
| --- | --- |
| `shard_reliability.prism` | Continuous-time Markov chain for one equal-stake shard's eligible-validator count, shared outage state, storage state, admitted-work pressure, lag, and normalized resident-memory headroom |
| `shard_reliability.props` | Thirty-day survival, expected downtime, first-loss lifetime, recovery, steady-state, and resource-tail queries |
| `component_parametric.prism` | Two-state parametric component used to derive exact mean-time-to-failure and steady-state availability functions |
| `component_parametric.props` | Storm-pars properties for the exact component controls |
| `profiles/ci-analytic.json` | Closed-form exponential reliability control |
| `profiles/recovery-quorum-loss.json` | Recoverable exact-quorum boundary |
| `profiles/unsafe-no-repair.json` | Required recovery-liveness counterexample |
| `profiles/unsafe-overload.json` | Required sustained-overload counterexample |
| `profiles/unsafe-memory-leak.json` | Required unreclaimed-resident-growth counterexample |
| `profiles/current-compose-envelope.json` | Complete parameter ranges, provenance, scope, historical context, and exclusions for the canonical three-validator compose shard |
| `profiles/month-*.json` | Exact adverse, central, and favorable instantiations of the declared envelope |
| `profiles/calibrated-profile.schema.json` | Fail-closed schema for a current clean-tree calibrated projection |
| `soak_backtest.prism` | Held-out Bernoulli backtest of the historical pre-repair harness outcome |
| `backtests/soak-31563121791.json` | Exact reconstructable revisions, topology, host limits, workload, observations, and identifiability limits for the longest published daily run |
| `backtests/preflight-33099406770.json` | Content-hashed four-validator preflight observation whose load failure prevented every scheduled soak segment; admissible for deploy-age and transient-response evidence, never lifetime-rate fitting |
| `backtests/public-rss-guard.json` | Revision-specific aggregate RSS guards, reported peaks, and protection outcomes for every published run with an identifiable peak |

The monthly profiles are explicitly bounded engineering assumptions. They are
reportable only together with their full parameter table and evidence hashes;
they do not certify a production uptime level. The historical backtest
reproduces aggregate harness failures but cannot identify continuous-shard
rates because its detailed artifact expired and each iteration rebuilt the
shard. `scripts/check-uptime-storm.sh` fails closed when release projection is
requested without a current, complete calibrated profile.

Each rate must be a nonnegative number. A zero rate disables the related
transition. Positive-rate guards let Storm represent a terminal state as an
absorbing state.

The exact field-by-field relationship between the historical observations and
the current model is recorded in
[`docs/casper/theory/uptime/verification.md`](../../../docs/casper/theory/uptime/verification.md#historical-evidence-mapping-to-the-current-model).
That mapping permits topology and guard reconstruction plus held-out aggregate
outcome validation; it explicitly forbids deriving CTMC transition rates,
continuous lifetime, or cause allocation from the available iteration counts
and peak-only RSS records.

Run `33099406770` supplies a current, non-expired artifact but still contributes
zero continuous-shard soak exposure. Its 124-test integration preflight failed
after 8,954 seconds because 604 load-test deploys had not finalized within the
45-second deadline; all six soak segments were skipped. The failing test used
four equal-stake validators, while the current engineering CTMC is scoped to
three. The checked manifest therefore classifies the deploy observations as
right-censored transient evidence and rejects them as a three-validator
lifetime calibration sample.

The worst/best interpretation is backed by
`formal/tlaplus/uptime/UptimeEnvelopeDominance.tla`, which proves a coupling
order for arbitrary interleavings of shared events, adverse-only failure/load
events, and favorable-only repair/service events. Its unsafe control breaks
that order with a favorable-only failure.

## Model-to-authority boundary

| Storm abstraction | Authoritative source |
| --- | --- |
| `eligible >= QUORUM` | exact weighted finality in `formal/rocq/finalized_floor`, `formal/tlaplus/finalized_floor`, and `clique_oracle.rs`; the count abstraction is valid only for an equal-stake profile |
| `storage_writable` | durable block/RSpace ownership and replay-root invariants in the finalized-floor and cost-accounting proof families |
| `queued < QUEUE_CAP` | block-admission and transport-residency TLA+, Kani, property, and Loom models |
| `lag <= LAG_SLO` | finalized-floor progress and heartbeat/recovery models |
| `resident < MEMORY_CAP` | aggregate RSS telemetry, bounded replay/index caches, and explicit deployment resource-guard configuration |
| independent transitions | mCRL2 overlap checks and Rust/Loom concurrency refinements |

Storm cannot weaken those authorities. A probabilistic result is discarded if
any hard semantic gate fails.

## Running

```bash
scripts/check-uptime-storm.sh
scripts/check-uptime-ALL.sh
RUN_WOLFRAM=1 scripts/check-uptime-ALL.sh
```

Generated matrices, logs, and reports are written below
`target/verification/uptime/`.

The aggregate runner rejects any input change between start and finish. Storm
also rechecks implementation and formal-authority hashes around its numerical
queries, while the shared TLC runner binds and rechecks source, configuration,
checker, fingerprint, seed, and worker identity.

The human-facing report is
`target/verification/uptime/engineering-envelope.md`. It is regenerated from
the machine-readable result so every numeric value stays adjacent to its
parameters and provenance.
