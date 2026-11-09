(* ═══════════════════════════════════════════════════════════════════════════
   Checkpoint.v — the checkpoint, the reset, the soft checkpoint and the
   replay of the hot store over the history

   This module models how the host RSpace writes its hot store into the
   history at a checkpoint, how a reset (or a restart) reads a root of the
   history back into a fresh hot store, how a soft checkpoint and its revert
   undo a failed deploy, and how a replay rigged with the log of play
   reproduces play. It is the successor of the legacy module Checkpoint.v of
   the legacy branch (f1r3node-reified-rspaces at ff479042). The legacy
   module declared apply_operation and the hash as a Parameter and stated
   two Axioms (hash validity and collision resistance). Here the operations,
   the checkpoint, the history, the reset, the soft checkpoint and the rig
   are concrete definitions. The premises on the encoders, the key function,
   the byte order and the root are explicit hypotheses of the theorems, and
   the lemma checkpoint_premises_satisfiable shows all of them for one
   concrete instance. The module also absorbs the codex module
   ReplayDeterminism.v (kept in the P0 backup): its theorems
   replay_from_equal_parent_states and replay_replay are the first and the
   last conjunct of C3 here, stated for the hot store of this module.

   Code base: the campaign tree after the merge of feature/cost-accounted-rho
   at 4d67b5ccc. The host files that this header cites equal that commit.
   Design: P10 design v2, section 2.2 (plan decision 85), and leaf
   rrs-fv-port-checkpoint (plan:989).

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   (rspace++/src/rspace/ is abbreviated as rs/)
   ─────────────────────────────────────────────────────────────────────────
   Datum, d_persist          rs/internal.rs:16-20 (Datum { a, persist,
                             source }: the flag is a field of the record)
   WK, w_persist             rs/internal.rs (WaitingContinuation, persist)
   CKey                      the three key kinds of the hot store: the data
                             of a channel, the continuations of a channel
                             group and the joins of a channel
                             (rs/hot_store.rs, HotStoreState)
   ser_datum, ser_wk,        rs/serializers/serializers.rs:18-26, 39-50,
     ser_join, bytes_leb     69-77 (encode_*: bincode each record, sort with
                             compare_byte_vectors)
   leaf_of, action_of,       rs/hot_store.rs changes() (an empty list
     changes                 becomes a delete action, any other list an
                             insert action of its sorted encodings),
                             rs/history/history_repository_impl.rs:169-304
                             (calculate_storage_actions)
   Trie, tlookup, tinsert,   rs/history/radix_tree.rs (the key-to-leaf map
     tdelete, make_actions   of the radix tree), rs/history/
                             history_repository_impl.rs:467-490 (checkpoint)
   History, lookup_root      the content-addressed store of the history: a
                             root names the trie that it was computed from
                             (history_repository_impl.rs:569, reset)
   HotStore, touch, view     rs/hot_store.rs (a first access caches the key;
                             an uncached key reads through to the history)
   reset_view, reset         rs/rspace/ispace_impl.rs:97-110 (reset:
                             create_new_hot_store over the reader of the
                             root), :60-95 (create_checkpoint: changes, the
                             history checkpoint, then a new hot store over
                             the new root)
   decode_*                  rs/history/history_reader.rs (get_data,
                             get_continuations, get_joins decode a leaf)
   Op, apply_operation       the store steps of a produce, a consume, a COMM
                             and the volatile sweep: rs/rspace/rspace.rs
                             store_data, store_waiting_continuation,
                             remove_matched_datum_and_join, and
                             rs/space_store_handle.rs sweep_volatile
   snapshot, revert          rs/rspace/ispace_impl.rs:197-207, 215-239
                             (create_soft_checkpoint: the cache snapshot;
                             revert_to_soft_checkpoint:
                             create_from_hs_and_hr(cache_snapshot, base))
   Rig, rig_and_reset,       rs/replay_rspace.rs:475-560 (rig_and_reset,
     replay_step, complete   rig, check_replay_data)

   ─────────────────────────────────────────────────────────────────────────
   Modeling notes
   ─────────────────────────────────────────────────────────────────────────
   - The trie is an association list from history keys to leaves, as in
     VolatileSweep.v. A leaf is the sorted list of the record encodings.
     The kind of a leaf is the kind of its key.
   - The hot store is three total functions (the read-through view) and a
     cache list of the keys that the deploy touched. An uncached key reads
     through to the trie (reads_through). Every step touches a key before
     it writes the key, so the steps keep reads_through.
   - A step changes the hot store as the host step does. A produce appends a
     datum. A consume appends a waiter and adds the group to the joins of
     each of its channels. A COMM removes the waiter and each matched datum,
     unless the record is persistent, and it removes the join entries of a
     group whose last waiter leaves. A removal takes the first equal record,
     as remove_matched_datum_and_join finds the matched record by equality.
     The volatile sweep clears the data and the joins of the swept channels
     and the continuations of the groups that the sweep lists.
     VolatileSweep.v proves which groups the sweep lists (sweep_exact).
   - The checkpoint applies one action for each cached key. VolatileSweep.v
     shows that the history rejects two actions on one history key (H22).
     A cache without duplicates and an injective key function exclude them,
     so the model applies the actions in order.
   - The history is the list of the checkpointed tries with their roots. A
     reset looks up the first trie with the root. Collision resistance of
     the root (root_complete) makes every trie with one root read alike.
   - The rig is the log of play in play order, and a position. The replay
     of the host pairs events by identity (EventCorrelation.v, H4 and H23).
     Here the replay re-executes the operations of the deploy in the order
     of the deterministic schedule, so the log is a sequence.
   - The premises are hypotheses of the theorems, not Section hypotheses:
     roundtrips (each decoder inverts its encoder), key_injective,
     bytes_order (a total order on the encodings) and root_complete. The
     lemma checkpoint_premises_satisfiable shows them for one instance.

   ─────────────────────────────────────────────────────────────────────────
   Headlines
   ─────────────────────────────────────────────────────────────────────────
   C1 checkpoint_reset_restores_store   a reset at the root of a checkpoint
                                        gives back every record of the
                                        checkpointed hot store
   C2 checkpoint_restore_preserves_persist
                                        a persistent datum keeps its flag
                                        across the checkpoint and the reset,
                                        and a later COMM keeps it (legacy
                                        defect D11 cannot occur)
   C3 replay_from_equal_parent_states   equivalent parents give equivalent
                                        hot stores and one checkpoint
   C4 checkpoint_root_order_independent the checkpoint does not depend on the
                                        order of the records of a key
   C5 rig_and_reset_replays_the_log     the rig accepts exactly the log of
                                        play and reproduces play
   C6 reset_after_sweep_drops_volatile  after the sweep and the checkpoint, a
                                        reset holds no record of a swept key
   C7 soft_revert_after_prefix_failure  a revert restores the hot store of
                                        the soft checkpoint, whatever prefix
                                        of the deploy ran (decision 19 D6)
   C8 chain_checkpoints_retrievable     every root of the history resets to
                                        a store that reads as its trie
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List Bool Arith PeanoNat Permutation Lia.
Import ListNotations.

(* ═══════════════════ Generic list facts ═══════════════════ *)

Fixpoint insb {A : Type} (leb : A -> A -> bool) (x : A) (l : list A) : list A :=
  match l with
  | [] => [x]
  | y :: r => if leb x y then x :: y :: r else y :: insb leb x r
  end.

Fixpoint isort {A : Type} (leb : A -> A -> bool) (l : list A) : list A :=
  match l with
  | [] => []
  | x :: r => insb leb x (isort leb r)
  end.

Lemma insb_perm : forall (A : Type) leb (x : A) l, Permutation (insb leb x l) (x :: l).
Proof.
  intros A leb x l. induction l as [| y r IH]; simpl; [reflexivity |].
  destruct (leb x y); [reflexivity |].
  eapply perm_trans; [apply perm_skip; exact IH | apply perm_swap].
Qed.

Lemma isort_perm : forall (A : Type) leb (l : list A), Permutation (isort leb l) l.
Proof.
  intros A leb l. induction l as [| x r IH]; simpl; [reflexivity |].
  eapply perm_trans; [apply insb_perm | apply perm_skip; exact IH].
Qed.

Section Sorting.
  Variable A : Type.
  Variable leb : A -> A -> bool.

  Definition total_order : Prop :=
    (forall x y, leb x y = true \/ leb y x = true) /\
    (forall x y z, leb x y = true -> leb y z = true -> leb x z = true) /\
    (forall x y, leb x y = true -> leb y x = true -> x = y).

  Inductive ssorted : list A -> Prop :=
  | ssorted_nil : ssorted []
  | ssorted_cons : forall x l, Forall (fun y => leb x y = true) l -> ssorted l -> ssorted (x :: l).

  Lemma insb_ssorted :
    total_order -> forall x l, ssorted l -> ssorted (insb leb x l).
  Proof.
    intros [Htot [Htr _]] x l Hs. induction Hs as [| y l Hy Hs IH]; simpl.
    - constructor; constructor.
    - destruct (leb x y) eqn:Exy.
      + constructor; [| constructor; assumption].
        constructor; [exact Exy |].
        eapply Forall_impl; [| exact Hy]. intros z Hz. eapply Htr; eassumption.
      + assert (Hyx : leb y x = true) by (destruct (Htot x y); [congruence | assumption]).
        constructor; [| exact IH].
        apply (Permutation_Forall (Permutation_sym (insb_perm A leb x l))).
        constructor; assumption.
  Qed.

  Lemma isort_ssorted : total_order -> forall l, ssorted (isort leb l).
  Proof.
    intros Hord l. induction l as [| x r IH]; simpl; [constructor |].
    apply insb_ssorted; assumption.
  Qed.

  Lemma ssorted_perm_eq :
    total_order -> forall l1 l2, ssorted l1 -> ssorted l2 -> Permutation l1 l2 -> l1 = l2.
  Proof.
    intros [_ [_ Hanti]] l1. induction l1 as [| x r IH]; intros l2 H1 H2 Hp.
    - symmetry. apply Permutation_nil. exact Hp.
    - destruct l2 as [| y s]; [apply Permutation_sym, Permutation_nil in Hp; discriminate |].
      inversion H1 as [| ? ? Hx Hr]; subst. inversion H2 as [| ? ? Hy Hs]; subst.
      assert (Hxy : x = y).
      { destruct (Permutation_in x Hp (or_introl eq_refl)) as [Ex | Ex]; [symmetry; exact Ex |].
        destruct (Permutation_in y (Permutation_sym Hp) (or_introl eq_refl)) as [Ey | Ey];
          [exact Ey |].
        apply Hanti.
        - exact (proj1 (Forall_forall _ _) Hx y Ey).
        - exact (proj1 (Forall_forall _ _) Hy x Ex). }
      subst y. f_equal. apply IH; [exact Hr | exact Hs |].
      exact (Permutation_cons_inv Hp).
  Qed.

  Lemma isort_canonical :
    total_order -> forall l1 l2, Permutation l1 l2 -> isort leb l1 = isort leb l2.
  Proof.
    intros Hord l1 l2 Hp. apply ssorted_perm_eq; [exact Hord | apply isort_ssorted; exact Hord
      | apply isort_ssorted; exact Hord |].
    eapply perm_trans; [apply isort_perm |].
    eapply perm_trans; [exact Hp | apply Permutation_sym, isort_perm].
  Qed.
End Sorting.

Fixpoint remove_first {A : Type} (eqd : forall x y : A, {x = y} + {x <> y}) (x : A) (l : list A)
  : list A :=
  match l with
  | [] => []
  | y :: r => if eqd x y then r else y :: remove_first eqd x r
  end.

Lemma remove_first_in :
  forall (A : Type) eqd (x : A) l, In x l -> Permutation l (x :: remove_first eqd x l).
Proof.
  intros A eqd x l. induction l as [| y r IH]; intros Hin; [destruct Hin |].
  simpl. destruct (eqd x y) as [-> | Hne]; [reflexivity |].
  destruct Hin as [-> | Hin]; [congruence |].
  eapply perm_trans; [apply perm_skip; exact (IH Hin) | apply perm_swap].
Qed.

Lemma remove_first_notin :
  forall (A : Type) eqd (x : A) l, ~ In x l -> remove_first eqd x l = l.
Proof.
  intros A eqd x l. induction l as [| y r IH]; intros Hn; simpl; [reflexivity |].
  destruct (eqd x y) as [-> | Hne]; [exfalso; apply Hn; left; reflexivity |].
  f_equal. apply IH. intros Hin. apply Hn. right. exact Hin.
Qed.

Lemma remove_first_perm :
  forall (A : Type) eqd (x : A) l1 l2,
    Permutation l1 l2 -> Permutation (remove_first eqd x l1) (remove_first eqd x l2).
Proof.
  intros A eqd x l1 l2 Hp.
  destruct (in_dec eqd x l1) as [Hin | Hn].
  - assert (Hin2 : In x l2) by exact (Permutation_in x Hp Hin).
    apply (@Permutation_cons_inv _ _ _ x).
    eapply perm_trans; [apply Permutation_sym, remove_first_in; exact Hin |].
    eapply perm_trans; [exact Hp | apply remove_first_in; exact Hin2].
  - assert (Hn2 : ~ In x l2) by (intros Hin; apply Hn; exact (Permutation_in x (Permutation_sym Hp) Hin)).
    rewrite (remove_first_notin A eqd x l1 Hn), (remove_first_notin A eqd x l2 Hn2). exact Hp.
Qed.

Lemma remove_first_keeps_other :
  forall (A : Type) eqd (x y : A) l, x <> y -> In y l -> In y (remove_first eqd x l).
Proof.
  intros A eqd x y l Hne. induction l as [| z r IH]; intros Hin; [destruct Hin |].
  simpl. destruct (eqd x z) as [-> | Hz].
  - destruct Hin as [-> | Hin]; [congruence | exact Hin].
  - destruct Hin as [-> | Hin]; [left; reflexivity | right; apply IH; exact Hin].
Qed.

Definition add_absent {A : Type} (eqd : forall x y : A, {x = y} + {x <> y}) (x : A) (l : list A)
  : list A :=
  if in_dec eqd x l then l else l ++ [x].

Lemma add_absent_perm :
  forall (A : Type) eqd (x : A) l1 l2,
    Permutation l1 l2 -> Permutation (add_absent eqd x l1) (add_absent eqd x l2).
Proof.
  intros A eqd x l1 l2 Hp. unfold add_absent.
  destruct (in_dec eqd x l1) as [H1 | H1]; destruct (in_dec eqd x l2) as [H2 | H2].
  - exact Hp.
  - exfalso. apply H2. exact (Permutation_in x Hp H1).
  - exfalso. apply H1. exact (Permutation_in x (Permutation_sym Hp) H2).
  - apply Permutation_app_tail. exact Hp.
Qed.

Definition drop_all {A : Type} (eqd : forall x y : A, {x = y} + {x <> y}) (x : A) (l : list A)
  : list A :=
  filter (fun y => if eqd x y then false else true) l.

Lemma filter_perm :
  forall (A : Type) (f : A -> bool) l1 l2, Permutation l1 l2 -> Permutation (filter f l1) (filter f l2).
Proof.
  intros A f l1 l2 Hp. induction Hp; simpl.
  - reflexivity.
  - destruct (f x); [apply perm_skip |]; exact IHHp.
  - destruct (f x), (f y); try apply perm_swap; reflexivity.
  - eapply perm_trans; eassumption.
Qed.

Lemma drop_all_perm :
  forall (A : Type) eqd (x : A) l1 l2,
    Permutation l1 l2 -> Permutation (drop_all eqd x l1) (drop_all eqd x l2).
Proof. intros. unfold drop_all. apply filter_perm. assumption. Qed.

(* ═══════════════════ The hot store and the history ═══════════════════ *)

Section CheckpointModel.

Variable Chan Val Pat Bytes HKey Root : Type.
Variable chan_eq_dec : forall x y : Chan, {x = y} + {x <> y}.
Variable val_eq_dec : forall x y : Val, {x = y} + {x <> y}.
Variable pat_eq_dec : forall x y : Pat, {x = y} + {x <> y}.
Variable bytes_eq_dec : forall x y : Bytes, {x = y} + {x <> y}.
Variable hkey_eq_dec : forall x y : HKey, {x = y} + {x <> y}.
Variable root_eq_dec : forall x y : Root, {x = y} + {x <> y}.

Record Datum := mkDatum { d_val : Val; d_persist : bool }.
Record WK := mkWK { w_pat : Pat; w_persist : bool }.
Definition Group := list Chan.

Definition datum_eq_dec : forall x y : Datum, {x = y} + {x <> y}.
Proof. decide equality; first [apply Bool.bool_dec | apply val_eq_dec]. Defined.

Definition wk_eq_dec : forall x y : WK, {x = y} + {x <> y}.
Proof. decide equality; first [apply Bool.bool_dec | apply pat_eq_dec]. Defined.

Definition group_eq_dec : forall x y : Group, {x = y} + {x <> y} := list_eq_dec chan_eq_dec.

Inductive CKey := CData (c : Chan) | CCont (g : Group) | CJoins (c : Chan).

Definition ckey_eq_dec : forall x y : CKey, {x = y} + {x <> y}.
Proof. decide equality; first [apply chan_eq_dec | apply group_eq_dec]. Defined.

Variable ser_datum : Datum -> Bytes.
Variable de_datum : Bytes -> option Datum.
Variable ser_wk : WK -> Bytes.
Variable de_wk : Bytes -> option WK.
Variable ser_join : Group -> Bytes.
Variable de_join : Bytes -> option Group.
Variable bytes_leb : Bytes -> Bytes -> bool.
Variable hkey_of : CKey -> HKey.
Variable root_of : list (HKey * list Bytes) -> Root.

(* ── The premises ── *)

Definition roundtrips : Prop :=
  (forall d, de_datum (ser_datum d) = Some d) /\
  (forall w, de_wk (ser_wk w) = Some w) /\
  (forall g, de_join (ser_join g) = Some g).

Definition key_injective : Prop := forall k1 k2, hkey_of k1 = hkey_of k2 -> k1 = k2.

Definition bytes_order : Prop := total_order Bytes bytes_leb.

(* ── The trie ── *)

Definition Trie := list (HKey * list Bytes).

Fixpoint tlookup (k : HKey) (t : Trie) : option (list Bytes) :=
  match t with
  | [] => None
  | (k', l) :: r => if hkey_eq_dec k k' then Some l else tlookup k r
  end.

Definition tinsert (k : HKey) (l : list Bytes) (t : Trie) : Trie :=
  (k, l) :: filter (fun e => if hkey_eq_dec k (fst e) then false else true) t.

Definition tdelete (k : HKey) (t : Trie) : Trie :=
  filter (fun e => if hkey_eq_dec k (fst e) then false else true) t.

Inductive Action := AInsert (k : HKey) (l : list Bytes) | ADelete (k : HKey).

Definition action_key (a : Action) : HKey := match a with AInsert k _ => k | ADelete k => k end.

Definition apply_action (t : Trie) (a : Action) : Trie :=
  match a with AInsert k l => tinsert k l t | ADelete k => tdelete k t end.

Definition make_actions (t : Trie) (acts : list Action) : Trie := fold_left apply_action acts t.

Lemma tlookup_filter_other :
  forall k k' t, k <> k' ->
    tlookup k (filter (fun e => if hkey_eq_dec k' (fst e) then false else true) t) = tlookup k t.
Proof.
  intros k k' t Hne. induction t as [| [k0 l0] r IH]; simpl; [reflexivity |].
  destruct (hkey_eq_dec k' k0) as [-> | Hne0]; simpl.
  - destruct (hkey_eq_dec k k0) as [-> | _]; [congruence | exact IH].
  - destruct (hkey_eq_dec k k0); [reflexivity | exact IH].
Qed.

Lemma tlookup_filter_self :
  forall k t, tlookup k (filter (fun e => if hkey_eq_dec k (fst e) then false else true) t) = None.
Proof.
  intros k t. induction t as [| [k0 l0] r IH]; simpl; [reflexivity |].
  destruct (hkey_eq_dec k k0) as [-> | Hne]; simpl; [exact IH |].
  destruct (hkey_eq_dec k k0); [congruence | exact IH].
Qed.

Definition action_effect (a : Action) : option (list Bytes) :=
  match a with AInsert _ l => Some l | ADelete _ => None end.

Lemma apply_action_other :
  forall t a k, k <> action_key a -> tlookup k (apply_action t a) = tlookup k t.
Proof.
  intros t [k' l | k'] k Hne; simpl in *.
  - destruct (hkey_eq_dec k k'); [congruence |]. apply tlookup_filter_other. exact Hne.
  - apply tlookup_filter_other. exact Hne.
Qed.

Lemma apply_action_self :
  forall t a, tlookup (action_key a) (apply_action t a) = action_effect a.
Proof.
  intros t [k l | k]; simpl.
  - destruct (hkey_eq_dec k k); [reflexivity | congruence].
  - apply tlookup_filter_self.
Qed.

Lemma make_actions_other :
  forall acts t k, ~ In k (map action_key acts) -> tlookup k (make_actions t acts) = tlookup k t.
Proof.
  induction acts as [| a r IH]; intros t k Hn; simpl; [reflexivity |].
  rewrite IH by (intros Hin; apply Hn; right; exact Hin).
  apply apply_action_other. intros ->. apply Hn. left. reflexivity.
Qed.

Lemma make_actions_in :
  forall acts t a, NoDup (map action_key acts) -> In a acts ->
    tlookup (action_key a) (make_actions t acts) = action_effect a.
Proof.
  induction acts as [| b r IH]; intros t a Hnd Hin; [destruct Hin |].
  simpl in Hnd. inversion Hnd as [| ? ? Hb Hr]; subst. simpl.
  destruct Hin as [-> | Hin].
  - rewrite make_actions_other by exact Hb. apply apply_action_self.
  - apply IH; assumption.
Qed.

(* ── The hot store ── *)

Record HotStore := mkHS {
  hs_data : Chan -> list Datum;
  hs_conts : Group -> list WK;
  hs_joins : Chan -> list Group;
  hs_cache : list CKey
}.

Definition touch (k : CKey) (S : HotStore) : HotStore :=
  if in_dec ckey_eq_dec k (hs_cache S) then S
  else mkHS (hs_data S) (hs_conts S) (hs_joins S) (hs_cache S ++ [k]).

Definition set_data (c : Chan) (l : list Datum) (S : HotStore) : HotStore :=
  let S := touch (CData c) S in
  mkHS (fun c' => if chan_eq_dec c c' then l else hs_data S c') (hs_conts S) (hs_joins S) (hs_cache S).

Definition set_conts (g : Group) (l : list WK) (S : HotStore) : HotStore :=
  let S := touch (CCont g) S in
  mkHS (hs_data S) (fun g' => if group_eq_dec g g' then l else hs_conts S g') (hs_joins S) (hs_cache S).

Definition set_joins (c : Chan) (l : list Group) (S : HotStore) : HotStore :=
  let S := touch (CJoins c) S in
  mkHS (hs_data S) (hs_conts S) (fun c' => if chan_eq_dec c c' then l else hs_joins S c') (hs_cache S).

(* ── The operations ── *)

Inductive Op :=
| OpProduce (c : Chan) (d : Datum)
| OpConsume (g : Group) (w : WK)
| OpComm (g : Group) (w : WK) (ds : list (Chan * Datum))
| OpSweep (chans : list Chan) (groups : list Group).

Definition cd_eq_dec : forall x y : Chan * Datum, {x = y} + {x <> y}.
Proof. decide equality; first [apply datum_eq_dec | apply chan_eq_dec]. Defined.

Definition op_eq_dec : forall x y : Op, {x = y} + {x <> y}.
Proof.
  decide equality;
    first [ apply chan_eq_dec | apply datum_eq_dec | apply wk_eq_dec | apply group_eq_dec
          | apply (list_eq_dec group_eq_dec) | apply (list_eq_dec chan_eq_dec)
          | apply (list_eq_dec cd_eq_dec) ].
Defined.

Definition add_join (g : Group) (S : HotStore) (c : Chan) : HotStore :=
  set_joins c (add_absent group_eq_dec g (hs_joins S c)) S.

Definition drop_join (g : Group) (S : HotStore) (c : Chan) : HotStore :=
  set_joins c (drop_all group_eq_dec g (hs_joins S c)) S.

Definition take_datum (S : HotStore) (cd : Chan * Datum) : HotStore :=
  let (c, d) := cd in
  if d_persist d then S else set_data c (remove_first datum_eq_dec d (hs_data S c)) S.

Definition nullb {A : Type} (l : list A) : bool := match l with [] => true | _ => false end.

Definition take_waiter (g : Group) (w : WK) (S : HotStore) : HotStore :=
  if w_persist w then S
  else
    let S := set_conts g (remove_first wk_eq_dec w (hs_conts S g)) S in
    if nullb (hs_conts S g) then fold_left (drop_join g) g S else S.

Definition clear_channel (S : HotStore) (c : Chan) : HotStore :=
  set_joins c [] (set_data c [] S).

Definition clear_group (S : HotStore) (g : Group) : HotStore := set_conts g [] S.

Definition apply_operation (S : HotStore) (op : Op) : HotStore :=
  match op with
  | OpProduce c d => set_data c (hs_data S c ++ [d]) S
  | OpConsume g w => fold_left (add_join g) g (set_conts g (hs_conts S g ++ [w]) S)
  | OpComm g w ds => fold_left take_datum ds (take_waiter g w S)
  | OpSweep chans groups => fold_left clear_group groups (fold_left clear_channel chans S)
  end.

Definition play (S : HotStore) (ops : list Op) : HotStore := fold_left apply_operation ops S.

(* ── The checkpoint ── *)

Definition leaf_of (S : HotStore) (k : CKey) : list Bytes :=
  match k with
  | CData c => isort bytes_leb (map ser_datum (hs_data S c))
  | CCont g => isort bytes_leb (map ser_wk (hs_conts S g))
  | CJoins c => isort bytes_leb (map ser_join (hs_joins S c))
  end.

Definition action_of (S : HotStore) (k : CKey) : Action :=
  match leaf_of S k with
  | [] => ADelete (hkey_of k)
  | l => AInsert (hkey_of k) l
  end.

Definition changes (S : HotStore) : list Action := map (action_of S) (hs_cache S).

Definition checkpoint (T : Trie) (S : HotStore) : Trie := make_actions T (changes S).

(* ── Reading the trie ── *)

Fixpoint decode {A : Type} (de : Bytes -> option A) (bs : list Bytes) : list A :=
  match bs with
  | [] => []
  | b :: r => match de b with Some x => x :: decode de r | None => decode de r end
  end.

Definition read_leaf {A : Type} (de : Bytes -> option A) (T : Trie) (k : CKey) : list A :=
  match tlookup (hkey_of k) T with Some bs => decode de bs | None => [] end.

Definition reset_view (T : Trie) : HotStore :=
  mkHS (fun c => read_leaf de_datum T (CData c))
       (fun g => read_leaf de_wk T (CCont g))
       (fun c => read_leaf de_join T (CJoins c))
       [].

Definition reads_through (T : Trie) (S : HotStore) : Prop :=
  (forall c, ~ In (CData c) (hs_cache S) -> hs_data S c = read_leaf de_datum T (CData c)) /\
  (forall g, ~ In (CCont g) (hs_cache S) -> hs_conts S g = read_leaf de_wk T (CCont g)) /\
  (forall c, ~ In (CJoins c) (hs_cache S) -> hs_joins S c = read_leaf de_join T (CJoins c)).

(* ── The history ── *)

Definition History := list (Root * Trie).

Fixpoint lookup_root (r : Root) (h : History) : option Trie :=
  match h with
  | [] => None
  | (r', T) :: rest => if root_eq_dec r r' then Some T else lookup_root r rest
  end.

Definition commit (h : History) (T : Trie) (S : HotStore) : History * Root :=
  let T' := checkpoint T S in ((root_of T', T') :: h, root_of T').

Definition reset (h : History) (r : Root) : option HotStore :=
  match lookup_root r h with Some T => Some (reset_view T) | None => None end.

Definition root_complete : Prop :=
  forall T1 T2, root_of T1 = root_of T2 -> forall k, tlookup k T1 = tlookup k T2.

(* ── The soft checkpoint ── *)

Record Snapshot := mkSnap {
  sn_cache : list CKey;
  sn_data : Chan -> list Datum;
  sn_conts : Group -> list WK;
  sn_joins : Chan -> list Group
}.

Definition snapshot (S : HotStore) : Snapshot :=
  mkSnap (hs_cache S) (hs_data S) (hs_conts S) (hs_joins S).

(* create_from_hs_and_hr: the cached keys take the snapshot, the others read
   through to the base of the history. *)
Definition revert (T : Trie) (sn : Snapshot) : HotStore :=
  mkHS (fun c => if in_dec ckey_eq_dec (CData c) (sn_cache sn) then sn_data sn c
                 else read_leaf de_datum T (CData c))
       (fun g => if in_dec ckey_eq_dec (CCont g) (sn_cache sn) then sn_conts sn g
                 else read_leaf de_wk T (CCont g))
       (fun c => if in_dec ckey_eq_dec (CJoins c) (sn_cache sn) then sn_joins sn c
                 else read_leaf de_join T (CJoins c))
       (sn_cache sn).

(* ── The rig ── *)

Record Rig := mkRig { rig_log : list Op; rig_pos : nat }.

Definition replay_step (st : HotStore * Rig) (op : Op) : option (HotStore * Rig) :=
  let (hs, rg) := st in
  match nth_error (rig_log rg) (rig_pos rg) with
  | Some op' =>
      if op_eq_dec op op' then Some (apply_operation hs op, mkRig (rig_log rg) (S (rig_pos rg)))
      else None
  | None => None
  end.

Fixpoint replay_ops (st : HotStore * Rig) (ops : list Op) : option (HotStore * Rig) :=
  match ops with
  | [] => Some st
  | op :: rest => match replay_step st op with Some st' => replay_ops st' rest | None => None end
  end.

Definition complete (rg : Rig) : bool := Nat.eqb (rig_pos rg) (length (rig_log rg)).

Definition rig_and_reset (h : History) (r : Root) (log : list Op) : option (HotStore * Rig) :=
  match reset h r with Some hs => Some (hs, mkRig log 0) | None => None end.

(* ── Equivalence of hot stores ── *)

Definition store_equiv (S1 S2 : HotStore) : Prop :=
  (forall c, Permutation (hs_data S1 c) (hs_data S2 c)) /\
  (forall g, Permutation (hs_conts S1 g) (hs_conts S2 g)) /\
  (forall c, Permutation (hs_joins S1 c) (hs_joins S2 c)).

Lemma store_equiv_refl : forall S, store_equiv S S.
Proof. intros S. repeat split; intros; reflexivity. Qed.

(* ═══════════════════ Lemmas on the steps ═══════════════════ *)

Lemma touch_fields :
  forall k S, hs_data (touch k S) = hs_data S /\ hs_conts (touch k S) = hs_conts S /\
              hs_joins (touch k S) = hs_joins S.
Proof. intros k S. unfold touch. destruct (in_dec _ _ _); simpl; auto. Qed.

Lemma touch_cache_in : forall k k' S, In k' (hs_cache S) -> In k' (hs_cache (touch k S)).
Proof.
  intros k k' S Hin. unfold touch. destruct (in_dec _ _ _); simpl; [exact Hin |].
  apply in_or_app. left. exact Hin.
Qed.

Lemma touch_cache_self : forall k S, In k (hs_cache (touch k S)).
Proof.
  intros k S. unfold touch. destruct (in_dec _ _ _) as [Hin | Hn]; simpl; [exact Hin |].
  apply in_or_app. right. left. reflexivity.
Qed.

Lemma touch_cache_inv :
  forall k k' S, In k' (hs_cache (touch k S)) -> k' = k \/ In k' (hs_cache S).
Proof.
  intros k k' S. unfold touch. destruct (in_dec _ _ _); simpl; [intros; right; assumption |].
  intros Hin. apply in_app_or in Hin. destruct Hin as [Hin | [-> | []]]; [right; exact Hin | left; reflexivity].
Qed.

Lemma touch_cache_same :
  forall k S1 S2, hs_cache S1 = hs_cache S2 -> hs_cache (touch k S1) = hs_cache (touch k S2).
Proof.
  intros k S1 S2 Heq. unfold touch. rewrite Heq.
  destruct (in_dec _ _ _); simpl; [exact Heq | reflexivity].
Qed.

Lemma set_data_proj :
  forall c l S,
    (forall c', hs_data (set_data c l S) c' = if chan_eq_dec c c' then l else hs_data S c') /\
    hs_conts (set_data c l S) = hs_conts S /\ hs_joins (set_data c l S) = hs_joins S.
Proof.
  intros c l S. destruct (touch_fields (CData c) S) as [Ed [Ec Ej]].
  unfold set_data. simpl. rewrite Ed, Ec, Ej. split; [intros; reflexivity | split; reflexivity].
Qed.

Lemma set_conts_proj :
  forall g l S,
    hs_data (set_conts g l S) = hs_data S /\
    (forall g', hs_conts (set_conts g l S) g' = if group_eq_dec g g' then l else hs_conts S g') /\
    hs_joins (set_conts g l S) = hs_joins S.
Proof.
  intros g l S. destruct (touch_fields (CCont g) S) as [Ed [Ec Ej]].
  unfold set_conts. simpl. rewrite Ed, Ec, Ej.
  split; [reflexivity | split; [intros; reflexivity | reflexivity]].
Qed.

Lemma set_joins_proj :
  forall c l S,
    hs_data (set_joins c l S) = hs_data S /\ hs_conts (set_joins c l S) = hs_conts S /\
    (forall c', hs_joins (set_joins c l S) c' = if chan_eq_dec c c' then l else hs_joins S c').
Proof.
  intros c l S. destruct (touch_fields (CJoins c) S) as [Ed [Ec Ej]].
  unfold set_joins. simpl. rewrite Ed, Ec, Ej.
  split; [reflexivity | split; [reflexivity | intros; reflexivity]].
Qed.

(* Every step keeps the read-through of the uncached keys, because it caches
   a key before it writes the key. *)
Definition steps_keep (P : HotStore -> Prop) (f : HotStore -> HotStore) : Prop :=
  forall S, P S -> P (f S).

Lemma reads_through_touch : forall T k, steps_keep (reads_through T) (touch k).
Proof.
  intros T k S [Hd [Hc Hj]]. destruct (touch_fields k S) as [Ed [Ec Ej]].
  repeat split; intros x Hx; [rewrite Ed | rewrite Ec | rewrite Ej];
    [apply Hd | apply Hc | apply Hj]; intros Hin; apply Hx; apply touch_cache_in; exact Hin.
Qed.

Lemma reads_through_set_data :
  forall T c l, steps_keep (reads_through T) (set_data c l).
Proof.
  intros T c l S HR. pose proof (reads_through_touch T (CData c) S HR) as [Hd [Hc Hj]].
  unfold set_data. repeat split; simpl; [| exact Hc | exact Hj].
  intros c' Hx. destruct (chan_eq_dec c c') as [-> | Hne].
  - exfalso. apply Hx. apply touch_cache_self.
  - apply Hd. exact Hx.
Qed.

Lemma reads_through_set_conts :
  forall T g l, steps_keep (reads_through T) (set_conts g l).
Proof.
  intros T g l S HR. pose proof (reads_through_touch T (CCont g) S HR) as [Hd [Hc Hj]].
  unfold set_conts. repeat split; simpl; [exact Hd | | exact Hj].
  intros g' Hx. destruct (group_eq_dec g g') as [-> | Hne].
  - exfalso. apply Hx. apply touch_cache_self.
  - apply Hc. exact Hx.
Qed.

Lemma reads_through_set_joins :
  forall T c l, steps_keep (reads_through T) (set_joins c l).
Proof.
  intros T c l S HR. pose proof (reads_through_touch T (CJoins c) S HR) as [Hd [Hc Hj]].
  unfold set_joins. repeat split; simpl; [exact Hd | exact Hc |].
  intros c' Hx. destruct (chan_eq_dec c c') as [-> | Hne].
  - exfalso. apply Hx. apply touch_cache_self.
  - apply Hj. exact Hx.
Qed.

Lemma fold_keeps :
  forall (A : Type) (P : HotStore -> Prop) (f : HotStore -> A -> HotStore),
    (forall a, steps_keep P (fun S => f S a)) ->
    forall l S, P S -> P (fold_left f l S).
Proof.
  intros A P f Hf l. induction l as [| a r IH]; intros S HS; simpl; [exact HS |].
  apply IH. apply Hf. exact HS.
Qed.

Lemma reads_through_apply :
  forall T op, steps_keep (reads_through T) (fun S => apply_operation S op).
Proof.
  intros T [c d | g w | g w ds | chans groups] S HR; cbv beta; unfold apply_operation.
  - apply reads_through_set_data. exact HR.
  - apply fold_keeps; [intros c S' H; cbv beta; unfold add_join; apply reads_through_set_joins; exact H |].
    apply reads_through_set_conts. exact HR.
  - apply fold_keeps.
    + intros [c d] S' H. cbv beta. unfold take_datum.
      destruct (d_persist d); first [exact H | apply reads_through_set_data; exact H].
    + unfold take_waiter. destruct (w_persist w); [exact HR |]. cbv zeta.
      match goal with
      | |- reads_through T (if nullb (hs_conts ?U g) then _ else _) =>
          assert (H1 : reads_through T U) by (apply reads_through_set_conts; exact HR);
          destruct (nullb (hs_conts U g));
          [ apply fold_keeps; [intros c S' H; cbv beta; unfold drop_join;
                                apply reads_through_set_joins; exact H | exact H1]
          | exact H1 ]
      end.
  - apply fold_keeps; [intros g S' H; cbv beta; unfold clear_group; apply reads_through_set_conts; exact H |].
    apply fold_keeps; [| exact HR].
    intros c S' H. cbv beta. unfold clear_channel.
    first [ apply reads_through_set_joins; apply reads_through_set_data; exact H
          | apply reads_through_set_data; exact H
          | apply reads_through_set_joins; exact H ].
Qed.

Lemma reads_through_play :
  forall T ops S, reads_through T S -> reads_through T (play S ops).
Proof.
  intros T ops S HR. unfold play. apply fold_keeps; [| exact HR].
  intros op. apply reads_through_apply.
Qed.

Lemma reset_view_reads_through : forall T, reads_through T (reset_view T).
Proof. intros T. repeat split; intros; reflexivity. Qed.

(* The cache never holds a key twice. *)
Lemma nodup_touch : forall k, steps_keep (fun S => NoDup (hs_cache S)) (touch k).
Proof.
  intros k S Hnd. unfold touch. destruct (in_dec _ _ _) as [Hin | Hn]; simpl; [exact Hnd |].
  apply NoDup_app; [exact Hnd | constructor; [intros [] | constructor] |].
  intros x Hx [-> | []]. apply Hn. exact Hx.
Qed.

Lemma nodup_apply :
  forall op, steps_keep (fun S => NoDup (hs_cache S)) (fun S => apply_operation S op).
Proof.
  assert (Hd : forall c l, steps_keep (fun S => NoDup (hs_cache S)) (set_data c l))
    by (intros c l S H; unfold set_data; simpl; apply nodup_touch; exact H).
  assert (Hc : forall g l, steps_keep (fun S => NoDup (hs_cache S)) (set_conts g l))
    by (intros g l S H; unfold set_conts; simpl; apply nodup_touch; exact H).
  assert (Hj : forall c l, steps_keep (fun S => NoDup (hs_cache S)) (set_joins c l))
    by (intros c l S H; unfold set_joins; simpl; apply nodup_touch; exact H).
  intros [c d | g w | g w ds | chans groups] S HS; cbv beta; unfold apply_operation.
  - exact (Hd c _ S HS).
  - apply fold_keeps; [intros c S' H; exact (Hj c _ S' H) | exact (Hc g _ S HS)].
  - apply fold_keeps.
    + intros [c d] S' H. cbv beta. unfold take_datum.
      destruct (d_persist d); first [exact H | exact (Hd c _ S' H)].
    + unfold take_waiter. destruct (w_persist w); [exact HS |]. cbv zeta.
      match goal with
      | |- NoDup (hs_cache (if nullb (hs_conts ?U g) then _ else _)) =>
          assert (H1 : NoDup (hs_cache U)) by (unfold set_conts; simpl; apply nodup_touch; exact HS);
          destruct (nullb (hs_conts U g));
          [ apply fold_keeps; [intros c S' H; exact (Hj c _ S' H) | exact H1] | exact H1 ]
      end.
  - apply fold_keeps; [intros g S' H; exact (Hc g _ S' H) |].
    apply fold_keeps; [| exact HS].
    intros c S' H. cbv beta. unfold clear_channel.
    first [ exact (Hj c _ _ (Hd c _ S' H)) | exact (Hd c _ S' H) | exact (Hj c _ S' H) ].
Qed.

Lemma nodup_play :
  forall ops S, NoDup (hs_cache S) -> NoDup (hs_cache (play S ops)).
Proof.
  intros ops S H. unfold play. apply fold_keeps; [| exact H]. intros op. apply nodup_apply.
Qed.

(* ═══════════════════ Reading back a checkpoint ═══════════════════ *)

Lemma decode_map :
  forall (A : Type) (de : Bytes -> option A) (ser : A -> Bytes),
    (forall x, de (ser x) = Some x) -> forall l, decode de (map ser l) = l.
Proof.
  intros A de ser Hrt l. induction l as [| x r IH]; simpl; [reflexivity |].
  rewrite Hrt. f_equal. exact IH.
Qed.

Lemma decode_perm :
  forall (A : Type) (de : Bytes -> option A) bs1 bs2,
    Permutation bs1 bs2 -> Permutation (decode de bs1) (decode de bs2).
Proof.
  intros A de bs1 bs2 Hp. induction Hp; simpl.
  - reflexivity.
  - destruct (de x); [apply perm_skip |]; exact IHHp.
  - destruct (de x), (de y); try apply perm_swap; try reflexivity; apply perm_skip; reflexivity.
  - eapply perm_trans; eassumption.
Qed.

Lemma decode_sorted :
  forall (A : Type) (de : Bytes -> option A) (ser : A -> Bytes),
    (forall x, de (ser x) = Some x) ->
    forall l, Permutation (decode de (isort bytes_leb (map ser l))) l.
Proof.
  intros A de ser Hrt l.
  eapply perm_trans; [apply decode_perm; apply isort_perm |].
  rewrite (decode_map A de ser Hrt). reflexivity.
Qed.

Lemma isort_nil_iff : forall (A : Type) leb (l : list A), isort leb l = [] <-> l = [].
Proof.
  intros A leb l. split; intros H; [| subst; reflexivity].
  pose proof (isort_perm A leb l) as Hp. rewrite H in Hp.
  apply Permutation_nil. exact Hp.
Qed.

Lemma nodup_map_injective :
  forall (A B : Type) (f : A -> B), (forall x y, f x = f y -> x = y) ->
    forall l, NoDup l -> NoDup (map f l).
Proof.
  intros A B f Hinj l Hnd. induction Hnd as [| x l Hx Hnd IH]; simpl; constructor; [| exact IH].
  intros Hin. apply in_map_iff in Hin. destruct Hin as [y [Hy Hin]].
  apply Hinj in Hy. subst y. contradiction.
Qed.

Lemma lookup_checkpoint_cached :
  forall T S k, key_injective -> NoDup (hs_cache S) -> In k (hs_cache S) ->
    tlookup (hkey_of k) (checkpoint T S) =
      match leaf_of S k with [] => None | l => Some l end.
Proof.
  intros T S k Hinj Hnd Hin. unfold checkpoint, changes.
  assert (Hkeys : NoDup (map action_key (map (action_of S) (hs_cache S)))).
  { rewrite map_map.
    assert (Heq : forall x, action_key (action_of S x) = hkey_of x)
      by (intros x; unfold action_of; destruct (leaf_of S x); reflexivity).
    rewrite (map_ext _ _ Heq). apply (nodup_map_injective _ _ hkey_of Hinj). exact Hnd. }
  assert (Hk : action_key (action_of S k) = hkey_of k)
    by (unfold action_of; destruct (leaf_of S k); reflexivity).
  rewrite <- Hk. rewrite (make_actions_in _ T (action_of S k) Hkeys) by (apply in_map; exact Hin).
  unfold action_of. destruct (leaf_of S k); reflexivity.
Qed.

Lemma lookup_checkpoint_uncached :
  forall T S k, key_injective -> ~ In k (hs_cache S) ->
    tlookup (hkey_of k) (checkpoint T S) = tlookup (hkey_of k) T.
Proof.
  intros T S k Hinj Hn. unfold checkpoint, changes. apply make_actions_other.
  intros Hin. apply in_map_iff in Hin. destruct Hin as [a [Ha Hin]].
  apply in_map_iff in Hin. destruct Hin as [k' [<- Hin']].
  assert (Hk : action_key (action_of S k') = hkey_of k')
    by (unfold action_of; destruct (leaf_of S k'); reflexivity).
  rewrite Hk in Ha. apply Hinj in Ha. subst k'. apply Hn. exact Hin'.
Qed.

Lemma read_checkpoint_cached :
  forall (A : Type) (de : Bytes -> option A) (ser : A -> Bytes) (l : list A) T S k,
    (forall x, de (ser x) = Some x) -> key_injective -> NoDup (hs_cache S) -> In k (hs_cache S) ->
    leaf_of S k = isort bytes_leb (map ser l) ->
    Permutation (read_leaf de (checkpoint T S) k) l.
Proof.
  intros A de ser l T S k Hrt Hinj Hnd Hin Hleaf. unfold read_leaf.
  rewrite (lookup_checkpoint_cached T S k Hinj Hnd Hin). rewrite Hleaf.
  destruct (isort bytes_leb (map ser l)) eqn:E.
  - apply isort_nil_iff in E. destruct l; [reflexivity | discriminate].
  - rewrite <- E. apply decode_sorted. exact Hrt.
Qed.

Lemma read_checkpoint_uncached :
  forall (A : Type) (de : Bytes -> option A) T S k,
    key_injective -> ~ In k (hs_cache S) ->
    read_leaf de (checkpoint T S) k = read_leaf de T k.
Proof.
  intros A de T S k Hinj Hn. unfold read_leaf.
  rewrite (lookup_checkpoint_uncached T S k Hinj Hn). reflexivity.
Qed.

(* ═══════════════════ C1: a reset gives back the checkpointed store ═══════════════════ *)

Theorem checkpoint_reset_restores_store :
  roundtrips -> key_injective ->
  forall h T S, reads_through T S -> NoDup (hs_cache S) ->
    let (h', r) := commit h T S in
    exists S', reset h' r = Some S' /\ store_equiv S' S /\ hs_cache S' = [] /\
               reads_through (checkpoint T S) S'.
Proof.
  intros [Rd [Rw Rj]] Hinj h T S [Hd [Hc Hj]] Hnd. unfold commit, reset. simpl.
  destruct (root_eq_dec (root_of (checkpoint T S)) (root_of (checkpoint T S))) as [_ | Hne];
    [| congruence].
  exists (reset_view (checkpoint T S)). split; [reflexivity |].
  split; [| split; [reflexivity | apply reset_view_reads_through]].
  repeat split; simpl.
  - intros c. destruct (in_dec ckey_eq_dec (CData c) (hs_cache S)) as [Hin | Hn].
    + apply (read_checkpoint_cached Datum de_datum ser_datum); auto.
    + rewrite (read_checkpoint_uncached Datum de_datum T S (CData c) Hinj Hn).
      rewrite <- (Hd c Hn). reflexivity.
  - intros g. destruct (in_dec ckey_eq_dec (CCont g) (hs_cache S)) as [Hin | Hn].
    + apply (read_checkpoint_cached WK de_wk ser_wk); auto.
    + rewrite (read_checkpoint_uncached WK de_wk T S (CCont g) Hinj Hn).
      rewrite <- (Hc g Hn). reflexivity.
  - intros c. destruct (in_dec ckey_eq_dec (CJoins c) (hs_cache S)) as [Hin | Hn].
    + apply (read_checkpoint_cached Group de_join ser_join); auto.
    + rewrite (read_checkpoint_uncached Group de_join T S (CJoins c) Hinj Hn).
      rewrite <- (Hj c Hn). reflexivity.
Qed.

(* ═══════════════════ C2: the persistent flag survives ═══════════════════ *)

Lemma take_datum_keeps_persistent :
  forall S cd c d, d_persist d = true -> In d (hs_data S c) -> In d (hs_data (take_datum S cd) c).
Proof.
  intros S [c' d'] c d Hp Hin. unfold take_datum.
  destruct (d_persist d') eqn:Ep; [exact Hin |].
  unfold set_data. simpl. destruct (touch_fields (CData c') S) as [Ed _]. rewrite Ed.
  destruct (chan_eq_dec c' c) as [-> | Hne]; [| exact Hin].
  apply remove_first_keeps_other; [| exact Hin].
  intros ->. congruence.
Qed.

Lemma set_conts_data : forall g l S, hs_data (set_conts g l S) = hs_data S.
Proof. intros g l S. unfold set_conts. simpl. apply touch_fields. Qed.

Lemma set_joins_data : forall c l S, hs_data (set_joins c l S) = hs_data S.
Proof. intros c l S. unfold set_joins. simpl. apply touch_fields. Qed.

Lemma fold_drop_join_data :
  forall g cs S, hs_data (fold_left (drop_join g) cs S) = hs_data S.
Proof.
  intros g cs. induction cs as [| c r IH]; intros S; simpl; [reflexivity |].
  rewrite IH. unfold drop_join. apply set_joins_data.
Qed.

Lemma take_waiter_data : forall g w S, hs_data (take_waiter g w S) = hs_data S.
Proof.
  intros g w S. unfold take_waiter. destruct (w_persist w); [reflexivity |]. cbv zeta.
  destruct (nullb _).
  - rewrite fold_drop_join_data. apply set_conts_data.
  - apply set_conts_data.
Qed.

Lemma comm_keeps_persistent :
  forall S g w ds c d, d_persist d = true -> In d (hs_data S c) ->
    In d (hs_data (apply_operation S (OpComm g w ds)) c).
Proof.
  intros S g w ds c d Hp Hin. simpl.
  assert (H0 : In d (hs_data (take_waiter g w S) c)) by (rewrite take_waiter_data; exact Hin).
  generalize dependent (take_waiter g w S). induction ds as [| cd r IH]; intros S0 H0; simpl;
    [exact H0 |].
  apply IH. apply take_datum_keeps_persistent; assumption.
Qed.

Theorem checkpoint_restore_preserves_persist :
  roundtrips -> key_injective ->
  forall h T S c d, reads_through T S -> NoDup (hs_cache S) ->
    In d (hs_data S c) -> d_persist d = true ->
    let (h', r) := commit h T S in
    exists S', reset h' r = Some S' /\ In d (hs_data S' c) /\
               forall g w ds, In d (hs_data (apply_operation S' (OpComm g w ds)) c).
Proof.
  intros Hrt Hinj h T S c d HR Hnd Hin Hp.
  pose proof (checkpoint_reset_restores_store Hrt Hinj h T S HR Hnd) as HC.
  destruct (commit h T S) as [h' r].
  destruct HC as [S' [Hres [[Hd _] _]]].
  exists S'. split; [exact Hres |].
  assert (Hin' : In d (hs_data S' c)) by exact (Permutation_in d (Permutation_sym (Hd c)) Hin).
  split; [exact Hin' |].
  intros g w ds. apply comm_keeps_persistent; assumption.
Qed.

(* ═══════════════════ C3 and C4: equivalence and order independence ═══════════════════ *)

Definition equiv_same_cache (S1 S2 : HotStore) : Prop :=
  store_equiv S1 S2 /\ hs_cache S1 = hs_cache S2.

Lemma equiv_touch :
  forall k S1 S2, equiv_same_cache S1 S2 -> equiv_same_cache (touch k S1) (touch k S2).
Proof.
  intros k S1 S2 [[Hd [Hc Hj]] Hca].
  destruct (touch_fields k S1) as [E1d [E1c E1j]]. destruct (touch_fields k S2) as [E2d [E2c E2j]].
  split; [| apply touch_cache_same; exact Hca].
  repeat split; intros x; [rewrite E1d, E2d | rewrite E1c, E2c | rewrite E1j, E2j];
    [apply Hd | apply Hc | apply Hj].
Qed.

Lemma equiv_set_data :
  forall c l1 l2 S1 S2, Permutation l1 l2 -> equiv_same_cache S1 S2 ->
    equiv_same_cache (set_data c l1 S1) (set_data c l2 S2).
Proof.
  intros c l1 l2 S1 S2 Hp HE. destruct (equiv_touch (CData c) S1 S2 HE) as [[Hd [Hc Hj]] Hca].
  unfold set_data. split; simpl; [| exact Hca].
  repeat split; simpl; [| exact Hc | exact Hj].
  intros c'. destruct (chan_eq_dec c c'); [exact Hp | apply Hd].
Qed.

Lemma equiv_set_conts :
  forall g l1 l2 S1 S2, Permutation l1 l2 -> equiv_same_cache S1 S2 ->
    equiv_same_cache (set_conts g l1 S1) (set_conts g l2 S2).
Proof.
  intros g l1 l2 S1 S2 Hp HE. destruct (equiv_touch (CCont g) S1 S2 HE) as [[Hd [Hc Hj]] Hca].
  unfold set_conts. split; simpl; [| exact Hca].
  repeat split; simpl; [exact Hd | | exact Hj].
  intros g'. destruct (group_eq_dec g g'); [exact Hp | apply Hc].
Qed.

Lemma equiv_set_joins :
  forall c l1 l2 S1 S2, Permutation l1 l2 -> equiv_same_cache S1 S2 ->
    equiv_same_cache (set_joins c l1 S1) (set_joins c l2 S2).
Proof.
  intros c l1 l2 S1 S2 Hp HE. destruct (equiv_touch (CJoins c) S1 S2 HE) as [[Hd [Hc Hj]] Hca].
  unfold set_joins. split; simpl; [| exact Hca].
  repeat split; simpl; [exact Hd | exact Hc |].
  intros c'. destruct (chan_eq_dec c c'); [exact Hp | apply Hj].
Qed.

Lemma equiv_fold :
  forall (A : Type) (f : HotStore -> A -> HotStore),
    (forall a S1 S2, equiv_same_cache S1 S2 -> equiv_same_cache (f S1 a) (f S2 a)) ->
    forall l S1 S2, equiv_same_cache S1 S2 -> equiv_same_cache (fold_left f l S1) (fold_left f l S2).
Proof.
  intros A f Hf l. induction l as [| a r IH]; intros S1 S2 HE; simpl; [exact HE |].
  apply IH. apply Hf. exact HE.
Qed.

Lemma nullb_perm : forall (A : Type) (l1 l2 : list A), Permutation l1 l2 -> nullb l1 = nullb l2.
Proof.
  intros A l1 l2 Hp. destruct l1 as [| x r], l2 as [| y s]; try reflexivity.
  - apply Permutation_nil in Hp. discriminate.
  - apply Permutation_sym, Permutation_nil in Hp. discriminate.
Qed.

Lemma equiv_apply :
  forall op S1 S2, equiv_same_cache S1 S2 ->
    equiv_same_cache (apply_operation S1 op) (apply_operation S2 op).
Proof.
  intros [c d | g w | g w ds | chans groups] S1 S2 HE; unfold apply_operation.
  - apply equiv_set_data; [apply Permutation_app_tail; apply (proj1 (proj1 HE)) | exact HE].
  - apply equiv_fold.
    + intros c T1 T2 HT. unfold add_join. apply equiv_set_joins; [| exact HT].
      apply add_absent_perm. apply (proj2 (proj2 (proj1 HT))).
    + apply equiv_set_conts; [| exact HE].
      apply Permutation_app_tail. apply (proj1 (proj2 (proj1 HE))).
  - apply equiv_fold.
    + intros [c d] T1 T2 HT. unfold take_datum.
      destruct (d_persist d);
        first [ exact HT
              | apply equiv_set_data; [apply remove_first_perm; apply (proj1 (proj1 HT)) | exact HT] ].
    + unfold take_waiter. destruct (w_persist w); [exact HE |].
      set (U1 := set_conts g (remove_first wk_eq_dec w (hs_conts S1 g)) S1).
      set (U2 := set_conts g (remove_first wk_eq_dec w (hs_conts S2 g)) S2).
      assert (H1 : equiv_same_cache U1 U2).
      { apply equiv_set_conts; [| exact HE]. apply remove_first_perm.
        apply (proj1 (proj2 (proj1 HE))). }
      rewrite (nullb_perm _ _ _ (proj1 (proj2 (proj1 H1)) g)).
      destruct (nullb (hs_conts U2 g)); [| exact H1].
      apply equiv_fold; [| exact H1].
      intros c T1 T2 HT. unfold drop_join. apply equiv_set_joins; [| exact HT].
      apply drop_all_perm. apply (proj2 (proj2 (proj1 HT))).
  - apply equiv_fold.
    + intros g T1 T2 HT. unfold clear_group. apply equiv_set_conts; [reflexivity | exact HT].
    + apply equiv_fold; [| exact HE].
      intros c T1 T2 HT. unfold clear_channel.
      first [ apply equiv_set_joins; [reflexivity |]; apply equiv_set_data; [reflexivity | exact HT]
            | apply equiv_set_data; [reflexivity | exact HT] ].
Qed.

Lemma equiv_play :
  forall ops S1 S2, equiv_same_cache S1 S2 -> equiv_same_cache (play S1 ops) (play S2 ops).
Proof. intros ops S1 S2 HE. unfold play. apply equiv_fold; [| exact HE]. intros op. apply equiv_apply. Qed.

Theorem checkpoint_root_order_independent :
  bytes_order -> forall T S1 S2, equiv_same_cache S1 S2 -> checkpoint T S1 = checkpoint T S2.
Proof.
  intros Hord T S1 S2 [[Hd [Hc Hj]] Hca]. unfold checkpoint, changes. rewrite Hca. f_equal.
  apply map_ext. intros k. unfold action_of.
  assert (Hleaf : leaf_of S1 k = leaf_of S2 k).
  { destruct k as [c | g | c]; simpl; apply isort_canonical; try exact Hord;
      apply Permutation_map; [apply Hd | apply Hc | apply Hj]. }
  rewrite Hleaf. reflexivity.
Qed.

Theorem replay_from_equal_parent_states :
  bytes_order ->
  forall T ops S1 S2, equiv_same_cache S1 S2 ->
    equiv_same_cache (play S1 ops) (play S2 ops) /\
    checkpoint T (play S1 ops) = checkpoint T (play S2 ops) /\
    root_of (checkpoint T (play S1 ops)) = root_of (checkpoint T (play S2 ops)) /\
    play S1 ops = play S1 ops.
Proof.
  intros Hord T ops S1 S2 HE.
  pose proof (equiv_play ops S1 S2 HE) as HP.
  pose proof (checkpoint_root_order_independent Hord T _ _ HP) as HK.
  split; [exact HP |]. split; [exact HK |]. split; [rewrite HK; reflexivity | reflexivity].
Qed.

(* ═══════════════════ C5: the rig accepts exactly the log of play ═══════════════════ *)

Lemma replay_ops_spec :
  forall ops S log p S' rg',
    replay_ops (S, mkRig log p) ops = Some (S', rg') ->
    rig_log rg' = log /\ rig_pos rg' = p + length ops /\ S' = play S ops /\
    (forall i, i < length ops -> nth_error ops i = nth_error log (p + i)).
Proof.
  induction ops as [| op r IH]; intros S log p S' rg' H; simpl in H.
  - inversion H; subst. simpl. split; [reflexivity |]. split; [lia |]. split; [reflexivity |].
    intros i Hi. simpl in Hi. lia.
  - simpl in H. destruct (nth_error log p) as [op' |] eqn:Ep; [| discriminate].
    destruct (op_eq_dec op op') as [-> | Hne]; [| discriminate].
    destruct (IH _ _ _ _ _ H) as [Hlog [Hpos [HS Hnth]]].
    split; [exact Hlog |]. split; [simpl; lia |]. split; [exact HS |].
    intros [| i] Hi; simpl.
    + rewrite Nat.add_0_r. symmetry. exact Ep.
    + rewrite Hnth by (simpl in Hi; lia). f_equal. lia.
Qed.

Lemma replay_ops_accepts :
  forall log pre st0,
    replay_ops (st0, mkRig (pre ++ log) (length pre)) log =
      Some (play st0 log, mkRig (pre ++ log) (length pre + length log)).
Proof.
  intros log. induction log as [| op r IH]; intros pre st0; simpl.
  - rewrite Nat.add_0_r. reflexivity.
  - rewrite nth_error_app2 by lia. rewrite Nat.sub_diag. simpl.
    destruct (op_eq_dec op op) as [_ | Hne]; [| congruence].
    specialize (IH (pre ++ [op]) (apply_operation st0 op)).
    rewrite <- app_assoc in IH. simpl in IH. rewrite length_app in IH. simpl in IH.
    replace (S (length pre)) with (length pre + 1) by lia. rewrite IH.
    replace (length pre + 1 + length r) with (length pre + S (length r)) by lia.
    reflexivity.
Qed.

Lemma nth_error_ext_len :
  forall (A : Type) (l1 l2 : list A),
    length l1 = length l2 -> (forall i, i < length l1 -> nth_error l1 i = nth_error l2 i) -> l1 = l2.
Proof.
  intros A l1. induction l1 as [| x r IH]; intros [| y s] Hlen Hn; simpl in *; try discriminate;
    [reflexivity |].
  pose proof (Hn 0 ltac:(lia)) as H0. simpl in H0. inversion H0; subst.
  f_equal. apply IH; [lia |]. intros i Hi. exact (Hn (S i) ltac:(lia)).
Qed.

Theorem rig_and_reset_replays_the_log :
  forall h r T log, lookup_root r h = Some T ->
    rig_and_reset h r log = Some (reset_view T, mkRig log 0) /\
    replay_ops (reset_view T, mkRig log 0) log =
      Some (play (reset_view T) log, mkRig log (length log)) /\
    complete (mkRig log (length log)) = true /\
    (forall ops S' rg', replay_ops (reset_view T, mkRig log 0) ops = Some (S', rg') ->
       complete rg' = true -> ops = log /\ S' = play (reset_view T) log).
Proof.
  intros h r T log Hl. split; [unfold rig_and_reset, reset; rewrite Hl; reflexivity |].
  split; [exact (replay_ops_accepts log [] (reset_view T)) |].
  split; [unfold complete; simpl; apply Nat.eqb_refl |].
  intros ops S' rg' Hrep Hc.
  destruct (replay_ops_spec _ _ _ _ _ _ Hrep) as [Hlog [Hpos [HS Hnth]]].
  unfold complete in Hc. apply Nat.eqb_eq in Hc. rewrite Hlog, Hpos in Hc. simpl in Hc.
  assert (Hops : ops = log).
  { apply nth_error_ext_len; [exact Hc |]. intros i Hi. rewrite Hnth by exact Hi. reflexivity. }
  subst ops. split; [reflexivity | exact HS].
Qed.

(* ═══════════════════ C6: the sweep leaves no volatile record ═══════════════════ *)

Definition channel_clear (S : HotStore) (c : Chan) : Prop := hs_data S c = [] /\ hs_joins S c = [].

Lemma clear_channel_self : forall S c, channel_clear (clear_channel S c) c.
Proof.
  intros S c. unfold channel_clear, clear_channel.
  destruct (set_joins_proj c [] (set_data c [] S)) as [Jd [_ Jj]].
  destruct (set_data_proj c [] S) as [Dd _].
  rewrite Jd, Jj, Dd. destruct (chan_eq_dec c c); [split; reflexivity | congruence].
Qed.

Lemma clear_channel_keeps : forall S x c, channel_clear S c -> channel_clear (clear_channel S x) c.
Proof.
  intros S x c [Hd Hj]. unfold channel_clear, clear_channel.
  destruct (set_joins_proj x [] (set_data x [] S)) as [Jd [_ Jj]].
  destruct (set_data_proj x [] S) as [Dd [_ Dj]].
  rewrite Jd, Jj, Dd, Dj. destruct (chan_eq_dec x c); split; first [reflexivity | assumption].
Qed.

Lemma clear_channels_spec :
  forall chans S c, In c chans -> channel_clear (fold_left clear_channel chans S) c.
Proof.
  intros chans. induction chans as [| x r IH]; intros S c Hin; [destruct Hin |]. simpl.
  destruct Hin as [-> | Hin]; [| apply IH; exact Hin].
  assert (Hk : forall l S', channel_clear S' c -> channel_clear (fold_left clear_channel l S') c).
  { intros l. induction l as [| y s IHl]; intros S' H; simpl; [exact H |].
    apply IHl. apply clear_channel_keeps. exact H. }
  apply Hk. apply clear_channel_self.
Qed.

Lemma clear_group_self : forall S g, hs_conts (clear_group S g) g = [].
Proof.
  intros S g. unfold clear_group. destruct (set_conts_proj g [] S) as [_ [Cc _]].
  rewrite Cc. destruct (group_eq_dec g g); [reflexivity | congruence].
Qed.

Lemma clear_group_keeps : forall S x g, hs_conts S g = [] -> hs_conts (clear_group S x) g = [].
Proof.
  intros S x g H. unfold clear_group. destruct (set_conts_proj x [] S) as [_ [Cc _]].
  rewrite Cc. destruct (group_eq_dec x g); [reflexivity | exact H].
Qed.

Lemma clear_groups_spec :
  forall groups S g, In g groups -> hs_conts (fold_left clear_group groups S) g = [].
Proof.
  intros groups. induction groups as [| x r IH]; intros S g Hin; [destruct Hin |]. simpl.
  destruct Hin as [-> | Hin]; [| apply IH; exact Hin].
  assert (Hk : forall l S', hs_conts S' g = [] -> hs_conts (fold_left clear_group l S') g = []).
  { intros l. induction l as [| y s IHl]; intros S' H; simpl; [exact H |].
    apply IHl. apply clear_group_keeps. exact H. }
  apply Hk. apply clear_group_self.
Qed.

Lemma clear_groups_keeps :
  forall groups S, hs_data (fold_left clear_group groups S) = hs_data S /\
                   hs_joins (fold_left clear_group groups S) = hs_joins S.
Proof.
  intros groups. induction groups as [| x r IH]; intros S; simpl; [split; reflexivity |].
  destruct (IH (clear_group S x)) as [Hd Hj]. rewrite Hd, Hj.
  unfold clear_group. destruct (set_conts_proj x [] S) as [Cd [_ Cj]]. split; assumption.
Qed.

Theorem reset_after_sweep_drops_volatile :
  roundtrips -> key_injective ->
  forall h T S chans groups, reads_through T S -> NoDup (hs_cache S) ->
    let S1 := apply_operation S (OpSweep chans groups) in
    let (h', r) := commit h T S1 in
    exists S', reset h' r = Some S' /\
      (forall c, In c chans -> hs_data S' c = [] /\ hs_joins S' c = []) /\
      (forall g, In g groups -> hs_conts S' g = []).
Proof.
  intros Hrt Hinj h T S chans groups HR Hnd S1.
  assert (HR1 : reads_through T S1) by (apply reads_through_apply; exact HR).
  assert (Hnd1 : NoDup (hs_cache S1)) by (apply nodup_apply; exact Hnd).
  pose proof (checkpoint_reset_restores_store Hrt Hinj h T S1 HR1 Hnd1) as HC.
  destruct (commit h T S1) as [h' r]. destruct HC as [S' [Hres [[Hd [Hc Hj]] _]]].
  exists S'. split; [exact Hres |]. split.
  - intros c Hin. specialize (Hd c). specialize (Hj c). unfold S1 in Hd, Hj. simpl in Hd, Hj.
    destruct (clear_groups_keeps groups (fold_left clear_channel chans S)) as [Kd Kj].
    destruct (clear_channels_spec chans S c Hin) as [Cd Cj].
    rewrite Kd, Cd in Hd. rewrite Kj, Cj in Hj.
    split; apply Permutation_nil, Permutation_sym; assumption.
  - intros g Hin. specialize (Hc g). unfold S1 in Hc. simpl in Hc.
    rewrite (clear_groups_spec groups (fold_left clear_channel chans S) g Hin) in Hc.
    apply Permutation_nil, Permutation_sym. exact Hc.
Qed.

(* ═══════════════════ C7: the revert of a soft checkpoint ═══════════════════ *)

Theorem soft_revert_after_prefix_failure :
  forall T S, reads_through T S ->
    (forall ops i, reads_through T (play S (firstn i ops))) /\
    let S' := revert T (snapshot S) in
    hs_cache S' = hs_cache S /\
    (forall c, hs_data S' c = hs_data S c) /\
    (forall g, hs_conts S' g = hs_conts S g) /\
    (forall c, hs_joins S' c = hs_joins S c) /\
    checkpoint T S' = checkpoint T S.
Proof.
  intros T S HR. split; [intros ops i; apply reads_through_play; exact HR |].
  destruct HR as [Hd [Hc Hj]]. intros S'.
  assert (ED : forall c, hs_data S' c = hs_data S c).
  { intros c. unfold S', revert, snapshot. simpl.
    destruct (in_dec ckey_eq_dec (CData c) (hs_cache S)) as [_ | Hn]; [reflexivity |].
    symmetry. apply Hd. exact Hn. }
  assert (EC : forall g, hs_conts S' g = hs_conts S g).
  { intros g. unfold S', revert, snapshot. simpl.
    destruct (in_dec ckey_eq_dec (CCont g) (hs_cache S)) as [_ | Hn]; [reflexivity |].
    symmetry. apply Hc. exact Hn. }
  assert (EJ : forall c, hs_joins S' c = hs_joins S c).
  { intros c. unfold S', revert, snapshot. simpl.
    destruct (in_dec ckey_eq_dec (CJoins c) (hs_cache S)) as [_ | Hn]; [reflexivity |].
    symmetry. apply Hj. exact Hn. }
  assert (ECa : hs_cache S' = hs_cache S) by reflexivity.
  clearbody S'.
  split; [exact ECa |]. split; [exact ED |]. split; [exact EC |]. split; [exact EJ |].
  unfold checkpoint, changes. rewrite ECa.
  f_equal. apply map_ext. intros k. unfold action_of.
  assert (Hleaf : leaf_of S' k = leaf_of S k)
    by (destruct k as [c | g | c]; simpl; [rewrite ED | rewrite EC | rewrite EJ]; reflexivity).
  rewrite Hleaf. reflexivity.
Qed.

(* ═══════════════════ C8: every root of the history resets ═══════════════════ *)

Lemma lookup_root_some :
  forall r h T, lookup_root r h = Some T -> In (r, T) h.
Proof.
  intros r h. induction h as [| [r' T'] rest IH]; intros T H; simpl in H; [discriminate |].
  destruct (root_eq_dec r r') as [-> | Hne]; [inversion H; subst; left; reflexivity |].
  right. apply IH. exact H.
Qed.

Lemma lookup_root_in :
  forall r h T, In (r, T) h -> exists T', lookup_root r h = Some T'.
Proof.
  intros r h. induction h as [| [r' T'] rest IH]; intros T Hin; [destruct Hin |].
  simpl. destruct (root_eq_dec r r') as [-> | Hne]; [eexists; reflexivity |].
  destruct Hin as [Heq | Hin]; [inversion Heq; congruence |]. exact (IH T Hin).
Qed.

Definition well_rooted (h : History) : Prop := forall r T, In (r, T) h -> r = root_of T.

Lemma commit_well_rooted :
  forall h T S, well_rooted h -> well_rooted (fst (commit h T S)).
Proof.
  intros h T S Hw r T' Hin. simpl in Hin. destruct Hin as [Heq | Hin].
  - inversion Heq; reflexivity.
  - apply Hw. exact Hin.
Qed.

Theorem chain_checkpoints_retrievable :
  root_complete ->
  forall h r T, well_rooted h -> In (r, T) h ->
    exists S', reset h r = Some S' /\
      (forall c, hs_data S' c = read_leaf de_datum T (CData c)) /\
      (forall g, hs_conts S' g = read_leaf de_wk T (CCont g)) /\
      (forall c, hs_joins S' c = read_leaf de_join T (CJoins c)).
Proof.
  intros Hrc h r T Hw Hin.
  destruct (lookup_root_in r h T Hin) as [T' HT'].
  exists (reset_view T'). split; [unfold reset; rewrite HT'; reflexivity |].
  assert (Hr1 : r = root_of T) by (apply Hw; exact Hin).
  assert (Hr2 : r = root_of T') by (apply Hw; apply lookup_root_some; exact HT').
  assert (Hlk : forall k, tlookup k T' = tlookup k T) by (apply Hrc; congruence).
  unfold reset_view, read_leaf. simpl.
  repeat split; intros; rewrite Hlk; reflexivity.
Qed.

End CheckpointModel.

(* ═══════════════════ The premises hold for one instance ═══════════════════ *)

(* The instance: channels, values and patterns are numbers; an encoding is a
   list of numbers with a tag (0 a datum, 1 a waiter, 2 a join); the history
   key of a store key is the store key itself; the root of a trie is the
   trie. The byte order is the lexicographic order of number lists. *)

Fixpoint lex_leb (a b : list nat) : bool :=
  match a, b with
  | [], _ => true
  | _ :: _, [] => false
  | x :: r, y :: s => if Nat.ltb x y then true else if Nat.eqb x y then lex_leb r s else false
  end.

Lemma lex_total : forall a b, lex_leb a b = true \/ lex_leb b a = true.
Proof.
  induction a as [| x r IH]; intros [| y s]; simpl; auto.
  destruct (Nat.ltb x y) eqn:E1; [left; reflexivity |].
  destruct (Nat.ltb y x) eqn:E2; [right; reflexivity |].
  apply Nat.ltb_ge in E1. apply Nat.ltb_ge in E2. assert (x = y) by lia. subst y.
  rewrite Nat.eqb_refl. apply IH.
Qed.

Lemma lex_trans : forall a b c, lex_leb a b = true -> lex_leb b c = true -> lex_leb a c = true.
Proof.
  induction a as [| x r IH]; intros [| y s] [| z t] H1 H2; simpl in *; try reflexivity; try discriminate.
  destruct (Nat.ltb x y) eqn:Exy; destruct (Nat.ltb y z) eqn:Eyz.
  - apply Nat.ltb_lt in Exy. apply Nat.ltb_lt in Eyz.
    assert (Hxz : x < z) by lia. apply Nat.ltb_lt in Hxz. rewrite Hxz. reflexivity.
  - destruct (Nat.eqb y z) eqn:Eq; [| discriminate]. apply Nat.eqb_eq in Eq. subst z.
    rewrite Exy. reflexivity.
  - destruct (Nat.eqb x y) eqn:Eq; [| discriminate]. apply Nat.eqb_eq in Eq. subst y.
    rewrite Eyz. reflexivity.
  - destruct (Nat.eqb x y) eqn:Eq1; [| discriminate]. destruct (Nat.eqb y z) eqn:Eq2; [| discriminate].
    apply Nat.eqb_eq in Eq1. apply Nat.eqb_eq in Eq2. subst y z.
    rewrite Exy, Nat.eqb_refl. eapply IH; eassumption.
Qed.

Lemma lex_antisym : forall a b, lex_leb a b = true -> lex_leb b a = true -> a = b.
Proof.
  induction a as [| x r IH]; intros [| y s] H1 H2; simpl in *; try reflexivity; try discriminate.
  destruct (Nat.ltb x y) eqn:Exy; destruct (Nat.ltb y x) eqn:Eyx.
  - apply Nat.ltb_lt in Exy. apply Nat.ltb_lt in Eyx. lia.
  - destruct (Nat.eqb y x) eqn:E; [| discriminate]. apply Nat.eqb_eq in E. subst.
    apply Nat.ltb_lt in Exy. lia.
  - destruct (Nat.eqb x y) eqn:E; [| discriminate]. apply Nat.eqb_eq in E. subst.
    apply Nat.ltb_lt in Eyx. lia.
  - destruct (Nat.eqb x y) eqn:E1; [| discriminate]. apply Nat.eqb_eq in E1. subst y.
    rewrite Nat.eqb_refl in H2. f_equal. apply IH; assumption.
Qed.

Definition b2n (b : bool) : nat := if b then 1 else 0.

Definition inst_ser_datum (d : Datum nat) : list nat := [0; d_val nat d; b2n (d_persist nat d)].
Definition inst_de_datum (bs : list nat) : option (Datum nat) :=
  match bs with
  | [0; v; 0] => Some (mkDatum nat v false)
  | [0; v; 1] => Some (mkDatum nat v true)
  | _ => None
  end.
Definition inst_ser_wk (w : WK nat) : list nat := [1; w_pat nat w; b2n (w_persist nat w)].
Definition inst_de_wk (bs : list nat) : option (WK nat) :=
  match bs with
  | [1; p; 0] => Some (mkWK nat p false)
  | [1; p; 1] => Some (mkWK nat p true)
  | _ => None
  end.
Definition inst_ser_join (g : list nat) : list nat := 2 :: g.
Definition inst_de_join (bs : list nat) : option (list nat) :=
  match bs with 2 :: g => Some g | _ => None end.

Definition inst_trie_eq_dec :
  forall x y : list (CKey nat * list (list nat)), {x = y} + {x <> y}.
Proof.
  apply list_eq_dec. intros [k1 l1] [k2 l2].
  destruct (ckey_eq_dec nat Nat.eq_dec k1 k2) as [-> | Hk];
    [| right; intros H; inversion H; contradiction].
  destruct (list_eq_dec (list_eq_dec Nat.eq_dec) l1 l2) as [-> | Hl];
    [left; reflexivity | right; intros H; inversion H; contradiction].
Defined.

Lemma checkpoint_premises_satisfiable :
  roundtrips nat nat nat (list nat) inst_ser_datum inst_de_datum inst_ser_wk inst_de_wk
    inst_ser_join inst_de_join /\
  key_injective nat (CKey nat) (fun k => k) /\
  bytes_order (list nat) lex_leb /\
  root_complete (list nat) (CKey nat) (list (CKey nat * list (list nat)))
    (ckey_eq_dec nat Nat.eq_dec) (fun t => t).
Proof.
  split; [| split; [| split]].
  - split; [| split].
    + intros [v [|]]; reflexivity.
    + intros [p [|]]; reflexivity.
    + intros g; reflexivity.
  - intros k1 k2 H. exact H.
  - split; [exact lex_total | split; [exact lex_trans | exact lex_antisym]].
  - intros T1 T2 H k. rewrite H. reflexivity.
Qed.

(* ═══════════ Headline trust base: every headline is axiom-free ═══════════ *)

Print Assumptions checkpoint_reset_restores_store.
Print Assumptions checkpoint_restore_preserves_persist.
Print Assumptions replay_from_equal_parent_states.
Print Assumptions checkpoint_root_order_independent.
Print Assumptions rig_and_reset_replays_the_log.
Print Assumptions reset_after_sweep_drops_volatile.
Print Assumptions soft_revert_after_prefix_failure.
Print Assumptions chain_checkpoints_retrievable.
