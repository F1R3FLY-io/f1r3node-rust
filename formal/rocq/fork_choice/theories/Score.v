(* ===========================================================================
   Score.v - Per-validator weight and the supporting-chain score fold.

   The estimator scores each candidate block by the cumulative bonded weight of
   the validators whose latest message it supports (estimator.rs:215-299,
   build_scores_map). For each latest message (v, h), a BFS descends h's
   supporting chain and adds v's weight to every block on it. A block b thus
   receives v's weight iff b is a DAG-ancestor-or-self of h (b lies in h's past).

   Two consensus-critical facts are proved here:

     * ORDER-INDEPENDENCE (T-SCORE-DET): the score is invariant under the order
       in which validators/latest-messages are folded (`score_perm_invariant`).
       The Rust folds a HashMap whose iteration order is nondeterministic across
       nodes; were the score order-dependent, two honest nodes could rank tips
       differently -> a FORK. Proof: the fold is over (Nat.add), a commutative
       monoid; permutation-invariance uses only add-comm/assoc (mirrors
       finalized_floor/Merge.merge_perm).

     * PURITY (T-WEIGHT-PURE): a validator's weight is a pure function of the
       RESOLVED main-parent bonds (`weight_is_pure`) - so equal DAGs (equal
       resolved main parent) yield equal weight, node-identically. Faithful to
       weight_from_validator_by_dag (proto_util.rs:160): read the block's main
       parent, look its bond for `v` out of the parent's weight_map (0 if
       unbonded); genesis reads its own map.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                  | Rust (casper/src/rust)
   ----------------------+---------------------------------------------------
   resolve_main / weight | util/proto_util.rs:160 weight_from_validator_by_dag
   supports d h b        | b on h's supporting chain (estimator.rs:257 BFS)
   contrib / build_scores| estimator.rs:267 `*entry = entry + validator_weight`
   score_perm_invariant  | HashMap fold order-independence (T-SCORE-DET)
   weight_is_pure        | pure fn of resolved main-parent weight_map
   bfs_visits_once       | `visited` HashSet dedup (estimator.rs:255-258)
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

From ForkChoice Require Import Foundation.

(* ===========================================================================
   Section 1 - Per-validator weight (pure function of the resolved main parent)
   =========================================================================== *)

(* v's bond in a weight map (0 if absent), mirroring `.get(v).unwrap_or(0)`. *)
Fixpoint bond_of (bonds : list (Validator * nat)) (v : Validator) : nat :=
  match bonds with
  | [] => 0
  | (v', w) :: rest => if Nat.eqb v' v then w else bond_of rest v
  end.

(* The block whose bond map `weight` reads: the main parent if any, else the
   block itself (genesis). Exactly `weight_from_validator_by_dag`'s dispatch. *)
Definition resolve_main (d : DAG) (h : BlockHash) : option Block :=
  match lookup d h with
  | None => None
  | Some b =>
      match blk_main_parent b with
      | Some ph => lookup d ph
      | None    => Some b
      end
  end.

Definition weight (d : DAG) (h : BlockHash) (v : Validator) : nat :=
  match resolve_main d h with
  | Some p => bond_of (blk_bonds p) v
  | None   => 0
  end.

(* T-WEIGHT-PURE: weight depends ONLY on the resolved main-parent block. Two
   DAGs that resolve `h`'s main parent identically compute identical weight -
   the node-determinism of the score's per-term contribution. NON-vacuous: the
   two DAGs may differ arbitrarily elsewhere. *)
Lemma weight_is_pure :
  forall d1 d2 h v,
    resolve_main d1 h = resolve_main d2 h ->
    weight d1 h v = weight d2 h v.
Proof.
  intros d1 d2 h v H. unfold weight. rewrite H. reflexivity.
Qed.

(* ===========================================================================
   Section 2 - Supporting chain and the score fold
   =========================================================================== *)

(* `b` supports latest-message `h`: b is an ancestor-or-self of h (in h's past),
   so h's validator's weight lands on b during the supporting-chain BFS. *)
Definition supports (d : DAG) (h : BlockHash) (b : BlockHash) : Prop :=
  anc_of d b h.

(* One latest-message entry's contribution to block `b`'s score: the validator's
   weight if b supports the entry's message, else 0. The `visited` HashSet in
   the Rust guarantees each block is credited at most ONCE per validator - here
   that is definitional (a single term per entry), see `bfs_visits_once`. *)
Definition contrib (d : DAG) (fuel : nat) (b : BlockHash)
                   (e : Validator * BlockHash) : nat :=
  if anc_ofb d fuel b (snd e) then weight d b (fst e) else 0.

(* Block b's fork-choice score = sum over latest messages of their contribution. *)
Definition build_scores (d : DAG) (fuel : nat)
                        (lms : list (Validator * BlockHash)) (b : BlockHash) : nat :=
  fold_right (fun e acc => contrib d fuel b e + acc) 0 lms.

Lemma build_scores_cons :
  forall d fuel e l b,
    build_scores d fuel (e :: l) b = contrib d fuel b e + build_scores d fuel l b.
Proof. reflexivity. Qed.

(* T-SCORE-DET: the score is invariant under the fold order of latest messages.
   Uses only Nat.add commutativity/associativity (mirrors Merge.merge_perm). No
   fork can arise from the nondeterministic HashMap iteration order. *)
Lemma score_perm_invariant :
  forall d fuel lms lms' b,
    Permutation lms lms' -> build_scores d fuel lms b = build_scores d fuel lms' b.
Proof.
  intros d fuel lms lms' b H.
  induction H as [| x l l' Hp IH | x y l | l1 l2 l3 Hp1 IH1 Hp2 IH2].
  - reflexivity.
  - rewrite !build_scores_cons. rewrite IH. reflexivity.
  - rewrite !build_scores_cons. lia.
  - rewrite IH1, IH2. reflexivity.
Qed.

(* score = sum of the supporting validators' weights: non-supporting entries
   contribute nothing and are excluded; supporting ones contribute exactly their
   weight. The definitional content of "cumulative supporter weight". *)
Lemma score_eq_support_sum :
  forall d fuel lms b,
    build_scores d fuel lms b
    = fold_right Nat.add 0
        (map (fun e => weight d b (fst e))
             (filter (fun e => anc_ofb d fuel b (snd e)) lms)).
Proof.
  intros d fuel lms b. induction lms as [| e l IH].
  - reflexivity.
  - rewrite build_scores_cons. unfold contrib. cbn [filter].
    destruct (anc_ofb d fuel b (snd e)) eqn:E; cbn [map fold_right]; rewrite IH; lia.
Qed.

(* T-DEDUP: each latest-message entry credits block b either nothing or exactly
   the validator's weight - never a multiple. The `visited` set's no-double-count
   guarantee, here folded into the definition (one term per entry). *)
Lemma bfs_visits_once :
  forall d fuel b e,
    contrib d fuel b e = 0 \/ contrib d fuel b e = weight d b (fst e).
Proof.
  intros d fuel b e. unfold contrib.
  destruct (anc_ofb d fuel b (snd e)); [right | left]; reflexivity.
Qed.
