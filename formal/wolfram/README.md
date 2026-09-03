# Wolfram exploration catalog

This directory contains licensed, opt-in exploration models. They complement
the repository's proof authorities; they do not replace them. Rocq carries
unbounded algebraic proofs, TLA+/Apalache carry concurrent protocol models,
Loom explores Rust memory-order interleavings, and Rust example/property tests
bind those obligations to production code.

Wolfram is used only where its symbolic reduction, exact parameter-region
analysis, graph enumeration, recurrence analysis, or multi-objective
optimization provides distinct evidence. A discovery that changes a
correctness claim must be promoted to the applicable authoritative model and a
production regression before it is accepted as repaired.

## License policy

No default verification gate starts a Wolfram kernel or acquires a license.
The licensed tier runs only when explicitly selected:

```bash
RUN_WOLFRAM=1 scripts/check-fork-choice-ALL.sh
RUN_WOLFRAM=1 scripts/check-finalized-floor-ALL.sh
RUN_WOLFRAM=1 scripts/check-cost-accounted-rho-ALL.sh
```

If a selected gate finds a kernel, every required model must complete and print
its `SELF-TEST: PASS` marker. Wolfram remains optional for release correctness:
the same correctness claim must have evidence in the authoritative unlicensed
layers.

## Artifact map

| Model | Distinct exploratory role | Authoritative counterparts |
| --- | --- | --- |
| `fork_choice/ghost_heaviest_subtree.wl` | Greedy-head and all-expansion-order graph enumeration | `formal/rocq/fork_choice`, `formal/tlaplus/fork_choice`, Rust fork-choice properties |
| `fork_choice/parent_frontier_capacity.wl` | Exhaustive rooted-DAG frontier, canonical-order, and capacity-region enumeration | Rocq `Bound`/`GuardBridge`, TLA+/Apalache `ParentFrontierCapacity`, Loom frozen-frontier capacity model, Rust snapshot properties |
| `finalized_floor/weighted_quorum_regions.wl` | Exact strict-quorum, hard-majority, accountable-overlap, and asymmetric-stake parameter regions | Rocq clique/accountable-safety proofs, TLA+ validator models, Rust `ft_decides_exact` properties |
| `finalized_floor/delta_ratchet.wl` | Symbolic service-rate regimes and lag-dependent positive-feedback exploration | TLA+ heartbeat/backpressure models, finalized-floor liveness tests, production metrics and soaks |
| `finalized_floor/repair_design_regions.wl` | Correctness-constrained parent-admission comparison and symbolic compute/storage/token crossover regions before profiling | Rocq/TLA+ parent-bound and floor-cache proofs, Rust/Loom capacity and cache regressions, targeted production benchmarks for the remaining constants |
| `cost_accounted_rho/reservation_admission_regions.wl` | Correctness-constrained located-purse reservation comparison, path-correlation headroom, priced resource envelopes, and capital-feasible admission concurrency | Rocq `VaultBackedByteAccounting` and settlement proofs, TLA+/Apalache vault/byte/lollipop models, Rust `delta_sigma` and settlement properties, Loom reservation races |

## Deliberate non-duplication

The complete settlement, purse, byte-transfer, storage, and compute-accounting
semantics are not rewritten here. Their conservation and concurrency
obligations already live in Rocq, TLA+/Apalache, Loom, Sage, and Rust. The
reservation/admission model treats component-wise purse isolation, pre-execution
funding, nonnegative refund, and checked nonnegative tariffs as hard constraints;
it compares only the capital and admission consequences around those
constraints. Wolfram may consume these exact formulas plus measured Rust cost
curves to compare candidate algorithms and find Pareto or robust operating
regions. Such a model must state its calibration range and cannot serve as proof
of protocol safety.
