(* ===========================================================================
   FtExact.v - The A9 exact-integer fault-tolerance (FT) finalization identity.

   The clique oracle certifies a block finalized when its fault tolerance clears
   the configured threshold theta. The legacy Rust path computes this in `f32`
   (clique_oracle.rs::compute_output_with_cache, :405):

       result = (max_clique_weight * 2.0 - total_stake) / total_stake   (an f32)

   and finalizes when `result >= theta`. Floating point makes the finalization
   frontier NODE-DEPENDENT (rounding differs across builds), which is a fork
   hazard (S1). The A9 fix replaces the ratio test by the EXACT-INTEGER test

       2 * q * den  >=  S * (den + num)

   over `Z` (i128 in Rust), which is bit-for-bit identical on every node.

   ---------------------------------------------------------------------------
   Symbol dictionary
   ---------------------------------------------------------------------------
   Symbol | Meaning                              | Rust (safety/clique_oracle.rs)
   -------+--------------------------------------+------------------------------
   q      | max-clique weight (agreeing stake of | compute_max_clique_weight
          | the heaviest agreeing validator set) |   (:396-403), `max_clique_weight`
   S      | total committee stake                | message_weight_map.values().sum
          |                                      |   (`total_stake`, :386)
   theta  | fault-tolerance threshold, in [0,1]  | the configured FT threshold;
          |   theta = num/den = ppm / 1_000_000  |   parts-per-million (den = 1e6)
   num    | threshold numerator (ppm value)      | numerator of theta
   den    | threshold denominator (= 1_000_000)  | fixed ppm denominator
   -------+--------------------------------------+------------------------------

   Derivation of the two forms (S > 0, den > 0, both order-preserving factors):

       result >= theta
   <=> (2*q - S) / S >= num / den                         (result, :405, = theta)
   <=> (2*q - S) * den >= num * S        (mult by S*den>0; = `ft_ratio_ge`)
   <=> 2*q*den - S*den  >= num*S
   <=> 2*q*den          >= S*den + S*num
   <=> 2*q*den          >= S*(den + num)                   (= `ft_exact_ge`)

   So `ft_ratio_ge` is exactly `(2q - S)/S >= num/den` cleared of its (positive)
   denominators, and `ft_exact_ge` is that regrouped. `ft_exact_iff_ratio` proves
   the two are literally the same test; the exact test is what the node evaluates.
   The strict variants (`_gt`) are the LFB (last-finalized-block) finalizer's
   strict clearance (`> theta`, no boundary finalization).

   The i128 envelope: with `den = 1_000_000` and stake bounded by `2^63` (an i64
   weight sum widened to i128), both sides of the exact test stay far below
   `2^127`, so the i128 arithmetic never overflows (`ft_exact_no_overflow`).

   Zero `Admitted`. No custom `Axiom` / `Parameter`. Stdlib (`ZArith`,`Lia`,
   `Psatz`) only. Every lemma is `lia`/`nia`-closed; verify with
   `Print Assumptions` (all "Closed under the global context").

   NOTE on `ft_exact_mono_q`: monotonicity of the test in `q` needs the
   denominator sign `0 <= den` (with a negative denominator, raising `q` would
   DECREASE `2*q*den` and break monotonicity). `den = 1_000_000 >= 0` always
   holds in the system, so this is a faithful domain side-condition, not a
   weakening of the finalization semantics. It is the ONLY hypothesis added
   beyond the literal A9 statement.
   =========================================================================== *)

From Stdlib Require Import ZArith.
From Stdlib Require Import Lia.
From Stdlib Require Import Psatz.

Open Scope Z_scope.

(* The exact-integer FT test the node evaluates over Z (i128): a block clears the
   threshold when twice the clique weight, scaled by the denominator, dominates
   the total stake scaled by (den + num). Bit-for-bit identical on every node. *)
Definition ft_exact_ge (q S num den : Z) : Prop := 2*q*den >= S*(den+num).

(* The rational test `(2q - S)/S >= num/den` cleared of its positive denominators
   S and den (order-preserving). Semantically equal to `ft_exact_ge`. *)
Definition ft_ratio_ge  (q S num den : Z) : Prop := (2*q - S)*den >= num*S.

(* Strict clearance (the LFB finalizer certifies only STRICTLY above threshold). *)
Definition ft_exact_gt (q S num den : Z) : Prop := 2*q*den >  S*(den+num).
Definition ft_ratio_gt  (q S num den : Z) : Prop := (2*q - S)*den >  num*S.

(* A9 CORE: the exact-integer test and the denominator-cleared ratio test are the
   SAME test, unconditionally (pure ring rearrangement; no sign hypotheses). This
   is why replacing the f32 ratio by the exact integer inequality preserves the
   finalization decision on every input. *)
Lemma ft_exact_iff_ratio :
  forall q S num den, ft_exact_ge q S num den <-> ft_ratio_ge q S num den.
Proof. intros q S num den. unfold ft_exact_ge, ft_ratio_ge. lia. Qed.

(* Strict variant: identical equivalence for the `>` (LFB) finalizer. *)
Lemma ft_exact_iff_ratio_strict :
  forall q S num den, ft_exact_gt q S num den <-> ft_ratio_gt q S num den.
Proof. intros q S num den. unfold ft_exact_gt, ft_ratio_gt. lia. Qed.

(* Monotone in the clique weight q (more agreeing stake never un-finalizes),
   given the non-negative denominator (den = 1e6 >= 0 in the system). *)
Lemma ft_exact_mono_q :
  forall q q' S num den,
    0 <= den ->
    q <= q' -> ft_exact_ge q S num den -> ft_exact_ge q' S num den.
Proof. intros q q' S num den Hden Hqq. unfold ft_exact_ge. nia. Qed.

(* i128 envelope: with the ppm denominator (den = 1e6) and an i64-bounded stake
   (S <= 2^63, q <= S), BOTH sides of the exact test have absolute value < 2^127,
   so the i128 evaluation never overflows.

   NUM RANGE (G2 widening): the runtime's own validated ppm range is the FULL
   symmetric interval `-den <= num <= den` — the on-chain ppm is range-checked to
   [-1e6, 1e6] (token_metadata_check.rs:105) and negative-θ sentinels are legal
   (`ft_decides_exact`, clique_oracle.rs:72-78: e.g. θ = -1 "finalize on any
   majority clique"). The earlier hypothesis `0 <= num <= den` was strictly
   narrower than what the code admits, leaving the sentinel band unmodelled.
   Widening the lower bound to `num >= -den` still keeps `den + num >= 0` (the
   ONLY place the sign of `num` is used), so the envelope holds essentially
   unchanged: |S*(den+num)| <= S*2*den < 2^84 << 2^127 for S <= 2^63, den = 1e6. *)
Lemma ft_exact_no_overflow :
  forall q S num den,
    0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
    Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127.
Proof.
  intros q S num den [Hq0 HqS] [HS0 HS63] [Hn0 Hnd] Hden. subst den.
  (* The widened lower bound num >= -den = -1000000 gives den + num = 1000000 + num
     >= 0, so the RHS product of two non-negatives stays non-negative exactly as in
     the narrower [0, den] case; the upper bound num <= den is unchanged. *)
  assert (Hpos : 0 <= 1000000 + num) by lia.
  assert (Hnn1 : 0 <= 2*q*1000000) by lia.
  assert (Hnn2 : 0 <= S*(1000000+num)) by nia.
  rewrite (Z.abs_eq _ Hnn1). rewrite (Z.abs_eq _ Hnn2).
  split; nia.
Qed.

Close Scope Z_scope.
