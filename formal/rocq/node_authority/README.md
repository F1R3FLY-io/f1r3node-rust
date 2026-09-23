# Node authority construction proofs

This project proves attachment, coverage, event ledger, work budget, and comparison guard properties for the Batch B2 detached authority observer. It does not discharge CLAIM-CASPER-NODE-OBSERVATION-003.

## Modules

| Module | Scope |
| --- | --- |
| `AuthorityObserver` | Once-only attachment, installation coverage, and the nonblocking event ledger. |
| `AuthorityWork` | The shared sticky work budget, checked overflow, and the digest guard on result comparison. |
| `MainTheorem` | The 15 exported results. |

## Correspondence

| Definition | Rust boundary | Scope |
| --- | --- | --- |
| `attach`, `attach_all` | The `OnceLock` in the Casper instance and `attach_observer` in dispatch. | The first attachment sets the cell. Every later attempt returns `AlreadyAttached`. |
| `install`, `shutdown`, `accepted` | `ObserverController::install`, `close`, and the instance check in `authority_snapshot`. | Installation numbers use checked addition under a bound. A request is accepted only for the live binding with the current installation number. |
| `emit`, `drain`, `ledger_run` | `ObserverBinding::emit_snapshot`, `lose`, and the bounded channel drained in `authority_snapshot`. | The sequence counter is checked under a bound. An invalid hash or a full queue counts as loss. A drain never changes the attempted, delivered, or lost counts. |
| `complete` | `Coverage::complete` in the controller. | Complete coverage means active, no loss, and no overflow. |
| `charge`, `charge_all` | `CheckedWork::update` for every path of one request. | One budget serves all paths. A failure is sticky and keeps the counters reached before it. |
| `checked_add` | `u64::checked_add` before each limit comparison in the work meter. | A machine bound at or above the limit cannot mask a limit failure. |
| `compare` | The authority digest check before an exact, original, or reference comparison. | A comparison result exists only for equal digests. |

The model uses natural numbers for identifiers, counters, and amounts. Rust uses checked `u64` arithmetic and atomic counters. The ledger models the sequence bound only. The lost counter uses the same checked pattern in Rust and is trusted to it.

The correspondence depends on the `OnceLock` semantics, the atomic ordering in the controller, the bounded channel, and the hash function. No theorem claims that equal digests imply equal inputs.

## Exported theorems

| Theorem | Result |
| --- | --- |
| `authority_instance_attaches_at_most_once` | Any sequence of attachment attempts on one instance has at most one success. |
| `authority_first_attachment_wins` | The cell holds the first binding after any later attempts. |
| `authority_replaced_binding_refused` | A binding accepted before an installation is refused after it, with or without installation overflow. |
| `authority_shutdown_refuses_all` | A shut down controller accepts no binding. |
| `authority_installation_never_wraps` | An installation number stays within the bound or does not change. |
| `authority_queue_never_exceeds_capacity` | Every reachable ledger holds at most the queue capacity. |
| `authority_ledger_accounts_for_every_attempt` | Every reachable ledger satisfies attempted equals delivered plus lost. |
| `authority_complete_coverage_delivers_every_attempt` | Complete coverage implies every attempt was delivered. |
| `authority_sequence_exhaustion_refused` | An attempt at the sequence bound keeps the count and records overflow. |
| `authority_shared_budget_bounded` | Any charge sequence keeps the shared budget within its limit. |
| `authority_budget_failure_sticky` | A failed budget refuses every later charge without change. |
| `authority_budget_failure_keeps_usage` | A failing charge keeps the prior usage and exceeded the limit. |
| `authority_paths_share_one_budget` | Charges from two paths compose as one sequence on one budget. |
| `authority_checked_overflow_is_limit_failure` | A checked-add overflow under a bound at or above the limit is also a limit failure. |
| `authority_comparison_requires_equal_digest` | A comparison result exists only when both digests are equal. |

`MainTheorem` exports all 15 results. Each result must report `Closed under the global context` after compilation.

No custom axiom, admission, or parameter is permitted in the trust base.

## Verification

Run these commands in the resource-limited verifier container:

```bash
coq_makefile -f _CoqProject -o Makefile
make -j1
coqchk -Q theories NodeAuthority NodeAuthority.MainTheorem
```

The formal gate separately prints the assumptions of each exported theorem and requires 15 closed sets.

The [TLA+ area](../../tlaplus/node_observation/README.md#batch-b2-applicability-review) holds the B2 applicability review. Batch B2 adds no bounded model, so its refutation tier is inherited from the accepted session and capture models only.

These theorems do not prove capability exclusion, finalizer hook placement, adoption routes, reference semantics, charge placement at every operation, refusal completeness, or effect confinement. Those properties keep pending construction.

Named maintainer review of every B2 applicability decision and acceptance of the claim remain pending.
