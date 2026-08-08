(* ===========================================================================
   Foundation.v - DAG, block numbers, the main-parent spine, and ancestry.

   Models the immutable structural facts of a block that the finalized floor
   is derived from: its block number, its main parent (parents[0]), and its
   full parent list. The finalized-floor walk (parent_frontier, floor.rs:351)
   descends the main-parent SPINE, so its termination rests on the spine being
   finite - which follows from block numbers strictly decreasing along the
   main parent. That is theorem T-TERM below.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq Definition   | Paper / Spec Notation      | Rust Implementation
   ------------------+----------------------------+--------------------------
   Block             | B in the DAG               | BlockMessage (proto)
   blk_num           | num(B)                     | b.body.state.block_number
   blk_main_parent   | main-parent, spine <|      | parents_hash_list[0]
   blk_parents       | parents(B), DAG <=         | b.header.parents_hash_list
   walk_spine        | parent_frontier main walk  | floor.rs:351-375 (main loop)
   spine_walk_terminates | T-TERM (finite spine)  | floor.rs:60 DEEP_WALK_WARN
   ---------------------------------------------------------------------------

   Companion doc: docs/theory/finalized-floor/finalized-floor-verification.md
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Arith.Wf_nat.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
Import ListNotations.

Set Implicit Arguments.

(* ===========================================================================
   Section 1 - Blocks and the DAG
   =========================================================================== *)

Definition BlockHash := nat.
Definition Validator := nat.

Record Block : Type := mkBlock {
  blk_hash        : BlockHash;
  blk_num         : nat;
  blk_main_parent : option BlockHash;              (* None = genesis *)
  blk_parents     : list BlockHash;                (* index 0 = main parent *)
  blk_just        : list (Validator * BlockHash)   (* frozen justification snapshot *)
}.

(* A DAG is a finite list of blocks; lookup is by hash. *)
Definition DAG := list Block.

Fixpoint lookup (d : DAG) (h : BlockHash) : option Block :=
  match d with
  | [] => None
  | b :: rest => if Nat.eqb (blk_hash b) h then Some b else lookup rest h
  end.

Lemma lookup_In :
  forall d h b, lookup d h = Some b -> In b d.
Proof.
  induction d as [| x rest IH]; simpl; intros h b Hl.
  - discriminate.
  - destruct (Nat.eqb (blk_hash x) h) eqn:E.
    + inversion Hl; subst. left; reflexivity.
    + right; eapply IH; eauto.
Qed.

(* ===========================================================================
   Section 2 - Well-formed main-parent spine

   Every non-genesis block's main parent is present in the DAG with a strictly
   smaller block number; genesis (no main parent) has block number 0. These are
   structural facts of a signed DAG: parents are older, and block numbers are a
   height. (floor.rs relies on exactly this to terminate its spine walk.)
   =========================================================================== *)

Definition wf_spine (d : DAG) : Prop :=
  forall b, In b d ->
    match blk_main_parent b with
    | None    => blk_num b = 0
    | Some ph => exists p, lookup d ph = Some p /\ blk_num p < blk_num b
    end.

(* ===========================================================================
   Section 3 - The main-parent spine walk (models parent_frontier's descent)

   A fuel-bounded walk down the main-parent spine. `parent_frontier` walks down
   from the tip; here we model the pure descent (the finalized-cut selection is
   layered on top in Floor.v). The walk stops at genesis (main_parent = None).
   =========================================================================== *)

Fixpoint walk_spine (d : DAG) (b : Block) (fuel : nat) : option Block :=
  match blk_main_parent b with
  | None => Some b                                  (* genesis: terminate *)
  | Some ph =>
      match fuel with
      | 0 => None
      | S fuel' =>
          match lookup d ph with
          | None => None
          | Some p => walk_spine d p fuel'
          end
      end
  end.

(* One-step unfolding, so proofs never fight the fixpoint reduction. *)
Lemma walk_spine_eq :
  forall d b fuel,
    walk_spine d b fuel =
      match blk_main_parent b with
      | None => Some b
      | Some ph =>
          match fuel with
          | 0 => None
          | S fuel' =>
              match lookup d ph with
              | None => None
              | Some p => walk_spine d p fuel'
              end
          end
      end.
Proof. intros d b fuel; destruct fuel; reflexivity. Qed.

(* Three reduction lemmas so the walk proofs never fight iota on the fixpoint. *)
Lemma walk_spine_genesis :
  forall d fuel b, blk_main_parent b = None -> walk_spine d b fuel = Some b.
Proof. intros d fuel b H. rewrite walk_spine_eq, H. reflexivity. Qed.

Lemma walk_spine_stuck0 :
  forall d b ph, blk_main_parent b = Some ph -> walk_spine d b 0 = None.
Proof. intros d b ph H. rewrite walk_spine_eq, H. reflexivity. Qed.

Lemma walk_spine_step :
  forall d fuel b ph p,
    blk_main_parent b = Some ph -> lookup d ph = Some p ->
    walk_spine d b (S fuel) = walk_spine d p fuel.
Proof.
  intros d fuel b ph p Hmp Hl.
  rewrite walk_spine_eq, Hmp. simpl. rewrite Hl. reflexivity.
Qed.

(* More fuel never changes a terminating result: once the walk reaches genesis,
   extra fuel is ignored. *)
Lemma walk_spine_fuel_monotone :
  forall d f b g,
    walk_spine d b f = Some g ->
    forall f', f <= f' -> walk_spine d b f' = Some g.
Proof.
  induction f as [| f IH]; intros b g Hw f' Hle.
  - (* f = 0 *)
    destruct (blk_main_parent b) as [b0 |] eqn:Emp.
    + rewrite (@walk_spine_stuck0 d b b0 Emp) in Hw; discriminate Hw.
    + rewrite (@walk_spine_genesis d 0 b Emp) in Hw.
      rewrite (@walk_spine_genesis d f' b Emp). exact Hw.
  - (* f = S f *)
    destruct (blk_main_parent b) as [b0 |] eqn:Emp.
    + destruct (lookup d b0) as [p |] eqn:El.
      * rewrite (@walk_spine_step d f b b0 p Emp El) in Hw.
        destruct f' as [| f''].
        -- exfalso; lia.
        -- rewrite (@walk_spine_step d f'' b b0 p Emp El). apply (IH p g Hw). lia.
      * rewrite walk_spine_eq, Emp, El in Hw; discriminate Hw.
    + rewrite (@walk_spine_genesis d (S f) b Emp) in Hw.
      rewrite (@walk_spine_genesis d f' b Emp). exact Hw.
Qed.

(* ===========================================================================
   Section 4 - T-TERM : the spine walk terminates at genesis

   With fuel = blk_num b the walk always reaches a genesis block. Hence the
   main-parent spine is finite and the floor derivation's descent (which H2
   makes O(Delta^2) but which is nonetheless finite) always terminates. The
   height blk_num b is a concrete termination bound - the DEEP_WALK_WARN
   threshold in floor.rs:60 is thus visibility-only, never a correctness cap.
   =========================================================================== *)

Theorem spine_walk_terminates :
  forall d, wf_spine d ->
  forall b, In b d ->
  exists g, walk_spine d b (blk_num b) = Some g /\ blk_main_parent g = None.
Proof.
  intros d Hwf.
  (* strong induction on the block number *)
  assert (AUX : forall n b, blk_num b = n -> In b d ->
            exists g, walk_spine d b (blk_num b) = Some g /\ blk_main_parent g = None).
  { intros n. induction n as [n IH] using lt_wf_ind.
    intros b Hnum Hin. subst n.
    destruct (blk_main_parent b) as [b0 |] eqn:Emp.
    - (* non-genesis: main parent p has strictly smaller number *)
      pose proof (Hwf b Hin) as Hwfb. rewrite Emp in Hwfb.
      destruct Hwfb as [p [Hlp Hlt]].
      assert (Hinp : In p d) by (eapply lookup_In; eauto).
      (* blk_num b >= 1 since it exceeds blk_num p *)
      destruct (blk_num b) as [| m] eqn:Eb; [lia |].
      (* by IH on blk_num p < blk_num b, the walk from p with fuel (blk_num p) ends *)
      destruct (IH (blk_num p) ltac:(lia) p eq_refl Hinp) as [g [Hwg Hgmp]].
      exists g. split; [| exact Hgmp].
      rewrite (@walk_spine_step d m b b0 p Emp Hlp).
      (* lift fuel from blk_num p to m = blk_num b - 1 >= blk_num p *)
      eapply walk_spine_fuel_monotone; [exact Hwg | lia].
    - (* genesis: returns immediately *)
      exists b. split; [ apply (@walk_spine_genesis d (blk_num b) b Emp) | exact Emp ]. }
  intros b Hin. eapply AUX; eauto.
Qed.
