(* D-O1 (epic 8946, Phase D items D-B1 and D-B3; decision record DR-92):
   block accounting for the clone-backing walker.

   Values. A value is a tree of nodes:
   - Field s: an inline scalar of s bytes (or a shared pointer); it lies in
     its enclosing region and the walker does not push it;
   - Empty s: an empty container of s bytes (an empty vector, string or
     map, or a None); it lies in its enclosing region, and the walker reads
     at most its own bytes (its length or discriminant) and does not push it
     (DR-94);
   - Entry s cs: an inline aggregate of s bytes (a struct, an enum, a vector
     header); it lies in its enclosing region and the walker pushes it on the
     worklist and visits it;
   - Block b v cs: a separate memory block of b bytes (a vector buffer, a
     box, string bytes); v says whether the walker visits its elements cs.
   The inline size of a node is the space it takes in its enclosing region
   (a block takes none: its parent holds a pointer to it). A tree is well
   formed when the inline children of every entry fit in the entry, the
   elements of every block fit in the block, and the elements of an opaque
   block are fields (the walker does not visit them: a boxed scalar, a
   shared scalar payload).

   Work. The walker visits each entry: it moves the entry through the
   worklist (one write and one read of p bytes), reads the entry's own bytes
   (those not inside an inline child: a discriminant, a pointer, a length)
   and may re-read one word of w bytes that an enclosing enum shares with
   the entry as a niche. It reads at most the own bytes of an empty
   container to find that the container is empty. One linear consumer traversal (a clone, a
   comparison, a drop, an encoding of a flat message) reads every block
   once and may re-read one word of each node. A clone also writes every
   block, the root into its destination and every other block into a new
   allocation.

   Charges, parametric in the constants: an entry costs E, a field F, a
   visited block 2b and an opaque block b; a copy adds one write of every
   block. The root's inline bytes form one more block, read through a
   reference: inspection 2s for a visited root and s for an inline root,
   copy 3s and 2s, with no allocation. Hypotheses: E >= 2p + 2w and F >= w.

   Results (D-B1):
   - visit_reads_le_block_bytes: the walker's reads of the entries inside a
     block's region are at most the block's bytes (the regions partition);
   - block_inspection_covers_walk_and_traversal and
     block_copy_covers_walk_and_clone: the charge covers the walker's reads
     and one traversal (and, for a copy, the clone's writes);
   - block_trace_covered: the walk reserves each node's charge before that
     node's reads and every block before the entries inside it, so every
     prefix of the run is covered (the covered notion of
     ObservationReadCoverage);
   - block_copy_backing_covers_allocation: a copy's backing equals the
     clone's allocation trace;
   - block_inspection_fits_block_copy;
   - block_charge_independent_of_sibling_order: the charge is a function of
     the tree's shape;
   - level_charge_counts_inline_bytes_per_level and level_charge_example:
     proved negative control: the legacy per-push charge 3 * size counts the
     inline bytes of nested values once per nesting level.

   Results (D-B3): the worklist is a stack of chunks, chunk 0 with 4 slots
   and chunk j with 4 * 2^j; a full chunk is never reallocated and every
   chunk is kept until the walk ends, so an entry is written once and read
   once; the vector of chunks 1..k grows by doubling from 4 headers.
   - worklist_charge_independent_of_traversal_order: the worklist charge is
     a function of the number of entries;
   - worklist_charge_covers_peak and worklist_charge_covers_entries: for a
     peak of P entries the chunks and the chunk headers allocate at most 4
     slots of 16 bytes per entry, and the header moves of the doubling
     vector (one read and one write of 24 bytes each) take at most 8 bytes
     per entry;
   - rust_entry_constant_covers_header_moves: the Rust constant meets the
     hypothesis on E and its slack pays the header moves;
   - worklist_charge_covers_peak_for_slot and
     rust_depth_entry_constant_covers_header_moves (D-O6, DR-93): the same
     bounds for the 24-byte entries of a depth walk.

   Results (D-E1, DR-108): the first entry that a popped entry's children
   push reuses the popped slot, so only the root and the further entries
   reserve worklist backing (1 + charged_pushes ks for the run ks).
   - run_peak_le_charged_pushes and walk_peak_le_charged_pushes: the peak of
     every run is at most the root plus the further entries;
   - charged_pushes_independent_of_step_order;
   - charged_pushes_le_entries: never more than the backing of DR-92;
   - chain_slot_charge_covers_worklist and
     chain_slot_charge_covers_worklist_for_slot: the chained slot pays the
     chunks, the chunk headers and the header moves;
   - shared_release_charge_covers_work: a block-mode release of a
     store-owned shared pointer pays the pointer read and the strong-count
     read and write when the header is at least two words;
   - negative controls free_root_push_uncovered_example,
     per_entry_backing_overcharges_chain_example and
     shared_release_without_header_uncovered_example.

   Rust correspondence: shared/src/rust/clone_backing.rs (Walker in block
   mode: push_block_entry, block, referent_block, ChunkedWorklist; the
   *_blocks entry points; E = BLOCK_ENTRY_SCANNED = 2p + 3w with p = 16 and
   w = 8, so the slack of w per entry pays the header moves; the backing
   BLOCK_ENTRY_BACKING = 4 slots); tests in
   shared/src/rust/clone_backing/tests.rs. *)

From Stdlib Require Import Lists.List Arith.PeanoNat Lia Sorting.Permutation.
From CostAccountedRho Require Import ObservationReadCoverage.
Import ListNotations.

Inductive node :=
  | Field (size : nat)
  | Empty (size : nat)
  | Entry (size : nat) (children : list node)
  | Block (bytes : nat) (visited : bool) (children : list node).

Definition inline_size (n : node) : nat :=
  match n with Field s => s | Empty s => s | Entry s _ => s | Block _ _ _ => 0 end.

Definition children_inline (cs : list node) : nat :=
  fold_right (fun c total => inline_size c + total) 0 cs.

Definition is_field (n : node) : Prop := match n with Field _ => True | _ => False end.

Fixpoint well_formed (n : node) : Prop :=
  match n with
  | Field _ => True
  | Empty _ => True
  | Entry s cs =>
      children_inline cs <= s /\ fold_right (fun c rest => well_formed c /\ rest) True cs
  | Block b v cs =>
      children_inline cs <= b /\
      (v = false -> fold_right (fun c rest => is_field c /\ rest) True cs) /\
      fold_right (fun c rest => well_formed c /\ rest) True cs
  end.

(* The walker's reads of an entry's own bytes, over a node's inline region
   (they stop at blocks, which are regions of their own). *)
Fixpoint inline_visit_reads (n : node) : nat :=
  match n with
  | Field _ => 0
  | Empty s => s
  | Entry s cs =>
      (s - children_inline cs) + fold_right (fun c total => inline_visit_reads c + total) 0 cs
  | Block _ _ _ => 0
  end.

Lemma inline_reads_le_inline_size : forall n, well_formed n -> inline_visit_reads n <= inline_size n.
Proof.
  fix IH 1. intros [s | s | s cs | b v cs] wf; cbn in *.
  - lia.
  - lia.
  - destruct wf as [fits children_wf]. unfold children_inline in *.
    assert (children : fold_right (fun c total => inline_visit_reads c + total) 0 cs
                       <= fold_right (fun c total => inline_size c + total) 0 cs).
    { clear fits. induction cs as [| c rest IHrest]; cbn in *; [lia |].
      destruct children_wf as [c_wf rest_wf].
      pose proof (IH c c_wf). specialize (IHrest rest_wf). lia. }
    lia.
  - lia.
Qed.

Lemma children_reads_le_children_inline : forall cs,
  fold_right (fun c rest => well_formed c /\ rest) True cs ->
  fold_right (fun c total => inline_visit_reads c + total) 0 cs <= children_inline cs.
Proof.
  unfold children_inline. induction cs as [| c rest IHrest]; cbn; [lia |].
  intros [c_wf rest_wf]. pose proof (inline_reads_le_inline_size c c_wf).
  specialize (IHrest rest_wf). lia.
Qed.

Theorem visit_reads_le_block_bytes : forall b v cs,
  well_formed (Block b v cs) ->
  fold_right (fun c total => inline_visit_reads c + total) 0 cs <= b.
Proof.
  intros b v cs [fits [_ children_wf]].
  pose proof (children_reads_le_children_inline cs children_wf). lia.
Qed.

Lemma fields_have_no_visit_reads : forall cs,
  fold_right (fun c rest => is_field c /\ rest) True cs ->
  fold_right (fun c total => inline_visit_reads c + total) 0 cs = 0.
Proof.
  induction cs as [| c rest IHrest]; cbn; [reflexivity |].
  intros [field rest_fields]. destruct c as [s | s | s cs | b v cs]; cbn in field; try contradiction.
  cbn. rewrite (IHrest rest_fields). reflexivity.
Qed.

(* Sums over sibling lists do not depend on the siblings' order. *)
Lemma sum_permutation : forall (f : node -> nat) cs cs',
  Permutation cs cs' ->
  fold_right (fun c total => f c + total) 0 cs = fold_right (fun c total => f c + total) 0 cs'.
Proof. intros f cs cs' perm. induction perm; cbn; lia. Qed.

(* The clone as a trace: the sizes of the blocks it allocates, in pre-order
   (every block below the root). *)
Fixpoint clone_allocations (n : node) : list nat :=
  match n with
  | Field _ => []
  | Empty _ => []
  | Entry _ cs => flat_map clone_allocations cs
  | Block b _ cs => b :: flat_map clone_allocations cs
  end.

Definition allocated (n : node) : nat := fold_right Nat.add 0 (clone_allocations n).

Lemma allocation_sum_shift : forall l k, fold_right Nat.add k l = fold_right Nat.add 0 l + k.
Proof. induction l as [| x rest IH]; intros k; cbn; [lia | rewrite IH; lia]. Qed.

Lemma allocation_sum_app : forall l1 l2,
  fold_right Nat.add 0 (l1 ++ l2) = fold_right Nat.add 0 l1 + fold_right Nat.add 0 l2.
Proof. intros l1 l2. rewrite fold_right_app, allocation_sum_shift. lia. Qed.

Section Charges.

Variables E F p w : nat.
Hypothesis entry_covers : 2 * p + 2 * w <= E.
Hypothesis field_covers : w <= F.

Fixpoint block_charge (n : node) : nat :=
  match n with
  | Field _ => F
  | Empty _ => F
  | Entry _ cs => E + fold_right (fun c total => block_charge c + total) 0 cs
  | Block b v cs => (if v then 2 * b else b) + fold_right (fun c total => block_charge c + total) 0 cs
  end.

Fixpoint copy_charge (n : node) : nat :=
  match n with
  | Field _ => F
  | Empty _ => F
  | Entry _ cs => E + fold_right (fun c total => copy_charge c + total) 0 cs
  | Block b v cs => (if v then 3 * b else 2 * b) + fold_right (fun c total => copy_charge c + total) 0 cs
  end.

Fixpoint copy_backing (n : node) : nat :=
  match n with
  | Field _ => 0
  | Empty _ => 0
  | Entry _ cs => fold_right (fun c total => copy_backing c + total) 0 cs
  | Block b _ cs => b + fold_right (fun c total => copy_backing c + total) 0 cs
  end.

Definition root_inspection (n : node) : nat :=
  match n with Field s => s | Empty s => 2 * s | Entry s _ => 2 * s | Block _ _ _ => 0 end.

Definition root_copy (n : node) : nat :=
  match n with Field s => 2 * s | Empty s => 3 * s | Entry s _ => 3 * s | Block _ _ _ => 0 end.

Definition inspection_charge (n : node) : nat := root_inspection n + block_charge n.
Definition full_copy_charge (n : node) : nat := root_copy n + copy_charge n.

(* The walker's reads: worklist moves, own bytes and one re-read word per
   entry. *)
Fixpoint walk_reads (n : node) : nat :=
  match n with
  | Field _ => 0
  | Empty s => s
  | Entry s cs =>
      2 * p + w + (s - children_inline cs) + fold_right (fun c total => walk_reads c + total) 0 cs
  | Block _ _ cs => fold_right (fun c total => walk_reads c + total) 0 cs
  end.

(* One linear traversal: every block once, one re-read word per node. *)
Fixpoint traversal_reads (n : node) : nat :=
  match n with
  | Field _ => w
  | Empty _ => w
  | Entry _ cs => w + fold_right (fun c total => traversal_reads c + total) 0 cs
  | Block b _ cs => b + fold_right (fun c total => traversal_reads c + total) 0 cs
  end.

Lemma node_covers : forall n, well_formed n ->
  walk_reads n + traversal_reads n <= block_charge n + inline_visit_reads n.
Proof.
  fix IH 1. intros [s | s | s cs | b v cs] wf; cbn in wf |- *.
  - lia.
  - lia.
  - destruct wf as [_ children_wf].
    assert (children : fold_right (fun c total => walk_reads c + total) 0 cs
                       + fold_right (fun c total => traversal_reads c + total) 0 cs
                       <= fold_right (fun c total => block_charge c + total) 0 cs
                          + fold_right (fun c total => inline_visit_reads c + total) 0 cs).
    { induction cs as [| c rest IHrest]; cbn in *; [lia |].
      destruct children_wf as [c_wf rest_wf].
      pose proof (IH c c_wf). specialize (IHrest rest_wf). lia. }
    lia.
  - pose proof (visit_reads_le_block_bytes b v cs wf) as partition.
    destruct wf as [_ [opaque children_wf]].
    assert (children : fold_right (fun c total => walk_reads c + total) 0 cs
                       + fold_right (fun c total => traversal_reads c + total) 0 cs
                       <= fold_right (fun c total => block_charge c + total) 0 cs
                          + fold_right (fun c total => inline_visit_reads c + total) 0 cs).
    { clear partition opaque. induction cs as [| c rest IHrest]; cbn in *; [lia |].
      destruct children_wf as [c_wf rest_wf].
      pose proof (IH c c_wf). specialize (IHrest rest_wf). lia. }
    destruct v; [lia |].
    rewrite (fields_have_no_visit_reads cs (opaque eq_refl)) in children. lia.
Qed.

Theorem block_inspection_covers_walk_and_traversal : forall n, well_formed n ->
  walk_reads n + inline_size n + traversal_reads n <= inspection_charge n.
Proof.
  intros n wf. unfold inspection_charge.
  pose proof (node_covers n wf). pose proof (inline_reads_le_inline_size n wf).
  destruct n as [s | s | s cs | b v cs]; cbn [root_inspection inline_size inline_visit_reads] in *; lia.
Qed.

Lemma node_copy_covers : forall n, well_formed n ->
  walk_reads n + traversal_reads n + allocated n <= copy_charge n + inline_visit_reads n.
Proof.
  unfold allocated. fix IH 1. intros [s | s | s cs | b v cs] wf; cbn in wf |- *.
  - lia.
  - lia.
  - destruct wf as [_ children_wf].
    assert (children : fold_right (fun c total => walk_reads c + total) 0 cs
                       + fold_right (fun c total => traversal_reads c + total) 0 cs
                       + fold_right Nat.add 0 (flat_map clone_allocations cs)
                       <= fold_right (fun c total => copy_charge c + total) 0 cs
                          + fold_right (fun c total => inline_visit_reads c + total) 0 cs).
    { induction cs as [| c rest IHrest]; cbn in *; [lia |].
      destruct children_wf as [c_wf rest_wf]. rewrite allocation_sum_app.
      pose proof (IH c c_wf). specialize (IHrest rest_wf). lia. }
    lia.
  - pose proof (visit_reads_le_block_bytes b v cs wf) as partition.
    destruct wf as [_ [opaque children_wf]].
    assert (children : fold_right (fun c total => walk_reads c + total) 0 cs
                       + fold_right (fun c total => traversal_reads c + total) 0 cs
                       + fold_right Nat.add 0 (flat_map clone_allocations cs)
                       <= fold_right (fun c total => copy_charge c + total) 0 cs
                          + fold_right (fun c total => inline_visit_reads c + total) 0 cs).
    { clear partition opaque. induction cs as [| c rest IHrest]; cbn in *; [lia |].
      destruct children_wf as [c_wf rest_wf]. rewrite allocation_sum_app.
      pose proof (IH c c_wf). specialize (IHrest rest_wf). lia. }
    destruct v; [lia |].
    rewrite (fields_have_no_visit_reads cs (opaque eq_refl)) in children. lia.
Qed.

(* The clone reads the root's inline bytes and writes them into its
   destination. *)
Theorem block_copy_covers_walk_and_clone : forall n, well_formed n ->
  walk_reads n + 2 * inline_size n + traversal_reads n + allocated n <= full_copy_charge n.
Proof.
  intros n wf. unfold full_copy_charge.
  pose proof (node_copy_covers n wf). pose proof (inline_reads_le_inline_size n wf).
  destruct n as [s | s | s cs | b v cs]; cbn [root_copy inline_size inline_visit_reads] in *; lia.
Qed.

(* The backing charge is the total of the clone's allocation trace. *)
Lemma backing_is_allocated : forall n, copy_backing n = allocated n.
Proof.
  unfold allocated. fix IH 1. intros [s | s | s cs | b v cs]; cbn; [reflexivity | reflexivity | |].
  - induction cs as [| c rest IHrest]; cbn; [reflexivity |].
    rewrite allocation_sum_app, (IH c), IHrest. reflexivity.
  - assert (children : fold_right (fun c total => copy_backing c + total) 0 cs
                       = fold_right Nat.add 0 (flat_map clone_allocations cs)).
    { induction cs as [| c rest IHrest]; cbn; [reflexivity |].
      rewrite allocation_sum_app, (IH c), IHrest. reflexivity. }
    rewrite children. reflexivity.
Qed.

Theorem block_copy_backing_covers_allocation : forall n, allocated n <= copy_backing n.
Proof. intros n. rewrite backing_is_allocated. lia. Qed.

Lemma node_inspection_le_copy : forall n, block_charge n <= copy_charge n.
Proof.
  fix IH 1. intros [s | s | s cs | b v cs]; cbn.
  - lia.
  - lia.
  - assert (children : fold_right (fun c total => block_charge c + total) 0 cs
                       <= fold_right (fun c total => copy_charge c + total) 0 cs).
    { induction cs as [| c rest IHrest]; cbn; [lia |]. pose proof (IH c). lia. }
    lia.
  - assert (children : fold_right (fun c total => block_charge c + total) 0 cs
                       <= fold_right (fun c total => copy_charge c + total) 0 cs).
    { induction cs as [| c rest IHrest]; cbn; [lia |]. pose proof (IH c). lia. }
    destruct v; lia.
Qed.

Theorem block_inspection_fits_block_copy : forall n, inspection_charge n <= full_copy_charge n.
Proof.
  intros n. unfold inspection_charge, full_copy_charge.
  pose proof (node_inspection_le_copy n).
  destruct n as [s | s | s cs | b v cs]; cbn [root_inspection root_copy]; lia.
Qed.

Theorem block_charge_independent_of_sibling_order : forall s b v cs cs',
  Permutation cs cs' ->
  block_charge (Entry s cs) = block_charge (Entry s cs') /\
  block_charge (Block b v cs) = block_charge (Block b v cs') /\
  copy_charge (Entry s cs) = copy_charge (Entry s cs') /\
  copy_charge (Block b v cs) = copy_charge (Block b v cs').
Proof.
  intros s b v cs cs' perm. cbn.
  rewrite (sum_permutation block_charge cs cs' perm), (sum_permutation copy_charge cs cs' perm).
  repeat split; reflexivity.
Qed.

(* The walk as an event trace: each node reserves its own charge and then
   performs its own reads; its children follow, depth first. *)
Definition own_charge (n : node) : nat :=
  match n with
  | Field _ => F
  | Empty _ => F
  | Entry _ _ => E
  | Block b v _ => if v then 2 * b else b
  end.

Definition own_walk_read (n : node) : nat :=
  match n with
  | Field _ => 0
  | Empty s => s
  | Entry s cs => 2 * p + w + (s - children_inline cs)
  | Block _ _ _ => 0
  end.

Fixpoint walk_trace (n : node) : list event :=
  match n with
  | Field _ => [Reserve (own_charge n); Read (own_walk_read n)]
  | Empty _ => [Reserve (own_charge n); Read (own_walk_read n)]
  | Entry _ cs => [Reserve (own_charge n); Read (own_walk_read n)] ++ flat_map walk_trace cs
  | Block _ _ cs => [Reserve (own_charge n); Read (own_walk_read n)] ++ flat_map walk_trace cs
  end.

Lemma reserve_then_read_covered : forall slack charge read,
  read <= slack + charge -> covered_from slack [Reserve charge; Read read].
Proof.
  intros slack charge read fits prefix suffix split.
  destruct prefix as [| first [| second [| third rest]]]; cbn in split.
  - unfold total. cbn. lia.
  - injection split as first_eq _. subst first. unfold total. cbn. lia.
  - injection split as first_eq second_eq _. subst first second. unfold total. cbn. lia.
  - discriminate.
Qed.

Lemma pair_total_reads : forall charge read,
  total read_units [Reserve charge; Read read] = read.
Proof. intros. unfold total. cbn. lia. Qed.

Lemma pair_total_reserves : forall charge read,
  total reserved_units [Reserve charge; Read read] = charge.
Proof. intros. unfold total. cbn. lia. Qed.

Lemma trace_totals : forall n,
  total read_units (walk_trace n) = walk_reads n /\
  total reserved_units (walk_trace n) = block_charge n.
Proof.
  fix IH 1. intros [s | s | s cs | b v cs]; cbn [walk_trace walk_reads block_charge].
  - rewrite pair_total_reads, pair_total_reserves. cbn. split; reflexivity.
  - rewrite pair_total_reads, pair_total_reserves. cbn. split; reflexivity.
  - assert (children : forall cs',
      total read_units (flat_map walk_trace cs')
      = fold_right (fun c total => walk_reads c + total) 0 cs' /\
      total reserved_units (flat_map walk_trace cs')
      = fold_right (fun c total => block_charge c + total) 0 cs').
    { induction cs' as [| c rest IHrest]; [unfold total; cbn; split; reflexivity |].
      change (flat_map walk_trace (c :: rest)) with (walk_trace c ++ flat_map walk_trace rest).
      rewrite !total_app. destruct (IH c) as [reads reserves]. destruct IHrest as [rest_reads rest_reserves].
      cbn. split; lia. }
    destruct (children cs) as [reads reserves].
    rewrite !total_app, pair_total_reads, pair_total_reserves. cbn. split; lia.
  - assert (children : forall cs',
      total read_units (flat_map walk_trace cs')
      = fold_right (fun c total => walk_reads c + total) 0 cs' /\
      total reserved_units (flat_map walk_trace cs')
      = fold_right (fun c total => block_charge c + total) 0 cs').
    { induction cs' as [| c rest IHrest]; [unfold total; cbn; split; reflexivity |].
      change (flat_map walk_trace (c :: rest)) with (walk_trace c ++ flat_map walk_trace rest).
      rewrite !total_app. destruct (IH c) as [reads reserves]. destruct IHrest as [rest_reads rest_reserves].
      cbn. split; lia. }
    destruct (children cs) as [reads reserves].
    rewrite !total_app, pair_total_reads, pair_total_reserves. cbn. split; lia.
Qed.

(* Starting with slack for the walker's reads of its own inline region, a
   node's walk is covered at every prefix. *)
Lemma walk_trace_covered : forall n slack,
  well_formed n -> inline_visit_reads n <= slack -> covered_from slack (walk_trace n).
Proof.
  fix IH 1. intros n slack wf enough.
  assert (children : forall cs slack',
    fold_right (fun c rest => well_formed c /\ rest) True cs ->
    fold_right (fun c total => inline_visit_reads c + total) 0 cs <= slack' ->
    covered_from slack' (flat_map walk_trace cs)).
  { induction cs as [| c rest IHrest]; intros slack' children_wf room.
    - intros prefix suffix split. destruct prefix; [unfold total; cbn; lia | discriminate].
    - cbn in children_wf, room. destruct children_wf as [c_wf rest_wf].
      change (flat_map walk_trace (c :: rest)) with (walk_trace c ++ flat_map walk_trace rest).
      destruct (trace_totals c) as [reads reserves].
      pose proof (node_covers c c_wf) as c_covers.
      apply covered_from_app.
      + apply IH; [exact c_wf | lia].
      + rewrite reads, reserves. lia.
      + apply IHrest; [exact rest_wf |]. rewrite reads, reserves. lia. }
  destruct n as [s | s | s cs | b v cs]; cbn [walk_trace].
  - apply reserve_then_read_covered. cbn. lia.
  - apply reserve_then_read_covered. cbn in enough |- *. lia.
  - cbn in wf, enough. destruct wf as [_ children_wf].
    apply covered_from_app.
    + apply reserve_then_read_covered. cbn. lia.
    + rewrite pair_total_reads, pair_total_reserves. cbn. lia.
    + apply children; [exact children_wf |].
      rewrite pair_total_reads, pair_total_reserves. cbn. lia.
  - pose proof (visit_reads_le_block_bytes b v cs wf) as partition.
    cbn in wf. destruct wf as [_ [opaque children_wf]].
    apply covered_from_app.
    + apply reserve_then_read_covered. cbn. lia.
    + rewrite pair_total_reads, pair_total_reserves. cbn. lia.
    + apply children; [exact children_wf |].
      rewrite pair_total_reads, pair_total_reserves. cbn.
      destruct v; [lia |]. rewrite (fields_have_no_visit_reads cs (opaque eq_refl)). lia.
Qed.

(* The root block is reserved first; the consumer's traversal follows the
   walk. *)
Theorem block_trace_covered : forall n, well_formed n ->
  covered ([Reserve (root_inspection n)] ++ walk_trace n ++ [Read (inline_size n + traversal_reads n)]).
Proof.
  intros n wf. unfold covered.
  pose proof (inline_reads_le_inline_size n wf) as region.
  pose proof (node_covers n wf) as covers.
  destruct (trace_totals n) as [reads reserves].
  assert (root_room : inline_visit_reads n + inline_size n <= root_inspection n).
  { destruct n as [s | s | s cs | b v cs]; cbn [root_inspection inline_size inline_visit_reads] in *; lia. }
  apply covered_from_app.
  - change [Reserve (root_inspection n)] with (map Reserve [root_inspection n]). apply reserves_covered.
  - unfold total. cbn. lia.
  - apply covered_from_app.
    + apply walk_trace_covered; [exact wf |]. unfold total. cbn. lia.
    + rewrite reads, reserves. unfold total. cbn. lia.
    + change [Read (inline_size n + traversal_reads n)] with (map Read [inline_size n + traversal_reads n]).
      apply reads_covered. rewrite reads, reserves. unfold total. cbn. lia.
Qed.

End Charges.

(* D-B3: the worklist charge. *)
Fixpoint entries (n : node) : nat :=
  match n with
  | Field _ => 0
  | Empty _ => 0
  | Entry _ cs => 1 + fold_right (fun c total => entries c + total) 0 cs
  | Block _ _ cs => fold_right (fun c total => entries c + total) 0 cs
  end.

Theorem worklist_charge_independent_of_traversal_order : forall cs cs',
  Permutation cs cs' ->
  fold_right (fun c total => entries c + total) 0 cs
  = fold_right (fun c total => entries c + total) 0 cs'.
Proof. intros cs cs' perm. exact (sum_permutation entries cs cs' perm). Qed.

(* The slots of chunks 0..k: chunk 0 holds 4 and chunk j holds 4 * 2^j. *)
Fixpoint chunk_slots (k : nat) : nat :=
  match k with 0 => 4 | S k' => chunk_slots k' + 4 * 2 ^ (S k') end.

Lemma chunk_slots_closed : forall k, chunk_slots k + 4 = 8 * 2 ^ k.
Proof.
  induction k as [| k IH]; cbn [chunk_slots]; [cbn; lia |].
  rewrite Nat.pow_succ_r'. lia.
Qed.

(* The vector of chunk headers grows by doubling from 4 (Rust's RawVec for
   24-byte elements): the capacities it allocates until one holds `needed`,
   and the headers it moves (each growth moves the old capacity). *)
Fixpoint doubling_capacities (fuel capacity needed : nat) : list nat :=
  match fuel with
  | 0 => [capacity]
  | S fuel' => if needed <=? capacity then [capacity]
               else capacity :: doubling_capacities fuel' (2 * capacity) needed
  end.

Fixpoint doubling_moves (fuel capacity needed : nat) : nat :=
  match fuel with
  | 0 => 0
  | S fuel' => if needed <=? capacity then 0
               else capacity + doubling_moves fuel' (2 * capacity) needed
  end.

Lemma doubling_sum_bound : forall fuel capacity needed,
  fold_right Nat.add 0 (doubling_capacities fuel capacity needed) + capacity
  <= 2 * Nat.max capacity (2 * needed - 2).
Proof.
  induction fuel as [| fuel IH]; intros capacity needed; cbn [doubling_capacities].
  - cbn [fold_right]. lia.
  - destruct (Nat.leb_spec needed capacity) as [fits | grows]; cbn [fold_right]; [lia |].
    pose proof (IH (2 * capacity) needed). lia.
Qed.

Lemma doubling_moves_bound : forall fuel capacity needed,
  doubling_moves fuel capacity needed + capacity <= Nat.max capacity (2 * needed - 2).
Proof.
  induction fuel as [| fuel IH]; intros capacity needed; cbn [doubling_moves]; [lia |].
  destruct (Nat.leb_spec needed capacity) as [fits | grows]; [lia |].
  pose proof (IH (2 * capacity) needed). lia.
Qed.

Lemma exponential_dominates : forall k, 1 <= k -> 96 * k + 128 <= 128 * 2 ^ k.
Proof.
  induction k as [| k IH]; intros positive; [lia |].
  rewrite Nat.pow_succ_r'.
  destruct k as [| k]; [cbn; lia |].
  pose proof (IH ltac:(lia)). lia.
Qed.

Lemma exponential_dominates_moves : forall k, 3 * k <= 2 ^ k + 8.
Proof.
  induction k as [| k IH]; [cbn; lia |].
  rewrite Nat.pow_succ_r'.
  destruct k as [| k]; [cbn; lia |].
  assert (2 <= 2 ^ S k).
  { replace 2 with (2 ^ 1) at 1 by reflexivity. apply Nat.pow_le_mono_r; lia. }
  lia.
Qed.

(* Allocation and header-move bounds for a peak that reaches chunk k (k = 0,
   or the chunks below k were full). *)
Definition worklist_allocated_bytes (fuel k : nat) : nat :=
  16 * chunk_slots k
  + (if k =? 0 then 0 else 24 * fold_right Nat.add 0 (doubling_capacities fuel 4 k)).

Definition worklist_moved_bytes (fuel k : nat) : nat :=
  if k =? 0 then 0 else 48 * doubling_moves fuel 4 k.

Theorem worklist_charge_covers_peak : forall fuel k peak,
  (k = 0 /\ 1 <= peak) \/ (1 <= k /\ 4 * (2 ^ k - 1) < peak) ->
  worklist_allocated_bytes fuel k <= 64 * peak /\
  worklist_moved_bytes fuel k <= 8 * peak.
Proof.
  intros fuel k peak [[zero positive] | [positive reached]];
    unfold worklist_allocated_bytes, worklist_moved_bytes.
  - subst k. cbn. lia.
  - destruct (Nat.eqb_spec k 0) as [zero | nonzero]; [lia |].
    pose proof (chunk_slots_closed k) as slots.
    pose proof (doubling_sum_bound fuel 4 k) as headers.
    pose proof (doubling_moves_bound fuel 4 k) as moves.
    pose proof (exponential_dominates k positive) as dominates.
    pose proof (exponential_dominates_moves k) as dominates_moves.
    assert (power : 2 <= 2 ^ k).
    { replace 2 with (2 ^ 1) at 1 by reflexivity. apply Nat.pow_le_mono_r; lia. }
    split; lia.
Qed.

(* The peak never exceeds the entries the walk pushes, so the per-entry
   worklist constants pay the worklist. *)
Corollary worklist_charge_covers_entries : forall fuel k peak entry_count,
  (k = 0 /\ 1 <= peak) \/ (1 <= k /\ 4 * (2 ^ k - 1) < peak) ->
  peak <= entry_count ->
  worklist_allocated_bytes fuel k <= 64 * entry_count /\
  worklist_moved_bytes fuel k <= 8 * entry_count.
Proof.
  intros fuel k peak entry_count reach bounded.
  destruct (worklist_charge_covers_peak fuel k peak reach). lia.
Qed.

(* The Rust constant: E = 2p + 3w with p = 16 and w = 8 meets the
   hypothesis E >= 2p + 2w with one word of slack per entry, and the slack
   pays the header moves. *)
Corollary rust_entry_constant_covers_header_moves : forall fuel k peak entry_count,
  (k = 0 /\ 1 <= peak) \/ (1 <= k /\ 4 * (2 ^ k - 1) < peak) ->
  peak <= entry_count ->
  2 * 16 + 2 * 8 <= 2 * 16 + 3 * 8 /\
  (2 * 16 + 2 * 8) * entry_count + worklist_moved_bytes fuel k <= (2 * 16 + 3 * 8) * entry_count.
Proof.
  intros fuel k peak entry_count reach bounded.
  destruct (worklist_charge_covers_entries fuel k peak entry_count reach bounded). lia.
Qed.

(* D-O6 (DR-93): the depth walk keeps 24-byte entries (a value and its
   depth) in the same chunked worklist. The allocation bound holds for every
   entry size of at least 16 bytes; the header moves do not depend on it. *)
Definition worklist_allocated_bytes_for (slot fuel k : nat) : nat :=
  slot * chunk_slots k
  + (if k =? 0 then 0 else 24 * fold_right Nat.add 0 (doubling_capacities fuel 4 k)).

Theorem worklist_charge_covers_peak_for_slot : forall slot fuel k peak,
  16 <= slot ->
  (k = 0 /\ 1 <= peak) \/ (1 <= k /\ 4 * (2 ^ k - 1) < peak) ->
  worklist_allocated_bytes_for slot fuel k <= 4 * slot * peak.
Proof.
  intros slot fuel k peak wide [[zero positive] | [positive reached]];
    unfold worklist_allocated_bytes_for.
  - subst k. cbn [chunk_slots Nat.eqb]. nia.
  - destruct (Nat.eqb_spec k 0) as [zero | nonzero]; [lia |].
    pose proof (chunk_slots_closed k) as slots.
    pose proof (doubling_sum_bound fuel 4 k) as headers.
    pose proof (exponential_dominates k positive) as dominates.
    assert (power : 2 <= 2 ^ k).
    { replace 2 with (2 ^ 1) at 1 by reflexivity. apply Nat.pow_le_mono_r; lia. }
    remember (2 ^ k) as x eqn:x_def.
    remember (chunk_slots k) as slots_k eqn:slots_def.
    remember (fold_right Nat.add 0 (doubling_capacities fuel 4 k)) as header_sum eqn:header_def.
    assert (header_bound : header_sum <= 4 * k) by lia.
    assert (chunk_bytes : slot * slots_k + 4 * slot = 8 * (slot * x)).
    { assert (scaled : slot * (slots_k + 4) = slot * (8 * x)) by (rewrite slots; reflexivity).
      rewrite Nat.mul_add_distr_l in scaled. lia. }
    assert (peak_bytes : 16 * (slot * x) <= 4 * (slot * peak) + 12 * slot).
    { assert (4 * x <= peak + 3) by lia.
      replace (16 * (slot * x)) with (4 * (slot * (4 * x))) by lia.
      replace (4 * (slot * peak) + 12 * slot) with (4 * (slot * (peak + 3))) by lia.
      apply Nat.mul_le_mono_l, Nat.mul_le_mono_l. assumption. }
    assert (wide_bytes : 16 * x + slot <= slot * x + 16) by nia.
    lia.
Qed.

(* The Rust constant of a depth entry: E = 2p + 3w with p = 24 and w = 8. *)
Corollary rust_depth_entry_constant_covers_header_moves : forall fuel k peak entry_count,
  (k = 0 /\ 1 <= peak) \/ (1 <= k /\ 4 * (2 ^ k - 1) < peak) ->
  peak <= entry_count ->
  2 * 24 + 2 * 8 <= 2 * 24 + 3 * 8 /\
  (2 * 24 + 2 * 8) * entry_count + worklist_moved_bytes fuel k <= (2 * 24 + 3 * 8) * entry_count /\
  worklist_allocated_bytes_for 24 fuel k <= 4 * 24 * entry_count.
Proof.
  intros fuel k peak entry_count reach bounded.
  destruct (worklist_charge_covers_entries fuel k peak entry_count reach bounded) as [_ moves].
  pose proof (worklist_charge_covers_peak_for_slot 24 fuel k peak ltac:(lia) reach). nia.
Qed.

(* Negative control: a chain of d entries of size s, each nested inline in
   the previous one. The legacy per-push charge 3 * size counts the same
   bytes at every level; the block charge counts them once (in the root's
   referent block) plus E per level. *)
Fixpoint nested_chain (depth size : nat) : node :=
  match depth with
  | 0 => Field size
  | S depth' => Entry size [nested_chain depth' size]
  end.

Fixpoint legacy_charge (n : node) : nat :=
  match n with
  | Field s => 3 * s
  | Empty s => 3 * s
  | Entry s cs => 3 * s + fold_right (fun c total => legacy_charge c + total) 0 cs
  | Block b _ cs => 2 * b + fold_right (fun c total => legacy_charge c + total) 0 cs
  end.

Theorem level_charge_counts_inline_bytes_per_level : forall depth size,
  3 * (depth + 1) * size <= legacy_charge (nested_chain depth size).
Proof.
  induction depth as [| depth IH]; intros size; cbn [nested_chain legacy_charge fold_right]; [lia |].
  pose proof (IH size). nia.
Qed.

Lemma nested_chain_well_formed : forall depth size, well_formed (nested_chain depth size).
Proof.
  induction depth as [| depth IH]; intros size; cbn; [exact I |].
  unfold children_inline. cbn. split; [| split; [apply IH | exact I]].
  destruct depth; cbn; lia.
Qed.

(* d = 3, s = 600 with the Rust constants E = 56 and F = 8. *)
Example level_charge_example :
  legacy_charge (nested_chain 3 600) = 3 * 4 * 600 /\
  inspection_charge 56 8 (nested_chain 3 600) = 2 * 600 + 3 * 56 + 8 /\
  inspection_charge 56 8 (nested_chain 3 600) < legacy_charge (nested_chain 3 600).
Proof.
  split; [vm_compute; reflexivity |]. split; [vm_compute; reflexivity |].
  apply Nat.ltb_lt. vm_compute. reflexivity.
Qed.

(* D-E1 (DR-108): the chained worklist slot. The walk pops one entry, and the
   popped entry's children push k entries; the first of them reuses the
   popped slot. The run of a walk is the list of k for the popped entries,
   in pop order. Only the root and, for each popped entry, the entries after
   the first reserve worklist backing: 1 + charged_pushes ks entries. *)
Fixpoint charged_pushes (ks : list nat) : nat :=
  match ks with
  | [] => 0
  | k :: rest => (k - 1) + charged_pushes rest
  end.

(* The highest height of the worklist from height h: each step pops one
   entry and pushes k. *)
Fixpoint run_peak (h : nat) (ks : list nat) : nat :=
  match ks with
  | [] => h
  | k :: rest => Nat.max h (run_peak (h - 1 + k) rest)
  end.

Theorem run_peak_le_charged_pushes : forall ks h c,
  h <= 1 + c -> run_peak h ks <= 1 + c + charged_pushes ks.
Proof.
  induction ks as [| k rest IH]; intros h c bounded; cbn [run_peak charged_pushes]; [lia |].
  pose proof (IH (h - 1 + k) (c + (k - 1)) ltac:(lia)) as rest_bound.
  apply Nat.max_lub; lia.
Qed.

(* A walk starts with the root alone on the worklist. *)
Corollary walk_peak_le_charged_pushes : forall ks,
  run_peak 1 ks <= 1 + charged_pushes ks.
Proof. intros ks. pose proof (run_peak_le_charged_pushes ks 1 0 ltac:(lia)). lia. Qed.

(* The charge counts each popped entry's k, so it does not depend on the
   order in which the walk pops the entries. *)
Theorem charged_pushes_independent_of_step_order : forall ks ks',
  Permutation ks ks' -> charged_pushes ks = charged_pushes ks'.
Proof. intros ks ks' perm. induction perm; cbn [charged_pushes]; lia. Qed.

(* DR-92 reserved backing for the root and every pushed entry. *)
Theorem charged_pushes_le_entries : forall ks,
  1 + charged_pushes ks <= 1 + fold_right Nat.add 0 ks.
Proof. induction ks as [| k rest IH]; cbn [charged_pushes fold_right]; lia. Qed.

(* The chained slot pays the worklist: the bounds of worklist_charge_covers_
   entries hold for the root and the further entries. *)
Theorem chain_slot_charge_covers_worklist : forall fuel k ks,
  (k = 0 /\ 1 <= run_peak 1 ks) \/ (1 <= k /\ 4 * (2 ^ k - 1) < run_peak 1 ks) ->
  worklist_allocated_bytes fuel k <= 64 * (1 + charged_pushes ks) /\
  worklist_moved_bytes fuel k <= 8 * (1 + charged_pushes ks).
Proof.
  intros fuel k ks reach.
  exact (worklist_charge_covers_entries fuel k (run_peak 1 ks) (1 + charged_pushes ks)
           reach (walk_peak_le_charged_pushes ks)).
Qed.

(* The same bound for the 24-byte entries of a depth walk (DR-93). *)
Theorem chain_slot_charge_covers_worklist_for_slot : forall slot fuel k ks,
  16 <= slot ->
  (k = 0 /\ 1 <= run_peak 1 ks) \/ (1 <= k /\ 4 * (2 ^ k - 1) < run_peak 1 ks) ->
  worklist_allocated_bytes_for slot fuel k <= 4 * slot * (1 + charged_pushes ks).
Proof.
  intros slot fuel k ks wide reach.
  pose proof (worklist_charge_covers_peak_for_slot slot fuel k (run_peak 1 ks) wide reach)
    as covered.
  assert (4 * slot * run_peak 1 ks <= 4 * slot * (1 + charged_pushes ks))
    by (apply Nat.mul_le_mono_l; apply walk_peak_le_charged_pushes).
  lia.
Qed.

(* D-E1 (DR-108): the block-mode cleanup of n store-owned shared pointers.
   Each release reads the pointer (w bytes) and reads and writes the strong
   count (2w bytes). The charge is one field read and the header hdr for
   each pointer. *)
Definition shared_release_work (n w : nat) : nat := n * (w + 2 * w).
Definition shared_release_charge (n w hdr : nat) : nat := n * w + n * hdr.

Theorem shared_release_charge_covers_work : forall n w hdr,
  2 * w <= hdr -> shared_release_work n w <= shared_release_charge n w hdr.
Proof.
  intros n w hdr header. unfold shared_release_work, shared_release_charge.
  assert (n * (2 * w) <= n * hdr) by (apply Nat.mul_le_mono_l; exact header).
  nia.
Qed.

(* Negative control: if the root also reused a slot, a walk of one entry
   would reserve nothing, while chunk 0 allocates 4 slots of 16 bytes. *)
Example free_root_push_uncovered_example :
  charged_pushes [] = 0 /\ worklist_allocated_bytes 0 0 = 64 /\
  64 * charged_pushes [] < worklist_allocated_bytes 0 0.
Proof. split; [reflexivity | split; [reflexivity | cbn; lia]]. Qed.

(* Negative control: in a chain, each popped entry pushes one entry and the
   last none. The chained slot reserves one slot, the peak; DR-92 reserved
   one slot for each of the four entries. *)
Example per_entry_backing_overcharges_chain_example :
  1 + charged_pushes [1; 1; 1; 0] = 1 /\ run_peak 1 [1; 1; 1; 0] = 1 /\
  1 + fold_right Nat.add 0 [1; 1; 1; 0] = 4.
Proof. split; [reflexivity | split; reflexivity]. Qed.

(* Negative control: with w = 8, a release charged one field read without
   the header pays 8 bytes of its 24. *)
Example shared_release_without_header_uncovered_example :
  shared_release_charge 1 8 0 = 8 /\ shared_release_work 1 8 = 24 /\
  shared_release_charge 1 8 0 < shared_release_work 1 8.
Proof. split; [reflexivity | split; [reflexivity | cbn; lia]]. Qed.


Print Assumptions visit_reads_le_block_bytes.
Print Assumptions fields_have_no_visit_reads.
Print Assumptions block_inspection_covers_walk_and_traversal.
Print Assumptions block_copy_covers_walk_and_clone.
Print Assumptions block_copy_backing_covers_allocation.
Print Assumptions block_inspection_fits_block_copy.
Print Assumptions block_charge_independent_of_sibling_order.
Print Assumptions block_trace_covered.
Print Assumptions worklist_charge_independent_of_traversal_order.
Print Assumptions worklist_charge_covers_peak.
Print Assumptions worklist_charge_covers_entries.
Print Assumptions rust_entry_constant_covers_header_moves.
Print Assumptions worklist_charge_covers_peak_for_slot.
Print Assumptions rust_depth_entry_constant_covers_header_moves.
Print Assumptions level_charge_counts_inline_bytes_per_level.
Print Assumptions nested_chain_well_formed.
Print Assumptions level_charge_example.
Print Assumptions charged_pushes_independent_of_step_order.
Print Assumptions run_peak_le_charged_pushes.
Print Assumptions walk_peak_le_charged_pushes.
Print Assumptions charged_pushes_le_entries.
Print Assumptions chain_slot_charge_covers_worklist.
Print Assumptions chain_slot_charge_covers_worklist_for_slot.
Print Assumptions shared_release_charge_covers_work.
Print Assumptions free_root_push_uncovered_example.
Print Assumptions per_entry_backing_overcharges_chain_example.
Print Assumptions shared_release_without_header_uncovered_example.
