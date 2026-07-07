# Case study #12 — Received slash deploys were not locally authorized

## 1 · Summary

Pre-fix, when a Rust node received a block containing a
`SlashDeploy`, the receiver accepted the deploy on the basis of the
proposer's claim of valid evidence — without independently
re-validating the authorization predicate. A malicious proposer
could mint a `SlashDeploy` against an honest validator with
fabricated evidence; the receiver would accept it. Post-fix,
the receiver runs the `received_slash_deploy_authorized` predicate
locally before accepting.

## 2 · Discovery technique

**Primary**: threat-modeling STRIDE pass (Tampering and Elevation
of Privilege rows on the `SlashDeploy` receive path) surfaced the
missing local-validation step. The STRIDE table entry pointed at
the receive function and asked *“what stops a malicious proposer
from minting an unauthorized SlashDeploy?”*; the answer was *“nothing
on the receive side; only the proposer's claim”*.

**Corroborating**:

- **Kani harnesses** for the authorization predicate
  (`received_slash_deploy_authorized_*`, eight harnesses total)
  proved the predicate is sufficient on the bounded domain.
- **libFuzzer** target `slash_authorization_paths` exercised the
  predicate with structured adversarial inputs.

## 3 · Witness reproduction

The fixture lives in
[`casper/tests/slashing/integration_t_neglected_invalid_block.rs`](../../../../../casper/tests/slashing/integration_t_neglected_invalid_block.rs)
and the authorization-specific regression in
[`casper/tests/slashing/slash_authorization_regressions.rs`](../../../../../casper/tests/slashing/slash_authorization_regressions.rs);
pre-fix the unauthorized SlashDeploy is accepted, post-fix it is
rejected with `InvalidBlock::UnauthorizedSlashDeploy`.

## 4 · Classification trace

```
threat_class       = projection_risk → permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep slash_authorization_regressions.rs + Kani
                     harnesses + libFuzzer target as a layered
                     defense
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                                                                                                                  |
|------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.12, T-Auth (`BugFixSlashAuthorization.v`)                                                                                                                                                                             |
| TLA⁺ model       | `AuthorizedSlashFlow.tla` `Inv_SlashOnlyIfAuthorized`                                                                                                                                                                     |
| Kani harnesses   | `received_slash_deploy_authorized_rejects_invalid_domain`, `received_slash_deploy_authorized_is_conjunction_on_bounded_domain`, plus 6 clause-necessity harnesses (`received_authorization_requires_*_on_bounded_domain`) |
| libFuzzer target | `fuzz/fuzz_targets/slash_authorization_paths.rs`                                                                                                                                                                          |
| Rust regression  | `slash_authorization_regressions.rs`, `prop_t_auth_check.rs`, `uc_21_auth_token_check.rs`                                                                                                                                 |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.14`](../../design/09-bug-fixes-and-rationale.md)                                                                                                                          |

**Stack depth: 5** (Rocq + TLA⁺ + Kani × 8 + libFuzzer + Rust regression + design).

## 6 · Lessons for the methodology

1. **Threat-modeling STRIDE catches *trust* defects**. The bug is
   a *trust-the-proposer* defect; no functional test would surface
   it because the proposer is well-behaved in the test. STRIDE
   asked the adversarial question that exposed the missing check.
2. **Kani harnesses for predicates are cheap and exhaustive**.
   Eight Kani harnesses (one per clause + the conjunction)
   completely characterize the authorization predicate on the
   bounded primitive domain in seconds.
3. **Layered defense for receive-side validation**. The methodology
   uses three layers (Rocq + Kani + libFuzzer) for the
   authorization predicate because it is the *primary security
   boundary* for receive-side trust; the cost is small and the
   coverage is total.
