(* D-S2 (epic 8946, Phase D item D-C1b; decision record DR-95): the exact
   charge of the metered history decoder in History mode.

   Shapes. A decoded value of a closed history record type is a tree of
   shapes:
   - Inline s cs: a struct, tuple, enum variant, option or scalar of s
     bytes; its children cs lie inside it, so decoding it allocates nothing;
   - Seq e cs: a vector whose elements of e bytes are cs; serde's vector
     visitor starts at capacity 0 (the decoder reports no size hint) and
     pushes each element, so the vector grows as Rust's RawVec grows;
   - TreeSet n ins: a tagged B-tree set (DR-95 part 1) whose tree bytes use
     nodes of n bytes; each insert pairs an element with the bytes that the
     insert allocates;
   - Map kn vn ins: a B-tree map; the decoder charges the key half of the
     tree bytes (nodes of kn bytes) before each key and the value half
     (nodes of vn bytes) before each value; each insert pairs a key and a
     value with the bytes that the insert allocates;
   - Str n: a string of n bytes, allocated exactly.
   Boxes, shared pointers, hash tables and untagged sets do not occur: the
   closed-world marker of the Rust decoder (ClosedDecode) admits only record
   types built from these shapes.

   Charges. The decoder charges each node once: one operation and the
   node's size in scanned bytes, with no backing. Before element i of a
   vector it computes the capacity that the push leaves and, when that
   capacity differs from the current one, reserves the new capacity times
   the element size. Before insert i of a tree set it reserves the
   increment tree_bytes (i + 1) - tree_bytes i (IncrementalTreeBacking); a
   map reserves the key half before the key and the value half before the
   value. Before a string it reserves the string's length.

   Allocation premises. A vector allocates as RawVec allocates. The inserts
   into a B-tree from empty allocate, after every prefix of k inserts, at
   most the tree bytes of k entries (wf). The premise is cumulative: one
   insert can allocate more than its own increment, and the slack of the
   earlier increments pays for it (twelfth_insert_needs_earlier_slack).
   btree_allocation_within derives the premise from the B-tree occupancy
   invariant through IncrementalTreeBacking.incremental_prefix_covers_nodes,
   and split_map_tree_covers_joint shows that the key and value halves of
   the Rust node size cover the joint node.

   Results:
   - growth_reservations_match_raw_vec: the decoder's growth reservations
     are exactly the buffers that RawVec allocates for the same pushes;
   - grown_holds_push: the capacity after a push holds the pushed element;
   - decode_charge_prefix_covers: in the decode trace of a well-formed
     shape, every prefix allocates at most what it reserved;
   - decode_charge_covers_allocation: the whole trace's reservation covers
     its allocation;
   - single_charge_per_node: the decoder makes one node charge per node;
   - split_map_tree_covers_joint and btree_allocation_within: the map and
     tree premises follow from the Rust node size and the B-tree occupancy
     invariant;
   - legacy_byte_array_excess and twelfth_insert_needs_earlier_slack
     (contrasts): the legacy rule charged a 32-byte vector 20,928 bytes of
     backing for 56 allocated bytes; a B-tree split allocates more than its
     own increment;
   - untagged_set_undercharges and box_invisible_undercharges (negative
     controls): a set decoded under the vector rule, and a box decoded as an
     inline node, leave allocations that no prefix of reservations covers.

   Rust correspondence: rspace++/src/rspace/history/native_reader/typed.rs
   (Mode::History: decode_history_record, grown, Budget::seq_growth and
   Budget::tree_insert, the Seq, TreeSet and Map guards);
   shared/src/rust/closed_decode.rs (ClosedDecode);
   shared/src/rust/collection_backing.rs (tree_backing, tree_growth). The
   extracted property tests are in native_reader/typed/tests.rs and in the
   rholang native_runtime history_decode tests. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
From CostAccountedRho Require Import ObservationReadCoverage IncrementalTreeBacking.
Import ListNotations.

(* ------------------------------------------------------------------------ *)
(* Vector growth                                                            *)

(* RawVec's smallest non-zero capacity by element size (min_non_zero_cap). *)
Definition min_capacity (element : nat) : nat :=
  if element =? 1 then 8 else if element <=? 1024 then 4 else 1.

(* RawVec's grow_amortized for one more element at length len and capacity
   cap. *)
Definition raw_vec_grown (element cap len : nat) : nat :=
  Nat.max (2 * cap) (Nat.max (len + 1) (min_capacity element)).

(* The buffers that count pushes allocate from capacity cap and length len:
   a push reallocates only when the vector is full. *)
Fixpoint push_allocations (element cap len count : nat) : list nat :=
  match count with
  | 0 => []
  | S count' =>
      if len <? cap then push_allocations element cap (S len) count'
      else raw_vec_grown element cap len * element
             :: push_allocations element (raw_vec_grown element cap len) (S len) count'
  end.

(* The decoder's capacity model (typed.rs, grown): the capacity after a
   push. *)
Definition grown (element cap len : nat) : nat :=
  if len <? cap then cap else raw_vec_grown element cap len.

(* The decoder's growth reservations (Budget::seq_growth): before each push
   it computes the capacity that the push leaves and reserves its bytes when
   it differs from the current capacity. *)
Fixpoint growth_reservations (element cap len count : nat) : list nat :=
  match count with
  | 0 => []
  | S count' =>
      let next := grown element cap len in
      (if next =? cap then [] else [next * element])
      ++ growth_reservations element next (S len) count'
  end.

Lemma raw_vec_grown_holds_push : forall element cap len, len < raw_vec_grown element cap len.
Proof. intros element cap len. unfold raw_vec_grown. lia. Qed.

Theorem grown_holds_push : forall element cap len, len < grown element cap len.
Proof.
  intros element cap len. unfold grown.
  destruct (Nat.ltb_spec len cap); [lia | apply raw_vec_grown_holds_push].
Qed.

(* The decoder's test (the new capacity differs) and RawVec's test (the
   vector is full) agree while the length stays within the capacity, so the
   decoder reserves exactly the buffers that RawVec allocates. *)
Theorem growth_reservations_match_raw_vec : forall element count cap len,
  len <= cap ->
  growth_reservations element cap len count = push_allocations element cap len count.
Proof.
  intros element count. induction count as [| count IH]; intros cap len invariant;
    [reflexivity |].
  cbn [growth_reservations push_allocations]. unfold grown.
  destruct (Nat.ltb_spec len cap) as [below | above]; cbn beta iota.
  - rewrite Nat.eqb_refl. cbn [app]. apply IH. lia.
  - pose proof (raw_vec_grown_holds_push element cap len) as holds.
    assert (changes : raw_vec_grown element cap len =? cap = false)
      by (apply Nat.eqb_neq; lia).
    rewrite changes. cbn [app]. f_equal. apply IH. lia.
Qed.

(* ------------------------------------------------------------------------ *)
(* Shapes, decode traces and the allocation premise                         *)

Inductive shape :=
  | Inline (size : nat) (children : list shape)
  | Seq (element : nat) (elements : list shape)
  | TreeSet (node : nat) (inserts : list (shape * nat))
  | Map (key_node value_node : nat) (inserts : list ((shape * shape) * nat))
  | Str (length : nat).

(* Events of a decode: a node charge, a backing reservation and an
   allocation. *)
Inductive decode_event := Node | Reserve (bytes : nat) | Alloc (bytes : nat).

(* The reservation before insert index into a tree whose nodes have node
   bytes (typed.rs, Budget::tree_insert, through tree_growth). *)
Definition increment (node index : nat) : nat :=
  tree_bytes node (S index) - tree_bytes node index.

(* A vector: before element i the growth reservation, when the decoder's
   capacity changes; then the element; then the push's reallocation, when
   RawVec's vector is full. *)
Fixpoint seq_trace (decode : shape -> list decode_event) (element cap len : nat)
  (elements : list shape) : list decode_event :=
  match elements with
  | [] => []
  | e :: rest =>
      (if grown element cap len =? cap then [] else [Reserve (grown element cap len * element)])
      ++ decode e
      ++ (if len <? cap then [] else [Alloc (raw_vec_grown element cap len * element)])
      ++ seq_trace decode element (grown element cap len) (S len) rest
  end.

(* A tree set: before insert i the increment, then the element, then the
   insert's allocation. *)
Fixpoint set_trace (decode : shape -> list decode_event) (node index : nat)
  (inserts : list (shape * nat)) : list decode_event :=
  match inserts with
  | [] => []
  | (e, allocated) :: rest =>
      Reserve (increment node index) :: decode e
      ++ Alloc allocated :: set_trace decode node (S index) rest
  end.

(* A map: before insert i the key half and the key, the value half and the
   value, then the insert's allocation. *)
Fixpoint map_trace (decode : shape -> list decode_event) (key_node value_node index : nat)
  (inserts : list ((shape * shape) * nat)) : list decode_event :=
  match inserts with
  | [] => []
  | ((key, value), allocated) :: rest =>
      Reserve (increment key_node index) :: decode key
      ++ Reserve (increment value_node index) :: decode value
      ++ Alloc allocated :: map_trace decode key_node value_node (S index) rest
  end.

Fixpoint decode_trace (s : shape) : list decode_event :=
  match s with
  | Inline _ children => Node :: flat_map decode_trace children
  | Seq element elements => Node :: seq_trace decode_trace element 0 0 elements
  | TreeSet node inserts => Node :: set_trace decode_trace node 0 inserts
  | Map key_node value_node inserts =>
      Node :: map_trace decode_trace key_node value_node 0 inserts
  | Str length => [Node; Reserve length; Alloc length]
  end.

(* The inserts into a B-tree from empty: after every prefix, the bytes
   allocated so far are at most bytes of the number of inserts. *)
Fixpoint within (bytes : nat -> nat) (index used : nat) (allocations : list nat) : Prop :=
  match allocations with
  | [] => True
  | allocated :: rest =>
      used + allocated <= bytes (S index) /\ within bytes (S index) (used + allocated) rest
  end.

Fixpoint wf (s : shape) : Prop :=
  match s with
  | Inline _ children => fold_right (fun c rest => wf c /\ rest) True children
  | Seq _ elements => fold_right (fun c rest => wf c /\ rest) True elements
  | TreeSet node inserts =>
      fold_right (fun '(e, _) rest => wf e /\ rest) True inserts
      /\ within (tree_bytes node) 0 0 (map snd inserts)
  | Map key_node value_node inserts =>
      fold_right (fun '((key, value), _) rest => wf key /\ wf value /\ rest) True inserts
      /\ within (fun entries => tree_bytes key_node entries + tree_bytes value_node entries)
           0 0 (map snd inserts)
  | Str _ => True
  end.

(* ------------------------------------------------------------------------ *)
(* Coverage                                                                 *)

(* The coverage view of a decode trace (ObservationReadCoverage): a node
   reads nothing, a reservation reserves and an allocation reads. *)
Definition as_event (e : decode_event) : event :=
  match e with
  | Node => Read 0
  | Reserve bytes => ObservationReadCoverage.Reserve bytes
  | Alloc bytes => Read bytes
  end.

Definition as_events (trace : list decode_event) : list event := map as_event trace.

Lemma as_events_cons : forall e trace, as_events (e :: trace) = as_event e :: as_events trace.
Proof. reflexivity. Qed.

Lemma as_events_app : forall a b, as_events (a ++ b) = as_events a ++ as_events b.
Proof. intros a b. apply map_app. Qed.

(* A trace is balanced when every prefix allocates at most what it
   reserved. *)
Definition balanced (trace : list decode_event) : Prop := covered (as_events trace).

Lemma covered_from_mono : forall slack slack' trace,
  slack <= slack' -> covered_from slack trace -> covered_from slack' trace.
Proof.
  intros slack slack' trace more covered_trace prefix suffix split.
  pose proof (covered_trace prefix suffix split). lia.
Qed.

Lemma covered_from_nil : forall slack, covered_from slack [].
Proof.
  intros slack prefix suffix split.
  destruct prefix; [unfold total; cbn; lia | discriminate].
Qed.

Lemma covered_from_reserve : forall slack units trace,
  covered_from (slack + units) trace ->
  covered_from slack (ObservationReadCoverage.Reserve units :: trace).
Proof.
  intros slack units trace covered_trace.
  change (ObservationReadCoverage.Reserve units :: trace)
    with ([ObservationReadCoverage.Reserve units] ++ trace).
  apply covered_from_app.
  - change [ObservationReadCoverage.Reserve units]
      with (map ObservationReadCoverage.Reserve [units]).
    apply reserves_covered.
  - unfold total. cbn. lia.
  - eapply covered_from_mono; [| exact covered_trace]. unfold total. cbn. lia.
Qed.

Lemma covered_from_read : forall slack units trace,
  units <= slack -> covered_from (slack - units) trace ->
  covered_from slack (Read units :: trace).
Proof.
  intros slack units trace fits covered_trace.
  change (Read units :: trace) with ([Read units] ++ trace).
  apply covered_from_app.
  - change [Read units] with (map Read [units]). apply reads_covered. cbn. lia.
  - unfold total. cbn. lia.
  - eapply covered_from_mono; [| exact covered_trace]. unfold total. cbn. lia.
Qed.

(* A covered block only adds slack. *)
Lemma covered_balanced_app : forall slack block trace,
  covered block -> covered_from slack trace -> covered_from slack (block ++ trace).
Proof.
  intros slack block trace covered_block covered_trace.
  pose proof (covered_block block [] (eq_sym (app_nil_r block))) as fits.
  apply covered_from_app.
  - apply (covered_from_mono 0); [lia | exact covered_block].
  - lia.
  - eapply covered_from_mono; [| exact covered_trace]. lia.
Qed.

Lemma seq_trace_covered : forall decode element elements cap len slack,
  fold_right (fun e rest => balanced (decode e) /\ rest) True elements ->
  len <= cap ->
  covered_from slack (as_events (seq_trace decode element cap len elements)).
Proof.
  intros decode element elements.
  induction elements as [| e rest IH]; intros cap len slack balanced_elements invariant;
    cbn [seq_trace].
  - apply covered_from_nil.
  - destruct balanced_elements as [balanced_e balanced_rest].
    unfold grown. destruct (Nat.ltb_spec len cap) as [below | above]; cbn beta iota.
    + rewrite Nat.eqb_refl. cbn [app]. rewrite as_events_app.
      apply covered_balanced_app; [exact balanced_e |].
      apply IH; [exact balanced_rest | lia].
    + pose proof (raw_vec_grown_holds_push element cap len) as holds.
      assert (changes : raw_vec_grown element cap len =? cap = false)
        by (apply Nat.eqb_neq; lia).
      rewrite changes. cbn [app].
      rewrite as_events_cons, as_events_app, as_events_cons. cbn [as_event].
      apply covered_from_reserve. apply covered_balanced_app; [exact balanced_e |].
      apply covered_from_read; [lia |].
      apply IH; [exact balanced_rest | lia].
Qed.

(* The slack invariant: before insert index the outer slack plus the bytes
   allocated so far cover the tree bytes reserved so far. *)
Lemma set_trace_covered : forall decode node inserts index used slack,
  fold_right (fun '(e, _) rest => balanced (decode e) /\ rest) True inserts ->
  within (tree_bytes node) index used (map snd inserts) ->
  tree_bytes node index <= slack + used ->
  covered_from slack (as_events (set_trace decode node index inserts)).
Proof.
  intros decode node inserts.
  induction inserts as [| [e allocated] rest IH]; intros index used slack elements premise surplus;
    cbn [set_trace].
  - apply covered_from_nil.
  - cbn [fold_right] in elements. destruct elements as [balanced_e balanced_rest].
    cbn [map snd within] in premise. destruct premise as [fits premise_rest].
    pose proof (tree_bytes_monotone node index (S index) ltac:(lia)) as grows.
    rewrite as_events_cons, as_events_app, as_events_cons. cbn [as_event].
    apply covered_from_reserve. apply covered_balanced_app; [exact balanced_e |].
    apply covered_from_read; [unfold increment; lia |].
    apply (IH (S index) (used + allocated)); [exact balanced_rest | exact premise_rest |].
    unfold increment. lia.
Qed.

Lemma map_trace_covered : forall decode key_node value_node inserts index used slack,
  fold_right (fun '((key, value), _) rest => balanced (decode key) /\ balanced (decode value) /\ rest)
    True inserts ->
  within (fun entries => tree_bytes key_node entries + tree_bytes value_node entries)
    index used (map snd inserts) ->
  tree_bytes key_node index + tree_bytes value_node index <= slack + used ->
  covered_from slack (as_events (map_trace decode key_node value_node index inserts)).
Proof.
  intros decode key_node value_node inserts.
  induction inserts as [| [[key value] allocated] rest IH];
    intros index used slack entries premise surplus; cbn [map_trace].
  - apply covered_from_nil.
  - cbn [fold_right] in entries. destruct entries as [balanced_key [balanced_value balanced_rest]].
    cbn [map snd within] in premise. destruct premise as [fits premise_rest].
    pose proof (tree_bytes_monotone key_node index (S index) ltac:(lia)) as key_grows.
    pose proof (tree_bytes_monotone value_node index (S index) ltac:(lia)) as value_grows.
    rewrite as_events_cons, as_events_app, as_events_cons, as_events_app, as_events_cons.
    cbn [as_event].
    apply covered_from_reserve. apply covered_balanced_app; [exact balanced_key |].
    apply covered_from_reserve. apply covered_balanced_app; [exact balanced_value |].
    apply covered_from_read; [unfold increment; lia |].
    apply (IH (S index) (used + allocated)); [exact balanced_rest | exact premise_rest |].
    unfold increment. lia.
Qed.

Lemma tree_bytes_empty : forall node, tree_bytes node 0 = 0.
Proof. reflexivity. Qed.

Theorem decode_charge_prefix_covers : forall s, wf s -> balanced (decode_trace s).
Proof.
  fix IH 1.
  intros [size children | element elements | node inserts | key_node value_node inserts | length]
    well_formed; unfold balanced, covered; cbn [decode_trace wf] in *;
    rewrite ?as_events_cons; cbn [as_event]; apply covered_from_read; try lia.
  - induction children as [| c rest IHrest]; cbn [flat_map].
    + apply covered_from_nil.
    + destruct well_formed as [wf_c wf_rest]. rewrite as_events_app.
      apply covered_balanced_app; [exact (IH c wf_c) | exact (IHrest wf_rest)].
  - apply seq_trace_covered; [| lia].
    induction elements as [| e rest IHrest]; cbn [fold_right]; [exact I |].
    destruct well_formed as [wf_e wf_rest].
    split; [exact (IH e wf_e) | exact (IHrest wf_rest)].
  - destruct well_formed as [wf_inserts premise].
    apply (set_trace_covered decode_trace node inserts 0 0);
      [| exact premise | rewrite tree_bytes_empty; lia].
    clear premise.
    induction inserts as [| [e allocated] rest IHrest]; cbn [fold_right] in *; [exact I |].
    destruct wf_inserts as [wf_e wf_rest].
    split; [exact (IH e wf_e) | exact (IHrest wf_rest)].
  - destruct well_formed as [wf_inserts premise].
    apply (map_trace_covered decode_trace key_node value_node inserts 0 0);
      [| exact premise | rewrite !tree_bytes_empty; lia].
    clear premise.
    induction inserts as [| [[key value] allocated] rest IHrest]; cbn [fold_right] in *;
      [exact I |].
    destruct wf_inserts as [wf_key [wf_value wf_rest]].
    split; [exact (IH key wf_key) | split; [exact (IH value wf_value) | exact (IHrest wf_rest)]].
  - unfold as_events. cbn [map as_event].
    apply covered_from_reserve. apply covered_from_read; [lia |]. apply covered_from_nil.
Qed.

Corollary decode_charge_covers_allocation : forall s, wf s ->
  total read_units (as_events (decode_trace s))
    <= total reserved_units (as_events (decode_trace s)).
Proof.
  intros s well_formed.
  pose proof (decode_charge_prefix_covers s well_formed (as_events (decode_trace s)) []
                (eq_sym (app_nil_r _))) as fits.
  lia.
Qed.

(* ------------------------------------------------------------------------ *)
(* One node charge per node                                                 *)

Definition node_charges (e : decode_event) : nat := match e with Node => 1 | _ => 0 end.

Definition sum (project : decode_event -> nat) (trace : list decode_event) : nat :=
  fold_right (fun e total => project e + total) 0 trace.

Lemma sum_cons : forall project e trace, sum project (e :: trace) = project e + sum project trace.
Proof. reflexivity. Qed.

Lemma sum_app : forall project a b, sum project (a ++ b) = sum project a + sum project b.
Proof.
  intros project a b. unfold sum.
  induction a as [| e rest IH]; cbn; [reflexivity | rewrite IH; lia].
Qed.

Fixpoint nodes (s : shape) : nat :=
  match s with
  | Inline _ children => 1 + fold_right (fun c total => nodes c + total) 0 children
  | Seq _ elements => 1 + fold_right (fun c total => nodes c + total) 0 elements
  | TreeSet _ inserts => 1 + fold_right (fun '(e, _) total => nodes e + total) 0 inserts
  | Map _ _ inserts =>
      1 + fold_right (fun '((key, value), _) total => nodes key + nodes value + total) 0 inserts
  | Str _ => 1
  end.

Lemma seq_trace_nodes : forall decode element elements cap len,
  sum node_charges (seq_trace decode element cap len elements) =
  fold_right (fun e total => sum node_charges (decode e) + total) 0 elements.
Proof.
  intros decode element elements.
  induction elements as [| e rest IH]; intros cap len; cbn [seq_trace fold_right];
    [reflexivity |].
  rewrite !sum_app, IH.
  destruct (grown element cap len =? cap); destruct (len <? cap);
    cbn [sum fold_right node_charges]; lia.
Qed.

Lemma set_trace_nodes : forall decode node inserts index,
  sum node_charges (set_trace decode node index inserts) =
  fold_right (fun '(e, _) total => sum node_charges (decode e) + total) 0 inserts.
Proof.
  intros decode node inserts.
  induction inserts as [| [e allocated] rest IH]; intros index; cbn [set_trace fold_right];
    [reflexivity |].
  rewrite sum_cons, sum_app, sum_cons, IH. cbn [node_charges]. lia.
Qed.

Lemma map_trace_nodes : forall decode key_node value_node inserts index,
  sum node_charges (map_trace decode key_node value_node index inserts) =
  fold_right (fun '((key, value), _) total =>
                sum node_charges (decode key) + sum node_charges (decode value) + total) 0 inserts.
Proof.
  intros decode key_node value_node inserts.
  induction inserts as [| [[key value] allocated] rest IH]; intros index;
    cbn [map_trace fold_right]; [reflexivity |].
  rewrite sum_cons, sum_app, sum_cons, sum_app, sum_cons, IH. cbn [node_charges]. lia.
Qed.

Theorem single_charge_per_node : forall s, sum node_charges (decode_trace s) = nodes s.
Proof.
  fix IH 1.
  intros [size children | element elements | node inserts | key_node value_node inserts | length];
    cbn [decode_trace nodes]; rewrite sum_cons; cbn [node_charges].
  - f_equal. induction children as [| c rest IHrest]; cbn [flat_map fold_right];
      [reflexivity |].
    rewrite sum_app, (IH c), IHrest. reflexivity.
  - rewrite seq_trace_nodes. f_equal.
    induction elements as [| e rest IHrest]; cbn [fold_right]; [reflexivity |].
    rewrite (IH e), IHrest. reflexivity.
  - rewrite set_trace_nodes. f_equal.
    induction inserts as [| [e allocated] rest IHrest]; cbn [fold_right]; [reflexivity |].
    rewrite (IH e), IHrest. reflexivity.
  - rewrite map_trace_nodes. f_equal.
    induction inserts as [| [[key value] allocated] rest IHrest]; cbn [fold_right];
      [reflexivity |].
    rewrite (IH key), (IH value), IHrest. reflexivity.
  - reflexivity.
Qed.

(* ------------------------------------------------------------------------ *)
(* The B-tree premises                                                      *)

(* The node size of tree_backing (collection_backing.rs) for keys of key
   bytes, values of value bytes and an alignment: 11 key and value slots,
   13 pointers, 4 bytes of lengths and 8 * alignment bytes of padding. *)
Definition rust_node_bytes (key value alignment : nat) : nat :=
  11 * (key + value) + 13 * 8 + 4 + 8 * alignment.

(* The alignment of tree_backing: the largest of the key, value, usize (8)
   and u16 (2) alignments. *)
Definition rust_alignment (key_align value_align : nat) : nat :=
  Nat.max key_align (Nat.max value_align (Nat.max 8 2)).

(* The History rule charges a map's key half with the nodes of the key and a
   unit value, and its value half with the nodes of a unit key and the value
   (a unit has size 0 and alignment 1). For every entry count, the two
   halves cover the tree bytes of the joint node. *)
Theorem split_map_tree_covers_joint : forall key value key_align value_align entries,
  tree_bytes (rust_node_bytes key value (rust_alignment key_align value_align)) entries <=
  tree_bytes (rust_node_bytes key 0 (rust_alignment key_align 1)) entries
  + tree_bytes (rust_node_bytes 0 value (rust_alignment 1 value_align)) entries.
Proof.
  intros key value key_align value_align entries. unfold tree_bytes.
  rewrite <- Nat.mul_add_distr_l. apply Nat.mul_le_mono_l.
  unfold rust_node_bytes, rust_alignment. lia.
Qed.

(* The premise of the tree traces follows from the B-tree occupancy
   invariant. Suppose the inserts so far leave a tree whose root holds at
   least one entry and whose other nodes hold at least five, with no more
   entries than inserts (a duplicate insert adds no entry). Suppose also
   that the inserts allocated at most node bytes for each node of that tree
   (an insert-only tree frees no node). Then the allocation is at most the
   tree bytes of the number of inserts. *)
Theorem btree_allocation_within : forall node inserts root occupancies used,
  1 <= root -> Forall (fun count => 5 <= count) occupancies ->
  root + fold_right Nat.add 0 occupancies <= inserts ->
  used <= S (length occupancies) * node ->
  used <= tree_bytes node inserts.
Proof.
  intros node inserts root occupancies used root_live occupied entries allocated.
  pose proof (incremental_prefix_covers_nodes node root occupancies root_live occupied) as covers.
  rewrite incremental_bytes_from_empty, sum_repeat_one in covers.
  pose proof (tree_bytes_monotone node _ _ entries). lia.
Qed.

(* ------------------------------------------------------------------------ *)
(* Contrasts and negative controls                                          *)

(* Contrast: the legacy rule charged each decoded u8 at two hooks: 8 * 1
   bytes, the tree_backing of one u8 entry (183) and the hash_backing of one
   u8 entry ((1 + 1) * 4 + 64 + 64 = 136), so 327 bytes per hook. A vector
   of 32 u8 allocates buffers of 8, 16 and 32 bytes. *)
Definition legacy_node_backing : nat := 8 * 1 + rust_node_bytes 1 0 (rust_alignment 1 1) + 136.

Example legacy_byte_array_excess :
  legacy_node_backing = 327 /\
  32 * (2 * legacy_node_backing) = 64 * 327 /\
  fold_right Nat.add 0 (push_allocations 1 0 0 32) = 56.
Proof. repeat split; vm_compute; reflexivity. Qed.

(* Contrast: the tree premise is cumulative. A Rust B-tree leaf holds 11
   keys. Twelve sorted inserts allocate a leaf at the first insert and, at
   the twelfth, a second leaf and an internal root. With nodes of one unit,
   the twelfth insert's own increment is 0, so no per-insert bound holds,
   but every prefix stays within the tree bytes. *)
Example twelfth_insert_needs_earlier_slack :
  increment 1 11 = 0 /\
  within (tree_bytes 1) 0 0 [1; 0; 0; 0; 0; 0; 0; 0; 0; 0; 0; 2].
Proof. split; [vm_compute; reflexivity |]. vm_compute. repeat split; lia. Qed.

(* Negative control: a set decoded under the vector rule. A one-element set
   of 4-byte values would reserve the vector's 16 bytes, but its insert
   allocates a B-tree leaf with 11 key slots, at least 44 bytes, and no
   prefix of the trace covers it. *)
Example untagged_set_undercharges :
  growth_reservations 4 0 0 1 = [16] /\
  ~ covered (as_events [Node; Reserve 16; Node; Alloc (11 * 4)]).
Proof.
  split; [vm_compute; reflexivity |].
  intros covered_trace.
  specialize (covered_trace (as_events [Node; Reserve 16; Node; Alloc (11 * 4)]) [] eq_refl).
  vm_compute in covered_trace. lia.
Qed.

(* Negative control: a box. A box of a 64-byte payload allocates 64 bytes,
   but the History rule decodes it as an inline node with no reservation,
   and no prefix covers the allocation. The closed-world marker excludes
   boxes. *)
Example box_invisible_undercharges :
  ~ covered (as_events (decode_trace (Inline 8 [Inline 64 []]) ++ [Alloc 64])).
Proof.
  intros covered_trace.
  specialize (covered_trace (as_events (decode_trace (Inline 8 [Inline 64 []]) ++ [Alloc 64])) []
                (eq_sym (app_nil_r _))).
  vm_compute in covered_trace. lia.
Qed.

Print Assumptions growth_reservations_match_raw_vec.
Print Assumptions grown_holds_push.
Print Assumptions decode_charge_prefix_covers.
Print Assumptions decode_charge_covers_allocation.
Print Assumptions single_charge_per_node.
Print Assumptions split_map_tree_covers_joint.
Print Assumptions btree_allocation_within.
Print Assumptions legacy_byte_array_excess.
Print Assumptions twelfth_insert_needs_earlier_slack.
Print Assumptions untagged_set_undercharges.
Print Assumptions box_invisible_undercharges.
