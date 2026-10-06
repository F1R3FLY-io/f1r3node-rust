(* D-S3 (epic 8946, Phase D item D-C3; decision record DR-97): the native
   session exports only its dirty entries, by reference, and the checkpoint
   root stays the root of the full export.

   The native session store (rspace++/src/rspace/hot_store/native_store.rs)
   caches history values. A cold fill inserts the value that the history
   holds as a clean entry; every publishing write replaces the entry with a
   dirty one; a checkpoint and a restore copy whole entries, flags included.
   The full export wrote every cached entry, clean or dirty; the dirty export
   writes only the dirty ones.

   Two premises make the two exports agree:
   - a clean entry holds what the history holds, so writing it again does not
     change the history (update_idempotent). For a non-empty value, the
     stored leaf is the sorted list of encoded rows, and a cold fill decodes
     those rows; with an encoding that is stable under decode and encode, the
     decoded values re-encode to the same sorted leaf (leaf_reencodes,
     clean_cold_fill_round_trips). An empty value has no leaf; its export is
     a deletion of an absent key, which also changes nothing.
   - every reachable store keeps that property: cold fills insert clean
     history values, writes mark entries dirty, and checkpoints and restores
     keep the flags (dirty_invariant_preserved).

   K below is the history key (prefix and projection); the radix history is
   canonical, so its root is a function of the map from keys to leaves, and
   the model compares the maps. The encoding premise enc_dec_stable is
   discharged for the real rhoapi types by the Rust test
   clean_cold_fill_reencodes_to_stored_leaf, and the sortedness of stored
   leaves is a property of every writer (see DR-97).

   Results:
   - leaf_reencodes, clean_cold_fill_round_trips,
     written_history_fills_round_trip: a cold fill of a written leaf
     re-encodes to the same leaf, and an empty value to no leaf;
   - unsorted_leaf_reencodes_differently (premise control): a leaf whose rows
     are not sorted does not re-encode to itself, so the sortedness of stored
     leaves is needed;
   - update_idempotent: writing the value that a key holds keeps the map;
   - dirty_export_equals_full_export: for a store whose keys are distinct and
     whose clean entries hold history values, the dirty export and the full
     export give the same map;
   - dirty_invariant_preserved, reachable_dirty_export_equals_full_export:
     cold fills, writes, checkpoints and restores keep that property, so the
     two exports agree in every reachable state;
   - write_without_dirty_changes_root (negative control): a write that does
     not mark its entry dirty makes the two exports differ.

   Rust correspondence: NativeHotStore::dirty_entries and
   NativeDirtyEntries::actions (native_store.rs), NativeCheckpoint::prepare
   and prepare_borrowed (rspace++/src/rspace/history/native_checkpoint.rs),
   HistoryRepository::prepare_native_checkpoint_borrowed; tests in
   native_store/tests.rs, native_session/tests/history.rs and the rholang
   native_runtime history_decode tests. The TLA+ model
   NativeDirtyExport.tla checks the same agreement with two mutations. *)

From Stdlib Require Import Lists.List Arith.PeanoNat.
Import ListNotations.

(* ------------------------------------------------------------------------ *)
(* Byte strings, their order, and insertion sort                            *)

Definition bytes := list nat.

Fixpoint lex_le (a b : bytes) : bool :=
  match a, b with
  | [], _ => true
  | _ :: _, [] => false
  | x :: a_rest, y :: b_rest =>
      match Nat.compare x y with Lt => true | Gt => false | Eq => lex_le a_rest b_rest end
  end.

Lemma lex_le_total : forall a b, lex_le a b = false -> lex_le b a = true.
Proof.
  induction a as [| x a IH]; intros [| y b] le; cbn in *; try discriminate; try reflexivity.
  rewrite (Nat.compare_antisym x y).
  destruct (x ?= y) eqn:c; cbn in *; [apply IH; exact le | discriminate | reflexivity].
Qed.

Fixpoint insert (x : bytes) (l : list bytes) : list bytes :=
  match l with [] => [x] | y :: rest => if lex_le x y then x :: l else y :: insert x rest end.

Definition isort (l : list bytes) : list bytes := fold_right insert [] l.

Inductive sorted : list bytes -> Prop :=
| sorted_nil : sorted []
| sorted_one : forall x, sorted [x]
| sorted_cons : forall x y l, lex_le x y = true -> sorted (y :: l) -> sorted (x :: y :: l).

Lemma insert_sorted : forall x l, sorted l -> sorted (insert x l).
Proof.
  intros x l s. induction s as [| y | y z l yz s IH]; cbn.
  - apply sorted_one.
  - destruct (lex_le x y) eqn:xy.
    + apply sorted_cons; [exact xy | apply sorted_one].
    + apply sorted_cons; [apply lex_le_total; exact xy | apply sorted_one].
  - destruct (lex_le x y) eqn:xy.
    + apply sorted_cons; [exact xy | apply sorted_cons; assumption].
    + cbn in IH. destruct (lex_le x z) eqn:xz.
      * apply sorted_cons; [apply lex_le_total; exact xy | exact IH].
      * apply sorted_cons; [exact yz | exact IH].
Qed.

Lemma isort_sorted : forall l, sorted (isort l).
Proof.
  induction l as [| x l IH]; unfold isort in *; cbn; [apply sorted_nil |].
  apply insert_sorted. exact IH.
Qed.

Lemma isort_of_sorted : forall l, sorted l -> isort l = l.
Proof.
  intros l s. induction s as [| x | x y l xy s IH]; unfold isort in *; cbn;
    [reflexivity | reflexivity |].
  cbn in IH. rewrite IH. cbn. rewrite xy. reflexivity.
Qed.

Lemma insert_in : forall r x l, In r (insert x l) -> r = x \/ In r l.
Proof.
  intros r x l. induction l as [| y l IH]; cbn; intros inside.
  - destruct inside as [<- | []]. left. reflexivity.
  - destruct (lex_le x y).
    + destruct inside as [<- | inside]; [left; reflexivity | right; exact inside].
    + destruct inside as [<- | inside]; [right; left; reflexivity |].
      destruct (IH inside) as [same | rest]; [left; exact same | right; right; exact rest].
Qed.

Lemma isort_in : forall r l, In r (isort l) -> In r l.
Proof.
  intros r l. induction l as [| x l IH]; unfold isort in *; cbn; intros inside;
    [exact inside |].
  destruct (insert_in r x _ inside) as [same | rest].
  - left. symmetry. exact same.
  - right. apply IH. exact rest.
Qed.

Lemma insert_not_nil : forall x l, insert x l <> [].
Proof. intros x [| y l]; cbn; [discriminate |]. destruct (lex_le x y); discriminate. Qed.

(* ------------------------------------------------------------------------ *)
(* Leaves and cold fills                                                    *)

Section Codec.

Variable T : Type.
Variable enc : T -> bytes.
Variable dec : bytes -> option T.
(* Decoding an encoded value and encoding it again gives the same bytes. *)
Hypothesis enc_dec_stable : forall x y, dec (enc x) = Some y -> enc y = enc x.

Fixpoint decode_all (rows : list bytes) : option (list T) :=
  match rows with
  | [] => Some []
  | r :: rest =>
      match dec r, decode_all rest with Some y, Some ys => Some (y :: ys) | _, _ => None end
  end.

(* The leaf of a value: its encoded rows, sorted. *)
Definition leaf_of (xs : list T) : list bytes := isort (map enc xs).

(* An empty value has no leaf. *)
Definition value_leaf (xs : list T) : option (list bytes) :=
  match xs with [] => None | _ :: _ => Some (leaf_of xs) end.

(* A cold fill decodes the stored rows; an absent leaf reads as empty. *)
Definition fill_of (stored : option (list bytes)) : option (list T) :=
  match stored with None => Some [] | Some rows => decode_all rows end.

Lemma decode_all_reencodes : forall rows ys,
  (forall r, In r rows -> exists x, r = enc x) ->
  decode_all rows = Some ys -> map enc ys = rows.
Proof.
  induction rows as [| r rows IH]; intros ys encoded decoded; cbn in decoded.
  - injection decoded as <-. reflexivity.
  - destruct (dec r) as [y |] eqn:row; [| discriminate].
    destruct (decode_all rows) as [rest_values |] eqn:rest; [| discriminate].
    injection decoded as <-. cbn.
    destruct (encoded r (or_introl eq_refl)) as [x ->].
    rewrite (enc_dec_stable x y row).
    rewrite (IH rest_values (fun other inside => encoded other (or_intror inside)) eq_refl).
    reflexivity.
Qed.

Theorem leaf_reencodes : forall xs ys,
  decode_all (leaf_of xs) = Some ys -> leaf_of ys = leaf_of xs.
Proof.
  intros xs ys decoded. unfold leaf_of in *.
  rewrite (decode_all_reencodes (isort (map enc xs)) ys).
  - apply isort_of_sorted. apply isort_sorted.
  - intros r inside. apply isort_in in inside. apply in_map_iff in inside.
    destruct inside as [x [<- _]]. exists x. reflexivity.
  - exact decoded.
Qed.

Theorem clean_cold_fill_round_trips : forall xs ys,
  fill_of (value_leaf xs) = Some ys -> value_leaf ys = value_leaf xs.
Proof.
  intros [| x xs] ys filled; cbn in filled.
  - injection filled as <-. reflexivity.
  - pose proof (leaf_reencodes (x :: xs) ys filled) as same. destruct ys as [| y ys].
    + exfalso. unfold leaf_of in same. cbn in same. symmetry in same.
      exact (insert_not_nil _ _ same).
    + exact (f_equal Some same).
Qed.

End Codec.

Corollary written_history_fills_round_trip :
  forall (T K : Type) (enc : T -> bytes) (dec : bytes -> option T) (written fill : K -> list T),
  (forall x y, dec (enc x) = Some y -> enc y = enc x) ->
  (forall k, fill_of T dec (value_leaf T enc (written k)) = Some (fill k)) ->
  forall k, value_leaf T enc (fill k) = value_leaf T enc (written k).
Proof.
  intros T K enc dec written fill stable filled k.
  exact (clean_cold_fill_round_trips T enc dec stable (written k) (fill k) (filled k)).
Qed.

(* Premise control: the reader accepts a leaf whose rows are not sorted, and
   such a leaf does not re-encode to itself. *)
Example unsorted_leaf_reencodes_differently :
  let enc := fun n : nat => [n] in
  let dec := fun r : bytes => match r with [n] => Some n | _ => None end in
  decode_all nat dec [[2]; [1]] = Some [2; 1] /\
  leaf_of nat enc [2; 1] = [[1]; [2]] /\ leaf_of nat enc [2; 1] <> [[2]; [1]].
Proof. cbn. repeat split; try reflexivity; discriminate. Qed.

(* ------------------------------------------------------------------------ *)
(* The dirty export and the full export                                     *)

Section Export.

Variables K V L : Type.
Variable key_eq_dec : forall a b : K, {a = b} + {a <> b}.
Variable leaf : V -> option L.
Variable hist : K -> option L.
Variable fill : K -> V.
(* A cold fill of a key holds what the history holds. *)
Hypothesis fill_round_trips : forall k, leaf (fill k) = hist k.

Inductive action := Put (k : K) (l : L) | Remove (k : K).

Definition target (a : action) : K := match a with Put k _ | Remove k => k end.

Definition written (a : action) : option L :=
  match a with Put _ l => Some l | Remove _ => None end.

Definition apply_action (m : K -> option L) (a : action) : K -> option L :=
  fun k => if key_eq_dec (target a) k then written a else m k.

Definition apply_all (m : K -> option L) (actions : list action) : K -> option L :=
  fold_left apply_action actions m.

Definition same_root (m1 m2 : K -> option L) : Prop := forall k, m1 k = m2 k.

Theorem update_idempotent : forall m a,
  m (target a) = written a -> same_root (apply_action m a) m.
Proof.
  intros m a holds k. unfold apply_action.
  destruct (key_eq_dec (target a) k) as [<- | _]; [symmetry; exact holds | reflexivity].
Qed.

Lemma apply_all_ext : forall actions m1 m2,
  same_root m1 m2 -> same_root (apply_all m1 actions) (apply_all m2 actions).
Proof.
  induction actions as [| a actions IH]; intros m1 m2 same; [exact same |].
  apply IH. intros k. unfold apply_action.
  destruct (key_eq_dec (target a) k); [reflexivity | apply same].
Qed.

Record entry := { ekey : K; evalue : V; edirty : bool }.

Definition store := list entry.

Definition export_action (e : entry) : action :=
  match leaf (evalue e) with Some l => Put (ekey e) l | None => Remove (ekey e) end.

Definition full_export (s : store) : list action := map export_action s.

Definition dirty_export (s : store) : list action := map export_action (filter edirty s).

Definition clean_ok (s : store) : Prop :=
  forall e, In e s -> edirty e = false -> leaf (evalue e) = hist (ekey e).

Definition store_ok (s : store) : Prop := NoDup (map ekey s) /\ clean_ok s.

Lemma export_target : forall e, target (export_action e) = ekey e.
Proof. intros e. unfold export_action. destruct (leaf (evalue e)); reflexivity. Qed.

Lemma export_written : forall e, written (export_action e) = leaf (evalue e).
Proof. intros e. unfold export_action. destruct (leaf (evalue e)); reflexivity. Qed.

Lemma dirty_export_from : forall s m,
  NoDup (map ekey s) -> clean_ok s ->
  (forall e, In e s -> m (ekey e) = hist (ekey e)) ->
  same_root (apply_all m (dirty_export s)) (apply_all m (full_export s)).
Proof.
  induction s as [| e s IH]; intros m distinct clean agree.
  - intros k. reflexivity.
  - cbn in distinct. apply NoDup_cons_iff in distinct as [fresh rest].
    assert (clean_rest : clean_ok s) by (intros other inside; apply clean; right; exact inside).
    unfold dirty_export, full_export. cbn [filter map]. destruct (edirty e) eqn:flag.
    + change (same_root (apply_all (apply_action m (export_action e)) (dirty_export s))
                        (apply_all (apply_action m (export_action e)) (full_export s))).
      apply IH; [exact rest | exact clean_rest |].
      intros other inside. unfold apply_action. rewrite export_target.
      destruct (key_eq_dec (ekey e) (ekey other)) as [same | _];
        [| apply agree; right; exact inside].
      exfalso. apply fresh. rewrite same. apply in_map. exact inside.
    + change (same_root (apply_all m (dirty_export s))
                        (apply_all (apply_action m (export_action e)) (full_export s))).
      assert (noop : same_root (apply_action m (export_action e)) m).
      { apply update_idempotent. rewrite export_target, export_written.
        rewrite (clean e (or_introl eq_refl) flag). apply agree. left. reflexivity. }
      intros k. rewrite (apply_all_ext (full_export s) _ _ noop k).
      apply IH; [exact rest | exact clean_rest |].
      intros other inside. apply agree. right. exact inside.
Qed.

Theorem dirty_export_equals_full_export : forall s,
  store_ok s -> same_root (apply_all hist (dirty_export s)) (apply_all hist (full_export s)).
Proof.
  intros s [distinct clean]. apply dirty_export_from; [exact distinct | exact clean |].
  intros e _. reflexivity.
Qed.

Definition keys (s : store) : list K := map ekey s.

Definition cold_fill (s : store) (k : K) : store :=
  {| ekey := k; evalue := fill k; edirty := false |} :: s.

Definition write (s : store) (k : K) (v : V) : store :=
  map (fun e => if key_eq_dec (ekey e) k then {| ekey := ekey e; evalue := v; edirty := true |}
                else e) s.

(* A write that keeps the old flag: the mutation of the negative control. *)
Definition write_unflagged (s : store) (k : K) (v : V) : store :=
  map (fun e => if key_eq_dec (ekey e) k
                then {| ekey := ekey e; evalue := v; edirty := edirty e |} else e) s.

(* The session: the store and the last checkpoint. *)
Inductive step : store * option store -> store * option store -> Prop :=
| step_cold_fill : forall s c k, ~ In k (keys s) -> step (s, c) (cold_fill s k, c)
| step_write : forall s c k v, In k (keys s) -> step (s, c) (write s k v, c)
| step_checkpoint : forall s c, step (s, c) (s, Some s)
| step_restore : forall s c, step (s, Some c) (c, Some c).

Definition state_ok (st : store * option store) : Prop :=
  store_ok (fst st) /\ match snd st with Some c => store_ok c | None => True end.

Lemma write_keys : forall s k v, keys (write s k v) = keys s.
Proof.
  intros s k v. unfold keys, write. rewrite map_map. apply map_ext.
  intros e. destruct (key_eq_dec (ekey e) k); reflexivity.
Qed.

Lemma cold_fill_ok : forall s k, ~ In k (keys s) -> store_ok s -> store_ok (cold_fill s k).
Proof.
  intros s k fresh [distinct clean]. split.
  - cbn. constructor; [exact fresh | exact distinct].
  - intros e [<- | inside] flag;
      [cbn; apply fill_round_trips | apply clean; [exact inside | exact flag]].
Qed.

Lemma write_ok : forall s k v, store_ok s -> store_ok (write s k v).
Proof.
  intros s k v [distinct clean]. split.
  - change (NoDup (keys (write s k v))). rewrite write_keys. exact distinct.
  - intros e inside flag. unfold write in inside. apply in_map_iff in inside.
    destruct inside as [other [built inside]]. destruct (key_eq_dec (ekey other) k).
    + subst e. discriminate flag.
    + subst e. apply clean; [exact inside | exact flag].
Qed.

Theorem dirty_invariant_preserved : forall st st',
  state_ok st -> step st st' -> state_ok st'.
Proof.
  intros st st' [now saved] transition.
  destruct transition as [s c k fresh | s c k v _ | s c | s c]; cbn in *.
  - split; [apply cold_fill_ok; assumption | exact saved].
  - split; [apply write_ok; exact now | exact saved].
  - split; exact now.
  - split; exact saved.
Qed.

Inductive reachable : store * option store -> Prop :=
| reach_start : reachable ([], None)
| reach_step : forall st st', reachable st -> step st st' -> reachable st'.

Lemma reachable_ok : forall st, reachable st -> state_ok st.
Proof.
  intros st reach. induction reach as [| st st' _ IH transition].
  - split; [split; [constructor | intros e []] | exact I].
  - exact (dirty_invariant_preserved st st' IH transition).
Qed.

Theorem reachable_dirty_export_equals_full_export : forall st, reachable st ->
  same_root (apply_all hist (dirty_export (fst st))) (apply_all hist (full_export (fst st))).
Proof.
  intros st reach. apply dirty_export_equals_full_export. exact (proj1 (reachable_ok st reach)).
Qed.

End Export.

(* Negative control: a write that keeps the clean flag makes the dirty export
   drop it, so the two exports differ. *)
Example write_without_dirty_changes_root :
  let leaf := fun v : nat => Some v in
  let hist := fun _ : nat => Some 0 in
  let s := write_unflagged nat nat Nat.eq_dec (cold_fill nat nat (fun _ => 0) [] 0) 0 1 in
  apply_all nat nat Nat.eq_dec hist (dirty_export nat nat nat leaf s) 0 = Some 0 /\
  apply_all nat nat Nat.eq_dec hist (full_export nat nat nat leaf s) 0 = Some 1 /\
  ~ same_root nat nat (apply_all nat nat Nat.eq_dec hist (dirty_export nat nat nat leaf s))
                      (apply_all nat nat Nat.eq_dec hist (full_export nat nat nat leaf s)).
Proof.
  cbn. split; [reflexivity | split; [reflexivity |]].
  intros same. specialize (same 0). cbn in same. discriminate same.
Qed.

Print Assumptions leaf_reencodes.
Print Assumptions clean_cold_fill_round_trips.
Print Assumptions written_history_fills_round_trip.
Print Assumptions unsorted_leaf_reencodes_differently.
Print Assumptions update_idempotent.
Print Assumptions dirty_export_equals_full_export.
Print Assumptions dirty_invariant_preserved.
Print Assumptions reachable_dirty_export_equals_full_export.
Print Assumptions write_without_dirty_changes_root.
