(* ═══════════════════════════════════════════════════════════════════════════
   VolatileSweep.v — the volatile sweep, the checkpoint, root neutrality and
   leaf counts

   This module models SpaceTxnHost::sweep_volatile and
   SpaceTxnHost::root_leaf_record_count of P1 of the Reified RSpaces
   campaign, and the checkpoint of the hot store. The sweep removes the
   data, the continuations and the joins of the volatile (lowercase)
   channels at the end of a deploy. The module proves the headlines H13-H16
   of the P1 design report (§5(a)) and the headlines H18 and H22 of the P1
   fix rules. H15 has the statement of the fix rules (FR-1). The round-2
   rules change two things here. FR-1' makes a group canonical when it is
   non-decreasing, so a group can repeat a channel (H22 proves that the
   sorted list is the unique canonical member of its permutation class).
   FR-9' sends the sweep through the handle, which refuses a sweep under an
   open transaction of the same task (H13).

   Code base: the campaign tree after the merge of feature/cost-accounted-rho
   at 4d67b5ccc. Every host file that this header cites with a line range
   equals that file at 4d67b5ccc, except the Reified hooks that footprint
   policy p4 approves. Before the merge, the code base was HEAD 6f7a80da3
   (e748426 for the cited host files).
   The P1 file space_store_handle.rs is new. This header cites its items by
   name, because its line numbers change often.
   Design: p1-design-report §3.1 (algorithm and root neutrality), §5(a).
   Plan decision 19 (D1 placement, D5 outer-frame count). Fix rules FR-1,
   FR-2 and FR-8, and the round-2 rules FR-1' and FR-9'
   (prompts/p1-fix-rules.md).

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code
   (rspace++/src/rspace/ is abbreviated as rs/, and
   rs/space_store_handle.rs as ssh)
   ─────────────────────────────────────────────────────────────────────────
   HKey, key_data, key_cont,  rs/history/history_repository_impl.rs:307-349
     key_joins                (transform: hash(channel), hash_from_vec),
                              184-300 (prepend PREFIX_DATUM, PREFIX_KONT,
                              PREFIX_JOINS), rs/history/
                              history_repository.rs:93-95, rs/hashing/
                              stable_hash_provider.rs (hash_from_hashes
                              sorts the channel hashes and keeps a repeated
                              hash)
   ser_datum, ser_wk,         rs/serializers/serializers.rs:18-26, 39-50,
     ser_join, bytes_leb      69-77 (encode_*: bincode each record, sort
                              with compare_byte_vectors)
   Leaf, data_leaf, ...       the outer frame of DataLeaf, ContinuationsLeaf,
                              JoinsLeaf (history_repository_impl.rs:169-304,
                              calculate_storage_actions)
   tlookup, tinsert, tdelete  rs/history/radix_tree.rs:1557-1608 (delete:
                              "Not found" is None)
   apply_action, make_actions rs/history/radix_tree.rs:1619-1663
                              (make_actions, clearing_delete_actions)
   checkpoint, dup_free       rs/history/instances/radix_history.rs:100-105
                              (process rejects two actions on one key with
                              "Cannot process duplicate actions on one
                              key."), rs/history/history_repository_impl.rs:
                              467-490 (checkpoint), rs/rspace/ispace_impl.rs:
                              60-95
   HotStore, touch            rs/hot_store.rs:661-718, 865-877, 999-1027
                              (a first read caches the key)
   remove_datum_at            rs/hot_store.rs:912-945
   remove_cont_at             rs/hot_store.rs:818-861
   remove_join                rs/hot_store.rs:1069-1113
   changes, action_of         rs/hot_store.rs:1127-1182
   chan_leb, nondecreasing,   ssh require_canonical, group_identity (FR-1':
     canonical, ck_canonical  the keys of a group must be non-decreasing,
                              else SpaceTxnError::NonCanonicalChannelOrder)
   isort, sorted_unique_      the canonical form of a group: its channels
     canonical                sorted by hash, as hash_vec sorts them
   hkey_up_to_order           stable_hash_provider.rs hash_from_vec under
                              collision-free Blake2b
   read_key, reads            ssh TxnCore::read_data, read_continuations,
                              read_joins (FR-1 before any hot-store access)
   sweep_volatile, validate,  design §3.1 (dedup, consume_lock, G, the three
     post_ok                  groups, post check), ssh sweep_volatile_channels
                              (the groups keyed by group_identity, FR-2,
                              and require_canonical on each stored join
                              group before any mutation, FR-1)
   Ctx, handle_sweep,         ssh SpaceStoreHandle::sweep_volatile, HandleState,
     SweepErr                 TxnContext (FR-9': SpaceTxnError::
                              NestedTransaction)
   root_leaf_record_count     design §0.5 and D5: rs/history/
                              history_reader.rs:21, rs/history/instances/
                              rspace_history_reader_impl.rs:127-139, ssh
                              root_leaf_records
   lockstep                   the joins invariant of the hot store (H7 of
                              EventCorrelation.v)
   Txn, covers, book          P7: spaces/agent.rs SpaceEnv::begin (the lock
                              keys), spaces/ephemeral.rs VolatileRegistry::
                              record_locked and take_channels
   ephemeral                  spaces/ephemeral.rs ephemeral_space
   check_live, Liveness       reified/runtime.rs ReifiedRuntime::check_live
                              and check_live_space (DG-1)

   ─────────────────────────────────────────────────────────────────────────
   Modeling notes
   ─────────────────────────────────────────────────────────────────────────
   - The trie is an association list from history keys to leaves. A leaf is
     its outer frame (the sorted record encodings) with a kind tag. The
     radix tree computes its root from the same key-to-leaf map. The model
     takes the root as any function root_of of the trie. H18 states root
     equality under the premise root_respects_lookup: the root is a
     function of the key-to-leaf map.
   - The checkpoint rejects two actions on one history key, as
     RadixHistory::process does (H22). make_actions applies the actions in
     sequence. The radix tree groups actions by key prefix, but the result
     for distinct keys is the same map.
   - The hot store is a set of total functions (the read-through view) and a
     cache list. The cache list stands for the keys of the cached maps, in
     the order of the iteration in changes().
   - A sweep holds consume_lock on all swept channels (design §3.1). The model
     runs the sweep with no interleaving.
   - The validation of the sweep reads the join lists through get_joins, so
     the code caches the canonical keys CJoins c even when the validation
     fails. H18 shows that such reads do not change the checkpoint, so the
     model of the validation is read-only.
   - H15 and H18 need one premise on the key functions: no two distinct
     canonical cache keys share a history key (key_injective). Collision-
     free Blake2b keys and the kind prefixes give it. The cache holds only
     canonical keys, because FR-1 rejects every other group before any
     hot-store access. A permuted group shares the history key of its
     canonical group (hash_from_vec sorts), and H22 shows that two such
     keys make the checkpoint fail.
   - FR-1' (round 2) allows a repeated channel. hash_from_hashes hashes the
     concatenation of the sorted channel hashes and keeps a repeated hash,
     so two groups share a history key only when they are permutations of
     each other (hkey_up_to_order). H22 shows that a permutation class has
     exactly one canonical member, its sorted list, and derives
     key_injective from hkey_up_to_order (sorted_keys_injective). So the
     premise of H15 and H18 holds also for groups with repeated channels.
     The lemma order_premises_satisfiable shows the order premises and
     key_injective for a key function that sorts the group.
   - FR-9' (round 2): the handle keeps the set of execution contexts with an
     open transaction (FR-9). A sweep in such a context fails closed with
     NestedTransaction before any lock or hot-store access. Otherwise the
     handle calls the sweep of the host. handle_sweep models this guard,
     and H13 shows both cases. The sweep also registers its own context
     until it returns, so a begin of the same task during the sweep fails
     with NestedTransaction. That is the FR-9 guard at begin, which the
     models do not include.
   - FR-8 (SweepReport counts and replay_waiting_continuations_estimate) is
     bookkeeping with no store effect. The model does not include it.

   ─────────────────────────────────────────────────────────────────────────
   Headlines (each is closed under the global context)
   ─────────────────────────────────────────────────────────────────────────
   H13 sweep_exact                 the sweep removes exactly the volatile
                                   state, after a read-only validation. A
                                   sweep under an open transaction of the
                                   same task fails closed (FR-9')
   H14 delete_absent_identity      a delete of an absent key changes nothing
   H15 sweep_root_neutral          under FR-1 the checkpoint after the sweep
                                   succeeds and does not change the root
   H16 leaf_count_is_length        the leaf record count is the list length
   H18 canonical_reads_root_neutral reads of canonical keys never change the
                                   checkpoint root
   H22 checkpoint_rejects_duplicate_key_actions
                                   the checkpoint refuses two actions on one
                                   key (radix_history.rs:100-105). Under
                                   FR-1' the sorted list is the unique
                                   canonical member of its permutation class,
                                   also with repeated channels
   P7-1 sweep_complete            the book of a root evaluation holds every
                                   volatile channel that a transaction staged
                                   (the lock keys cover the staged channels,
                                   readiness review S2), so the sweep of the
                                   book leaves no volatile record, join entry
                                   or waiting continuation (design v3 VR-2)
   P7-2 dead_ephemeral_dangling   a space of another deploy is never live in
                                   this evaluation (I-19), so its ids fail
                                   before any job (design v3 DG-1)

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables become universally quantified parameters of the closed
   theorems. The file imports only the Rocq standard library.
   ═══════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import List PeanoNat Bool Lia Permutation Arith Sorting.Sorted.
Import ListNotations.

(* ───────────────────────── Generic list toolkit ───────────────────────── *)

Fixpoint remove_first {A : Type} (eqd : forall x y : A, {x = y} + {x <> y})
    (x : A) (l : list A) : list A :=
  match l with
  | [] => []
  | y :: t => if eqd x y then t else y :: remove_first eqd x t
  end.

Fixpoint remove_nth {A : Type} (i : nat) (l : list A) : list A :=
  match l, i with
  | [], _ => []
  | _ :: t, 0 => t
  | y :: t, S j => y :: remove_nth j t
  end.

(* Insertion sort under an arbitrary comparison: the serializers sort the
   encoded records with compare_byte_vectors (serializers.rs). *)
Fixpoint insb {A : Type} (leb : A -> A -> bool) (x : A) (l : list A) : list A :=
  match l with
  | [] => [x]
  | y :: t => if leb x y then x :: y :: t else y :: insb leb x t
  end.

Fixpoint isort {A : Type} (leb : A -> A -> bool) (l : list A) : list A :=
  match l with [] => [] | x :: t => insb leb x (isort leb t) end.

Lemma insb_perm : forall (A : Type) leb (x : A) l, Permutation (insb leb x l) (x :: l).
Proof.
  intros A leb x l. induction l as [| y t IH]; simpl; [reflexivity |].
  destruct (leb x y); [reflexivity |].
  eapply perm_trans; [apply perm_skip, IH | apply perm_swap].
Qed.

Lemma isort_perm : forall (A : Type) leb (l : list A), Permutation (isort leb l) l.
Proof.
  intros A leb l. induction l as [| x t IH]; simpl; [reflexivity |].
  eapply perm_trans; [apply insb_perm | apply perm_skip, IH].
Qed.

(* Under a total comparison, insertion keeps a list sorted, so the
   insertion sort is sorted (FR-1': the canonical form of a group). *)
Lemma insb_sorted :
  forall (A : Type) (leb : A -> A -> bool), (forall x y, leb x y = true \/ leb y x = true) ->
    forall x l, Sorted (fun a b => leb a b = true) l -> Sorted (fun a b => leb a b = true) (insb leb x l).
Proof.
  intros A leb Htot x l Hs. induction Hs as [| y t Hs IH Hhd]; simpl; [repeat constructor |].
  destruct (leb x y) eqn:Exy.
  - constructor; [constructor; [exact Hs | exact Hhd] | constructor; exact Exy].
  - assert (Hyx : leb y x = true) by (destruct (Htot x y) as [C | C]; [congruence | exact C]).
    constructor; [exact IH |].
    destruct t as [| z t']; simpl; [constructor; exact Hyx |].
    destruct (leb x z); constructor; [exact Hyx | inversion Hhd; assumption].
Qed.

Lemma isort_sorted :
  forall (A : Type) (leb : A -> A -> bool), (forall x y, leb x y = true \/ leb y x = true) ->
    forall l, Sorted (fun a b => leb a b = true) (isort leb l).
Proof.
  intros A leb Htot l. induction l as [| x t IH]; simpl; [constructor |].
  apply insb_sorted; assumption.
Qed.

Lemma remove_nth_last : forall (A : Type) (l : list A) x, remove_nth (length l) (l ++ [x]) = l.
Proof.
  intros A l x. induction l as [| y t IH]; simpl; [reflexivity |]. rewrite IH. reflexivity.
Qed.

(* Removing the indices n-1, ..., 1, 0 of a list of length n empties it
   (the reverse-index loops of the sweep, design §3.1). *)
Definition rev_indices (n : nat) : list nat := rev (seq 0 n).

Lemma remove_rev_indices_empty :
  forall (A : Type) (l : list A),
    fold_left (fun acc i => remove_nth i acc) (rev_indices (length l)) l = [].
Proof.
  intros A l. induction l as [| x t IH] using rev_ind; [reflexivity |].
  unfold rev_indices. rewrite length_app. simpl. rewrite Nat.add_1_r, seq_S, rev_app_distr.
  simpl. rewrite remove_nth_last. exact IH.
Qed.

Lemma remove_first_self_fold :
  forall (A : Type) (eqd : forall x y : A, {x = y} + {x <> y}) (l : list A),
    fold_left (fun acc j => remove_first eqd j acc) l l = [].
Proof.
  intros A eqd l. induction l as [| x t IH]; simpl; [reflexivity |].
  destruct (eqd x x) as [_ | C]; [exact IH | contradiction].
Qed.

(* ═════════════════════════════ The model ═════════════════════════════════ *)

Section Sweep.

Variables Chan Datum WK HKey Bytes Root : Type.
Variable chan_eq_dec : forall x y : Chan, {x = y} + {x <> y}.
Variable hkey_eq_dec : forall x y : HKey, {x = y} + {x <> y}.

(* History keys: the prefixed Blake2b keys of a data leaf, a continuation
   leaf and a joins leaf (history_repository_impl.rs, PREFIX_DATUM,
   PREFIX_KONT, PREFIX_JOINS). *)
Variable key_data : Chan -> HKey.
Variable key_cont : list Chan -> HKey.
Variable key_joins : Chan -> HKey.

(* bincode of one record, and compare_byte_vectors (serializers.rs). *)
Variable ser_datum : Datum -> Bytes.
Variable ser_wk : WK -> Bytes.
Variable ser_join : list Chan -> Bytes.
Variable bytes_leb : Bytes -> Bytes -> bool.

(* The Merkle root of a trie (any function of the trie). *)
Variable root_of : list (HKey * (list Bytes * nat)) -> Root.

Definition lc_eq_dec : forall x y : list Chan, {x = y} + {x <> y} := list_eq_dec chan_eq_dec.

(* ── History trie (radix_tree.rs) ──────────────────────────────────────── *)

(* A leaf is its outer frame (the sorted record encodings) tagged with its
   kind: 0 data, 1 continuations, 2 joins. *)
Definition Leaf := (list Bytes * nat)%type.
Definition Trie := list (HKey * Leaf).

Fixpoint tlookup (k : HKey) (t : Trie) : option Leaf :=
  match t with
  | [] => None
  | (k', l) :: rest => if hkey_eq_dec k k' then Some l else tlookup k rest
  end.

Definition hkey_neqb (k : HKey) (e : HKey * Leaf) : bool :=
  if hkey_eq_dec (fst e) k then false else true.

(* RadixTree::delete returns None ("Not found") for an absent key
   (radix_tree.rs:1584-1608). The caller then leaves the node unchanged. *)
Definition tdelete (k : HKey) (t : Trie) : option Trie :=
  match tlookup k t with
  | None => None
  | Some _ => Some (filter (hkey_neqb k) t)
  end.

Definition tinsert (k : HKey) (l : Leaf) (t : Trie) : Trie := (k, l) :: filter (hkey_neqb k) t.

Inductive Action := AInsert (k : HKey) (l : Leaf) | ADelete (k : HKey).

Definition apply_action (t : Trie) (a : Action) : Trie :=
  match a with
  | AInsert k l => tinsert k l t
  | ADelete k => match tdelete k t with None => t | Some t' => t' end
  end.

Definition action_key (a : Action) : HKey := match a with AInsert k _ => k | ADelete k => k end.

(* RadixTree::make_actions: the history actions in sequence. *)
Definition make_actions (t : Trie) (acts : list Action) : Trie := fold_left apply_action acts t.

Definition hkey_eqb (x y : HKey) : bool := if hkey_eq_dec x y then true else false.

Fixpoint dup_free (l : list HKey) : bool :=
  match l with
  | [] => true
  | x :: t => negb (existsb (hkey_eqb x) t) && dup_free t
  end.

(* The checkpoint: RadixHistory::process sorts the action keys and rejects
   two actions on one key with "Cannot process duplicate actions on one
   key." (radix_history.rs:100-105). The checkpoint then fails (None).
   Otherwise make_actions applies the actions. *)
Definition checkpoint (t : Trie) (acts : list Action) : option Trie :=
  if dup_free (map action_key acts) then Some (make_actions t acts) else None.

(* ── Hot store with its cache (hot_store.rs) ────────────────────────────── *)

Inductive CKey := CData (c : Chan) | CCont (g : list Chan) | CJoins (c : Chan).

Definition ckey_eq_dec : forall x y : CKey, {x = y} + {x <> y}.
Proof. decide equality; first [apply chan_eq_dec | apply lc_eq_dec]. Defined.

Record HotStore := mkHS {
  hs_data : Chan -> list Datum;
  hs_conts : list Chan -> list WK;
  hs_joins : Chan -> list (list Chan);
  hs_cache : list CKey }.

(* A first read or write caches the key (hot_store.rs:713, 874, 1006). *)
Definition touch (k : CKey) (S : HotStore) : HotStore :=
  if in_dec ckey_eq_dec k (hs_cache S) then S
  else mkHS (hs_data S) (hs_conts S) (hs_joins S) (hs_cache S ++ [k]).

Definition set_data (S : HotStore) (c : Chan) (l : list Datum) : HotStore :=
  let S0 := touch (CData c) S in
  mkHS (fun c' => if chan_eq_dec c' c then l else hs_data S0 c') (hs_conts S0) (hs_joins S0) (hs_cache S0).
Definition set_conts (S : HotStore) (g : list Chan) (l : list WK) : HotStore :=
  let S0 := touch (CCont g) S in
  mkHS (hs_data S0) (fun g' => if lc_eq_dec g' g then l else hs_conts S0 g') (hs_joins S0) (hs_cache S0).
Definition set_joins (S : HotStore) (c : Chan) (l : list (list Chan)) : HotStore :=
  let S0 := touch (CJoins c) S in
  mkHS (hs_data S0) (hs_conts S0) (fun c' => if chan_eq_dec c' c then l else hs_joins S0 c') (hs_cache S0).

(* remove_datum and remove_continuation by index (hot_store.rs:912-945,
   818-861). remove_join removes a join only when the continuation list of
   the join is empty (hot_store.rs:1069-1113). *)
Definition remove_datum_at (S : HotStore) (c : Chan) (i : nat) : HotStore :=
  set_data S c (remove_nth i (hs_data S c)).
Definition remove_cont_at (S : HotStore) (g : list Chan) (i : nat) : HotStore :=
  set_conts S g (remove_nth i (hs_conts S g)).
Definition remove_join (S : HotStore) (c : Chan) (j : list Chan) : HotStore :=
  match hs_conts S j with
  | [] => set_joins S c (remove_first lc_eq_dec j (hs_joins S c))
  | _ :: _ => touch (CJoins c) S
  end.

(* The three loops of sweep_volatile (design §3.1). Each loop first reads
   its key, and the read caches the key. *)
Definition remove_all_conts (S : HotStore) (g : list Chan) : HotStore :=
  let S0 := touch (CCont g) S in
  fold_left (fun S i => remove_cont_at S g i) (rev_indices (length (hs_conts S0 g))) S0.
Definition remove_all_joins (S : HotStore) (c : Chan) : HotStore :=
  let S0 := touch (CJoins c) S in
  fold_left (fun S j => remove_join S c j) (hs_joins S0 c) S0.
Definition remove_all_data (S : HotStore) (c : Chan) : HotStore :=
  let S0 := touch (CData c) S in
  fold_left (fun S i => remove_datum_at S c i) (rev_indices (length (hs_data S0 c))) S0.

(* G := ⋃_{c∈S} get_joins(c) ∪ {[c] | c∈S}. *)
Definition groups (S : HotStore) (sw : list Chan) : list (list Chan) :=
  flat_map (fun c => hs_joins S c) sw ++ map (fun c => [c]) sw.

Definition is_none {A : Type} (o : option A) : bool := match o with None => true | Some _ => false end.

Definition inb (c : Chan) (l : list Chan) : bool := if in_dec chan_eq_dec c l then true else false.

(* FR-1' (round 2): chan_leb x y is the comparison hash(x) <= hash(y) of the
   stable_hash_provider::hash bytes of two channels. Any comparison function
   is allowed. The order lemmas state their premises (transitive,
   antisymmetric, total) explicitly. A group is canonical when it is
   non-decreasing. A group can repeat a channel. FR-1 (round 1) required a
   strictly increasing group and rejected a repeated channel with
   DuplicateChannelInGroup. FR-1' removes that error. *)
Variable chan_leb : Chan -> Chan -> bool.

Fixpoint nondecreasing (g : list Chan) : bool :=
  match g with
  | [] => true
  | x :: t => match t with [] => true | y :: _ => chan_leb x y && nondecreasing t end
  end.

Definition canonical (g : list Chan) : bool := nondecreasing g.

Definition ck_canonical (k : CKey) : bool :=
  match k with CCont g => canonical g | _ => true end.

(* Validation, read-only (design §3.1, FR-1 and FR-1'): SweepCrossesDurable,
   InstalledChannel, SweepTouchesPersisted, and NonCanonicalChannelOrder for
   a stored join group that is not non-decreasing. *)
Definition validate (T : Trie) (installed : Chan -> bool) (sw : list Chan) (G : list (list Chan)) : bool :=
  forallb (fun g => forallb (fun c => inb c sw) g) G
  && forallb (fun c => negb (installed c)) sw
  && forallb (fun c => is_none (tlookup (key_data c) T) && is_none (tlookup (key_joins c) T)) sw
  && forallb (fun g => is_none (tlookup (key_cont g) T)) G
  && forallb canonical G.

Definition nullb {A : Type} (l : list A) : bool := match l with [] => true | _ => false end.

Definition post_ok (S : HotStore) (sw : list Chan) (G : list (list Chan)) : bool :=
  forallb (fun c => nullb (hs_data S c) && nullb (hs_joins S c)) sw
  && forallb (fun g => nullb (hs_conts S g)) G.

Definition sweep_volatile (T : Trie) (installed : Chan -> bool) (S : HotStore) (chans : list Chan)
    : option HotStore :=
  let sw := nodup chan_eq_dec chans in
  let G := groups S sw in
  if negb (validate T installed sw G) then None else
  let S1 := fold_left remove_all_conts G S in
  let S2 := fold_left remove_all_joins sw S1 in
  let S3 := fold_left remove_all_data sw S2 in
  if post_ok S3 sw G then Some S3 else None.

(* ── The sweep through the handle (FR-9') ─────────────────────────────── *)

(* FR-9' (round 2): SpaceStoreHandle::sweep_volatile refuses a sweep in an
   execution context (a tokio task, or a thread without a task) that holds
   an open transaction of the same handle. It fails closed with
   NestedTransaction before any lock or hot-store access. In any other
   context the handle runs the sweep of the host. [open] is the set of
   contexts with an open transaction of the handle (HandleState). *)
Variable Ctx : Type.
Variable ctx_eq_dec : forall x y : Ctx, {x = y} + {x <> y}.

Inductive SweepErr := NestedTransaction | SweepRejected.

(* The store after the call, and the error of a failed call. A failed sweep
   leaves the store as it was (the validation is read-only, H13). *)
Definition handle_sweep (open : list Ctx) (cur : Ctx) (T : Trie) (installed : Chan -> bool)
    (S : HotStore) (chans : list Chan) : HotStore * option SweepErr :=
  if in_dec ctx_eq_dec cur open then (S, Some NestedTransaction)
  else match sweep_volatile T installed S chans with
       | Some S' => (S', None)
       | None => (S, Some SweepRejected)
       end.

(* ── Checkpoint: HotStore::changes then the history update ─────────────── *)

Definition data_leaf (l : list Datum) : Leaf := (isort bytes_leb (map ser_datum l), 0).
Definition cont_leaf (l : list WK) : Leaf := (isort bytes_leb (map ser_wk l), 1).
Definition joins_leaf (l : list (list Chan)) : Leaf := (isort bytes_leb (map ser_join l), 2).

(* hot_store.rs:1127-1182: an empty cached list becomes a Delete, a
   non-empty one an Insert of the sorted leaf (serializers.rs). *)
Definition action_of (S : HotStore) (k : CKey) : Action :=
  match k with
  | CData c => match hs_data S c with [] => ADelete (key_data c) | l => AInsert (key_data c) (data_leaf l) end
  | CCont g => match hs_conts S g with [] => ADelete (key_cont g) | l => AInsert (key_cont g) (cont_leaf l) end
  | CJoins c => match hs_joins S c with [] => ADelete (key_joins c) | l => AInsert (key_joins c) (joins_leaf l) end
  end.

Definition changes (S : HotStore) : list Action := map (action_of S) (hs_cache S).

Definition hkey_of (k : CKey) : HKey :=
  match k with CData c => key_data c | CCont g => key_cont g | CJoins c => key_joins c end.

Lemma action_key_of : forall S k, action_key (action_of S k) = hkey_of k.
Proof.
  intros S [c | g | c]; simpl;
    [destruct (hs_data S c) | destruct (hs_conts S g) | destruct (hs_joins S c)]; reflexivity.
Qed.

Lemma changes_keys : forall S, map action_key (changes S) = map hkey_of (hs_cache S).
Proof. intros S. unfold changes. rewrite map_map. apply map_ext. apply action_key_of. Qed.

(* Collision freedom of the history keys on canonical cache keys: the
   prefixes separate the three kinds, and Blake2b is collision free. *)
Definition key_injective : Prop :=
  forall k1 k2, ck_canonical k1 = true -> ck_canonical k2 = true -> hkey_of k1 = hkey_of k2 -> k1 = k2.

Definition cache_canonical (S : HotStore) : Prop := Forall (fun k => ck_canonical k = true) (hs_cache S).

(* root_leaf_record_count (decision D5): the number of records in the outer
   frame of the data leaf (get_data_proj_binary,
   rspace_history_reader_impl.rs:127-139). An absent leaf counts zero. *)
Definition root_leaf_record_count (t : Trie) (c : Chan) : nat :=
  match tlookup (key_data c) t with Some (fr, 0) => length fr | _ => 0 end.

(* The swept cache keys and the store without them. *)
Definition swept (sw : list Chan) (G : list (list Chan)) (k : CKey) : bool :=
  match k with
  | CData c | CJoins c => inb c sw
  | CCont g => if in_dec lc_eq_dec g G then true else false
  end.

Definition forget (S : HotStore) (sw : list Chan) (G : list (list Chan)) : HotStore :=
  mkHS (hs_data S) (hs_conts S) (hs_joins S) (filter (fun k => negb (swept sw G k)) (hs_cache S)).

Definition lockstep (S : HotStore) : Prop :=
  forall c g, In g (hs_joins S c) <-> (In c g /\ hs_conts S g <> []).

(* ═══════════════════════════ Store lemmas ════════════════════════════════ *)

(* The cache only grows, by appending keys that satisfy P. *)
Definition grows_by (P : CKey -> bool) (S S' : HotStore) : Prop :=
  exists added, hs_cache S' = hs_cache S ++ added /\ forallb P added = true.

Lemma grows_refl : forall P S, grows_by P S S.
Proof. intros P S. exists []. rewrite app_nil_r. split; reflexivity. Qed.

Lemma grows_trans : forall P S1 S2 S3, grows_by P S1 S2 -> grows_by P S2 S3 -> grows_by P S1 S3.
Proof.
  intros P S1 S2 S3 (a1 & E1 & F1) (a2 & E2 & F2). exists (a1 ++ a2).
  rewrite E2, E1, app_assoc. split; [reflexivity |]. rewrite forallb_app, F1, F2. reflexivity.
Qed.

Lemma touch_fields :
  forall k S, hs_data (touch k S) = hs_data S /\ hs_conts (touch k S) = hs_conts S /\
              hs_joins (touch k S) = hs_joins S.
Proof. intros k S. unfold touch. destruct (in_dec ckey_eq_dec k (hs_cache S)); auto. Qed.

Lemma touch_grows : forall (P : CKey -> bool) k S, P k = true -> grows_by P S (touch k S).
Proof.
  intros P k S Hk. unfold touch. destruct (in_dec ckey_eq_dec k (hs_cache S)).
  - apply grows_refl.
  - exists [k]. simpl. rewrite Hk. split; reflexivity.
Qed.

Lemma touch_in : forall k S, In k (hs_cache (touch k S)).
Proof.
  intros k S. unfold touch. destruct (in_dec ckey_eq_dec k (hs_cache S)) as [H | H]; [exact H |].
  simpl. apply in_or_app. right. left. reflexivity.
Qed.

Lemma touch_cached : forall k S, In k (hs_cache S) -> touch k S = S.
Proof. intros k S H. unfold touch. destruct (in_dec ckey_eq_dec k (hs_cache S)); [reflexivity | contradiction]. Qed.

(* The removal fold over one key, as a list fold. *)
Lemma conts_fold :
  forall is S g, In (CCont g) (hs_cache S) ->
    let S' := fold_left (fun S i => remove_cont_at S g i) is S in
    hs_conts S' g = fold_left (fun acc i => remove_nth i acc) is (hs_conts S g) /\
    (forall g', g' <> g -> hs_conts S' g' = hs_conts S g') /\
    hs_data S' = hs_data S /\ hs_joins S' = hs_joins S /\ hs_cache S' = hs_cache S.
Proof.
  induction is as [| i rest IH]; intros S g Hin; simpl; [auto |].
  set (S1 := remove_cont_at S g i).
  assert (E1 : hs_cache S1 = hs_cache S /\ hs_data S1 = hs_data S /\ hs_joins S1 = hs_joins S /\
               hs_conts S1 g = remove_nth i (hs_conts S g) /\
               (forall g', g' <> g -> hs_conts S1 g' = hs_conts S g')).
  { subst S1. unfold remove_cont_at, set_conts. rewrite (touch_cached _ _ Hin). simpl.
    split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
    split; [destruct (lc_eq_dec g g); [reflexivity | contradiction] |].
    intros g' N. destruct (lc_eq_dec g' g); [contradiction | reflexivity]. }
  destruct E1 as (Ec & Ed & Ej & Eg & Eo).
  destruct (IH S1 g ltac:(rewrite Ec; exact Hin)) as (Ig & Io & Id & Ij & Ic).
  split; [rewrite Ig, Eg; reflexivity |].
  split; [intros g' N; rewrite Io by exact N; apply Eo, N |].
  split; [rewrite Id; exact Ed |]. split; [rewrite Ij; exact Ej | rewrite Ic; exact Ec].
Qed.

Lemma data_fold :
  forall is S c, In (CData c) (hs_cache S) ->
    let S' := fold_left (fun S i => remove_datum_at S c i) is S in
    hs_data S' c = fold_left (fun acc i => remove_nth i acc) is (hs_data S c) /\
    (forall c', c' <> c -> hs_data S' c' = hs_data S c') /\
    hs_conts S' = hs_conts S /\ hs_joins S' = hs_joins S /\ hs_cache S' = hs_cache S.
Proof.
  induction is as [| i rest IH]; intros S c Hin; simpl; [auto |].
  set (S1 := remove_datum_at S c i).
  assert (E1 : hs_cache S1 = hs_cache S /\ hs_conts S1 = hs_conts S /\ hs_joins S1 = hs_joins S /\
               hs_data S1 c = remove_nth i (hs_data S c) /\
               (forall c', c' <> c -> hs_data S1 c' = hs_data S c')).
  { subst S1. unfold remove_datum_at, set_data. rewrite (touch_cached _ _ Hin). simpl.
    split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
    split; [destruct (chan_eq_dec c c); [reflexivity | contradiction] |].
    intros c' N. destruct (chan_eq_dec c' c); [contradiction | reflexivity]. }
  destruct E1 as (Ec & Ek & Ej & Eg & Eo).
  destruct (IH S1 c ltac:(rewrite Ec; exact Hin)) as (Ig & Io & Ik & Ij & Ic).
  split; [rewrite Ig, Eg; reflexivity |].
  split; [intros c' N; rewrite Io by exact N; apply Eo, N |].
  split; [rewrite Ik; exact Ek |]. split; [rewrite Ij; exact Ej | rewrite Ic; exact Ec].
Qed.

Lemma joins_fold :
  forall js S c, In (CJoins c) (hs_cache S) -> (forall j, In j js -> hs_conts S j = []) ->
    let S' := fold_left (fun S j => remove_join S c j) js S in
    hs_joins S' c = fold_left (fun acc j => remove_first lc_eq_dec j acc) js (hs_joins S c) /\
    (forall c', c' <> c -> hs_joins S' c' = hs_joins S c') /\
    hs_data S' = hs_data S /\ hs_conts S' = hs_conts S /\ hs_cache S' = hs_cache S.
Proof.
  induction js as [| j rest IH]; intros S c Hin Hemp; simpl; [auto |].
  set (S1 := remove_join S c j).
  assert (E1 : hs_cache S1 = hs_cache S /\ hs_conts S1 = hs_conts S /\ hs_data S1 = hs_data S /\
               hs_joins S1 c = remove_first lc_eq_dec j (hs_joins S c) /\
               (forall c', c' <> c -> hs_joins S1 c' = hs_joins S c')).
  { subst S1. unfold remove_join. rewrite (Hemp j (or_introl eq_refl)).
    unfold set_joins. rewrite (touch_cached _ _ Hin). simpl.
    split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
    split; [destruct (chan_eq_dec c c); [reflexivity | contradiction] |].
    intros c' N. destruct (chan_eq_dec c' c); [contradiction | reflexivity]. }
  destruct E1 as (Ec & Ek & Ed & Eg & Eo).
  assert (Hemp1 : forall j', In j' rest -> hs_conts S1 j' = []).
  { intros j' Hj'. rewrite Ek. apply Hemp. right. exact Hj'. }
  destruct (IH S1 c ltac:(rewrite Ec; exact Hin) Hemp1) as (Ig & Io & Id & Ik & Ic).
  split; [rewrite Ig, Eg; reflexivity |].
  split; [intros c' N; rewrite Io by exact N; apply Eo, N |].
  split; [rewrite Id; exact Ed |]. split; [rewrite Ik; exact Ek | rewrite Ic; exact Ec].
Qed.

(* ═══════════════════════════ The three sweep steps ════════════════════════ *)


Lemma remove_all_conts_spec :
  forall S g,
    hs_conts (remove_all_conts S g) g = [] /\
    (forall g', g' <> g -> hs_conts (remove_all_conts S g) g' = hs_conts S g') /\
    hs_data (remove_all_conts S g) = hs_data S /\ hs_joins (remove_all_conts S g) = hs_joins S /\
    hs_cache (remove_all_conts S g) = hs_cache (touch (CCont g) S).
Proof.
  intros S g. unfold remove_all_conts.
  destruct (touch_fields (CCont g) S) as (T0d & T0c & T0j).
  destruct (conts_fold (rev_indices (length (hs_conts (touch (CCont g) S) g))) (touch (CCont g) S) g
              (touch_in _ _)) as (Fg & Fo & Fd & Fj & Fc).
  rewrite remove_rev_indices_empty in Fg.
  split; [exact Fg |]. split; [intros g' N; rewrite Fo by exact N; rewrite T0c; reflexivity |].
  split; [rewrite Fd, T0d; reflexivity |]. split; [rewrite Fj, T0j; reflexivity | exact Fc].
Qed.

Lemma remove_all_joins_spec :
  forall S c, (forall j, In j (hs_joins S c) -> hs_conts S j = []) ->
    hs_joins (remove_all_joins S c) c = [] /\
    (forall c', c' <> c -> hs_joins (remove_all_joins S c) c' = hs_joins S c') /\
    hs_data (remove_all_joins S c) = hs_data S /\ hs_conts (remove_all_joins S c) = hs_conts S /\
    hs_cache (remove_all_joins S c) = hs_cache (touch (CJoins c) S).
Proof.
  intros S c Hemp. unfold remove_all_joins.
  destruct (touch_fields (CJoins c) S) as (T0d & T0c & T0j).
  assert (Hemp0 : forall j, In j (hs_joins (touch (CJoins c) S) c) -> hs_conts (touch (CJoins c) S) j = []).
  { intros j Hj. rewrite T0c. rewrite T0j in Hj. apply Hemp, Hj. }
  destruct (joins_fold (hs_joins (touch (CJoins c) S) c) (touch (CJoins c) S) c (touch_in _ _) Hemp0)
    as (Fg & Fo & Fd & Fk & Fc).
  rewrite remove_first_self_fold in Fg.
  split; [exact Fg |]. split; [intros c' N; rewrite Fo by exact N; rewrite T0j; reflexivity |].
  split; [rewrite Fd, T0d; reflexivity |]. split; [rewrite Fk, T0c; reflexivity | exact Fc].
Qed.

Lemma remove_all_data_spec :
  forall S c,
    hs_data (remove_all_data S c) c = [] /\
    (forall c', c' <> c -> hs_data (remove_all_data S c) c' = hs_data S c') /\
    hs_conts (remove_all_data S c) = hs_conts S /\ hs_joins (remove_all_data S c) = hs_joins S /\
    hs_cache (remove_all_data S c) = hs_cache (touch (CData c) S).
Proof.
  intros S c. unfold remove_all_data.
  destruct (touch_fields (CData c) S) as (T0d & T0c & T0j).
  destruct (data_fold (rev_indices (length (hs_data (touch (CData c) S) c))) (touch (CData c) S) c
              (touch_in _ _)) as (Fg & Fo & Fk & Fj & Fc).
  rewrite remove_rev_indices_empty in Fg.
  split; [exact Fg |]. split; [intros c' N; rewrite Fo by exact N; rewrite T0d; reflexivity |].
  split; [rewrite Fk, T0c; reflexivity |]. split; [rewrite Fj, T0j; reflexivity | exact Fc].
Qed.

Lemma grows_cache_eq :
  forall P S S' k, P k = true -> hs_cache S' = hs_cache (touch k S) -> grows_by P S S'.
Proof.
  intros P S S' k Hk E. destruct (touch_grows P k S Hk) as (a & Ea & Fa).
  exists a. rewrite E, Ea. auto.
Qed.

Lemma step_conts :
  forall G S (P : CKey -> bool),
    (forall g, In g G -> P (CCont g) = true) ->
    let S' := fold_left remove_all_conts G S in
    (forall g, In g G -> hs_conts S' g = []) /\
    (forall g, ~ In g G -> hs_conts S' g = hs_conts S g) /\
    hs_data S' = hs_data S /\ hs_joins S' = hs_joins S /\ grows_by P S S'.
Proof.
  induction G as [| g rest IH]; intros S P HP; simpl.
  - split; [intros g []|]. split; [auto |]. split; [reflexivity |]. split; [reflexivity | apply grows_refl].
  - set (S1 := remove_all_conts S g).
    destruct (remove_all_conts_spec S g) as (Fg & Fo & Fd & Fj & Fc). fold S1 in Fg, Fo, Fd, Fj, Fc.
    destruct (IH S1 P ltac:(intros g' Hg'; apply HP; right; exact Hg')) as (Ig & Io & Id & Ij & Ic).
    split.
    + intros g' [E | Hg'].
      * subst g'. destruct (in_dec lc_eq_dec g rest) as [Hr | Hr]; [apply Ig, Hr |].
        rewrite Io by exact Hr. exact Fg.
      * apply Ig, Hg'.
    + split.
      * intros g' Hn. rewrite Io by (intro H; apply Hn; right; exact H).
        apply Fo. intro E. apply Hn. left. symmetry. exact E.
      * split; [rewrite Id, Fd; reflexivity |]. split; [rewrite Ij, Fj; reflexivity |].
        eapply grows_trans; [| exact Ic].
        apply (grows_cache_eq P S S1 (CCont g) (HP g (or_introl eq_refl)) Fc).
Qed.

Lemma step_joins :
  forall sw S (P : CKey -> bool),
    (forall c, In c sw -> P (CJoins c) = true) ->
    (forall c j, In c sw -> In j (hs_joins S c) -> hs_conts S j = []) ->
    let S' := fold_left remove_all_joins sw S in
    (forall c, In c sw -> hs_joins S' c = []) /\
    (forall c, ~ In c sw -> hs_joins S' c = hs_joins S c) /\
    hs_data S' = hs_data S /\ hs_conts S' = hs_conts S /\ grows_by P S S'.
Proof.
  induction sw as [| c rest IH]; intros S P HP Hemp; simpl.
  - split; [intros c []|]. split; [auto |]. split; [reflexivity |]. split; [reflexivity | apply grows_refl].
  - set (S1 := remove_all_joins S c).
    destruct (remove_all_joins_spec S c ltac:(intros j Hj; apply (Hemp c j (or_introl eq_refl) Hj)))
      as (Fg & Fo & Fd & Fk & Fc). fold S1 in Fg, Fo, Fd, Fk, Fc.
    assert (Hemp1 : forall c' j, In c' rest -> In j (hs_joins S1 c') -> hs_conts S1 j = []).
    { intros c' j Hc' Hj. rewrite Fk.
      destruct (chan_eq_dec c' c) as [E | N].
      - subst c'. rewrite Fg in Hj. contradiction.
      - rewrite Fo in Hj by exact N. apply (Hemp c' j (or_intror Hc') Hj). }
    destruct (IH S1 P ltac:(intros c' Hc'; apply HP; right; exact Hc') Hemp1) as (Ig & Io & Id & Ik & Ic).
    split.
    + intros c' [E | Hc'].
      * subst c'. destruct (in_dec chan_eq_dec c rest) as [Hr | Hr]; [apply Ig, Hr |].
        rewrite Io by exact Hr. exact Fg.
      * apply Ig, Hc'.
    + split.
      * intros c' Hn. rewrite Io by (intro H; apply Hn; right; exact H).
        apply Fo. intro E. apply Hn. left. symmetry. exact E.
      * split; [rewrite Id, Fd; reflexivity |]. split; [rewrite Ik, Fk; reflexivity |].
        eapply grows_trans; [| exact Ic].
        apply (grows_cache_eq P S S1 (CJoins c) (HP c (or_introl eq_refl)) Fc).
Qed.

Lemma step_data :
  forall sw S (P : CKey -> bool),
    (forall c, In c sw -> P (CData c) = true) ->
    let S' := fold_left remove_all_data sw S in
    (forall c, In c sw -> hs_data S' c = []) /\
    (forall c, ~ In c sw -> hs_data S' c = hs_data S c) /\
    hs_conts S' = hs_conts S /\ hs_joins S' = hs_joins S /\ grows_by P S S'.
Proof.
  induction sw as [| c rest IH]; intros S P HP; simpl.
  - split; [intros c []|]. split; [auto |]. split; [reflexivity |]. split; [reflexivity | apply grows_refl].
  - set (S1 := remove_all_data S c).
    destruct (remove_all_data_spec S c) as (Fg & Fo & Fk & Fj & Fc). fold S1 in Fg, Fo, Fk, Fj, Fc.
    destruct (IH S1 P ltac:(intros c' Hc'; apply HP; right; exact Hc')) as (Ig & Io & Ik & Ij & Ic).
    split.
    + intros c' [E | Hc'].
      * subst c'. destruct (in_dec chan_eq_dec c rest) as [Hr | Hr]; [apply Ig, Hr |].
        rewrite Io by exact Hr. exact Fg.
      * apply Ig, Hc'.
    + split.
      * intros c' Hn. rewrite Io by (intro H; apply Hn; right; exact H).
        apply Fo. intro E. apply Hn. left. symmetry. exact E.
      * split; [rewrite Ik, Fk; reflexivity |]. split; [rewrite Ij, Fj; reflexivity |].
        eapply grows_trans; [| exact Ic].
        apply (grows_cache_eq P S S1 (CData c) (HP c (or_introl eq_refl)) Fc).
Qed.

Lemma validate_spec :
  forall T installed sw G,
    validate T installed sw G = true ->
    (forall g c, In g G -> In c g -> In c sw) /\
    (forall c, In c sw -> installed c = false) /\
    (forall c, In c sw -> tlookup (key_data c) T = None /\ tlookup (key_joins c) T = None) /\
    (forall g, In g G -> tlookup (key_cont g) T = None) /\
    (forall g, In g G -> canonical g = true).
Proof.
  intros T installed sw G H. unfold validate in H. repeat rewrite andb_true_iff in H.
  destruct H as ((((Hg & Hi) & Hd) & Hk) & Hcan).
  rewrite forallb_forall in Hg, Hi, Hd, Hk, Hcan.
  split; [| split; [| split; [| split]]]; [| | | | exact Hcan].
  - intros g c Hgin Hc. specialize (Hg g Hgin). rewrite forallb_forall in Hg.
    specialize (Hg c Hc). unfold inb in Hg. destruct (in_dec chan_eq_dec c sw); [assumption | discriminate].
  - intros c Hc. apply negb_true_iff, Hi, Hc.
  - intros c Hc. specialize (Hd c Hc). apply andb_true_iff in Hd as [A B].
    unfold is_none in A, B. destruct (tlookup (key_data c) T); [discriminate |].
    destruct (tlookup (key_joins c) T); [discriminate | auto].
  - intros g Hgin. specialize (Hk g Hgin). unfold is_none in Hk.
    destruct (tlookup (key_cont g) T); [discriminate | reflexivity].
Qed.

Lemma in_groups_joins :
  forall S sw c j, In c sw -> In j (hs_joins S c) -> In j (groups S sw).
Proof.
  intros S sw c j Hc Hj. unfold groups. apply in_or_app. left.
  apply in_flat_map. exists c. auto.
Qed.

Lemma in_groups_single : forall S sw c, In c sw -> In [c] (groups S sw).
Proof.
  intros S sw c Hc. unfold groups. apply in_or_app. right. apply in_map_iff. exists c. split; [reflexivity | exact Hc].
Qed.

(* The sweep specification: after validation, the sweep empties exactly the
   volatile data, continuations and joins. It leaves every other key
   unchanged. It adds only swept keys to the cache. It keeps joins in
   lockstep. A failed validation leaves the store untouched, because
   validation comes before any mutation. *)
Lemma sweep_spec :
  forall T installed S chans,
    let sw := nodup chan_eq_dec chans in
    let G := groups S sw in
    (validate T installed sw G = false -> sweep_volatile T installed S chans = None) /\
    (validate T installed sw G = true ->
     exists S',
       sweep_volatile T installed S chans = Some S' /\
       (forall c, In c chans -> hs_data S' c = [] /\ hs_joins S' c = []) /\
       (forall g, In g G -> hs_conts S' g = []) /\
       (forall c, ~ In c chans -> hs_data S' c = hs_data S c /\ hs_joins S' c = hs_joins S c) /\
       (forall g, ~ In g G -> hs_conts S' g = hs_conts S g) /\
       grows_by (swept sw G) S S' /\
       (lockstep S -> lockstep S')).
Proof.
  intros T installed S chans sw G. split.
  - intros Hv. unfold sweep_volatile. fold sw G. rewrite Hv. reflexivity.
  - intros Hv. destruct (validate_spec T installed sw G Hv) as (Hsub & _ & _ & _ & _).
    assert (Hsw : forall c, In c sw <-> In c chans) by (intros c; apply nodup_In).
    destruct (step_conts G S (swept sw G)) as (C1g & C1o & C1d & C1j & C1c).
    { intros g Hg. simpl. destruct (in_dec lc_eq_dec g G); [reflexivity | contradiction]. }
    set (S1 := fold_left remove_all_conts G S) in *.
    destruct (step_joins sw S1 (swept sw G)) as (J2g & J2o & J2d & J2c & J2r).
    { intros c Hc. simpl. unfold inb. destruct (in_dec chan_eq_dec c sw); [reflexivity | contradiction]. }
    { intros c j Hc Hj. apply C1g. rewrite C1j in Hj. apply (in_groups_joins S sw c j Hc Hj). }
    set (S2 := fold_left remove_all_joins sw S1) in *.
    destruct (step_data sw S2 (swept sw G)) as (D3g & D3o & D3c & D3j & D3r).
    { intros c Hc. simpl. unfold inb. destruct (in_dec chan_eq_dec c sw); [reflexivity | contradiction]. }
    set (S3 := fold_left remove_all_data sw S2) in *.
    assert (Hpost : post_ok S3 sw G = true).
    { unfold post_ok. apply andb_true_iff. split; rewrite forallb_forall.
      - intros c Hc. rewrite D3g by exact Hc. rewrite D3j, J2g by exact Hc. reflexivity.
      - intros g Hg. rewrite D3c, J2c, C1g by exact Hg. reflexivity. }
    exists S3. split.
    { unfold sweep_volatile. fold sw G. rewrite Hv. simpl. fold S1 S2 S3. rewrite Hpost. reflexivity. }
    split; [intros c Hc; apply Hsw in Hc; split; [apply D3g, Hc | rewrite D3j; apply J2g, Hc] |].
    split; [intros g Hg; rewrite D3c, J2c; apply C1g, Hg |].
    split.
    { intros c Hc. assert (Hc' : ~ In c sw) by (intro H; apply Hc, Hsw, H).
      split; [rewrite D3o by exact Hc'; rewrite J2d, C1d; reflexivity |].
      rewrite D3j, J2o by exact Hc'. rewrite C1j. reflexivity. }
    split; [intros g Hg; rewrite D3c, J2c; apply C1o, Hg |].
    split; [eapply grows_trans; [exact C1c | eapply grows_trans; [exact J2r | exact D3r]] |].
    intros L c g.
    assert (Hj3 : hs_joins S3 c = if in_dec chan_eq_dec c sw then [] else hs_joins S c).
    { destruct (in_dec chan_eq_dec c sw) as [Hc | Hc]; rewrite D3j;
        [apply J2g, Hc | rewrite J2o by exact Hc; rewrite C1j; reflexivity]. }
    assert (Hk3 : hs_conts S3 g = if in_dec lc_eq_dec g G then [] else hs_conts S g).
    { rewrite D3c, J2c. destruct (in_dec lc_eq_dec g G) as [Hg | Hg]; [apply C1g, Hg | apply C1o, Hg]. }
    rewrite Hj3, Hk3.
    destruct (in_dec chan_eq_dec c sw) as [Hc | Hc]; destruct (in_dec lc_eq_dec g G) as [Hg | Hg].
    + split; [contradiction | intros [_ C]; exfalso; apply C; reflexivity].
    + split; [contradiction |]. intros [Hcg Hne]. exfalso. apply Hg.
      apply (in_groups_joins S sw c g Hc). apply L. split; assumption.
    + split.
      * intros Hgj. exfalso. apply Hc. apply (Hsub g c Hg). apply L in Hgj as [Hcg _]. exact Hcg.
      * intros [_ C]. exfalso. apply C. reflexivity.
    + apply L.
Qed.

(* H13: the sweep specification (sweep_spec), and the FR-9' guard of the
   handle: a sweep in a context that holds an open transaction of the same
   handle fails closed with NestedTransaction and leaves the store as it
   was. In any other context the handle runs exactly the sweep of
   sweep_spec. *)
Theorem sweep_exact :
  forall T installed S chans,
    let sw := nodup chan_eq_dec chans in
    let G := groups S sw in
    (validate T installed sw G = false -> sweep_volatile T installed S chans = None) /\
    (validate T installed sw G = true ->
     exists S',
       sweep_volatile T installed S chans = Some S' /\
       (forall c, In c chans -> hs_data S' c = [] /\ hs_joins S' c = []) /\
       (forall g, In g G -> hs_conts S' g = []) /\
       (forall c, ~ In c chans -> hs_data S' c = hs_data S c /\ hs_joins S' c = hs_joins S c) /\
       (forall g, ~ In g G -> hs_conts S' g = hs_conts S g) /\
       grows_by (swept sw G) S S' /\
       (lockstep S -> lockstep S')) /\
    (forall open cur, In cur open ->
       handle_sweep open cur T installed S chans = (S, Some NestedTransaction)) /\
    (forall open cur, ~ In cur open ->
       handle_sweep open cur T installed S chans =
         match sweep_volatile T installed S chans with
         | Some S' => (S', None)
         | None => (S, Some SweepRejected)
         end).
Proof.
  intros T installed S chans sw G.
  destruct (sweep_spec T installed S chans) as [Hf Ht]. fold sw G in Hf, Ht.
  split; [exact Hf |]. split; [exact Ht |]. split.
  - intros open cur Hin. unfold handle_sweep.
    destruct (in_dec ctx_eq_dec cur open); [reflexivity | contradiction].
  - intros open cur Hn. unfold handle_sweep.
    destruct (in_dec ctx_eq_dec cur open); [contradiction | reflexivity].
Qed.

(* ═══════════════════ History trie: delete and lookup ═════════════════════ *)

Lemma tlookup_filter :
  forall t k k', tlookup k' (filter (hkey_neqb k) t) = if hkey_eq_dec k' k then None else tlookup k' t.
Proof.
  induction t as [| [k0 l0] t IH]; intros k k'; simpl.
  - destruct (hkey_eq_dec k' k); reflexivity.
  - unfold hkey_neqb at 1. simpl. destruct (hkey_eq_dec k0 k) as [E | N].
    + subst k0. rewrite IH. destruct (hkey_eq_dec k' k) as [E' | N']; [reflexivity |].
      reflexivity.
    + simpl. destruct (hkey_eq_dec k' k0) as [E' | N'].
      * subst k'. destruct (hkey_eq_dec k0 k) as [C | _]; [contradiction | reflexivity].
      * rewrite IH. reflexivity.
Qed.

Lemma dup_free_nodup : forall l, dup_free l = true <-> NoDup l.
Proof.
  induction l as [| x t IH]; simpl; [split; [intros _; constructor | reflexivity] |].
  split.
  - intros H. apply andb_true_iff in H as [Hn Ht]. constructor; [| apply IH, Ht].
    intros Hin. apply negb_true_iff in Hn. rewrite <- Bool.not_true_iff_false in Hn. apply Hn.
    apply existsb_exists. exists x. split; [exact Hin |]. unfold hkey_eqb.
    destruct (hkey_eq_dec x x); [reflexivity | contradiction].
  - intros H. inversion H as [| x' t' Hn Ht]; subst. apply andb_true_iff. split; [| apply IH, Ht].
    apply negb_true_iff, Bool.not_true_iff_false. intros Hex.
    apply existsb_exists in Hex as (y & Hy & Ey). unfold hkey_eqb in Ey.
    destruct (hkey_eq_dec x y) as [E | _]; [subst y; contradiction | discriminate].
Qed.

Lemma checkpoint_spec :
  forall t acts,
    (checkpoint t acts = None <-> ~ NoDup (map action_key acts)) /\
    (NoDup (map action_key acts) -> checkpoint t acts = Some (make_actions t acts)).
Proof.
  intros t acts. unfold checkpoint. split.
  - destruct (dup_free (map action_key acts)) eqn:E.
    + apply dup_free_nodup in E. split; [discriminate | intros C; contradiction].
    + split; [intros _ Hnd; apply dup_free_nodup in Hnd; congruence | reflexivity].
  - intros Hnd. apply dup_free_nodup in Hnd. rewrite Hnd. reflexivity.
Qed.

(* H14: deleting a key that the trie does not contain is "Not found", and
   the history update then leaves the trie exactly unchanged
   (radix_tree.rs:1584-1608, clearing_delete_actions at 1646-1663). The
   checkpoint is unchanged too, when no other action targets the key. *)
Theorem delete_absent_identity :
  forall t k, tlookup k t = None ->
    tdelete k t = None /\ apply_action t (ADelete k) = t /\
    (forall acts, make_actions t (ADelete k :: acts) = make_actions t acts) /\
    (forall acts, ~ In k (map action_key acts) -> checkpoint t (ADelete k :: acts) = checkpoint t acts) /\
    (forall l, tlookup k t = Some l -> False).
Proof.
  intros t k H.
  assert (Hd : tdelete k t = None) by (unfold tdelete; rewrite H; reflexivity).
  assert (Ha : apply_action t (ADelete k) = t) by (unfold apply_action; rewrite Hd; reflexivity).
  assert (Hm : forall acts, make_actions t (ADelete k :: acts) = make_actions t acts).
  { intros acts. unfold make_actions. cbn [fold_left]. rewrite Ha. reflexivity. }
  split; [exact Hd |]. split; [exact Ha |]. split; [exact Hm |].
  split; [| intros l E; rewrite H in E; discriminate].
  intros acts Hn. unfold checkpoint. rewrite Hm. cbn [map dup_free action_key].
  replace (existsb (hkey_eqb k) (map action_key acts)) with false; [reflexivity |].
  symmetry. apply Bool.not_true_iff_false. intros Hex.
  apply existsb_exists in Hex as (y & Hy & Ey). unfold hkey_eqb in Ey.
  destruct (hkey_eq_dec k y) as [E | _]; [subst y; contradiction | discriminate].
Qed.

Lemma apply_action_other :
  forall t a k, action_key a <> k -> tlookup k (apply_action t a) = tlookup k t.
Proof.
  intros t [k0 l | k0] k N; simpl in N; simpl.
  - unfold tinsert. simpl. destruct (hkey_eq_dec k k0) as [E | _]; [subst; contradiction |].
    rewrite tlookup_filter. destruct (hkey_eq_dec k k0); [subst; contradiction | reflexivity].
  - unfold tdelete. destruct (tlookup k0 t); [| reflexivity].
    rewrite tlookup_filter. destruct (hkey_eq_dec k k0); [subst; contradiction | reflexivity].
Qed.

Lemma apply_action_self :
  forall t a, tlookup (action_key a) (apply_action t a) =
              match a with AInsert _ l => Some l | ADelete _ => None end.
Proof.
  intros t [k l | k]; simpl.
  - unfold tinsert. simpl. destruct (hkey_eq_dec k k) as [_ | C]; [reflexivity | contradiction].
  - unfold tdelete. destruct (tlookup k t) eqn:E; [| exact E].
    rewrite tlookup_filter. destruct (hkey_eq_dec k k) as [_ | C]; [reflexivity | contradiction].
Qed.

Lemma make_actions_preserves_lookup :
  forall A t k, (forall a, In a A -> action_key a <> k) -> tlookup k (make_actions t A) = tlookup k t.
Proof.
  induction A as [| a A IH]; intros t k H; [reflexivity |].
  unfold make_actions in *. simpl. rewrite IH by (intros a' Ha'; apply H; right; exact Ha').
  apply apply_action_other. apply H. left. reflexivity.
Qed.

Lemma lookup_after_unique :
  forall A1 a A2 t,
    (forall a', In a' A2 -> action_key a' <> action_key a) ->
    tlookup (action_key a) (make_actions t (A1 ++ a :: A2)) =
    match a with AInsert _ l => Some l | ADelete _ => None end.
Proof.
  intros A1 a A2 t H. unfold make_actions. rewrite fold_left_app. simpl.
  fold (make_actions (apply_action (fold_left apply_action A1 t) a) A2).
  rewrite make_actions_preserves_lookup by exact H. apply apply_action_self.
Qed.

(* ═══════════════════ Root neutrality of the sweep (H15) ═══════════════════ *)

Definition swept_hkeys (sw : list Chan) (G : list (list Chan)) : list HKey :=
  flat_map (fun c => [key_data c; key_joins c]) sw ++ map key_cont G.

Definition del_in (K : list HKey) (a : Action) : bool :=
  match a with
  | ADelete k => if in_dec hkey_eq_dec k K then true else false
  | AInsert _ _ => false
  end.

Lemma apply_keeps_absent :
  forall t a K, (forall k, In k K -> tlookup k t = None) ->
    (forall k l, a = AInsert k l -> ~ In k K) ->
    forall k, In k K -> tlookup k (apply_action t a) = None.
Proof.
  intros t [k0 l | k0] K Habs Hins k Hk.
  - rewrite apply_action_other; [apply Habs, Hk |]. simpl. intro E. subst k0.
    exact (Hins k l eq_refl Hk).
  - unfold apply_action, tdelete. destruct (tlookup k0 t); [| apply Habs, Hk].
    rewrite tlookup_filter. destruct (hkey_eq_dec k k0); [reflexivity | apply Habs, Hk].
Qed.

Lemma make_actions_drop_absent_deletes :
  forall A t K,
    (forall k, In k K -> tlookup k t = None) ->
    (forall k l, In (AInsert k l) A -> ~ In k K) ->
    make_actions t A = make_actions t (filter (fun a => negb (del_in K a)) A).
Proof.
  induction A as [| a A IH]; intros t K Habs Hins; [reflexivity |].
  assert (Habs' : forall k, In k K -> tlookup k (apply_action t a) = None).
  { apply apply_keeps_absent; [exact Habs |]. intros k l E. subst a. apply (Hins k l). left. reflexivity. }
  assert (Hins' : forall k l, In (AInsert k l) A -> ~ In k K) by (intros k l H; apply (Hins k l); right; exact H).
  unfold make_actions in *.
  destruct a as [k l | k].
  - assert (Ef : filter (fun a => negb (del_in K a)) (AInsert k l :: A) =
                 AInsert k l :: filter (fun a => negb (del_in K a)) A) by reflexivity.
    rewrite Ef. cbn [fold_left]. apply IH; assumption.
  - destruct (in_dec hkey_eq_dec k K) as [Hk | Hk].
    + assert (Ef : filter (fun a => negb (del_in K a)) (ADelete k :: A) =
                   filter (fun a => negb (del_in K a)) A).
      { cbn [filter del_in]. destruct (in_dec hkey_eq_dec k K); [reflexivity | contradiction]. }
      rewrite Ef. cbn [fold_left].
      assert (Hid : apply_action t (ADelete k) = t).
      { unfold apply_action, tdelete. rewrite (Habs k Hk). reflexivity. }
      rewrite Hid. apply IH; assumption.
    + assert (Ef : filter (fun a => negb (del_in K a)) (ADelete k :: A) =
                   ADelete k :: filter (fun a => negb (del_in K a)) A).
      { cbn [filter del_in]. destruct (in_dec hkey_eq_dec k K); [contradiction | reflexivity]. }
      rewrite Ef. cbn [fold_left]. apply IH; assumption.
Qed.

Lemma filter_map_swept :
  forall (l : list CKey) (f f' : CKey -> Action) (Q : Action -> bool) (sw_k : CKey -> bool),
    (forall k, In k l -> sw_k k = true -> Q (f k) = false) ->
    (forall k, In k l -> sw_k k = false -> f k = f' k) ->
    filter Q (map f l) = filter Q (map f' (filter (fun k => negb (sw_k k)) l)).
Proof.
  induction l as [| k l IH]; intros f f' Q sw_k Hsw Hns; [reflexivity |].
  simpl. destruct (sw_k k) eqn:Ek; simpl.
  - rewrite (Hsw k (or_introl eq_refl) Ek). apply IH.
    + intros k' Hk'. apply Hsw. right. exact Hk'.
    + intros k' Hk'. apply Hns. right. exact Hk'.
  - rewrite (Hns k (or_introl eq_refl) Ek).
    destruct (Q (f' k)); [f_equal |]; apply IH;
      first [intros k' Hk'; apply Hsw; right; exact Hk' | intros k' Hk'; apply Hns; right; exact Hk'].
Qed.

Lemma filter_map_all_dropped :
  forall (l : list CKey) (f : CKey -> Action) (Q : Action -> bool) (P : CKey -> bool),
    (forall k, In k l -> P k = true) -> (forall k, P k = true -> Q (f k) = false) ->
    filter Q (map f l) = [].
Proof.
  induction l as [| k l IH]; intros f Q P Hl Hd; [reflexivity |].
  simpl. rewrite (Hd k (Hl k (or_introl eq_refl))).
  apply (IH f Q P); [intros k' Hk'; apply Hl; right; exact Hk' | exact Hd].
Qed.

Lemma filter_all_false :
  forall (l : list CKey) (P : CKey -> bool), (forall k, In k l -> P k = true) ->
    filter (fun k => negb (P k)) l = [].
Proof.
  induction l as [| k l IH]; intros P H; [reflexivity |].
  simpl. rewrite (H k (or_introl eq_refl)). simpl. apply IH. intros k' Hk'. apply H. right. exact Hk'.
Qed.

Lemma sweep_some_validate :
  forall T installed S chans S', sweep_volatile T installed S chans = Some S' ->
    validate T installed (nodup chan_eq_dec chans) (groups S (nodup chan_eq_dec chans)) = true.
Proof.
  intros T installed S chans S' H. unfold sweep_volatile in H.
  destruct (validate T installed _ _); [reflexivity | discriminate].
Qed.

(* The sweep and make_actions: if no durable insert targets a volatile
   history key, the trie after the sweep and make_actions is the trie of the
   durable changes alone. *)
Lemma sweep_make_actions_neutral :
  forall T installed S chans S',
    sweep_volatile T installed S chans = Some S' ->
    let sw := nodup chan_eq_dec chans in
    let G := groups S sw in
    (forall k l, In (AInsert k l) (changes (forget S sw G)) -> ~ In k (swept_hkeys sw G)) ->
    make_actions T (changes S') = make_actions T (changes (forget S sw G)) /\
    ((forall k, In k (hs_cache S) -> swept sw G k = true) -> make_actions T (changes S') = T).
Proof.
  intros T installed S chans S' Hs sw G Hdur.
  assert (Hv := sweep_some_validate T installed S chans S' Hs). fold sw G in Hv.
  destruct (validate_spec T installed sw G Hv) as (_ & _ & Habs_c & Habs_g & Hcan_g).
  destruct (sweep_spec T installed S chans) as [_ Hex]. fold sw G in Hex.
  destruct (Hex Hv) as (S'' & Hs'' & Hsw_c & Hsw_g & Hns_c & Hns_g & (added & Hcache & Hadded) & _).
  rewrite Hs in Hs''. injection Hs'' as <-.
  assert (Hsw : forall c, In c sw <-> In c chans) by (intros c; apply nodup_In).
  set (K := swept_hkeys sw G).
  assert (Habs : forall k, In k K -> tlookup k T = None).
  { intros k Hk. unfold K, swept_hkeys in Hk. apply in_app_or in Hk as [Hk | Hk].
    - apply in_flat_map in Hk as (c & Hc & Hk). destruct Hk as [E | [E | []]]; subst k;
        apply (Habs_c c Hc).
    - apply in_map_iff in Hk as (g & E & Hg). subst k. apply Habs_g, Hg. }
  set (Q := fun a => negb (del_in K a)).
  (* swept keys produce deletes of swept history keys *)
  assert (Hdel : forall k, swept sw G k = true -> Q (action_of S' k) = false).
  { intros [c | g | c] Hk; simpl in Hk; unfold Q.
    - unfold inb in Hk. destruct (in_dec chan_eq_dec c sw) as [Hc | _]; [| discriminate].
      simpl. rewrite (proj1 (Hsw_c c (proj1 (Hsw c) Hc))). simpl.
      destruct (in_dec hkey_eq_dec (key_data c) K) as [_ | C]; [reflexivity |].
      exfalso. apply C. unfold K, swept_hkeys. apply in_or_app. left. apply in_flat_map.
      exists c. split; [exact Hc | left; reflexivity].
    - destruct (in_dec lc_eq_dec g G) as [Hg | _]; [| discriminate].
      simpl. rewrite (Hsw_g g Hg). simpl.
      destruct (in_dec hkey_eq_dec (key_cont g) K) as [_ | C]; [reflexivity |].
      exfalso. apply C. unfold K, swept_hkeys. apply in_or_app. right. apply in_map. exact Hg.
    - unfold inb in Hk. destruct (in_dec chan_eq_dec c sw) as [Hc | _]; [| discriminate].
      simpl. rewrite (proj2 (Hsw_c c (proj1 (Hsw c) Hc))). simpl.
      destruct (in_dec hkey_eq_dec (key_joins c) K) as [_ | C]; [reflexivity |].
      exfalso. apply C. unfold K, swept_hkeys. apply in_or_app. left. apply in_flat_map.
      exists c. split; [exact Hc | right; left; reflexivity]. }
  (* durable keys keep their action *)
  assert (Hsame : forall k, swept sw G k = false -> action_of S' k = action_of S k).
  { intros [c | g | c] Hk; simpl in Hk; simpl.
    - unfold inb in Hk. destruct (in_dec chan_eq_dec c sw) as [_ | Hc]; [discriminate |].
      rewrite (proj1 (Hns_c c (fun H => Hc (proj2 (Hsw c) H)))). reflexivity.
    - destruct (in_dec lc_eq_dec g G) as [_ | Hg]; [discriminate |].
      rewrite (Hns_g g Hg). reflexivity.
    - unfold inb in Hk. destruct (in_dec chan_eq_dec c sw) as [_ | Hc]; [discriminate |].
      rewrite (proj2 (Hns_c c (fun H => Hc (proj2 (Hsw c) H)))). reflexivity. }
  assert (Hfilt : filter Q (changes S') = filter Q (changes (forget S sw G))).
  { unfold changes, forget. simpl. rewrite Hcache, map_app, filter_app.
    assert (Hz : filter Q (map (action_of S') added) = []).
    { apply (filter_map_all_dropped added (action_of S') Q (swept sw G)); [| exact Hdel].
      rewrite forallb_forall in Hadded. exact Hadded. }
    rewrite Hz, app_nil_r.
    apply filter_map_swept with (sw_k := swept sw G).
    - intros k _ Hk. apply Hdel, Hk.
    - intros k _ Hk. apply Hsame, Hk. }
  assert (Hins' : forall k l, In (AInsert k l) (changes S') -> ~ In k K).
  { intros k l Hin. apply (Hdur k l).
    assert (Hq : Q (AInsert k l) = true) by reflexivity.
    assert (Hf : In (AInsert k l) (filter Q (changes S'))) by (apply filter_In; split; assumption).
    rewrite Hfilt in Hf. apply filter_In in Hf as [Hf _]. exact Hf. }
  assert (Heq : make_actions T (changes S') = make_actions T (changes (forget S sw G))).
  { rewrite (make_actions_drop_absent_deletes (changes S') T K Habs Hins').
    rewrite (make_actions_drop_absent_deletes (changes (forget S sw G)) T K Habs Hdur).
    fold Q. rewrite Hfilt. reflexivity. }
  split; [exact Heq |].
  intros Hall. rewrite Heq. unfold changes, forget. cbn [hs_cache].
  rewrite (filter_all_false (hs_cache S) (swept sw G) Hall). reflexivity.
Qed.

(* The cache stays duplicate free: every key enters through touch. *)
Lemma touch_nodup : forall k S, NoDup (hs_cache S) -> NoDup (hs_cache (touch k S)).
Proof.
  intros k S Hnd. unfold touch. destruct (in_dec ckey_eq_dec k (hs_cache S)) as [_ | Hn]; [exact Hnd |].
  simpl. apply NoDup_app; [exact Hnd | repeat constructor; intros [] |].
  intros x Hx [E | []]. subst x. contradiction.
Qed.

Lemma fold_nodup :
  forall (A : Type) (f : HotStore -> A -> HotStore),
    (forall S a, NoDup (hs_cache S) -> NoDup (hs_cache (f S a))) ->
    forall l S, NoDup (hs_cache S) -> NoDup (hs_cache (fold_left f l S)).
Proof.
  intros A f Hf l. induction l as [| a l IH]; intros S Hnd; simpl; [exact Hnd |]. apply IH, Hf, Hnd.
Qed.

Lemma sweep_nodup :
  forall T installed S chans S', sweep_volatile T installed S chans = Some S' ->
    NoDup (hs_cache S) -> NoDup (hs_cache S').
Proof.
  intros T installed S chans S' Hs Hnd. unfold sweep_volatile in Hs.
  destruct (negb (validate _ _ _ _)); [discriminate |].
  destruct (post_ok _ _ _); [| discriminate]. injection Hs as <-.
  apply fold_nodup; [| apply fold_nodup; [| apply fold_nodup; [| exact Hnd]]].
  - intros S0 c H0. unfold remove_all_data. apply fold_nodup; [| apply touch_nodup, H0].
    intros S1 i H1. unfold remove_datum_at, set_data. simpl. apply touch_nodup, H1.
  - intros S0 c H0. unfold remove_all_joins. apply fold_nodup; [| apply touch_nodup, H0].
    intros S1 j H1. unfold remove_join. destruct (hs_conts S1 j); [| apply touch_nodup, H1].
    unfold set_joins. simpl. apply touch_nodup, H1.
  - intros S0 g H0. unfold remove_all_conts. apply fold_nodup; [| apply touch_nodup, H0].
    intros S1 i H1. unfold remove_cont_at, set_conts. simpl. apply touch_nodup, H1.
Qed.

Lemma nodup_map_hkey :
  forall l, key_injective -> NoDup l -> Forall (fun k => ck_canonical k = true) l -> NoDup (map hkey_of l).
Proof.
  intros l Hinj. induction l as [| k t IH]; simpl; intros Hnd Hc; [constructor |].
  inversion Hnd as [| k' t' Hn Ht]; subst. inversion Hc as [| k' t' Hk Hct]; subst.
  constructor; [| apply IH; assumption].
  intros Hin. apply in_map_iff in Hin as (k2 & E & Hk2). apply Hn.
  rewrite Forall_forall in Hct.
  replace k with k2; [exact Hk2 |]. apply Hinj; [apply Hct, Hk2 | exact Hk | exact E].
Qed.

Lemma checkpoint_changes_some :
  forall T S, key_injective -> NoDup (hs_cache S) -> cache_canonical S ->
    checkpoint T (changes S) = Some (make_actions T (changes S)).
Proof.
  intros T S Hinj Hnd Hc. apply (proj2 (checkpoint_spec T (changes S))).
  rewrite changes_keys. apply nodup_map_hkey; assumption.
Qed.

Lemma swept_canonical :
  forall sw G k, (forall g, In g G -> canonical g = true) -> swept sw G k = true -> ck_canonical k = true.
Proof.
  intros sw G [c | g | c] HG Hk; simpl in *; [reflexivity | | reflexivity].
  destruct (in_dec lc_eq_dec g G) as [Hg | _]; [apply HG, Hg | discriminate].
Qed.

(* H15 (FR-1): the sweep validates that every group is canonical, and the
   cache holds only canonical keys. Collision-free history keys then give
   that no durable insert targets a volatile history key, and that no two
   cached keys share a history key. So the checkpoint after the sweep
   succeeds, and its trie is exactly the trie of the durable changes alone.
   A deploy that touched only volatile keys leaves the trie, and so the
   root, unchanged. *)
Theorem sweep_root_neutral :
  forall T installed S chans S',
    key_injective ->
    NoDup (hs_cache S) ->
    cache_canonical S ->
    sweep_volatile T installed S chans = Some S' ->
    let sw := nodup chan_eq_dec chans in
    let G := groups S sw in
    exists T1,
      checkpoint T (changes S') = Some T1 /\
      checkpoint T (changes (forget S sw G)) = Some T1 /\
      ((forall k, In k (hs_cache S) -> swept sw G k = true) -> T1 = T /\ root_of T1 = root_of T).
Proof.
  intros T installed S chans S' Hinj Hnd Hcan Hs sw G.
  assert (Hv := sweep_some_validate T installed S chans S' Hs). fold sw G in Hv.
  destruct (validate_spec T installed sw G Hv) as (_ & _ & _ & _ & Hcan_g).
  destruct (sweep_spec T installed S chans) as [_ Hex]. fold sw G in Hex.
  destruct (Hex Hv) as (S'' & Hs'' & _ & _ & _ & _ & (added & Hcache & Hadded) & _).
  rewrite Hs in Hs''. injection Hs'' as <-.
  unfold cache_canonical in Hcan. rewrite Forall_forall in Hcan.
  assert (Hdur : forall k l, In (AInsert k l) (changes (forget S sw G)) -> ~ In k (swept_hkeys sw G)).
  { intros k l Hin HK.
    unfold changes in Hin.
    apply in_map_iff in Hin as (ck & Eck & Hck).
    assert (Ek : hkey_of ck = k) by (rewrite <- (action_key_of (forget S sw G) ck), Eck; reflexivity).
    unfold forget in Hck. cbn [hs_cache] in Hck. apply filter_In in Hck as [Hck Hns].
    apply negb_true_iff in Hns.
    assert (Hsw' : exists ck', ck_canonical ck' = true /\ hkey_of ck' = k /\ swept sw G ck' = true).
    { unfold swept_hkeys in HK. apply in_app_or in HK as [HK | HK].
      - apply in_flat_map in HK as (c & Hc & HK).
        assert (Hin_c : inb c sw = true) by (unfold inb; destruct (in_dec chan_eq_dec c sw); [reflexivity | contradiction]).
        destruct HK as [E | [E | []]].
        + exists (CData c). simpl. split; [reflexivity | split; [exact E | exact Hin_c]].
        + exists (CJoins c). simpl. split; [reflexivity | split; [exact E | exact Hin_c]].
      - apply in_map_iff in HK as (g & E & Hg). exists (CCont g). simpl.
        split; [apply Hcan_g, Hg |]. split; [exact E |].
        destruct (in_dec lc_eq_dec g G); [reflexivity | contradiction]. }
    destruct Hsw' as (ck' & Hc' & Ek' & Hsw').
    assert (E : ck = ck') by (apply Hinj; [apply Hcan, Hck | exact Hc' | congruence]).
    subst ck'. congruence. }
  destruct (sweep_make_actions_neutral T installed S chans S' Hs Hdur) as (Heq & Hall).
  fold sw G in Heq, Hall.
  assert (Hnd' : NoDup (hs_cache S')) by exact (sweep_nodup T installed S chans S' Hs Hnd).
  assert (Hcan' : cache_canonical S').
  { unfold cache_canonical. rewrite Forall_forall. intros k Hk. rewrite Hcache in Hk.
    apply in_app_or in Hk as [Hk | Hk]; [apply Hcan, Hk |].
    rewrite forallb_forall in Hadded. apply (swept_canonical sw G k Hcan_g), Hadded, Hk. }
  assert (Hndf : NoDup (hs_cache (forget S sw G))) by (apply NoDup_filter, Hnd).
  assert (Hcanf : cache_canonical (forget S sw G)).
  { unfold cache_canonical, forget. cbn [hs_cache]. rewrite Forall_forall. intros k Hk.
    apply filter_In in Hk as [Hk _]. apply Hcan, Hk. }
  exists (make_actions T (changes S')).
  split; [apply checkpoint_changes_some; assumption |].
  split; [rewrite Heq; apply checkpoint_changes_some; assumption |].
  intros Hsw. rewrite (Hall Hsw). split; reflexivity.
Qed.

(* ═══════════════ Leaf record count (H16, decision D5) ═════════════════════ *)

(* H16: after a successful checkpoint, root_leaf_record_count of a channel
   equals the number of records that the hot store held at that channel.
   The data key of the channel must be in the cache once, and no other
   cached key may share its history key. *)
Theorem leaf_count_is_length :
  forall T S c T',
    In (CData c) (hs_cache S) ->
    NoDup (hs_cache S) ->
    (forall k, In k (hs_cache S) -> hkey_of k = key_data c -> k = CData c) ->
    checkpoint T (changes S) = Some T' ->
    root_leaf_record_count T' c = length (hs_data S c).
Proof.
  intros T S c T' Hin Hnd Huniq Hck.
  assert (ET : T' = make_actions T (changes S)).
  { unfold checkpoint in Hck. destruct (dup_free _); [injection Hck as <-; reflexivity | discriminate]. }
  subst T'.
  apply in_split in Hin as (l1 & l2 & El).
  assert (Hn2 : ~ In (CData c) l2).
  { rewrite El in Hnd. apply NoDup_remove_2 in Hnd. intro H. apply Hnd. apply in_or_app. right. exact H. }
  unfold changes. rewrite El, map_app. cbn [map].
  assert (Hk : action_key (action_of S (CData c)) = key_data c) by apply action_key_of.
  assert (Hother : forall a', In a' (map (action_of S) l2) -> action_key a' <> action_key (action_of S (CData c))).
  { intros a' Ha'. apply in_map_iff in Ha' as (k & Ek & Hk2). subst a'. rewrite !action_key_of. simpl.
    intro E. apply Hn2. replace (CData c) with k; [exact Hk2 |].
    apply Huniq; [rewrite El; apply in_or_app; right; right; exact Hk2 | exact E]. }
  assert (L := lookup_after_unique (map (action_of S) l1) (action_of S (CData c)) (map (action_of S) l2) T Hother).
  rewrite Hk in L. unfold root_leaf_record_count. rewrite L.
  unfold action_of. destruct (hs_data S c) as [| d ds] eqn:Ed; [reflexivity |].
  unfold data_leaf. rewrite <- Ed.
  rewrite (Permutation_length (isort_perm Bytes bytes_leb (map ser_datum (hs_data S c)))).
  rewrite length_map. reflexivity.
Qed.

(* ═══════════════════ Canonical reads (H18, FR-1) ═══════════════════════════ *)

(* A read through the hot store. FR-1 rejects a non-canonical key before
   any hot-store access. A canonical read caches the key (touch). *)
Definition read_key (k : CKey) (S : HotStore) : option HotStore :=
  if ck_canonical k then Some (touch k S) else None.

(* A sequence of canonical reads. *)
Definition reads (ks : list CKey) (S : HotStore) : HotStore := fold_left (fun S k => touch k S) ks S.

(* The leaf that the checkpoint writes for a key: Some leaf for an Insert,
   None for a Delete. *)
Definition leaf_opt (S : HotStore) (k : CKey) : option Leaf :=
  match action_of S k with AInsert _ l => Some l | ADelete _ => None end.

(* An uncached key reads through to the trie: the history read decodes the
   leaf at its history key, and the encoders of changes() give the same leaf
   back. *)
Definition read_through (T : Trie) (S : HotStore) : Prop :=
  forall k, ~ In k (hs_cache S) -> tlookup (hkey_of k) T = leaf_opt S k.

(* The Merkle root of the radix tree is a function of its key-to-leaf map. *)
Definition root_respects_lookup : Prop :=
  forall t1 t2, (forall k, tlookup k t1 = tlookup k t2) -> root_of t1 = root_of t2.

Lemma reads_fields :
  forall ks S, hs_data (reads ks S) = hs_data S /\ hs_conts (reads ks S) = hs_conts S /\
               hs_joins (reads ks S) = hs_joins S.
Proof.
  unfold reads. induction ks as [| k ks IH]; intros S; simpl; [auto |].
  destruct (IH (touch k S)) as (A & B & C). destruct (touch_fields k S) as (A' & B' & C').
  rewrite A, B, C, A', B', C'. auto.
Qed.

Lemma reads_cache :
  forall ks S, exists added, hs_cache (reads ks S) = hs_cache S ++ added /\
    (forall k, In k added -> In k ks /\ ~ In k (hs_cache S)).
Proof.
  unfold reads. induction ks as [| k ks IH]; intros S; simpl.
  - exists []. rewrite app_nil_r. split; [reflexivity | intros k []].
  - destruct (IH (touch k S)) as (added & E & Ha). rewrite E.
    unfold touch. destruct (in_dec ckey_eq_dec k (hs_cache S)) as [Hin | Hn]; simpl.
    + exists added. split; [reflexivity |]. intros k' Hk'. destruct (Ha k' Hk') as [H1 H2].
      split; [right; exact H1 |]. unfold touch in H2. destruct (in_dec ckey_eq_dec k (hs_cache S)); [exact H2 | contradiction].
    + exists (k :: added). split; [rewrite <- app_assoc; reflexivity |].
      intros k' [<- | Hk']; [split; [left; reflexivity | exact Hn] |].
      destruct (Ha k' Hk') as [H1 H2]. split; [right; exact H1 |].
      intros Hin. apply H2. unfold touch. destruct (in_dec ckey_eq_dec k (hs_cache S)); [contradiction |].
      simpl. apply in_or_app. left. exact Hin.
Qed.

Lemma action_of_fields :
  forall S1 S2 k, hs_data S1 = hs_data S2 -> hs_conts S1 = hs_conts S2 -> hs_joins S1 = hs_joins S2 ->
    action_of S1 k = action_of S2 k.
Proof. intros S1 S2 [c | g | c] Ed Ec Ej; simpl; [rewrite Ed | rewrite Ec | rewrite Ej]; reflexivity. Qed.

(* An action that writes what the trie already holds at its key changes no
   lookup. *)
Lemma apply_neutral :
  forall t a,
    tlookup (action_key a) t = match a with AInsert _ l => Some l | ADelete _ => None end ->
    forall hk, tlookup hk (apply_action t a) = tlookup hk t.
Proof.
  intros t a Hself hk. destruct (hkey_eq_dec hk (action_key a)) as [E | N].
  - subst hk. rewrite apply_action_self. symmetry. exact Hself.
  - apply apply_action_other. intro E. apply N. symmetry. exact E.
Qed.

Lemma make_actions_neutral :
  forall acts t,
    (forall a, In a acts -> tlookup (action_key a) t = match a with AInsert _ l => Some l | ADelete _ => None end) ->
    forall hk, tlookup hk (make_actions t acts) = tlookup hk t.
Proof.
  induction acts as [| a rest IH]; intros t H hk; [reflexivity |].
  unfold make_actions. simpl. fold (make_actions (apply_action t a) rest).
  assert (Ha : forall hk', tlookup hk' (apply_action t a) = tlookup hk' t).
  { apply apply_neutral. apply H. left. reflexivity. }
  rewrite IH; [apply Ha |].
  intros a' Ha'. rewrite Ha. apply H. right. exact Ha'.
Qed.

(* H18 (FR-1): a read of a non-canonical key fails before any access. Reads
   of canonical keys change no stored list. They keep the cache duplicate
   free, canonical and read-through. The checkpoint succeeds before and
   after the reads, the two tries hold the same leaf at every history key,
   and so the two roots are equal. *)
Theorem canonical_reads_root_neutral :
  forall T S ks,
    (forall k, ck_canonical k = false -> read_key k S = None) /\
    (forall k, ck_canonical k = true -> read_key k S = Some (touch k S)) /\
    (key_injective -> NoDup (hs_cache S) -> cache_canonical S -> read_through T S ->
     Forall (fun k => ck_canonical k = true) ks ->
     let S' := reads ks S in
     hs_data S' = hs_data S /\ hs_conts S' = hs_conts S /\ hs_joins S' = hs_joins S /\
     NoDup (hs_cache S') /\ cache_canonical S' /\ read_through T S' /\
     exists T1 T2, checkpoint T (changes S) = Some T1 /\ checkpoint T (changes S') = Some T2 /\
       (forall hk, tlookup hk T2 = tlookup hk T1) /\
       (root_respects_lookup -> root_of T2 = root_of T1)).
Proof.
  intros T S ks.
  split; [intros k Hk; unfold read_key; rewrite Hk; reflexivity |].
  split; [intros k Hk; unfold read_key; rewrite Hk; reflexivity |].
  intros Hinj Hnd Hcan Hrt Hks S'.
  destruct (reads_fields ks S) as (Fd & Fc & Fj). fold S' in Fd, Fc, Fj.
  destruct (reads_cache ks S) as (added & Ec & Ha). fold S' in Ec.
  rewrite Forall_forall in Hks. unfold cache_canonical in Hcan. rewrite Forall_forall in Hcan.
  assert (Hnd' : NoDup (hs_cache S')).
  { subst S'. unfold reads. apply fold_nodup; [| exact Hnd]. intros S0 k H0. apply touch_nodup, H0. }
  assert (Hcan' : cache_canonical S').
  { unfold cache_canonical. rewrite Forall_forall. intros k Hk. rewrite Ec in Hk.
    apply in_app_or in Hk as [Hk | Hk]; [apply Hcan, Hk | apply Hks, (Ha k Hk)]. }
  assert (Hact : forall k, action_of S' k = action_of S k) by (intros k; apply action_of_fields; assumption).
  assert (Hrt' : read_through T S').
  { intros k Hk. unfold leaf_opt. rewrite Hact. apply Hrt. intros Hin. apply Hk. rewrite Ec.
    apply in_or_app. left. exact Hin. }
  split; [exact Fd |]. split; [exact Fc |]. split; [exact Fj |].
  split; [exact Hnd' |]. split; [exact Hcan' |]. split; [exact Hrt' |].
  assert (Hcan0 : cache_canonical S) by (unfold cache_canonical; rewrite Forall_forall; exact Hcan).
  exists (make_actions T (changes S)), (make_actions T (changes S')).
  split; [apply checkpoint_changes_some; assumption |].
  split; [apply checkpoint_changes_some; assumption |].
  assert (Hch : changes S' = changes S ++ map (action_of S) added).
  { unfold changes. rewrite Ec, map_app. f_equal; apply map_ext; intros k; apply Hact. }
  assert (Hlk : forall hk, tlookup hk (make_actions T (changes S')) = tlookup hk (make_actions T (changes S))).
  { intros hk. rewrite Hch. unfold make_actions at 1. rewrite fold_left_app.
    fold (make_actions T (changes S)). fold (make_actions (make_actions T (changes S)) (map (action_of S) added)).
    apply make_actions_neutral. intros a Hina. apply in_map_iff in Hina as (k & <- & Hk).
    destruct (Ha k Hk) as [Hkks Hkn].
    rewrite action_key_of. rewrite make_actions_preserves_lookup.
    - rewrite Hrt by exact Hkn. unfold leaf_opt. reflexivity.
    - intros a' Ha'. unfold changes in Ha'. apply in_map_iff in Ha' as (k' & <- & Hk').
      rewrite action_key_of. intros E. apply Hkn.
      replace k with k'; [exact Hk' |]. apply Hinj; [apply Hcan, Hk' | apply Hks, Hkks | exact E]. }
  split; [exact Hlk |].
  intros Hroot. apply Hroot. exact Hlk.
Qed.

(* ═══════════ Duplicate-key rejection of the checkpoint (H22) ══════════════ *)

Lemma nodup_map_eq :
  forall (A B : Type) (f : A -> B) l x y, NoDup (map f l) -> In x l -> In y l -> f x = f y -> x = y.
Proof.
  intros A B f l. induction l as [| z t IH]; intros x y Hnd Hx Hy E; [contradiction |].
  simpl in Hnd. inversion Hnd as [| fz ft Hn Ht]; subst.
  destruct Hx as [<- | Hx]; destruct Hy as [<- | Hy].
  - reflexivity.
  - exfalso. apply Hn. rewrite E. apply in_map, Hy.
  - exfalso. apply Hn. rewrite <- E. apply in_map, Hx.
  - apply IH; assumption.
Qed.

(* The order premises. chan_leb compares the hash bytes of two store
   identities (FR-2), and the byte order of Blake2b256Hash is a total order.
   So chan_leb is transitive, antisymmetric and total. The lemma
   order_premises_satisfiable shows the three premises for one order. *)
Definition leb_transitive : Prop :=
  forall x y z, chan_leb x y = true -> chan_leb y z = true -> chan_leb x z = true.

Definition leb_antisymmetric : Prop :=
  forall x y, chan_leb x y = true -> chan_leb y x = true -> x = y.

Definition leb_total : Prop :=
  forall x y, chan_leb x y = true \/ chan_leb y x = true.

Lemma nondecreasing_sorted :
  forall g, nondecreasing g = true <-> Sorted (fun x y => chan_leb x y = true) g.
Proof.
  induction g as [| x t IH]; [split; [intros _; constructor | reflexivity] |].
  destruct t as [| y t'].
  - split; [intros _; repeat constructor | reflexivity].
  - change (nondecreasing (x :: y :: t')) with (chan_leb x y && nondecreasing (y :: t')).
    rewrite andb_true_iff, IH. split.
    + intros [Hxy Hs]. constructor; [exact Hs | constructor; exact Hxy].
    + intros Hs. apply Sorted_inv in Hs as [Hs Hhd]. apply HdRel_inv in Hhd. split; assumption.
Qed.

Lemma canonical_strongly_sorted :
  leb_transitive -> forall g, canonical g = true -> StronglySorted (fun x y => chan_leb x y = true) g.
Proof.
  intros Htr g Hg. apply Sorted_StronglySorted; [intros x y z; apply Htr |].
  apply nondecreasing_sorted, Hg.
Qed.

(* Two strongly sorted permutations of one list are equal when the relation
   is antisymmetric. *)
Lemma sorted_perm_unique :
  forall (R : Chan -> Chan -> Prop), (forall x y, R x y -> R y x -> x = y) ->
    forall l1 l2, StronglySorted R l1 -> StronglySorted R l2 -> Permutation l1 l2 -> l1 = l2.
Proof.
  intros R Hanti l1. induction l1 as [| x t1 IH]; intros l2 S1 S2 P.
  - apply Permutation_nil in P. subst. reflexivity.
  - destruct l2 as [| y t2]; [apply Permutation_sym, Permutation_nil in P; discriminate |].
    apply StronglySorted_inv in S1 as [S1 F1]. apply StronglySorted_inv in S2 as [S2 F2].
    rewrite Forall_forall in F1, F2.
    assert (Exy : x = y).
    { assert (Hy : In y (x :: t1)) by (apply (Permutation_in _ (Permutation_sym P)); left; reflexivity).
      assert (Hx : In x (y :: t2)) by (apply (Permutation_in _ P); left; reflexivity).
      destruct Hy as [Ey | Hy]; [exact Ey |].
      destruct Hx as [Ex | Hx]; [symmetry; exact Ex |].
      exact (Hanti x y (F1 y Hy) (F2 x Hx)). }
    subst y. f_equal. apply IH; [exact S1 | exact S2 |]. apply Permutation_cons_inv in P. exact P.
Qed.

(* FR-1': a sorted list is the unique canonical member of its permutation
   class. So the canonical cache never holds two orders of one multiset of
   channels. *)
Lemma sorted_unique_canonical :
  leb_transitive -> leb_antisymmetric ->
  forall g g', canonical g = true -> Permutation g g' -> canonical g' = true -> g' = g.
Proof.
  intros Htr Hanti g g' Hg P Hg'. symmetry.
  apply (sorted_perm_unique (fun x y => chan_leb x y = true)); [exact Hanti | | | exact P];
    apply canonical_strongly_sorted; assumption.
Qed.

(* Every permutation class has a canonical member: the insertion sort of any
   member. hash_vec sorts the channel hashes in the same order. *)
Lemma isort_canonical :
  leb_total -> forall g, canonical (isort chan_leb g) = true /\ Permutation (isort chan_leb g) g.
Proof.
  intros Htot g. split; [| apply isort_perm].
  apply nondecreasing_sorted. apply isort_sorted. exact Htot.
Qed.

(* FR-1' example: a group that repeats a channel is canonical when it is
   sorted. The other orders of [x; x; y] are not canonical. *)
Lemma repeated_group_canonical_when_sorted :
  forall x y, chan_leb x x = true -> chan_leb x y = true -> chan_leb y x = false ->
    canonical [x; x] = true /\ canonical [x; x; y] = true /\
    canonical [x; y; x] = false /\ canonical [y; x; x] = false.
Proof.
  intros x y Hxx Hxy Hyx. unfold canonical. cbn [nondecreasing]. rewrite Hxx, Hxy, Hyx.
  repeat split; reflexivity.
Qed.

(* Collision freedom up to order: hash_from_vec hashes the sorted channel
   hashes of a group, and hash_from_hashes keeps a repeated hash. So two
   groups share a history key only when they are permutations of each
   other. The kind prefixes separate the three kinds of keys. *)
Definition hkey_up_to_order : Prop :=
  forall k1 k2, hkey_of k1 = hkey_of k2 ->
    match k1, k2 with
    | CData c1, CData c2 => c1 = c2
    | CCont g1, CCont g2 => Permutation g1 g2
    | CJoins c1, CJoins c2 => c1 = c2
    | _, _ => False
    end.

(* FR-1': with collision freedom up to order, no two canonical cache keys
   share a history key, also when a group repeats a channel. This gives the
   premise key_injective of H15 and H18. *)
Lemma sorted_keys_injective :
  leb_transitive -> leb_antisymmetric -> hkey_up_to_order -> key_injective.
Proof.
  intros Htr Hanti Hup k1 k2 C1 C2 E. specialize (Hup k1 k2 E).
  destruct k1 as [c1 | g1 | c1]; destruct k2 as [c2 | g2 | c2]; try contradiction;
    [subst c2; reflexivity | | subst c2; reflexivity].
  simpl in C1, C2. f_equal. symmetry. exact (sorted_unique_canonical Htr Hanti g1 g2 C1 Hup C2).
Qed.

(* H22: the checkpoint refuses two actions on one history key, as
   RadixHistory::process does (radix_history.rs:100-105), and it applies the
   actions otherwise. hash_from_vec sorts the channel hashes, so two cached
   continuation keys that are permutations of one group make the checkpoint
   fail, and so does a read of a permuted key next to its cached group
   (review F-ORDER-1 to F-ORDER-4). FR-1' makes a group canonical when it
   is non-decreasing, and a group can repeat a channel. Under an
   antisymmetric order a permutation class has exactly one canonical member,
   its sorted list. So FR-1' excludes this case: with collision freedom up
   to order, the canonical cache keys have distinct history keys. *)
Theorem checkpoint_rejects_duplicate_key_actions :
  (forall t acts, checkpoint t acts = None <-> ~ NoDup (map action_key acts)) /\
  (forall t acts, NoDup (map action_key acts) -> checkpoint t acts = Some (make_actions t acts)) /\
  (forall t acts1 a1 acts2 a2 acts3, action_key a1 = action_key a2 ->
     checkpoint t (acts1 ++ a1 :: acts2 ++ a2 :: acts3) = None) /\
  (forall S g1 g2 t,
     (forall g g', Permutation g g' -> key_cont g = key_cont g') ->
     Permutation g1 g2 -> g1 <> g2 ->
     In (CCont g1) (hs_cache S) -> In (CCont g2) (hs_cache S) ->
     checkpoint t (changes S) = None) /\
  (forall S g1 g2 t,
     (forall g g', Permutation g g' -> key_cont g = key_cont g') ->
     Permutation g1 g2 -> g1 <> g2 -> In (CCont g1) (hs_cache S) ->
     checkpoint t (changes (touch (CCont g2) S)) = None) /\
  (forall g1 g2,
     leb_transitive -> leb_antisymmetric ->
     Permutation g1 g2 -> canonical g1 = true -> canonical g2 = true -> g1 = g2) /\
  (leb_total -> forall g, canonical (isort chan_leb g) = true /\ Permutation (isort chan_leb g) g) /\
  (forall x y, chan_leb x x = true -> chan_leb x y = true -> chan_leb y x = false ->
     canonical [x; x] = true /\ canonical [x; x; y] = true /\
     canonical [x; y; x] = false /\ canonical [y; x; x] = false) /\
  (leb_transitive -> leb_antisymmetric -> hkey_up_to_order -> key_injective).
Proof.
  assert (Hdup : forall S g1 g2 t,
            (forall g g', Permutation g g' -> key_cont g = key_cont g') ->
            Permutation g1 g2 -> g1 <> g2 ->
            In (CCont g1) (hs_cache S) -> In (CCont g2) (hs_cache S) ->
            checkpoint t (changes S) = None).
  { intros S g1 g2 t Hperm P N H1 H2. apply (proj1 (checkpoint_spec t (changes S))).
    rewrite changes_keys. intros Hnd.
    assert (E : CCont g1 = CCont g2).
    { apply (nodup_map_eq _ _ hkey_of (hs_cache S)); [exact Hnd | exact H1 | exact H2 |].
      simpl. apply Hperm, P. }
    injection E as E. contradiction. }
  split; [intros t acts; exact (proj1 (checkpoint_spec t acts)) |].
  split; [intros t acts; exact (proj2 (checkpoint_spec t acts)) |].
  split.
  { intros t acts1 a1 acts2 a2 acts3 E. apply (proj1 (checkpoint_spec t _)). intros Hnd.
    rewrite map_app in Hnd. cbn [map] in Hnd. rewrite map_app in Hnd. cbn [map] in Hnd.
    apply NoDup_remove_2 in Hnd. apply Hnd. rewrite E. apply in_or_app. right.
    apply in_or_app. right. left. reflexivity. }
  split; [exact Hdup |].
  split.
  { intros S g1 g2 t Hperm P N H1. apply (Hdup (touch (CCont g2) S) g1 g2 t Hperm P N).
    - unfold touch. destruct (in_dec ckey_eq_dec (CCont g2) (hs_cache S)); [exact H1 |].
      simpl. apply in_or_app. left. exact H1.
    - apply touch_in. }
  split.
  { intros g1 g2 Htr Hanti P C1 C2. symmetry. exact (sorted_unique_canonical Htr Hanti g1 g2 C1 P C2). }
  split; [exact isort_canonical |].
  split; [exact repeated_group_canonical_when_sorted |].
  exact sorted_keys_injective.
Qed.

(* ══════════ P7: the book of a root evaluation (design v3 VR-1, VR-2, DG-1) ══════════ *)

(* A committed transaction of a space job: its lock keys (SpaceEnv::begin) and the channels
   whose records, waiting continuations or join entries it stages (Prepared::commit). *)
Record Txn := mkTxn { txn_locks : list Chan; txn_staged : list Chan }.

(* Readiness review S2 and FR-4: the lock keys of a transaction hold every channel that it
   stages. *)
Definition covers (t : Txn) : Prop := forall c, In c (txn_staged t) -> In c (txn_locks t).

(* A space-scoped id with an ephemeral KIND (spaces/ephemeral.rs, ephemeral_space). *)
Variable ephemeral : Chan -> bool.

(* SpaceEnv::begin records the volatile lock keys of each transaction of the root evaluation
   (VolatileRegistry::record_locked), and the sweep takes them all (take_channels). *)
Definition book (ts : list Txn) : list Chan :=
  flat_map (fun t => filter ephemeral (txn_locks t)) ts.

Lemma book_holds_staged :
  forall ts t c,
    Forall covers ts -> In t ts -> In c (txn_staged t) -> ephemeral c = true ->
    In c (book ts).
Proof.
  intros ts t c Hcov Ht Hc He. unfold book. apply in_flat_map. exists t.
  split; [exact Ht |]. apply filter_In. split; [| exact He].
  rewrite Forall_forall in Hcov. exact (Hcov t Ht c Hc).
Qed.

(* P7-1 sweep_complete: when the lock keys of every transaction of the evaluation hold its
   staged channels, the sweep of the book leaves no record and no join entry on any volatile
   channel that a transaction staged, and no waiting continuation on any group with a channel
   of the book. *)
Theorem sweep_complete :
  forall ts T installed S,
    Forall covers ts -> lockstep S ->
    validate T installed (nodup chan_eq_dec (book ts))
      (groups S (nodup chan_eq_dec (book ts))) = true ->
    exists S',
      sweep_volatile T installed S (book ts) = Some S' /\
      (forall t c, In t ts -> In c (txn_staged t) -> ephemeral c = true ->
         hs_data S' c = [] /\ hs_joins S' c = []) /\
      (forall g c, In c g -> In c (book ts) -> hs_conts S' g = []).
Proof.
  intros ts T installed S Hcov Hlock Hval.
  pose proof (sweep_exact T installed S (book ts)) as Hexact. cbv zeta in Hexact.
  destruct Hexact as [_ [Ht _]].
  destruct (Ht Hval) as [S' [Hsw [Hdata [Hconts [_ [Hconts_out _]]]]]].
  exists S'. split; [exact Hsw |]. split.
  - intros t c Ht' Hc He. apply Hdata. exact (book_holds_staged ts t c Hcov Ht' Hc He).
  - intros g c Hcg Hcb.
    destruct (in_dec lc_eq_dec g (groups S (nodup chan_eq_dec (book ts)))) as [Hin | Hout].
    + exact (Hconts g Hin).
    + rewrite (Hconts_out g Hout).
      destruct (hs_conts S g) as [| w l] eqn:E; [reflexivity |].
      exfalso. apply Hout.
      apply (in_groups_joins S (nodup chan_eq_dec (book ts)) c g).
      * apply (proj2 (nodup_In chan_eq_dec (book ts) c)). exact Hcb.
      * apply (proj2 (Hlock c g)). split; [exact Hcg |].
        rewrite E. intro Habs. discriminate Habs.
Qed.

(* DG-1: the ephemeral spaces that the root evaluation created. The creation job records a
   space after its commit, inside the job (VolatileRegistry::record_created, decision 83). *)
Variable Space : Type.
Variable space_eq_dec : forall x y : Space, {x = y} + {x <> y}.

Inductive Liveness := Live | Dangling.

(* ReifiedRuntime::check_live_space and check_live: an ephemeral space that the book does
   not hold fails before any job. *)
Definition check_live (created : list Space) (s : Space) : Liveness :=
  if in_dec space_eq_dec s created then Live else Dangling.

(* P7-2 dead_ephemeral_dangling: I-19 derives a space id from the message that created the
   space, so the spaces of two deploys are disjoint. A space of another deploy is then never
   live in this one, and the check is live exactly for the spaces that this evaluation
   created. *)
Theorem dead_ephemeral_dangling :
  forall (created other : list Space) s,
    (forall x, In x created -> ~ In x other) ->
    In s other ->
    check_live created s = Dangling /\
    (forall s', check_live created s' = Live <-> In s' created).
Proof.
  intros created other s Hdisj Hs. split.
  - unfold check_live. destruct (in_dec space_eq_dec s created) as [Hin | _]; [| reflexivity].
    exfalso. exact (Hdisj s Hin Hs).
  - intros s'. unfold check_live.
    destruct (in_dec space_eq_dec s' created) as [Hin | Hout]; split; intro H.
    + exact Hin.
    + reflexivity.
    + discriminate H.
    + contradiction.
Qed.

End Sweep.

(* ═══════════ The premises of H15, H18 and H22 are satisfiable ════════════ *)

(* History keys that are the cache keys themselves satisfy key_injective for
   every channel type and order. An empty store over the empty trie meets
   the other premises of H18. So H15 and H18 do not hold vacuously. *)
Lemma key_premises_satisfiable :
  forall (Chan Datum WK Bytes : Type) (chan_leb : Chan -> Chan -> bool)
    (ckey_eq : forall x y : CKey Chan, {x = y} + {x <> y})
    (ser_datum : Datum -> Bytes) (ser_wk : WK -> Bytes) (ser_join : list Chan -> Bytes)
    (bytes_leb : Bytes -> Bytes -> bool),
    let S := mkHS Chan Datum WK (fun _ => []) (fun _ => []) (fun _ => []) [] in
    key_injective Chan (CKey Chan) (@CData Chan) (@CCont Chan) (@CJoins Chan) chan_leb /\
    NoDup (hs_cache Chan Datum WK S) /\
    cache_canonical Chan Datum WK chan_leb S /\
    read_through Chan Datum WK (CKey Chan) Bytes ckey_eq (@CData Chan) (@CCont Chan) (@CJoins Chan)
      ser_datum ser_wk ser_join bytes_leb [] S.
Proof.
  intros Chan Datum WK Bytes chan_leb ckey_eq ser_datum ser_wk ser_join bytes_leb S.
  split; [intros k1 k2 _ _ E; destruct k1, k2; exact E |].
  split; [constructor |]. split; [constructor |].
  intros [c | g | c] _; reflexivity.
Qed.

(* The order premises of H22 hold for the order of the natural numbers. A
   history key that sorts the group, as hash_from_vec does, is the same for
   every order of one group, and it is collision free up to order. With
   sorted_keys_injective this key function meets key_injective. So the
   premise of H15 and H18 also holds for a key function that, as in the
   code, gives every order of one group the same history key. *)
Lemma order_premises_satisfiable :
  let sort_key := fun g : list nat => @CCont nat (isort Nat.leb g) in
  leb_transitive nat Nat.leb /\ leb_antisymmetric nat Nat.leb /\ leb_total nat Nat.leb /\
  (forall g g', Permutation g g' -> sort_key g = sort_key g') /\
  hkey_up_to_order nat (CKey nat) (@CData nat) sort_key (@CJoins nat) /\
  key_injective nat (CKey nat) (@CData nat) sort_key (@CJoins nat) Nat.leb.
Proof.
  intros sort_key.
  assert (Htr : leb_transitive nat Nat.leb).
  { intros x y z Hxy Hyz. apply Nat.leb_le in Hxy, Hyz. apply Nat.leb_le. lia. }
  assert (Hanti : leb_antisymmetric nat Nat.leb).
  { intros x y Hxy Hyx. apply Nat.leb_le in Hxy, Hyx. lia. }
  assert (Htot : leb_total nat Nat.leb).
  { intros x y. destruct (Nat.le_ge_cases x y) as [H | H]; [left | right]; apply Nat.leb_le, H. }
  assert (Hup : hkey_up_to_order nat (CKey nat) (@CData nat) sort_key (@CJoins nat)).
  { intros [c1 | g1 | c1] [c2 | g2 | c2] E; unfold sort_key in E; simpl in E;
      try discriminate; injection E as E; try exact E.
    apply (Permutation_trans (Permutation_sym (isort_perm nat Nat.leb g1))).
    rewrite E. apply isort_perm. }
  split; [exact Htr |]. split; [exact Hanti |]. split; [exact Htot |].
  split.
  { intros g g' P. unfold sort_key. f_equal.
    destruct (isort_canonical nat Nat.leb Htot g) as [Cg Pg].
    destruct (isort_canonical nat Nat.leb Htot g') as [Cg' Pg'].
    apply (sorted_unique_canonical nat Nat.leb Htr Hanti (isort Nat.leb g') (isort Nat.leb g) Cg').
    - apply (Permutation_trans Pg'). apply (Permutation_trans (Permutation_sym P)).
      apply Permutation_sym, Pg.
    - exact Cg. }
  split; [exact Hup |].
  exact (sorted_keys_injective nat (CKey nat) (@CData nat) sort_key (@CJoins nat) Nat.leb Htr Hanti Hup).
Qed.

(* The order premises of H22 are needed. Without antisymmetry, two orders of
   one group can both be canonical: under the comparison that holds for
   every pair, [true; false] and [false; true] are both non-decreasing.
   Without totality, the insertion sort can give a group that is not
   canonical. *)
Lemma order_premises_needed :
  (canonical bool (fun _ _ => true) [true; false] = true /\
   canonical bool (fun _ _ => true) [false; true] = true /\
   Permutation [true; false] [false; true] /\ [true; false] <> [false; true]) /\
  canonical bool (fun _ _ => false) (isort (fun _ _ => false) [true; false]) = false.
Proof.
  split; [| reflexivity].
  split; [reflexivity |]. split; [reflexivity |]. split; [apply perm_swap | discriminate].
Qed.

(* ═══════════ Headline trust base: every headline is axiom-free ═══════════ *)

Print Assumptions sweep_exact.
Print Assumptions delete_absent_identity.
Print Assumptions sweep_root_neutral.
Print Assumptions leaf_count_is_length.
Print Assumptions canonical_reads_root_neutral.
Print Assumptions checkpoint_rejects_duplicate_key_actions.
Print Assumptions sweep_complete.
Print Assumptions dead_ephemeral_dangling.
