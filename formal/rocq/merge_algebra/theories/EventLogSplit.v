(* ===========================================================================
   EventLogSplit.v - THE P2 proof: the user/system EventLogIndex split is a
   set-union semilattice, so combine(fold user, fold system) = fold all on the
   conflict-relevant fields, and conflict detection on the split-then-recombined
   index is IDENTICAL to detection on the monolithic index (the split hides no
   user<->system conflict).

   `DeployChainIndex::new` (casper/src/rust/merging/deploy_chain_index.rs:70-88)
   folds each deploy into a `user_event_log_index` or `system_event_log_index`
   (partitioned by `is_system_deploy_id`), then
       event_log_index = EventLogIndex::combine(user, system).
   `EventLogIndex::combine` (event_log_index.rs:343) is field-wise SET UNION on the
   conflict-relevant produce/consume sets (`.union()`). Set union is a commutative,
   associative, IDEMPOTENT monoid (a join-semilattice) with the empty index as
   identity. We must show the two-bucket fold-then-combine equals the one-bucket
   fold of ALL deploys, so `conflicts` cannot see a different picture.

   ---------------------------------------------------------------------------
   Model
   ---------------------------------------------------------------------------
   The conflict-relevant fields `conflicts` reads are the three sets that drive
   its three checks: produces_consumed (pc), consumes_produced (cp), and
   produces_touching_base_joins (ptbj). We model each set as a nat BITMASK (a
   channel/event is a bit; membership = a set bit); `combine` is field-wise
   bitwise OR (`Nat.lor`) -- exactly the `.union()` the code performs, and a
   join-semilattice with plain `=` equality (no funext needed). The number-channel
   diff (which `combine` folds with a checked add that only ERRORS) is orthogonal
   to conflict detection and is modeled in ChannelNetting.v / IntegerAdd.v.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                       | Rust (event_log_index.rs / deploy_chain_index.rs)
   ---------------------------+-----------------------------------------------
   Idx = (pc, cp, ptbj)       | conflict-relevant EventLogIndex set fields
   combine (field-wise lor)   | EventLogIndex::combine (.union() per set)
   foldi                      | fold of a deploy bucket into one index
   combine_split_eq           | combine(fold user, fold system) = fold all
   conflicts_split_complete   | conflicts(split) = conflicts(monolithic)
   event_log_split_sound      | the split hides no user<->system conflict (P2)

   Stdlib-only, axiom-free. `Print Assumptions event_log_split_sound`
   => "Closed under the global context".
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

(* Conflict-relevant EventLogIndex fields as bitmask sets. *)
Record Idx := { pc : nat; cp : nat; ptbj : nat }.

Definition idx_empty : Idx := {| pc := 0; cp := 0; ptbj := 0 |}.

Definition combine (x y : Idx) : Idx :=
  {| pc   := Nat.lor (pc x)   (pc y);
     cp   := Nat.lor (cp x)   (cp y);
     ptbj := Nat.lor (ptbj x) (ptbj y) |}.

(* ===========================================================================
   Section 1 - combine is a commutative, associative, idempotent monoid.
   =========================================================================== *)

Lemma combine_comm : forall x y, combine x y = combine y x.
Proof.
  intros [xa xb xc] [ya yb yc]. unfold combine; simpl.
  rewrite (Nat.lor_comm xa ya), (Nat.lor_comm xb yb), (Nat.lor_comm xc yc). reflexivity.
Qed.

(* orientation a (b c) = (a b) c, matching Merge.v's op_assoc. *)
Lemma combine_assoc : forall x y z, combine x (combine y z) = combine (combine x y) z.
Proof.
  intros [xa xb xc] [ya yb yc] [za zb zc]. unfold combine; simpl.
  rewrite (Nat.lor_assoc xa ya za), (Nat.lor_assoc xb yb zb), (Nat.lor_assoc xc yc zc).
  reflexivity.
Qed.

Lemma combine_idem : forall x, combine x x = x.
Proof.
  intros [xa xb xc]. unfold combine; simpl.
  rewrite (Nat.lor_diag xa), (Nat.lor_diag xb), (Nat.lor_diag xc). reflexivity.
Qed.

Lemma combine_id_l : forall x, combine idx_empty x = x.
Proof.
  intros [xa xb xc]. unfold combine, idx_empty; simpl.
  rewrite (Nat.lor_0_l xa), (Nat.lor_0_l xb), (Nat.lor_0_l xc). reflexivity.
Qed.

(* ===========================================================================
   Section 2 - the fold, its append-homomorphism, and permutation invariance.
   =========================================================================== *)

Definition foldi (l : list Idx) : Idx := fold_right combine idx_empty l.

Lemma foldi_app : forall u s, foldi (u ++ s) = combine (foldi u) (foldi s).
Proof.
  induction u as [| x u IH]; intros s; simpl.
  - rewrite combine_id_l. reflexivity.
  - rewrite IH, combine_assoc. reflexivity.
Qed.

Lemma foldi_perm : forall l1 l2, Permutation l1 l2 -> foldi l1 = foldi l2.
Proof.
  intros l1 l2 H.
  induction H as [| x l1 l2 Hp IH | x y l | l1 l2 l3 Hp1 IH1 Hp2 IH2].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
  - simpl. rewrite combine_assoc, combine_assoc, (combine_comm y x). reflexivity.
  - rewrite IH1, IH2. reflexivity.
Qed.

(* A partition into (filter p) ++ (filter !p) is a permutation of the whole list
   -- the abstract shape of the user/system split by `is_system_deploy_id`. *)
Lemma partition_perm :
  forall (p : Idx -> bool) l,
    Permutation l (filter p l ++ filter (fun x => negb (p x)) l).
Proof.
  induction l as [| x l IH]; simpl.
  - apply Permutation_refl.
  - destruct (p x) eqn:E; simpl.
    + apply perm_skip; exact IH.
    + eapply Permutation_trans; [apply perm_skip; exact IH |].
      apply Permutation_middle.
Qed.

(* ===========================================================================
   Section 3 - the split soundness: combine(fold user, fold system) = fold all,
   and conflict detection is identical on the split and the monolithic index.
   =========================================================================== *)

(* the split-then-recombined index: fold each bucket, then combine (as the code
   does with user_event_log_index / system_event_log_index). *)
Definition combine_split (p : Idx -> bool) (l : list Idx) : Idx :=
  combine (foldi (filter p l)) (foldi (filter (fun x => negb (p x)) l)).

Theorem combine_split_eq : forall p l, combine_split p l = foldi l.
Proof.
  intros p l. unfold combine_split. rewrite <- foldi_app.
  apply foldi_perm. apply Permutation_sym. apply partition_perm.
Qed.

(* Any conflict detector (a pure function of the combined index) yields the SAME
   verdict on the split-recombined index and the monolithic index -- so the split
   can hide no user<->system conflict. *)
Theorem conflicts_split_complete :
  forall (conflicts : Idx -> Idx -> bool) (p : Idx -> bool) (l : list Idx) (other : Idx),
    conflicts (combine_split p l) other = conflicts (foldi l) other.
Proof. intros conflicts p l other. rewrite combine_split_eq. reflexivity. Qed.

(* THE headline. *)
Theorem event_log_split_sound :
  (forall p l, combine_split p l = foldi l)
  /\ (forall (conflicts : Idx -> Idx -> bool) p l other,
        conflicts (combine_split p l) other = conflicts (foldi l) other).
Proof. split; [exact combine_split_eq | exact conflicts_split_complete]. Qed.
