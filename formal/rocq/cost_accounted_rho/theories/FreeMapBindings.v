(* D-D1a (epic 8946, Phase D; decision record DR-103, part 1): a free-variable
   binding charges B-tree bounds instead of whole-map walks.

   The matcher binds each free variable in a free map, a standard BTreeMap
   from i32 levels to Par bindings. Before DR-103, each binding charged a walk
   of the whole map and the backing of the whole tree. The remainder handler
   runs ten times for each bound variable (once for each field of a Par), so
   binding v variables of payload s charged 5 * s * v * (v - 1) for the walks
   alone (legacy_remainder_inspection_quadratic).

   DR-103 charges what one binding does:
   - One search reads one root-to-leaf path: at most 11 comparisons on each
     level (OrderedLookupBound.search_within_size_bound), and each comparison
     reads two keys of 4 bytes (free_map_search_covered).
   - One insert moves or writes, on each level that it touches, at most 12
     key-value slots, 12 edges, 12 child parent links, 4 node lengths and one
     parent pointer (insert_level_moves_bounded). The model of a level follows
     alloc::collections::btree::node: splitpoint, insert_fit, slice_insert,
     split_leaf_data, split, from_new_internal, push_internal_level, push and
     correct_childrens_parent_links. With an i32 key and a 296-byte Par, a
     level costs at most 3,832 bytes (level_charge_value), and the internal
     split at edge 0 costs exactly that (internal_split_attains_charge).
   - An insert path is a sequence of splits, one per level, then one last
     level: an insert into a node with room, a new root, the first leaf of an
     empty map, or the replacement of the value at an existing key. The last
     level leaves room for the writes to the map's length and root, so a path
     that touches k levels costs at most k * 3,832 bytes
     (insert_path_within_charge). The path touches distinct levels of the
     tree after the insert, so the height bound of the size after the insert
     bounds k (insert_within_size_charge). The Rust charge uses the height
     bound of n + 1, which also covers a replacement (insert_within_rust_charge).
   - The nodes that the inserts allocate are the growth of the tree backing.
     It telescopes from an empty map or from a charged clone
     (clone_growth_covers_nodes, via IncrementalTreeBacking).
   - The charge depends on the number of bindings only, not on their payloads
     (remainder_charge_independent_of_bindings).

   The model counts the bytes that the insert moves or writes in tree nodes
   and in the map's header. Moves of the inserted value between stack frames
   are outside the model. The drop of a replaced value is outside it too: the
   matcher charges the cleanup of a value with the clone that creates it
   (reserve_copy_and_cleanup).

   Negative controls: legacy_remainder_inspection_quadratic and
   legacy_remainder_example (280,000 for v = 8 and s = 1,000);
   kv_area_undercounts_internal_split (11 slots alone undercount an internal
   split); eleven_slot_level_undercounts_leaf_split (a bound of 11 slots
   undercounts a leaf split that writes its new entry);
   twelve_byte_header_undercounts_internal_split (a bound that counts one
   12-byte node header instead of every length and parent-pointer write
   undercounts the internal split at edge 0).

   No axiom, parameter or admission is used. Section variables are
   universally quantified when the sections close.

   Rust correspondence: shared/src/rust/collection_backing.rs
   (tree_insert_level_bytes, tree_insert_moves, tree_growth,
   tree_search_bound), rholang/src/rust/interpreter/matcher/spatial_matcher.rs
   (MatcherWork::reserve_free_map_search and reserve_free_map_insert), and the
   call sites in list_match.rs and match.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Bool Lia NArith.
From CostAccountedRho Require OrderedLookupBound IncrementalTreeBacking.
Import ListNotations.

(* Searches. *)

Definition search_operations (entries : nat) : nat :=
  11 * OrderedLookupBound.height_bound entries.

Definition search_scanned (entries : nat) : nat := 8 * search_operations entries.

Theorem free_map_search_covered : forall t target fuel h,
  OrderedLookupBound.wf true h t ->
  OrderedLookupBound.search_comparisons fuel t target
    <= search_operations (OrderedLookupBound.size t)
  /\ 8 * OrderedLookupBound.search_comparisons fuel t target
    <= search_scanned (OrderedLookupBound.size t).
Proof.
  intros t target fuel h well.
  pose proof (OrderedLookupBound.search_within_size_bound t target fuel h well).
  unfold search_scanned, search_operations. lia.
Qed.

(* One level of an insert. A std BTreeMap node holds at most 11 keys and 12
   edges. A key-value slot is moved as one key and one value. *)

Record level_moves := {
  slots : nat;
  edges : nat;
  links : nat;
  lengths : nat;
  parents : nat
}.

(* [insert_fit] at edge index [e] of a node with [len] keys. [slice_insert]
   shifts the slots after [e] and writes the new slot. An internal node also
   shifts the edges after [e + 1], writes the new edge and corrects the
   parent links of the children from [e + 1] to [len + 1]. The node's length
   is written once. *)
Definition fit_level (internal : bool) (len e : nat) : level_moves :=
  {| slots := len - e + 1;
     edges := if internal then len - e + 1 else 0;
     links := if internal then len + 1 - e else 0;
     lengths := 1;
     parents := 0 |}.

(* [splitpoint]: for an insert at edge index [e] of a full node, the middle
   key that moves up, whether the new entry goes to the left half, and its
   edge index in that half. *)
Definition split_middle (e : nat) : nat :=
  if e <? 5 then 4 else if e <=? 6 then 5 else 6.

Definition split_left (e : nat) : bool := e <=? 5.

Definition split_index (e : nat) : nat :=
  if e <=? 5 then e else if e =? 6 then 0 else e - 7.

Definition half_length (e : nat) : nat :=
  if split_left e then split_middle e else 10 - split_middle e.

(* [split] of a full node at the middle key, then [insert_fit] into the
   receiving half. A new node is initialized with its parent pointer and its
   length. [split_leaf_data] reads the middle slot, moves the slots after it
   into the new node, and writes the lengths of both nodes. An internal split
   also moves the edges after the middle, and [from_new_internal] corrects the
   parent links of every child of the new node. *)
Definition split_level (internal : bool) (e : nat) : level_moves :=
  let middle := split_middle e in
  let fit := fit_level internal (half_length e) (split_index e) in
  {| slots := (11 - middle) + slots fit;
     edges := (if internal then 11 - middle else 0) + edges fit;
     links := (if internal then 11 - middle else 0) + links fit;
     lengths := 3 + lengths fit;
     parents := 1 |}.

(* The level that an insert at edge index [e] of a node with [len] keys
   touches. *)
Definition level (internal : bool) (len e : nat) : level_moves :=
  if len <? 11 then fit_level internal len e else split_level internal e.

(* [push_internal_level] then [push]: a new root is initialized with its
   parent pointer and length, its first edge is the old root, and that child
   gets its parent link. [push] writes the length, the middle slot, the
   second edge and its parent link. *)
Definition new_root_level : level_moves :=
  {| slots := 1; edges := 2; links := 2; lengths := 2; parents := 1 |}.

(* The first insert into an empty map: [new_leaf] initializes a leaf with its
   parent pointer and length, and [push] writes the length and the entry. *)
Definition first_leaf_level : level_moves :=
  {| slots := 1; edges := 0; links := 0; lengths := 2; parents := 1 |}.

(* An insert at an existing key: the old value moves out and the new value
   moves in. *)
Definition replace_level : level_moves :=
  {| slots := 2; edges := 0; links := 0; lengths := 0; parents := 0 |}.

Definition every_level_fits : bool :=
  forallb
    (fun internal =>
       forallb
         (fun len =>
            forallb
              (fun e =>
                 let moves := level internal len e in
                 (slots moves <=? 12) && (edges moves <=? 12) && (links moves <=? 12)
                 && (lengths moves <=? 4) && (parents moves <=? 1))
              (seq 0 (len + 1)))
         (seq 0 12))
    [false; true].

Lemma every_level_fits_true : every_level_fits = true.
Proof. vm_compute. reflexivity. Qed.

Lemma in_booleans : forall b : bool, In b [false; true].
Proof. intros []; simpl; auto. Qed.

Theorem insert_level_moves_bounded : forall internal len e,
  len <= 11 -> e <= len ->
  slots (level internal len e) <= 12
  /\ edges (level internal len e) <= 12
  /\ links (level internal len e) <= 12
  /\ lengths (level internal len e) <= 4
  /\ parents (level internal len e) <= 1.
Proof.
  intros internal len e Hlen He.
  pose proof every_level_fits_true as Hall. unfold every_level_fits in Hall.
  rewrite forallb_forall in Hall. specialize (Hall internal (in_booleans internal)).
  rewrite forallb_forall in Hall. specialize (Hall len ltac:(apply in_seq; lia)).
  rewrite forallb_forall in Hall. specialize (Hall e ltac:(apply in_seq; lia)).
  cbv zeta in Hall.
  repeat rewrite andb_true_iff in Hall.
  destruct Hall as [[[[Hslots Hedges] Hlinks] Hlengths] Hparents].
  apply Nat.leb_le in Hslots, Hedges, Hlinks, Hlengths, Hparents.
  repeat split; assumption.
Qed.

(* Bytes on a 64-bit target: a slot holds an i32 key and a 296-byte Par, an
   edge is a pointer, a parent link is a pointer and a u16 index, a length is
   a u16, and a parent pointer is a pointer. The map's header holds its length
   (a usize) and its root (a node pointer and a height). *)
Definition slot_bytes : nat := 300.
Definition edge_bytes : nat := 8.
Definition link_bytes : nat := 10.
Definition length_bytes : nat := 2.
Definition parent_bytes : nat := 8.
Definition map_length_bytes : nat := 8.
Definition map_root_bytes : nat := 16.

Definition level_work (moves : level_moves) : nat :=
  slots moves * slot_bytes + edges moves * edge_bytes + links moves * link_bytes
  + lengths moves * length_bytes + parents moves * parent_bytes.

Definition level_charge : nat :=
  12 * slot_bytes + 12 * edge_bytes + 12 * link_bytes + 4 * length_bytes + parent_bytes.

Example level_charge_value : level_charge = 3832.
Proof. reflexivity. Qed.

Theorem insert_level_work_bounded : forall internal len e,
  len <= 11 -> e <= len -> level_work (level internal len e) <= level_charge.
Proof.
  intros internal len e Hlen He.
  destruct (insert_level_moves_bounded internal len e Hlen He)
    as [Hslots [Hedges [Hlinks [Hlengths Hparents]]]].
  unfold level_work, level_charge, slot_bytes, edge_bytes, link_bytes, length_bytes,
    parent_bytes.
  lia.
Qed.

(* The bound is tight: an internal split at edge 0 moves 12 slots, 12 edges
   and 12 links, and writes 4 lengths and one parent pointer. *)
Example internal_split_attains_charge : level_work (level true 11 0) = level_charge.
Proof. vm_compute. reflexivity. Qed.

(* An insert path: one split per level from the leaf up, then one last level,
   and the writes to the map's header. *)
Inductive last_level :=
| StopsAt (internal : bool) (len e : nat)
| NewRoot
| FirstLeaf
| Replaces.

Definition valid_last (last : last_level) : Prop :=
  match last with
  | StopsAt _ len e => len <= 10 /\ e <= len
  | NewRoot | FirstLeaf | Replaces => True
  end.

Definition last_moves (last : last_level) : level_moves :=
  match last with
  | StopsAt internal len e => fit_level internal len e
  | NewRoot => new_root_level
  | FirstLeaf => first_leaf_level
  | Replaces => replace_level
  end.

(* Every insert of a new key writes the map's length. An insert that changes
   the root also writes the map's root. A replacement writes neither. *)
Definition map_writes (last : last_level) : nat :=
  match last with
  | StopsAt _ _ _ => map_length_bytes
  | NewRoot | FirstLeaf => map_length_bytes + map_root_bytes
  | Replaces => 0
  end.

Definition split_work (splits : list (bool * nat)) : nat :=
  fold_right (fun step total => level_work (split_level (fst step) (snd step)) + total) 0 splits.

Definition insert_path_work (splits : list (bool * nat)) (last : last_level) : nat :=
  split_work splits + (level_work (last_moves last) + map_writes last).

Theorem split_level_within_charge : forall internal e,
  e <= 11 -> level_work (split_level internal e) <= level_charge.
Proof.
  intros internal e He.
  pose proof (insert_level_work_bounded internal 11 e ltac:(lia) He) as Hlevel.
  exact Hlevel.
Qed.

Definition every_fit_leaves_room : bool :=
  forallb
    (fun internal =>
       forallb
         (fun len =>
            forallb
              (fun e => level_work (fit_level internal len e) + map_length_bytes <=? level_charge)
              (seq 0 (len + 1)))
         (seq 0 11))
    [false; true].

Lemma every_fit_leaves_room_true : every_fit_leaves_room = true.
Proof. vm_compute. reflexivity. Qed.

Theorem last_level_within_charge : forall last,
  valid_last last -> level_work (last_moves last) + map_writes last <= level_charge.
Proof.
  intros [internal len e | | |] Hvalid; cbn [last_moves map_writes].
  - destruct Hvalid as [Hlen He].
    pose proof every_fit_leaves_room_true as Hall. unfold every_fit_leaves_room in Hall.
    rewrite forallb_forall in Hall. specialize (Hall internal (in_booleans internal)).
    rewrite forallb_forall in Hall. specialize (Hall len ltac:(apply in_seq; lia)).
    rewrite forallb_forall in Hall. specialize (Hall e ltac:(apply in_seq; lia)).
    apply Nat.leb_le in Hall. exact Hall.
  - vm_compute. lia.
  - vm_compute. lia.
  - vm_compute. lia.
Qed.

Lemma split_work_within_charge : forall splits,
  Forall (fun step => snd step <= 11) splits ->
  split_work splits <= length splits * level_charge.
Proof.
  induction splits as [| [internal e] rest IH]; intros Hvalid.
  - cbn. lia.
  - inversion Hvalid as [| ? ? He Hrest]; subst. cbn [snd] in He.
    pose proof (split_level_within_charge internal e He) as Hlevel.
    specialize (IH Hrest).
    change (split_work ((internal, e) :: rest))
      with (level_work (split_level internal e) + split_work rest).
    cbn [length]. rewrite Nat.mul_succ_l. lia.
Qed.

Theorem insert_path_within_charge : forall splits last height,
  Forall (fun step => snd step <= 11) splits -> valid_last last ->
  length splits + 1 <= height ->
  insert_path_work splits last <= height * level_charge.
Proof.
  intros splits last height Hsplits Hlast Hheight.
  pose proof (split_work_within_charge splits Hsplits) as Hwork.
  pose proof (last_level_within_charge last Hlast) as Hfinal.
  pose proof (Nat.mul_le_mono_r (length splits + 1) height level_charge Hheight) as Hscale.
  rewrite Nat.mul_add_distr_r in Hscale.
  unfold insert_path_work. lia.
Qed.

(* The path touches distinct levels of the tree after the insert. That tree
   is a well-formed B-tree, so the height bound of its size bounds the
   charge (OrderedLookupBound.btree_height_bound). *)
Theorem insert_within_size_charge : forall splits last h t,
  OrderedLookupBound.wf true h t ->
  Forall (fun step => snd step <= 11) splits -> valid_last last ->
  length splits + 1 <= h ->
  insert_path_work splits last
    <= OrderedLookupBound.height_bound (OrderedLookupBound.size t) * level_charge.
Proof.
  intros splits last h t well Hsplits Hlast Hlevels.
  pose proof (OrderedLookupBound.btree_height_bound h t well) as Hheight.
  apply insert_path_within_charge; [exact Hsplits | exact Hlast | lia].
Qed.

(* The Rust charge uses the height bound of n + 1, the size after an insert
   of a new key into a map with n entries. A replacement leaves n entries,
   and the height bound is monotone (OrderedLookupBound.height_bound_monotone). *)
Theorem insert_within_rust_charge : forall splits last h t n,
  OrderedLookupBound.wf true h t -> OrderedLookupBound.size t <= n + 1 ->
  Forall (fun step => snd step <= 11) splits -> valid_last last ->
  length splits + 1 <= h ->
  insert_path_work splits last <= OrderedLookupBound.height_bound (n + 1) * level_charge.
Proof.
  intros splits last h t n well Hsize Hsplits Hlast Hlevels.
  pose proof (insert_within_size_charge splits last h t well Hsplits Hlast Hlevels) as Hcharge.
  pose proof (OrderedLookupBound.height_bound_monotone (OrderedLookupBound.size t) (n + 1) Hsize)
    as Hmonotone.
  pose proof (Nat.mul_le_mono_r _ _ level_charge Hmonotone) as Hscale.
  lia.
Qed.

(* Node growth from a charged clone. The clone of a map with [base] entries
   pays the backing of that map, and each later insert pays the growth by
   one entry. *)

Theorem clone_growth_covers_nodes : forall node_bytes base k,
  IncrementalTreeBacking.tree_bytes node_bytes base
  + IncrementalTreeBacking.batch_total
      (IncrementalTreeBacking.tree_bytes node_bytes) base (repeat 1 k)
  = IncrementalTreeBacking.tree_bytes node_bytes (base + k).
Proof.
  intros node_bytes base k.
  rewrite (IncrementalTreeBacking.incremental_charges_telescope
             (IncrementalTreeBacking.tree_bytes node_bytes)
             (IncrementalTreeBacking.tree_bytes_monotone node_bytes)
             (repeat 1 k) base).
  rewrite IncrementalTreeBacking.sum_repeat_one.
  pose proof (IncrementalTreeBacking.tree_bytes_monotone node_bytes base (base + k)
                ltac:(lia)).
  lia.
Qed.

(* The charge of one binding: one search, then one insert. It is a function of
   the size of the free map alone. *)
Definition binding_scanned (entries : nat) : nat :=
  search_scanned entries + search_scanned entries
  + OrderedLookupBound.height_bound (entries + 1) * level_charge.

Theorem remainder_charge_independent_of_bindings : forall (first second : list nat),
  length first = length second ->
  binding_scanned (length first) = binding_scanned (length second).
Proof. intros first second Hlength. rewrite Hlength. reflexivity. Qed.

(* Negative controls. *)

(* The legacy walk: the j-th bound variable walked the j earlier bindings of
   payload s, once for each of the ten fields. *)
Fixpoint legacy_walks (s v : nat) : nat :=
  match v with
  | 0 => 0
  | S j => legacy_walks s j + 10 * j * s
  end.

Lemma legacy_walks_closed_form : forall s v, legacy_walks s v = 5 * s * v * (v - 1).
Proof.
  intros s v. induction v as [| j IH].
  - cbn [legacy_walks]. nia.
  - cbn [legacy_walks]. rewrite IH. destruct j as [| j].
    + nia.
    + replace (S j - 1) with j by lia. replace (S (S j) - 1) with (S j) by lia. nia.
Qed.

Theorem legacy_remainder_inspection_quadratic : forall s v,
  5 * s * v * (v - 1) <= legacy_walks s v.
Proof. intros s v. rewrite legacy_walks_closed_form. lia. Qed.

Example legacy_remainder_example : N.of_nat (legacy_walks 1000 8) = 280000%N.
Proof. vm_compute. reflexivity. Qed.

(* A bound of 11 key-value slots alone does not cover an internal split. *)
Theorem kv_area_undercounts_internal_split :
  11 * slot_bytes < level_work (level true 11 6).
Proof. vm_compute. lia. Qed.

(* A bound of 11 slots with every other write still undercounts a leaf split
   at edge 0, which moves 11 slots and writes the new entry. *)
Theorem eleven_slot_level_undercounts_leaf_split :
  11 * slot_bytes + 12 * edge_bytes + 12 * link_bytes + 4 * length_bytes + parent_bytes
  < level_work (level false 11 0).
Proof. vm_compute. lia. Qed.

(* A bound that counts one 12-byte node header (a parent pointer, a parent
   index and a length) instead of every length and parent-pointer write
   undercounts the internal split at edge 0. *)
Theorem twelve_byte_header_undercounts_internal_split :
  12 * slot_bytes + 12 * edge_bytes + 12 * link_bytes + 12 < level_work (level true 11 0).
Proof. vm_compute. lia. Qed.

Module StringMapInsertion.

Definition weighted_level_work key value word index (moves : level_moves) :=
  slots moves * (key + value) + edges moves * word + links moves * (word + index)
  + lengths moves * index + parents moves * word.

Definition weighted_level_charge key value word index :=
  12 * (key + value) + 12 * word + 12 * (word + index) + 4 * index + word.

Lemma weighted_moves_monotone : forall key value word index left right,
  slots left <= slots right -> edges left <= edges right -> links left <= links right ->
  lengths left <= lengths right -> parents left <= parents right ->
  weighted_level_work key value word index left <= weighted_level_work key value word index right.
Proof.
  intros key value word index left right Hslots Hedges Hlinks Hlengths Hparents.
  pose proof (Nat.mul_le_mono_r _ _ (key + value) Hslots) as Hslot_bytes.
  pose proof (Nat.mul_le_mono_r _ _ word Hedges) as Hedge_bytes.
  pose proof (Nat.mul_le_mono_r _ _ (word + index) Hlinks) as Hlink_bytes.
  pose proof (Nat.mul_le_mono_r _ _ index Hlengths) as Hlength_bytes.
  pose proof (Nat.mul_le_mono_r _ _ word Hparents) as Hparent_bytes.
  unfold weighted_level_work. lia.
Qed.

Theorem weighted_insert_level_moves_bounded : forall key value word index internal len edge,
  len <= 11 -> edge <= len ->
  weighted_level_work key value word index (level internal len edge) <=
    weighted_level_charge key value word index.
Proof.
  intros key value word index internal len edge Hlen Hedge.
  destruct (insert_level_moves_bounded internal len edge Hlen Hedge)
    as [Hslots [Hedges [Hlinks [Hlengths Hparents]]]].
  assert (Hcharge : weighted_level_work key value word index
      {| slots := 12; edges := 12; links := 12; lengths := 4; parents := 1 |} =
      weighted_level_charge key value word index).
  { unfold weighted_level_work, weighted_level_charge.
    cbn [slots edges links lengths parents]. lia. }
  rewrite <- Hcharge.
  apply weighted_moves_monotone; assumption.
Qed.

Definition weighted_map_writes word (last : last_level) :=
  match last with
  | StopsAt _ _ _ => word
  | NewRoot | FirstLeaf => word + 2 * word
  | Replaces => 0
  end.

Lemma weighted_last_level_within_charge : forall key value word index last,
  valid_last last ->
  weighted_level_work key value word index (last_moves last) + weighted_map_writes word last <=
    weighted_level_charge key value word index.
Proof.
  intros key value word index [internal len edge | | |] Hvalid.
  - destruct Hvalid as [Hlen Hedge].
    assert (Hfit : weighted_level_work key value word index (fit_level internal len edge) <=
      weighted_level_work key value word index
        {| slots := 11; edges := 11; links := 11; lengths := 1; parents := 0 |}).
    { apply weighted_moves_monotone; unfold fit_level;
        destruct internal; cbn [slots edges links lengths parents]; lia. }
    change (weighted_level_work key value word index (fit_level internal len edge) + word <=
      weighted_level_charge key value word index).
    eapply Nat.le_trans.
    + exact (proj1 (Nat.add_le_mono_r _ _ word) Hfit).
    + unfold weighted_level_work, weighted_level_charge.
      cbn [slots edges links lengths parents]. lia.
  - unfold last_moves, new_root_level, weighted_map_writes, weighted_level_work, weighted_level_charge.
    cbn [slots edges links lengths parents]. nia.
  - unfold last_moves, first_leaf_level, weighted_map_writes, weighted_level_work, weighted_level_charge.
    cbn [slots edges links lengths parents]. nia.
  - unfold last_moves, replace_level, weighted_map_writes, weighted_level_work, weighted_level_charge.
    cbn [slots edges links lengths parents]. nia.
Qed.

Definition weighted_split_work key value word index (splits : list (bool * nat)) :=
  fold_right (fun step total =>
    weighted_level_work key value word index (split_level (fst step) (snd step)) + total) 0 splits.

Definition weighted_insert_path_work key value word index splits last :=
  weighted_split_work key value word index splits +
    (weighted_level_work key value word index (last_moves last) + weighted_map_writes word last).

Lemma weighted_split_work_within_charge : forall key value word index splits,
  Forall (fun step => snd step <= 11) splits ->
  weighted_split_work key value word index splits <=
    length splits * weighted_level_charge key value word index.
Proof.
  intros key value word index splits.
  induction splits as [| [internal edge] rest IH]; intros Hvalid.
  - change (0 <= 0). lia.
  - inversion Hvalid as [| step tail Hedge Hrest]; subst.
    cbn [snd] in Hedge.
    pose proof (weighted_insert_level_moves_bounded key value word index internal 11 edge
      ltac:(lia) Hedge) as Hlevel.
    change (weighted_level_work key value word index (split_level internal edge) <=
      weighted_level_charge key value word index) in Hlevel.
    specialize (IH Hrest).
    change (weighted_level_work key value word index (split_level internal edge) +
      weighted_split_work key value word index rest <=
      S (length rest) * weighted_level_charge key value word index).
    rewrite Nat.mul_succ_l. lia.
Qed.

Theorem weighted_insert_path_within_charge : forall key value word index splits last height,
  Forall (fun step => snd step <= 11) splits -> valid_last last ->
  length splits + 1 <= height ->
  weighted_insert_path_work key value word index splits last <=
    height * weighted_level_charge key value word index.
Proof.
  intros key value word index splits last height Hsplits Hlast Hheight.
  pose proof (weighted_split_work_within_charge key value word index splits Hsplits) as Hwork.
  pose proof (weighted_last_level_within_charge key value word index last Hlast) as Hfinal.
  pose proof (Nat.mul_le_mono_r _ _ (weighted_level_charge key value word index) Hheight) as Hscale.
  rewrite Nat.mul_add_distr_r in Hscale.
  unfold weighted_insert_path_work. lia.
Qed.

Theorem weighted_insert_within_rust_charge : forall key value word index splits last height tree entries,
  OrderedLookupBound.wf true height tree -> OrderedLookupBound.size tree <= entries + 1 ->
  Forall (fun step => snd step <= 11) splits -> valid_last last ->
  length splits + 1 <= height ->
  weighted_insert_path_work key value word index splits last <=
    OrderedLookupBound.height_bound (entries + 1) * weighted_level_charge key value word index.
Proof.
  intros key value word index splits last height tree entries Htree Hsize Hsplits Hlast Hlevels.
  pose proof (OrderedLookupBound.btree_height_bound height tree Htree) as Hheight.
  pose proof (OrderedLookupBound.height_bound_monotone _ _ Hsize) as Hsize_height.
  apply weighted_insert_path_within_charge; [exact Hsplits | exact Hlast | lia].
Qed.

Theorem original_level_weights_are_preserved : forall moves,
  weighted_level_work 4 296 8 2 moves = level_work moves.
Proof. intro moves. reflexivity. Qed.

Definition string_comparison_scan word incoming resident :=
  2 * (Nat.min incoming resident + word).

Definition string_search_scan word incoming resident_lengths :=
  fold_right (fun resident total => string_comparison_scan word incoming resident + total)
    0 resident_lengths.

Lemma incoming_length_bounds_comparison : forall word incoming resident,
  string_comparison_scan word incoming resident <= 2 * (incoming + word).
Proof.
  intros word incoming resident. unfold string_comparison_scan.
  pose proof (Nat.le_min_l incoming resident). lia.
Qed.

Lemma incoming_length_bounds_comparison_trace : forall word incoming resident_lengths,
  string_search_scan word incoming resident_lengths <=
    length resident_lengths * (2 * (incoming + word)).
Proof.
  intros word incoming resident_lengths.
  induction resident_lengths as [| resident rest IH].
  - change (0 <= 0). lia.
  - pose proof (incoming_length_bounds_comparison word incoming resident) as Hcomparison.
    change (string_comparison_scan word incoming resident + string_search_scan word incoming rest <=
      S (length rest) * (2 * (incoming + word))).
    rewrite Nat.mul_succ_l. lia.
Qed.

Theorem incoming_string_search_within_rust_charge : forall word incoming resident_lengths comparisons entries,
  length resident_lengths <= comparisons -> comparisons <= search_operations entries ->
  string_search_scan word incoming resident_lengths <=
    2 * (incoming + word) * search_operations entries.
Proof.
  intros word incoming resident_lengths comparisons entries Hlength Hcomparisons.
  pose proof (incoming_length_bounds_comparison_trace word incoming resident_lengths) as Hscan.
  assert (Hcount : length resident_lengths <= search_operations entries) by lia.
  pose proof (Nat.mul_le_mono_r _ _ (2 * (incoming + word)) Hcount) as Hscale.
  rewrite (Nat.mul_comm (search_operations entries) (2 * (incoming + word))) in Hscale.
  lia.
Qed.

Definition terminal_node_allocation (last : last_level) :=
  match last with NewRoot | FirstLeaf => 1 | StopsAt _ _ _ | Replaces => 0 end.

Definition transient_node_count (splits : list (bool * nat)) last :=
  length splits + terminal_node_allocation last.

Theorem transient_insert_nodes_fit_height : forall (splits : list (bool * nat)) last height,
  length splits + 1 <= height -> transient_node_count splits last <= height.
Proof.
  intros splits last height Hheight. unfold transient_node_count, terminal_node_allocation.
  destruct last; lia.
Qed.

Theorem transient_backing_covers_source_allocations : forall splits last height node_bytes allocations,
  length splits + 1 <= height -> length allocations <= transient_node_count splits last ->
  Forall (fun bytes => bytes <= node_bytes) allocations ->
  fold_right Nat.add 0 allocations <= height * node_bytes.
Proof.
  intros splits last height node_bytes allocations Hheight Hcount Hsizes.
  pose proof (transient_insert_nodes_fit_height splits last height Hheight) as Hnodes.
  pose proof (NativeCheckpointBacking.allocation_sum allocations node_bytes Hsizes) as Hbytes.
  assert (Hall : length allocations <= height) by lia.
  pose proof (Nat.mul_le_mono_r _ _ node_bytes Hall) as Hscale. lia.
Qed.

Record accepted_insert := AcceptedInsert {
  insert_splits : list (bool * nat);
  insert_last : last_level;
  entries_before : nat
}.

Definition insert_height event := OrderedLookupBound.height_bound (entries_before event + 1).

Definition valid_insert_source event :=
  Forall (fun step => snd step <= 11) (insert_splits event) /\
  valid_last (insert_last event) /\ length (insert_splits event) + 1 <= insert_height event.

Definition inserted_entry event := match insert_last event with Replaces => 0 | _ => 1 end.

Definition batch_heights events := fold_right (fun event total => insert_height event + total) 0 events.
Definition batch_allocated_nodes events := fold_right (fun event total =>
  transient_node_count (insert_splits event) (insert_last event) + total) 0 events.
Definition batch_inserted_entries events := fold_right (fun event total => inserted_entry event + total) 0 events.

Lemma insertion_batch_nodes_fit_prepaid_heights : forall events,
  Forall valid_insert_source events -> batch_allocated_nodes events <= batch_heights events.
Proof.
  intros events Hvalid. induction Hvalid as [| event rest Hevent Hrest IH].
  - reflexivity.
  - destruct Hevent as [Hsplits [Hlast Hheight]].
    pose proof (transient_insert_nodes_fit_height (insert_splits event) (insert_last event)
      (insert_height event) Hheight) as Hnodes.
    change (transient_node_count (insert_splits event) (insert_last event) + batch_allocated_nodes rest <=
      insert_height event + batch_heights rest). lia.
Qed.

Lemma insertion_batch_entries_fit_attempts : forall events,
  batch_inserted_entries events <= length events.
Proof.
  intro events. induction events as [| event rest IH].
  - reflexivity.
  - change (inserted_entry event + batch_inserted_entries rest <= S (length rest)).
    unfold inserted_entry. destruct (insert_last event); lia.
Qed.

Record owning_trace := OwningTrace {
  extracted_entries : nat;
  descended_edges : nat;
  released_nodes : nat;
  inspected_lengths : nat
}.

Definition owning_trace_source entries nodes trace :=
  extracted_entries trace = entries /\ descended_edges trace <= nodes - 1 /\
  released_nodes trace = nodes /\ inspected_lengths trace <= entries + nodes.

Definition owning_trace_actions trace :=
  extracted_entries trace + descended_edges trace + released_nodes trace.

Definition owning_trace_scanned key value word trace :=
  extracted_entries trace * (key + value) + descended_edges trace * word +
  released_nodes trace * (word + 2) + inspected_lengths trace * 2.

Lemma node_occupancies_bound_entries : forall occupancies,
  Forall (fun count => count <= 11) occupancies ->
  fold_right Nat.add 0 occupancies <= 11 * length occupancies.
Proof.
  intros occupancies Hoccupied. induction Hoccupied as [| count rest Hcount Hrest IH].
  - reflexivity.
  - cbn [fold_right length]. lia.
Qed.

Theorem owning_trace_actions_fit : forall entries nodes trace,
  owning_trace_source entries nodes trace -> owning_trace_actions trace <= entries + 2 * nodes.
Proof.
  intros entries nodes trace [Hextracted [Hdescended [Hreleased Hlengths]]].
  unfold owning_trace_actions. lia.
Qed.

Theorem owning_trace_scans_fit_node_envelope : forall key value word alignment occupancies trace,
  8 <= alignment -> Forall (fun count => count <= 11) occupancies ->
  owning_trace_source (fold_right Nat.add 0 occupancies) (length occupancies) trace ->
  owning_trace_scanned key value word trace <=
    length occupancies * NativeCheckpointBacking.node_layout_bound key value word alignment.
Proof.
  intros key value word alignment occupancies trace Halignment Hoccupied
    [Hextracted [Hdescended [Hreleased Hlengths]]].
  pose proof (node_occupancies_bound_entries occupancies Hoccupied) as Hentries.
  assert (Hdown : descended_edges trace <= length occupancies) by lia.
  pose proof (Nat.mul_le_mono_r _ _ word Hdown) as Hdown_bytes.
  assert (Hextract : extracted_entries trace <= 11 * length occupancies) by lia.
  pose proof (Nat.mul_le_mono_r _ _ (key + value) Hextract) as Hentry_bytes.
  assert (Hreads : inspected_lengths trace <= 12 * length occupancies) by lia.
  pose proof (Nat.mul_le_mono_r _ _ 2 Hreads) as Hlength_bytes.
  assert (Hpadding : 22 <= 11 * word + 8 * alignment) by lia.
  pose proof (Nat.mul_le_mono_l _ _ (length occupancies) Hpadding) as Hnode_padding.
  unfold owning_trace_scanned, NativeCheckpointBacking.node_layout_bound.
  rewrite Hreleased. nia.
Qed.

Definition prepaid_owning_actions events := length events + 2 * batch_heights events.
Definition prepaid_owning_scanned key value word alignment events :=
  batch_heights events * NativeCheckpointBacking.node_layout_bound key value word alignment.

Theorem accepted_insertions_prepay_final_owning_traversal :
  forall key value word alignment events occupancies trace,
  8 <= alignment -> Forall valid_insert_source events ->
  Forall (fun count => count <= 11) occupancies ->
  fold_right Nat.add 0 occupancies <= batch_inserted_entries events ->
  length occupancies <= batch_allocated_nodes events ->
  owning_trace_source (fold_right Nat.add 0 occupancies) (length occupancies) trace ->
  owning_trace_actions trace <= prepaid_owning_actions events /\
  owning_trace_scanned key value word trace <= prepaid_owning_scanned key value word alignment events.
Proof.
  intros key value word alignment events occupancies trace Halignment Hvalid Hoccupied
    Hentries Hnodes Htrace.
  pose proof (insertion_batch_nodes_fit_prepaid_heights events Hvalid) as Hnode_credit.
  pose proof (insertion_batch_entries_fit_attempts events) as Hentry_credit.
  pose proof (owning_trace_actions_fit _ _ trace Htrace) as Hactions.
  pose proof (owning_trace_scans_fit_node_envelope key value word alignment occupancies trace
    Halignment Hoccupied Htrace) as Hscanned.
  assert (Hnode_bound : length occupancies <= batch_heights events) by lia.
  pose proof (Nat.mul_le_mono_r _ _
    (NativeCheckpointBacking.node_layout_bound key value word alignment) Hnode_bound) as Hscan_credit.
  unfold prepaid_owning_actions, prepaid_owning_scanned. split; lia.
Qed.

Theorem per_insert_owning_credit_composes : forall events,
  fold_right (fun event total => 1 + 2 * insert_height event + total) 0 events =
    prepaid_owning_actions events.
Proof.
  intro events. induction events as [| event rest IH].
  - reflexivity.
  - change (1 + 2 * insert_height event +
      fold_right (fun next total => 1 + 2 * insert_height next + total) 0 rest =
      S (length rest) + 2 * (insert_height event + batch_heights rest)).
    rewrite IH. unfold prepaid_owning_actions. lia.
Qed.

Theorem per_insert_owning_scan_credit_composes : forall key value word alignment events,
  fold_right (fun event total =>
    insert_height event * NativeCheckpointBacking.node_layout_bound key value word alignment + total)
    0 events = prepaid_owning_scanned key value word alignment events.
Proof.
  intros key value word alignment events. induction events as [| event rest IH].
  - reflexivity.
  - change (insert_height event * NativeCheckpointBacking.node_layout_bound key value word alignment +
      fold_right (fun next total =>
        insert_height next * NativeCheckpointBacking.node_layout_bound key value word alignment + total)
        0 rest =
      (insert_height event + batch_heights rest) *
        NativeCheckpointBacking.node_layout_bound key value word alignment).
    rewrite IH. unfold prepaid_owning_scanned. rewrite Nat.mul_add_distr_r. reflexivity.
Qed.

Definition comparison_allowance word incoming comparisons :=
  if comparisons =? 0 then 0 else 2 * (incoming + word) * comparisons.

Theorem zero_comparisons_skip_key_length_arithmetic : forall word incoming comparisons,
  comparison_allowance word incoming comparisons = 2 * (incoming + word) * comparisons.
Proof.
  intros word incoming [| comparisons].
  - unfold comparison_allowance. rewrite Nat.mul_0_r. reflexivity.
  - reflexivity.
Qed.

Definition insertion_reservation key value word alignment entries incoming :=
  let height := OrderedLookupBound.height_bound (entries + 1) in
  let comparisons := search_operations entries in
  let node_bytes := NativeCheckpointBacking.node_layout_bound key value word alignment in
  (3 + 4 * height + comparisons,
    comparison_allowance word incoming comparisons +
      height * weighted_level_charge key value word 2 + height * node_bytes,
    height * node_bytes).

Theorem insertion_reservation_components_compose : forall key value word alignment entries incoming,
  let height := OrderedLookupBound.height_bound (entries + 1) in
  let comparisons := search_operations entries in
  let node_bytes := NativeCheckpointBacking.node_layout_bound key value word alignment in
  insertion_reservation key value word alignment entries incoming =
    (2 * (1 + height) + comparisons + (1 + 2 * height),
      2 * (incoming + word) * comparisons +
        height * weighted_level_charge key value word 2 + height * node_bytes,
      height * node_bytes).
Proof.
  intros key value word alignment entries incoming.
  cbv zeta. unfold insertion_reservation.
  rewrite zero_comparisons_skip_key_length_arithmetic.
  assert (Hoperations : 3 + 4 * OrderedLookupBound.height_bound (entries + 1) +
      search_operations entries =
      2 * (1 + OrderedLookupBound.height_bound (entries + 1)) + search_operations entries +
        (1 + 2 * OrderedLookupBound.height_bound (entries + 1))) by lia.
  rewrite Hoperations. reflexivity.
Qed.

Print Assumptions weighted_insert_level_moves_bounded.
Print Assumptions weighted_insert_path_within_charge.
Print Assumptions weighted_insert_within_rust_charge.
Print Assumptions original_level_weights_are_preserved.
Print Assumptions incoming_length_bounds_comparison.
Print Assumptions incoming_string_search_within_rust_charge.
Print Assumptions transient_insert_nodes_fit_height.
Print Assumptions transient_backing_covers_source_allocations.
Print Assumptions insertion_batch_nodes_fit_prepaid_heights.
Print Assumptions insertion_batch_entries_fit_attempts.
Print Assumptions owning_trace_actions_fit.
Print Assumptions owning_trace_scans_fit_node_envelope.
Print Assumptions accepted_insertions_prepay_final_owning_traversal.
Print Assumptions per_insert_owning_credit_composes.
Print Assumptions per_insert_owning_scan_credit_composes.
Print Assumptions zero_comparisons_skip_key_length_arithmetic.
Print Assumptions insertion_reservation_components_compose.

End StringMapInsertion.
