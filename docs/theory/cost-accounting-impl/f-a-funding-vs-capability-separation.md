# F-A — Separating the FUNDING-Signature Algebra from the VALUE/CAPABILITY Type-Logic on the Consensus Wire

**Status:** Authoritative design (Plan-agent pass, read-only; grounded against `feature/cost-accounted-rho @ 087e77b6`). **Consensus-critical.** Requires user RATIFICATION before any wire/consensus change.

> **S0 caveat (process mandate):** the `.tex` line/eq anchors below (`cost-accounted-rho.tex` §App-A `eq:app-sig-ground`/`eq:app-sig-hash`, §3.2/§3.6; `typed_value.tex`) are the design pass's citations. `publications/` is NOT in this working tree, so these anchors are **unverified against the real paper** — confirm them before relying on the prose, and do NOT edit the `.tex`. The Rust/Rocq file:line anchors WERE verified against the tree.

## 0. TL;DR — the divergence is REAL but NOT yet a live vulnerability

The `Sig` enum (`rholang/src/rust/interpreter/accounting/mod.rs:1245`) conflates **two grammars the papers keep apart**:
- **FUNDING-signature grammar** (cost-accounted-rho §App-A): `s(G) ::= g | #P | s ∘ s` — atoms (`Unit`/`Ground`/`Quote`) + the tensor `∘` (`And`). Exactly what the Rocq `sig` inductive admits (`SUnit | SGround | SQuote | SAnd`, `CostAccountedSyntax.v:93-97`). Nothing else.
- **VALUE/CAPABILITY type-logic** (`typed_value.tex`): `Threshold`, `Plus` (⊕), `With` (&), `Bang` (!), `WhyNot` (?), `Lolly` (⊸). NOT funding-sig formers.

**Critical mitigating fact (grep-verified):** the six connectives are NEVER constructed on the consensus FUNDING path. The funding `Sig` that keys `Σ⟦s⟧` is built EXCLUSIVELY by `accounting::envelope_sig*` (`mod.rs:1324-1393`), total to `Quote` atoms folded by `And`, by ARITY over the `Cosigned` envelope — independent of any wire algebra. `Sig::from_proto` (the full-algebra decoder, `mod.rs:1467`) and `from_sig`'s six type-logic arms (`mod.rs:1708-1782`) are DEAD on every non-test consensus path (only non-test callers of `from_sig` are `supply_channel` + `lane_hash`, both fed `envelope_sig` output; `from_proto` has zero non-test callers).

So F-A is a **latent confused-deputy / spec-faithfulness defect**, not a currently-exploitable double-spend: the funding decode is already `g|#P|s∘s`, but the shared `Sig` enum + shared `SigCompound` proto leave the door wedged open for a future refactor to route a `⊕/&/!/?/⊸`-formed channel into funding. The fix makes the separation explicit and enforced.

## 1. Provenance of the funding signature (traced end-to-end)

- **Deploy ingress** (`node/src/rust/api/deploy_grpc_service_v1.rs:256-287`): the wire `DeployDataProto` (which MAY carry `sig_algebra: SigCompound`, field 17) is decoded by `DeployData::from_proto_cosigned` (`models/src/rust/casper/protocol/casper_message.rs:1135`).
- **Dispatch** (`casper_message.rs:1148-1214`): if `sig_algebra` is `Some`, it OVERRIDES the flat `cosigners[]` path → `from_proto_cosigned_with_sig_algebra` (`:1237`). That function does NOT build a `Sig`. It walks the `SigCompound` via `collect_atoms` (`:1315`) to gather leaf `SigAtom`s, and `min_required_for` (`:1402`) to compute the quorum tally, then folds them into a FLAT `Cosigned<DeployData>` via `Cosigned::from_signed_data`/`from_signed_data_threshold` (`crypto/src/rust/signatures/signed.rs:212`). **The connective structure is discarded** — the result carries only a canonical pk-sorted signer list + a scalar `cosigner_threshold`.
- **The funding `Sig` is re-derived from that `Cosigned` by ARITY only** (`accounting::envelope_sig`, `mod.rs:1385-1393`): one signer → `Sig::Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ sig))`; ≥2 → left-assoc `Sig::And`-fold of `Sig::Quote(Blake2b256(COMPOUND_DEPLOY_SIGNATURE_DOMAIN ‖ sig_i))`.
- **The acceptance gate** (`acceptance.rs:266`) calls `envelope_sig(&cosigned)` — NEVER `from_proto`. `sig_key = envelope.key()` (`lane_hash`), `channel = supply::supply_channel(&envelope)`. Only the `Unit`/`Ground`/`Quote`/`And` arms of `from_sig` are ever reached on consensus.

**Conclusion:** a deploy's funding signature cannot today carry the six connectives into the pool key. The wire `SigCompound` influences a deploy in exactly two consensus-visible ways: `collect_atoms` (which atoms become signers) and `min_required_for` (how many must verify). Neither reaches `from_sig` as a connective.

> **Pin with a test (NOT a bug):** `envelope_sig_compound` folds over ALL signers (incl. threshold placeholders), so a k-of-N deploy funds from the tensor of all N member channels, not a k-subset. Internally consistent (funding is always Quote/And) — freeze the meaning with a test (§6).

## 2. Which connectives are funding-LEGITIMATE

| Connective | Paper home | Funding-legitimate? | Disposition |
|---|---|---|---|
| `Unit`/`Ground`/`Quote` | §App-A atoms (`g`/`#P`) | YES — funding atoms | Stay FUNDING formers |
| `And` (tensor ∘) | §3.2/§App-A `s∘s` | YES — the only funding combinator | Stays the FUNDING former |
| `Threshold{k, members}` | impl extension (k-of-N) | CONTESTED — flag for Greg | See decision |
| `Plus` (⊕), `With` (&), `Bang` (!), `WhyNot` (?), `Lolly` (⊸) | `typed_value.tex` | NO — value/capability type-logic | → CAPABILITY layer |

### The Threshold decision (the one genuine judgment call)
`Threshold` is a real funding pattern (k-of-N multi-sig), but the paper's funding grammar `g|#P|s∘s` has no quorum former, and the Rocq `sig` inductive has no `SThreshold`.
- **(A) RECOMMENDED — Threshold is an admission-boundary quorum, NOT a funding-`Sig` former.** This is what the code ALREADY does: `from_proto_cosigned_with_sig_algebra` lowers a `Threshold` to a flat `Cosigned` + scalar `cosigner_threshold`; the funding pool is the `And`-fold of the collected member atoms. The quorum is a crypto-admission predicate (`from_signed_data_threshold`), not a funding-channel former. Funding grammar stays exactly `g|#P|s∘s` (paper- + Rocq-faithful). Zero semantic change; `Sig::Threshold` as a variant reclassifies to the capability/type layer and the funding guard rejects it.
- **(B) — Threshold STAYS a funding former (impl extension beyond the paper).** Then keep it funding-legitimate AND flag Greg for a spec amendment + add a Rocq `SThreshold` constructor and re-check the conservation/confluence proofs.

**The plan proceeds on (A)** and surfaces (B) as a ratification question. Either way, `Plus/With/Bang/WhyNot/Lolly` move to the capability layer unconditionally.

## 3. The separation design

**Goal:** the FUNDING decode/reflection path accepts ONLY `{Unit, Ground, Quote, And}` (+ Threshold per §2) and REJECTS the type-logic connectives, while the CAPABILITY/type path keeps the full algebra.

### Option (a) — `Sig::is_funding_former()` guard at the funding entry points — RECOMMENDED, no wire change
Add `Sig::is_funding_former(&self) -> bool` = `matches!(self, Unit|Ground|Quote) || And(l,r) => l.is_funding_former() && r.is_funding_former()` (false for the type-logic connectives). Enforce at the funding chokepoint `acceptance.rs::build_candidate_with_logic` (`:263`): after `envelope_sig(&cosigned)`, assert `envelope.is_funding_former()`; if not, route to `malformed`/rejected (the `source_to_adt`-failure path, `:287-294`). Since `envelope_sig` is already total to Quote/And, this is a **belt-and-suspenders invariant guard** that can only fire if a future change makes `envelope_sig` non-total — exactly the regression F-A wants to make impossible. Optionally `debug_assert!` + document the precondition on `from_sig`/`supply_channel` (the six arms = capability-only, unreachable on funding).
- **Consensus/back-compat:** ZERO behavior change for any currently-valid deploy; replay-deterministic; no wire change; **no hard-fork.** Independently shippable.

### Option (b) — split the proto into `FundingSig` vs `CapabilitySig` — clean end-state, WIRE CHANGE → ratify
`SigCompound` (field 17) splits into `FundingSig` (atoms + tensor [+ Threshold]) and `CapabilitySig` (the full current algebra, for the `rho:system:capabilities` registry + W2). `DeployDataProto.sig_algebra` re-types to `FundingSig`. This is a wire-format change → MUST be ratified + coordinated with any external client constructing `sig_algebra`. Defer until (a) lands and the W2 capability layer owns `CapabilitySig`.

### Option (c) — ingress reject — complement to (a)
In `from_proto_cosigned_with_sig_algebra` (or `collect_atoms`): if the algebra contains any `Plus/With/Bang/WhyNot/Lolly[/Threshold per §2]`, return a crisp `Err` at the gRPC boundary (the deploy is refused with a clear protocol error rather than silently dropped at the gate).

**RECOMMENDATION:** ship **(a) gate-invariant + (c) ingress reject** now (pure guards, no wire change, no hard-fork, zero behavior change on valid traffic). Hold **(b)** as the ratified end-state once W2's `CapabilitySig` consumer exists.

## 4. Back-compat / consensus analysis
- **Wire producers of `sig_algebra`:** grep (non-test) across `casper/`/`node/`/`models/` → **ZERO**. No production path emits a `SigCompound`. The single-sig encoder explicitly omits it (`single_sig_to_proto_omits_sig_algebra_and_cosigners`, `casper_message.rs:2047`).
- **Genesis/standard deploys:** zero `sig_algebra`/`SigCompound`/`Threshold` (non-test). Genesis builds plain single-signer deploys.
- **`CapabilitiesRegistry.rhox`:** treats `fromSig`/`toSig` as OPAQUE byte strings content-hashed into a handle; never constructs a Rust `Sig::Bang`/`Lolly`, never feeds the funding gate/`from_sig`. It is on the CAPABILITY side already; F-A does not disturb it.
- **Construction sites of the six in Rust:** all in TESTS + the `to_proto`/`from_proto`/`from_sig` codec/reflection arms (exercised only by round-trip tests + the dormant `from_proto_cosigned_with_sig_algebra`).

**Therefore:** (a)+(c) cannot reject any currently-valid funding → **no hard-fork.** Only **(b)** carries wire/consensus-fork weight. **Rocq:** the funding `sig` inductive is `SUnit|SGround|SQuote|SAnd` only; gating the six breaks no proof (makes Rust MATCH the proved model). Under (B), adding `SThreshold` is the new proof obligation.

## 5. Ratification points (what the user must approve)
1. **Threshold classification (§2):** (A) admission-boundary quorum, funding stays `g|#P|s∘s` [recommended] — OR (B) Threshold stays a funding former (→ flag Greg + Rocq `SThreshold` + re-proof).
2. **Separation mechanism + sequencing (§3):** ship (a) gate-invariant + (c) ingress reject now (no wire change, no hard-fork); defer (b) proto split.
3. **Wire change (b), if/when adopted:** re-typing `DeployDataProto.sig_algebra` (field 17) is a wire-format change → explicit ratification + external-client coordination; confirm re-type-in-place vs new field + deprecation.
4. **Hard-fork determination:** confirm (a)/(c) imply no hard-fork (no valid funding rejected; verified no producer emits the six). Only (b) is fork-weight.
5. **Capability-layer ownership:** confirm the six connectives (+ their codec/reflection arms) are capability/type-layer only (W2 + `rho:system:capabilities`), to move out of the funding `Sig` enum in a later coordinated refactor (the planned `And`→`Tensor` rename PR, `mod.rs:1270-1272`, is the natural carrier).

## 6. Implementation checklist (post-ratification)
1. `Sig::is_funding_former()` in `accounting/mod.rs`.
2. Enforce in `acceptance.rs::build_candidate_with_logic` (post-`envelope_sig` → non-funding ⇒ rejected). `debug_assert!` + doc precondition on `from_sig`/`supply_channel`.
3. Ingress reject in `casper_message.rs::from_proto_cosigned_with_sig_algebra`/`collect_atoms` (Err on the type-logic connectives; coordinate Threshold per §2).
4. Tests: (i) a `⊕/&/!/?/⊸` `sig_algebra` deploy rejected at ingress; (ii) the gate invariant fires if `envelope_sig` is made non-total (regression lock); (iii) freeze the Threshold→funding-pool meaning (§2); (iv) plain single-sig and flat N-of-N byte-identical before/after.
5. Docs: add an F-A row to `cost-accounting-threat-model.md`; if §2=(B), open the Greg spec-amendment note + Rocq `SThreshold` task.
6. Do NOT alter `publications/` or the `CapabilitiesRegistry.rhox` wiring.

## Critical files
- `rholang/src/rust/interpreter/accounting/mod.rs` (`Sig` @1245, `envelope_sig*` @1324-1393, `from_proto` @1467, `from_sig` @1681; add `is_funding_former()`)
- `casper/src/rust/util/rholang/acceptance.rs` (`build_candidate_with_logic` @263 — enforcement chokepoint)
- `models/src/rust/casper/protocol/casper_message.rs` (`from_proto_cosigned` @1135, `from_proto_cosigned_with_sig_algebra` @1237, `collect_atoms`/`min_required_for` @1315-1500; ingress reject + Option-b split)
- `models/src/main/protobuf/CasperMessage.proto` (`sig_algebra` field 17 @184, `SigCompound`/`SigAtom`/`SigThreshold` @219-289; Option-b split)
- `casper/src/rust/util/rholang/supply.rs` (`supply_channel` @47 — funding channel keying)
