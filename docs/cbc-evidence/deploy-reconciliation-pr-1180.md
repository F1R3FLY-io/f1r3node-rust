# CbC binding evidence for deploy reconciliation

The [claim](../claims/deploy-reconciliation.md) covers the new verdict, expiry arithmetic, and carrier retention.
This record covers the working tree based on `3adb6942037fa14ef624c97791770db3cea4cf43`.
Maintainer acceptance and the refutation and construction tiers remain pending.

| Artifact | Rust binding evidence |
| --- | --- |
| [Block API](../../casper/src/rust/api/block_api.rs) | The API wrapper tests cover signature-only lookup, rejection of a signature mismatch, rejection of another shard, and the adopted floor as the basis. |
| [Reconciliation](../../casper/src/rust/api/deploy_reconciliation.rs) | The tests cover the strict height bound, held equivocations, a floor-lagged carrier above the bound, and incomplete index coverage. |
| [Deploy lifecycle](../../casper/src/rust/finality/deploy_lifecycle.rs) | The bound test covers both expiry additions at overflow, an included carrier beyond the window end, and unlimited contestability. |
| [Finalizer](../../casper/src/rust/engine/multi_parent_casper/finalization_runner.rs) | The cutoff test covers retention at the first possible carrier height and removal below the cutoff. The finalizer passes that cutoff to the carrier index after an advance. |

The [reconciliation tests](../../casper/tests/api/deploy_reconciliation_test.rs) use the in-memory DAG store.
The held equivocation test checks index presence before the register receives any events.
The floor-lag test uses the actual transaction expiration check with the carrier's own floor.
The prune test removes an indexed carrier and checks both sides of the cutoff.
The restore and upgrade tests cover the anchor and the carrier index watermark.

Run the binding tests with the repository test lock.

```bash
bash scripts/ci/run-tests.sh cargo test --offline --locked -p casper --lib --tests -- \
  deploy_reconciliation deploy_lifecycle deploy_finalization_status
```

The reconciliation implementation has SHA-256 `fe33f07b9cde6dd43e7d24b30f74c3abfb43e3c9100f2a56d413571c6e294cfa`.
The integration test file has SHA-256 `3da6284629df6ead98f4925b62c3c2cb52c02993ba8c7a02d32726e39b5c7db7`.

The prior discharge records cover earlier work.

- [Block API discharge record](https://github.com/F1R3FLY-io/f1r3node-rust/blob/3adb6942037fa14ef624c97791770db3cea4cf43/docs/cbc-evidence/casper-src-rust-api-block-api-rs.md)
  (local Git: `3adb6942037fa14ef624c97791770db3cea4cf43:docs/cbc-evidence/casper-src-rust-api-block-api-rs.md`).
- [Deploy lifecycle discharge record](https://github.com/F1R3FLY-io/f1r3node-rust/blob/3adb6942037fa14ef624c97791770db3cea4cf43/docs/casper/cbc-evidence/casper-src-rust-finality-deploy-lifecycle-rs.md)
  (local Git: `3adb6942037fa14ef624c97791770db3cea4cf43:docs/casper/cbc-evidence/casper-src-rust-finality-deploy-lifecycle-rs.md`).
