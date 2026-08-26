(* ===========================================================================
   Score.v - Frozen-authority supporting-chain score fold.

   Fork choice consumes a CertifiedConsensusContext. For every eligible latest
   message (v,h), the implementation adds the single stake assigned to v by the
   certified finalized-floor authority to each block in h's supporting chain.
   Candidate, traversed-block, and receiver-local bond maps are not inputs.

   Rust correspondence:

     authority               CertifiedConsensusContext::authority_stakes
     weight authority v      one frozen stake per validator and round
     supports d h b          b is visited by h's ancestor BFS
     contrib/build_scores    parallel contributions, deterministic reduction
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

From ForkChoice Require Import Foundation.

Fixpoint bond_of (bonds : list (Validator * nat)) (v : Validator) : nat :=
  match bonds with
  | [] => 0
  | (v', w) :: rest => if Nat.eqb v' v then w else bond_of rest v
  end.

Definition Authority := list (Validator * nat).

Definition weight (authority : Authority) (v : Validator) : nat :=
  bond_of authority v.

Definition estimator_weight (authority candidate_bonds : Authority)
                            (v : Validator) : nat :=
  weight authority v.

Lemma weight_is_pure :
  forall authority1 authority2 v,
    authority1 = authority2 ->
    weight authority1 v = weight authority2 v.
Proof.
  intros authority1 authority2 v H. subst. reflexivity.
Qed.

Lemma candidate_bonds_noninterference :
  forall authority candidate_bonds1 candidate_bonds2 v,
    estimator_weight authority candidate_bonds1 v =
    estimator_weight authority candidate_bonds2 v.
Proof. reflexivity. Qed.

Definition supports (d : DAG) (h : BlockHash) (b : BlockHash) : Prop :=
  anc_of d b h.

Definition contrib (authority : Authority) (d : DAG) (fuel : nat)
                   (b : BlockHash) (e : Validator * BlockHash) : nat :=
  if anc_ofb d fuel b (snd e) then weight authority (fst e) else 0.

Definition build_scores (authority : Authority) (d : DAG) (fuel : nat)
                        (lms : list (Validator * BlockHash))
                        (b : BlockHash) : nat :=
  fold_right (fun e acc => contrib authority d fuel b e + acc) 0 lms.

Lemma build_scores_cons :
  forall authority d fuel e l b,
    build_scores authority d fuel (e :: l) b =
    contrib authority d fuel b e + build_scores authority d fuel l b.
Proof. reflexivity. Qed.

Lemma score_perm_invariant :
  forall authority d fuel lms lms' b,
    Permutation lms lms' ->
    build_scores authority d fuel lms b =
    build_scores authority d fuel lms' b.
Proof.
  intros authority d fuel lms lms' b H.
  induction H as [| x l l' Hp IH | x y l | l1 l2 l3 Hp1 IH1 Hp2 IH2].
  - reflexivity.
  - rewrite !build_scores_cons. rewrite IH. reflexivity.
  - rewrite !build_scores_cons. lia.
  - rewrite IH1, IH2. reflexivity.
Qed.

Lemma score_eq_support_sum :
  forall authority d fuel lms b,
    build_scores authority d fuel lms b
    = fold_right Nat.add 0
        (map (fun e => weight authority (fst e))
             (filter (fun e => anc_ofb d fuel b (snd e)) lms)).
Proof.
  intros authority d fuel lms b. induction lms as [| e l IH].
  - reflexivity.
  - rewrite build_scores_cons. unfold contrib. cbn [filter].
    destruct (anc_ofb d fuel b (snd e)) eqn:E;
      cbn [map fold_right]; rewrite IH; lia.
Qed.

Lemma bfs_visits_once :
  forall authority d fuel b e,
    contrib authority d fuel b e = 0 \/
    contrib authority d fuel b e = weight authority (fst e).
Proof.
  intros authority d fuel b e. unfold contrib.
  destruct (anc_ofb d fuel b (snd e)); [right | left]; reflexivity.
Qed.
