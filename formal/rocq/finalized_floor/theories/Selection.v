(* ===========================================================================
   Selection.v - The finalized-floor SOUND-BASE selection (T-SOUND, T-PS,
   T-FIN, T-LIN, T-COMM) and the H3 scope-coverage lemma.

   Models `derive_floor`'s Case-A / Case-B pick (casper/src/rust/finality/floor.rs
   :108-194): from the candidate set (inherited parent floors ∪ per-parent
   frontiers), ordered highest-first, choose the highest candidate that is a
   SOUND merge base, where a candidate `c` is sound when EITHER

     Case A: `c` is a general DAG-ancestor of (or equals) EVERY parent
             (`covers_all_parents`); OR
     Case B: every OTHER candidate is compatible with `c` — it is `c` itself, it
             lies in `c`'s DAG past, or it is mergeable with `c` via a common
             descendant parent (`all_compatible`).

   If NO candidate is sound, `derive_floor` returns `Err` (an incompatible
   finalized fork) — never a papered-over unsound base.

   Proved here (all axiom-free, Stdlib-only):
     * T-SOUND  — the chosen base is sound; and `None` ⇒ no candidate is sound
                  (so the `Err` path is correct, not a silent unsound merge).
     * T-SOUND-A / T-LIN — a Case-A base is a common DAG-ancestor of every parent
                  (a shared lower bound = the parents' finalized cut lies on one
                  chain below the base).
     * T-PS     — selection is a total, deterministic pure function of ANY parent
                  list (parent selection modeled as an unconstrained oracle); the
                  soundness / none-correctness guarantees carry no precondition on
                  which parents were chosen.
     * T-FIN    — the chosen base is drawn from the candidate set, so if every
                  candidate is finalized (the frontiers are), so is the base.
     * H3-cov   — with a common-ancestor base, every block between the base and a
                  parent is at height ≥ the base and reachable from that parent,
                  hence in the merge scope: the floor-bounded scan drops no parent
                  write in [floor, tip].
     * T-COMM   — the authorization committee is `bonds_of(floor)`, a pure
                  function of the deterministic floor — never a node-local
                  view or the candidate's post-state cache (S8).

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                     | Rust (casper/src/rust/finality/floor.rs)
   -------------------------+----------------------------------------------
   anc_ofb / anc_of         | dag.is_dag_ancestor (bdkvs.rs:517)
   case_a                   | covers_all_parents loop (:138-148)
   case_b                   | all_compatible loop (:150-169)
   is_sound                 | Case A ∨ Case B
   select_floor             | the `chosen`/`ordered` top-down pick (:128-181)
   select_none_correct      | the "no sound base ⇒ Err" branch (:162-182)
   scope_covers_band        | floor-bounded ancestor scan (interpreter_util.rs H3)
   committee_used           | authority_committee_for_evidence +
                            | Validate::floor_authority
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lia.
Import ListNotations.

From FinalizedFloor Require Import Foundation.
From FinalizedFloor Require Import CliqueOracle.

(* ===========================================================================
   Section 1 - Block numbers by hash, and a decidable ancestry check
   =========================================================================== *)

Definition numof (d : DAG) (h : BlockHash) : nat :=
  match lookup d h with
  | Some b => blk_num b
  | None => 0
  end.

(* Fuel-bounded boolean general-DAG ancestry (models is_dag_ancestor). We only
   need SOUNDNESS (true ⇒ anc_of), which holds for any fuel. *)
Fixpoint anc_ofb (d : DAG) (fuel : nat) (a x : BlockHash) : bool :=
  if Nat.eqb a x then true
  else match fuel with
       | 0 => false
       | S f =>
           match lookup d x with
           | None => false
           | Some b => existsb (fun p => anc_ofb d f a p) (blk_parents b)
           end
       end.

Lemma anc_ofb_sound :
  forall d fuel a x, anc_ofb d fuel a x = true -> anc_of d a x.
Proof.
  intros d fuel. induction fuel as [| f IH]; intros a x H; simpl in H.
  - destruct (Nat.eqb a x) eqn:E.
    + apply Nat.eqb_eq in E. subst x. apply anc_refl.
    + discriminate.
  - destruct (Nat.eqb a x) eqn:E.
    + apply Nat.eqb_eq in E. subst x. apply anc_refl.
    + destruct (lookup d x) as [b |] eqn:Elk; [| discriminate].
      apply existsb_exists in H. destruct H as [p [Hin Hp]].
      eapply anc_par; [exact Elk | exact Hin | apply IH; exact Hp].
Qed.

(* ===========================================================================
   Section 2 - The sound-base predicate and the selection function
   =========================================================================== *)

(* Case A: `c` is (or is a DAG-ancestor of) every parent. *)
Definition case_a (d : DAG) (fuel : nat) (parents : list BlockHash) (c : BlockHash) : bool :=
  forallb (fun p => Nat.eqb c p || anc_ofb d fuel c p) parents.

(* Case B: every other candidate is compatible with `c` (it is `c`, lies in `c`'s
   past, or is mergeable with `c` via a common descendant parent). *)
Definition case_b (d : DAG) (fuel : nat) (parents cands : list BlockHash) (c : BlockHash) : bool :=
  forallb
    (fun o =>
       Nat.eqb o c
       || anc_ofb d fuel o c
       || existsb (fun p => anc_ofb d fuel o p && anc_ofb d fuel c p) parents)
    cands.

Definition is_sound (d : DAG) (fuel : nat) (parents cands : list BlockHash) (c : BlockHash) : bool :=
  case_a d fuel parents c || case_b d fuel parents cands c.

(* The selection: the highest sound candidate, else None. `cands` is ordered
   highest-first by the caller; `find` returns the first (= highest) sound one.
   Soundness and none-correctness below do not depend on the ordering. *)
Definition select_floor (d : DAG) (fuel : nat) (parents cands : list BlockHash) : option BlockHash :=
  find (fun c => is_sound d fuel parents cands c) cands.

(* ===========================================================================
   Section 3 - T-SOUND : the chosen base is sound; None ⇒ no sound base
   =========================================================================== *)

Theorem select_sound :
  forall d fuel parents cands f,
    select_floor d fuel parents cands = Some f ->
    is_sound d fuel parents cands f = true.
Proof.
  intros d fuel parents cands f H. unfold select_floor in H.
  apply find_some in H. destruct H as [_ Hsound]. exact Hsound.
Qed.

Theorem select_in :
  forall d fuel parents cands f,
    select_floor d fuel parents cands = Some f -> In f cands.
Proof.
  intros d fuel parents cands f H. unfold select_floor in H.
  apply find_some in H. destruct H as [Hin _]. exact Hin.
Qed.

(* None ⇒ NO candidate is sound: the incompatible-fork Err is correct, never a
   silently-substituted unsound base. *)
Theorem select_none_correct :
  forall d fuel parents cands,
    select_floor d fuel parents cands = None ->
    forall c, In c cands -> is_sound d fuel parents cands c = false.
Proof.
  intros d fuel parents cands H c Hin. unfold select_floor in H.
  eapply find_none; [exact H | exact Hin].
Qed.

(* ===========================================================================
   Section 4 - T-SOUND-A / T-LIN : a Case-A base is a common ancestor
   =========================================================================== *)

Theorem case_a_common_ancestor :
  forall d fuel parents c,
    case_a d fuel parents c = true ->
    forall p, In p parents -> c = p \/ anc_of d c p.
Proof.
  intros d fuel parents c H p Hin. unfold case_a in H.
  rewrite forallb_forall in H. specialize (H p Hin).
  apply orb_true_iff in H. destruct H as [Heq | Hanc].
  - left. apply Nat.eqb_eq in Heq. exact Heq.
  - right. apply (anc_ofb_sound d fuel c p Hanc).
Qed.

(* The chosen floor, when it is a Case-A base, is a common DAG-ancestor (a shared
   lower bound) of the whole parent set — the parents' finalized cut lies on one
   chain below it (T-LIN's "one chain" content at the floor). *)
Corollary select_caseA_common_ancestor :
  forall d fuel parents cands f,
    select_floor d fuel parents cands = Some f ->
    case_a d fuel parents f = true ->
    forall p, In p parents -> f = p \/ anc_of d f p.
Proof.
  intros d fuel parents cands f _ Hca p Hin.
  apply (case_a_common_ancestor d fuel parents f Hca p Hin).
Qed.

(* ===========================================================================
   Section 5 - T-PS : safety holds for ANY parent list (unconstrained oracle)

   `select_floor` places no precondition on the parent set: it is total and
   deterministic (a pure Coq function) for every `parents : list BlockHash`, and
   both guarantees (sound-if-Some, no-sound-if-None) are proved ∀ parents. This
   bundles that fact — the parent set is an unconstrained oracle.
   =========================================================================== *)

Theorem T_PS :
  forall d fuel parents cands,
    (forall f, select_floor d fuel parents cands = Some f ->
       is_sound d fuel parents cands f = true) /\
    (select_floor d fuel parents cands = None ->
       forall c, In c cands -> is_sound d fuel parents cands c = false).
Proof.
  intros d fuel parents cands. split.
  - intros f Hf. apply (select_sound d fuel parents cands f Hf).
  - intros Hnone c Hin. apply (select_none_correct d fuel parents cands Hnone c Hin).
Qed.

(* Determinism: two evaluations on the same inputs agree (it is a function).
   Combined with the floor being a pure function of the block's frozen data,
   every honest node selects the same floor. *)
Theorem select_deterministic :
  forall d fuel parents cands r1 r2,
    select_floor d fuel parents cands = r1 ->
    select_floor d fuel parents cands = r2 ->
    r1 = r2.
Proof. intros d fuel parents cands r1 r2 H1 H2. rewrite <- H1, <- H2. reflexivity. Qed.

(* ===========================================================================
   Section 6 - T-FIN : the chosen base is finalized

   Parametric in a finalization predicate `Fin` (instantiated at use with
   `CliqueOracle.Finalized d committee J`). The candidate set is the parent
   frontiers, each finalized; the chosen base is a member, hence finalized.
   =========================================================================== *)

Theorem select_finalized :
  forall (Fin : BlockHash -> Prop) d fuel parents cands f,
    Forall Fin cands ->
    select_floor d fuel parents cands = Some f ->
    Fin f.
Proof.
  intros Fin d fuel parents cands f Hall Hsel.
  rewrite Forall_forall in Hall.
  apply Hall. apply (select_in d fuel parents cands f Hsel).
Qed.

(* ===========================================================================
   Section 7 - H3 scope-coverage : the floor-bounded scan drops no parent write

   Well-formed block numbers: every parent edge strictly decreases the number
   (the general-DAG analogue of Foundation.wf_spine). Then ancestry never raises
   the number, so any block between the floor and a parent sits at height ≥ the
   floor and is reachable from that parent — i.e. inside the merge scope
   { w : reachable from a parent ∧ num w ≥ num floor }. Nothing in [floor, tip]
   is excluded (the H3 fix vs the old config-depth cut).
   =========================================================================== *)

Definition wf_dag_num (d : DAG) : Prop :=
  forall x b p, lookup d x = Some b -> In p (blk_parents b) -> numof d p < numof d x.

Lemma anc_of_num_le :
  forall d, wf_dag_num d -> forall a x, anc_of d a x -> numof d a <= numof d x.
Proof.
  intros d Hwf a x H. induction H as [x | a x b p Hlk Hin Hanc IH].
  - reflexivity.
  - assert (Hlt : numof d p < numof d x) by (apply (Hwf x b p Hlk Hin)).
    lia.
Qed.

(* The merge scope built by the floor-bounded scan. *)
Definition in_scope (d : DAG) (parents : list BlockHash) (floor : BlockHash) (w : BlockHash) : Prop :=
  (exists p, In p parents /\ anc_of d w p) /\ numof d floor <= numof d w.

(* Coverage: any block `w` on a path from the floor up to some parent is in the
   scope. So with a common-ancestor floor (Case A), every parent write above the
   floor is retained — the scan drops nothing between floor and tip. *)
Theorem scope_covers_band :
  forall d parents floor p w,
    wf_dag_num d ->
    In p parents ->
    anc_of d floor w ->
    anc_of d w p ->
    in_scope d parents floor w.
Proof.
  intros d parents floor p w Hwf Hin Hfw Hwp.
  split.
  - exists p. split; [exact Hin | exact Hwp].
  - apply (anc_of_num_le d Hwf floor w Hfw).
Qed.

(* ===========================================================================
   Section 8 - T-COMM : the committee is a pure function of the floor (S8)

   The authorization committee is `bonds_of(floor)`. Since the floor is the
   deterministic `select_floor` result and `bonds_of` reads only the floor's
   post-state, the committee is node-identical — never a node-local finalized
   view or the candidate block's post-state bond cache.
   =========================================================================== *)

Definition committee_used
  (bonds_of : BlockHash -> Committee) (d : DAG) (fuel : nat) (parents cands : list BlockHash)
  : option Committee :=
  match select_floor d fuel parents cands with
  | Some f => Some (bonds_of f)
  | None => None
  end.

Theorem committee_is_floor_bonds :
  forall bonds_of d fuel parents cands f,
    select_floor d fuel parents cands = Some f ->
    committee_used bonds_of d fuel parents cands = Some (bonds_of f).
Proof.
  intros bonds_of d fuel parents cands f H.
  unfold committee_used. rewrite H. reflexivity.
Qed.

Theorem committee_deterministic :
  forall bonds_of d fuel parents cands r1 r2,
    committee_used bonds_of d fuel parents cands = r1 ->
    committee_used bonds_of d fuel parents cands = r2 ->
    r1 = r2.
Proof. intros. subst. reflexivity. Qed.

(* ===========================================================================
   Section 9 (Phase 7) - Case-B compatibility + selection maximality
   =========================================================================== *)

(* Case-B, unpacked (mirrors case_a_common_ancestor). A Case-B base `c` is
   COMPATIBLE with every other candidate: each is `c` itself, lies in `c`'s DAG
   past, or is mergeable with `c` via a common descendant parent. This is the
   precise Case-B guarantee — weaker than Case-A common-ancestry: it establishes
   there is no incompatible finalized fork among the candidates, NOT that `c` is
   below every parent. The full no-lost-write for a Case-B floor rests on the
   merge-scope + common-descendant semantics (modeled in TLA+ FinalizedFloorScan
   / exercised by the Rust merge tests), which this predicate feeds. *)
Theorem case_b_compatible :
  forall d fuel parents cands c,
    case_b d fuel parents cands c = true ->
    forall o, In o cands ->
      o = c
      \/ anc_of d o c
      \/ (exists p, In p parents /\ anc_of d o p /\ anc_of d c p).
Proof.
  intros d fuel parents cands c H o Hin. unfold case_b in H.
  rewrite forallb_forall in H. specialize (H o Hin).
  apply orb_true_iff in H. destruct H as [H | Hex].
  - apply orb_true_iff in H. destruct H as [Heq | Hoc].
    + left. apply Nat.eqb_eq in Heq. exact Heq.
    + right. left. exact (anc_ofb_sound d fuel o c Hoc).
  - right. right. apply existsb_exists in Hex. destruct Hex as [p [Hpin Hp]].
    apply andb_true_iff in Hp. destruct Hp as [Hop Hcp].
    exists p. split; [exact Hpin |].
    split; [exact (anc_ofb_sound d fuel o p Hop) | exact (anc_ofb_sound d fuel c p Hcp)].
Qed.

(* Candidates sorted highest-first by block number (models floor.rs:156-160's
   `sort_by` descending; the hash tiebreak is omitted — it only orders equal-height
   candidates and does not affect the height-maximality below). *)
Inductive DescSorted (d : DAG) : list BlockHash -> Prop :=
  | ds_nil  : DescSorted d []
  | ds_one  : forall x, DescSorted d [x]
  | ds_cons : forall x y r,
      numof d y <= numof d x -> DescSorted d (y :: r) -> DescSorted d (x :: y :: r).

Lemma descsorted_tail : forall d x l, DescSorted d (x :: l) -> DescSorted d l.
Proof.
  intros d x l H. inversion H; subst.
  - apply ds_nil.
  - assumption.
Qed.

Lemma descsorted_head_max :
  forall d x l, DescSorted d (x :: l) -> forall c, In c l -> numof d c <= numof d x.
Proof.
  intros d x l. revert x. induction l as [| y r IH]; intros x H c Hin.
  - contradiction.
  - inversion H as [ | | x0 y0 r0 Hle Hds ]; subst.
    destruct Hin as [Heq | Hin].
    + subst c. exact Hle.
    + (* c in r : numof c <= numof y <= numof x *)
      assert (Hcy : numof d c <= numof d y) by (apply (IH y Hds c Hin)).
      apply (Nat.le_trans _ (numof d y) _ Hcy Hle).
Qed.

(* Maximality: over a descending-sorted candidate list, `find` returns the
   sound candidate of GREATEST block number — the highest sound base (the Rust
   picks the first sound one after sorting descending). Generic in the predicate
   so it applies to `is_sound … cands`. *)
Lemma find_desc_max :
  forall d (sound : BlockHash -> bool) l f,
    DescSorted d l ->
    find sound l = Some f ->
    sound f = true /\ In f l /\
    (forall c, In c l -> sound c = true -> numof d c <= numof d f).
Proof.
  intros d sound l. induction l as [| x tl IH]; intros f Hsorted Hfind.
  - simpl in Hfind. discriminate.
  - simpl in Hfind. destruct (sound x) eqn:Esx.
    + (* x is the first sound → f = x, and x has max number over x::tl *)
      injection Hfind as Hf. subst f. repeat split.
      * exact Esx.
      * left. reflexivity.
      * intros c Hin _. destruct Hin as [Heq | Hin].
        -- subst c. apply Nat.le_refl.
        -- apply (descsorted_head_max d x tl Hsorted c Hin).
    + (* x not sound → recurse on tl *)
      assert (Htl : DescSorted d tl) by (apply (descsorted_tail d x tl Hsorted)).
      destruct (IH f Htl Hfind) as [Hsf [Hinf Hmax]].
      repeat split.
      * exact Hsf.
      * right. exact Hinf.
      * intros c Hin Hsc. destruct Hin as [Heq | Hin].
        -- subst c. rewrite Esx in Hsc. discriminate.
        -- apply (Hmax c Hin Hsc).
Qed.

(* T-SOUND maximality / canonical choice: on descending-sorted candidates,
   select_floor returns the sound base of greatest block number. Combined with the
   candidate list being node-identical (parents are block-ordered; frontiers are
   pure functions), every honest node selects the same floor — no fork from
   candidate enumeration order. *)
Theorem select_highest_sound :
  forall d fuel parents cands f,
    DescSorted d cands ->
    select_floor d fuel parents cands = Some f ->
    is_sound d fuel parents cands f = true /\
    (forall c, In c cands -> is_sound d fuel parents cands c = true ->
       numof d c <= numof d f).
Proof.
  intros d fuel parents cands f Hsorted Hsel. unfold select_floor in Hsel.
  destruct (find_desc_max d (is_sound d fuel parents cands) cands f Hsorted Hsel)
    as [Hsf [_ Hmax]].
  split; [exact Hsf | exact Hmax].
Qed.
