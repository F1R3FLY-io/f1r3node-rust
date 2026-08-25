(* ═══════════════════════════════════════════════════════════════════════════
   GuardParity.v — check_commit guard parity between play and replay

   Mechanizes CLAIM-RSPACE-001 (docs/claims/rspace-check-commit-play-replay.md):
   the cross-channel `where`-guard veto (Match::check_commit) is consulted on
   every COMM-forming path in the play tuple space, and replay of a play event
   log reproduces the identical COMM sequence.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition        │ Claim │ Rust Implementation
   ───────────────────────┼───────┼────────────────────────────────────────
   guard_eval (Variable)  │ C3    │ rholang/.../matcher/match.rs::check_commit
                          │       │ (guard_passes via rho-pure-eval; purity
                          │       │ and bind-order agreement are the seam
                          │       │ premises Rust enforces)
   first_match            │ C1    │ rspace++/src/rspace/space_matcher.rs:161
                          │       │ (extract_first_match: guard veto rolls
                          │       │ back and continues, like a spatial miss)
   consume_commits        │ C1    │ rspace++/src/rspace/rspace/ops_consume.rs:79
                          │       │ (locked_consume commit_ok block)
   play_from              │ C1    │ play COMM formation (consume + produce)
   replay_from            │ C2/D2 │ rspace++/src/rspace/replay_rspace.rs:
                          │       │ produce path re-runs extract_first_match
                          │       │ (1383-1403); consume path is log-gated
                          │       │ (run_matcher_consume 787-811 filters
                          │       │ candidates to a recorded COMM); both are
                          │       │ keyed by op identity (replay_data map)
   OpInstall (no commit)  │ D1    │ locked_install_internal: never forms a
                          │       │ COMM (a spatial match is an error)
   ─────────────────────────────────────────────────────────────────────────

   Modeling notes.
   - Guard determinism (C3) is BY CONSTRUCTION: play and replay apply the
     same Rocq function [guard_eval]. The Section Variable is a premise of
     the closed theorem, not an axiom.
   - COMMs are keyed by op index (comm_id), mirroring replay_data's keyed
     map: replay decides per-op from the log, not positionally.
   - Replay's consume path does NOT evaluate the guard (the D2 asymmetry);
     the main theorem shows the log gate substitutes for it exactly.

   Zero `Admitted`. No custom `Axiom` or `Parameter`.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
From Stdlib Require Import Lists.List.
From Stdlib Require Import PeanoNat.
Import ListNotations.

Section GuardParity.

Variable Data : Type.
Variable Guard : Type.

(* C3 premise: guard evaluation is a pure, total function of the guard and
   the matched data in receive-bind order. *)
Variable guard_eval : Guard -> list Data -> bool.

(* One waiting-continuation candidate: whether its spatial match succeeds,
   its guard, and the data the match would bind. *)
Record Candidate := mkCandidate {
  cand_spatial : bool;
  cand_guard : Guard;
  cand_data : list Data
}.

(* extract_first_match: first candidate that matches spatially AND passes
   its guard; a guard veto is skipped exactly like a spatial miss. *)
Fixpoint first_match (cs : list Candidate) : option Candidate :=
  match cs with
  | [] => None
  | c :: rest =>
      if cand_spatial c && guard_eval (cand_guard c) (cand_data c)
      then Some c
      else first_match rest
  end.

Lemma first_match_nil : first_match [] = None.
Proof. reflexivity. Qed.

Lemma first_match_cons :
  forall c rest,
    first_match (c :: rest) =
    if cand_spatial c && guard_eval (cand_guard c) (cand_data c)
    then Some c
    else first_match rest.
Proof. reflexivity. Qed.

Arguments first_match : simpl never.

(* C1, produce side: a selected candidate passed both checks. *)
Theorem first_match_guard_passes :
  forall cs c,
    first_match cs = Some c ->
    cand_spatial c = true /\ guard_eval (cand_guard c) (cand_data c) = true.
Proof.
  induction cs as [| a rest IH]; intros c H.
  - rewrite first_match_nil in H. discriminate.
  - rewrite first_match_cons in H.
    destruct (cand_spatial a && guard_eval (cand_guard a) (cand_data a)) eqn:E.
    + injection H as <-. apply andb_true_iff in E. exact E.
    + apply IH. exact H.
Qed.

(* C1, consume side: locked_consume's commit_ok. *)
Definition consume_commits (spatial_ok : bool) (g : Guard) (bound : list Data)
  : bool :=
  spatial_ok && guard_eval g bound.

Arguments consume_commits : simpl never.

Theorem consume_commit_guard_passes :
  forall s g d, consume_commits s g d = true -> guard_eval g d = true.
Proof.
  intros s g d H. unfold consume_commits in H.
  apply andb_true_iff in H. apply H.
Qed.

(* ── Operations and the event log ──────────────────────────────────────── *)

Inductive Op : Type :=
  | OpProduce (cs : list Candidate)
  | OpConsume (spatial_ok : bool) (g : Guard) (bound : list Data)
  | OpInstall.

Record Comm := mkComm {
  comm_id : nat;
  comm_guard : Guard;
  comm_data : list Data
}.

Definition has_id (i : nat) (cm : Comm) : bool := Nat.eqb (comm_id cm) i.

Lemma existsb_id_head_hit :
  forall i g d l, existsb (has_id i) (mkComm i g d :: l) = true.
Proof.
  intros i g d l. simpl. unfold has_id. simpl.
  rewrite Nat.eqb_refl. reflexivity.
Qed.

Lemma existsb_id_head_skip :
  forall j i g d l,
    i <> j ->
    existsb (has_id j) (mkComm i g d :: l) = existsb (has_id j) l.
Proof.
  intros j i g d l Hne. simpl. unfold has_id. simpl.
  apply Nat.eqb_neq in Hne. rewrite Hne. reflexivity.
Qed.

(* Play: commits iff spatial match AND guard pass; installs never commit. *)
Fixpoint play_from (i : nat) (ops : list Op) : list Comm :=
  match ops with
  | [] => []
  | OpProduce cs :: rest =>
      match first_match cs with
      | Some c => mkComm i (cand_guard c) (cand_data c) :: play_from (S i) rest
      | None => play_from (S i) rest
      end
  | OpConsume s g d :: rest =>
      if consume_commits s g d
      then mkComm i g d :: play_from (S i) rest
      else play_from (S i) rest
  | OpInstall :: rest => play_from (S i) rest
  end.

(* Replay: the produce path is log-gated AND re-runs first_match (guard
   re-evaluated); the consume path is log-gated ONLY — it never evaluates
   the guard (deviation D2). *)
Fixpoint replay_from (i : nat) (ops : list Op) (log : list Comm) : list Comm :=
  match ops with
  | [] => []
  | OpProduce cs :: rest =>
      if existsb (has_id i) log
      then
        match first_match cs with
        | Some c =>
            mkComm i (cand_guard c) (cand_data c) :: replay_from (S i) rest log
        | None => replay_from (S i) rest log
        end
      else replay_from (S i) rest log
  | OpConsume s g d :: rest =>
      if existsb (has_id i) log
      then mkComm i g d :: replay_from (S i) rest log
      else replay_from (S i) rest log
  | OpInstall :: rest => replay_from (S i) rest log
  end.

(* ── Helper lemmas ─────────────────────────────────────────────────────── *)

Lemma play_id_lower_bound :
  forall ops i cm, In cm (play_from i ops) -> i <= comm_id cm.
Proof.
  induction ops as [| op rest IH]; intros i cm H.
  - cbn in H. contradiction.
  - destruct op as [cs | s g d |]; cbn [play_from] in H.
    + destruct (first_match cs) eqn:FM; cbn beta iota in H.
      * destruct H as [<- | H]; cbn [comm_id]; [lia | apply IH in H; lia].
      * apply IH in H; lia.
    + destruct (consume_commits s g d) eqn:CC; cbn beta iota in H.
      * destruct H as [<- | H]; cbn [comm_id]; [lia | apply IH in H; lia].
      * apply IH in H; lia.
    + apply IH in H; lia.
Qed.

Lemma play_no_lower_id :
  forall ops i j, j < i -> existsb (has_id j) (play_from i ops) = false.
Proof.
  intros ops i j Hlt.
  destruct (existsb (has_id j) (play_from i ops)) eqn:E; [exfalso | reflexivity].
  apply existsb_exists in E as (cm & Hin & Hid).
  unfold has_id in Hid. apply Nat.eqb_eq in Hid.
  apply play_id_lower_bound in Hin. lia.
Qed.

(* ── Main theorems ─────────────────────────────────────────────────────── *)

(* C1: every COMM play records passed its guard. *)
Theorem play_guard_complete :
  forall ops i cm,
    In cm (play_from i ops) ->
    guard_eval (comm_guard cm) (comm_data cm) = true.
Proof.
  induction ops as [| op rest IH]; intros i cm H.
  - cbn in H. contradiction.
  - destruct op as [cs | s g d |]; cbn [play_from] in H.
    + destruct (first_match cs) eqn:FM; cbn beta iota in H.
      * destruct H as [<- | H]; cbn [comm_guard comm_data].
        -- apply first_match_guard_passes in FM. apply FM.
        -- eapply IH. exact H.
      * eapply IH. exact H.
    + destruct (consume_commits s g d) eqn:CC; cbn beta iota in H.
      * destruct H as [<- | H]; cbn [comm_guard comm_data].
        -- eapply consume_commit_guard_passes. exact CC.
        -- eapply IH. exact H.
      * eapply IH. exact H.
    + eapply IH. exact H.
Qed.

(* D2: replay commits only COMMs whose op identity the log records. *)
Theorem replay_log_gated :
  forall ops i log cm,
    In cm (replay_from i ops log) ->
    exists cm', In cm' log /\ comm_id cm' = comm_id cm.
Proof.
  induction ops as [| op rest IH]; intros i log cm H.
  - cbn in H. contradiction.
  - destruct op as [cs | s g d |]; cbn [replay_from] in H.
    + destruct (existsb (has_id i) log) eqn:E; cbn beta iota in H.
      * destruct (first_match cs) eqn:FM; cbn beta iota in H.
        -- destruct H as [<- | H].
           ++ apply existsb_exists in E as (cm' & Hin & Hid).
              unfold has_id in Hid. apply Nat.eqb_eq in Hid.
              exists cm'. split; [exact Hin | cbn [comm_id]; exact Hid].
           ++ eapply IH. exact H.
        -- eapply IH. exact H.
      * eapply IH. exact H.
    + destruct (existsb (has_id i) log) eqn:E; cbn beta iota in H.
      * destruct H as [<- | H].
        -- apply existsb_exists in E as (cm' & Hin & Hid).
           unfold has_id in Hid. apply Nat.eqb_eq in Hid.
           exists cm'. split; [exact Hin | cbn [comm_id]; exact Hid].
        -- eapply IH. exact H.
      * eapply IH. exact H.
    + eapply IH. exact H.
Qed.

(* The generalized induction behind C2: replay agrees with play whenever the
   log answers id-membership queries at indices >= i exactly as play's own
   log does. Replay only ever queries the current index and recurses at S i,
   so this hypothesis is exactly what the recursion consumes. *)
Lemma replay_play_general :
  forall ops i log,
    (forall j,
        i <= j ->
        existsb (has_id j) log = existsb (has_id j) (play_from i ops)) ->
    replay_from i ops log = play_from i ops.
Proof.
  induction ops as [| op rest IH]; intros i log Hlog.
  - reflexivity.
  - destruct op as [cs | s g d |].
    + (* produce *)
      pose proof (Hlog i (Nat.le_refl i)) as Hi.
      cbn [play_from] in Hi.
      destruct (first_match cs) eqn:FM; cbn beta iota in Hi.
      * (* play commits at i *)
        rewrite existsb_id_head_hit in Hi.
        cbn [replay_from play_from]. rewrite FM, Hi.
        cbn beta iota. f_equal.
        apply IH. intros j Hj.
        rewrite (Hlog j) by lia. cbn [play_from]. rewrite FM.
        cbn beta iota.
        rewrite existsb_id_head_skip by lia. reflexivity.
      * (* play skips at i *)
        rewrite (play_no_lower_id rest (S i) i) in Hi by lia.
        cbn [replay_from play_from]. rewrite FM, Hi.
        cbn beta iota.
        apply IH. intros j Hj.
        rewrite (Hlog j) by lia. cbn [play_from]. rewrite FM.
        cbn beta iota. reflexivity.
    + (* consume *)
      pose proof (Hlog i (Nat.le_refl i)) as Hi.
      cbn [play_from] in Hi.
      destruct (consume_commits s g d) eqn:CC; cbn beta iota in Hi.
      * rewrite existsb_id_head_hit in Hi.
        cbn [replay_from play_from]. rewrite CC, Hi.
        cbn beta iota. f_equal.
        apply IH. intros j Hj.
        rewrite (Hlog j) by lia. cbn [play_from]. rewrite CC.
        cbn beta iota.
        rewrite existsb_id_head_skip by lia. reflexivity.
      * rewrite (play_no_lower_id rest (S i) i) in Hi by lia.
        cbn [replay_from play_from]. rewrite CC, Hi.
        cbn beta iota.
        apply IH. intros j Hj.
        rewrite (Hlog j) by lia. cbn [play_from]. rewrite CC.
        cbn beta iota. reflexivity.
    + (* install: never commits in either space *)
      cbn [replay_from play_from]. apply IH. intros j Hj.
      rewrite (Hlog j) by lia. cbn [play_from]. reflexivity.
Qed.

(* C2: replay of a play log reproduces the identical COMM sequence. The
   consume-side guard asymmetry (D2) is discharged: the log gate substitutes
   exactly for the guard verdict play already took. *)
Theorem replay_equiv :
  forall ops, replay_from 0 ops (play_from 0 ops) = play_from 0 ops.
Proof.
  intros ops.
  apply replay_play_general.
  intros j _. reflexivity.
Qed.

(* Corollary: every COMM replay commits (from a play log) passed its guard —
   even though replay's consume path never evaluates guards. *)
Corollary replay_guard_complete :
  forall ops cm,
    In cm (replay_from 0 ops (play_from 0 ops)) ->
    guard_eval (comm_guard cm) (comm_data cm) = true.
Proof.
  intros ops cm H.
  rewrite replay_equiv in H.
  eapply play_guard_complete. exact H.
Qed.

End GuardParity.
