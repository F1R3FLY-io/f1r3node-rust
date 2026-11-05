# FX-21: the elaborated seal format (proposal to lane C)

Lane B, 2026-11-05. Base: `patch/FX-17` at 0b01b1663. Branch `patch/FX-21`. Decision 121 (design r26, §"The elaborated forms" and §"Scope of the handoff"). Lane C's FX-19 memo (527665315) is the other half.

Names: u1…um are deployers, d1…di are deployments, B1…Bj are blocks.

**What lane B asks:** agree items 1 to 4 below, or change them. Lane B then builds the pass and turns `forms_spec` into a normalizer test. No code depends on this memo yet.

## 1. The elaborated seal

The normalizer writes every seal as a `CostSignedTerm`. The term gets one new field, `seal`, that only elaboration writes:

```proto
message CostSignedTerm {
  Par           body      = 1;
  CostSignature signature = 2;
  reserved 3;
  ElaboratedSeal seal     = 4;
}

message ElaboratedSeal {
  int32      kind         = 1;  // 0 deployment, 1 signed, 2 process (# P)
  SealOrigin origin       = 2;  // the admitting envelope: signature, signers, phloPrice
  uint64     phlo_limit = 3;  // phlo, the declared bound of each firing (FX-19)
}
```

- `signature` stays as it is. The owner is still resolved from it at run time (`signature_owner` after substitution), because a seal can name a bound variable.
- `kind` replaces the kind that `check_region_signature` derives today.
- `origin` is the `SealOrigin` message that `SealRecordProto` already uses.
- `phlo_limit` is FX-19's `phlo_limit`. Lane C's bound is b = `phlo_limit` × `origin.phlo_price`.

**Question C1:** is `phlo_limit` on every seal acceptable, with FX-19 reading it only when a sealed receive is stored? The alternative is a limit only on the seals of continuations, but the normalizer cannot always tell which seals will be stored (a `for` in a received process, for example).

## 2. Where the limit comes from

- **Default:** the `phloLimit` of the deployment, as in FX-19 §1 item 1. Every seal that elaboration writes for d1 gets d1's `phloLimit`.
- **Declared:** the parser has no syntax for a limit per seal (`rholang-rs` 17d2f98). Lane B proposes no new syntax for the handoff.

**Question C2:** is the default enough for FX-19, or does lane C need a declared limit per seal before the handoff? A declared limit needs a parser change in `rholang-rs`, and that is a second dependency.

## 3. What the pass writes (the normative forms of §6.3)

| Sugar in d1 by u1 | Elaborated term |
|---|---|
| `P` (no seal) | `{% P' %}[u1]`, kind deployment, where P' is P with every continuation sealed (next row) |
| `for(R){Q}` under seal s, Q not sealed | `for(R){ {% Q %}[s] }` (the defaulting rule, at every depth: `<-`, `<=`, `<<-`, joins, `contract`) |
| `{% for(R){Q} %}[s1 -o s2]` | `{% for(R){ {% Q %}[s2] } %}[s1]` (already done by `desugar::lollipop`) |
| `{% {% P %}[s2] %}[s1]` (only a nested region) | `{% P %}[s1 (*) s2]`, compound sorted (already done by `flatten_nested_signature`) |
| `{% P %}[s1 (*) s2]` | the same compound, sorted, kind signed |
| `{% P %}[# P]` | the same term, kind process. When P is closed, the pass checks that the body is P and refuses a mismatch. Otherwise the reducer keeps the check after substitution (`hash_seal_admissible`). |

- **The deployment seal is not written as `new d(`rho:system:deployerId`) in {% P %}[d]`.** It is written with the signature value `Name(GDeployerId(u1))` and kind deployment. So the elaborated sugar and the explicit form differ in one way only: the explicit form has kind signed and a `new` binding. `forms_spec` compares the cost shape of the two, as it does today. The new normalizer test compares each sugar with its elaborated Par exactly.
- **Inputs to the pass:** the deployment's signature, signers, `phloPrice` and `phloLimit`. `Compiler::source_to_adt_with_normalizer_env` gets an elaboration context with these four values. User deployments go through it from `interpreter.rs`, the v6 gate (`executor.rs`) and block admission.
- **Not elaborated:** system deployments, genesis and exploratory deployments. Their terms have no seal, as today (Decisions 81, 94, 119).

## 4. What the reducer reads

- The reducer reads owner, kind, origin and limit from the term. It keeps no record of its own (Decision 121).
- **Check at entry:** a sealed term is honoured only if its `origin` equals the origin of the code that evaluates it. A term from received data that names another envelope's origin is refused. This is the rule of Decisions 52, 53 and 63, applied to the term.
- **Stored code:** when the reducer stores a receive, it checks the continuation's outermost seal at that moment, against the installer's run. When the receive fires, the reducer evaluates the stored body under that seal. Seals inside the body must have the same origin.
- **Question C3:** `TaggedContinuation.seal` and `ListParWithRandom.seal` then hold only a copy of the term's seal. Lane B proposes to keep both fields for the handoff, filled from the term at store time, because the matcher and the surface charge read them before any body runs. Does FX-19's candidate test read the continuation's limit from `TaggedContinuation.seal`, or from the stored body?
- **Question C4:** the reducer refuses a user deployment whose top level is not one sealed term of kind deployment. Does any lane C test run an unelaborated user term through `EnvelopeExecutor`? Those tests then go through the pass.

## 5. What lane B builds after agreement

1. The proto field and the sorter, hash and substitution changes for it.
2. The elaboration context and the pass (§3).
3. The reducer reads the term's seal and checks its origin (§4).
4. `forms_spec` becomes a normalizer test: each sugar elaborates to exactly its form. The current execution tests stay as a second check of the cost shape.

## 6. Alignment with FX-19 (revision 2, 2026-11-05)

Lane C's FX-19 is in build, and its seal must carry the installing deployment's limit and price. To keep one format for both lanes:

| Value | In the term (`ElaboratedSeal`, lane B) | In the stored record (`SealRecordProto`, lane C) |
|---|---|---|
| Limit | `phlo_limit = 3` | `uint64 phlo_limit = 5` (field 4 stays reserved) |
| Price | `origin.phlo_price` | `origin.phlo_price` (already present) |
| Owner | `signature`, resolved at run time | `owner` (already present) |
| Kind | `kind = 1` | `kind` (already present) |

- The name is `phlo_limit` in both messages, as in the FX-19 memo. Revision 1 of this memo called it `firing_limit`.
- **Lane C can build FX-19 now on `SealRecordProto.phlo_limit`.** Until FX-21 lands, the reducer fills it from the deployment's `phloLimit` when it installs the deployment seal. After FX-21, the reducer copies it from the term's `ElaboratedSeal` when it stores a receive. FX-19's candidate test, charge and refund read only the stored record, so they do not change when FX-21 lands.
- **Question C5:** confirm field number 5 and the name `phlo_limit`, so that the two branches merge without a conflict in `RhoTypes.proto`.
