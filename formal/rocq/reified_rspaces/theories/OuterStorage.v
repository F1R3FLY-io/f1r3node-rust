(* ═══════════════════════════════════════════════════════════════════════════
   OuterStorage.v — the allocator of the Array and Vector spaces (P5d)

   This module models the allocator of the index names of the Array and
   Vector outer stores (P5 design v3 A.10, design v2 section 3.3, F7, F8,
   F11 and S6). An Array or a Vector space names each collection by an index
   name N(sid ‖ u64_be(i)). The allocator record A(sid) holds the next index
   n. The creation of the space stages A(sid) = 0, and an allocation of k
   names is one transaction that plans the allocation, then takes A(sid) = n
   and puts A(sid) = next:

     fixed array of size s:  n + k ≤ s, else OutOfNames      indices n..n+k−1
     cyclic array of size s: (n + i) mod s                   next (n + k) mod s
     vector:                 n + k ≤ i64::MAX, else OutOfNames

   A store accepts only the index names of its own space, below the size of
   an Array space (F8). The take and the put derive their random states from
   the random state of the job, with RandomPurpose::Allocator and the split
   bytes 0 and 1, so their identities are new for every job (S6). Every read
   and copy of the job is reserved before it is made (decision 42).

   Sources
   - Design: P5 design v3 A.10 (ALLOC), A.12 (the allocator row), A.13 (the
     P5d headlines); P5 design v2 section 3.3 (the allocator, S6, F7, F8,
     the acceptance list); the D5 report B.6 (the allocation protocol).
   - Plan: decisions 39 (F7, F8, F11), 42 (metering) and 46 (the job fails
     closed in native execution, outside this model).
   - Code: the P5 W2 tree of this repository. The paths below are relative
     to rholang/src/rust/interpreter.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   ─────────────────────────────────────────────────────────────────────────
   rule                     spaces/allocator.rs (enum AllocationRule: Fixed,
                            Cyclic, Unbounded); spaces/outer/array.rs
                            (ArrayShape::rule), spaces/outer/vector.rs
                            (VectorOuter::allocate)
   fixed_plan, cyclic_plan, vector_plan, plan
                            spaces/allocator.rs (AllocationRule::plan, with
                            bounded_sum, sequential and cyclic)
   wrap_from                spaces/allocator.rs (fn cyclic: push the index,
                            add one, wrap to 0 at the size)
   wf                       the range checks of AllocationRule::plan (an
                            allocator beyond its range is Inconsistent)
   staged, alloc, after     spaces/allocator.rs (fn allocate: the plan, then
                            AllocCx::take and AllocCx::put) and
                            spaces/generic.rs (GenericSpace::run_allocate,
                            AllocCx::take, AllocCx::put)
   run, final, allocated    a sequence of allocation jobs on one space (each
                            job commits once, generic.rs run_allocate)
   channel, check_key       spaces/allocator.rs (index_of_name, check_below),
                            spaces/outer/array.rs and vector.rs (check_key)
   rand, staged_identity, creation_identity
                            spaces/counter.rs (take: K(value) with
                            purpose.derive(r).split_byte(0); put: the record
                            with .split_byte(1)), spaces/allocator.rs
                            (creation_records), spaces/namespace.rs
                            (RandomPurpose::Allocator = 0x0B)
   event, step, alloc_trace spaces/generic.rs (begin_allocation,
                            run_allocate), spaces/outer/array.rs
                            (ArrayShape::of_config), spaces/allocator.rs
                            (allocate, AllocationRule::plan), spaces/
                            counter.rs (take, put), reified/costs.rs
                            (ScanMeter: reserve, then work)

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - A value and an index are natural numbers. The bound of the allocator
     record of a Vector space is the section variable vmax, i64::MAX in the
     code. A size is at most i64::MAX in the code, so the sum n + k of a
     fixed array never overflows where the code compares it (the code
     rejects an overflow with OutOfNames, which the model gives too).
   - A space and a random tail are natural numbers. A forged channel is a
     number. The decoder of the code (namespace::reified_id_of) is modeled
     by the constructor of the channel.
   - A random state is a structured value: the job, the purpose byte and
     the split byte. A Blake2b random state that splits a byte appends the
     byte to its path, so two states with different paths differ. The model
     takes this as the injectivity of the record constructor.
   - The metering rule is the one of AgentReadCoverage.v (DR-88 of
     feature/cost-accounted-rho), restated here so that the module is
     standalone: a self-metered step is Reserve u; Read u, and a trace is
     covered when every prefix reads at most what it reserved.
     Changed by decision 96 (decision 89, DR-131): AgentReadCoverage.v is
     deleted, and section 8 goes with the host-work metering.
   - The commit applies the staged operations in order (decision 19, D6).
     The outcome after the commit only moves the planned indices out of the
     prepared job, so its trace is empty.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. fixed_array_allocates_at_most_size  a fixed array never passes its
                                           size, and it accepts exactly the
                                           index names of its space below
                                           its size
    2. fixed_array_out_of_names_exact      OutOfNames exactly when the
                                           allocation does not fit, and it
                                           stages nothing
    3. cyclic_array_index_mod_size         the code's wrap loop gives the
                                           indices (n + i) mod s, and the
                                           next value is (n + k) mod s
    4. allocation_batch_atomic             an allocation stages nothing, or
                                           exactly one take and one put with
                                           all k indices
    5. fixed_array_indices_sequential      a fixed array allocates 0, 1, 2,
                                           ... in order, each once
    6. allocator_indices_distinct          the fixed and vector rules never
                                           allocate one index twice
    7. allocator_identities_unique         the creation record and the take
                                           and put of every job have
                                           pairwise distinct identities,
                                           also when a cyclic value repeats
    8. alloc_trace_covered                 an allocation reserves each read
                                           and copy before it makes it, and
                                           nothing runs after the commit
                                           Decision 96 (decision 89,
                                           DR-131) disables this headline
                                           with the host-work metering.

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables are parameters of the closed theorems. The file imports only
   the Rocq standard library.

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - The legacy model (f1r3node-reified-rspaces, OuterStorage.v) kept the
     array in memory as a list of slots, with fixed_array_find_empty. It
     cannot state the race of two branches or the restart, because its
     next index is not a record. This model keeps the next index in A(sid).
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Arith Bool Lia PeanoNat.
Import ListNotations.

(* ── 1. The allocation rules and the plan ─────────────────────────────── *)

(* How a store allocates its index names. *)
Inductive rule : Type :=
| Fixed (size : nat)
| Cyclic (size : nat)
| Unbounded.

(* The plan of one allocation. *)
Inductive outcome : Type :=
| Allocated (indices : list nat) (next : nat)
| OutOfNames
| Inconsistent.

(* The indices of a cyclic allocation as the code computes them: it pushes
   the index, adds one, and wraps to 0 at the size. *)
Fixpoint wrap_from (s idx k : nat) : list nat :=
  match k with
  | 0 => []
  | S k' => idx :: wrap_from s (if idx + 1 =? s then 0 else idx + 1) k'
  end.

(* The plan of a fixed array of size s from the value n. *)
Definition fixed_plan (s n k : nat) : outcome :=
  if s <? n then Inconsistent
  else if n + k <=? s then Allocated (seq n k) (n + k) else OutOfNames.

(* The plan of a cyclic array of size s from the value n. *)
Definition cyclic_plan (s n k : nat) : outcome :=
  if n <? s then Allocated (wrap_from s n k) ((n + k mod s) mod s) else Inconsistent.

Section Allocator.
  (* The greatest value of the allocator record: i64::MAX in the code. *)
  Variable vmax : nat.

  (* The plan of a vector from the value n. *)
  Definition vector_plan (n k : nat) : outcome :=
    if n + k <=? vmax then Allocated (seq n k) (n + k) else OutOfNames.

  Definition plan (r : rule) (n k : nat) : outcome :=
    match r with
    | Fixed s => fixed_plan s n k
    | Cyclic s => cyclic_plan s n k
    | Unbounded => vector_plan n k
    end.

  (* The range of the allocator value of a rule. The creation stages 0, and
     every allocation keeps the value in the range (allocation_batch_atomic). *)
  Definition wf (r : rule) (n : nat) : Prop :=
    match r with
    | Fixed s => n <= s
    | Cyclic s => n < s
    | Unbounded => n <= vmax
    end.

  (* ── 2. The staged operations and the runs ──────────────────────────── *)

  (* A staged operation of an allocation job: the take of A(sid) = value
     and the put of A(sid) = value. *)
  Inductive staged : Type :=
  | Take (value : nat) (job : nat)
  | Put (value : nat) (job : nat).

  (* One allocation job: the plan, then the take and the put. A plan that
     does not allocate stages nothing. *)
  Definition alloc (r : rule) (n k job : nat) : outcome * list staged :=
    match plan r n k with
    | Allocated idx next => (Allocated idx next, [Take n job; Put next job])
    | o => (o, [])
    end.

  (* The allocator value after the commit of one job. *)
  Definition after (r : rule) (n k : nat) : nat :=
    match plan r n k with
    | Allocated _ next => next
    | OutOfNames | Inconsistent => n
    end.

  (* A run of jobs (k, job) on one space from the value n. *)
  Fixpoint run (r : rule) (n : nat) (reqs : list (nat * nat)) : list (outcome * list staged) :=
    match reqs with
    | [] => []
    | (k, job) :: rest => alloc r n k job :: run r (after r n k) rest
    end.

  Fixpoint final (r : rule) (n : nat) (reqs : list (nat * nat)) : nat :=
    match reqs with
    | [] => n
    | (k, _) :: rest => final r (after r n k) rest
    end.

  Definition indices_of (o : outcome) : list nat :=
    match o with
    | Allocated idx _ => idx
    | OutOfNames | Inconsistent => []
    end.

  (* Every index that a run allocates, in allocation order. *)
  Definition allocated (steps : list (outcome * list staged)) : list nat :=
    flat_map (fun st => indices_of (fst st)) steps.

  (* ── 3. The key check (F8) ──────────────────────────────────────────── *)

  (* A channel: an index name of a space, a random-tail name of a space, or
     any other channel. *)
  Inductive channel : Type :=
  | IndexName (space index : nat)
  | RandomName (space tail : nat)
  | Forged (c : nat).

  Definition check_key (r : rule) (space : nat) (c : channel) : bool :=
    match c with
    | IndexName sp i =>
        Nat.eqb sp space &&
        match r with
        | Fixed s | Cyclic s => i <? s
        | Unbounded => true
        end
    | RandomName _ _ | Forged _ => false
    end.

  Lemma check_key_bounded : forall s space c,
    check_key (Fixed s) space c = true <-> exists i, c = IndexName space i /\ i < s.
  Proof.
    intros s space c. split.
    - destruct c as [sp i | sp t | f]; simpl; try discriminate.
      intros accepted. apply andb_true_iff in accepted as [same below].
      apply Nat.eqb_eq in same. apply Nat.ltb_lt in below. subst sp.
      exists i. split; [reflexivity | exact below].
    - intros [i [-> below]]. simpl.
      rewrite Nat.eqb_refl. apply Nat.ltb_lt in below. rewrite below. reflexivity.
  Qed.

  (* ── 4. Fixed arrays ────────────────────────────────────────────────── *)

  Lemma fixed_plan_fits : forall s n k,
    n <= s -> n + k <= s -> fixed_plan s n k = Allocated (seq n k) (n + k).
  Proof.
    intros s n k within fits. unfold fixed_plan.
    destruct (Nat.ltb_spec s n) as [over | _]; [lia |].
    destruct (Nat.leb_spec (n + k) s) as [_ | over]; [reflexivity | lia].
  Qed.

  Lemma fixed_plan_full : forall s n k,
    n <= s -> s < n + k -> fixed_plan s n k = OutOfNames.
  Proof.
    intros s n k within full. unfold fixed_plan.
    destruct (Nat.ltb_spec s n) as [over | _]; [lia |].
    destruct (Nat.leb_spec (n + k) s) as [fits | _]; [lia | reflexivity].
  Qed.

  (* From a value n within the size, a run of a fixed array allocates the
     indices n, n + 1, ... up to its final value, which stays within the
     size. *)
  Lemma fixed_run_seq : forall s reqs n,
    n <= s ->
    allocated (run (Fixed s) n reqs) = seq n (final (Fixed s) n reqs - n) /\
    n <= final (Fixed s) n reqs <= s.
  Proof.
    intros s reqs. induction reqs as [| [k job] rest IH]; intros n within.
    - simpl. rewrite Nat.sub_diag. split; [reflexivity | lia].
    - destruct (Nat.leb_spec (n + k) s) as [fits | full].
      + assert (step : plan (Fixed s) n k = Allocated (seq n k) (n + k))
          by (apply fixed_plan_fits; lia).
        assert (moved : after (Fixed s) n k = n + k)
          by (unfold after; rewrite step; reflexivity).
        destruct (IH (n + k) fits) as [same bounds].
        simpl. unfold alloc. rewrite step, moved. simpl.
        unfold allocated in *. simpl in *. rewrite same.
        split; [| lia].
        replace (final (Fixed s) (n + k) rest - n)
          with (k + (final (Fixed s) (n + k) rest - (n + k))) by lia.
        rewrite seq_app. reflexivity.
      + assert (step : plan (Fixed s) n k = OutOfNames)
          by (apply fixed_plan_full; lia).
        assert (stays : after (Fixed s) n k = n)
          by (unfold after; rewrite step; reflexivity).
        destruct (IH n within) as [same bounds].
        simpl. unfold alloc. rewrite step, stays. simpl.
        unfold allocated in *. simpl in *. rewrite same. split; [reflexivity | lia].
  Qed.

  Theorem fixed_array_allocates_at_most_size : forall s space reqs,
    final (Fixed s) 0 reqs <= s /\
    length (allocated (run (Fixed s) 0 reqs)) <= s /\
    (forall i, In i (allocated (run (Fixed s) 0 reqs)) ->
               i < s /\ check_key (Fixed s) space (IndexName space i) = true) /\
    (forall c, check_key (Fixed s) space c = true <-> exists i, c = IndexName space i /\ i < s).
  Proof.
    intros s space reqs.
    destruct (fixed_run_seq s reqs 0 (Nat.le_0_l s)) as [same bounds].
    rewrite Nat.sub_0_r in same.
    split; [lia |]. split; [rewrite same, length_seq; lia |]. split.
    - intros i member. rewrite same in member. apply in_seq in member.
      assert (below : i < s) by lia. split; [exact below |].
      apply check_key_bounded. exists i. split; [reflexivity | exact below].
    - intros c. apply check_key_bounded.
  Qed.

  Theorem fixed_array_out_of_names_exact : forall s n k job,
    n <= s ->
    (plan (Fixed s) n k = OutOfNames <-> s < n + k) /\
    (s < n + k -> alloc (Fixed s) n k job = (OutOfNames, []) /\ after (Fixed s) n k = n).
  Proof.
    intros s n k job within. split.
    - split.
      + intros out. simpl in out. destruct (Nat.leb_spec (n + k) s) as [fits | full];
          [| exact full].
        rewrite (fixed_plan_fits s n k within fits) in out. discriminate.
      + intros full. simpl. apply fixed_plan_full; assumption.
    - intros full. unfold alloc, after. simpl.
      rewrite (fixed_plan_full s n k within full). split; reflexivity.
  Qed.

  Theorem fixed_array_indices_sequential : forall s reqs,
    allocated (run (Fixed s) 0 reqs) = seq 0 (final (Fixed s) 0 reqs) /\
    (forall n k idx next, n <= s ->
       plan (Fixed s) n k = Allocated idx next -> idx = seq n k /\ next = n + k).
  Proof.
    intros s reqs. split.
    - destruct (fixed_run_seq s reqs 0 (Nat.le_0_l s)) as [same _].
      rewrite Nat.sub_0_r in same. exact same.
    - intros n k idx next within planned. simpl in planned.
      destruct (Nat.leb_spec (n + k) s) as [fits | full].
      + rewrite (fixed_plan_fits s n k within fits) in planned.
        injection planned as <- <-. split; reflexivity.
      + rewrite (fixed_plan_full s n k within full) in planned. discriminate.
  Qed.

  (* ── 5. Cyclic arrays ───────────────────────────────────────────────── *)

  (* The wrap loop of the code computes the modulo rule. *)
  Lemma wrap_from_mod : forall s k idx,
    idx < s -> wrap_from s idx k = map (fun i => (idx + i) mod s) (seq 0 k).
  Proof.
    intros s k. induction k as [| k IH]; intros idx below; [reflexivity |].
    simpl. rewrite Nat.add_0_r, Nat.mod_small by exact below. f_equal.
    rewrite <- seq_shift, map_map.
    destruct (Nat.eqb_spec (idx + 1) s) as [wraps | inside].
    - rewrite IH by lia. apply map_ext. intros i.
      replace (idx + S i) with (i + 1 * s) by lia.
      rewrite Nat.Div0.mod_add. reflexivity.
    - rewrite IH by lia. apply map_ext. intros i. f_equal. lia.
  Qed.

  Theorem cyclic_array_index_mod_size : forall s n k,
    n < s ->
    plan (Cyclic s) n k = Allocated (map (fun i => (n + i) mod s) (seq 0 k)) ((n + k) mod s) /\
    (n + k) mod s < s /\
    (forall i, In i (map (fun i => (n + i) mod s) (seq 0 k)) -> i < s).
  Proof.
    intros s n k below.
    assert (positive : s <> 0) by lia.
    split; [| split].
    - simpl. unfold cyclic_plan.
      replace (n <? s) with true by (symmetry; apply Nat.ltb_lt; exact below).
      rewrite wrap_from_mod by exact below.
      rewrite Nat.Div0.add_mod_idemp_r. reflexivity.
    - apply Nat.mod_upper_bound. exact positive.
    - intros i member. apply in_map_iff in member as [j [<- _]].
      apply Nat.mod_upper_bound. exact positive.
  Qed.

  (* A cyclic array reuses its names (the modulo rule of the FIPS): two
     allocations from a space of two names share the index 0. *)
  Example cyclic_reuses_names :
    allocated (run (Cyclic 2) 0 [(1, 1); (2, 2)]) = [0; 1; 0].
  Proof. reflexivity. Qed.

  (* ── 6. Batches and distinct indices ─────────────────────────────────── *)

  Theorem allocation_batch_atomic : forall r n k job,
    wf r n ->
    (alloc r n k job = (OutOfNames, []) /\ after r n k = n) \/
    (exists idx next,
       alloc r n k job = (Allocated idx next, [Take n job; Put next job]) /\
       length idx = k /\ after r n k = next /\ wf r next).
  Proof.
    intros r n k job well.
    destruct r as [s | s | ]; simpl in well; unfold alloc, after.
    - destruct (Nat.leb_spec (n + k) s) as [fits | full].
      + right. exists (seq n k), (n + k). simpl.
        rewrite (fixed_plan_fits s n k well fits).
        split; [reflexivity |]. split; [apply length_seq |]. split; [reflexivity |].
        simpl. exact fits.
      + left. simpl. rewrite (fixed_plan_full s n k well full). split; reflexivity.
    - right.
      destruct (cyclic_array_index_mod_size s n k well) as [planned [next_below _]].
      exists (map (fun i => (n + i) mod s) (seq 0 k)), ((n + k) mod s).
      rewrite planned. split; [reflexivity |].
      split; [rewrite length_map; apply length_seq |].
      split; [reflexivity | simpl; exact next_below].
    - simpl. unfold vector_plan. destruct (Nat.leb_spec (n + k) vmax) as [fits | full].
      + right. eexists. eexists.
        split; [reflexivity |]. split; [apply length_seq |].
        split; [reflexivity | simpl; lia].
      + left. split; reflexivity.
  Qed.

  (* From a value within the bound, a run of a vector allocates the indices
     n, n + 1, ... up to its final value. *)
  Lemma vector_run_seq : forall reqs n,
    n <= vmax ->
    allocated (run Unbounded n reqs) = seq n (final Unbounded n reqs - n) /\
    n <= final Unbounded n reqs <= vmax.
  Proof.
    intros reqs. induction reqs as [| [k job] rest IH]; intros n within.
    - simpl. rewrite Nat.sub_diag. split; [reflexivity | lia].
    - simpl. unfold alloc, after. simpl. unfold vector_plan.
      destruct (Nat.leb_spec (n + k) vmax) as [fits | full].
      + destruct (IH (n + k) fits) as [same bounds].
        unfold allocated in *. simpl in *. rewrite same. split; [| lia].
        replace (final Unbounded (n + k) rest - n)
          with (k + (final Unbounded (n + k) rest - (n + k))) by lia.
        rewrite seq_app. reflexivity.
      + destruct (IH n within) as [same bounds].
        unfold allocated in *. simpl in *. rewrite same. split; [reflexivity | lia].
  Qed.

  Theorem allocator_indices_distinct : forall r reqs,
    (forall s, r <> Cyclic s) ->
    NoDup (allocated (run r 0 reqs)).
  Proof.
    intros r reqs monotone. destruct r as [s | s | ].
    - destruct (fixed_run_seq s reqs 0 (Nat.le_0_l s)) as [same _].
      rewrite same. apply seq_NoDup.
    - exfalso. apply (monotone s). reflexivity.
    - destruct (vector_run_seq reqs 0 (Nat.le_0_l vmax)) as [same _].
      rewrite same. apply seq_NoDup.
  Qed.

  (* ── 7. Identities (S6) ─────────────────────────────────────────────── *)

  (* The purpose byte of the allocator: RandomPurpose::Allocator = 0x0B. *)
  Definition allocator_purpose : nat := 11.

  (* A random state: the job whose random state it derives from, the
     purpose byte and the split byte. *)
  Record rand : Type := mkRand { r_job : nat; r_purpose : nat; r_split : nat }.

  Definition job_random (job split : nat) : rand := mkRand job allocator_purpose split.

  (* The identity of a staged operation on A(sid): a consume of K(value) or
     a produce of the record A(sid) = value, with its random state. *)
  Inductive identity : Type :=
  | ConsumeId (value : nat) (random : rand)
  | ProduceId (value : nat) (random : rand).

  Definition staged_identity (op : staged) : identity :=
    match op with
    | Take v job => ConsumeId v (job_random job 0)
    | Put v job => ProduceId v (job_random job 1)
    end.

  (* The identity of the record A(sid) = 0 of the creation, with the random
     state of the factory call. *)
  Definition creation_identity (cjob : nat) : identity := ProduceId 0 (job_random cjob 1).

  Definition run_identities (r : rule) (n : nat) (reqs : list (nat * nat)) : list identity :=
    flat_map (fun st => map staged_identity (snd st)) (run r n reqs).

  Definition identity_job (id : identity) : nat :=
    match id with
    | ConsumeId _ random | ProduceId _ random => r_job random
    end.

  Lemma alloc_identities : forall r n k job id,
    In id (map staged_identity (snd (alloc r n k job))) -> identity_job id = job.
  Proof.
    intros r n k job id member. unfold alloc in member.
    destruct (plan r n k) as [idx next | |]; simpl in member;
      [| contradiction | contradiction].
    destruct member as [<- | [<- | []]]; reflexivity.
  Qed.

  Lemma alloc_identities_nodup : forall r n k job,
    NoDup (map staged_identity (snd (alloc r n k job))).
  Proof.
    intros r n k job. unfold alloc.
    destruct (plan r n k) as [idx next | |]; simpl; [| constructor | constructor].
    constructor.
    - intros [same | []]. discriminate.
    - constructor; [intros [] | constructor].
  Qed.

  Lemma run_identities_jobs : forall r reqs n id,
    In id (run_identities r n reqs) -> In (identity_job id) (map snd reqs).
  Proof.
    intros r reqs. induction reqs as [| [k job] rest IH]; intros n id member;
      [contradiction |].
    unfold run_identities in member. simpl in member.
    apply in_app_or in member as [here | later].
    - left. symmetry. apply (alloc_identities r n k job id here).
    - right. apply (IH (after r n k) id later).
  Qed.

  Lemma nodup_app_disjoint : forall (A : Type) (left right : list A),
    NoDup left -> NoDup right -> (forall a, In a left -> ~ In a right) ->
    NoDup (left ++ right).
  Proof.
    intros A left. induction left as [| a rest IH]; intros right nl nr disjoint;
      [exact nr |].
    inversion nl as [| a' rest' absent nrest]; subst. simpl. constructor.
    - intros member. apply in_app_or in member as [inner | other].
      + exact (absent inner).
      + exact (disjoint a (or_introl eq_refl) other).
    - apply IH; [exact nrest | exact nr |].
      intros b inner. apply disjoint. right. exact inner.
  Qed.

  Lemma run_identities_nodup : forall r reqs n,
    NoDup (map snd reqs) -> NoDup (run_identities r n reqs).
  Proof.
    intros r reqs. induction reqs as [| [k job] rest IH]; intros n distinct;
      [constructor |].
    simpl in distinct. inversion distinct as [| j js fresh drest]; subst.
    unfold run_identities. simpl. apply nodup_app_disjoint.
    - apply alloc_identities_nodup.
    - apply IH. exact drest.
    - intros id here later.
      apply (alloc_identities r n k job id) in here.
      apply run_identities_jobs in later. rewrite here in later. exact (fresh later).
  Qed.

  Theorem allocator_identities_unique : forall r n cjob reqs,
    NoDup (cjob :: map snd reqs) ->
    NoDup (creation_identity cjob :: run_identities r n reqs).
  Proof.
    intros r n cjob reqs distinct.
    inversion distinct as [| j js fresh drest]; subst. constructor.
    - intros member. apply run_identities_jobs in member. exact (fresh member).
    - apply run_identities_nodup. exact drest.
  Qed.

  (* A cyclic space of one name takes and puts the value 0 in every job, and
     the identities still differ (the case that S6 names). *)
  Example cyclic_single_name_values_repeat :
    map (fun st => indices_of (fst st)) (run (Cyclic 1) 0 [(1, 7); (1, 8)]) = [[0]; [0]] /\
    run_identities (Cyclic 1) 0 [(1, 7); (1, 8)] =
      [ConsumeId 0 (job_random 7 0); ProduceId 0 (job_random 7 1);
       ConsumeId 0 (job_random 8 0); ProduceId 0 (job_random 8 1)].
  Proof. split; reflexivity. Qed.

  (* Disabled by decision 96 (decision 89, DR-131): alloc_trace_covered goes
     with the host-work metering. The section stays below as a comment.
  (* ── 8. The metering of an allocation (decision 42) ─────────────────── *)

  (* The metering rule of DR-88, restated from AgentReadCoverage.v. *)
  Inductive event : Type := Reserve (units : nat) | Read (units : nat).

  Definition reserved_units (e : event) : nat :=
    match e with Reserve units => units | Read _ => 0 end.

  Definition read_units (e : event) : nat :=
    match e with Read units => units | Reserve _ => 0 end.

  Definition total (project : event -> nat) (trace : list event) : nat :=
    fold_right Nat.add 0 (map project trace).

  Lemma total_app : forall project left right,
    total project (left ++ right) = total project left + total project right.
  Proof.
    intros project left right. unfold total.
    rewrite map_app, fold_right_app.
    induction left as [| e rest IH]; simpl; [reflexivity |].
    unfold total in IH. rewrite IH. lia.
  Qed.

  (* Every prefix reads at most [slack] units more than it reserves. *)
  Definition covered_from (slack : nat) (trace : list event) : Prop :=
    forall prefix suffix, trace = prefix ++ suffix ->
      total read_units prefix <= slack + total reserved_units prefix.

  Definition covered (trace : list event) : Prop := covered_from 0 trace.

  Lemma covered_from_nil : forall slack, covered_from slack [].
  Proof.
    intros slack prefix suffix split.
    destruct prefix; [unfold total; simpl; lia | discriminate].
  Qed.

  (* A self-metered step: reserve the work, then perform it. *)
  Definition step (units : nat) : list event := [Reserve units; Read units].

  (* A balanced segment: covered from any slack, with as many reads as
     reservations. *)
  Definition balanced (trace : list event) : Prop :=
    (forall slack, covered_from slack trace) /\
    total read_units trace = total reserved_units trace.

  Lemma step_balanced : forall units, balanced (step units).
  Proof.
    intros units. split.
    - intros slack prefix suffix split. unfold step in split.
      destruct prefix as [| a [| b [| c rest]]]; simpl in split.
      + unfold total. simpl. lia.
      + injection split as ha _. subst a. unfold total. simpl. lia.
      + injection split as ha hb _. subst a b. unfold total. simpl. lia.
      + discriminate.
    - unfold total, step. simpl. lia.
  Qed.

  Lemma balanced_nil : balanced [].
  Proof. split; [apply covered_from_nil | reflexivity]. Qed.

  Lemma balanced_app : forall left right,
    balanced left -> balanced right -> balanced (left ++ right).
  Proof.
    intros left right [covered_left equal_left] [covered_right equal_right].
    split.
    - intros slack prefix suffix split.
      apply app_eq_app in split as [middle [[left_prefix right_suffix] | [prefix_left right_middle]]].
      + exact (covered_left slack prefix middle left_prefix).
      + subst prefix. rewrite !total_app.
        pose proof (covered_right slack middle suffix right_middle) as inner. lia.
    - rewrite !total_app. lia.
  Qed.

  Lemma steps_balanced : forall amounts, balanced (flat_map step amounts).
  Proof.
    induction amounts as [| amount rest IH]; [exact balanced_nil |].
    change (balanced (step amount ++ flat_map step rest)).
    apply balanced_app; [apply step_balanced | exact IH].
  Qed.

  (* The units of the reservations of one allocation job. *)
  Record alloc_units : Type := mkUnits {
    u_names : nat;      (* the names C(sid) and A(sid) of begin_allocation *)
    u_config : nat;     (* the read and the strict decode of the config *)
    u_family : nat;     (* the comparison of the family names *)
    u_context : nat;    (* the matcher handle and the introduction *)
    u_shape : nat;      (* the lookups and the reads of the shape *)
    u_copy : nat;       (* the copy of the allocator record *)
    u_decode : nat;     (* the decode of its value *)
    u_compare : nat;    (* one comparison of two values *)
    u_buffer : nat;     (* the buffer of the indices *)
    u_take : list nat;  (* the self-metered steps of counter::take *)
    u_put : list nat    (* the self-metered steps of counter::put *)
  }.

  (* The comparisons of the plan: the range of the value, then the bound of
     a fixed array. *)
  Definition compares (r : rule) (n : nat) : nat :=
    match r with
    | Fixed s => if s <? n then 1 else 2
    | Cyclic _ | Unbounded => 1
    end.

  (* The trace of a job before its commit. A plan that does not allocate
     stops after its comparisons. *)
  Definition staged_trace (u : alloc_units) (r : rule) (n k : nat) : list event :=
    step (u_names u) ++ step (u_config u) ++ step (u_family u) ++ step (u_context u) ++
    step (u_shape u) ++ step (u_copy u) ++ step (u_decode u) ++
    flat_map step (repeat (u_compare u) (compares r n)) ++
    match plan r n k with
    | Allocated _ _ => step (u_buffer u) ++ flat_map step (u_take u) ++ flat_map step (u_put u)
    | OutOfNames | Inconsistent => []
    end.

  (* The trace after the commit: the finish of the job moves the planned
     indices out, so it reads and reserves nothing. *)
  Definition after_commit_trace : list event := [].

  Definition alloc_trace (u : alloc_units) (r : rule) (n k : nat) : list event :=
    staged_trace u r n k ++ after_commit_trace.

  Theorem alloc_trace_covered : forall u r n k,
    covered (alloc_trace u r n k) /\
    total reserved_units after_commit_trace = 0 /\
    total read_units (alloc_trace u r n k) = total reserved_units (alloc_trace u r n k).
  Proof.
    intros u r n k.
    assert (staged : balanced (staged_trace u r n k)).
    { unfold staged_trace.
      repeat (apply balanced_app; [apply step_balanced |]).
      apply balanced_app; [apply steps_balanced |].
      destruct (plan r n k); [| exact balanced_nil | exact balanced_nil].
      apply balanced_app; [apply step_balanced |].
      apply balanced_app; apply steps_balanced. }
    assert (whole : balanced (alloc_trace u r n k)).
    { unfold alloc_trace. apply balanced_app; [exact staged |].
      unfold after_commit_trace. exact balanced_nil. }
    destruct whole as [covered_whole equal]. split; [exact (covered_whole 0) |].
    split; [reflexivity | exact equal].
  Qed.
  *)

End Allocator.

(* ── 9. Assumption audit ─────────────────────────────────────────────── *)

Print Assumptions fixed_array_allocates_at_most_size.
Print Assumptions fixed_array_out_of_names_exact.
Print Assumptions cyclic_array_index_mod_size.
Print Assumptions allocation_batch_atomic.
Print Assumptions fixed_array_indices_sequential.
Print Assumptions allocator_indices_distinct.
Print Assumptions allocator_identities_unique.
(* Disabled by decision 96 (decision 89, DR-131): alloc_trace_covered goes
   with the host-work metering.
Print Assumptions alloc_trace_covered.
*)
