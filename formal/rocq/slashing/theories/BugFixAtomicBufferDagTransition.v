(* ═══════════════════════════════════════════════════════════════════════════
   BugFixAtomicBufferDagTransition.v — Proof for Bug Fix #17 (T-9.20)

   Bug (Rust regression). Five sites in casper validation paths perform
   a non-transactional pair:
     (1) block_dag_storage.insert(block, mode);
     (2) casper_buffer_storage.remove(block_hash);
   The two stores live in distinct LMDB environments. A process crash
   between (1) and (2) leaves the block in DAG (state Invalid/Normal/
   Approved) but still in the casper buffer as a pending dependency.

   Fix. Introduce `atomic_insert_then_buffer(dag, block, mode, buffer,
   buffer_op)` in `block-storage/src/rust/dag/buffer_dag_transition.rs`
   that acquires both stores' write locks in documented order and
   performs (1)+(2) under one critical section. Cross-store ACID is
   physically impossible (distinct envs), so the helper is in-process
   best-effort; the on-resume `reconcile_buffer_against_dag` function
   closes any crash-window drift.

   Theorem T-9.20.recon. For every crash point during
   atomic_insert_then_buffer(B), applying reconcile_buffer_against_dag
   on resume yields the same slashing projection as the no-crash run.

   Companion doc: docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
   Companion Rust: block-storage/src/rust/dag/buffer_dag_transition.rs,
                   block-storage/tests/atomic_buffer_dag_transition.rs.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lists.List.
From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Logic.Decidable.
Import ListNotations.

Set Implicit Arguments.

(* ═══════════════════════════════════════════════════════════════════════════
   §1 — Abstract storage model

   We use `nat` as a concrete placeholder for `BlockHash` (any type
   with decidable equality would do; using a concrete type keeps the
   proof axiom-free per the project's no-Hypothesis / no-Axiom rule).
   The DAG and Buffer are sets of block hashes (functions to bool).
   Because a `HashSet` is a FUNCTION, set equality is stated and proved
   POINTWISE (`forall x, s1 x = s2 x`) — the observational meaning of
   "same set" — rather than as Leibniz equality `s1 = s2`, which would
   require the functional-extensionality AXIOM. Pointwise keeps the
   whole development axiom-free (no FunctionalExtensionality import).
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition BlockHash := nat.

Definition BlockHash_dec : forall (h1 h2 : BlockHash), {h1 = h2} + {h1 <> h2}
  := Nat.eq_dec.

(* Set representation: predicate-valued. *)
Definition HashSet := BlockHash -> bool.

Definition empty_set : HashSet := fun _ => false.

Definition set_insert (s : HashSet) (h : BlockHash) : HashSet :=
  fun x => if BlockHash_dec x h then true else s x.

Definition set_remove (s : HashSet) (h : BlockHash) : HashSet :=
  fun x => if BlockHash_dec x h then false else s x.

Definition set_contains (s : HashSet) (h : BlockHash) : bool := s h.

(* System state: pair of DAG and Buffer. *)
Record SystemState := mkState {
  dag    : HashSet;
  buffer : HashSet;
}.

(* ═══════════════════════════════════════════════════════════════════════════
   §2 — Operations

   `insert_dag` and `remove_buffer` are the two halves of the
   (potentially non-atomic) transition. The composed Step is the
   atomic helper's effect.
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition insert_dag (s : SystemState) (h : BlockHash) : SystemState :=
  mkState (set_insert s.(dag) h) s.(buffer).

Definition remove_buffer (s : SystemState) (h : BlockHash) : SystemState :=
  mkState s.(dag) (set_remove s.(buffer) h).

(* `atomic_insert_then_buffer` semantics: do both atomically. *)
Definition Step (s : SystemState) (h : BlockHash) : SystemState :=
  remove_buffer (insert_dag s h) h.

(* ═══════════════════════════════════════════════════════════════════════════
   §3 — Crash points

   A crash can happen at three observable points during atomic_insert_then_buffer:
     CrashBeforeInsert — neither side committed
     CrashBetweenInsertAndRemove — DAG side committed, buffer side did NOT
     CrashAfterRemove — both committed (steady state, no crash effectively)
   ═══════════════════════════════════════════════════════════════════════════ *)

Inductive CrashPoint :=
  | CrashBeforeInsert
  | CrashBetweenInsertAndRemove
  | CrashAfterRemove.

Definition crash_step (s : SystemState) (h : BlockHash) (c : CrashPoint) : SystemState :=
  match c with
  | CrashBeforeInsert => s
  | CrashBetweenInsertAndRemove => insert_dag s h
  | CrashAfterRemove => Step s h
  end.

(* ═══════════════════════════════════════════════════════════════════════════
   §4 — Reconciliation

   `reconcile_buffer_against_dag` walks the buffer; for any pendant
   whose hash is in the DAG, removes it from the buffer.

   We model "walk the buffer" implicitly: the reconcile function is
   defined for a SINGLE hash. Iterating over all pendants is the
   real Rust implementation, but each pendant-purge step is
   independent of the others — so it suffices to prove the property
   for one hash; the iteration is a fold over independent applications.
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition reconcile_one (s : SystemState) (h : BlockHash) : SystemState :=
  if andb (set_contains s.(dag) h) (set_contains s.(buffer) h)
  then remove_buffer s h
  else s.

(* ═══════════════════════════════════════════════════════════════════════════
   §5 — Slashing projection

   The slashing pipeline observes the DAG state (and the equivocation
   tracker, which is governed by T-9.2 and unchanged by this fix).
   Buffer state is operational metadata used to schedule pending-
   dependency block processing — it is NOT part of the slashing
   projection.

   This is the load-bearing observation for T-9.20: any drift between
   the two stores affects only the buffer side, and the buffer is
   below the slashing-projection horizon.
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition slashing_proj (s : SystemState) : HashSet := s.(dag).

(* ═══════════════════════════════════════════════════════════════════════════
   §6 — Lemmas
   ═══════════════════════════════════════════════════════════════════════════ *)

Lemma set_insert_preserves_existing :
  forall s h h', s h = true -> set_insert s h' h = true.
Proof.
  intros s h h' Hs. unfold set_insert.
  destruct (BlockHash_dec h h'); [reflexivity | assumption].
Qed.

Lemma set_insert_contains :
  forall s h, set_insert s h h = true.
Proof.
  intros s h. unfold set_insert.
  destruct (BlockHash_dec h h) as [_ | Hne]; [reflexivity | contradiction].
Qed.

Lemma set_remove_does_not_contain :
  forall s h, set_remove s h h = false.
Proof.
  intros s h. unfold set_remove.
  destruct (BlockHash_dec h h) as [_ | Hne]; [reflexivity | contradiction].
Qed.

Lemma set_remove_preserves_other :
  forall s h h', h <> h' -> set_remove s h' h = s h.
Proof.
  intros s h h' Hne. unfold set_remove.
  destruct (BlockHash_dec h h') as [E | _]; [contradiction | reflexivity].
Qed.

Lemma insert_dag_dag :
  forall s h, (insert_dag s h).(dag) = set_insert s.(dag) h.
Proof. reflexivity. Qed.

Lemma step_dag :
  forall s h, (Step s h).(dag) = set_insert s.(dag) h.
Proof.
  intros s h. unfold Step, remove_buffer, insert_dag. reflexivity.
Qed.

Lemma step_buffer_post :
  forall s h, (Step s h).(buffer) h = false.
Proof.
  intros s h. unfold Step, remove_buffer, insert_dag. simpl.
  apply set_remove_does_not_contain.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   §7 — Resume operation and main theorem T-9.20.recon

   On resume, the system runs `reconcile_buffer_against_dag` (closes
   any (c) drift) AND replays the operation via the normal admission
   path (closes any (b) pre-crash gap). The composed resume operation:

       resume_step s h c = Step (reconcile_one (crash_step s h c) h) h

   For each crash point:
     - CrashBeforeInsert: reconcile is a no-op; Step replays from
       scratch. Post-resume = Step s h. ✓
     - CrashBetweenInsertAndRemove: reconcile removes the drifted
       pendant; Step is then idempotent (insert is no-op because dag
       already has h; remove is no-op because buffer is already clean
       at h). Post-resume = Step s h. ✓
     - CrashAfterRemove: reconcile is a no-op; Step is idempotent.
       Post-resume = Step s h. ✓

   This is the load-bearing observational-equivalence theorem.
   ═══════════════════════════════════════════════════════════════════════════ *)

Definition resume_step (s : SystemState) (h : BlockHash) (c : CrashPoint) : SystemState :=
  Step (reconcile_one (crash_step s h c) h) h.

(* Step is idempotent on the dag projection, stated POINTWISE (forall x)
   so it is provable WITHOUT functional extensionality (see the §1 note).
   `forall x, ... x = ... x` is the observational meaning of "same dag". *)
Lemma step_idempotent_dag :
  forall s h x, (Step (Step s h) h).(dag) x = (Step s h).(dag) x.
Proof.
  intros s h x.
  rewrite !step_dag.
  unfold set_insert.
  destruct (BlockHash_dec x h) as [E | _]; reflexivity.
Qed.

(* Main theorem T-9.20.recon: for every crash point, the post-resume
   slashing projection equals the no-crash Step's projection.

   Strategy: in each branch, after substituting `resume_step`, we get
   a term `Step (reconcile_one (crash_step s h c) h) h`. The dag
   component is `set_insert ((reconcile_one (crash_step s h c) h).(dag)) h`.
   Reconcile NEVER changes the dag component (it only removes from
   buffer), so this equals `set_insert ((crash_step s h c).(dag)) h`.
   Each crash_step variant determines what `.(dag)` is:
     - CrashBeforeInsert: s.(dag)
     - CrashBetweenInsertAndRemove: set_insert s.(dag) h
     - CrashAfterRemove: set_insert s.(dag) h
   For CrashBetweenInsertAndRemove and CrashAfterRemove,
   `set_insert (set_insert s.(dag) h) h` is pointwise equal to
   `set_insert s.(dag) h` (set_insert is idempotent for the inserted
   element). For CrashBeforeInsert, the result is already
   `set_insert s.(dag) h`. So all three branches collapse to the same
   dag state as `Step s h`. *)

Lemma reconcile_one_preserves_dag :
  forall s h, (reconcile_one s h).(dag) = s.(dag).
Proof.
  intros s h.
  unfold reconcile_one.
  destruct (set_contains s.(dag) h && set_contains s.(buffer) h)%bool; reflexivity.
Qed.

(* Pointwise (forall x) so it needs no functional extensionality. *)
Lemma set_insert_idempotent :
  forall s h x, set_insert (set_insert s h) h x = set_insert s h x.
Proof.
  intros s h x.
  unfold set_insert.
  destruct (BlockHash_dec x h) as [E | _]; reflexivity.
Qed.

Theorem t_9_20_recon :
  forall s h c x,
    slashing_proj (resume_step s h c) x = slashing_proj (Step s h) x.
Proof.
  intros s h c x.
  unfold slashing_proj, resume_step.
  rewrite !step_dag.
  rewrite reconcile_one_preserves_dag.
  destruct c; simpl.
  - (* CrashBeforeInsert: (crash_step s h CrashBeforeInsert).dag = s.dag *)
    reflexivity.
  - (* CrashBetweenInsertAndRemove: .dag = set_insert s.dag h *)
    apply set_insert_idempotent.
  - (* CrashAfterRemove: .dag = set_insert s.dag h *)
    apply set_insert_idempotent.
Qed.

(* Reconcile is idempotent: running it twice produces the same state. *)
Theorem t_9_20_reconcile_idempotent :
  forall s h,
    reconcile_one (reconcile_one s h) h = reconcile_one s h.
Proof.
  intros s h.
  (* Strategy: case-split on the inner reconcile's branch. In the drift
     branch, the inner step removes the hash from the buffer, so the
     outer's `andb` is false and returns the inner result. In all other
     branches, the inner is a no-op and the outer applies the same case
     analysis. *)
  unfold reconcile_one.
  unfold set_contains.
  destruct (s.(dag) h) eqn:Edag; destruct (s.(buffer) h) eqn:Ebuf; simpl.
  - (* Edag = true, Ebuf = true: inner returns remove_buffer s h.
       Outer: dag of remove_buffer = s.(dag), still true at h.
              buffer of remove_buffer at h = false.
              andb is false → no change. Result = remove_buffer s h. *)
    unfold remove_buffer. simpl. rewrite Edag.
    rewrite set_remove_does_not_contain. reflexivity.
  - (* Edag = true, Ebuf = false: inner returns s (andb is false).
       Outer same case → returns s. *)
    rewrite Edag. rewrite Ebuf. reflexivity.
  - (* Edag = false, Ebuf = true: inner returns s.
       Outer: dag s h = false → andb false → returns s. *)
    rewrite Edag. reflexivity.
  - (* Edag = false, Ebuf = false. *)
    rewrite Edag. reflexivity.
Qed.

(* Insert+remove idempotence: applying the helper twice for the same
   block is observationally equivalent to applying it once. *)
Theorem t_9_20_step_idempotent_on_projection :
  forall s h x,
    slashing_proj (Step (Step s h) h) x = slashing_proj (Step s h) x.
Proof.
  intros s h x.
  unfold slashing_proj.
  apply step_idempotent_dag.
Qed.
