(* ===========================================================================
   Filter.v - Certified latest-message eligibility projection (T-10 reuse).

   CertifiedConsensusContext derives one exclusion set from the finalized-floor
   authority, certified generations, intrinsic block validity, causal objective
   evidence, sender provenance, and floor ancestry. The estimator consumes that
   projection without consulting or mutating a receiver-local invalid cache.

   This is the operational realization of the slashing property T-10 (a slashed
   validator, whose post-slash bond is 0, is excluded from the fork-choice
   estimator). The BOND semantics (why a slashed validator has zero weight) are
   proved UPSTREAM and are NOT re-derived here:

       formal/rocq/slashing/theories/ForkChoice.v : fork_choice_exclusion
         (bm_lookup bonds v = 0  ->  fc_lookup (filter_slashed lm bonds) v = None)

   Here `inv` denotes the certified exclusion set. We prove exclusion,
   preservation, idempotence, and receiver-cache noninterference.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                | Rust (casper/src/rust/estimator.rs)
   --------------------+-----------------------------------------------------
   filter_inv          | FinalityVoteProjection::eligible_latest_messages
   invalid_excluded    | certified-excluded validator contributes no vote
   valid_preserved     | active validator's tip survives the filter
   filter_idempotent   | retain is idempotent on the same invalid set
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
Import ListNotations.

From ForkChoice Require Import Foundation.

(* Validator `v` is in the certified exclusion set. *)
Definition in_inv (inv : list Validator) (v : Validator) : bool :=
  existsb (Nat.eqb v) inv.

Lemma in_inv_true_iff :
  forall inv v, in_inv inv v = true <-> In v inv.
Proof.
  intros inv v. unfold in_inv. split; intro H.
  - apply existsb_exists in H. destruct H as [x [Hin He]].
    apply Nat.eqb_eq in He. subst x. exact Hin.
  - apply existsb_exists. exists v. split; [exact H | apply Nat.eqb_refl].
Qed.

Lemma in_inv_false_iff :
  forall inv v, in_inv inv v = false <-> ~ In v inv.
Proof.
  intros inv v. split; intro H.
  - intro Hin. apply (in_inv_true_iff inv v) in Hin. rewrite Hin in H. discriminate.
  - destruct (in_inv inv v) eqn:E; [| reflexivity].
    exfalso. apply H. apply (in_inv_true_iff inv v). exact E.
Qed.

(* Drop every latest message whose validator is certified ineligible. *)
Definition filter_inv (lms : list (Validator * BlockHash)) (inv : list Validator)
  : list (Validator * BlockHash) :=
  filter (fun e => negb (in_inv inv (fst e))) lms.

(* T-10: a certified-ineligible validator's latest message is excluded. *)
Theorem invalid_excluded :
  forall lms inv v h, In v inv -> ~ In (v, h) (filter_inv lms inv).
Proof.
  intros lms inv v h Hin Hcontra.
  unfold filter_inv in Hcontra. apply filter_In in Hcontra.
  destruct Hcontra as [_ Hpred]. simpl in Hpred.
  apply negb_true_iff in Hpred.
  apply (in_inv_false_iff inv v) in Hpred. contradiction.
Qed.

(* A valid (non-slashed) validator's latest message survives the filter. *)
Theorem valid_preserved :
  forall lms inv v h, ~ In v inv -> In (v, h) lms -> In (v, h) (filter_inv lms inv).
Proof.
  intros lms inv v h Hnin Hin.
  unfold filter_inv. apply filter_In. split; [exact Hin |].
  simpl. apply negb_true_iff. apply (in_inv_false_iff inv v). exact Hnin.
Qed.

(* Idempotence: filtering twice with the same invalid set = filtering once. *)
Theorem filter_idempotent :
  forall lms inv, filter_inv (filter_inv lms inv) inv = filter_inv lms inv.
Proof.
  intros lms inv. unfold filter_inv.
  induction lms as [| e l IH]; simpl.
  - reflexivity.
  - destruct (negb (in_inv inv (fst e))) eqn:E; simpl.
    + rewrite E. rewrite IH. reflexivity.
    + rewrite IH. reflexivity.
Qed.

Definition projection_with_receiver_cache
           (lms : list (Validator * BlockHash))
           (certified_exclusions receiver_cache : list Validator)
  : list (Validator * BlockHash) :=
  filter_inv lms certified_exclusions.

Theorem receiver_cache_noninterference :
  forall lms certified_exclusions receiver_cache1 receiver_cache2,
    projection_with_receiver_cache lms certified_exclusions receiver_cache1 =
    projection_with_receiver_cache lms certified_exclusions receiver_cache2.
Proof. reflexivity. Qed.
