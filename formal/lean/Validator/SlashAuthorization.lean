/-
  Validator.SlashAuthorization — Lean 4 mirror of the validator-scoped P1
  (slash-authorization soundness) obligation from the Rocq slashing development
    `formal/rocq/slashing/theories/Validator.v`         (the BondMap slash taxonomy)
    `formal/rocq/slashing/theories/ValidatorLifetime.v` (`stale_evidence_not_authorized`)
  (Workstream E, stage E4; DR-12).

  SCOPE (DR-12). P1 is a PLATFORM obligation that custom validators INHERIT, so
  the Lean mirror proves it for the BUILT-IN once. We mirror the LOAD-BEARING
  KERNEL — the BondMap slash EFFECT taxonomy — over the exact Rocq model, and we
  port the small `ValidatorLifetime` authorization kernel verbatim. We do NOT
  port the full slashing development (evidence/epoch closure machinery,
  `TwoLevelSlashing`, `authorized_slash_candidate`'s evidence-lookup oracle, the
  Rust shell). Per DR-12 those stay Rocq-only; the BondMap taxonomy here IS the
  load-bearing slash-effect soundness, and the lifetime authorization predicate
  is the faithful kernel of "stale evidence against a rebonded key is rejected".

  The Rocq `Validator` and `BondMap` are modelled EXACTLY as in Rocq:42,56
  (`Validator := nat`; `BondMap := list (Validator * nat)`). Validator equality
  is the Rocq `validator_eq_dec` (Rocq:44) = core `Nat` `DecidableEq`; every Rocq
  `destruct (validator_eq_dec …)` becomes a Lean `if h : k = v then … else …` or
  a `Nat`-eq `split`/`simp` over the same decision.

  NON-VACUITY. `bm_slash_changes_lookup_example` exhibits a CONCRETE BondMap on
  which `bm_slash` actually changes a lookup from a nonzero bond (7) to 0, so the
  slash taxonomy is not vacuous over a trivial map where every lookup is already
  0. `evidence_authorizes_self_example` exhibits a matching (validator,epoch)
  evidence that IS authorized, so `stale_evidence_not_authorized` is not vacuous
  over a predicate that rejects everything.

  DEPENDENCY-FREE: core `Init` only (no mathlib/batteries). `List.Perm` and
  `List.Perm.mem_iff` (used by `bm_slash_many_order_independent`) and the
  decidable `∈` over `Nat` are part of Lean core, so every statement is provable
  fully offline. `omega`/`simp`/`decide`/`rfl`/induction discharge the rest.
-/

namespace Validator

/- ═══════════════════════════════════════════════════════════════════════════
   §1 — Validator identity  (Validator.v:42-45)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- Validators are abstract identifiers, modelled as `Nat` for decidable
    equality without committing to a byte representation (Rocq `Validator`,
    Validator.v:42). The Rocq `validator_eq_dec` (Validator.v:44) is the core
    `Nat` `DecidableEq` instance, used implicitly by every `if … = … then` below. -/
abbrev Validator : Type := Nat

/- ═══════════════════════════════════════════════════════════════════════════
   §2 — Bond map  (Validator.v:56-89)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- A `BondMap` is an association list `(Validator × Nat)` used as a partial
    function from validators to bond amounts (Rocq `BondMap`, Validator.v:56). -/
abbrev BondMap : Type := List (Validator × Nat)

/-- Lookup returns 0 for absent keys, matching the Rust
    `bonds_map.get(v).unwrap_or(&0)` (Rocq `bm_lookup`, Validator.v:58-63):
    walk the list; on the first key equal to `v` return its bond, else recurse;
    `[]` ↦ 0. -/
def bm_lookup : BondMap → Validator → Nat
  | [], _ => 0
  | (k, n) :: rest, v => if k = v then n else bm_lookup rest v

/-- Functional update `B[v ↦ n]` (Rocq `bm_update`, Validator.v:65-72): replace
    the first `(v, _)` entry's bond with `n`, or append `(v, n)` if `v` is
    absent. The faithful structural mirror of the Rocq `Fixpoint`. -/
def bm_update : BondMap → Validator → Nat → BondMap
  | [], v, n => [(v, n)]
  | (k, m) :: rest, v, n =>
      if k = v then (v, n) :: rest else (k, m) :: bm_update rest v n

/-- The slash transition: zero out a validator's bond (Rocq `bm_slash`,
    Validator.v:88), matching the Rholang PoS `state.allBonds.set(validator, 0)`.
    P1 = soundness of THIS effect: a slash zeros exactly the slashed key. -/
def bm_slash (bm : BondMap) (v : Validator) : BondMap :=
  bm_update bm v 0

/-- Slash a whole SET of validators by folding `bm_slash` over the list (Rocq
    `bm_slash_many`, Validator.v:172-176). The order in which the set is folded
    is what `bm_slash_many_order_independent` proves immaterial — the
    multi-parent-merge determinism of slashing. -/
def bm_slash_many : BondMap → List Validator → BondMap
  | bm, [] => bm
  | bm, v :: rest => bm_slash_many (bm_slash bm v) rest

/- ═══════════════════════════════════════════════════════════════════════════
   §3 — Lookup-over-update lemmas  (Validator.v:95-119)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- Lookup hits the value just written at the SAME key (Rocq
    `bm_lookup_update_same`, Validator.v:95-103). Induction on the map; at each
    cons, case on whether the head key equals `v`. -/
theorem bm_lookup_update_same (bm : BondMap) (v n : Nat) :
    bm_lookup (bm_update bm v n) v = n := by
  induction bm with
  | nil => simp [bm_update, bm_lookup]
  | cons hd rest ih =>
      obtain ⟨k, m⟩ := hd
      by_cases hkv : k = v
      · subst hkv; simp [bm_update, bm_lookup]
      · simp [bm_update, bm_lookup, hkv, ih]

/-- Lookup at a DIFFERENT key is unaffected by an update (Rocq
    `bm_lookup_update_diff`, Validator.v:105-119). This is the locality of a
    bond write: writing `v` cannot change `v'`'s observed bond. -/
theorem bm_lookup_update_diff (bm : BondMap) (v v' n : Nat) (hne : v ≠ v') :
    bm_lookup (bm_update bm v n) v' = bm_lookup bm v' := by
  induction bm with
  | nil =>
      simp only [bm_update, bm_lookup]
      -- head key is `v`; `v ≠ v'`, so the lookup misses and falls through to 0.
      have : ¬ (v = v') := hne
      simp [this]
  | cons hd rest ih =>
      obtain ⟨k, m⟩ := hd
      by_cases hkv : k = v
      · -- updated entry is the head: it becomes `(v, n)`; `v ≠ v'` ⇒ lookup misses it.
        subst hkv
        have hne' : ¬ (k = v') := by intro h; exact hne (h ▸ rfl)
        simp only [bm_update, bm_lookup, if_neg hne', reduceIte]
      · -- head untouched; recurse on the tail.
        simp only [bm_update, if_neg hkv, bm_lookup]
        by_cases hkv' : k = v'
        · simp [hkv']
        · simp [hkv', ih]

/- ═══════════════════════════════════════════════════════════════════════════
   §4 — Slash taxonomy (the P1 kernel)  (Validator.v:154-227)
   ═══════════════════════════════════════════════════════════════════════════ -/

/-- P1 — slash zeros the offender's bond (Rocq `bm_slash_lookup`,
    Validator.v:154-158): after slashing `v`, `bm_lookup` returns 0. The base
    slash-effect soundness, directly from `bm_lookup_update_same` at `n = 0`. -/
theorem bm_slash_lookup (bm : BondMap) (v : Validator) :
    bm_lookup (bm_slash bm v) v = 0 := by
  unfold bm_slash
  exact bm_lookup_update_same bm v 0

/-- P1 — slashing twice is idempotent on the lookup (Rocq
    `bm_slash_idempotent_lookup`, Validator.v:160-164): the second slash cannot
    move an already-zeroed bond. Algebraic precursor to protocol-level slash
    idempotence; a double-slash from a multi-parent merge is harmless. -/
theorem bm_slash_idempotent_lookup (bm : BondMap) (v : Validator) :
    bm_lookup (bm_slash (bm_slash bm v) v) v = 0 :=
  bm_slash_lookup (bm_slash bm v) v

/-- P1 — slash LOCALITY / authorization-locality (Rocq `bm_slash_other`,
    Validator.v:166-170): slashing `v` leaves every OTHER validator's bond
    untouched. This is the soundness direction that a slash AFFECTS ONLY THE
    SLASHED KEY — a slash cannot collaterally unbond an innocent validator. -/
theorem bm_slash_other (bm : BondMap) (v w : Validator) (hne : v ≠ w) :
    bm_lookup (bm_slash bm v) w = bm_lookup bm w := by
  unfold bm_slash
  exact bm_lookup_update_diff bm v w 0 hne

/-- P1 — validators OUTSIDE the slashed set keep their bond (Rocq
    `bm_lookup_slash_many_notin`, Validator.v:178-190): if `v ∉ vs`, then folding
    `bm_slash` over `vs` does not change `v`'s lookup. Mirrors the Rocq induction
    (`revert bm; induction vs`), peeling the head with `bm_slash_other`. -/
theorem bm_lookup_slash_many_notin (bm : BondMap) (vs : List Validator)
    (v : Validator) (hnot : v ∉ vs) :
    bm_lookup (bm_slash_many bm vs) v = bm_lookup bm v := by
  induction vs generalizing bm with
  | nil => simp [bm_slash_many]
  | cons x xs ih =>
      -- v ∉ x :: xs ⇒ v ≠ x and v ∉ xs.
      simp only [List.mem_cons, not_or] at hnot
      obtain ⟨hvx, hvxs⟩ := hnot
      simp only [bm_slash_many]
      rw [ih (bm_slash bm x) hvxs]
      -- peel the head slash: x ≠ v, so v's lookup is unchanged by slashing x.
      exact bm_slash_other bm x v (fun hxv => hvx (hxv ▸ rfl))

/-- P1 — every member of the slashed set ends at zero (Rocq
    `bm_lookup_slash_many_in`, Validator.v:192-209): if `v ∈ vs`, then
    `bm_lookup (bm_slash_many bm vs) v = 0`. Mirrors the Rocq case split on the
    `∈` witness, with the `v = x` arm dispatching on whether `x` recurs in the
    tail (if so, the IH closes it; if not, `bm_lookup_slash_many_notin` carries
    the head-slash's zero through the untouched tail). EVERY slashed validator is
    unbonded — soundness of the batch-slash effect. -/
theorem bm_lookup_slash_many_in (bm : BondMap) (vs : List Validator)
    (v : Validator) (hin : v ∈ vs) :
    bm_lookup (bm_slash_many bm vs) v = 0 := by
  induction vs generalizing bm with
  | nil => exact absurd hin (List.not_mem_nil)
  | cons x xs ih =>
      simp only [bm_slash_many]
      rcases List.mem_cons.mp hin with heq | hinxs
      · -- v = x: the head slashes v, then fold over xs.
        subst heq
        by_cases htail : v ∈ xs
        · -- v recurs in the tail ⇒ IH zeros it regardless of the head slash.
          exact ih (bm_slash bm v) htail
        · -- v not in the tail ⇒ the head slash's zero survives the untouched fold.
          rw [bm_lookup_slash_many_notin (bm_slash bm v) xs v htail]
          exact bm_slash_lookup bm v
      · -- v ∈ xs: the IH closes it irrespective of the head slash.
        exact ih (bm_slash bm x) hinxs

/-- P1 HEADLINE — slashing a SET is ORDER-INDEPENDENT at the observable
    (lookup) level (Rocq `bm_slash_many_order_independent`, Validator.v:211-227).
    THIS IS CONSENSUS-CRITICAL: it is the multi-parent-merge determinism of
    slashing — two validators that slash the same offender set in different
    orders reach the same bond observation, so a merge cannot disagree on who is
    bonded.

    FORM (decision-point 3). The Rocq theorem is ALREADY stated at the LOOKUP
    LEVEL (`bm_lookup (bm_slash_many bm xs) v = bm_lookup (bm_slash_many bm ys) v`)
    under the SET-EXTENSIONAL premise `∀ v, In v xs ↔ In v ys`. We give the
    headline here under the STRICTLY STRONGER core `List.Perm` premise (a
    permutation implies same membership, via `Perm.mem_iff`), because the task's
    consensus-critical framing is "slashing a SET, reordered" and `List.Perm` is
    the canonical core notion of "same multiset / reordering"; the exact Rocq
    set-extensional form is ALSO provided immediately below
    (`bm_slash_many_order_independent_seteq`) as the 1:1 mirror. Both reduce to
    the same two load-bearing cases (`v ∈ both` ⇒ both 0 by
    `bm_lookup_slash_many_in`; `v ∉ both` ⇒ both unchanged by
    `bm_lookup_slash_many_notin`), exactly as Rocq:221-226 — so the observable
    content is identical. We keep the lookup-level form (not full BondMap
    equality) because that is the FAITHFUL Rocq content: the association lists may
    differ as lists (different residual key order) while denoting the same partial
    function, so only the lookup is order-invariant. -/
theorem bm_slash_many_order_independent (bm : BondMap) (xs ys : List Validator)
    (hperm : List.Perm xs ys) (v : Validator) :
    bm_lookup (bm_slash_many bm xs) v = bm_lookup (bm_slash_many bm ys) v := by
  by_cases hin : v ∈ xs
  · -- v ∈ xs ⇒ (by Perm) v ∈ ys ⇒ both sides are 0.
    have hiny : v ∈ ys := (hperm.mem_iff).mp hin
    rw [bm_lookup_slash_many_in bm xs v hin, bm_lookup_slash_many_in bm ys v hiny]
  · -- v ∉ xs ⇒ (by Perm) v ∉ ys ⇒ both sides equal the un-slashed lookup.
    have hnoty : v ∉ ys := fun hy => hin ((hperm.mem_iff).mpr hy)
    rw [bm_lookup_slash_many_notin bm xs v hin,
        bm_lookup_slash_many_notin bm ys v hnoty]

/-- P1 — the EXACT Rocq:211 mirror (set-extensional premise). Identical
    load-bearing content to the `List.Perm` headline above; provided so the Lean
    development carries the precise Rocq statement (`∀ v, In v xs ↔ In v ys`),
    which is strictly WEAKER (more general) than `List.Perm` and is the form the
    Rocq batch-slash determinism is actually phrased over. -/
theorem bm_slash_many_order_independent_seteq (bm : BondMap)
    (xs ys : List Validator) (hsame : ∀ w, w ∈ xs ↔ w ∈ ys) (v : Validator) :
    bm_lookup (bm_slash_many bm xs) v = bm_lookup (bm_slash_many bm ys) v := by
  by_cases hin : v ∈ xs
  · have hiny : v ∈ ys := (hsame v).mp hin
    rw [bm_lookup_slash_many_in bm xs v hin, bm_lookup_slash_many_in bm ys v hiny]
  · have hnoty : v ∉ ys := fun hy => hin ((hsame v).mpr hy)
    rw [bm_lookup_slash_many_notin bm xs v hin,
        bm_lookup_slash_many_notin bm ys v hnoty]

/-- NON-VACUITY WITNESS (slash taxonomy): a CONCRETE BondMap on which `bm_slash`
    changes a lookup from a nonzero bond to 0. Validator 1 is bonded with stake 7
    in `[(0,5),(1,7)]`; after `bm_slash … 1`, `bm_lookup` of 1 is 0 (whereas it
    was 7 before). This proves the slash effect is observable and the taxonomy is
    NOT vacuously about a map where every lookup is already 0. -/
theorem bm_slash_changes_lookup_example :
    bm_lookup [(0, 5), (1, 7)] 1 = 7 ∧
    bm_lookup (bm_slash [(0, 5), (1, 7)] 1) 1 = 0 := by
  constructor
  · decide
  · decide

/-- NON-VACUITY WITNESS (locality): the SAME slash leaves validator 0's bond
    (5) untouched, witnessing `bm_slash_other` concretely (a slash of 1 does not
    disturb 0). -/
theorem bm_slash_other_example :
    bm_lookup (bm_slash [(0, 5), (1, 7)] 1) 0 = 5 := by
  decide

/- ═══════════════════════════════════════════════════════════════════════════
   §5 — Slash-authorization predicate (the lifetime kernel of P1)
         (ValidatorLifetime.v:7-54)
   ═══════════════════════════════════════════════════════════════════════════

   The load-bearing AUTHORIZATION fact (`main_T9_12_stale_evidence_not_authorized`,
   MainTheorem.v:210 = `stale_evidence_not_authorized`, ValidatorLifetime.v:31):
   evidence authorizes a slash only if it targets the SAME validator AT THE SAME
   epoch. Evidence carried against a STALE epoch (a key that has since rebonded
   into a new lifetime) is NOT authorized — the platform rejects it. This small
   `ValidatorLifetimeId` model ports faithfully offline (no evidence/epoch closure
   machinery), so we port it rather than flag it Rocq-only. -/

/-- An epoch index (Rocq `Epoch := nat`, ValidatorLifetime.v:7). -/
abbrev Epoch : Type := Nat

/-- A validator LIFETIME identity: a validator paired with the epoch it bonded
    in (Rocq `ValidatorLifetimeId`, ValidatorLifetime.v:9-12). Rebonding starts a
    new lifetime (same `vl_validator`, fresh `vl_epoch`). -/
structure ValidatorLifetimeId where
  vl_validator : Validator
  vl_epoch : Epoch
  deriving DecidableEq

/-- Evidence authorizes a target lifetime iff it names the SAME validator AND the
    SAME epoch (Rocq `evidence_authorizes_lifetime`, ValidatorLifetime.v:17-21):
    the validator-equality guard (`validator_eq_dec`) then the epoch equality
    (`Nat.eqb`). Mismatched validator ⇒ false; matching validator but mismatched
    epoch ⇒ false. -/
def evidence_authorizes_lifetime (evidence target : ValidatorLifetimeId) : Bool :=
  if evidence.vl_validator = target.vl_validator then
    evidence.vl_epoch == target.vl_epoch
  else
    false

/-- P1 HEADLINE (authorization) — STALE EVIDENCE IS NOT AUTHORIZED (Rocq
    `stale_evidence_not_authorized`, ValidatorLifetime.v:31-42; lifted to
    `main_T9_12_stale_evidence_not_authorized`, MainTheorem.v:210). For the SAME
    validator `v` but DIFFERENT epochs `e_old ≠ e_new`, authorization is `false`:
    evidence against a key in a prior (now-rebonded / re-epoched) lifetime cannot
    authorize a slash. This is the authorization-soundness companion to the
    BondMap effect taxonomy — together they say the platform only ever slashes a
    key for evidence in that key's CURRENT lifetime, and the slash then zeros
    exactly that key. -/
theorem stale_evidence_not_authorized (v : Validator) (e_old e_new : Epoch)
    (hne : e_old ≠ e_new) :
    evidence_authorizes_lifetime
        ⟨v, e_old⟩ ⟨v, e_new⟩ = false := by
  unfold evidence_authorizes_lifetime
  -- vl_validator matches (v = v), so the guard takes the `then` branch; the
  -- epoch comparison `e_old == e_new` is then `false` because `e_old ≠ e_new`.
  simp only [reduceIte]
  exact beq_eq_false_iff_ne.mpr hne

/-- NON-VACUITY DUAL (authorization) — MATCHING evidence IS authorized (Rocq
    `matching_lifetime_authorized`, ValidatorLifetime.v:44-54): same validator,
    same epoch ⇒ `true`. Without this, `stale_evidence_not_authorized` could be
    vacuously satisfied by a predicate that rejects EVERYTHING; this witnesses
    that `evidence_authorizes_lifetime` genuinely accepts the in-lifetime case,
    so the rejection in the stale case carries real content. -/
theorem matching_lifetime_authorized (v : Validator) (e : Epoch) :
    evidence_authorizes_lifetime ⟨v, e⟩ ⟨v, e⟩ = true := by
  unfold evidence_authorizes_lifetime
  simp

/-- NON-VACUITY WITNESS (authorization, concrete): evidence for validator 3 at
    epoch 9 authorizes the same lifetime, but the SAME validator at the stale
    epoch 8 does NOT. A single concrete pair pinning down both directions. -/
theorem evidence_authorizes_self_example :
    evidence_authorizes_lifetime ⟨3, 9⟩ ⟨3, 9⟩ = true ∧
    evidence_authorizes_lifetime ⟨3, 8⟩ ⟨3, 9⟩ = false := by
  constructor
  · decide
  · decide

end Validator
