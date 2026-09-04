# F-A — Separating the FUNDING-Signature Algebra from the VALUE/CAPABILITY Type-Logic on the Consensus Wire

**Status:** Implemented and verification-gated on `feature/cost-accounted-rho`. **Consensus-critical.** The admission-boundary quorum classification in Section 2 is authoritative for the current wire decoder.

The publication sources were checked directly at `../publications/cost-accounting/cost-accounted-rho.tex` and `../publications/TypedCurrency/typed_value.tex`. The former defines the complete funding-signature grammar at `eq:sig-syntax` as `g | #P | s ∘ s` and its reflective encoding at `eq:app-sig-ground`, `eq:app-sig-hash`, and `eq:app-sig-comp`. The latter supplies the distinct DILL-style value/capability layer. The implementation and proof anchors below were checked against the current tree.

## 0. TL;DR — the two semantic layers are now separated at ingress

The implementation exposes one reflected `Sig` family for two grammars that must remain semantically distinct:

- **Funding-signature grammar** (cost-accounted-rho Appendix A): `s(G) ::= g | #P | s ∘ s` — atoms (`Unit`/`Ground`/`Quote`) and tensor (`And`). This is the grammar used to derive the supply channel `Σ⟦s⟧`.
- **Value/capability type logic** (`typed_value.tex`): `Plus` (`⊕`), `With` (`&`), `Bang` (`!`), `WhyNot` (`?`), and `Lolly` (`⊸`). These are not deploy-funding formers and are rejected by the deploy-envelope decoder.
- **Admission quorum:** `Threshold(k, members)` is neither a funding former nor a capability former at deploy ingress. It is a wire-level `k`-of-`N` predicate over atomic candidate-signer slots and lowers to the scalar threshold carried by `Cosigned`.

**Critical invariant:** neither an admission threshold nor a capability connective is constructed on the consensus funding path. The funding `Sig` that keys `Σ⟦s⟧` is built exclusively by `accounting::funding_sig`: one verified signer becomes `Sig::Ground(pk)`, while multiple verified signers become a left-associated `And` fold of `Ground` atoms. `Sig::from_proto` remains a broad reflection codec, but the deploy decoder never uses it to derive funding.

> **Note — post-§D2.9 funding key (F-A unchanged).** §D2.9 (the funding-key correction, `wd-d2-acceptance-gate.md` §D2.9; cross-ref `d2-9-funding-flow.md`) replaced the funding key with `funding_sig = Sig::Ground(pk)` (single) / the `And`-fold of `Sig::Ground(pkᵢ)` (multi-sig) — the signer's genesis-seeded wallet `Σ⟦Ground(pk)⟧`. The wire-sig digest `Quote(Blake2b256(DEPLOY_SIGNATURE_DOMAIN ‖ wire_sig))` is now ONLY the `deploy_id` basis (on-chain identity), no longer a pool key. **This does not change F-A:** F-A's `is_funding_former` guard gates whatever the funding entry point installs, and since both `Ground` and `And` are funding formers (`is_funding_former(Ground) = true`, `is_funding_former(And(l,r)) = is_funding_former(l) && is_funding_former(r)`), `funding_sig` remains total to funding formers exactly as `envelope_sig*` was — the connective-separation behavior (reject `⊕/&/!/?/⊸`) is identical. If anything §D2.9 makes the funding path MORE paper-faithful: the funding key is now literally the `g` ground atom (the signer's public key), matching the spec's funding grammar `g = `*"an Ed25519 public key, a secp256k1 key hash"* directly, rather than a `Quote` of a wire-sig hash.

The prior decoder traversed every connective as though a scalar signer threshold could preserve its meaning. The repaired decoder performs one structural analysis, admits either an all-required atom/tensor tree or one top-level atomic-member threshold, rejects every capability connective and threshold composition, and derives the funding signature independently from the verified signer set. This closes the confused-deputy path instead of relying on downstream call-graph accidents.

## 1. Provenance of the funding signature (traced end-to-end)

- **Deploy ingress** (`node/src/rust/api/deploy_grpc_service_v1.rs:256-287`): the wire `DeployDataProto` (which MAY carry `sig_algebra: SigCompound`, field 17) is decoded by `DeployData::from_proto_cosigned` (`models/src/rust/casper/protocol/casper_message.rs:1135`).
- **Dispatch:** if `sig_algebra` is present, it is the authoritative envelope representation. `from_proto_cosigned` decodes the deploy payload and dispatches to `from_proto_cosigned_with_sig_algebra` before reading or validating the unused flat signer, algorithm, or threshold fields. This prevents stale compatibility fields from changing the meaning of an algebra-bearing envelope.
- **Single-pass structural analysis:** `analyze_funding_algebra` simultaneously collects signer atoms, computes the minimum required signer count with checked arithmetic, and records whether all collected signers are required. An atom/tensor tree is accepted only when every leaf is an atom. A threshold is accepted only at the top level, with `1 ≤ k ≤ N` and each of its `N` direct members one atomic signer. `Plus`, `With`, `Bang`, `WhyNot`, `Lolly`, missing connectives, and every nested threshold composition are rejected.
- **Lowering:** the accepted analysis is lowered to a canonical flat `Cosigned<DeployData>` through `Cosigned::from_signed_data` for an all-required envelope or `Cosigned::from_signed_data_threshold` for a quorum. The scalar threshold therefore counts candidate-signer slots exactly; it never pretends to encode a quorum over nested formulas.
- **The funding `Sig` is re-derived from that `Cosigned`** (`accounting::funding_sig`, `mod.rs`): one signer → `Sig::Ground(pk)`; ≥2 → left-assoc `Sig::And`-fold of `Sig::Ground(pkᵢ)` over the *non-placeholder* signer set — keyed by the signers' GROUND public keys, so the pool is the genesis-seeded wallet `Σ⟦Ground(pk)⟧` (§D2.9). *(Pre-§D2.9 this was `envelope_sig`, an arity-only `Sig::Quote(Blake2b256(…‖sig))` fold; that wire-sig digest now survives only as the `deploy_id` basis.)*
- **The acceptance gate** (`acceptance.rs`) calls `funding_sig(&cosigned)` — NEVER `from_proto`. `sig_key = funding_sig.lane_hash()`, `channel = supply::supply_channel(&funding_sig)` = the signer's wallet `Σ⟦Ground(pk)⟧`. Only the `Unit`/`Ground`/`Quote`/`And` arms of `from_sig` are ever reached on consensus.

**Conclusion:** a deploy's funding signature cannot carry a capability connective or a threshold node into the pool key. An accepted wire algebra determines only the canonical candidate signer set and the scalar admission quorum. Funding is re-derived from the verified, non-placeholder signer set as `Ground` atoms combined by `And`.

> **§D2.9 update to this trace (F-A unchanged).** For the record, the PRE-§D2.9 keying was `envelope_sig*` (a `Quote`-atom arity fold); the bullets above now state the §D2.9 key directly. The §D2.9 funding `Sig` re-derived from the `Cosigned` is `funding_sig` — one signer → `Sig::Ground(pk)`; ≥2 → the left-assoc `And`-fold of `Sig::Ground(pkᵢ)` — keyed by the signers' GROUND public keys (the genesis-seeded wallet `Σ⟦Ground(pk)⟧`); the `Quote(Blake2b256(… ‖ sig))` derivation survives ONLY as the (unchanged) `deploy_id` basis. The acceptance gate (`acceptance.rs`) now keys `sig_key`/`channel` from `accounting::funding_sig` instead of `envelope_sig`. F-A is untouched by this: `funding_sig` is still total to `Ground`/`And` (both funding formers), so `is_funding_former` and the connective separation behave identically; the funding path becomes MORE paper-faithful (the key is literally `g` = the public key).

> **Pin with a test:** post-§D2.9 `funding_sig` folds over the *non-placeholder* signer set (unsigned threshold cosigners are filtered out), so a k-of-N deploy funds from the tensor of the k present members' wallets `Σ⟦Ground(pkᵢ)⟧`. The `deploy_id` basis (`envelope_sig`) still folds over all N for stable on-chain identity. Internally consistent (funding is always `Ground`/`And`) — freeze the meaning with a test (§6).

## 2. Which connectives are funding-LEGITIMATE

| Connective | Paper home | Funding-legitimate? | Disposition |
|---|---|---|---|
| `Unit`/`Ground`/`Quote` | §App-A atoms (`g`/`#P`) | YES — funding atoms | Stay FUNDING formers |
| `And` (tensor ∘) | §3.2/§App-A `s∘s` | YES — the only funding combinator | Stays the FUNDING former |
| `Threshold{k, members}` | admission extension (k-of-N) | NO — admission predicate only | Lower to a scalar quorum over atomic members |
| `Plus` (⊕), `With` (&), `Bang` (!), `WhyNot` (?), `Lolly` (⊸) | `typed_value.tex` | NO — value/capability type-logic | → CAPABILITY layer |

### The Threshold decision

`Threshold` is an admission-boundary quorum, not a funding-`Sig` former. The paper's funding grammar `g | #P | s ∘ s` has no quorum constructor, and the deployed `Cosigned` representation carries one scalar threshold over signer slots. Consequently, every direct threshold member must be atomic. Supporting a nested member formula would require a different wire representation that preserves a quorum over subformulas; flattening it would change the authorization policy.

The broad Rocq `sig_algebra` remains available to reason about value/capability formulas. The executable admission subset is defined separately by `admission_sig_algebra_atom`, `admission_sig_algebra_all_required`, and `admission_sig_algebra_valid`. `admission_sig_algebra_scalar_policy_sound` proves that every admitted term is either an all-required atom/tensor tree or one top-level atomic-member threshold. `admission_sig_algebra_valid_sound` proves broad-algebra well-formedness, while `admission_sig_algebra_quorum_sound` proves `1 ≤ min_required ≤ atom_count`.

## 3. The separation design

**Goal:** the funding reflection accepts only `{Unit, Ground, Quote, And}`; deploy-envelope admission accepts an all-required atomic tensor or a top-level atomic-member threshold; and the capability/type path retains the broad algebra without being confused for funding.

### Option (a) — `Sig::is_funding_former()` guard at the funding entry points — implemented, no wire change
*(In the F-A implementation steps below, the funding entry point is named `envelope_sig` as it was at F-A time; §D2.9 renamed it to `funding_sig` and re-keyed it to `Sig::Ground(pk)`. Read `envelope_sig → funding_sig`; the `is_funding_former` separation is unchanged.)*

Add `Sig::is_funding_former(&self) -> bool` = `matches!(self, Unit|Ground|Quote) || And(l,r) => l.is_funding_former() && r.is_funding_former()` (false for the type-logic connectives). Enforce at the funding chokepoint `acceptance.rs::build_candidate_with_logic` (`:263`): after `envelope_sig(&cosigned)`, assert `envelope.is_funding_former()`; if not, route to `malformed`/rejected (the `source_to_adt`-failure path, `:287-294`). Since `envelope_sig` is already total to Quote/And, this is a **belt-and-suspenders invariant guard** that can only fire if a future change makes `envelope_sig` non-total — exactly the regression F-A wants to make impossible. Optionally `debug_assert!` + document the precondition on `from_sig`/`supply_channel` (the six arms = capability-only, unreachable on funding).
- **Consensus/back-compat:** ZERO behavior change for any currently-valid deploy; replay-deterministic; no wire change; **no hard-fork.** Independently shippable.

### Option (b) — split the proto into `FundingSig` vs `CapabilitySig` — rejected for this protocol version
Replacing field 17 with separate protobuf types would be a consensus-wire migration without adding an enforceable semantic distinction: every decoder must still classify untrusted input, old producers must remain decodable, and threshold composition still cannot be lowered to the scalar `Cosigned` representation. The current authoritative decoder makes the distinction total and rejects every out-of-subset term before signature validation. A protobuf split is therefore not an unfinished safety requirement; it would be a separately versioned compatibility migration if the wire protocol is ever revised.

### Option (c) — ingress reject — implemented complement to (a)
`from_proto_cosigned_with_sig_algebra` returns a protocol error for every value/capability connective. It also rejects malformed thresholds, any threshold whose direct member is not atomic, and any threshold nested under a tensor. For example, flattening `Tensor(Threshold(1,[a,b]),d)` to 2-of-3 would admit `{a,b}` without mandatory signer `d`; the scalar wire quorum cannot preserve that policy.

The implemented state is **(a) gate invariant + (c) ingress rejection**. This is the complete semantic boundary for the current wire version: the decoder is authoritative, total over every `SigCompound` constructor, and cannot construct a capability or composed-threshold funding policy.

## 4. Back-compat / consensus analysis
- **Wire producers of `sig_algebra`:** grep (non-test) across `casper/`/`node/`/`models/` → **ZERO**. No production path emits a `SigCompound`. The single-sig encoder explicitly omits it (`single_sig_to_proto_omits_sig_algebra_and_cosigners`, `casper_message.rs:2047`).
- **Genesis/standard deploys:** zero `sig_algebra`/`SigCompound`/`Threshold` (non-test). Genesis builds plain single-signer deploys.
- **`CapabilitiesRegistry.rhox`:** treats `fromSig`/`toSig` as OPAQUE byte strings content-hashed into a handle; never constructs a Rust `Sig::Bang`/`Lolly`, never feeds the funding gate/`from_sig`. It is on the CAPABILITY side already; F-A does not disturb it.
- **Construction sites of the six in Rust:** all in TESTS + the `to_proto`/`from_proto`/`from_sig` codec/reflection arms (exercised only by round-trip tests + the dormant `from_proto_cosigned_with_sig_algebra`).

**Therefore:** (a)+(c) cannot reject any currently-valid funding → **no hard-fork.** Only **(b)** carries wire/consensus-fork weight. **Rocq:** the funding `sig` inductive is `SUnit|SGround|SQuote|SAnd` only; gating the six breaks no proof (makes Rust MATCH the proved model). Under (B), adding `SThreshold` is the new proof obligation.

## 5. Consensus decisions and compatibility boundary

1. **Threshold classification:** an admission-boundary quorum over atomic candidate-signer slots; funding remains `g | #P | s ∘ s`.
2. **Separation mechanism:** the funding-former invariant and ingress rejection are both enforced.
3. **Compatibility precedence:** when `sig_algebra` is present, all unused flat envelope fields are ignored. When it is absent, the legacy flat signer and threshold fields retain their existing validation rules.
4. **Wire representation:** field 17 remains backward-compatible, while its decoder enforces the funding/admission subset before any lowering. Separate protobuf types are unnecessary for semantic safety and are not part of this protocol version.
5. **Capability ownership:** `Plus`, `With`, `Bang`, `WhyNot`, and `Lolly` remain value/capability constructs and cannot authorize a deploy envelope.

## 6. Verification obligations

1. Unit tests cover every accepted atom/tensor/threshold shape and every rejected missing, capability, nested-threshold, invalid-bound, overflow, duplicate-signer, empty-signer, and invalid-signature case.
2. Wire-boundary tests prove that an algebra-bearing envelope ignores every flat compatibility field, while an algebra-free envelope preserves the legacy path.
3. Round-trip and integration tests prove canonical signer ordering, exact scalar quorum reconstruction, and stable deploy identity.
4. Rocq proves admission-subset soundness and quorum bounds without axioms; `coqchk` rechecks the compiled proof graph.
5. Property tests cover signer permutations and threshold boundaries; the complete release, strict Clippy, doctest, security, TLA+, and branch-coverage gates remain release criteria.

## Critical files
- `rholang/src/rust/interpreter/accounting/mod.rs` (`Sig` @1245, `envelope_sig*` @1324-1393, `from_proto` @1467, `from_sig` @1681; add `is_funding_former()`)
- `casper/src/rust/util/rholang/acceptance.rs` (`build_candidate_with_logic` @263 — enforcement chokepoint)
- `models/src/rust/casper/protocol/casper_message.rs` (`from_proto_cosigned`, `from_proto_cosigned_with_sig_algebra`, and `analyze_funding_algebra`; authoritative algebra dispatch, single-pass admission analysis, and lowering)
- `models/src/main/protobuf/CasperMessage.proto` (`sig_algebra` field 17 @184, `SigCompound`/`SigAtom`/`SigThreshold` @219-289)
- `casper/src/rust/util/rholang/supply.rs` (`supply_channel` @47 — funding channel keying)
