(* ═══════════════════════════════════════════════════════════════════════════
   Similarity.v — the exact similarity decisions of the VectorDB family (P6)

   This module models the exact numeric core of the VectorDB spaces
   (rholang/src/rust/interpreter/tensor/exact.rs). Every consensus decision
   of a VectorDB space is exact (decision 54): a selection never takes a
   square root and never rounds. The module proves that these exact
   decisions are the decisions of the real-valued metrics, the laws of the
   metrics, and the determinism of the selection order.

   Why an ordered ring. The cosine of two rational vectors is
   cos = (q·e) / (‖q‖·‖e‖), and ‖q‖·‖e‖ is usually irrational. The Rust code
   decides cos ≥ t without the root: it compares squares and signs. A proof
   over the rationals alone would say nothing about an irrational cosine. So
   Part 1 proves the squared forms in every ordered commutative ring with a
   decidable order. The real numbers are such a ring, so the theorems cover
   the real cosine. The proofs never divide, so a field is not needed.
   Part 2 shows that an embedding of ordered rings keeps the decisions, so
   the decision that the Rust code makes in the rationals is the decision in
   the real numbers. Part 5 discharges every hypothesis of Parts 1, 2 and 4
   with the canonical rationals Qc (and the integers Z), which model the
   reduced rationals of the Rust code (num_rational::BigRational).

   No IEEE-754 model. The delivered values (scores=true) are RN_53 of the
   exact values. Rocq does not model the rounding. The Rust tests check it
   against IEEE division and IEEE square root (both correctly rounded), and
   the Sage leg (MPFR) and the Python fractions receipt check it against
   test-vectors/numeric-reference-v1.json.

   Definitions
     rle x y                 x < y or x = y
     cosine_within u t N     the threshold decision of cosine: u = q·e,
                             N = ‖q‖²·‖e‖² > 0, the threshold t
     cosine_le u1 n1 u2 n2   the order of two cosine scores: u_i = q·e_i,
                             n_i = ‖e_i‖² > 0 (‖q‖² cancels)
     euclidean_within d2 r   the radius decision of euclidean: d2 = ‖q − e‖²
     dot, dist2, manhattan   the kernels over vectors of one length
     rabs, ssq               |x| and the signed square x·|x|
     ratio_le a1 b1 a2 b2    a1·b2 ≤ a2·b1: the order of a1/b1 and a2/b2
     hamming, inter_count,   the counts of the binary metrics
     union_count
     precedes                the selection order of design v3, section 4.4:
                             the better score, then the lower kappa (the
                             DR-75 key of decision 41), then the lower index
                             in the view. The Rust candidate carries one
                             rank, its position in the canonical order of
                             DR-75, which orders by kappa and then by the
                             index, so tie_before is the order of the ranks

   Headline theorems (each one has a Print Assumptions line at the end)
     S1  cosine_squared_form_iff        t·s ≤ u ⇔ cosine_within u t (s·s)
     S2  cosine_order_squared_form      u1·s2 ≤ u2·s1 ⇔ cosine_le u1 n1 u2 n2
     S3  euclidean_radius_squared_form  d ≤ r ⇔ euclidean_within (d·d) r
     S4  threshold_monotone             cosine, dot and jaccard thresholds
     S5  radius_monotone                euclidean, manhattan and hamming radii
     S6  dot_laws                       symmetry and permutation invariance
     S7  distance_laws                  dist2 and manhattan are metrics
     S8  cauchy_schwarz                 (a·b)² ≤ ‖a‖²·‖b‖²
     S9  cosine_bounds                  every record passes −1, none passes
                                        a threshold above 1
     S10 score_orders_total             each order of the six metrics is a
                                        total preorder
     S11 squared_forms_embed            ordered ring embeddings keep S1-S3
     S12 binary_laws                    hamming is a metric, 0 ≤ ∧ ≤ ∨
     S13 selection_total_order          precedes is a strict total order
     S14 topk_deterministic             sorted arrangements are unique
     S15 list_selection_deterministic   list mode is a function of the
                                        multiset of records
     S16 kappa_dropped_not_unique       negative control: without kappa, two
                                        sorted arrangements differ
     S17 Qc_ordered_ring                Qc satisfies the hypotheses of Part 1
     S18 similarity_headlines_Qc        S1, S2, S3, S8 and S9 hold in Qc
     S19 squared_forms_embed_Z_Qc       S11 holds for the embedding Z → Qc
     S20 selection_Qc                   S13 and S14 hold for rational scores

   Sources
   - Design: .reified-rspaces-work/p6/design-v3.md, sections 4.1 (the exact
     numeric core), 4.4 (the selection semantics) and 5 (Similarity.v).
   - Plan: decisions 41 (the DR-75 tie-break), 54 (exact arithmetic for
     every consensus decision), 60 (the VectorDB tie-break by kappa), 62
     and 65 (tensor logic waits for F1r3lang), and 66 (the four choices of
     design v3: hamming and jaccard on binary vectors, a zero query under
     cosine fails, a zero record is ineligible, values beyond f64 fail).
   - Tracker: rrs-fv-similarity and rrs-vdb-exact-numeric-core.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code (rholang/src/rust/interpreter)
   ─────────────────────────────────────────────────────────────────────────
   cosine_within             tensor/exact.rs:532-550 (the cosine arm of
                             within: by the signs, and otherwise by the
                             squares, with N = ‖q‖²·‖e‖² from Query::new at
                             tensor/exact.rs:151-170)
   cosine_le                 tensor/exact.rs:480-498 (the cosine arm of
                             better, COSINE-BETTER: a pair is cosine_le
                             exactly when better is not Greater)
   euclidean_within          tensor/exact.rs:557-564 (the euclidean arm of
                             within: a negative radius admits nothing)
   dot                       tensor/exact.rs:265-291 (dot, with the exact
                             DYADIC-SUM of tensor/exact.rs:175-228)
   dist2                     tensor/exact.rs:294-319 (distance2)
   manhattan, rabs           tensor/exact.rs:322-348 (manhattan)
   threshold_monotone        tensor/exact.rs:551-556 (the dot and jaccard
                             arms of within)
   radius_monotone           tensor/exact.rs:565-571 (the manhattan and
                             hamming arms of within)
   ratio_le                  tensor/exact.rs:507-518 (the jaccard arm of
                             better: inter·union' against inter'·union)
   hamming, inter_count,     tensor/exact.rs:352-361, 378-394, 402-405 and
   union_count               409-425 (bit_of_dyadic, bits, hamming and
                             jaccard)
   score_orders_total        tensor/exact.rs:478-521 (better), with the
                             undefined scores of tensor/exact.rs:429-462
   precedes, insertion_sort  tensor/exact.rs:605-610, 616-629 and 635-642
                             (Candidate, selection_order: the better score,
                             then the lower rank in the canonical order of
                             DR-75, and rank: the metered heap sort of the
                             space order)
   Qc                        num_rational::BigRational, the Exact type of
                             tensor/num.rs
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Bool Arith Lia PeanoNat.
From Stdlib Require Import ZArith QArith Qreduction Qcanon.
From Stdlib Require Import Ring_theory Ring Setoid.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
Import ListNotations.

(* ═══════════════════════════════════════════════════════════════════════════
   Part 1. The squared forms in an ordered commutative ring
   ═══════════════════════════════════════════════════════════════════════════ *)

Declare Scope sim_scope.
Delimit Scope sim_scope with sim.

Section OrderedRing.

  Variable R : Type.
  Variables (rzero rone : R).
  Variables (radd rmul rsub : R -> R -> R).
  Variable ropp : R -> R.
  Variable rlt : R -> R -> Prop.

  (* The laws of an ordered commutative ring with a decidable order. The real
     numbers satisfy them, and so do the rationals (Part 6 discharges them for
     Qc, the canonical rationals). *)
  Hypothesis R_ring : ring_theory rzero rone radd rmul rsub ropp (@eq R).
  Hypothesis rlt_dec : forall x y, {rlt x y} + {~ rlt x y}.
  Hypothesis rlt_irrefl : forall x, ~ rlt x x.
  Hypothesis rlt_trans : forall x y z, rlt x y -> rlt y z -> rlt x z.
  Hypothesis rlt_total : forall x y, rlt x y \/ x = y \/ rlt y x.
  Hypothesis rlt_add_r : forall x y z, rlt x y -> rlt (radd x z) (radd y z).
  Hypothesis rlt_mul_pos : forall x y, rlt rzero x -> rlt rzero y -> rlt rzero (rmul x y).
  Hypothesis rlt_zero_one : rlt rzero rone.

  Add Ring R_ring_instance : R_ring.

  Local Infix "+" := radd : sim_scope.
  Local Infix "*" := rmul : sim_scope.
  Local Infix "-" := rsub : sim_scope.
  Local Notation "- x" := (ropp x) : sim_scope.
  Local Infix "<" := rlt : sim_scope.
  Local Open Scope sim_scope.

  Definition rle (x y : R) : Prop := x < y \/ x = y.
  Local Infix "<=" := rle : sim_scope.

  (* ── The order ─────────────────────────────────────────────────────────── *)

  Lemma rle_refl : forall x, x <= x.
  Proof. intro x. right. reflexivity. Qed.

  Lemma rlt_asym : forall x y, x < y -> ~ y < x.
  Proof. intros x y Hxy Hyx. exact (rlt_irrefl x (rlt_trans _ _ _ Hxy Hyx)). Qed.

  Lemma rle_trans : forall x y z, x <= y -> y <= z -> x <= z.
  Proof.
    intros x y z [Hxy | <-] [Hyz | <-].
    - left. exact (rlt_trans _ _ _ Hxy Hyz).
    - left. exact Hxy.
    - left. exact Hyz.
    - right. reflexivity.
  Qed.

  Lemma rlt_rle_trans : forall x y z, x < y -> y <= z -> x < z.
  Proof. intros x y z Hxy [Hyz | <-]; [exact (rlt_trans _ _ _ Hxy Hyz) | exact Hxy]. Qed.

  Lemma rle_rlt_trans : forall x y z, x <= y -> y < z -> x < z.
  Proof. intros x y z [Hxy | <-] Hyz; [exact (rlt_trans _ _ _ Hxy Hyz) | exact Hyz]. Qed.

  Lemma rle_antisym : forall x y, x <= y -> y <= x -> x = y.
  Proof.
    intros x y [Hxy | Hxy] [Hyx | Hyx]; try assumption.
    - exfalso. exact (rlt_asym _ _ Hxy Hyx).
    - symmetry. exact Hyx.
  Qed.

  Lemma rle_total : forall x y, x <= y \/ y <= x.
  Proof.
    intros x y. destruct (rlt_total x y) as [H | [H | H]].
    - left. left. exact H.
    - left. right. exact H.
    - right. left. exact H.
  Qed.

  Lemma rlt_not_rle : forall x y, x < y -> ~ y <= x.
  Proof. intros x y Hxy [Hyx | Hyx]; [exact (rlt_asym _ _ Hxy Hyx) | subst; exact (rlt_irrefl _ Hxy)]. Qed.

  Lemma not_rlt_rle : forall x y, ~ x < y -> y <= x.
  Proof.
    intros x y H. destruct (rlt_total x y) as [Hxy | [Hxy | Hxy]].
    - contradiction.
    - right. symmetry. exact Hxy.
    - left. exact Hxy.
  Qed.

  Lemma not_rle_rlt : forall x y, ~ x <= y -> y < x.
  Proof.
    intros x y H. destruct (rlt_total x y) as [Hxy | [Hxy | Hxy]].
    - exfalso. apply H. left. exact Hxy.
    - exfalso. apply H. right. exact Hxy.
    - exact Hxy.
  Qed.

  (* ── Addition, negation and multiplication ────────────────────────────── *)

  Lemma rlt_add_l : forall x y z, x < y -> z + x < z + y.
  Proof.
    intros x y z H. pose proof (rlt_add_r x y z H) as H'.
    replace (z + x) with (x + z) by ring. replace (z + y) with (y + z) by ring. exact H'.
  Qed.

  Lemma rlt_sub_pos : forall x y, x < y <-> rzero < y - x.
  Proof.
    intros x y. split; intro H.
    - pose proof (rlt_add_r x y (- x) H) as H'.
      replace (x + - x) with rzero in H' by ring.
      replace (y + - x) with (y - x) in H' by ring. exact H'.
    - pose proof (rlt_add_r rzero (y - x) x H) as H'.
      replace (rzero + x) with x in H' by ring.
      replace (y - x + x) with y in H' by ring. exact H'.
  Qed.

  Lemma rle_sub_nonneg : forall x y, x <= y <-> rzero <= y - x.
  Proof.
    intros x y. split; intros [H | H].
    - left. exact (proj1 (rlt_sub_pos x y) H).
    - right. subst. ring.
    - left. exact (proj2 (rlt_sub_pos x y) H).
    - right. assert (Hy : y = (y - x) + x) by ring. rewrite <- H in Hy.
      rewrite Hy. ring.
  Qed.

  Lemma rlt_opp : forall x y, x < y <-> - y < - x.
  Proof.
    intros x y. rewrite (rlt_sub_pos x y), (rlt_sub_pos (- y) (- x)).
    replace (- x - - y) with (y - x) by ring. reflexivity.
  Qed.

  Lemma rle_opp : forall x y, x <= y <-> - y <= - x.
  Proof.
    intros x y. split; intros [H | H].
    - left. exact (proj1 (rlt_opp x y) H).
    - right. subst. reflexivity.
    - left. exact (proj2 (rlt_opp x y) H).
    - right. replace x with (- - x) by ring. replace y with (- - y) by ring.
      rewrite H. reflexivity.
  Qed.

  Lemma rle_add : forall x y z w, x <= y -> z <= w -> x + z <= y + w.
  Proof.
    intros x y z w Hxy Hzw. apply (proj2 (rle_sub_nonneg (x + z) (y + w))).
    apply (proj1 (rle_sub_nonneg x y)) in Hxy. apply (proj1 (rle_sub_nonneg z w)) in Hzw.
    replace (y + w - (x + z)) with ((y - x) + (w - z)) by ring.
    destruct Hxy as [Hxy | Hxy]; destruct Hzw as [Hzw | Hzw].
    - left. pose proof (rlt_add_r rzero (y - x) (w - z) Hxy) as H.
      replace (rzero + (w - z)) with (w - z) in H by ring. exact (rlt_trans _ _ _ Hzw H).
    - left. rewrite <- Hzw. replace (y - x + rzero) with (y - x) by ring. exact Hxy.
    - left. rewrite <- Hxy. replace (rzero + (w - z)) with (w - z) by ring. exact Hzw.
    - right. rewrite <- Hxy, <- Hzw. ring.
  Qed.

  Lemma rle_nonneg_add : forall x y, rzero <= x -> rzero <= y -> rzero <= x + y.
  Proof.
    intros x y Hx Hy. pose proof (rle_add _ _ _ _ Hx Hy) as H.
    replace (rzero + rzero) with rzero in H by ring. exact H.
  Qed.

  Lemma rlt_mul_l : forall c x y, rzero < c -> x < y -> c * x < c * y.
  Proof.
    intros c x y Hc Hxy. apply (proj2 (rlt_sub_pos (c * x) (c * y))).
    apply (proj1 (rlt_sub_pos x y)) in Hxy.
    replace (c * y - c * x) with (c * (y - x)) by ring. apply rlt_mul_pos; assumption.
  Qed.

  Lemma rle_mul_l : forall c x y, rzero <= c -> x <= y -> c * x <= c * y.
  Proof.
    intros c x y [Hc | <-] [Hxy | <-].
    - left. apply rlt_mul_l; assumption.
    - right. reflexivity.
    - right. ring.
    - right. reflexivity.
  Qed.

  Lemma rle_mul_pos_iff : forall c x y, rzero < c -> (c * x <= c * y <-> x <= y).
  Proof.
    intros c x y Hc. split; intro H.
    - apply not_rlt_rle. intro Hyx. apply (rlt_not_rle _ _ (rlt_mul_l c y x Hc Hyx)). exact H.
    - apply rle_mul_l; [left; exact Hc | exact H].
  Qed.

  Lemma rle_mul_r_pos_iff : forall c x y, rzero < c -> (x * c <= y * c <-> x <= y).
  Proof.
    intros c x y Hc. replace (x * c) with (c * x) by ring. replace (y * c) with (c * y) by ring.
    apply rle_mul_pos_iff. exact Hc.
  Qed.

  Lemma mul_nonneg : forall x y, rzero <= x -> rzero <= y -> rzero <= x * y.
  Proof.
    intros x y [Hx | <-] [Hy | <-].
    - left. apply rlt_mul_pos; assumption.
    - right. ring.
    - right. ring.
    - right. ring.
  Qed.

  Lemma opp_pos : forall x, x < rzero <-> rzero < - x.
  Proof.
    intro x. rewrite (rlt_opp x rzero). replace (- rzero) with rzero by ring. reflexivity.
  Qed.

  Lemma mul_neg_pos : forall x y, x < rzero -> rzero < y -> x * y < rzero.
  Proof.
    intros x y Hx Hy. apply (proj2 (opp_pos (x * y))). apply (proj1 (opp_pos x)) in Hx.
    replace (- (x * y)) with ((- x) * y) by ring. apply rlt_mul_pos; assumption.
  Qed.

  Lemma mul_nonneg_pos : forall x y, rzero <= x -> rzero < y -> rzero <= x * y.
  Proof. intros x y Hx Hy. apply mul_nonneg; [exact Hx | left; exact Hy]. Qed.

  Lemma square_nonneg : forall x, rzero <= x * x.
  Proof.
    intro x. destruct (rlt_total x rzero) as [H | [H | H]].
    - left. apply (proj1 (opp_pos x)) in H. replace (x * x) with ((- x) * (- x)) by ring.
      apply rlt_mul_pos; assumption.
    - right. subst. ring.
    - left. apply rlt_mul_pos; assumption.
  Qed.

  Lemma square_pos : forall x, x <> rzero -> rzero < x * x.
  Proof.
    intros x Hx. destruct (rlt_total x rzero) as [H | [H | H]].
    - apply (proj1 (opp_pos x)) in H. replace (x * x) with ((- x) * (- x)) by ring.
      apply rlt_mul_pos; assumption.
    - contradiction.
    - apply rlt_mul_pos; assumption.
  Qed.

  Lemma square_zero : forall x, x * x = rzero -> x = rzero.
  Proof.
    intros x H. destruct (rlt_total x rzero) as [Hx | [Hx | Hx]].
    - exfalso. apply (proj1 (opp_pos x)) in Hx. pose proof (rlt_mul_pos _ _ Hx Hx) as Hp.
      replace (- x * - x) with (x * x) in Hp by ring. rewrite H in Hp.
      exact (rlt_irrefl _ Hp).
    - exact Hx.
    - exfalso. pose proof (rlt_mul_pos _ _ Hx Hx) as Hp. rewrite H in Hp.
      exact (rlt_irrefl _ Hp).
  Qed.


  (* ── Squares ──────────────────────────────────────────────────────────── *)

  Lemma square_lt : forall x y, rzero <= x -> x < y -> x * x < y * y.
  Proof.
    intros x y Hx Hxy. apply (proj2 (rlt_sub_pos (x * x) (y * y))).
    replace (y * y - x * x) with ((y - x) * (y + x)) by ring.
    apply rlt_mul_pos.
    - exact (proj1 (rlt_sub_pos x y) Hxy).
    - pose proof (rle_rlt_trans _ _ _ Hx Hxy) as Hy.
      assert (Hyx : y <= y + x).
      { pose proof (rle_add y y rzero x (rle_refl y) Hx) as H.
        replace (y + rzero) with y in H by ring. exact H. }
      exact (rlt_rle_trans _ _ _ Hy Hyx).
  Qed.

  Lemma square_le_iff : forall x y, rzero <= x -> rzero <= y -> (x <= y <-> x * x <= y * y).
  Proof.
    intros x y Hx Hy. split.
    - intros [Hxy | <-].
      + left. apply square_lt; assumption.
      + right. reflexivity.
    - intro H. apply not_rlt_rle. intro Hyx. apply (rlt_not_rle _ _ (square_lt y x Hy Hyx)).
      exact H.
  Qed.

  Lemma neg_nonneg_absurd : forall x, x < rzero -> rzero <= x -> False.
  Proof. intros x Hx Hx'. exact (rlt_not_rle _ _ Hx Hx'). Qed.

  (* ── The squared form of the cosine threshold ────────────────────────── *)

  (* cosine_within u t N is the decision of the Rust function within for a
     cosine score (tensor/exact.rs): u = q·e, N = ‖q‖²·‖e‖² > 0, and t is the
     threshold. It uses no root and no division. *)
  Definition cosine_within (u t N : R) : Prop :=
    (rzero <= u /\ t < rzero) \/
    (rzero <= u /\ rzero <= t /\ t * t * N <= u * u) \/
    (u < rzero /\ t < rzero /\ u * u <= t * t * N).

  (* Headline 1. For s = ‖q‖·‖e‖ > 0 (so s·s = N), the real comparison
     cos = u/s ≥ t, that is t·s ≤ u, holds exactly when cosine_within holds.
     The theorem holds in every ordered commutative ring, the real numbers
     included, so it covers the real cosine although s is irrational. *)
  Theorem cosine_squared_form_iff :
    forall u t s, rzero < s -> (t * s <= u <-> cosine_within u t (s * s)).
  Proof.
    intros u t s Hs. unfold cosine_within.
    destruct (rlt_dec u rzero) as [Hu | Hu]; destruct (rlt_dec t rzero) as [Ht | Ht].
    - (* u < 0, t < 0: both sides negative, compare the squares of the opposites. *)
      pose proof (mul_neg_pos _ _ Ht Hs) as Hts.
      rewrite (rle_opp (t * s) u).
      rewrite (square_le_iff (- u) (- (t * s))).
      2: { left. exact (proj1 (opp_pos u) Hu). }
      2: { left. exact (proj1 (opp_pos (t * s)) Hts). }
      replace (- u * - u) with (u * u) by ring.
      replace (- (t * s) * - (t * s)) with (t * t * (s * s)) by ring.
      split.
      + intro H. right. right. split; [exact Hu | split; [exact Ht | exact H]].
      + intros [[Hu' _] | [[Hu' _] | [_ [_ H]]]].
        * exfalso. exact (neg_nonneg_absurd _ Hu Hu').
        * exfalso. exact (neg_nonneg_absurd _ Hu Hu').
        * exact H.
    - (* u < 0 ≤ t: t·s ≥ 0 > u, so both sides are false. *)
      apply not_rlt_rle in Ht.
      assert (Hts : rzero <= t * s) by (apply mul_nonneg_pos; assumption).
      split.
      + intro H. exfalso. exact (neg_nonneg_absurd _ Hu (rle_trans _ _ _ Hts H)).
      + intros [[Hu' _] | [[Hu' _] | [_ [Ht' _]]]].
        * exfalso. exact (neg_nonneg_absurd _ Hu Hu').
        * exfalso. exact (neg_nonneg_absurd _ Hu Hu').
        * exfalso. exact (neg_nonneg_absurd _ Ht' Ht).
    - (* t < 0 ≤ u: t·s < 0 ≤ u, so both sides are true. *)
      apply not_rlt_rle in Hu.
      split.
      + intro H. left. split; assumption.
      + intro H. left. exact (rlt_rle_trans _ _ _ (mul_neg_pos _ _ Ht Hs) Hu).
    - (* 0 ≤ u, 0 ≤ t: compare the squares. *)
      apply not_rlt_rle in Hu. apply not_rlt_rle in Ht.
      assert (Hts : rzero <= t * s) by (apply mul_nonneg_pos; assumption).
      rewrite (square_le_iff (t * s) u Hts Hu).
      replace (t * s * (t * s)) with (t * t * (s * s)) by ring.
      split.
      + intro H. right. left. split; [exact Hu | split; [exact Ht | exact H]].
      + intros [[_ Ht'] | [[_ [_ H]] | [Hu' _]]].
        * exfalso. exact (neg_nonneg_absurd _ Ht' Ht).
        * exact H.
        * exfalso. exact (neg_nonneg_absurd _ Hu' Hu).
  Qed.

  (* ── The squared form of the cosine order ────────────────────────────── *)

  (* cosine_le u1 n1 u2 n2 is the order of the Rust function better on two
     cosine scores (tensor/exact.rs, COSINE-BETTER): u_i = q·e_i and
     n_i = ‖e_i‖² > 0. The norm of the query is common, so it cancels. *)
  Definition cosine_le (u1 n1 u2 n2 : R) : Prop :=
    (u1 < rzero /\ rzero <= u2) \/
    (rzero <= u1 /\ rzero <= u2 /\ u1 * u1 * n2 <= u2 * u2 * n1) \/
    (u1 < rzero /\ u2 < rzero /\ u2 * u2 * n1 <= u1 * u1 * n2).

  (* Headline 2. With nq = ‖q‖² > 0 and s_i = ‖q‖·‖e_i‖ > 0 (so
     s_i·s_i = nq·n_i), the real comparison cos_1 = u1/s1 ≤ cos_2 = u2/s2, that
     is u1·s2 ≤ u2·s1, holds exactly when cosine_le holds. *)
  Theorem cosine_order_squared_form :
    forall nq u1 u2 s1 s2 n1 n2,
      rzero < nq -> rzero < s1 -> rzero < s2 ->
      s1 * s1 = nq * n1 -> s2 * s2 = nq * n2 ->
      (u1 * s2 <= u2 * s1 <-> cosine_le u1 n1 u2 n2).
  Proof.
    intros nq u1 u2 s1 s2 n1 n2 Hq Hs1 Hs2 H1 H2. unfold cosine_le.
    assert (Hsq : forall a b, a * s2 * (a * s2) <= b * s1 * (b * s1) <->
                              a * a * n2 <= b * b * n1).
    { intros a b.
      replace (a * s2 * (a * s2)) with (nq * (a * a * n2))
        by (transitivity (a * a * (s2 * s2)); [rewrite H2; ring | ring]).
      replace (b * s1 * (b * s1)) with (nq * (b * b * n1))
        by (transitivity (b * b * (s1 * s1)); [rewrite H1; ring | ring]).
      apply rle_mul_pos_iff. exact Hq. }
    destruct (rlt_dec u1 rzero) as [Hu1 | Hu1]; destruct (rlt_dec u2 rzero) as [Hu2 | Hu2].
    - (* both negative *)
      pose proof (mul_neg_pos _ _ Hu1 Hs2) as Ha.
      pose proof (mul_neg_pos _ _ Hu2 Hs1) as Hb.
      rewrite (rle_opp (u1 * s2) (u2 * s1)).
      rewrite (square_le_iff (- (u2 * s1)) (- (u1 * s2))).
      2: { left. exact (proj1 (opp_pos _) Hb). }
      2: { left. exact (proj1 (opp_pos _) Ha). }
      replace (- (u2 * s1) * - (u2 * s1)) with (u2 * s1 * (u2 * s1)) by ring.
      replace (- (u1 * s2) * - (u1 * s2)) with (u1 * s2 * (u1 * s2)) by ring.
      assert (Hsq' : u2 * s1 * (u2 * s1) <= u1 * s2 * (u1 * s2) <->
                     u2 * u2 * n1 <= u1 * u1 * n2).
      { replace (u2 * s1 * (u2 * s1)) with (nq * (u2 * u2 * n1))
          by (transitivity (u2 * u2 * (s1 * s1)); [rewrite H1; ring | ring]).
        replace (u1 * s2 * (u1 * s2)) with (nq * (u1 * u1 * n2))
          by (transitivity (u1 * u1 * (s2 * s2)); [rewrite H2; ring | ring]).
        apply rle_mul_pos_iff. exact Hq. }
      rewrite Hsq'. split.
      + intro H. right. right. split; [exact Hu1 | split; [exact Hu2 | exact H]].
      + intros [[_ Hu2'] | [[Hu1' _] | [_ [_ H]]]].
        * exfalso. exact (neg_nonneg_absurd _ Hu2 Hu2').
        * exfalso. exact (neg_nonneg_absurd _ Hu1 Hu1').
        * exact H.
    - (* u1 < 0 ≤ u2 *)
      apply not_rlt_rle in Hu2. split.
      + intro H. left. split; assumption.
      + intro H. left. apply (rlt_rle_trans _ rzero).
        * exact (mul_neg_pos _ _ Hu1 Hs2).
        * apply mul_nonneg_pos; assumption.
    - (* u2 < 0 ≤ u1 *)
      apply not_rlt_rle in Hu1.
      assert (Ha : rzero <= u1 * s2) by (apply mul_nonneg_pos; assumption).
      pose proof (mul_neg_pos _ _ Hu2 Hs1) as Hb.
      split.
      + intro H. exfalso. exact (neg_nonneg_absurd _ Hb (rle_trans _ _ _ Ha H)).
      + intros [[Hu1' _] | [[_ [Hu2' _]] | [Hu1' _]]].
        * exfalso. exact (neg_nonneg_absurd _ Hu1' Hu1).
        * exfalso. exact (neg_nonneg_absurd _ Hu2 Hu2').
        * exfalso. exact (neg_nonneg_absurd _ Hu1' Hu1).
    - (* both non-negative *)
      apply not_rlt_rle in Hu1. apply not_rlt_rle in Hu2.
      assert (Ha : rzero <= u1 * s2) by (apply mul_nonneg_pos; assumption).
      assert (Hb : rzero <= u2 * s1) by (apply mul_nonneg_pos; assumption).
      rewrite (square_le_iff _ _ Ha Hb), Hsq. split.
      + intro H. right. left. split; [exact Hu1 | split; [exact Hu2 | exact H]].
      + intros [[Hu1' _] | [[_ [_ H]] | [Hu1' _]]].
        * exfalso. exact (neg_nonneg_absurd _ Hu1' Hu1).
        * exact H.
        * exfalso. exact (neg_nonneg_absurd _ Hu1' Hu1).
  Qed.

  (* ── The squared form of the euclidean radius ────────────────────────── *)

  (* euclidean_within d2 r is the decision of within for a euclidean score:
     d2 = ‖q − e‖² and r is the radius. A negative radius admits nothing. *)
  Definition euclidean_within (d2 r : R) : Prop := rzero <= r /\ d2 <= r * r.

  (* Headline 3. For the real distance d ≥ 0 (so d·d = d2), d ≤ r holds
     exactly when euclidean_within holds. *)
  Theorem euclidean_radius_squared_form :
    forall d r, rzero <= d -> (d <= r <-> euclidean_within (d * d) r).
  Proof.
    intros d r Hd. unfold euclidean_within. split.
    - intro H. pose proof (rle_trans _ _ _ Hd H) as Hr.
      split; [exact Hr | exact (proj1 (square_le_iff d r Hd Hr) H)].
    - intros [Hr H]. exact (proj2 (square_le_iff d r Hd Hr) H).
  Qed.

  (* ── Monotonicity of the bounds ─────────────────────────────────────── *)

  Lemma rle_mul_r_nonneg : forall c x y, rzero <= c -> x <= y -> x * c <= y * c.
  Proof.
    intros c x y Hc Hxy. replace (x * c) with (c * x) by ring. replace (y * c) with (c * y) by ring.
    apply rle_mul_l; assumption.
  Qed.

  (* Headline 4. A lower threshold admits every record that a higher threshold
     admits: cosine (for N > 0), dot (the score itself) and jaccard (with the
     count of the union n ≥ 0, a record passes when t·n ≤ i). *)
  Theorem threshold_monotone :
    forall t1 t2, t1 <= t2 ->
      (forall u N, rzero < N -> cosine_within u t2 N -> cosine_within u t1 N) /\
      (forall v, t2 <= v -> t1 <= v) /\
      (forall i n, rzero <= n -> t2 * n <= i -> t1 * n <= i).
  Proof.
    intros t1 t2 Ht. split; [| split].
    - intros u N HN. unfold cosine_within.
      intros [[Hu Ht2] | [[Hu [Ht2 H]] | [Hu [Ht2 H]]]].
      + left. split; [exact Hu | exact (rle_rlt_trans _ _ _ Ht Ht2)].
      + destruct (rlt_dec t1 rzero) as [Ht1 | Ht1].
        * left. split; assumption.
        * apply not_rlt_rle in Ht1. right. left.
          split; [exact Hu | split; [exact Ht1 |]].
          apply (rle_trans _ (t2 * t2 * N)); [| exact H].
          apply rle_mul_r_nonneg; [left; exact HN |].
          exact (proj1 (square_le_iff t1 t2 Ht1 Ht2) Ht).
      + right. right. pose proof (rle_rlt_trans _ _ _ Ht Ht2) as Ht1.
        split; [exact Hu | split; [exact Ht1 |]].
        apply (rle_trans _ (t2 * t2 * N)); [exact H |].
        apply rle_mul_r_nonneg; [left; exact HN |].
        replace (t2 * t2) with (- t2 * - t2) by ring.
        replace (t1 * t1) with (- t1 * - t1) by ring.
        apply (proj1 (square_le_iff (- t2) (- t1)
                        (or_introl (proj1 (opp_pos t2) Ht2))
                        (or_introl (proj1 (opp_pos t1) Ht1)))).
        exact (proj1 (rle_opp t1 t2) Ht).
    - intros v H. exact (rle_trans _ _ _ Ht H).
    - intros i n Hn H. apply (rle_trans _ (t2 * n)); [| exact H].
      apply rle_mul_r_nonneg; assumption.
  Qed.

  (* Headline 5. A larger radius admits every record that a smaller radius
     admits: euclidean (squared form), manhattan and hamming (the distance
     itself). *)
  Theorem radius_monotone :
    forall r1 r2, r1 <= r2 ->
      (forall d2, euclidean_within d2 r1 -> euclidean_within d2 r2) /\
      (forall d, d <= r1 -> d <= r2).
  Proof.
    intros r1 r2 Hr. split.
    - intros d2 [Hr1 H]. unfold euclidean_within.
      pose proof (rle_trans _ _ _ Hr1 Hr) as Hr2. split; [exact Hr2 |].
      apply (rle_trans _ (r1 * r1)); [exact H |].
      exact (proj1 (square_le_iff r1 r2 Hr1 Hr2) Hr).
    - intros d H. exact (rle_trans _ _ _ H Hr).
  Qed.

  (* ── The kernels ────────────────────────────────────────────────────── *)

  (* The kernels of tensor/exact.rs over vectors of one length: the dot
     product, the squared euclidean distance and the manhattan distance. *)
  Fixpoint dot (a b : list R) : R :=
    match a, b with
    | x :: a', y :: b' => x * y + dot a' b'
    | _, _ => rzero
    end.

  Fixpoint dist2 (a b : list R) : R :=
    match a, b with
    | x :: a', y :: b' => (x - y) * (x - y) + dist2 a' b'
    | _, _ => rzero
    end.

  Definition rabs (x : R) : R := if rlt_dec x rzero then - x else x.

  Fixpoint manhattan (a b : list R) : R :=
    match a, b with
    | x :: a', y :: b' => rabs (x - y) + manhattan a' b'
    | _, _ => rzero
    end.

  (* The sum of the products of a list of coordinate pairs. *)
  Definition pair_sum (ps : list (R * R)) : R :=
    fold_right (fun p acc => fst p * snd p + acc) rzero ps.

  Lemma dot_pair_sum : forall a b, dot a b = pair_sum (combine a b).
  Proof.
    induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
    rewrite IH. reflexivity.
  Qed.

  Lemma pair_sum_perm : forall ps ps', Permutation ps ps' -> pair_sum ps = pair_sum ps'.
  Proof.
    intros ps ps' H. induction H; simpl.
    - reflexivity.
    - unfold pair_sum in *. simpl. rewrite IHPermutation. reflexivity.
    - unfold pair_sum. simpl. ring.
    - transitivity (pair_sum l'); assumption.
  Qed.

  (* Headline 6. The dot product is symmetric, and its exact sum does not
     depend on the order of the coordinates (an f64 sum does). *)
  Theorem dot_laws :
    (forall a b, dot a b = dot b a) /\
    (forall a b a' b', Permutation (combine a b) (combine a' b') -> dot a b = dot a' b').
  Proof.
    split.
    - induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
      rewrite IH. ring.
    - intros a b a' b' H. rewrite !dot_pair_sum. apply pair_sum_perm. exact H.
  Qed.

  Lemma rabs_cases : forall x, (rabs x = x /\ rzero <= x) \/ (rabs x = - x /\ x < rzero).
  Proof.
    intro x. unfold rabs. destruct (rlt_dec x rzero) as [H | H].
    - right. split; [reflexivity | exact H].
    - left. split; [reflexivity | exact (not_rlt_rle _ _ H)].
  Qed.

  Lemma rabs_nonneg : forall x, rzero <= rabs x.
  Proof.
    intro x. destruct (rabs_cases x) as [[-> H] | [-> H]].
    - exact H.
    - left. exact (proj1 (opp_pos x) H).
  Qed.

  Lemma rabs_opp : forall x, rabs (- x) = rabs x.
  Proof.
    intro x. destruct (rabs_cases x) as [[Hx Hx'] | [Hx Hx']];
      destruct (rabs_cases (- x)) as [[Hy Hy'] | [Hy Hy']]; rewrite Hx, Hy.
    - (* 0 <= x and 0 <= - x, so x = 0. *)
      assert (Hneg : - x <= rzero).
      { pose proof (proj1 (rle_opp rzero x) Hx') as H.
        replace (- rzero) with rzero in H by ring. exact H. }
      pose proof (rle_antisym _ _ Hneg Hy') as H0.
      assert (Hx0 : x = rzero) by (replace x with (- - x) by ring; rewrite H0; ring).
      rewrite Hx0. ring.
    - ring.
    - ring.
    - exfalso. apply (proj1 (opp_pos x)) in Hx'. exact (rlt_asym _ _ Hx' Hy').
  Qed.

  Lemma rabs_upper : forall x, x <= rabs x /\ - x <= rabs x.
  Proof.
    intro x. destruct (rabs_cases x) as [[-> H] | [-> H]]; split.
    - apply rle_refl.
    - apply (rle_trans _ rzero); [| exact H].
      replace rzero with (- rzero) by ring. exact (proj1 (rle_opp _ _) H).
    - left. exact (rlt_trans _ _ _ H (proj1 (opp_pos x) H)).
    - apply rle_refl.
  Qed.

  Lemma rabs_triangle : forall x y, rabs (x + y) <= rabs x + rabs y.
  Proof.
    intros x y. destruct (rabs_upper x) as [Hx Hx']. destruct (rabs_upper y) as [Hy Hy'].
    destruct (rabs_cases (x + y)) as [[-> _] | [-> _]].
    - exact (rle_add _ _ _ _ Hx Hy).
    - replace (- (x + y)) with (- x + - y) by ring. exact (rle_add _ _ _ _ Hx' Hy').
  Qed.

  Lemma nonneg_sum_zero : forall x y, rzero <= x -> rzero <= y -> x + y = rzero -> x = rzero /\ y = rzero.
  Proof.
    intros x y Hx Hy H. destruct Hx as [Hx | Hx]; destruct Hy as [Hy | Hy].
    - exfalso. assert (Hs : rzero < x + y).
      { pose proof (rlt_add_r _ _ y Hx) as H'. replace (rzero + y) with y in H' by ring.
        exact (rlt_trans _ _ _ Hy H'). }
      rewrite H in Hs. exact (rlt_irrefl _ Hs).
    - exfalso. rewrite <- Hy in H. replace (x + rzero) with x in H by ring.
      rewrite H in Hx. exact (rlt_irrefl _ Hx).
    - exfalso. rewrite <- Hx in H. replace (rzero + y) with y in H by ring.
      rewrite H in Hy. exact (rlt_irrefl _ Hy).
    - split; symmetry; assumption.
  Qed.

  Lemma dist2_nonneg : forall a b, rzero <= dist2 a b.
  Proof.
    induction a as [| x a IH]; intros [| y b]; simpl; try apply rle_refl.
    apply rle_nonneg_add; [apply square_nonneg | apply IH].
  Qed.

  Lemma manhattan_nonneg : forall a b, rzero <= manhattan a b.
  Proof.
    induction a as [| x a IH]; intros [| y b]; simpl; try apply rle_refl.
    apply rle_nonneg_add; [apply rabs_nonneg | apply IH].
  Qed.

  (* Headline 7. The laws of the distances: dist2 is non-negative, symmetric,
     zero on equal vectors and only on them; manhattan is non-negative,
     symmetric and satisfies the triangle inequality. *)
  Theorem distance_laws :
    (forall a b, rzero <= dist2 a b) /\
    (forall a b, dist2 a b = dist2 b a) /\
    (forall a, dist2 a a = rzero) /\
    (forall a b, length a = length b -> dist2 a b = rzero -> a = b) /\
    (forall a b, rzero <= manhattan a b) /\
    (forall a b, manhattan a b = manhattan b a) /\
    (forall a b c, length a = length b -> length b = length c ->
       manhattan a c <= manhattan a b + manhattan b c).
  Proof.
    split; [exact dist2_nonneg |]. split; [| split; [| split; [| split; [exact manhattan_nonneg | split]]]].
    - induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
      rewrite IH. ring.
    - induction a as [| x a IH]; simpl; [reflexivity |]. rewrite IH. ring.
    - induction a as [| x a IH]; intros [| y b] Hl H; simpl in *; try reflexivity; try discriminate.
      injection Hl as Hl.
      destruct (nonneg_sum_zero _ _ (square_nonneg (x - y)) (dist2_nonneg a b) H) as [Hxy Hab].
      apply square_zero in Hxy. assert (x = y).
      { replace x with ((x - y) + y) by ring. rewrite Hxy. ring. }
      subst y. f_equal. exact (IH b Hl Hab).
    - induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
      rewrite IH. f_equal. replace (y - x) with (- (x - y)) by ring. symmetry. apply rabs_opp.
    - induction a as [| x a IH]; intros [| y b] [| z c] Hab Hbc; simpl in *;
        try discriminate.
      + right. ring.
      + injection Hab as Hab. injection Hbc as Hbc.
        replace (rabs (x - y) + manhattan a b + (rabs (y - z) + manhattan b c))
          with ((rabs (x - y) + rabs (y - z)) + (manhattan a b + manhattan b c)) by ring.
        apply rle_add; [| exact (IH b c Hab Hbc)].
        replace (x - z) with ((x - y) + (y - z)) by ring. apply rabs_triangle.
  Qed.

  (* ── Cauchy–Schwarz and the bounds of cosine ──────────────────────────── *)

  Lemma dot_self_nonneg : forall a, rzero <= dot a a.
  Proof.
    induction a as [| x a IH]; simpl; [apply rle_refl |].
    apply rle_nonneg_add; [apply square_nonneg | exact IH].
  Qed.

  (* The quadratic form x²·B + A·y² − 2·x·y·C is non-negative when A ≥ 0,
     B ≥ 0 and C² ≤ A·B: A times the form is (A·y − C·x)² + (A·B − C²)·x². *)
  Lemma quadratic_form_nonneg :
    forall A B C x y, rzero <= A -> rzero <= B -> C * C <= A * B ->
      rzero <= x * x * B + A * (y * y) - (rone + rone) * x * y * C.
  Proof.
    intros A B C x y HA HB HC.
    destruct HA as [HA | HA].
    - apply not_rlt_rle. intro Hneg.
      pose proof (rlt_mul_l A _ _ HA Hneg) as Hprod.
      replace (A * rzero) with rzero in Hprod by ring.
      assert (Hsum : rzero <= (A * y - C * x) * (A * y - C * x) + (A * B - C * C) * (x * x)).
      { apply rle_nonneg_add; [apply square_nonneg |].
        apply mul_nonneg; [| apply square_nonneg].
        exact (proj1 (rle_sub_nonneg (C * C) (A * B)) HC). }
      replace ((A * y - C * x) * (A * y - C * x) + (A * B - C * C) * (x * x))
        with (A * (x * x * B + A * (y * y) - (rone + rone) * x * y * C)) in Hsum by ring.
      exact (rlt_not_rle _ _ Hprod Hsum).
    - subst A. assert (HC0 : C = rzero).
      { apply square_zero. apply rle_antisym; [| apply square_nonneg].
        replace (rzero * B) with rzero in HC by ring. exact HC. }
      subst C. replace (x * x * B + rzero * (y * y) - (rone + rone) * x * y * rzero)
        with (x * x * B) by ring.
      apply mul_nonneg; [apply square_nonneg | exact HB].
  Qed.

  (* Headline 8. Cauchy–Schwarz: (a·b)² ≤ ‖a‖²·‖b‖². *)
  Theorem cauchy_schwarz :
    forall a b, length a = length b -> dot a b * dot a b <= dot a a * dot b b.
  Proof.
    induction a as [| x a IH]; intros [| y b] Hl; simpl in *; try discriminate.
    - right. ring.
    - injection Hl as Hl. specialize (IH b Hl).
      apply (proj2 (rle_sub_nonneg _ _)).
      replace ((x * x + dot a a) * (y * y + dot b b) - (x * y + dot a b) * (x * y + dot a b))
        with ((dot a a * dot b b - dot a b * dot a b) +
              (x * x * dot b b + dot a a * (y * y) - (rone + rone) * x * y * dot a b)) by ring.
      apply rle_nonneg_add.
      + exact (proj1 (rle_sub_nonneg _ _) IH).
      + apply quadratic_form_nonneg; [apply dot_self_nonneg | apply dot_self_nonneg | exact IH].
  Qed.

  (* Headline 9. The bounds of cosine in squared form: every record passes the
     threshold −1, and no record passes a threshold above 1 (cos lies in
     [−1, 1]). *)
  Theorem cosine_bounds :
    forall q e, length q = length e -> rzero < dot q q * dot e e ->
      cosine_within (dot q e) (- rone) (dot q q * dot e e) /\
      (forall t, rone < t -> ~ cosine_within (dot q e) t (dot q q * dot e e)).
  Proof.
    intros q e Hl HN. pose proof (cauchy_schwarz q e Hl) as Hcs.
    set (u := dot q e) in *. set (N := dot q q * dot e e) in *.
    split.
    - unfold cosine_within. destruct (rlt_dec u rzero) as [Hu | Hu].
      + right. right. split; [exact Hu | split].
        * pose proof (proj1 (rlt_opp rzero rone) rlt_zero_one) as H.
          replace (- rzero) with rzero in H by ring. exact H.
        * replace (- rone * - rone * N) with N by ring. exact Hcs.
      + left. split; [exact (not_rlt_rle _ _ Hu) |].
        pose proof (proj1 (rlt_opp rzero rone) rlt_zero_one) as H.
        replace (- rzero) with rzero in H by ring. exact H.
    - intros t Ht. unfold cosine_within.
      assert (Ht0 : rzero < t) by exact (rlt_trans _ _ _ rlt_zero_one Ht).
      intros [[_ Hneg] | [[_ [_ H]] | [_ [Hneg _]]]].
      + exact (rlt_asym _ _ Ht0 Hneg).
      + (* t·t·N ≤ u·u ≤ N = 1·N, so t·t ≤ 1, but 1 < t gives 1 < t·t. *)
        assert (Htt : t * t <= rone).
        { apply (proj1 (rle_mul_r_pos_iff N _ _ HN)).
          replace (rone * N) with N by ring. exact (rle_trans _ _ _ H Hcs). }
        pose proof (square_lt rone t (or_introl rlt_zero_one) Ht) as H1.
        replace (rone * rone) with rone in H1 by ring.
        exact (rlt_not_rle _ _ H1 Htt).
      + exact (rlt_asym _ _ Ht0 Hneg).
  Qed.

  (* ── The score orders ─────────────────────────────────────────────────── *)

  (* The signed square of x: x·|x|. It is strictly increasing, so it keeps
     the order of the cosines: sign(cos)·cos² = ssq(u) / (‖q‖²·n). *)
  Definition ssq (x : R) : R := x * rabs x.

  Lemma cosine_le_ssq :
    forall u1 n1 u2 n2, rzero < n1 -> rzero < n2 ->
      (cosine_le u1 n1 u2 n2 <-> ssq u1 * n2 <= ssq u2 * n1).
  Proof.
    intros u1 n1 u2 n2 Hn1 Hn2. unfold cosine_le, ssq.
    destruct (rabs_cases u1) as [[E1 H1] | [E1 H1]]; destruct (rabs_cases u2) as [[E2 H2] | [E2 H2]];
      rewrite E1, E2.
    - split.
      + intros [[Hu1 _] | [[_ [_ H]] | [Hu1 _]]];
          [exfalso; exact (neg_nonneg_absurd _ Hu1 H1) | exact H |
           exfalso; exact (neg_nonneg_absurd _ Hu1 H1)].
      + intro H. right. left. split; [exact H1 | split; [exact H2 | exact H]].
    - split.
      + intros [[_ Hu2] | [[_ [Hu2 _]] | [Hu1 _]]];
          [exfalso; exact (neg_nonneg_absurd _ H2 Hu2) | exfalso; exact (neg_nonneg_absurd _ H2 Hu2) |
           exfalso; exact (neg_nonneg_absurd _ Hu1 H1)].
      + intro H. exfalso.
        assert (Ha : rzero <= u1 * u1 * n2) by (apply mul_nonneg_pos; [apply square_nonneg | exact Hn2]).
        assert (Hb : u2 * - u2 * n1 < rzero).
        { replace (u2 * - u2 * n1) with (- (u2 * u2 * n1)) by ring.
          apply (proj2 (opp_pos _)). replace (- - (u2 * u2 * n1)) with (u2 * u2 * n1) by ring.
          apply rlt_mul_pos; [| exact Hn1].
          apply square_pos. intro Hz. subst u2. exact (rlt_irrefl _ H2). }
        exact (rlt_not_rle _ _ Hb (rle_trans _ _ _ Ha H)).
    - split.
      + intros _. apply (rle_trans _ rzero).
        * left. replace (u1 * - u1 * n2) with (- (u1 * u1 * n2)) by ring.
          apply (proj2 (opp_pos _)). replace (- - (u1 * u1 * n2)) with (u1 * u1 * n2) by ring.
          apply rlt_mul_pos; [| exact Hn2].
          apply square_pos. intro Hz. subst u1. exact (rlt_irrefl _ H1).
        * apply mul_nonneg_pos; [apply square_nonneg | exact Hn1].
      + intros _. left. split; assumption.
    - replace (u1 * - u1 * n2) with (- (u1 * u1 * n2)) by ring.
      replace (u2 * - u2 * n1) with (- (u2 * u2 * n1)) by ring.
      rewrite <- (rle_opp (u2 * u2 * n1) (u1 * u1 * n2)). split.
      + intros [[_ Hu2] | [[Hu1 _] | [_ [_ H]]]];
          [exfalso; exact (neg_nonneg_absurd _ H2 Hu2) | exfalso; exact (neg_nonneg_absurd _ H1 Hu1) |
           exact H].
      + intro H. right. right. split; [exact H1 | split; [exact H2 | exact H]].
  Qed.

  (* The order of two ratios a1/b1 and a2/b2 with positive denominators, by
     cross-multiplication: a1·b2 ≤ a2·b1. Cosine (a = ssq(u), b = n) and
     jaccard (a = the intersection, b = the union) compare this way. *)
  Definition ratio_le (a1 b1 a2 b2 : R) : Prop := a1 * b2 <= a2 * b1.

  Lemma ratio_le_trans :
    forall a1 b1 a2 b2 a3 b3, rzero < b1 -> rzero < b2 -> rzero < b3 ->
      ratio_le a1 b1 a2 b2 -> ratio_le a2 b2 a3 b3 -> ratio_le a1 b1 a3 b3.
  Proof.
    unfold ratio_le. intros a1 b1 a2 b2 a3 b3 H1 H2 H3 H12 H23.
    apply (proj1 (rle_mul_pos_iff b2 _ _ H2)).
    apply (rle_trans _ (b1 * (a2 * b3))).
    - replace (b2 * (a1 * b3)) with (b3 * (a1 * b2)) by ring.
      replace (b1 * (a2 * b3)) with (b3 * (a2 * b1)) by ring.
      apply rle_mul_l; [left; exact H3 | exact H12].
    - replace (b2 * (a3 * b1)) with (b1 * (a3 * b2)) by ring.
      apply rle_mul_l; [left; exact H1 | exact H23].
  Qed.

  (* Headline 10. Every score order of the six metrics is a total preorder
     (reflexive, transitive and total) on its domain: cosine over positive
     record norms, dot and the distances as the order of their values, and
     jaccard over positive unions. So better decides every pair, and the
     order of the eligible records is well defined. *)
  Theorem score_orders_total :
    (* cosine *)
    (forall u n, rzero < n -> cosine_le u n u n) /\
    (forall u1 n1 u2 n2 u3 n3, rzero < n1 -> rzero < n2 -> rzero < n3 ->
       cosine_le u1 n1 u2 n2 -> cosine_le u2 n2 u3 n3 -> cosine_le u1 n1 u3 n3) /\
    (forall u1 n1 u2 n2, rzero < n1 -> rzero < n2 ->
       cosine_le u1 n1 u2 n2 \/ cosine_le u2 n2 u1 n1) /\
    (* dot, euclidean, manhattan and hamming: the order of their values *)
    (forall x, x <= x) /\
    (forall x y z, x <= y -> y <= z -> x <= z) /\
    (forall x y, x <= y \/ y <= x) /\
    (* jaccard *)
    (forall a1 b1 a2 b2 a3 b3, rzero < b1 -> rzero < b2 -> rzero < b3 ->
       ratio_le a1 b1 a2 b2 -> ratio_le a2 b2 a3 b3 -> ratio_le a1 b1 a3 b3) /\
    (forall a1 b1 a2 b2, ratio_le a1 b1 a2 b2 \/ ratio_le a2 b2 a1 b1).
  Proof.
    split; [| split; [| split; [| split; [| split; [| split; [| split]]]]]].
    - intros u n Hn. apply (proj2 (cosine_le_ssq u n u n Hn Hn)). apply rle_refl.
    - intros u1 n1 u2 n2 u3 n3 H1 H2 H3 H12 H23.
      apply (proj2 (cosine_le_ssq u1 n1 u3 n3 H1 H3)).
      apply (ratio_le_trans (ssq u1) n1 (ssq u2) n2 (ssq u3) n3 H1 H2 H3).
      + exact (proj1 (cosine_le_ssq u1 n1 u2 n2 H1 H2) H12).
      + exact (proj1 (cosine_le_ssq u2 n2 u3 n3 H2 H3) H23).
    - intros u1 n1 u2 n2 H1 H2.
      destruct (rle_total (ssq u1 * n2) (ssq u2 * n1)) as [H | H].
      + left. exact (proj2 (cosine_le_ssq u1 n1 u2 n2 H1 H2) H).
      + right. exact (proj2 (cosine_le_ssq u2 n2 u1 n1 H2 H1) H).
    - exact rle_refl.
    - exact rle_trans.
    - exact rle_total.
    - exact ratio_le_trans.
    - intros a1 b1 a2 b2. unfold ratio_le. destruct (rle_total (a1 * b2) (a2 * b1)) as [H | H].
      + left. exact H.
      + right. exact H.
  Qed.

End OrderedRing.


(* ═══════════════════════════════════════════════════════════════════════════
   Part 2. An embedding of ordered rings keeps the decisions
   ═══════════════════════════════════════════════════════════════════════════ *)

Section Embedding.

  Variables (R T : Type).
  Variables (rzero : R) (rmul : R -> R -> R) (rlt : R -> R -> Prop).
  Variables (tzero : T) (tmul : T -> T -> T) (tlt : T -> T -> Prop).
  Variable f : R -> T.

  (* f embeds R into T: it keeps zero and products, and it keeps and reflects
     the order. The embedding of the rationals into the real numbers is one. *)
  Hypothesis f_zero : f rzero = tzero.
  Hypothesis f_mul : forall x y, f (rmul x y) = tmul (f x) (f y).
  Hypothesis f_lt : forall x y, rlt x y <-> tlt (f x) (f y).
  Hypothesis f_injective : forall x y, f x = f y -> x = y.

  Lemma f_le : forall x y, rle R rlt x y <-> rle T tlt (f x) (f y).
  Proof.
    intros x y. unfold rle. rewrite f_lt. split; intros [H | H].
    - left. exact H.
    - right. rewrite H. reflexivity.
    - left. exact H.
    - right. apply f_injective. exact H.
  Qed.

  (* Headline 11. The decisions of the squared forms are the same in R and in
     T. The Rust code decides them in the rationals, and headlines 1 to 3 hold
     in the real numbers, so the rational decisions are the real ones. *)
  Theorem squared_forms_embed :
    (forall u t N, cosine_within R rzero rmul rlt u t N <->
                   cosine_within T tzero tmul tlt (f u) (f t) (f N)) /\
    (forall u1 n1 u2 n2, cosine_le R rzero rmul rlt u1 n1 u2 n2 <->
                         cosine_le T tzero tmul tlt (f u1) (f n1) (f u2) (f n2)) /\
    (forall d2 r, euclidean_within R rzero rmul rlt d2 r <->
                  euclidean_within T tzero tmul tlt (f d2) (f r)).
  Proof.
    split; [| split]; intros; unfold cosine_within, cosine_le, euclidean_within;
      rewrite ?f_le, ?f_lt, ?f_mul, ?f_zero; reflexivity.
  Qed.

End Embedding.

(* ═══════════════════════════════════════════════════════════════════════════
   Part 3. The binary metrics: hamming and jaccard
   ═══════════════════════════════════════════════════════════════════════════ *)

(* A binary vector is a list of bits. The Rust code reads each element as 0 or
   1, and any other element makes the score undefined (tensor/exact.rs, bits). *)
Fixpoint hamming (a b : list bool) : nat :=
  match a, b with
  | x :: a', y :: b' => (if Bool.eqb x y then 0 else 1) + hamming a' b'
  | _, _ => 0
  end.

Fixpoint inter_count (a b : list bool) : nat :=
  match a, b with
  | x :: a', y :: b' => (if x && y then 1 else 0) + inter_count a' b'
  | _, _ => 0
  end.

Fixpoint union_count (a b : list bool) : nat :=
  match a, b with
  | x :: a', y :: b' => (if x || y then 1 else 0) + union_count a' b'
  | _, _ => 0
  end.

(* Headline 12. Hamming is a metric on binary vectors of one length, and the
   counts of jaccard satisfy 0 ≤ |q ∧ e| ≤ |q ∨ e|. The union is empty only
   when neither vector sets a position, the one case where the score of
   jaccard is undefined. *)
Theorem binary_laws :
  (forall a b, hamming a b = hamming b a) /\
  (forall a b c, length a = length b -> length b = length c ->
     (hamming a c <= hamming a b + hamming b c)%nat) /\
  (forall a b, length a = length b -> hamming a b = 0%nat -> a = b) /\
  (forall a b, (inter_count a b <= union_count a b)%nat) /\
  (forall a b, inter_count a b = inter_count b a /\ union_count a b = union_count b a) /\
  (forall a b, length a = length b -> union_count a b = 0%nat ->
     Forall (fun x => x = false) a /\ Forall (fun x => x = false) b).
Proof.
  split; [| split; [| split; [| split; [| split]]]].
  - induction a as [| x a IH]; intros [| y b]; simpl; try reflexivity.
    rewrite IH. destruct x, y; reflexivity.
  - induction a as [| x a IH]; intros [| y b] [| z c] Hab Hbc; simpl in *; try lia.
    injection Hab as Hab. injection Hbc as Hbc. specialize (IH b c Hab Hbc).
    destruct x, y, z; simpl; lia.
  - induction a as [| x a IH]; intros [| y b] Hl H; simpl in *; try reflexivity; try discriminate.
    injection Hl as Hl. destruct x, y; simpl in H; try discriminate; f_equal; exact (IH b Hl H).
  - induction a as [| x a IH]; intros [| y b]; simpl; try lia.
    specialize (IH b). destruct x, y; simpl; lia.
  - induction a as [| x a IH]; intros [| y b]; simpl; try (split; reflexivity).
    destruct (IH b) as [Hi Hu]. rewrite Hi, Hu. destruct x, y; split; reflexivity.
  - induction a as [| x a IH]; intros [| y b] Hl H; simpl in *; try discriminate.
    + split; constructor.
    + injection Hl as Hl. destruct x, y; simpl in H; try discriminate.
      destruct (IH b Hl H) as [Ha Hb]. split; constructor; auto.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Part 4. The selection order and the determinism of top-k
   ═══════════════════════════════════════════════════════════════════════════ *)

Section Selection.

  Variable S : Type.
  (* at_least a b: the score a is at least as good as the score b. Headline 10
     shows that the order of each metric is such a total preorder. *)
  Variable at_least : S -> S -> Prop.
  Hypothesis at_least_dec : forall a b, {at_least a b} + {~ at_least a b}.
  Hypothesis at_least_refl : forall a, at_least a a.
  Hypothesis at_least_trans : forall a b c, at_least a b -> at_least b c -> at_least a c.
  Hypothesis at_least_total : forall a b, at_least a b \/ at_least b a.

  (* A candidate is an eligible record: its exact score, the DR-75 key kappa of
     decision 41 (modeled as a natural number, the rank of the key among the
     records), and its index in the view. Two records of one view have
     different indices. *)
  Record candidate := Candidate { c_score : S; c_kappa : nat; c_index : nat }.

  Definition strictly_better (a b : S) : Prop := at_least a b /\ ~ at_least b a.
  Definition same_score (a b : S) : Prop := at_least a b /\ at_least b a.

  Definition tie_key (r : candidate) : nat * nat := (c_kappa r, c_index r).

  Definition tie_before (r1 r2 : candidate) : Prop :=
    (c_kappa r1 < c_kappa r2)%nat \/ (c_kappa r1 = c_kappa r2 /\ (c_index r1 < c_index r2)%nat).

  (* The selection order of design v3, section 4.4: the better score first,
     then the lower kappa, then the lower view index. *)
  Definition precedes (r1 r2 : candidate) : Prop :=
    strictly_better (c_score r1) (c_score r2) \/
    (same_score (c_score r1) (c_score r2) /\ tie_before r1 r2).

  Lemma precedes_irrefl : forall r, ~ precedes r r.
  Proof.
    intros r [[_ H] | [_ [H | [_ H]]]]; [exact (H (at_least_refl _)) | lia | lia].
  Qed.

  Lemma precedes_trans : forall r1 r2 r3, precedes r1 r2 -> precedes r2 r3 -> precedes r1 r3.
  Proof.
    unfold precedes, strictly_better, same_score, tie_before.
    intros r1 r2 r3 [[H12 N21] | [[H12 H21] T12]] [[H23 N32] | [[H23 H32] T23]].
    - left. split; [exact (at_least_trans _ _ _ H12 H23) |].
      intro H31. exact (N32 (at_least_trans _ _ _ H31 H12)).
    - left. split; [exact (at_least_trans _ _ _ H12 H23) |].
      intro H31. exact (N21 (at_least_trans _ _ _ H23 H31)).
    - left. split; [exact (at_least_trans _ _ _ H12 H23) |].
      intro H31. exact (N32 (at_least_trans _ _ _ H31 H12)).
    - right. split; [split; [exact (at_least_trans _ _ _ H12 H23) |
                             exact (at_least_trans _ _ _ H32 H21)] |].
      lia.
  Qed.

  Lemma precedes_asym : forall r1 r2, precedes r1 r2 -> ~ precedes r2 r1.
  Proof.
    intros r1 r2 H12 H21. exact (precedes_irrefl r1 (precedes_trans _ _ _ H12 H21)).
  Qed.

  Lemma precedes_total :
    forall r1 r2, tie_key r1 <> tie_key r2 -> precedes r1 r2 \/ precedes r2 r1.
  Proof.
    unfold precedes, strictly_better, same_score, tie_before, tie_key.
    intros r1 r2 Hkey.
    destruct (at_least_dec (c_score r1) (c_score r2)) as [H12 | H12];
      destruct (at_least_dec (c_score r2) (c_score r1)) as [H21 | H21].
    - assert (Hk : (c_kappa r1 < c_kappa r2)%nat \/ (c_kappa r2 < c_kappa r1)%nat \/
                   (c_kappa r1 = c_kappa r2 /\ (c_index r1 < c_index r2)%nat) \/
                   (c_kappa r2 = c_kappa r1 /\ (c_index r2 < c_index r1)%nat)).
      { destruct (Nat.lt_trichotomy (c_kappa r1) (c_kappa r2)) as [H | [H | H]];
          [left; exact H | | right; left; exact H].
        destruct (Nat.lt_trichotomy (c_index r1) (c_index r2)) as [Hi | [Hi | Hi]].
        - right. right. left. split; assumption.
        - exfalso. apply Hkey. rewrite H, Hi. reflexivity.
        - right. right. right. split; [symmetry; exact H | exact Hi]. }
      destruct Hk as [Hk | [Hk | [Hk | Hk]]].
      + left. right. split; [split; assumption | left; exact Hk].
      + right. right. split; [split; assumption | left; exact Hk].
      + left. right. split; [split; assumption | right; exact Hk].
      + right. right. split; [split; assumption | right; exact Hk].
    - left. left. split; assumption.
    - right. left. split; assumption.
    - destruct (at_least_total (c_score r1) (c_score r2)); contradiction.
  Qed.

  Lemma precedes_dec : forall r1 r2, {precedes r1 r2} + {~ precedes r1 r2}.
  Proof.
    intros r1 r2. unfold precedes, strictly_better, same_score, tie_before.
    destruct (at_least_dec (c_score r1) (c_score r2)) as [H12 | H12];
      destruct (at_least_dec (c_score r2) (c_score r1)) as [H21 | H21];
      destruct (lt_dec (c_kappa r1) (c_kappa r2)) as [Hk | Hk];
      destruct (Nat.eq_dec (c_kappa r1) (c_kappa r2)) as [He | He];
      destruct (lt_dec (c_index r1) (c_index r2)) as [Hi | Hi];
      try (left; tauto); right; intros [[A B] | [[A B] C]]; tauto.
  Qed.

  (* Headline 13. The selection order is a strict total order on the records
     of one view: irreflexive, transitive, and total on distinct tie keys. *)
  Theorem selection_total_order :
    (forall r, ~ precedes r r) /\
    (forall r1 r2 r3, precedes r1 r2 -> precedes r2 r3 -> precedes r1 r3) /\
    (forall r1 r2, tie_key r1 <> tie_key r2 -> precedes r1 r2 \/ precedes r2 r1).
  Proof. split; [exact precedes_irrefl | split; [exact precedes_trans | exact precedes_total]]. Qed.

  Lemma strongly_sorted_unique :
    forall l1 l2, StronglySorted precedes l1 -> StronglySorted precedes l2 ->
      Permutation l1 l2 -> l1 = l2.
  Proof.
    induction l1 as [| x t1 IH]; intros l2 S1 S2 P.
    - symmetry. exact (Permutation_nil P).
    - destruct l2 as [| y t2].
      + exfalso. exact (Permutation_nil_cons (Permutation_sym P)).
      + apply StronglySorted_inv in S1 as [S1 F1]. apply StronglySorted_inv in S2 as [S2 F2].
        assert (Hxy : x = y).
        { assert (Hy : In y (x :: t1)) by (apply (Permutation_in _ (Permutation_sym P)); left; reflexivity).
          assert (Hx : In x (y :: t2)) by (apply (Permutation_in _ P); left; reflexivity).
          destruct Hy as [Hy | Hy]; [exact Hy |].
          destruct Hx as [Hx | Hx]; [exact (eq_sym Hx) |].
          exfalso. pose proof (proj1 (Forall_forall _ _) F1 y Hy) as Pxy.
          pose proof (proj1 (Forall_forall _ _) F2 x Hx) as Pyx.
          exact (precedes_asym _ _ Pxy Pyx). }
        subst y. f_equal. apply IH; [exact S1 | exact S2 |].
        exact (Permutation_cons_inv P).
  Qed.

  (* Headline 14. top-k is deterministic: two sorted arrangements of one
     multiset of records are equal, so their first k records are equal. The
     order in which a node reads the records does not change the result. *)
  Theorem topk_deterministic :
    forall l1 l2 k, StronglySorted precedes l1 -> StronglySorted precedes l2 ->
      Permutation l1 l2 -> firstn k l1 = firstn k l2.
  Proof.
    intros l1 l2 k S1 S2 P. rewrite (strongly_sorted_unique l1 l2 S1 S2 P). reflexivity.
  Qed.

  Lemma filter_permutation :
    forall (p : candidate -> bool) l l', Permutation l l' -> Permutation (filter p l) (filter p l').
  Proof.
    intros p l l' H. induction H; simpl.
    - constructor.
    - destruct (p x); [constructor |]; exact IHPermutation.
    - destruct (p x), (p y); [apply perm_swap | reflexivity | reflexivity | reflexivity].
    - exact (perm_trans IHPermutation1 IHPermutation2).
  Qed.

  (* The insertion of a record into a sorted list, and the insertion sort. *)
  Fixpoint insert (x : candidate) (l : list candidate) : list candidate :=
    match l with
    | [] => [x]
    | y :: l' => if precedes_dec x y then x :: y :: l' else y :: insert x l'
    end.

  Fixpoint insertion_sort (l : list candidate) : list candidate :=
    match l with
    | [] => []
    | x :: l' => insert x (insertion_sort l')
    end.

  Lemma insert_permutation : forall x l, Permutation (insert x l) (x :: l).
  Proof.
    intros x l. induction l as [| y l IH]; simpl; [reflexivity |].
    destruct (precedes_dec x y); [reflexivity |].
    transitivity (y :: x :: l); [constructor; exact IH | apply perm_swap].
  Qed.

  Lemma insertion_sort_permutation : forall l, Permutation (insertion_sort l) l.
  Proof.
    induction l as [| x l IH]; simpl; [reflexivity |].
    transitivity (x :: insertion_sort l); [apply insert_permutation | constructor; exact IH].
  Qed.

  Lemma insert_sorted :
    forall x l, StronglySorted precedes l -> (forall z, In z l -> tie_key z <> tie_key x) ->
      StronglySorted precedes (insert x l).
  Proof.
    intros x l. induction l as [| y l IH]; intros Sl Hkey; simpl.
    - constructor; constructor.
    - apply StronglySorted_inv in Sl as Sl'. destruct Sl' as [Sl' Fl].
      destruct (precedes_dec x y) as [Pxy | Pxy].
      + constructor; [exact Sl |]. constructor; [exact Pxy |].
        apply Forall_forall. intros z Hz.
        exact (precedes_trans _ _ _ Pxy (proj1 (Forall_forall _ _) Fl z Hz)).
      + assert (Pyx : precedes y x).
        { destruct (precedes_total x y) as [H | H]; [| contradiction | exact H].
          intro Heq. apply (Hkey y); [left; reflexivity | symmetry; exact Heq]. }
        constructor.
        * apply IH; [exact Sl' |]. intros z Hz. apply Hkey. right. exact Hz.
        * apply Forall_forall. intros z Hz.
          apply (Permutation_in _ (insert_permutation x l)) in Hz. destruct Hz as [<- | Hz].
          -- exact Pyx.
          -- exact (proj1 (Forall_forall _ _) Fl z Hz).
  Qed.

  Lemma insertion_sort_sorted :
    forall l, NoDup (map tie_key l) -> StronglySorted precedes (insertion_sort l).
  Proof.
    induction l as [| x l IH]; intro Hnd; simpl; [constructor |].
    inversion Hnd as [| ? ? Hx Hnd']; subst.
    apply insert_sorted; [exact (IH Hnd') |].
    intros z Hz Heq. apply Hx.
    apply (Permutation_in _ (insertion_sort_permutation l)) in Hz.
    rewrite <- Heq. apply in_map. exact Hz.
  Qed.

  (* Headline 15. List mode is deterministic: for any eligibility test, the
     list of the first k eligible records has one value for one multiset of
     records, and the insertion sort computes it. *)
  Theorem list_selection_deterministic :
    forall (eligible : candidate -> bool) l1 l2 s1 s2 k,
      Permutation l1 l2 ->
      Permutation s1 (filter eligible l1) -> StronglySorted precedes s1 ->
      Permutation s2 (filter eligible l2) -> StronglySorted precedes s2 ->
      firstn k s1 = firstn k s2 /\
      (NoDup (map tie_key l1) ->
         firstn k s1 = firstn k (insertion_sort (filter eligible l1))).
  Proof.
    intros eligible l1 l2 s1 s2 k P P1 S1 P2 S2. split.
    - apply topk_deterministic; [exact S1 | exact S2 |].
      transitivity (filter eligible l1); [exact P1 |].
      transitivity (filter eligible l2); [exact (filter_permutation _ _ _ P) |].
      symmetry. exact P2.
    - intro Hnd. apply topk_deterministic; [exact S1 | |].
      + apply insertion_sort_sorted.
        clear -Hnd. induction l1 as [| x l IH]; simpl; [constructor |].
        inversion Hnd as [| ? ? Hx Hnd']; subst. destruct (eligible x); simpl.
        * constructor; [| exact (IH Hnd')].
          intro Hin. apply Hx. apply in_map_iff in Hin as [z [Hz Hzin]].
          apply filter_In in Hzin as [Hzin _]. rewrite <- Hz. apply in_map. exact Hzin.
        * exact (IH Hnd').
      + transitivity (filter eligible l1); [exact P1 |].
        symmetry. apply insertion_sort_permutation.
  Qed.

End Selection.

(* Headline 16, the negative control of the kappa tie-break. Without kappa
   and the view index, two records with equal scores can be sorted in either
   order: both lists are sorted by score, they hold the same records, and
   their first records differ. So two nodes that read the view in different
   orders could fire different records. *)
Theorem kappa_dropped_not_unique :
  exists l1 l2 : list (candidate nat),
    Sorted (fun r1 r2 => (c_score nat r2 <= c_score nat r1)%nat) l1 /\
    Sorted (fun r1 r2 => (c_score nat r2 <= c_score nat r1)%nat) l2 /\
    Permutation l1 l2 /\ firstn 1 l1 <> firstn 1 l2.
Proof.
  set (r1 := Candidate nat 5%nat 1%nat 0%nat). set (r2 := Candidate nat 5%nat 2%nat 1%nat).
  exists [r1; r2], [r2; r1]. split; [| split; [| split]].
  - repeat constructor.
  - repeat constructor.
  - apply perm_swap.
  - simpl. intro H. injection H as H. discriminate.
Qed.


(* ═══════════════════════════════════════════════════════════════════════════
   Part 5. The rationals discharge the hypotheses
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The Rust code computes with reduced rationals (num_rational::BigRational).
   The canonical rationals Qc of the standard library model them: every value
   has one representation, so equality is Leibniz equality, as Part 1 asks. *)

Section QcInstance.

  Local Open Scope Qc_scope.

  Definition Qc_lt_dec (x y : Qc) : {x < y} + {~ x < y} :=
    match Qclt_le_dec x y with
    | left H => left H
    | right H => right (Qcle_not_lt _ _ H)
    end.

  Lemma Qc_lt_irrefl : forall x : Qc, ~ x < x.
  Proof. intros x H. exact (Qclt_not_eq _ _ H eq_refl). Qed.

  Lemma Qc_lt_total : forall x y : Qc, x < y \/ x = y \/ y < x.
  Proof.
    intros x y. destruct (Qc_dec x y) as [[H | H] | H].
    - left. exact H.
    - right. right. exact H.
    - right. left. exact H.
  Qed.

  Lemma Qc_lt_add_r : forall x y z : Qc, x < y -> x + z < y + z.
  Proof.
    intros x y z H. unfold Qclt, Qcplus, Q2Qc in *. cbn [this].
    rewrite !Qred_correct. exact (proj2 (Qplus_lt_l _ _ _) H).
  Qed.

  Lemma Qc_lt_mul_pos : forall x y : Qc, 0 < x -> 0 < y -> 0 < x * y.
  Proof.
    intros x y Hx Hy. pose proof (Qcmult_lt_compat_r 0 x y Hy Hx) as H.
    rewrite Qcmult_0_l in H. exact H.
  Qed.

  Lemma Qc_zero_one : 0 < 1.
  Proof. reflexivity. Qed.

End QcInstance.

(* Applies a theorem of Part 1 to Qc: each hypothesis of an ordered ring is
   discharged by its Qc lemma. *)
Ltac qc_discharge thm :=
  let H := fresh "H" in
  pose proof thm as H;
  repeat (specialize (H ltac:(first [ exact Qcrt | exact Qc_lt_dec | exact Qc_lt_irrefl
                                     | exact Qclt_trans | exact Qc_lt_total | exact Qc_lt_add_r
                                     | exact Qc_lt_mul_pos | exact Qc_zero_one ])));
  exact H.

(* Headline 17. The canonical rationals satisfy every hypothesis of Part 1,
   with Qc_lt_dec as the decision of the order. *)
Theorem Qc_ordered_ring :
  ring_theory 0%Qc 1%Qc Qcplus Qcmult Qcminus Qcopp (@eq Qc) /\
  (forall x : Qc, ~ (x < x)%Qc) /\
  (forall x y z : Qc, (x < y)%Qc -> (y < z)%Qc -> (x < z)%Qc) /\
  (forall x y : Qc, (x < y)%Qc \/ x = y \/ (y < x)%Qc) /\
  (forall x y z : Qc, (x < y)%Qc -> (x + z < y + z)%Qc) /\
  (forall x y : Qc, (0 < x)%Qc -> (0 < y)%Qc -> (0 < x * y)%Qc) /\
  (0 < 1)%Qc.
Proof.
  split; [exact Qcrt |]. split; [exact Qc_lt_irrefl |]. split; [exact Qclt_trans |].
  split; [exact Qc_lt_total |]. split; [exact Qc_lt_add_r |].
  split; [exact Qc_lt_mul_pos | exact Qc_zero_one].
Qed.

(* Headline 18. The headlines 1, 2, 3, 8 and 9 hold in Qc. *)
Theorem similarity_headlines_Qc :
  (forall u t s : Qc, (0 < s)%Qc ->
     (rle Qc Qclt (t * s)%Qc u <-> cosine_within Qc 0%Qc Qcmult Qclt u t (s * s)%Qc)) /\
  (forall nq u1 u2 s1 s2 n1 n2 : Qc,
     (0 < nq)%Qc -> (0 < s1)%Qc -> (0 < s2)%Qc ->
     (s1 * s1 = nq * n1)%Qc -> (s2 * s2 = nq * n2)%Qc ->
     (rle Qc Qclt (u1 * s2)%Qc (u2 * s1)%Qc <-> cosine_le Qc 0%Qc Qcmult Qclt u1 n1 u2 n2)) /\
  (forall d r : Qc, rle Qc Qclt 0%Qc d ->
     (rle Qc Qclt d r <-> euclidean_within Qc 0%Qc Qcmult Qclt (d * d)%Qc r)) /\
  (forall a b : list Qc, length a = length b ->
     rle Qc Qclt (dot Qc 0%Qc Qcplus Qcmult a b * dot Qc 0%Qc Qcplus Qcmult a b)%Qc
                 (dot Qc 0%Qc Qcplus Qcmult a a * dot Qc 0%Qc Qcplus Qcmult b b)%Qc) /\
  (forall q e : list Qc, length q = length e ->
     (0 < dot Qc 0%Qc Qcplus Qcmult q q * dot Qc 0%Qc Qcplus Qcmult e e)%Qc ->
     cosine_within Qc 0%Qc Qcmult Qclt (dot Qc 0%Qc Qcplus Qcmult q e) (Qcopp 1%Qc)
       (dot Qc 0%Qc Qcplus Qcmult q q * dot Qc 0%Qc Qcplus Qcmult e e)%Qc).
Proof.
  split; [| split; [| split; [| split]]].
  - qc_discharge (cosine_squared_form_iff Qc 0%Qc 1%Qc Qcplus Qcmult Qcminus Qcopp Qclt).
  - qc_discharge (cosine_order_squared_form Qc 0%Qc 1%Qc Qcplus Qcmult Qcminus Qcopp Qclt).
  - qc_discharge (euclidean_radius_squared_form Qc 0%Qc 1%Qc Qcplus Qcmult Qcminus Qcopp Qclt).
  - qc_discharge (cauchy_schwarz Qc 0%Qc 1%Qc Qcplus Qcmult Qcminus Qcopp Qclt).
  - intros q e Hl HN.
    pose proof (cosine_bounds Qc 0%Qc 1%Qc Qcplus Qcmult Qcminus Qcopp Qclt
                  Qcrt Qc_lt_dec Qc_lt_irrefl Qclt_trans Qc_lt_total Qc_lt_add_r
                  Qc_lt_mul_pos Qc_zero_one q e Hl HN) as [H _].
    exact H.
Qed.

(* The embedding of the integers into the rationals, as the Rust code embeds
   the u128 counts of hamming and jaccard. *)
Definition Z_to_Qc (z : Z) : Qc := Q2Qc (inject_Z z).

Lemma Z_to_Qc_mul : forall x y, Z_to_Qc (x * y) = (Z_to_Qc x * Z_to_Qc y)%Qc.
Proof.
  intros x y. apply Qc_is_canon. unfold Z_to_Qc, Qcmult, Q2Qc. cbn [this].
  rewrite !Qred_correct, inject_Z_mult. reflexivity.
Qed.

Lemma Z_to_Qc_lt : forall x y, (x < y)%Z <-> (Z_to_Qc x < Z_to_Qc y)%Qc.
Proof.
  intros x y. unfold Z_to_Qc, Qclt, Q2Qc. cbn [this]. rewrite !Qred_correct, Zlt_Qlt.
  reflexivity.
Qed.

Lemma Z_to_Qc_injective : forall x y, Z_to_Qc x = Z_to_Qc y -> x = y.
Proof.
  intros x y H. destruct (Z.lt_trichotomy x y) as [Hxy | [Hxy | Hxy]].
  - exfalso. apply (proj1 (Z_to_Qc_lt x y)) in Hxy. rewrite H in Hxy. exact (Qc_lt_irrefl _ Hxy).
  - exact Hxy.
  - exfalso. apply (proj1 (Z_to_Qc_lt y x)) in Hxy. rewrite H in Hxy. exact (Qc_lt_irrefl _ Hxy).
Qed.

(* Headline 19. Headline 11 holds for the embedding of Z into Qc: integer
   decisions are rational decisions. *)
Theorem squared_forms_embed_Z_Qc :
  (forall u t N, cosine_within Z 0%Z Z.mul Z.lt u t N <->
                 cosine_within Qc 0%Qc Qcmult Qclt (Z_to_Qc u) (Z_to_Qc t) (Z_to_Qc N)) /\
  (forall u1 n1 u2 n2, cosine_le Z 0%Z Z.mul Z.lt u1 n1 u2 n2 <->
                       cosine_le Qc 0%Qc Qcmult Qclt (Z_to_Qc u1) (Z_to_Qc n1) (Z_to_Qc u2) (Z_to_Qc n2)) /\
  (forall d2 r, euclidean_within Z 0%Z Z.mul Z.lt d2 r <->
                euclidean_within Qc 0%Qc Qcmult Qclt (Z_to_Qc d2) (Z_to_Qc r)).
Proof.
  apply squared_forms_embed.
  - reflexivity.
  - exact Z_to_Qc_mul.
  - exact Z_to_Qc_lt.
  - exact Z_to_Qc_injective.
Qed.

(* Headline 20. Headlines 13, 14 and 15 hold for rational scores, where a
   score a is at least as good as b when b ≤ a (dot, and the reversed
   distances). *)
Definition Qc_at_least (a b : Qc) : Prop := (b <= a)%Qc.

Definition Qc_at_least_dec (a b : Qc) : {Qc_at_least a b} + {~ Qc_at_least a b} :=
  match Qclt_le_dec a b with
  | left H => right (Qclt_not_le _ _ H)
  | right H => left H
  end.

Theorem selection_Qc :
  (forall r : candidate Qc, ~ precedes Qc Qc_at_least r r) /\
  (forall r1 r2 r3 : candidate Qc, precedes Qc Qc_at_least r1 r2 ->
     precedes Qc Qc_at_least r2 r3 -> precedes Qc Qc_at_least r1 r3) /\
  (forall r1 r2 : candidate Qc, tie_key Qc r1 <> tie_key Qc r2 ->
     precedes Qc Qc_at_least r1 r2 \/ precedes Qc Qc_at_least r2 r1) /\
  (forall l1 l2 k, StronglySorted (precedes Qc Qc_at_least) l1 ->
     StronglySorted (precedes Qc Qc_at_least) l2 -> Permutation l1 l2 ->
     firstn k l1 = firstn k l2).
Proof.
  assert (Hrefl : forall a, Qc_at_least a a) by (intro a; apply Qcle_refl).
  assert (Htrans : forall a b c, Qc_at_least a b -> Qc_at_least b c -> Qc_at_least a c)
    by (unfold Qc_at_least; intros a b c Hab Hbc; exact (Qcle_trans _ _ _ Hbc Hab)).
  assert (Htotal : forall a b, Qc_at_least a b \/ Qc_at_least b a).
  { unfold Qc_at_least. intros a b. destruct (Qclt_le_dec a b) as [H | H].
    - right. apply Qclt_le_weak. exact H.
    - left. exact H. }
  pose proof (selection_total_order Qc Qc_at_least Qc_at_least_dec Hrefl Htrans Htotal)
    as [Hirr [Htr Htot]].
  split; [exact Hirr |]. split; [exact Htr |]. split; [exact Htot |].
  exact (topk_deterministic Qc Qc_at_least Hrefl Htrans).
Qed.

Print Assumptions cosine_squared_form_iff.
Print Assumptions cosine_order_squared_form.
Print Assumptions euclidean_radius_squared_form.
Print Assumptions threshold_monotone.
Print Assumptions radius_monotone.
Print Assumptions dot_laws.
Print Assumptions distance_laws.
Print Assumptions cauchy_schwarz.
Print Assumptions cosine_bounds.
Print Assumptions score_orders_total.
Print Assumptions squared_forms_embed.
Print Assumptions binary_laws.
Print Assumptions selection_total_order.
Print Assumptions topk_deterministic.
Print Assumptions list_selection_deterministic.
Print Assumptions kappa_dropped_not_unique.
Print Assumptions Qc_ordered_ring.
Print Assumptions similarity_headlines_Qc.
Print Assumptions squared_forms_embed_Z_Qc.
Print Assumptions selection_Qc.
