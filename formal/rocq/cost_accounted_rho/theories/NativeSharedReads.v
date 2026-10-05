(* C1 (epic 8946, B1 phase B; decision record DR-81): native continuation
   reads share the cached payloads instead of copying them.

   The native hot store caches the continuations of a key once, each behind
   a shared pointer (Arc). The legacy read copied every cached continuation
   on every read. The C1 read returns the pointers: a warm read clones
   pointers, a cold read moves each decoded continuation into its pointer
   and prepays its release when it enters the cache, and the caller copies
   only the continuation that it selects.

   Results:
   - shared_selection_equals_deep_selection: selection over views that
     resolve to the payloads equals selection over the copied payloads.
   - prefetch_then_read_equals_read: a prefetch (a read whose result is
     dropped) followed by a read gives the same result and the same cache
     as the read alone.
   - cleanup_prepaid_preserved: every step keeps the invariant that each
     live payload has its release prepaid.
   - every_release_was_prepaid: in every trace that starts in a state with
     that invariant, no payload is released without prepayment.
   - fill_without_prepay_releases_unpaid: a negative control. A cold fill
     that skips the prepayment leads to an unpaid release.

   C2 (decision record DR-82) gives the data reads the same treatment: a
   data view is an O(1) snapshot of the persistent shard, so the views
   above also stand for borrowed datums. It adds:
   - rejected_cold_read_leaves_cache and accepted_cold_read_equals_read: a
     cold read that reserves before it inserts leaves the cache unchanged
     when a reservation is rejected, and otherwise equals the read.
   - insert_first_fills_cache_on_rejection: a negative control. A read that
     inserts before it reserves leaves the cache filled after a rejection.

   Rust correspondence: rspace++/src/rspace/hot_store/native.rs
   (native_continuation_views); rspace++/src/rspace/replay_rspace/
   native_session/history.rs (read_continuation_views,
   prefetch_continuations); native_candidate/metered.rs (the selected
   continuation is copied); hot_store/native.rs (native_data_view,
   native_insert_new_snapshot) for C2. TLA+: NativeSharedReadCleanup.tla. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Lia.
Import ListNotations.

Section Selection.
  Variable payload : Type.
  Variable matches : payload -> bool.
  (* The heap maps a pointer to its payload. *)
  Variable heap : nat -> option payload.

  (* Selection over copied payloads: the first match and its index. *)
  Fixpoint select_values (index : nat) (values : list payload) : option (nat * payload) :=
    match values with
    | [] => None
    | value :: rest =>
        if matches value then Some (index, value) else select_values (S index) rest
    end.

  (* Selection over shared views: each view is read through the heap. *)
  Fixpoint select_views (index : nat) (views : list nat) : option (nat * payload) :=
    match views with
    | [] => None
    | view :: rest =>
        match heap view with
        | Some value =>
            if matches value then Some (index, value) else select_views (S index) rest
        | None => None
        end
    end.

  (* The views resolve, in order, to the payloads that a copy would hold. *)
  Fixpoint resolves (views : list nat) (values : list payload) : Prop :=
    match views, values with
    | [], [] => True
    | view :: rest, value :: values => heap view = Some value /\ resolves rest values
    | _, _ => False
    end.

  Theorem shared_selection_equals_deep_selection : forall views values index,
    resolves views values -> select_views index views = select_values index values.
  Proof.
    induction views as [| view rest IH]; intros [| value values] index resolved;
      simpl in *; try contradiction; [reflexivity |].
    destruct resolved as [found rest_resolved]. rewrite found.
    destruct (matches value); [reflexivity |]. apply IH. exact rest_resolved.
  Qed.
End Selection.

Section Cache.
  (* The cache maps a key to its cached pointers. A cold read decodes the
     pointers of a key and stores them; a warm read returns them. *)
  Variable decode : nat -> list nat.

  Definition cache := nat -> option (list nat).

  Definition update (store : cache) (key : nat) (views : list nat) : cache :=
    fun other => if Nat.eqb key other then Some views else store other.

  Definition read (store : cache) (key : nat) : list nat * cache :=
    match store key with
    | Some views => (views, store)
    | None => (decode key, update store key (decode key))
    end.

  Definition prefetch (store : cache) (key : nat) : cache := snd (read store key).

  Theorem prefetch_then_read_equals_read : forall store key,
    read (prefetch store key) key = read store key.
  Proof.
    intros store key. unfold prefetch, read.
    destruct (store key) as [views |] eqn:cached; simpl.
    - rewrite cached. reflexivity.
    - unfold update at 1. rewrite Nat.eqb_refl. reflexivity.
  Qed.

  (* C2 (DR-82): a metered cold read reserves its [cost] against [budget]
     before it inserts. A rejected reservation returns no view and leaves
     the cache unchanged. *)
  Definition read_metered (store : cache) (key : nat) (cost budget : nat)
    : option (list nat) * cache :=
    match store key with
    | Some views => (Some views, store)
    | None =>
        if Nat.leb cost budget then (Some (decode key), update store key (decode key))
        else (None, store)
    end.

  Theorem rejected_cold_read_leaves_cache : forall store key cost budget,
    store key = None -> budget < cost ->
    read_metered store key cost budget = (None, store).
  Proof.
    intros store key cost budget empty short. unfold read_metered. rewrite empty.
    destruct (Nat.leb cost budget) eqn:fits; [| reflexivity].
    apply Nat.leb_le in fits. lia.
  Qed.

  Theorem accepted_cold_read_equals_read : forall store key cost budget,
    cost <= budget -> read_metered store key cost budget =
      (Some (fst (read store key)), snd (read store key)).
  Proof.
    intros store key cost budget fits. unfold read_metered, read.
    destruct (store key); [reflexivity |].
    apply Nat.leb_le in fits. rewrite fits. reflexivity.
  Qed.

  (* Negative control: a read that inserts before it reserves leaves the
     cache filled after a rejected reservation. *)
  Definition read_insert_first (store : cache) (key : nat) (cost budget : nat)
    : option (list nat) * cache :=
    match store key with
    | Some views => (Some views, store)
    | None =>
        let filled := update store key (decode key) in
        if Nat.leb cost budget then (Some (decode key), filled) else (None, filled)
    end.
End Cache.

Example insert_first_fills_cache_on_rejection :
  snd (read_insert_first (fun _ => [1]) (fun _ => None) 0 2 1) 0 = Some [1].
Proof. vm_compute. reflexivity. Qed.

Section Release.
  (* Pointer counts and prepayment flags of the payloads. *)
  Record state := {
    count : nat -> nat;
    prepaid : nat -> bool;
    unpaid_release : bool
  }.

  Definition set_count (counts : nat -> nat) (target value : nat) : nat -> nat :=
    fun other => if Nat.eqb target other then value else counts other.

  Definition set_flag (flags : nat -> bool) (target : nat) (value : bool) : nat -> bool :=
    fun other => if Nat.eqb target other then value else flags other.

  (* Fill: a cold read creates a payload with two pointers (the cache and
     the returned view) and prepays its release, when [prepay] holds.
     Share: a warm read adds one pointer. Drop: a view or the cache entry
     drops one pointer, and the last drop releases the payload. *)
  Inductive event := Fill (target : nat) | Share (target : nat) | Drop (target : nat).

  Definition step_with (prepay : bool) (current : state) (happening : event) : state :=
    match happening with
    | Fill target =>
        if Nat.eqb (count current target) 0 then
          {| count := set_count (count current) target 2;
             prepaid := set_flag (prepaid current) target prepay;
             unpaid_release := unpaid_release current |}
        else current
    | Share target =>
        if Nat.eqb (count current target) 0 then current
        else {| count := set_count (count current) target (S (count current target));
                prepaid := prepaid current;
                unpaid_release := unpaid_release current |}
    | Drop target =>
        if Nat.eqb (count current target) 0 then current
        else {| count := set_count (count current) target (pred (count current target));
                prepaid := prepaid current;
                unpaid_release :=
                  unpaid_release current
                  || (Nat.eqb (pred (count current target)) 0 && negb (prepaid current target)) |}
    end.

  Definition step := step_with true.

  Definition live_prepaid (current : state) : Prop :=
    forall payload, 0 < count current payload -> prepaid current payload = true.

  Theorem cleanup_prepaid_preserved : forall current happening,
    live_prepaid current -> live_prepaid (step current happening).
  Proof.
    intros current [target | target | target] live payload positive;
      unfold step, step_with in *; simpl in *.
    - destruct (Nat.eqb (count current target) 0) eqn:empty; [| exact (live payload positive)].
      simpl in *. unfold set_count, set_flag in *.
      destruct (Nat.eqb target payload); [reflexivity |]. exact (live payload positive).
    - destruct (Nat.eqb (count current target) 0) eqn:empty; [exact (live payload positive) |].
      simpl in *. unfold set_count in positive.
      destruct (Nat.eqb target payload) eqn:same.
      + apply Nat.eqb_eq in same. subst payload. apply live.
        apply Nat.eqb_neq in empty. lia.
      + exact (live payload positive).
    - destruct (Nat.eqb (count current target) 0) eqn:empty; [exact (live payload positive) |].
      simpl in *. unfold set_count in positive.
      destruct (Nat.eqb target payload) eqn:same.
      + apply Nat.eqb_eq in same. subst payload. apply live.
        apply Nat.eqb_neq in empty. lia.
      + exact (live payload positive).
  Qed.

  Lemma step_keeps_paid_releases : forall current happening,
    live_prepaid current -> unpaid_release current = false ->
    unpaid_release (step current happening) = false.
  Proof.
    intros current [target | target | target] live paid; unfold step, step_with; simpl.
    - destruct (Nat.eqb (count current target) 0); exact paid.
    - destruct (Nat.eqb (count current target) 0); exact paid.
    - destruct (Nat.eqb (count current target) 0) eqn:empty; [exact paid |]. simpl.
      rewrite paid. simpl. apply Nat.eqb_neq in empty.
      rewrite (live target ltac:(lia)). apply andb_false_r.
  Qed.

  Theorem every_release_was_prepaid : forall trace current,
    live_prepaid current -> unpaid_release current = false ->
    unpaid_release (fold_left step trace current) = false.
  Proof.
    induction trace as [| happening rest IH]; intros current live paid; simpl; [exact paid |].
    apply IH.
    - apply cleanup_prepaid_preserved. exact live.
    - apply step_keeps_paid_releases; assumption.
  Qed.

  Definition empty_state : state :=
    {| count := fun _ => 0; prepaid := fun _ => false; unpaid_release := false |}.

  (* Negative control: a cold fill without prepayment, then the drop of the
     view and of the cache entry, releases the payload unpaid. *)
  Example fill_without_prepay_releases_unpaid :
    unpaid_release (fold_left (step_with false) [Fill 7; Drop 7; Drop 7] empty_state) = true.
  Proof. vm_compute. reflexivity. Qed.

  Example prepaid_fill_releases_paid :
    unpaid_release (fold_left step [Fill 7; Share 7; Drop 7; Drop 7; Drop 7] empty_state) = false.
  Proof. vm_compute. reflexivity. Qed.
End Release.

Print Assumptions shared_selection_equals_deep_selection.
Print Assumptions prefetch_then_read_equals_read.
Print Assumptions cleanup_prepaid_preserved.
Print Assumptions every_release_was_prepaid.
Print Assumptions fill_without_prepay_releases_unpaid.
Print Assumptions prepaid_fill_releases_paid.
Print Assumptions rejected_cold_read_leaves_cache.
Print Assumptions accepted_cold_read_equals_read.
Print Assumptions insert_first_fills_cache_on_rejection.
