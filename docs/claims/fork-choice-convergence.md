# Fork-Choice Convergence Claim

## Status

This claim is mandatory and pending a complete transition-system bridge to Rust.

## Claim

Deploy promotion can override the GHOST head only while the promoted branch contains a novel deploy signature.

Main-ancestry coverage must disable promotion for that signature.

For finite deploy pressure, fair coverage must eventually restore stable GHOST selection.

## Formal statement

For main signature set `M` and candidate signature set `C`:

```text
promote(C, M) => exists sig: sig in C and sig not in M
C subset M => not promote(C, M)
finite pending signatures and fair merge => eventually always GHOST
```

## Assumptions

The deploy signature set is finite during one recovery interval.

The merge action is weakly fair.

No accepted transition removes a covered signature from the main ancestry.

## Evidence

- `formal/rocq/fork_choice/theories/GuardBridge.v::promotion_gate_requires_novel`
- `formal/rocq/fork_choice/theories/GuardBridge.v::covered_branch_cannot_promote`
- `formal/tlaplus/fork_choice/PromotionConvergence.tla`
- `formal/tlaplus/fork_choice/MC_PromotionConvergence.cfg`
- `formal/tlaplus/fork_choice/MC_PromotionConvergence_covered_pre_fix.cfg`

## Gate rule

A counterexample to eventual GHOST restoration refutes this claim and blocks completion.

Documenting the counterexample as an allowed exception does not discharge the claim.
