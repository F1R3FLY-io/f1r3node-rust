# Case study #14 — Slash liveness depended on invalid latest messages

## 1 · Summary

Pre-fix, the slash-liveness argument depended on a proposer's
latest-message claim — even if the latest message itself was a
malformed or invalid block. A Byzantine proposer could submit a
malformed latest message *exactly* to derail the slash liveness
deadline; honest validators would block on the malformed claim,
never reaching the slash decision. Post-fix, slash liveness is
proven under fairness over *valid* proposer slots, and invalid
proposer slots are explicitly skipped without delay.

## 2 · Discovery technique

**Primary**: TLA⁺ liveness invariant on `AuthorizedSlashFlow.tla`:
`Inv_InvalidIndexSlashLiveness` — *“an invalid-index slash
candidate is eventually decided”* — exhausted finite slot
sequences and surfaced the latent dependency on invalid latest
messages.

**Corroborating**: Hypothesis `liveness_as_safety` encoded the
liveness property as a bounded-depth safety property and
corroborated the TLA⁺ result on randomized lifecycle traces.

## 3 · Witness reproduction

```
cargo test -p casper --test mod -- slashing::integration_t_neglected_invalid_block
```

The fixture
[`casper/tests/slashing/integration_t_neglected_invalid_block.rs`](../../../../../casper/tests/slashing/integration_t_neglected_invalid_block.rs)
encodes the canonical scenario where an invalid proposer slot
would otherwise stall the slash decision.

## 4 · Classification trace

```
threat_class       = projection_risk → permitted_bug_fix
ledger_status      = confirmed_fixed_bug
action             = Keep integration_t_neglected_invalid_block.rs +
                     hypothesis_liveness_as_safety.rs + TLA+ invariant
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                          |
|------------------|-------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | T-9.14, T-LivenessGap (`BugFixSlashLiveness.v` — implicit)                                                          |
| TLA⁺ invariant   | `AuthorizedSlashFlow.tla` `Inv_InvalidIndexSlashLiveness`                                                          |
| Hypothesis       | `hypothesis_liveness_as_safety.rs`                                                                                |
| Rust regression  | `integration_t_neglected_invalid_block.rs`, `uc_27_neglected_invalid_block.rs`, `operational_halt.rs`              |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.16`](../../design/09-bug-fixes-and-rationale.md)                  |

**Stack depth: 4** (Rocq + TLA⁺ + Hypothesis + Rust regression + design).

## 6 · Lessons for the methodology

1. **Liveness is a first-class slashing property**. A protocol that
   detects but does not eventually decide is broken; the
   methodology requires every detection theorem to be paired with
   a liveness theorem.
2. **`liveness_as_safety` is a useful pattern**. Bounded-depth
   liveness (the property holds within `k` rounds) is a safety
   property, which Hypothesis can search efficiently; unbounded
   liveness requires Büchi conversion in TLC, which is more
   expensive.
3. **Invalid latest messages are an adversary lever**. Any protocol
   step that depends on the proposer's latest message must
   gracefully handle malformed claims; the methodology requires
   every such step to have an explicit skip-on-invalid path.
