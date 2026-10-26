From Stdlib Require Import Bool List Arith Lia.
From Stdlib Require Import Sorting.Sorted.
From StackSafePDA Require ChannelImage.
From StackSafePDA Require Import GenerationCheckpointPublication IncrementalLedgerTrial
  IncrementalDomainComposition CreditedClosureLedger RetentionV2Format.

Import ListNotations.

(**
  Path-local retention V2 metadata editing.

  [RetentionV2Format] fixed the V2 retention representation: a constant-size
  anchor object in the domain [RSpaceDAG.RetA] that carries the current entry
  verbatim, the exact historical entry count and the root of a radix tree over
  48-byte keys, whose nodes reuse the canonical DAG node body under the domain
  [RSpaceDAG.RetR].  This module proves that the existing iterative radix
  editor of the persistent DAG ([rewrite.rs], [RadixEditor]) maintains that
  representation by editing one key path per step, so a successor never
  collects or re-encodes the history.

  Model.  A [radix_tree] is a compressed radix tree over byte keys.  A node
  consumes its [prefix], may end an entry ([endpoint = Some claim], the
  [Multiplicity] claim), and every child consumes its edge byte before its own
  prefix, as in [radix_unfold] and in the Rust editor.  [canonical width] is
  the shape rule the cold reader checks: a leaf ends a full-width key and has
  no children; an inner node is [Absent], shorter than the remaining width and
  has at least two children under strictly increasing edges; only the empty
  root has no entries.  [partition_build] is the independent rebuild: the
  longest common prefix of the sorted entries, then one child per next byte.
  The editor follows the Rust code step by step: [edit_node] is one
  compare-and-swap [(key, before, after)], with the split of a diverging prefix
  ([PrefixDiverged]), a fresh leaf for a missing edge and the CAS check of the
  endpoint; [edit_batch] applies a sorted batch, walking every CAS from the
  root, where [RadixEditor::apply] resumes at the last cursor step that ends
  within the longest common prefix of the previous and the current key; that
  step's path is a prefix of the current key, so resuming skips only nodes
  the batch has already opened, an equivalence that the Rust properties
  RV-30, RV-31 and RV-37 check and this module does not prove;
  [finalize_node] prunes childless [Absent] nodes and merges a unary [Absent]
  node into its child, also at the root ([finalize_root]).  Node objects are
  named by a universally
  quantified injective function (the content address); no naming function,
  store or key width is postulated.

  The development proves, without axioms:
  - T1: the partition build over sorted full-width entries is a canonical
    root denoting exactly those entries, and the only one, so a canonical
    image is determined by its entries; a shape rule that admits unary inner
    nodes gives one entry two images (T1m);
  - T2: on a canonical root a CAS batch is refused exactly when some CAS does
    not match the entry it meets, and otherwise yields the canonical rebuild
    of the updated entries (the empty root when none remain); a finalizer that
    does not prune keeps an empty leaf that is neither the rebuild nor
    canonical (T2m);
  - T3: one insertion or eviction changes only the nodes on the key's two
    search paths, at most [width + 1] nodes each, plus the one node split off
    or merged away, and every node is at most [radix_node_byte_bound width]
    bytes, so the bytes written depend on the key width alone; the V1 flat
    manifest grows by 56 bytes per entry and passes that bound (T3m);
  - T4: from the anchor of a valid V1 manifest, a commit (one insertion CAS
    of the outgoing current entry, whose key is above every historical key)
    and an eviction (one removal CAS) yield the anchor of the stepped
    manifest, which reads back as that manifest; its composition entries are
    [step_entries] of [IncrementalDomainComposition], the count comes from
    the CAS outcomes, and the metadata root delta [[A']] in, [[A]] out is
    authorized; counting inserted leaf objects instead undercounts a history
    whose new leaf equals an existing leaf object (T4m);
  - T5: the cold reader admits every canonical image: the empty image is
    named exactly by the empty radix node (the anchor rule), and every other
    image counts, by the plain and by the memoized count, exactly its entries
    and unfolds to them in key order; a finalizer that leaves a unary root is
    refused by all three passes of the reader (T5m);
  - T6: the trial of the metadata root change [[A']] in, [[A]] out, from the
    seeded predecessor, ends in the cold ledger of the new anchor, expands
    exactly the objects in one of the two closures, runs the two root events
    plus the expansion credit of those objects, and for one insertion or
    eviction spends at most [2 width + 7] root events and expansions;
    deactivating first re-expands the shared history (T6m);
  - T7: the metadata domain seeded with occurrence counts (the anchor's root
    multiplicity plus child occurrences across the distinct objects of the
    image) is the credited cold ledger of [CreditedClosureLedger] and a valid
    trial base; a seed that counts each object once lets the trial deactivate
    a leaf shared by two parents that the successor still references (T7m);
  - T8: the one-time V1 migration, one insertion per history entry from the
    empty root, is the canonical rebuild of the history with its exact count,
    denotes [migrate_v1] and agrees with a commit on the migrated anchor; a
    migration that also inserts the current entry reads back with the current
    entry in the history, which V1 refuses, and counts it twice among the
    retained roots (T8m).

  Crash atomicity of the successor (the commit's WAL transaction) and the
  old-reader fence are unchanged; the TLA+ modules
  [RSpaceAtomicCheckpointAuthority] and [RSpaceRetentionV2Upgrade] check them.
  Byte-level encodings are bound by [RetentionV2Format] and by the Rust
  properties RV-30 to RV-37.
*)

Module RetentionV2Editor.

Import RetentionV2Format.

(** * Compressed radix trees *)

(** A compressed radix tree over byte keys.  [prefix] is the run of key bytes
    the node consumes, [endpoint] is the [Multiplicity] claim of the entry whose
    key ends at the node, and every child consumes its edge byte before its own
    prefix, exactly as in [radix_unfold] and in the Rust editor, where
    [key[position]] is the edge and the child prefix starts at [position + 1]. *)
Inductive radix_tree : Type :=
| RadixTree (prefix : list nat) (endpoint : option nat)
    (children : list (nat * radix_tree)).

Definition empty_tree : radix_tree := RadixTree [] None [].

Section TreeInduction.
Variable holds : radix_tree -> Prop.
Hypothesis node_case :
  forall prefix endpoint children,
    Forall (fun child => holds (snd child)) children ->
    holds (RadixTree prefix endpoint children).

Fixpoint radix_tree_induction (tree : radix_tree) : holds tree :=
  match tree with
  | RadixTree prefix endpoint children =>
      node_case prefix endpoint children
        ((fix children_induction (rest : list (nat * radix_tree))
            : Forall (fun child => holds (snd child)) rest :=
            match rest with
            | [] => Forall_nil _
            | (edge, child) :: later =>
                @Forall_cons _ (fun child => holds (snd child)) (edge, child) later
                  (radix_tree_induction child) (children_induction later)
            end) children)
  end.
End TreeInduction.

Lemma forall_children_member :
  forall (holds : radix_tree -> Prop) (children : list (nat * radix_tree)) edge child,
    Forall (fun child => holds (snd child)) children ->
    In (edge, child) children -> holds child.
Proof.
  intros holds children edge child every member.
  rewrite Forall_forall in every. exact (every (edge, child) member).
Qed.

Fixpoint all_children (holds : radix_tree -> Prop) (children : list (nat * radix_tree))
    : Prop :=
  match children with
  | [] => True
  | (_, child) :: rest => holds child /\ all_children holds rest
  end.

Lemma all_children_iff :
  forall holds children,
    all_children holds children <->
      (forall edge child, In (edge, child) children -> holds child).
Proof.
  intros holds children. induction children as [|[edge child] rest IH];
    cbn [all_children In].
  - split; [intros _ edge child []|intros _; exact I].
  - rewrite IH. split.
    + intros [here later] other target [same|member].
      * injection same as same_edge same_child. subst. exact here.
      * exact (later other target member).
    + intros every. split.
      * exact (every edge child (or_introl eq_refl)).
      * intros other target member. exact (every other target (or_intror member)).
Qed.

(** Children are kept in strictly increasing edge order. *)
Definition edges_sorted (children : list (nat * radix_tree)) : Prop :=
  StronglySorted (fun left right => fst left < fst right) children.

(** ** Denotation: entries and lookups *)

Definition prepend_entry (bytes : list nat) (entry : list nat * nat) : list nat * nat :=
  (bytes ++ fst entry, snd entry).

Definition endpoint_entries (prefix : list nat) (endpoint : option nat)
    : list (list nat * nat) :=
  match endpoint with
  | Some claim => [(prefix, claim)]
  | None => []
  end.

Fixpoint children_entries (entries_of : radix_tree -> list (list nat * nat))
    (children : list (nat * radix_tree)) : list (list nat * nat) :=
  match children with
  | [] => []
  | (edge, child) :: rest =>
      map (prepend_entry [edge]) (entries_of child) ++ children_entries entries_of rest
  end.

(** [tree_entries] lists the (key, claim) entries in depth-first order, which
    is the order in which [radix_unfold] enumerates them. *)
Fixpoint tree_entries (tree : radix_tree) : list (list nat * nat) :=
  match tree with
  | RadixTree prefix endpoint children =>
      endpoint_entries prefix endpoint ++
        map (prepend_entry prefix) (children_entries tree_entries children)
  end.

Fixpoint strip_prefix (prefix key : list nat) : option (list nat) :=
  match prefix, key with
  | [], rest => Some rest
  | byte :: prefix_rest, next :: key_rest =>
      if Nat.eqb byte next then strip_prefix prefix_rest key_rest else None
  | _ :: _, [] => None
  end.

Fixpoint child_lookup (lookup_in : radix_tree -> option nat) (edge : nat)
    (children : list (nat * radix_tree)) : option nat :=
  match children with
  | [] => None
  | (other, child) :: rest =>
      if Nat.eqb edge other then lookup_in child else child_lookup lookup_in edge rest
  end.

Fixpoint tree_lookup (tree : radix_tree) (key : list nat) : option nat :=
  match tree with
  | RadixTree prefix endpoint children =>
      match strip_prefix prefix key with
      | None => None
      | Some [] => endpoint
      | Some (edge :: rest) =>
          child_lookup (fun child => tree_lookup child rest) edge children
      end
  end.

Fixpoint entry_lookup (entries : list (list nat * nat)) (key : list nat) : option nat :=
  match entries with
  | [] => None
  | (candidate, claim) :: rest =>
      if bytes_eqb candidate key then Some claim else entry_lookup rest key
  end.

(** ** Shapes *)

(** The cold reader's shape rules: a leaf consumes the whole remaining width
    and carries a claim; an inner node carries no claim, consumes less than the
    remaining width and has at least two children with strictly increasing
    edges.  The empty node is allowed at the root only. *)
Fixpoint canonical (width : nat) (tree : radix_tree) : Prop :=
  match tree with
  | RadixTree prefix (Some _) [] => length prefix = width
  | RadixTree _ (Some _) (_ :: _) => False
  | RadixTree prefix None children =>
      length prefix < width /\ 2 <= length children /\ edges_sorted children /\
      all_children (canonical (width - S (length prefix))) children
  end.

Definition canonical_root (width : nat) (tree : radix_tree) : Prop :=
  tree = empty_tree \/ canonical width tree.

(** The editor's invariant between CAS steps: keys have the fixed width, so a
    node that consumes the whole width has no children and a node that does not
    carries no claim; empty leaves, empty inner nodes and unary inner nodes are
    allowed until finalization. *)
Fixpoint fixed_width (width : nat) (tree : radix_tree) : Prop :=
  match tree with
  | RadixTree prefix endpoint children =>
      length prefix <= width /\
      (length prefix = width -> children = []) /\
      (length prefix < width -> endpoint = None) /\
      edges_sorted children /\
      all_children (fixed_width (width - S (length prefix))) children
  end.

Lemma canonical_fixed_width :
  forall tree width, canonical width tree -> fixed_width width tree.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width shaped.
  destruct endpoint as [claim|].
  - destruct children as [|first rest]; cbn [canonical] in shaped; [|destruct shaped].
    cbn [fixed_width]. split; [lia|]. split; [intros _; reflexivity|].
    split; [intros below; lia|]. split; [constructor|exact I].
  - cbn [canonical] in shaped. destruct shaped as (below & _ & sorted & child_shapes).
    rewrite all_children_iff in child_shapes.
    cbn [fixed_width]. split; [lia|]. split; [intros full; lia|].
    split; [intros _; reflexivity|]. split; [exact sorted|].
    apply all_children_iff. intros edge child member.
    exact (IH (edge, child) member _ (child_shapes edge child member)).
Qed.

Lemma empty_tree_fixed_width : forall width, fixed_width width empty_tree.
Proof.
  intros width. cbn [fixed_width empty_tree length].
  split; [lia|]. split; [intros _; reflexivity|]. split; [intros _; reflexivity|].
  split; [constructor|exact I].
Qed.

(** ** Lookup lemmas *)

Lemma strip_prefix_app :
  forall prefix rest, strip_prefix prefix (prefix ++ rest) = Some rest.
Proof.
  induction prefix as [|byte prefix IH]; intros rest; cbn [strip_prefix app];
    [reflexivity|].
  rewrite Nat.eqb_refl. apply IH.
Qed.

Lemma strip_prefix_self : forall prefix, strip_prefix prefix prefix = Some [].
Proof.
  intros prefix. rewrite <- (app_nil_r prefix) at 2. apply strip_prefix_app.
Qed.

Lemma strip_prefix_some :
  forall prefix key rest, strip_prefix prefix key = Some rest -> key = prefix ++ rest.
Proof.
  induction prefix as [|byte prefix IH]; intros [|next key] rest stripped;
    cbn [strip_prefix] in stripped.
  - injection stripped as <-. reflexivity.
  - injection stripped as <-. reflexivity.
  - discriminate stripped.
  - destruct (Nat.eqb_spec byte next) as [same|different]; [|discriminate stripped].
    subst next. cbn [app]. f_equal. exact (IH key rest stripped).
Qed.

Lemma strip_prefix_none :
  forall prefix key, strip_prefix prefix key = None -> forall rest, key <> prefix ++ rest.
Proof.
  intros prefix key none rest same. subst key.
  rewrite strip_prefix_app in none. discriminate none.
Qed.

Lemma bytes_eqb_refl : forall bytes, bytes_eqb bytes bytes = true.
Proof. intros bytes. apply bytes_eqb_true_iff. reflexivity. Qed.

Lemma bytes_eqb_false_iff :
  forall left right, bytes_eqb left right = false <-> left <> right.
Proof.
  intros left right. split.
  - intros different same. apply bytes_eqb_true_iff in same. congruence.
  - intros different. destruct (bytes_eqb left right) eqn:test; [|reflexivity].
    apply bytes_eqb_true_iff in test. contradiction.
Qed.

Lemma entry_lookup_app :
  forall left right key,
    entry_lookup (left ++ right) key =
      match entry_lookup left key with
      | Some claim => Some claim
      | None => entry_lookup right key
      end.
Proof.
  induction left as [|[candidate claim] left IH]; intros right key;
    cbn [app entry_lookup]; [reflexivity|].
  destruct (bytes_eqb candidate key); [reflexivity|apply IH].
Qed.

Lemma entry_lookup_prepend :
  forall prefix entries key,
    entry_lookup (map (prepend_entry prefix) entries) key =
      match strip_prefix prefix key with
      | Some rest => entry_lookup entries rest
      | None => None
      end.
Proof.
  intros prefix entries key.
  destruct (strip_prefix prefix key) as [rest|] eqn:stripped.
  - apply strip_prefix_some in stripped. subst key.
    induction entries as [|[candidate claim] entries IH];
      cbn [map entry_lookup prepend_entry fst snd]; [reflexivity|].
    destruct (bytes_eqb candidate rest) eqn:same.
    + apply bytes_eqb_true_iff in same. subst rest. rewrite bytes_eqb_refl. reflexivity.
    + assert (different : bytes_eqb (prefix ++ candidate) (prefix ++ rest) = false).
      { apply bytes_eqb_false_iff. intros equal. apply app_inv_head in equal.
        apply bytes_eqb_false_iff in same. contradiction. }
      rewrite different. exact IH.
  - induction entries as [|[candidate claim] entries IH];
      cbn [map entry_lookup prepend_entry fst snd]; [reflexivity|].
    assert (different : bytes_eqb (prefix ++ candidate) key = false).
    { apply bytes_eqb_false_iff. intros equal.
      apply (strip_prefix_none prefix key stripped candidate). symmetry. exact equal. }
    rewrite different. exact IH.
Qed.

Lemma children_entries_lookup_nil :
  forall entries_of children, entry_lookup (children_entries entries_of children) [] = None.
Proof.
  intros entries_of children.
  induction children as [|[edge child] rest IH]; cbn [children_entries]; [reflexivity|].
  rewrite entry_lookup_app, entry_lookup_prepend. cbn [strip_prefix]. exact IH.
Qed.

Lemma children_entries_lookup_absent :
  forall entries_of children edge rest,
    (forall child, ~ In (edge, child) children) ->
    entry_lookup (children_entries entries_of children) (edge :: rest) = None.
Proof.
  intros entries_of children edge rest absent.
  induction children as [|[other child] later IH]; cbn [children_entries]; [reflexivity|].
  rewrite entry_lookup_app, entry_lookup_prepend. cbn [strip_prefix].
  destruct (Nat.eqb_spec other edge) as [same|different].
  - subst other. exfalso. apply (absent child). left. reflexivity.
  - cbv beta iota. apply IH. intros target member. apply (absent target). right. exact member.
Qed.

Lemma children_entries_lookup :
  forall entries_of children edge rest,
    edges_sorted children ->
    entry_lookup (children_entries entries_of children) (edge :: rest) =
      child_lookup (fun child => entry_lookup (entries_of child) rest) edge children.
Proof.
  intros entries_of children edge rest sorted.
  induction children as [|[other child] later IH];
    cbn [children_entries child_lookup]; [reflexivity|].
  apply StronglySorted_inv in sorted as [later_sorted head_below].
  rewrite entry_lookup_app, entry_lookup_prepend. cbn [strip_prefix].
  destruct (Nat.eqb_spec other edge) as [same|different].
  - subst other. rewrite Nat.eqb_refl. cbv beta iota.
    destruct (entry_lookup (entries_of child) rest) as [claim|] eqn:found; [reflexivity|].
    apply children_entries_lookup_absent.
    intros target member. rewrite Forall_forall in head_below.
    specialize (head_below (edge, target) member). cbn [fst] in head_below. lia.
  - assert (skip : Nat.eqb edge other = false) by (apply Nat.eqb_neq; congruence).
    rewrite skip. cbv beta iota. exact (IH later_sorted).
Qed.

Lemma child_lookup_ext_in :
  forall (left right : radix_tree -> option nat) edge children,
    (forall other child, In (other, child) children -> left child = right child) ->
    child_lookup left edge children = child_lookup right edge children.
Proof.
  intros left right edge children same.
  induction children as [|[other child] later IH]; cbn [child_lookup]; [reflexivity|].
  rewrite (same other child (or_introl eq_refl)).
  rewrite IH; [reflexivity|].
  intros next target member. exact (same next target (or_intror member)).
Qed.

Lemma child_lookup_member :
  forall (lookup_in : radix_tree -> option nat) children edge child,
    edges_sorted children -> In (edge, child) children ->
    child_lookup lookup_in edge children = lookup_in child.
Proof.
  intros lookup_in children edge child sorted member.
  induction children as [|[other target] later IH]; [destruct member|].
  apply StronglySorted_inv in sorted as [later_sorted head_below].
  cbn [child_lookup].
  destruct member as [same|member].
  - injection same as same_edge same_child. subst. rewrite Nat.eqb_refl. reflexivity.
  - destruct (Nat.eqb_spec edge other) as [same|different].
    + subst other. rewrite Forall_forall in head_below.
      specialize (head_below (edge, child) member). cbn [fst] in head_below. lia.
    + exact (IH later_sorted member).
Qed.

Lemma child_lookup_absent :
  forall (lookup_in : radix_tree -> option nat) children edge,
    (forall child, ~ In (edge, child) children) ->
    child_lookup lookup_in edge children = None.
Proof.
  intros lookup_in children edge absent.
  induction children as [|[other child] later IH]; cbn [child_lookup]; [reflexivity|].
  destruct (Nat.eqb_spec edge other) as [same|different].
  - subst other. exfalso. apply (absent child). left. reflexivity.
  - apply IH. intros target member. apply (absent target). right. exact member.
Qed.

Lemma child_lookup_some :
  forall (lookup_in : radix_tree -> option nat) children edge claim,
    child_lookup lookup_in edge children = Some claim ->
    exists child, In (edge, child) children /\ lookup_in child = Some claim.
Proof.
  intros lookup_in children edge claim.
  induction children as [|[other child] later IH]; cbn [child_lookup];
    [intro impossible; discriminate impossible|].
  destruct (Nat.eqb_spec edge other) as [same|different]; intro found.
  - subst other. exists child. split; [left; reflexivity|exact found].
  - destruct (IH found) as [target [member target_found]].
    exists target. split; [right; exact member|exact target_found].
Qed.

Lemma member_lookup :
  forall prefix endpoint children edge child rest,
    edges_sorted children -> In (edge, child) children ->
    tree_lookup (RadixTree prefix endpoint children) (prefix ++ edge :: rest) =
      tree_lookup child rest.
Proof.
  intros prefix endpoint children edge child rest sorted member.
  cbn [tree_lookup]. rewrite strip_prefix_app.
  exact (child_lookup_member (fun target => tree_lookup target rest) children edge child
    sorted member).
Qed.

(** On fixed-width trees the lookup is the first matching entry. *)
Lemma tree_lookup_entries :
  forall tree width, fixed_width width tree ->
    forall key, tree_lookup tree key = entry_lookup (tree_entries tree) key.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width shaped key. cbn [fixed_width] in shaped.
  destruct shaped as (_ & _ & _ & sorted & child_shapes).
  rewrite all_children_iff in child_shapes.
  cbn [tree_lookup tree_entries].
  rewrite entry_lookup_app, entry_lookup_prepend.
  destruct (strip_prefix prefix key) as [rest|] eqn:stripped.
  - apply strip_prefix_some in stripped. subst key.
    destruct rest as [|edge rest].
    + rewrite app_nil_r.
      destruct endpoint as [claim|]; cbn [endpoint_entries entry_lookup].
      * rewrite bytes_eqb_refl. reflexivity.
      * rewrite children_entries_lookup_nil. reflexivity.
    + assert (endpoint_miss :
        entry_lookup (endpoint_entries prefix endpoint) (prefix ++ edge :: rest) = None).
      { destruct endpoint as [claim|]; cbn [endpoint_entries entry_lookup]; [|reflexivity].
        assert (different : bytes_eqb prefix (prefix ++ edge :: rest) = false).
        { apply bytes_eqb_false_iff. intros same.
          apply (f_equal (@length nat)) in same. rewrite length_app in same.
          cbn [length] in same. lia. }
        rewrite different. reflexivity. }
      rewrite endpoint_miss, children_entries_lookup by exact sorted.
      apply child_lookup_ext_in. intros other child member.
      exact (IH (other, child) member _ (child_shapes other child member) rest).
  - destruct endpoint as [claim|]; cbn [endpoint_entries entry_lookup]; [|reflexivity].
    assert (different : bytes_eqb prefix key = false).
    { apply bytes_eqb_false_iff. intros same.
      apply (strip_prefix_none prefix key stripped []). rewrite app_nil_r. symmetry.
      exact same. }
    rewrite different. reflexivity.
Qed.

Lemma fixed_width_lookup_length :
  forall tree width key claim,
    fixed_width width tree -> tree_lookup tree key = Some claim -> length key = width.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width key claim shaped found. cbn [fixed_width] in shaped.
  destruct shaped as (bounded & full_leaf & below_inner & sorted & child_shapes).
  rewrite all_children_iff in child_shapes.
  cbn [tree_lookup] in found.
  destruct (strip_prefix prefix key) as [rest|] eqn:stripped; [|discriminate found].
  apply strip_prefix_some in stripped. subst key.
  destruct rest as [|edge rest].
  - rewrite app_nil_r.
    destruct (Nat.eq_dec (length prefix) width) as [full|partial]; [exact full|].
    rewrite (below_inner ltac:(lia)) in found. discriminate found.
  - apply child_lookup_some in found as [child [member child_found]].
    destruct (Nat.eq_dec (length prefix) width) as [full|partial].
    { rewrite (full_leaf full) in member. destruct member. }
    pose proof (IH (edge, child) member _ rest claim
      (child_shapes edge child member) child_found) as child_length.
    rewrite length_app. cbn [length]. lia.
Qed.

Lemma leaf_lookup_self :
  forall prefix claim, tree_lookup (RadixTree prefix (Some claim) []) prefix = Some claim.
Proof.
  intros prefix claim. cbn [tree_lookup]. rewrite strip_prefix_self. reflexivity.
Qed.

Lemma leaf_lookup_some :
  forall prefix claim key value,
    tree_lookup (RadixTree prefix (Some claim) []) key = Some value ->
    key = prefix /\ value = claim.
Proof.
  intros prefix claim key value found. cbn [tree_lookup child_lookup] in found.
  destruct (strip_prefix prefix key) as [[|edge rest]|] eqn:stripped;
    try discriminate found.
  apply strip_prefix_some in stripped. rewrite app_nil_r in stripped.
  injection found as same. split; [exact stripped|symmetry; exact same].
Qed.

Lemma inner_lookup_shape :
  forall prefix children key value,
    tree_lookup (RadixTree prefix None children) key = Some value ->
    exists edge rest, key = prefix ++ edge :: rest.
Proof.
  intros prefix children key value found. cbn [tree_lookup] in found.
  destruct (strip_prefix prefix key) as [[|edge rest]|] eqn:stripped;
    try discriminate found.
  exists edge, rest. exact (strip_prefix_some _ _ _ stripped).
Qed.

Lemma canonical_has_entry :
  forall tree width,
    canonical width tree -> exists key claim, tree_lookup tree key = Some claim.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width shaped.
  destruct endpoint as [claim|].
  - destruct children as [|first rest]; cbn [canonical] in shaped; [|destruct shaped].
    exists prefix, claim. apply leaf_lookup_self.
  - cbn [canonical] in shaped. destruct shaped as (_ & wide & sorted & child_shapes).
    rewrite all_children_iff in child_shapes.
    destruct children as [|[edge child] rest]; [cbn [length] in wide; lia|].
    destruct (IH (edge, child) (in_eq (edge, child) rest) _
      (child_shapes edge child (in_eq (edge, child) rest))) as [key [claim found]].
    exists (prefix ++ edge :: key), claim.
    rewrite (member_lookup prefix None ((edge, child) :: rest) edge child key sorted
      (in_eq _ _)).
    exact found.
Qed.

(** An inner canonical node denotes two keys that diverge right after its
    prefix. *)
Lemma inner_two_keys :
  forall width prefix children,
    canonical width (RadixTree prefix None children) ->
    exists left left_rest left_value right right_rest right_value,
      left <> right /\
      tree_lookup (RadixTree prefix None children) (prefix ++ left :: left_rest) =
        Some left_value /\
      tree_lookup (RadixTree prefix None children) (prefix ++ right :: right_rest) =
        Some right_value.
Proof.
  intros width prefix children shaped.
  cbn [canonical] in shaped. destruct shaped as (_ & wide & sorted & child_shapes).
  rewrite all_children_iff in child_shapes.
  destruct children as [|[left left_child] [|[right right_child] rest]];
    cbn [length] in wide; try lia.
  pose proof sorted as sorted_copy.
  apply StronglySorted_inv in sorted_copy as [_ head_below].
  rewrite Forall_forall in head_below.
  pose proof (head_below (right, right_child) (in_eq _ _)) as ordered. cbn [fst] in ordered.
  assert (left_member : In (left, left_child) ((left, left_child) :: (right, right_child) :: rest))
    by (left; reflexivity).
  assert (right_member :
    In (right, right_child) ((left, left_child) :: (right, right_child) :: rest))
    by (right; left; reflexivity).
  destruct (canonical_has_entry left_child _ (child_shapes _ _ left_member))
    as [left_rest [left_value left_found]].
  destruct (canonical_has_entry right_child _ (child_shapes _ _ right_member))
    as [right_rest [right_value right_found]].
  exists left, left_rest, left_value, right, right_rest, right_value.
  split; [lia|]. split.
  - rewrite (member_lookup _ _ _ _ _ _ sorted left_member). exact left_found.
  - rewrite (member_lookup _ _ _ _ _ _ sorted right_member). exact right_found.
Qed.

Lemma diverging_keys_bound_common_prefix :
  forall (prefix : list nat) (left right : nat)
    (left_rest right_rest other other_left other_right : list nat),
    left <> right ->
    prefix ++ left :: left_rest = other ++ other_left ->
    prefix ++ right :: right_rest = other ++ other_right ->
    length other <= length prefix.
Proof.
  intros prefix left right left_rest right_rest other other_left other_right
    different left_key right_key.
  destruct (le_lt_dec (length other) (length prefix)) as [short|long]; [exact short|].
  exfalso. apply different.
  assert (left_at : nth (length prefix) (prefix ++ left :: left_rest) 0 = left).
  { rewrite app_nth2 by lia. rewrite Nat.sub_diag. reflexivity. }
  assert (right_at : nth (length prefix) (prefix ++ right :: right_rest) 0 = right).
  { rewrite app_nth2 by lia. rewrite Nat.sub_diag. reflexivity. }
  rewrite left_key, app_nth1 in left_at by exact long.
  rewrite right_key, app_nth1 in right_at by exact long.
  congruence.
Qed.

Lemma same_length_prefixes_are_equal :
  forall left right left_rest right_rest : list nat,
    length left = length right -> left ++ left_rest = right ++ right_rest -> left = right.
Proof.
  induction left as [|head tail IH]; intros [|other rest] left_rest right_rest
    same_length same; cbn [length] in same_length; try discriminate same_length;
    [reflexivity|].
  injection same_length as tail_length. cbn [app] in same.
  injection same as head_same tail_same. subst other. f_equal.
  exact (IH rest left_rest right_rest tail_length tail_same).
Qed.

Lemma edges_sorted_unique :
  forall left right,
    edges_sorted left -> edges_sorted right ->
    (forall child, In child left <-> In child right) -> left = right.
Proof.
  induction left as [|first rest IH]; intros [|other later] left_sorted right_sorted same.
  - reflexivity.
  - exfalso. apply (proj2 (same other)). left. reflexivity.
  - exfalso. apply (proj1 (same first)). left. reflexivity.
  - apply StronglySorted_inv in left_sorted as [rest_sorted first_below].
    apply StronglySorted_inv in right_sorted as [later_sorted other_below].
    rewrite Forall_forall in first_below, other_below.
    assert (heads : first = other).
    { destruct (proj1 (same first) (or_introl eq_refl)) as [same_head|first_later];
        [symmetry; exact same_head|].
      destruct (proj2 (same other) (or_introl eq_refl)) as [same_head|other_rest];
        [exact same_head|].
      specialize (other_below first first_later).
      specialize (first_below other other_rest). lia. }
    subst other. f_equal. apply IH; [exact rest_sorted|exact later_sorted|].
    intros child. split; intros member.
    + destruct (proj1 (same child) (or_intror member)) as [head|tail]; [|exact tail].
      subst child. specialize (first_below first member). lia.
    + destruct (proj2 (same child) (or_intror member)) as [head|tail]; [|exact tail].
      subst child. specialize (other_below first member). lia.
Qed.

(** Uniqueness: two canonical trees of one width that agree on every lookup
    are equal.  The prefix is the longest common prefix of the denoted keys
    (two keys diverge right after it), and each child is determined by the
    lookups below its edge. *)
Lemma canonical_lookup_unique :
  forall left width right,
    canonical width left -> canonical width right ->
    (forall key, tree_lookup left key = tree_lookup right key) -> left = right.
Proof.
  intros left. induction left as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width [right_prefix right_endpoint right_children] left_shape right_shape same.
  destruct endpoint as [claim|]; destruct right_endpoint as [right_claim|].
  - destruct children as [|first rest]; [|destruct left_shape].
    destruct right_children as [|first rest]; [|destruct right_shape].
    pose proof (same prefix) as at_prefix. rewrite leaf_lookup_self in at_prefix.
    symmetry in at_prefix. apply leaf_lookup_some in at_prefix as [same_prefix same_claim].
    subst. reflexivity.
  - exfalso.
    destruct children as [|first rest]; [|destruct left_shape].
    destruct (inner_two_keys _ _ _ right_shape)
      as (left_edge & left_rest & left_value & right_edge & right_rest & right_value &
          different & left_found & right_found).
    rewrite <- same in left_found, right_found.
    apply leaf_lookup_some in left_found as [left_key _].
    apply leaf_lookup_some in right_found as [right_key _].
    rewrite <- right_key in left_key. apply app_inv_head in left_key.
    injection left_key as edges_same _. contradiction.
  - exfalso.
    destruct right_children as [|first rest]; [|destruct right_shape].
    destruct (inner_two_keys _ _ _ left_shape)
      as (left_edge & left_rest & left_value & right_edge & right_rest & right_value &
          different & left_found & right_found).
    rewrite same in left_found, right_found.
    apply leaf_lookup_some in left_found as [left_key _].
    apply leaf_lookup_some in right_found as [right_key _].
    rewrite <- right_key in left_key. apply app_inv_head in left_key.
    injection left_key as edges_same _. contradiction.
  - assert (same_prefix : prefix = right_prefix).
    { destruct (inner_two_keys _ _ _ left_shape)
        as (a & a_rest & a_value & b & b_rest & b_value & a_b & a_found & b_found).
      destruct (inner_two_keys _ _ _ right_shape)
        as (c & c_rest & c_value & d & d_rest & d_value & c_d & c_found & d_found).
      pose proof a_found as a_right. rewrite same in a_right.
      pose proof b_found as b_right. rewrite same in b_right.
      pose proof c_found as c_left. rewrite <- same in c_left.
      pose proof d_found as d_left. rewrite <- same in d_left.
      apply inner_lookup_shape in a_right as (a' & a_rest' & a_key).
      apply inner_lookup_shape in b_right as (b' & b_rest' & b_key).
      apply inner_lookup_shape in c_left as (c' & c_rest' & c_key).
      apply inner_lookup_shape in d_left as (d' & d_rest' & d_key).
      pose proof (diverging_keys_bound_common_prefix prefix a b a_rest b_rest right_prefix
        _ _ a_b a_key b_key) as right_short.
      pose proof (diverging_keys_bound_common_prefix right_prefix c d c_rest d_rest prefix
        _ _ c_d c_key d_key) as left_short.
      apply (same_length_prefixes_are_equal prefix right_prefix (a :: a_rest)
        (a' :: a_rest')); [lia|exact a_key]. }
    subst right_prefix.
    cbn [canonical] in left_shape, right_shape.
    destruct left_shape as (below & wide & sorted & child_shapes).
    destruct right_shape as (_ & right_wide & right_sorted & right_child_shapes).
    rewrite all_children_iff in child_shapes, right_child_shapes.
    assert (left_to_right : forall edge child, In (edge, child) children ->
              In (edge, child) right_children).
    { intros edge child member.
      destruct (canonical_has_entry child _ (child_shapes edge child member))
        as [rest [value found]].
      pose proof (same (prefix ++ edge :: rest)) as at_key.
      rewrite (member_lookup _ _ _ _ _ _ sorted member), found in at_key.
      cbn [tree_lookup] in at_key. rewrite strip_prefix_app in at_key.
      symmetry in at_key.
      destruct (child_lookup_some _ _ _ _ at_key) as [other [other_member _]].
      assert (child = other) as same_child.
      { apply (IH (edge, child) member (width - S (length prefix)) other
          (child_shapes edge child member) (right_child_shapes edge other other_member)).
        intros key. cbn [snd].
        rewrite <- (member_lookup prefix None children edge child key sorted member).
        rewrite <- (member_lookup prefix None right_children edge other key right_sorted
          other_member).
        apply same. }
      subst other. exact other_member. }
    assert (right_to_left : forall edge child, In (edge, child) right_children ->
              In (edge, child) children).
    { intros edge child member.
      destruct (canonical_has_entry child _ (right_child_shapes edge child member))
        as [rest [value found]].
      pose proof (same (prefix ++ edge :: rest)) as at_key.
      rewrite (member_lookup _ _ _ _ _ _ right_sorted member), found in at_key.
      cbn [tree_lookup] in at_key. rewrite strip_prefix_app in at_key.
      destruct (child_lookup_some _ _ _ _ at_key) as [other [other_member _]].
      assert (other = child) as same_child.
      { apply (IH (edge, other) other_member (width - S (length prefix))
          child (child_shapes edge other other_member)
          (right_child_shapes edge child member)).
        intros key. cbn [snd].
        rewrite <- (member_lookup prefix None children edge other key sorted other_member).
        rewrite <- (member_lookup prefix None right_children edge child key right_sorted
          member).
        apply same. }
      subst other. exact other_member. }
    f_equal. apply edges_sorted_unique; [exact sorted|exact right_sorted|].
    intros [edge child]. split; [apply left_to_right|apply right_to_left].
Qed.

(** ** Entry lists of fixed-width trees *)

Definition key_before (left right : list nat * nat) : Prop :=
  ChannelImage.bytes_ltb (fst left) (fst right) = true.

Lemma prepend_preserves_key_order :
  forall bytes left right,
    key_before left right -> key_before (prepend_entry bytes left) (prepend_entry bytes right).
Proof.
  intros bytes left right before. unfold key_before, prepend_entry in *. cbn [fst].
  rewrite bytes_ltb_same_prefix. exact before.
Qed.

Lemma children_entries_in :
  forall entries_of children entry,
    In entry (children_entries entries_of children) ->
    exists edge child inner,
      In (edge, child) children /\ entry = prepend_entry [edge] inner /\
      In inner (entries_of child).
Proof.
  intros entries_of children entry.
  induction children as [|[edge child] later IH]; cbn [children_entries]; [intros []|].
  intros member. apply in_app_or in member as [here|there].
  - apply in_map_iff in here as [inner [same inner_member]].
    exists edge, child, inner. split; [left; reflexivity|]. split; [symmetry; exact same|].
    exact inner_member.
  - destruct (IH there) as [other [target [inner [member [same inner_member]]]]].
    exists other, target, inner. split; [right; exact member|]. split; assumption.
Qed.

Lemma children_entries_sorted :
  forall entries_of children,
    edges_sorted children ->
    (forall edge child, In (edge, child) children ->
       StronglySorted key_before (entries_of child)) ->
    StronglySorted key_before (children_entries entries_of children).
Proof.
  intros entries_of children sorted each.
  induction children as [|[edge child] later IH]; cbn [children_entries]; [constructor|].
  apply StronglySorted_inv in sorted as [later_sorted head_below].
  apply strongly_sorted_app.
  - apply (strongly_sorted_map key_before key_before (prepend_entry [edge]));
      [intros x y; apply prepend_preserves_key_order|].
    exact (each edge child (or_introl eq_refl)).
  - apply IH; [exact later_sorted|].
    intros other target member. exact (each other target (or_intror member)).
  - intros x y x_member y_member.
    apply in_map_iff in x_member as [inner [x_is _]]. subst x.
    apply children_entries_in in y_member
      as [other [target [inner' [member [y_is _]]]]]. subst y.
    unfold key_before, prepend_entry. cbn [fst app].
    apply bytes_ltb_cons_lt. rewrite Forall_forall in head_below.
    exact (head_below (other, target) member).
Qed.

Lemma prefix_entry_before_extension :
  forall prefix edge rest claim value,
    key_before (prefix, claim) (prefix ++ edge :: rest, value).
Proof.
  intros prefix edge rest claim value. unfold key_before. cbn [fst].
  rewrite <- (app_nil_r prefix) at 1. rewrite bytes_ltb_same_prefix. reflexivity.
Qed.

Lemma fixed_width_entries :
  forall tree width,
    fixed_width width tree ->
    StronglySorted key_before (tree_entries tree) /\
    Forall (fun entry => length (fst entry) = width) (tree_entries tree).
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width shaped. cbn [fixed_width] in shaped.
  destruct shaped as (bounded & full_leaf & below_inner & sorted & child_shapes).
  rewrite all_children_iff in child_shapes.
  assert (children_facts : forall edge child, In (edge, child) children ->
    StronglySorted key_before (tree_entries child) /\
    Forall (fun entry => length (fst entry) = width - S (length prefix))
      (tree_entries child)).
  { intros edge child member.
    exact (IH (edge, child) member _ (child_shapes edge child member)). }
  cbn [tree_entries]. split.
  - apply strongly_sorted_app.
    + destruct endpoint; repeat constructor.
    + apply (strongly_sorted_map key_before key_before (prepend_entry prefix));
        [intros x y; apply prepend_preserves_key_order|].
      apply children_entries_sorted; [exact sorted|].
      intros edge child member. exact (proj1 (children_facts edge child member)).
    + intros x y x_member y_member.
      destruct endpoint as [claim|]; [|destruct x_member].
      destruct x_member as [x_is|[]]. subst x.
      apply in_map_iff in y_member as [inner [y_is inner_member]]. subst y.
      apply children_entries_in in inner_member
        as [edge [child [deeper [_ [inner_is _]]]]]. subst inner.
      unfold prepend_entry. cbn [fst snd app].
      apply prefix_entry_before_extension.
  - apply Forall_app. split.
    + destruct endpoint as [claim|]; [|constructor].
      constructor; [|constructor]. cbn [fst].
      destruct (Nat.eq_dec (length prefix) width) as [full|partial]; [exact full|].
      pose proof (below_inner ltac:(lia)) as impossible. discriminate impossible.
    + apply Forall_forall. intros entry member.
      apply in_map_iff in member as [inner [entry_is inner_member]]. subst entry.
      apply children_entries_in in inner_member
        as [edge [child [deeper [child_member [inner_is deeper_member]]]]]. subst inner.
      destruct (Nat.eq_dec (length prefix) width) as [full|partial].
      { rewrite (full_leaf full) in child_member. destruct child_member. }
      pose proof (proj2 (children_facts edge child child_member)) as lengths.
      rewrite Forall_forall in lengths. specialize (lengths deeper deeper_member).
      unfold prepend_entry. cbn [fst]. rewrite !length_app. cbn [length]. lia.
Qed.

Lemma canonical_entries_nonempty :
  forall tree width, canonical width tree -> tree_entries tree <> [].
Proof.
  intros tree width shaped empty.
  destruct (canonical_has_entry tree width shaped) as [key [claim found]].
  rewrite (tree_lookup_entries tree width (canonical_fixed_width tree width shaped)),
    empty in found.
  discriminate found.
Qed.

Lemma canonical_root_entries_unique :
  forall width left right,
    canonical_root width left -> canonical_root width right ->
    tree_entries left = tree_entries right -> left = right.
Proof.
  intros width left right [left_empty|left_shape] [right_empty|right_shape] same.
  - congruence.
  - subst left. exfalso. apply (canonical_entries_nonempty right width right_shape).
    rewrite <- same. reflexivity.
  - subst right. exfalso. apply (canonical_entries_nonempty left width left_shape).
    rewrite same. reflexivity.
  - apply (canonical_lookup_unique left width right left_shape right_shape).
    intros key.
    rewrite (tree_lookup_entries left width (canonical_fixed_width _ _ left_shape)).
    rewrite (tree_lookup_entries right width (canonical_fixed_width _ _ right_shape)).
    rewrite same. reflexivity.
Qed.

(** * The independent rebuild *)

(** [partition_build] is the oracle of the property tests
    ([build_radix], retention_v2_format_properties.rs): over sorted entries, a
    single entry is a leaf whose prefix is the whole remaining key; otherwise
    the prefix is the longest common prefix of the first and the last key and
    the children partition the entries by the next byte.  The fuel is the key
    width. *)

Fixpoint common_prefix (left right : list nat) : list nat :=
  match left, right with
  | left_head :: left_tail, right_head :: right_tail =>
      if Nat.eqb left_head right_head then left_head :: common_prefix left_tail right_tail
      else []
  | _, _ => []
  end.

Fixpoint group_by_edge (entries : list (list nat * nat))
    : list (nat * list (list nat * nat)) :=
  match entries with
  | [] => []
  | (key, claim) :: rest =>
      match key with
      | [] => group_by_edge rest
      | edge :: tail =>
          match group_by_edge rest with
          | (next, group) :: later =>
              if Nat.eqb edge next then (edge, (tail, claim) :: group) :: later
              else (edge, [(tail, claim)]) :: (next, group) :: later
          | [] => [(edge, [(tail, claim)])]
          end
      end
  end.

Definition strip_entries (count : nat) (entries : list (list nat * nat))
    : list (list nat * nat) :=
  map (fun entry => (skipn count (fst entry), snd entry)) entries.

Definition last_key (entries : list (list nat * nat)) : list nat :=
  fst (last entries ([], 0)).

Fixpoint partition_build (fuel : nat) (entries : list (list nat * nat)) : radix_tree :=
  match entries with
  | [] => empty_tree
  | [(key, claim)] => RadixTree key (Some claim) []
  | (first, _) :: _ =>
      match fuel with
      | 0 => empty_tree
      | S smaller =>
          RadixTree (common_prefix first (last_key entries)) None
            (map (fun group => (fst group, partition_build smaller (snd group)))
               (group_by_edge
                  (strip_entries (length (common_prefix first (last_key entries)))
                     entries)))
      end
  end.

Definition ungroup (groups : list (nat * list (list nat * nat))) : list (list nat * nat) :=
  flat_map (fun group => map (prepend_entry [fst group]) (snd group)) groups.

Lemma common_prefix_split :
  forall left right,
    exists left_rest right_rest,
      left = common_prefix left right ++ left_rest /\
      right = common_prefix left right ++ right_rest /\
      match left_rest, right_rest with
      | left_head :: _, right_head :: _ => left_head <> right_head
      | _, _ => True
      end.
Proof.
  induction left as [|left_head left_tail IH]; intros [|right_head right_tail].
  - exists [], []. split; [reflexivity|]. split; [reflexivity|exact I].
  - exists [], (right_head :: right_tail). split; [reflexivity|]. split; [reflexivity|exact I].
  - exists (left_head :: left_tail), []. split; [reflexivity|]. split; [reflexivity|exact I].
  - cbn [common_prefix]. destruct (Nat.eqb_spec left_head right_head) as [same|different].
    + subst right_head.
      destruct (IH right_tail) as [left_rest [right_rest [left_is [right_is diverge]]]].
      exists left_rest, right_rest. cbn [app].
      split; [f_equal; exact left_is|]. split; [f_equal; exact right_is|exact diverge].
    + exists (left_head :: left_tail), (right_head :: right_tail). cbn [app].
      split; [reflexivity|]. split; [reflexivity|exact different].
Qed.

Lemma common_prefix_shorter :
  forall left right,
    length left = length right -> left <> right ->
    length (common_prefix left right) < length left.
Proof.
  intros left right same_length different.
  destruct (common_prefix_split left right) as [left_rest [right_rest [left_is [right_is _]]]].
  pose proof (f_equal (@length nat) left_is) as left_length.
  pose proof (f_equal (@length nat) right_is) as right_length.
  rewrite length_app in left_length, right_length.
  destruct left_rest as [|left_head left_tail]; [|cbn [length] in left_length; lia].
  destruct right_rest as [|right_head right_tail]; [|cbn [length] in *; lia].
  exfalso. apply different. rewrite app_nil_r in left_is, right_is. congruence.
Qed.

Definition key_le (left right : list nat) : Prop :=
  ChannelImage.bytes_ltb left right = true \/ left = right.

Lemma key_le_cons :
  forall left_head left_tail right_head right_tail,
    key_le (left_head :: left_tail) (right_head :: right_tail) ->
    left_head <= right_head /\
    (left_head = right_head -> key_le left_tail right_tail).
Proof.
  intros left_head left_tail right_head right_tail [below|same].
  - cbn [ChannelImage.bytes_ltb] in below.
    destruct (Nat.ltb_spec left_head right_head) as [lt|ge].
    + split; [lia|intros same; lia].
    + destruct (Nat.ltb_spec right_head left_head) as [gt|le]; [discriminate below|].
      split; [lia|intros _; left; exact below].
  - injection same as head_same tail_same. subst.
    split; [lia|intros _; right; reflexivity].
Qed.

Lemma common_prefix_between :
  forall low middle high,
    key_le low middle -> key_le middle high ->
    exists rest, middle = common_prefix low high ++ rest.
Proof.
  induction low as [|low_head low_tail IH]; intros middle high low_middle middle_high.
  - exists middle. reflexivity.
  - destruct high as [|high_head high_tail]; [exists middle; reflexivity|].
    cbn [common_prefix].
    destruct (Nat.eqb_spec low_head high_head) as [same|different];
      [|exists middle; reflexivity].
    subst high_head.
    destruct middle as [|middle_head middle_tail].
    + exfalso. destruct low_middle as [below|same];
        [cbn in below; discriminate below|discriminate same].
    + destruct (key_le_cons _ _ _ _ low_middle) as [low_le low_tail_le].
      destruct (key_le_cons _ _ _ _ middle_high) as [high_le high_tail_le].
      assert (middle_head = low_head) as heads by lia. subst middle_head.
      destruct (IH middle_tail high_tail (low_tail_le eq_refl) (high_tail_le eq_refl))
        as [rest rest_is].
      exists rest. cbn [app]. rewrite <- rest_is. reflexivity.
Qed.

Lemma last_member :
  forall {A : Type} (head : A) tail default, In (last (head :: tail) default) (head :: tail).
Proof.
  intros A head tail default. revert head.
  induction tail as [|next rest IH]; intros head; [left; reflexivity|].
  right. change (last (head :: next :: rest) default) with (last (next :: rest) default).
  exact (IH next).
Qed.

Lemma strongly_sorted_last :
  forall {A : Type} (R : A -> A -> Prop) values default x,
    StronglySorted R values -> In x values ->
    x = last values default \/ R x (last values default).
Proof.
  intros A R values default x sorted.
  induction sorted as [|head tail tail_sorted IH head_before]; intros member;
    [destruct member|].
  destruct tail as [|next rest].
  - destruct member as [same|[]]. left. subst x. reflexivity.
  - change (last (head :: next :: rest) default) with (last (next :: rest) default).
    destruct member as [same|member].
    + subst x. right. rewrite Forall_forall in head_before.
      apply head_before. apply last_member.
    + exact (IH member).
Qed.

Lemma strongly_sorted_first :
  forall {A : Type} (R : A -> A -> Prop) first rest x,
    StronglySorted R (first :: rest) -> In x (first :: rest) -> x = first \/ R first x.
Proof.
  intros A R first rest x sorted [same|member]; [left; symmetry; exact same|].
  apply StronglySorted_inv in sorted as [_ below]. rewrite Forall_forall in below.
  right. exact (below x member).
Qed.

Lemma strongly_sorted_map_in {A B : Type}
    (R : A -> A -> Prop) (R' : B -> B -> Prop) (f : A -> B) :
  forall values,
    (forall x y, In x values -> In y values -> R x y -> R' (f x) (f y)) ->
    StronglySorted R values -> StronglySorted R' (map f values).
Proof.
  intros values preserves sorted.
  induction sorted as [|head tail tail_sorted IH head_before]; cbn [map]; constructor.
  - apply IH. intros x y x_in y_in. apply preserves; right; assumption.
  - apply Forall_forall. intros image image_in.
    apply in_map_iff in image_in as [source [image_is source_in]]. subst image.
    rewrite Forall_forall in head_before.
    apply preserves; [left; reflexivity|right; exact source_in|].
    exact (head_before source source_in).
Qed.

Lemma sorted_keys_share_common_prefix :
  forall first rest entry,
    StronglySorted key_before (first :: rest) -> In entry (first :: rest) ->
    exists tail, fst entry = common_prefix (fst first) (last_key (first :: rest)) ++ tail.
Proof.
  intros first rest entry sorted member.
  apply common_prefix_between.
  - destruct (strongly_sorted_first key_before first rest entry sorted member)
      as [same|before].
    + right. rewrite same. reflexivity.
    + left. exact before.
  - unfold last_key.
    destruct (strongly_sorted_last key_before (first :: rest) ([], 0) entry sorted member)
      as [same|before].
    + right. rewrite <- same. reflexivity.
    + left. exact before.
Qed.

Lemma skipn_length_app :
  forall (prefix rest : list nat), skipn (length prefix) (prefix ++ rest) = rest.
Proof.
  induction prefix as [|byte prefix IH]; intros rest; [reflexivity|]. cbn. apply IH.
Qed.

Lemma strip_entries_prepend :
  forall prefix entries,
    (forall entry, In entry entries -> exists tail, fst entry = prefix ++ tail) ->
    map (prepend_entry prefix) (strip_entries (length prefix) entries) = entries.
Proof.
  intros prefix entries shared. unfold strip_entries. rewrite map_map.
  rewrite <- (map_id entries) at 2. apply map_ext_in.
  intros [key claim] member. destruct (shared (key, claim) member) as [tail key_is].
  cbn [fst] in key_is. subst key. unfold prepend_entry. cbn [fst snd]. cbv [id].
  rewrite skipn_length_app. reflexivity.
Qed.

Lemma strip_entries_sorted :
  forall prefix entries,
    (forall entry, In entry entries -> exists tail, fst entry = prefix ++ tail) ->
    StronglySorted key_before entries ->
    StronglySorted key_before (strip_entries (length prefix) entries).
Proof.
  intros prefix entries shared sorted. unfold strip_entries.
  apply (strongly_sorted_map_in key_before key_before); [|exact sorted].
  intros [x_key x_claim] [y_key y_claim] x_member y_member before.
  destruct (shared _ x_member) as [x_tail x_is].
  destruct (shared _ y_member) as [y_tail y_is].
  cbn [fst] in x_is, y_is. subst x_key y_key.
  unfold key_before in *. cbn [fst] in *.
  rewrite !skipn_length_app. rewrite bytes_ltb_same_prefix in before. exact before.
Qed.

Lemma strip_entries_lengths :
  forall count width entries,
    Forall (fun entry => length (fst entry) = width) entries ->
    Forall (fun entry => length (fst entry) = width - count) (strip_entries count entries).
Proof.
  intros count width entries lengths. unfold strip_entries.
  apply Forall_map. eapply Forall_impl; [|exact lengths].
  intros [key claim] key_length. cbn [fst] in *. rewrite length_skipn. lia.
Qed.

Lemma full_width_keys_nonempty :
  forall width (entries : list (list nat * nat)),
    Forall (fun entry => length (fst entry) = S width) entries ->
    Forall (fun entry => fst entry <> []) entries.
Proof.
  intros width entries lengths. eapply Forall_impl; [|exact lengths].
  intros [key claim] key_length empty. cbn [fst] in *. subst key. discriminate key_length.
Qed.

Lemma group_by_edge_ungroup :
  forall entries,
    Forall (fun entry => fst entry <> []) entries ->
    ungroup (group_by_edge entries) = entries.
Proof.
  induction entries as [|[key claim] rest IH]; intros nonempty; [reflexivity|].
  pose proof (Forall_inv nonempty) as key_nonempty. cbn [fst] in key_nonempty.
  pose proof (IH (Forall_inv_tail nonempty)) as rest_is.
  destruct key as [|edge tail]; [contradiction|].
  cbn [group_by_edge]. revert rest_is.
  destruct (group_by_edge rest) as [|[next group] later] eqn:grouped; intros rest_is.
  - unfold ungroup in *. cbn [flat_map] in rest_is. subst rest. reflexivity.
  - destruct (Nat.eqb_spec edge next) as [same|different].
    + subst next. unfold ungroup in *. cbn [flat_map map fst snd] in *.
      rewrite <- rest_is. reflexivity.
    + unfold ungroup in *. cbn [flat_map map fst snd] in *.
      rewrite <- rest_is. reflexivity.
Qed.

Lemma ungroup_member :
  forall groups edge group entry,
    In (edge, group) groups -> In entry group ->
    In (prepend_entry [edge] entry) (ungroup groups).
Proof.
  intros groups edge group entry group_member entry_member. unfold ungroup.
  apply in_flat_map. exists (edge, group). split; [exact group_member|].
  cbn [fst snd]. apply in_map. exact entry_member.
Qed.

Lemma group_by_edge_nonempty_groups :
  forall entries group, In group (group_by_edge entries) -> snd group <> [].
Proof.
  induction entries as [|[key claim] rest IH]; intros target member; [destruct member|].
  destruct key as [|edge tail]; [exact (IH target member)|].
  cbn [group_by_edge] in member. revert member.
  destruct (group_by_edge rest) as [|[next group] later] eqn:grouped; intros member.
  - destruct member as [same|[]]. subst target. cbn [snd]. discriminate.
  - destruct (Nat.eqb_spec edge next) as [same|different].
    + destruct member as [same_group|member]; [subst target; cbn [snd]; discriminate|].
      apply IH. try rewrite grouped. right. exact member.
    + destruct member as [same_group|member]; [subst target; cbn [snd]; discriminate|].
      apply IH. try rewrite grouped. exact member.
Qed.

Lemma group_by_edge_head :
  forall edge tail claim rest next group later,
    group_by_edge ((edge :: tail, claim) :: rest) = (next, group) :: later -> next = edge.
Proof.
  intros edge tail claim rest next group later grouped.
  cbn [group_by_edge] in grouped. revert grouped.
  destruct (group_by_edge rest) as [|[other other_group] other_later];
    [|destruct (Nat.eqb_spec edge other)]; intros grouped; congruence.
Qed.

Lemma group_by_edge_edges_sorted :
  forall entries,
    Forall (fun entry => fst entry <> []) entries ->
    StronglySorted key_before entries ->
    StronglySorted (fun left right => fst left < fst right) (group_by_edge entries).
Proof.
  induction entries as [|[key claim] rest IH]; intros nonempty sorted; [constructor|].
  pose proof (Forall_inv nonempty) as key_nonempty. cbn [fst] in key_nonempty.
  apply StronglySorted_inv in sorted as [rest_sorted head_before].
  pose proof (IH (Forall_inv_tail nonempty) rest_sorted) as rest_groups.
  destruct key as [|edge tail]; [contradiction|].
  cbn [group_by_edge]. revert rest_groups.
  destruct (group_by_edge rest) as [|[next group] later] eqn:grouped; intros rest_groups.
  - repeat constructor.
  - destruct rest as [|[rest_key rest_claim] rest_later]; [discriminate grouped|].
    pose proof (Forall_inv (Forall_inv_tail nonempty)) as rest_key_nonempty.
    cbn [fst] in rest_key_nonempty.
    destruct rest_key as [|rest_edge rest_tail]; [contradiction|].
    pose proof (group_by_edge_head _ _ _ _ _ _ _ grouped) as next_is. subst next.
    rewrite Forall_forall in head_before.
    pose proof (head_before (rest_edge :: rest_tail, rest_claim) (in_eq _ _)) as first_before.
    unfold key_before in first_before. cbn [fst] in first_before.
    assert (edge_le : edge <= rest_edge).
    { cbn [ChannelImage.bytes_ltb] in first_before.
      destruct (Nat.ltb_spec edge rest_edge); [lia|].
      destruct (Nat.ltb_spec rest_edge edge); [discriminate first_before|lia]. }
    apply StronglySorted_inv in rest_groups as [later_sorted next_below].
    rewrite Forall_forall in next_below.
    destruct (Nat.eqb_spec edge rest_edge) as [same|different].
    + subst rest_edge. constructor; [exact later_sorted|].
      apply Forall_forall. intros other member. exact (next_below other member).
    + constructor.
      * constructor; [exact later_sorted|]. apply Forall_forall. exact next_below.
      * constructor; [cbn [fst]; lia|].
        apply Forall_forall. intros other member.
        specialize (next_below other member). cbn [fst] in next_below |- *. lia.
Qed.

Lemma group_by_edge_groups :
  forall entries width,
    StronglySorted key_before entries ->
    Forall (fun entry => length (fst entry) = S width) entries ->
    forall group, In group (group_by_edge entries) ->
      StronglySorted key_before (snd group) /\
      Forall (fun entry => length (fst entry) = width) (snd group).
Proof.
  induction entries as [|[key claim] rest IH]; intros width sorted lengths target member;
    [destruct member|].
  pose proof (Forall_inv lengths) as key_length. cbn [fst] in key_length.
  apply StronglySorted_inv in sorted as [rest_sorted head_before].
  pose proof (Forall_inv_tail lengths) as rest_lengths.
  pose proof (IH width rest_sorted rest_lengths) as rest_groups.
  pose proof (full_width_keys_nonempty width rest rest_lengths) as rest_nonempty.
  destruct key as [|edge tail]; [discriminate key_length|].
  cbn [length] in key_length. injection key_length as tail_length.
  cbn [group_by_edge] in member. revert member.
  destruct (group_by_edge rest) as [|[next group] later] eqn:grouped; intros member.
  - destruct member as [same|[]]. subst target. cbn [snd].
    split; [repeat constructor|]. constructor; [exact tail_length|constructor].
  - destruct (Nat.eqb_spec edge next) as [same|different].
    + subst next. destruct member as [same_group|member].
      * subst target. cbn [snd].
        assert (group_member : In (edge, group) (group_by_edge rest))
          by (rewrite grouped; left; reflexivity).
        destruct (rest_groups (edge, group) ltac:(try rewrite grouped; left; reflexivity))
          as [group_sorted group_lengths].
        cbn [snd] in group_sorted, group_lengths.
        split.
        -- constructor; [exact group_sorted|].
           apply Forall_forall. intros [inner_key inner_claim] inner_member.
           pose proof (ungroup_member _ _ _ _ group_member inner_member) as in_rest.
           rewrite (group_by_edge_ungroup rest rest_nonempty) in in_rest.
           rewrite Forall_forall in head_before.
           specialize (head_before _ in_rest).
           unfold key_before, prepend_entry in *. cbn [fst] in *.
           change (edge :: tail) with ([edge] ++ tail) in head_before.
           rewrite bytes_ltb_same_prefix in head_before. exact head_before.
        -- constructor; [exact tail_length|exact group_lengths].
      * apply rest_groups. try rewrite grouped. right. exact member.
    + destruct member as [same_group|member].
      * subst target. cbn [snd]. split; [repeat constructor|].
        constructor; [exact tail_length|constructor].
      * apply rest_groups. try rewrite grouped. exact member.
Qed.

Lemma children_entries_groups :
  forall (entries_of : radix_tree -> list (list nat * nat))
         (build : list (list nat * nat) -> radix_tree) groups,
    (forall group, In group groups -> entries_of (build (snd group)) = snd group) ->
    children_entries entries_of (map (fun group => (fst group, build (snd group))) groups) =
      ungroup groups.
Proof.
  intros entries_of build groups same.
  induction groups as [|[edge group] later IH]; [reflexivity|].
  cbn [map children_entries fst snd]. unfold ungroup. cbn [flat_map fst snd].
  pose proof (same (edge, group) (in_eq _ _)) as here. cbn [snd] in here.
  rewrite here. f_equal.
  apply IH. intros other member. exact (same other (or_intror member)).
Qed.

Lemma partition_build_correct :
  forall fuel width entries,
    width <= fuel ->
    StronglySorted key_before entries ->
    Forall (fun entry => length (fst entry) = width) entries ->
    entries <> [] ->
    canonical width (partition_build fuel entries) /\
    tree_entries (partition_build fuel entries) = entries.
Proof.
  induction fuel as [|smaller IH]; intros width entries enough sorted lengths nonempty;
    (destruct entries as [|[key claim] [|[next_key next_claim] rest]];
     [contradiction| |]).
  - pose proof (Forall_inv lengths) as key_length. cbn [fst] in key_length.
    cbn [partition_build canonical tree_entries endpoint_entries children_entries map app].
    split; [exact key_length|reflexivity].
  - exfalso.
    pose proof (Forall_inv lengths) as key_length.
    pose proof (Forall_inv (Forall_inv_tail lengths)) as next_length.
    cbn [fst] in key_length, next_length.
    destruct key; [|cbn [length] in key_length; lia].
    destruct next_key; [|cbn [length] in next_length; lia].
    apply StronglySorted_inv in sorted as [_ head_before].
    pose proof (Forall_inv head_before) as before. discriminate before.
  - pose proof (Forall_inv lengths) as key_length. cbn [fst] in key_length.
    cbn [partition_build canonical tree_entries endpoint_entries children_entries map app].
    split; [exact key_length|reflexivity].
  - set (entries := (key, claim) :: (next_key, next_claim) :: rest) in *.
    set (shared := common_prefix key (last_key entries)).
    set (stripped := strip_entries (length shared) entries).
    set (groups := group_by_edge stripped).
    assert (built : partition_build (S smaller) entries =
      RadixTree shared None
        (map (fun group => (fst group, partition_build smaller (snd group))) groups))
      by reflexivity.
    rewrite built.
    assert (share : forall entry, In entry entries -> exists tail, fst entry = shared ++ tail).
    { intros entry member. exact (sorted_keys_share_common_prefix _ _ entry sorted member). }
    assert (last_member_entries : In (last entries ([], 0)) entries) by apply last_member.
    assert (first_before_last : ChannelImage.bytes_ltb key (last_key entries) = true).
    { pose proof sorted as sorted_copy.
      apply StronglySorted_inv in sorted_copy as [rest_sorted head_before].
      rewrite Forall_forall in head_before.
      pose proof (head_before (next_key, next_claim) (in_eq _ _)) as first_next.
      unfold key_before in first_next. cbn [fst] in first_next.
      destruct (strongly_sorted_last key_before entries ([], 0) (next_key, next_claim)
        sorted (in_cons _ _ _ (in_eq _ _))) as [same|next_last].
      - unfold last_key. rewrite <- same. exact first_next.
      - unfold key_before in next_last. cbn [fst] in next_last. unfold last_key.
        exact (bytes_ltb_transitive _ _ _ first_next next_last). }
    assert (key_not_last : key <> last_key entries).
    { intros same. rewrite <- same, ChannelImage.bytes_ltb_irreflexive in first_before_last.
      discriminate first_before_last. }
    pose proof (Forall_inv lengths) as key_length. cbn [fst] in key_length.
    assert (last_length : length (last_key entries) = width).
    { rewrite Forall_forall in lengths. exact (lengths _ last_member_entries). }
    assert (shared_short : length shared < width).
    { unfold shared. rewrite <- key_length.
      apply common_prefix_shorter; [congruence|exact key_not_last]. }
    assert (stripped_sorted : StronglySorted key_before stripped)
      by exact (strip_entries_sorted shared entries share sorted).
    assert (stripped_lengths :
      Forall (fun entry => length (fst entry) = S (width - S (length shared))) stripped).
    { pose proof (strip_entries_lengths (length shared) width entries lengths) as raw.
      eapply Forall_impl; [|exact raw]. intros entry entry_length. cbv beta in *. lia. }
    pose proof (full_width_keys_nonempty _ stripped stripped_lengths) as stripped_nonempty.
    assert (reassembled : ungroup groups = stripped)
      by exact (group_by_edge_ungroup stripped stripped_nonempty).
    assert (edges_ordered : StronglySorted (fun left right => fst left < fst right) groups)
      by exact (group_by_edge_edges_sorted stripped stripped_nonempty stripped_sorted).
    assert (group_facts : forall group, In group groups ->
      StronglySorted key_before (snd group) /\
      Forall (fun entry => length (fst entry) = width - S (length shared)) (snd group))
      by exact (group_by_edge_groups stripped _ stripped_sorted stripped_lengths).
    assert (group_nonempty : forall group, In group groups -> snd group <> [])
      by exact (group_by_edge_nonempty_groups stripped).
    assert (child_facts : forall group, In group groups ->
      canonical (width - S (length shared)) (partition_build smaller (snd group)) /\
      tree_entries (partition_build smaller (snd group)) = snd group).
    { intros group member. destruct (group_facts group member) as [group_sorted group_lengths].
      apply (IH (width - S (length shared))); [lia|exact group_sorted|exact group_lengths|].
      exact (group_nonempty group member). }
    assert (wide : 2 <= length groups).
    { destruct (common_prefix_split key (last_key entries))
        as [key_rest [last_rest [key_is [last_is diverge]]]].
      fold shared in key_is, last_is, diverge.
      pose proof (f_equal (@length nat) key_is) as key_rest_length.
      pose proof (f_equal (@length nat) last_is) as last_rest_length.
      rewrite length_app in key_rest_length, last_rest_length.
      destruct key_rest as [|key_edge key_tail]; [cbn [length] in key_rest_length; lia|].
      destruct last_rest as [|last_edge last_tail]; [cbn [length] in last_rest_length; lia|].
      destruct groups as [|[edge group] [|other later]] eqn:grouping.
      - exfalso. cbn [ungroup flat_map] in reassembled.
        assert (first_stripped : In (skipn (length shared) key, claim) stripped).
        { unfold stripped, strip_entries.
          apply (in_map (fun entry => (skipn (length shared) (fst entry), snd entry))
            entries (key, claim)). left. reflexivity. }
        rewrite <- reassembled in first_stripped. destruct first_stripped.
      - exfalso.
        assert (first_stripped : In (skipn (length shared) key, claim) stripped).
        { unfold stripped, strip_entries.
          apply (in_map (fun entry => (skipn (length shared) (fst entry), snd entry))
            entries (key, claim)). left. reflexivity. }
        assert (last_stripped : In (skipn (length shared) (last_key entries),
          snd (last entries ([], 0))) stripped).
        { unfold stripped, strip_entries.
          apply (in_map (fun entry => (skipn (length shared) (fst entry), snd entry))
            entries (last entries ([], 0))). exact last_member_entries. }
        rewrite <- reassembled in first_stripped, last_stripped.
        unfold ungroup in first_stripped, last_stripped.
        cbn [flat_map fst snd] in first_stripped, last_stripped.
        rewrite app_nil_r in first_stripped, last_stripped.
        apply in_map_iff in first_stripped as [[first_inner first_claim] [first_is _]].
        apply in_map_iff in last_stripped as [[last_inner last_claim] [last_is' _]].
        unfold prepend_entry in first_is, last_is'. cbn [fst snd app] in first_is, last_is'.
        injection first_is as first_key_is _. injection last_is' as last_key_is _.
        rewrite key_is, skipn_length_app in first_key_is.
        rewrite last_is, skipn_length_app in last_key_is.
        injection first_key_is as first_edge _. injection last_key_is as last_edge' _.
        apply diverge. congruence.
      - cbn [length]. lia. }
    split.
    + cbn [canonical]. split; [exact shared_short|]. split; [rewrite length_map; exact wide|].
      split.
      * apply (strongly_sorted_map (fun left right : nat * list (list nat * nat) =>
          fst left < fst right)); [|exact edges_ordered].
        intros x y below. cbn [fst]. exact below.
      * apply all_children_iff. intros edge child member.
        apply in_map_iff in member as [group [child_is group_member]].
        injection child_is as edge_is child_is. subst child.
        exact (proj1 (child_facts group group_member)).
    + cbn [tree_entries endpoint_entries app].
      rewrite (children_entries_groups tree_entries (partition_build smaller) groups).
      * rewrite reassembled. apply strip_entries_prepend. exact share.
      * intros group member. exact (proj2 (child_facts group member)).
Qed.

Lemma partition_build_nil : forall fuel, partition_build fuel [] = empty_tree.
Proof. intros [|fuel]; reflexivity. Qed.

(** Capstone T1: the partition build over sorted full-width entries is a
    canonical root whose entries are exactly the input, and it is the only
    canonical root with these entries. *)
Theorem canonical_radix_exists_and_is_unique :
  forall width entries,
    StronglySorted key_before entries ->
    Forall (fun entry => length (fst entry) = width) entries ->
    canonical_root width (partition_build width entries) /\
    tree_entries (partition_build width entries) = entries /\
    (forall tree, canonical_root width tree -> tree_entries tree = entries ->
       tree = partition_build width entries).
Proof.
  intros width entries sorted lengths.
  assert (built : canonical_root width (partition_build width entries) /\
                  tree_entries (partition_build width entries) = entries).
  { destruct entries as [|entry rest].
    { rewrite partition_build_nil. split; [left; reflexivity|reflexivity]. }
    destruct (partition_build_correct width width (entry :: rest) (le_n _) sorted lengths
      ltac:(discriminate)) as [shape same].
    split; [right; exact shape|exact same]. }
  destruct built as [shape same]. split; [exact shape|]. split; [exact same|].
  intros tree tree_shape tree_same.
  apply (canonical_root_entries_unique width); [exact tree_shape|exact shape|congruence].
Qed.

(** The unary mutant relaxes the inner-node rule to one child. *)
Fixpoint unary_canonical_mutant (width : nat) (tree : radix_tree) : Prop :=
  match tree with
  | RadixTree prefix (Some _) [] => length prefix = width
  | RadixTree _ (Some _) (_ :: _) => False
  | RadixTree prefix None children =>
      length prefix < width /\ 1 <= length children /\ edges_sorted children /\
      all_children (unary_canonical_mutant (width - S (length prefix))) children
  end.

Definition leaf_root_sample : radix_tree := RadixTree (repeat 0 48) (Some 50) [].

Definition unary_root_sample : radix_tree :=
  RadixTree [] None [(0, RadixTree (repeat 0 47) (Some 50) [])].

(** Capstone T1m (negative control): with unary inner nodes allowed, a leaf and a
    unary root denote the same single entry, so the shape no longer determines
    the bytes; the faithful rule refuses the unary root. *)
Theorem unary_inner_mutant_breaks_uniqueness :
  unary_canonical_mutant retention_key_len unary_root_sample /\
  unary_canonical_mutant retention_key_len leaf_root_sample /\
  tree_entries unary_root_sample = tree_entries leaf_root_sample /\
  unary_root_sample <> leaf_root_sample /\
  ~ canonical retention_key_len unary_root_sample.
Proof.
  split.
  { cbn [unary_canonical_mutant unary_root_sample all_children length].
    split; [unfold retention_key_len; lia|]. split; [lia|].
    split; [repeat constructor|]. split; [|exact I].
    rewrite repeat_length. reflexivity. }
  split.
  { cbn [unary_canonical_mutant leaf_root_sample]. rewrite repeat_length. reflexivity. }
  split; [reflexivity|]. split; [discriminate|].
  cbn [canonical unary_root_sample length]. intros (_ & wide & _). lia.
Qed.

(** * The editor *)

(** A CAS mutation [(key, before, after)] with [None] for [Absent] and
    [Some claim] for [Multiplicity(claim)]. *)
Definition cas_mutation : Type := (list nat * option nat * option nat)%type.

Definition endpoint_eqb (left right : option nat) : bool :=
  match left, right with
  | Some left_claim, Some right_claim => Nat.eqb left_claim right_claim
  | None, None => true
  | _, _ => false
  end.

Lemma endpoint_eqb_true_iff :
  forall left right, endpoint_eqb left right = true <-> left = right.
Proof.
  intros [left|] [right|]; cbn [endpoint_eqb]; split; intro same;
    try discriminate same; try reflexivity.
  - apply Nat.eqb_eq in same. subst. reflexivity.
  - injection same as same'. subst. apply Nat.eqb_refl.
Qed.

(** [prefix_split] is the editor's [lcp] test of a node prefix against the
    remaining key: either the prefix is matched, or it diverges after
    [shared], at the old byte [old_edge], from the rest of the key. *)
Inductive prefix_split_result :=
| PrefixMatched (rest : list nat)
| PrefixDiverged (shared : list nat) (old_edge : nat) (old_suffix rest : list nat).

Fixpoint prefix_split (prefix key : list nat) : prefix_split_result :=
  match prefix, key with
  | [], rest => PrefixMatched rest
  | byte :: prefix_rest, [] => PrefixDiverged [] byte prefix_rest []
  | byte :: prefix_rest, next :: key_rest =>
      if Nat.eqb byte next then
        match prefix_split prefix_rest key_rest with
        | PrefixMatched rest => PrefixMatched rest
        | PrefixDiverged shared old_edge old_suffix rest =>
            PrefixDiverged (byte :: shared) old_edge old_suffix rest
        end
      else PrefixDiverged [] byte prefix_rest (next :: key_rest)
  end.

(** A missing child is inserted as a leaf whose endpoint is [Absent], and the
    CAS is then checked against it; [fresh_leaf] is the outcome. *)
Definition fresh_leaf (rest : list nat) (before after : option nat) : option radix_tree :=
  match before with
  | None => Some (RadixTree rest after [])
  | Some _ => None
  end.

(** The sorted child slot search ([binary_search_by_key]) with insertion at
    the slot or descent into the found child. *)
Fixpoint edit_children (edit : radix_tree -> option radix_tree) (fresh : option radix_tree)
    (edge : nat) (children : list (nat * radix_tree)) : option (list (nat * radix_tree)) :=
  match children with
  | [] =>
      match fresh with
      | Some leaf => Some [(edge, leaf)]
      | None => None
      end
  | (other, child) :: rest =>
      if edge <? other then
        match fresh with
        | Some leaf => Some ((edge, leaf) :: (other, child) :: rest)
        | None => None
        end
      else if Nat.eqb edge other then
        match edit child with
        | Some edited => Some ((other, edited) :: rest)
        | None => None
        end
      else
        match edit_children edit fresh edge rest with
        | Some later => Some ((other, child) :: later)
        | None => None
        end
  end.

(** A diverging prefix is split: the split node keeps the shared bytes, has
    an [Absent] endpoint and gets the old node, shortened past its old byte,
    as its only child; the CAS then continues at the split node. *)
Definition split_edit (shared : list nat) (old_edge : nat) (old_node : radix_tree)
    (rest : list nat) (before after : option nat) : option radix_tree :=
  match rest with
  | [] =>
      if endpoint_eqb None before then Some (RadixTree shared after [(old_edge, old_node)])
      else None
  | edge :: tail =>
      match edit_children (fun _ => None) (fresh_leaf tail before after) edge
              [(old_edge, old_node)] with
      | Some children => Some (RadixTree shared None children)
      | None => None
      end
  end.

(** One CAS step of [RadixEditor::apply]: walk the key, split a diverging
    prefix, insert a missing child, and compare-and-set the endpoint. *)
Fixpoint edit_node (tree : radix_tree) (key : list nat) (before after : option nat)
    {struct tree} : option radix_tree :=
  match tree with
  | RadixTree prefix endpoint children =>
      match prefix_split prefix key with
      | PrefixMatched [] =>
          if endpoint_eqb endpoint before then Some (RadixTree prefix after children)
          else None
      | PrefixMatched (edge :: rest) =>
          match edit_children (fun child => edit_node child rest before after)
                  (fresh_leaf rest before after) edge children with
          | Some edited => Some (RadixTree prefix endpoint edited)
          | None => None
          end
      | PrefixDiverged shared old_edge old_suffix rest =>
          split_edit shared old_edge (RadixTree old_suffix endpoint children) rest
            before after
      end
  end.

Fixpoint edit_batch (tree : radix_tree) (batch : list cas_mutation) : option radix_tree :=
  match batch with
  | [] => Some tree
  | (key, before, after) :: rest =>
      match edit_node tree key before after with
      | Some next => edit_batch next rest
      | None => None
      end
  end.

(** Finalization: children finalize first; an empty child is pruned; an
    [Absent] node without children is empty (the empty node at the root); an
    [Absent] node with one child merges with it, the root included. *)
Fixpoint finalize_children (finalize : radix_tree -> option radix_tree)
    (children : list (nat * radix_tree)) : list (nat * radix_tree) :=
  match children with
  | [] => []
  | (edge, child) :: rest =>
      match finalize child with
      | Some kept => (edge, kept) :: finalize_children finalize rest
      | None => finalize_children finalize rest
      end
  end.

Definition merge_unary (prefix : list nat) (endpoint : option nat)
    (kept : list (nat * radix_tree)) : option radix_tree :=
  match endpoint, kept with
  | None, [] => None
  | None, [(edge, RadixTree child_prefix child_endpoint child_children)] =>
      Some (RadixTree (prefix ++ edge :: child_prefix) child_endpoint child_children)
  | _, _ => Some (RadixTree prefix endpoint kept)
  end.

Fixpoint finalize_node (tree : radix_tree) : option radix_tree :=
  match tree with
  | RadixTree prefix endpoint children =>
      merge_unary prefix endpoint (finalize_children finalize_node children)
  end.

Definition finalize_root (tree : radix_tree) : radix_tree :=
  match finalize_node tree with
  | Some final => final
  | None => empty_tree
  end.

(** [rewrite_root]: an empty batch returns the root unchanged; otherwise every
    CAS is applied in order (a refused CAS returns before anything is staged,
    because staging happens only in [finish]) and the result is finalized. *)
Definition radix_edit (tree : radix_tree) (batch : list cas_mutation) : option radix_tree :=
  match batch with
  | [] => Some tree
  | _ :: _ =>
      match edit_batch tree batch with
      | Some edited => Some (finalize_root edited)
      | None => None
      end
  end.

(** ** Child lists *)

Fixpoint replace_child (edge : nat) (edited : radix_tree)
    (children : list (nat * radix_tree)) : list (nat * radix_tree) :=
  match children with
  | [] => []
  | (other, child) :: rest =>
      if Nat.eqb edge other then (other, edited) :: rest
      else (other, child) :: replace_child edge edited rest
  end.

Fixpoint insert_child (edge : nat) (leaf : radix_tree)
    (children : list (nat * radix_tree)) : list (nat * radix_tree) :=
  match children with
  | [] => [(edge, leaf)]
  | (other, child) :: rest =>
      if edge <? other then (edge, leaf) :: (other, child) :: rest
      else (other, child) :: insert_child edge leaf rest
  end.

Fixpoint find_child (edge : nat) (children : list (nat * radix_tree)) : option radix_tree :=
  match children with
  | [] => None
  | (other, child) :: rest => if Nat.eqb edge other then Some child else find_child edge rest
  end.

Lemma find_child_some :
  forall edge children child, find_child edge children = Some child -> In (edge, child) children.
Proof.
  intros edge children child.
  induction children as [|[other target] rest IH]; cbn [find_child];
    [intro impossible; discriminate impossible|].
  destruct (Nat.eqb_spec edge other) as [same|different]; intro found.
  - injection found as same_child. subst. left. reflexivity.
  - right. exact (IH found).
Qed.

Lemma find_child_none :
  forall edge children, find_child edge children = None ->
    forall child, ~ In (edge, child) children.
Proof.
  intros edge children.
  induction children as [|[other target] rest IH]; cbn [find_child]; [intros _ child []|].
  destruct (Nat.eqb_spec edge other) as [same|different]; intros missing child member;
    [discriminate missing|].
  destruct member as [same|member].
  - injection same as same_edge _. congruence.
  - exact (IH missing child member).
Qed.

Lemma edit_children_present :
  forall edit fresh edge old children,
    edges_sorted children -> In (edge, old) children ->
    edit_children edit fresh edge children =
      match edit old with
      | Some edited => Some (replace_child edge edited children)
      | None => None
      end.
Proof.
  intros edit fresh edge old children sorted member.
  induction children as [|[other child] rest IH]; [destruct member|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  rewrite Forall_forall in head_below.
  cbn [edit_children replace_child].
  destruct member as [same|member].
  - injection same as same_edge same_child. subst other child.
    rewrite Nat.ltb_irrefl, Nat.eqb_refl. destruct (edit old); reflexivity.
  - pose proof (head_below (edge, old) member) as above. cbn [fst] in above.
    assert (not_below : (edge <? other) = false) by (apply Nat.ltb_ge; lia).
    assert (not_same : Nat.eqb edge other = false) by (apply Nat.eqb_neq; lia).
    rewrite not_below, not_same. rewrite (IH rest_sorted member).
    destruct (edit old); reflexivity.
Qed.

Lemma edit_children_absent :
  forall edit fresh edge children,
    edges_sorted children -> (forall old, ~ In (edge, old) children) ->
    edit_children edit fresh edge children =
      match fresh with
      | Some leaf => Some (insert_child edge leaf children)
      | None => None
      end.
Proof.
  intros edit fresh edge children sorted absent.
  induction children as [|[other child] rest IH]; cbn [edit_children insert_child];
    [destruct fresh; reflexivity|].
  apply StronglySorted_inv in sorted as [rest_sorted _].
  destruct (Nat.ltb_spec edge other) as [below|above]; [destruct fresh; reflexivity|].
  destruct (Nat.eqb_spec edge other) as [same|different].
  - subst other. exfalso. apply (absent child). left. reflexivity.
  - rewrite IH; [destruct fresh; reflexivity|exact rest_sorted|].
    intros old member. apply (absent old). right. exact member.
Qed.

Lemma child_lookup_replace :
  forall (lookup_in : radix_tree -> option nat) edge edited old children query,
    In (edge, old) children ->
    child_lookup lookup_in query (replace_child edge edited children) =
      if Nat.eqb query edge then lookup_in edited
      else child_lookup lookup_in query children.
Proof.
  intros lookup_in edge edited old children query member.
  induction children as [|[other child] rest IH]; [destruct member|].
  cbn [replace_child child_lookup].
  destruct (Nat.eqb_spec edge other) as [same|different].
  - subst other. cbn [child_lookup].
    destruct (Nat.eqb_spec query edge); reflexivity.
  - destruct member as [same|member]; [injection same as same_edge _; congruence|].
    cbn [child_lookup]. rewrite (IH member).
    destruct (Nat.eqb_spec query other) as [at_other|not_other];
      destruct (Nat.eqb_spec query edge) as [at_edge|not_edge]; try reflexivity.
    congruence.
Qed.

Lemma child_lookup_insert :
  forall (lookup_in : radix_tree -> option nat) edge leaf children query,
    (forall old, ~ In (edge, old) children) ->
    child_lookup lookup_in query (insert_child edge leaf children) =
      if Nat.eqb query edge then lookup_in leaf
      else child_lookup lookup_in query children.
Proof.
  intros lookup_in edge leaf children query absent.
  induction children as [|[other child] rest IH]; cbn [insert_child child_lookup];
    [reflexivity|].
  destruct (Nat.ltb_spec edge other) as [below|above]; cbn [child_lookup]; [reflexivity|].
  assert (different : edge <> other).
  { intros same. subst other. apply (absent child). left. reflexivity. }
  rewrite IH by (intros old member; apply (absent old); right; exact member).
  destruct (Nat.eqb_spec query other) as [at_other|not_other];
    destruct (Nat.eqb_spec query edge) as [at_edge|not_edge]; try reflexivity.
  congruence.
Qed.

Lemma replace_child_member :
  forall edge edited children other child,
    In (other, child) (replace_child edge edited children) ->
    (other = edge /\ child = edited /\ exists old, In (edge, old) children) \/
    In (other, child) children.
Proof.
  intros edge edited children other child.
  induction children as [|[next target] rest IH]; cbn [replace_child]; [intros []|].
  destruct (Nat.eqb_spec edge next) as [same|different]; intros [here|there].
  - injection here as other_is child_is. subst.
    left. split; [reflexivity|]. split; [reflexivity|]. exists target. left. reflexivity.
  - right. right. exact there.
  - right. left. exact here.
  - destruct (IH there) as [(other_is & child_is & old & old_member)|old].
    + left. split; [exact other_is|]. split; [exact child_is|].
      exists old. right. exact old_member.
    + right. right. exact old.
Qed.

Lemma replace_child_in :
  forall edge edited children old,
    In (edge, old) children -> In (edge, edited) (replace_child edge edited children).
Proof.
  intros edge edited children old member.
  induction children as [|[next target] rest IH]; [destruct member|].
  cbn [replace_child].
  destruct (Nat.eqb_spec edge next) as [same|different].
  - subst next. left. reflexivity.
  - destruct member as [here|there]; [injection here as next_is _; congruence|].
    right. exact (IH there).
Qed.

Lemma insert_child_member :
  forall edge leaf children other child,
    In (other, child) (insert_child edge leaf children) ->
    (other = edge /\ child = leaf) \/ In (other, child) children.
Proof.
  intros edge leaf children other child.
  induction children as [|[next target] rest IH]; cbn [insert_child].
  - intros [here|[]]. injection here as other_is child_is. subst. left. split; reflexivity.
  - destruct (Nat.ltb_spec edge next) as [below|above]; intros [here|there].
    + injection here as other_is child_is. subst. left. split; reflexivity.
    + right. exact there.
    + right. left. exact here.
    + destruct (IH there) as [inserted|old]; [left; exact inserted|right; right; exact old].
Qed.

Lemma insert_child_in :
  forall edge leaf children, In (edge, leaf) (insert_child edge leaf children).
Proof.
  intros edge leaf children.
  induction children as [|[next target] rest IH]; cbn [insert_child]; [left; reflexivity|].
  destruct (edge <? next); [left; reflexivity|right; exact IH].
Qed.

Lemma replace_child_sorted :
  forall edge edited children,
    edges_sorted children -> edges_sorted (replace_child edge edited children).
Proof.
  intros edge edited children sorted.
  induction children as [|[other child] rest IH]; cbn [replace_child]; [constructor|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  rewrite Forall_forall in head_below.
  destruct (Nat.eqb_spec edge other) as [same|different].
  - constructor; [exact rest_sorted|]. apply Forall_forall. intros next member.
    exact (head_below next member).
  - constructor; [exact (IH rest_sorted)|]. apply Forall_forall.
    intros [next target] member. cbn [fst].
    destruct (replace_child_member _ _ _ _ _ member) as [(next_is & _ & old & old_member)|old].
    + subst next. exact (head_below (edge, old) old_member).
    + exact (head_below (next, target) old).
Qed.

Lemma insert_child_sorted :
  forall edge leaf children,
    edges_sorted children -> (forall old, ~ In (edge, old) children) ->
    edges_sorted (insert_child edge leaf children).
Proof.
  intros edge leaf children sorted absent.
  induction children as [|[other child] rest IH]; cbn [insert_child].
  - constructor; constructor.
  - apply StronglySorted_inv in sorted as [rest_sorted head_below].
    rewrite Forall_forall in head_below.
    destruct (Nat.ltb_spec edge other) as [below|above].
    + constructor.
      * constructor; [exact rest_sorted|]. apply Forall_forall. exact head_below.
      * apply Forall_forall. intros [next target] [same|member]; cbn [fst].
        -- injection same as next_is _. subst next. exact below.
        -- pose proof (head_below (next, target) member) as other_below.
           cbn [fst] in other_below. lia.
    + assert (different : edge <> other).
      { intros same. subst other. apply (absent child). left. reflexivity. }
      constructor.
      * apply IH; [exact rest_sorted|]. intros old member. apply (absent old).
        right. exact member.
      * apply Forall_forall. intros [next target] member. cbn [fst].
        destruct (insert_child_member _ _ _ _ _ member) as [[next_is _]|old].
        -- subst next. lia.
        -- exact (head_below (next, target) old).
Qed.

(** ** Lookups through edited nodes *)

Lemma strip_prefix_app_split :
  forall first second key,
    strip_prefix (first ++ second) key =
      match strip_prefix first key with
      | Some rest => strip_prefix second rest
      | None => None
      end.
Proof.
  induction first as [|byte first_rest IH]; intros second key; [reflexivity|].
  destruct key as [|next key_rest]; cbn [app strip_prefix]; [reflexivity|].
  destruct (Nat.eqb byte next); [apply IH|reflexivity].
Qed.

Lemma strip_prefix_app_same :
  forall shared left right,
    strip_prefix (shared ++ left) (shared ++ right) = strip_prefix left right.
Proof.
  induction shared as [|byte rest IH]; intros left right; [reflexivity|].
  cbn [app strip_prefix]. rewrite Nat.eqb_refl. apply IH.
Qed.

Lemma bytes_eqb_app_same :
  forall shared left right, bytes_eqb (shared ++ left) (shared ++ right) = bytes_eqb left right.
Proof.
  induction shared as [|byte rest IH]; intros left right; [reflexivity|].
  cbn [app bytes_eqb]. rewrite Nat.eqb_refl. cbn [andb]. apply IH.
Qed.

Lemma bytes_eqb_extension_false :
  forall prefix edge rest, bytes_eqb (prefix ++ edge :: rest) prefix = false.
Proof.
  intros prefix edge rest. apply bytes_eqb_false_iff. intro same.
  apply (f_equal (@length nat)) in same. rewrite length_app in same.
  cbn [length] in same. lia.
Qed.

Lemma bytes_eqb_extension_false_right :
  forall prefix edge rest, bytes_eqb prefix (prefix ++ edge :: rest) = false.
Proof.
  intros prefix edge rest. apply bytes_eqb_false_iff. intro same.
  apply (f_equal (@length nat)) in same. rewrite length_app in same.
  cbn [length] in same. lia.
Qed.

Definition node_below (endpoint : option nat) (children : list (nat * radix_tree))
    (rest : list nat) : option nat :=
  match rest with
  | [] => endpoint
  | next :: later => child_lookup (fun child => tree_lookup child later) next children
  end.

Lemma tree_lookup_node_below :
  forall prefix endpoint children key,
    tree_lookup (RadixTree prefix endpoint children) key =
      match strip_prefix prefix key with
      | Some rest => node_below endpoint children rest
      | None => None
      end.
Proof.
  intros prefix endpoint children key. cbn [tree_lookup].
  destruct (strip_prefix prefix key) as [[|next rest]|]; reflexivity.
Qed.

Lemma tree_lookup_prefix_app :
  forall first second endpoint children key,
    tree_lookup (RadixTree (first ++ second) endpoint children) key =
      match strip_prefix first key with
      | Some rest => tree_lookup (RadixTree second endpoint children) rest
      | None => None
      end.
Proof.
  intros first second endpoint children key.
  rewrite !tree_lookup_node_below. rewrite strip_prefix_app_split.
  destruct (strip_prefix first key) as [rest|]; [|reflexivity].
  rewrite tree_lookup_node_below. reflexivity.
Qed.

Lemma tree_lookup_cons_prefix :
  forall edge suffix endpoint children next rest,
    tree_lookup (RadixTree (edge :: suffix) endpoint children) (next :: rest) =
      if Nat.eqb edge next then tree_lookup (RadixTree suffix endpoint children) rest
      else None.
Proof.
  intros edge suffix endpoint children next rest.
  rewrite !tree_lookup_node_below. cbn [strip_prefix].
  destruct (Nat.eqb edge next); reflexivity.
Qed.

Lemma node_lookup_extension :
  forall prefix endpoint children edge rest,
    tree_lookup (RadixTree prefix endpoint children) (prefix ++ edge :: rest) =
      child_lookup (fun child => tree_lookup child rest) edge children.
Proof.
  intros prefix endpoint children edge rest.
  rewrite tree_lookup_node_below, strip_prefix_app. reflexivity.
Qed.

Lemma leaf_lookup :
  forall suffix after key,
    tree_lookup (RadixTree suffix after []) key = if bytes_eqb key suffix then after else None.
Proof.
  intros suffix after key. rewrite tree_lookup_node_below.
  destruct (strip_prefix suffix key) as [[|edge rest]|] eqn:stripped; cbn [node_below].
  - apply strip_prefix_some in stripped. rewrite app_nil_r in stripped. subst key.
    rewrite bytes_eqb_refl. reflexivity.
  - apply strip_prefix_some in stripped. subst key.
    rewrite bytes_eqb_extension_false. reflexivity.
  - destruct (bytes_eqb key suffix) eqn:same; [|reflexivity].
    apply bytes_eqb_true_iff in same. subst key.
    rewrite strip_prefix_self in stripped. discriminate stripped.
Qed.

Lemma empty_tree_lookup : forall key, tree_lookup empty_tree key = None.
Proof. intros [|edge rest]; reflexivity. Qed.

Lemma endpoint_lookup_update :
  forall prefix endpoint after children key,
    tree_lookup (RadixTree prefix after children) key =
      if bytes_eqb key prefix then after
      else tree_lookup (RadixTree prefix endpoint children) key.
Proof.
  intros prefix endpoint after children key. rewrite !tree_lookup_node_below.
  destruct (strip_prefix prefix key) as [[|edge rest]|] eqn:stripped; cbn [node_below].
  - apply strip_prefix_some in stripped. rewrite app_nil_r in stripped. subst key.
    rewrite bytes_eqb_refl. reflexivity.
  - apply strip_prefix_some in stripped. subst key.
    rewrite bytes_eqb_extension_false. reflexivity.
  - destruct (bytes_eqb key prefix) eqn:same; [|reflexivity].
    apply bytes_eqb_true_iff in same. subst key.
    rewrite strip_prefix_self in stripped. discriminate stripped.
Qed.

(** The common shape of every lookup update below a node: a node with prefix
    [shared] whose children answer the update at [edge :: tail] and otherwise
    the old lookup [below]. *)
Lemma node_lookup_update :
  forall shared endpoint children (old_lookup below : list nat -> option nat)
         edge tail after,
    (forall key, old_lookup key =
       match strip_prefix shared key with Some rest => below rest | None => None end) ->
    endpoint = below [] ->
    (forall next rest,
       child_lookup (fun child => tree_lookup child rest) next children =
         if Nat.eqb next edge && bytes_eqb rest tail then after else below (next :: rest)) ->
    forall key,
      tree_lookup (RadixTree shared endpoint children) key =
        if bytes_eqb key (shared ++ edge :: tail) then after else old_lookup key.
Proof.
  intros shared endpoint children old_lookup below edge tail after old_is endpoint_is
    children_are key.
  rewrite old_is, tree_lookup_node_below.
  destruct (strip_prefix shared key) as [[|next rest]|] eqn:stripped; cbn [node_below].
  - apply strip_prefix_some in stripped. rewrite app_nil_r in stripped. subst key.
    rewrite bytes_eqb_extension_false_right. exact endpoint_is.
  - apply strip_prefix_some in stripped. subst key.
    rewrite bytes_eqb_app_same. cbn [bytes_eqb]. apply children_are.
  - destruct (bytes_eqb key (shared ++ edge :: tail)) eqn:same; [|reflexivity].
    apply bytes_eqb_true_iff in same. subst key.
    rewrite strip_prefix_app in stripped. discriminate stripped.
Qed.

(** ** The CAS step *)

Lemma prefix_split_matched :
  forall prefix key rest, prefix_split prefix key = PrefixMatched rest -> key = prefix ++ rest.
Proof.
  induction prefix as [|byte prefix_rest IH]; intros key rest split_is.
  - cbn [prefix_split] in split_is. injection split_is as rest_is. subst rest. reflexivity.
  - destruct key as [|next key_rest]; cbn [prefix_split] in split_is;
      [discriminate split_is|].
    destruct (Nat.eqb_spec byte next) as [same|different]; [|discriminate split_is].
    destruct (prefix_split prefix_rest key_rest) as [inner|shared old_edge old_suffix inner]
      eqn:inner_split; [|discriminate split_is].
    injection split_is as inner_is. subst inner next.
    rewrite (IH key_rest rest inner_split). reflexivity.
Qed.

Lemma prefix_split_diverged :
  forall prefix key shared old_edge old_suffix rest,
    prefix_split prefix key = PrefixDiverged shared old_edge old_suffix rest ->
    prefix = shared ++ old_edge :: old_suffix /\ key = shared ++ rest /\
    (forall next tail, rest = next :: tail -> next <> old_edge).
Proof.
  induction prefix as [|byte prefix_rest IH];
    intros key shared old_edge old_suffix rest split_is;
    [cbn [prefix_split] in split_is; discriminate split_is|].
  destruct key as [|next key_rest]; cbn [prefix_split] in split_is.
  - injection split_is as shared_is edge_is suffix_is rest_is. subst.
    split; [reflexivity|]. split; [reflexivity|]. intros next tail impossible.
    discriminate impossible.
  - destruct (Nat.eqb_spec byte next) as [same|different].
    + destruct (prefix_split prefix_rest key_rest)
        as [inner|inner_shared inner_edge inner_suffix inner_rest] eqn:inner_split;
        [discriminate split_is|].
      injection split_is as shared_is edge_is suffix_is rest_is. subst.
      destruct (IH key_rest inner_shared old_edge old_suffix rest inner_split)
        as (prefix_is & key_is & differs).
      split; [rewrite prefix_is; reflexivity|]. split; [rewrite key_is; reflexivity|].
      exact differs.
    + injection split_is as shared_is edge_is suffix_is rest_is. subst.
      split; [reflexivity|]. split; [reflexivity|].
      intros other tail rest_is. injection rest_is as other_is _. subst other.
      intro same. apply different. symmetry. exact same.
Qed.

Lemma fixed_width_leaf :
  forall width suffix after, length suffix = width -> fixed_width width (RadixTree suffix after []).
Proof.
  intros width suffix after suffix_length. cbn [fixed_width].
  split; [lia|]. split; [intros _; reflexivity|]. split; [intros below; exfalso; lia|].
  split; [constructor|exact I].
Qed.

(** The CAS step on lookups: on a fixed-width tree, [edit_node] succeeds
    exactly when the key's current endpoint is [before]; it then keeps the
    fixed width and updates the lookup at the key alone. *)
Lemma edit_node_spec :
  forall tree width key before after,
    fixed_width width tree -> length key = width ->
    match edit_node tree key before after with
    | Some edited =>
        tree_lookup tree key = before /\ fixed_width width edited /\
        (forall query, tree_lookup edited query =
           if bytes_eqb query key then after else tree_lookup tree query)
    | None => tree_lookup tree key <> before
    end.
Proof.
  intros tree.
  induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width key before after shaped key_length.
  pose proof shaped as shaped_copy.
  cbn [fixed_width] in shaped.
  destruct shaped as (prefix_bound & full_leaf & short_absent & sorted & child_shapes).
  rewrite all_children_iff in child_shapes.
  cbn [edit_node].
  destruct (prefix_split prefix key) as [rest|shared old_edge old_suffix rest] eqn:split_is.
  - pose proof (prefix_split_matched _ _ _ split_is) as key_is. subst key.
    destruct rest as [|edge tail].
    + rewrite app_nil_r in key_length |- *.
      pose proof (full_leaf key_length) as no_children.
      destruct (endpoint_eqb endpoint before) eqn:cas.
      * apply endpoint_eqb_true_iff in cas. subst before.
        split; [rewrite tree_lookup_node_below, strip_prefix_self; reflexivity|].
        split.
        { cbn [fixed_width]. split; [exact prefix_bound|]. split; [exact full_leaf|].
          split; [intros below; exfalso; lia|]. split; [exact sorted|].
          apply all_children_iff. exact child_shapes. }
        intros query. apply endpoint_lookup_update.
      * rewrite tree_lookup_node_below, strip_prefix_self. cbn [node_below].
        intros same. subst before. rewrite (proj2 (endpoint_eqb_true_iff endpoint endpoint)
          eq_refl) in cas. discriminate cas.
    + rewrite length_app in key_length. cbn [length] in key_length.
      assert (tail_length : length tail = width - S (length prefix)) by lia.
      assert (endpoint_none : endpoint = None) by (apply short_absent; lia).
      subst endpoint.
      destruct (find_child edge children) as [old|] eqn:found.
      * pose proof (find_child_some _ _ _ found) as member.
        rewrite (edit_children_present _ _ _ old _ sorted member). cbv beta.
        pose proof (IH (edge, old) member (width - S (length prefix)) tail before after
          (child_shapes edge old member) tail_length) as child_spec.
        cbn [snd] in child_spec.
        destruct (edit_node old tail before after) as [edited|] eqn:edited_is;
          cbv beta iota.
        -- destruct child_spec as (child_before & edited_shape & edited_lookup).
           split; [rewrite (member_lookup _ _ _ _ _ _ sorted member); exact child_before|].
           split.
           { cbn [fixed_width]. split; [exact prefix_bound|].
             split; [intros full; exfalso; lia|]. split; [intros _; reflexivity|].
             split; [apply replace_child_sorted; exact sorted|].
             apply all_children_iff. intros other child member'.
             destruct (replace_child_member _ _ _ _ _ member')
               as [(other_is & child_is & _)|original].
             - subst child. exact edited_shape.
             - exact (child_shapes other child original). }
           intros query.
           apply (node_lookup_update prefix None _
                    (tree_lookup (RadixTree prefix None children))
                    (node_below None children)).
           ++ exact (tree_lookup_node_below prefix None children).
           ++ reflexivity.
           ++ intros next rest.
              rewrite (child_lookup_replace _ edge edited old children next member).
              cbn [node_below].
              destruct (Nat.eqb_spec next edge) as [same|different].
              ** subst next. cbn [andb]. rewrite (edited_lookup rest).
                 rewrite (child_lookup_member _ children edge old sorted member).
                 reflexivity.
              ** reflexivity.
        -- rewrite (member_lookup _ _ _ _ _ _ sorted member). exact child_spec.
      * pose proof (find_child_none _ _ found) as absent.
        rewrite (edit_children_absent _ _ _ _ sorted absent).
        unfold fresh_leaf.
        destruct before as [claim|]; cbv beta iota.
        -- rewrite node_lookup_extension, (child_lookup_absent _ _ _ absent).
           discriminate.
        -- split; [rewrite node_lookup_extension, (child_lookup_absent _ _ _ absent);
                   reflexivity|].
           split.
           { cbn [fixed_width]. split; [exact prefix_bound|].
             split; [intros full; exfalso; lia|]. split; [intros _; reflexivity|].
             split; [apply insert_child_sorted; [exact sorted|exact absent]|].
             apply all_children_iff. intros other child member'.
             destruct (insert_child_member _ _ _ _ _ member')
               as [(other_is & child_is)|original].
             - subst child. apply fixed_width_leaf. exact tail_length.
             - exact (child_shapes other child original). }
           intros query.
           apply (node_lookup_update prefix None _
                    (tree_lookup (RadixTree prefix None children))
                    (node_below None children)).
           ++ exact (tree_lookup_node_below prefix None children).
           ++ reflexivity.
           ++ intros next rest.
              rewrite (child_lookup_insert _ edge _ children next absent).
              cbn [node_below].
              destruct (Nat.eqb_spec next edge) as [same|different].
              ** subst next. cbn [andb]. rewrite leaf_lookup.
                 rewrite (child_lookup_absent _ children edge absent). reflexivity.
              ** reflexivity.
  - destruct (prefix_split_diverged _ _ _ _ _ _ split_is) as (prefix_is & key_is & differs).
    subst prefix key.
    rewrite length_app in prefix_bound. cbn [length] in prefix_bound.
    destruct rest as [|next tail].
    + rewrite app_nil_r in key_length. exfalso. lia.
    + pose proof (differs next tail eq_refl) as next_differs.
      rewrite length_app in key_length. cbn [length] in key_length.
      assert (absent : forall old,
        ~ In (next, old) [(old_edge, RadixTree old_suffix endpoint children)]).
      { intros old [same|[]]. injection same as same_edge _. congruence. }
      assert (singleton_sorted :
        edges_sorted [(old_edge, RadixTree old_suffix endpoint children)])
        by (constructor; constructor).
      assert (old_lookup_none :
        tree_lookup (RadixTree (shared ++ old_edge :: old_suffix) endpoint children)
          (shared ++ next :: tail) = None).
      { rewrite tree_lookup_prefix_app, strip_prefix_app. cbv beta iota.
        rewrite tree_lookup_cons_prefix.
        destruct (Nat.eqb_spec old_edge next) as [same|_]; [congruence|reflexivity]. }
      unfold split_edit.
      rewrite (edit_children_absent _ _ _ _ singleton_sorted absent).
      unfold fresh_leaf.
      destruct before as [claim|]; cbv beta iota.
      * rewrite old_lookup_none. discriminate.
      * split; [exact old_lookup_none|].
        split.
        { cbn [fixed_width]. split; [lia|]. split; [intros full; exfalso; lia|].
          split; [intros _; reflexivity|].
          split; [apply insert_child_sorted; [exact singleton_sorted|exact absent]|].
          apply all_children_iff. intros other child member'.
          destruct (insert_child_member _ _ _ _ _ member')
            as [(other_is & child_is)|[original|[]]].
          - subst child. apply fixed_width_leaf. lia.
          - injection original as other_is child_is. subst child.
            cbn [fixed_width]. split; [lia|].
            split; [intros full; apply full_leaf; rewrite length_app; cbn [length]; lia|].
            split; [intros below; apply short_absent; rewrite length_app; cbn [length]; lia|].
            split; [exact sorted|]. apply all_children_iff.
            intros edge child member.
            replace (width - S (length shared) - S (length old_suffix))
              with (width - S (length (shared ++ old_edge :: old_suffix)))
              by (rewrite length_app; cbn [length]; lia).
            exact (child_shapes edge child member). }
        intros query.
        apply (node_lookup_update shared None _
                 (tree_lookup (RadixTree (shared ++ old_edge :: old_suffix) endpoint children))
                 (tree_lookup (RadixTree (old_edge :: old_suffix) endpoint children))).
        -- exact (tree_lookup_prefix_app shared (old_edge :: old_suffix) endpoint children).
        -- reflexivity.
        -- intros other rest.
           rewrite (child_lookup_insert _ next _ _ other absent).
           rewrite tree_lookup_cons_prefix. cbn [child_lookup].
           destruct (Nat.eqb_spec other next) as [same|different].
           ++ subst other. cbn [andb]. rewrite leaf_lookup.
              destruct (Nat.eqb_spec old_edge next) as [same_edge|_]; [congruence|].
              reflexivity.
           ++ rewrite Nat.eqb_sym. reflexivity.
Qed.

(** ** Finalization *)

Lemma finalize_children_member :
  forall (finalize : radix_tree -> option radix_tree) children edge kept,
    In (edge, kept) (finalize_children finalize children) ->
    exists child, In (edge, child) children /\ finalize child = Some kept.
Proof.
  intros finalize children edge kept.
  induction children as [|[other child] rest IH]; cbn [finalize_children]; [intros []|].
  destruct (finalize child) as [final|] eqn:finalized.
  - intros [here|there].
    + injection here as edge_is kept_is. subst.
      exists child. split; [left; reflexivity|exact finalized].
    + destruct (IH there) as (source & member & source_final).
      exists source. split; [right; exact member|exact source_final].
  - intros there. destruct (IH there) as (source & member & source_final).
    exists source. split; [right; exact member|exact source_final].
Qed.

Lemma finalize_children_sorted :
  forall finalize children,
    edges_sorted children -> edges_sorted (finalize_children finalize children).
Proof.
  intros finalize children sorted.
  induction children as [|[edge child] rest IH]; cbn [finalize_children]; [constructor|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  rewrite Forall_forall in head_below.
  destruct (finalize child) as [kept|]; [|exact (IH rest_sorted)].
  constructor; [exact (IH rest_sorted)|].
  apply Forall_forall. intros [other target] member. cbn [fst].
  destruct (finalize_children_member _ _ _ _ member) as (source & source_member & _).
  exact (head_below (other, source) source_member).
Qed.

Lemma finalize_children_lookup :
  forall (finalize : radix_tree -> option radix_tree)
         (lookup_in : radix_tree -> option nat) children,
    edges_sorted children ->
    (forall edge child, In (edge, child) children ->
       match finalize child with
       | Some kept => lookup_in kept = lookup_in child
       | None => lookup_in child = None
       end) ->
    forall query,
      child_lookup lookup_in query (finalize_children finalize children) =
        child_lookup lookup_in query children.
Proof.
  intros finalize lookup_in children sorted preserved query.
  induction children as [|[edge child] rest IH]; [reflexivity|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  rewrite Forall_forall in head_below.
  assert (rest_preserved : forall other target, In (other, target) rest ->
    match finalize target with
    | Some kept => lookup_in kept = lookup_in target
    | None => lookup_in target = None
    end) by (intros other target member; exact (preserved other target (or_intror member))).
  pose proof (preserved edge child (or_introl eq_refl)) as here.
  cbn [finalize_children child_lookup].
  destruct (finalize child) as [kept|] eqn:finalized; cbv beta iota in here.
  - cbn [child_lookup]. rewrite (IH rest_sorted rest_preserved).
    destruct (Nat.eqb query edge); [exact here|reflexivity].
  - rewrite (IH rest_sorted rest_preserved).
    destruct (Nat.eqb_spec query edge) as [same|different]; [|reflexivity].
    subst query. rewrite here.
    apply child_lookup_absent. intros target member.
    pose proof (head_below (edge, target) member) as below. cbn [fst] in below. lia.
Qed.

Lemma finalize_children_identity :
  forall (finalize : radix_tree -> option radix_tree) children,
    (forall edge child, In (edge, child) children -> finalize child = Some child) ->
    finalize_children finalize children = children.
Proof.
  intros finalize children same.
  induction children as [|[edge child] rest IH]; [reflexivity|].
  cbn [finalize_children]. rewrite (same edge child (or_introl eq_refl)).
  f_equal. apply IH. intros other target member. exact (same other target (or_intror member)).
Qed.

(** The finalizer establishes the cold reader's shape rules and keeps every
    lookup; a subtree that finalizes to nothing denotes no entry. *)
Lemma finalize_node_spec :
  forall tree width, fixed_width width tree ->
    match finalize_node tree with
    | Some final =>
        canonical width final /\ (forall key, tree_lookup final key = tree_lookup tree key)
    | None => forall key, tree_lookup tree key = None
    end.
Proof.
  intros tree.
  induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width shaped. cbn [fixed_width] in shaped.
  destruct shaped as (prefix_bound & full_leaf & short_absent & sorted & child_shapes).
  rewrite all_children_iff in child_shapes.
  assert (child_spec : forall edge child, In (edge, child) children ->
    match finalize_node child with
    | Some final => canonical (width - S (length prefix)) final /\
        (forall key, tree_lookup final key = tree_lookup child key)
    | None => forall key, tree_lookup child key = None
    end).
  { intros edge child member.
    exact (IH (edge, child) member (width - S (length prefix)) (child_shapes edge child member)). }
  assert (kept_lookup : forall next rest,
    child_lookup (fun child => tree_lookup child rest) next
      (finalize_children finalize_node children) =
    child_lookup (fun child => tree_lookup child rest) next children).
  { intros next rest. apply finalize_children_lookup; [exact sorted|].
    intros edge child member. pose proof (child_spec edge child member) as spec.
    destruct (finalize_node child) as [final|].
    - destruct spec as [_ same]. exact (same rest).
    - exact (spec rest). }
  assert (kept_shapes : forall edge final,
    In (edge, final) (finalize_children finalize_node children) ->
    canonical (width - S (length prefix)) final).
  { intros edge final member.
    destruct (finalize_children_member _ _ _ _ member) as (child & child_member & finalized).
    pose proof (child_spec edge child child_member) as spec. rewrite finalized in spec.
    exact (proj1 spec). }
  pose proof (finalize_children_sorted finalize_node children sorted) as kept_sorted.
  assert (prefix_short : finalize_children finalize_node children <> [] ->
    length prefix < width).
  { intros kept_nonempty.
    destruct (Nat.lt_ge_cases (length prefix) width) as [below|above]; [exact below|].
    assert (no_children : children = []) by (apply full_leaf; lia).
    subst children. cbn [finalize_children] in kept_nonempty.
    exfalso. apply kept_nonempty. reflexivity. }
  cbn [finalize_node].
  destruct endpoint as [claim|].
  - assert (full : length prefix = width).
    { destruct (Nat.lt_ge_cases (length prefix) width) as [below|above];
        [pose proof (short_absent below) as impossible; discriminate impossible|lia]. }
    pose proof (full_leaf full) as no_children. subst children.
    cbn [finalize_children merge_unary].
    split; [cbn [canonical]; exact full|]. intros key. reflexivity.
  - destruct (finalize_children finalize_node children)
      as [|[edge [child_prefix child_endpoint child_children]] [|second rest]] eqn:kept_is.
    + cbn [merge_unary]. intros key. rewrite tree_lookup_node_below.
      destruct (strip_prefix prefix key) as [[|next rest]|]; cbn [node_below];
        [reflexivity| |reflexivity].
      rewrite <- kept_lookup. reflexivity.
    + cbn [merge_unary].
      pose proof (prefix_short ltac:(discriminate)) as short.
      pose proof (kept_shapes edge (RadixTree child_prefix child_endpoint child_children)
        (or_introl eq_refl)) as child_shape.
      split.
      * destruct child_endpoint as [child_claim|].
        -- destruct child_children as [|first others]; cbn [canonical] in child_shape |- *;
             [|destruct child_shape].
           rewrite length_app. cbn [length]. lia.
        -- cbn [canonical] in child_shape |- *.
           destruct child_shape as (child_below & child_wide & child_sorted & grandchildren).
           rewrite length_app. cbn [length].
           split; [lia|]. split; [exact child_wide|]. split; [exact child_sorted|].
           replace (width - S (length prefix + S (length child_prefix)))
             with (width - S (length prefix) - S (length child_prefix)) by lia.
           exact grandchildren.
      * intros key. rewrite tree_lookup_prefix_app.
        rewrite (tree_lookup_node_below prefix None children key).
        destruct (strip_prefix prefix key) as [[|next later]|]; [reflexivity| |reflexivity].
        rewrite tree_lookup_cons_prefix. cbn [node_below].
        rewrite <- kept_lookup. cbn [child_lookup].
        rewrite Nat.eqb_sym. reflexivity.
    + cbn [merge_unary].
      pose proof (prefix_short ltac:(discriminate)) as short.
      split.
      * cbn [canonical]. split; [exact short|]. split; [cbn [length]; lia|].
        split; [exact kept_sorted|]. apply all_children_iff. exact kept_shapes.
      * intros key. rewrite !tree_lookup_node_below.
        destruct (strip_prefix prefix key) as [[|next later]|]; cbn [node_below];
          [reflexivity| |reflexivity].
        apply kept_lookup.
Qed.

Lemma finalize_root_spec :
  forall tree width, fixed_width width tree ->
    canonical_root width (finalize_root tree) /\
    (forall key, tree_lookup (finalize_root tree) key = tree_lookup tree key).
Proof.
  intros tree width shaped. unfold finalize_root.
  pose proof (finalize_node_spec tree width shaped) as spec.
  destruct (finalize_node tree) as [final|].
  - destruct spec as [final_shape same]. split; [right; exact final_shape|exact same].
  - split; [left; reflexivity|]. intros key. rewrite empty_tree_lookup. symmetry.
    exact (spec key).
Qed.

(** Untouched children are [Reference] links in the editor and are emitted
    verbatim; finalizing a canonical subtree returns it unchanged, so the
    model, which finalizes every child, agrees with the editor. *)
Lemma finalize_node_canonical_identity :
  forall tree width, canonical width tree -> finalize_node tree = Some tree.
Proof.
  intros tree.
  induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width shaped.
  destruct endpoint as [claim|].
  - destruct children as [|first rest]; cbn [canonical] in shaped; [|destruct shaped].
    reflexivity.
  - cbn [canonical] in shaped. destruct shaped as (below & wide & sorted & child_shapes).
    rewrite all_children_iff in child_shapes.
    cbn [finalize_node]. rewrite finalize_children_identity.
    + destruct children as [|first [|second rest]]; cbn [length] in wide; [lia|lia|].
      destruct first as [edge [child_prefix child_endpoint child_children]].
      reflexivity.
    + intros edge child member.
      exact (IH (edge, child) member _ (child_shapes edge child member)).
Qed.

Lemma canonical_root_lookup_unique :
  forall width left right,
    canonical_root width left -> canonical_root width right ->
    (forall key, tree_lookup left key = tree_lookup right key) -> left = right.
Proof.
  intros width left right [left_empty|left_shape] [right_empty|right_shape] same.
  - subst. reflexivity.
  - subst left. destruct (canonical_has_entry right width right_shape) as (key & claim & found).
    rewrite <- same, empty_tree_lookup in found. discriminate found.
  - subst right. destruct (canonical_has_entry left width left_shape) as (key & claim & found).
    rewrite same, empty_tree_lookup in found. discriminate found.
  - exact (canonical_lookup_unique left width right left_shape right_shape same).
Qed.

Lemma canonical_root_fixed_width :
  forall width tree, canonical_root width tree -> fixed_width width tree.
Proof.
  intros width tree [empty|shape]; [subst tree; apply empty_tree_fixed_width|].
  exact (canonical_fixed_width tree width shape).
Qed.

(** ** The entry semantics of a CAS batch *)

Definition entries_remove (key : list nat) (entries : list (list nat * nat))
    : list (list nat * nat) :=
  filter (fun entry => negb (bytes_eqb (fst entry) key)) entries.

Fixpoint entries_insert (key : list nat) (claim : nat) (entries : list (list nat * nat))
    : list (list nat * nat) :=
  match entries with
  | [] => [(key, claim)]
  | (candidate, value) :: rest =>
      if ChannelImage.bytes_ltb key candidate then (key, claim) :: (candidate, value) :: rest
      else (candidate, value) :: entries_insert key claim rest
  end.

Definition entries_update (entries : list (list nat * nat)) (key : list nat)
    (after : option nat) : list (list nat * nat) :=
  match after with
  | Some claim => entries_insert key claim (entries_remove key entries)
  | None => entries_remove key entries
  end.

(** [E (+) batch]: each CAS compares the entry's current claim with [before]
    and installs [after]; the first mismatch refuses the whole batch. *)
Definition entries_cas (entries : list (list nat * nat)) (mutation : cas_mutation)
    : option (list (list nat * nat)) :=
  match mutation with
  | (key, before, after) =>
      if endpoint_eqb (entry_lookup entries key) before
      then Some (entries_update entries key after)
      else None
  end.

Fixpoint entries_batch (entries : list (list nat * nat)) (batch : list cas_mutation)
    : option (list (list nat * nat)) :=
  match batch with
  | [] => Some entries
  | mutation :: rest =>
      match entries_cas entries mutation with
      | Some next => entries_batch next rest
      | None => None
      end
  end.

Lemma bytes_eqb_sym : forall left right, bytes_eqb left right = bytes_eqb right left.
Proof.
  intros left right. destruct (bytes_eqb left right) eqn:forward.
  - apply bytes_eqb_true_iff in forward. subst right. rewrite bytes_eqb_refl. reflexivity.
  - symmetry. apply bytes_eqb_false_iff. intro same. subst right.
    rewrite bytes_eqb_refl in forward. discriminate forward.
Qed.

Lemma bytes_ltb_total :
  forall left right, left <> right ->
    ChannelImage.bytes_ltb left right = true \/ ChannelImage.bytes_ltb right left = true.
Proof.
  induction left as [|left_head left_tail IH]; intros [|right_head right_tail] different.
  - exfalso. apply different. reflexivity.
  - left. reflexivity.
  - right. reflexivity.
  - cbn [ChannelImage.bytes_ltb].
    destruct (Nat.ltb_spec left_head right_head) as [below|above]; [left; reflexivity|].
    destruct (Nat.ltb_spec right_head left_head) as [below'|above']; [right; reflexivity|].
    assert (same_head : left_head = right_head) by lia. subst right_head.
    cbv beta iota. apply IH. intro same. apply different. rewrite same. reflexivity.
Qed.

Lemma entry_lookup_remove :
  forall key entries query,
    entry_lookup (entries_remove key entries) query =
      if bytes_eqb query key then None else entry_lookup entries query.
Proof.
  intros key entries query. unfold entries_remove.
  induction entries as [|[candidate claim] rest IH]; cbn [filter entry_lookup fst].
  - destruct (bytes_eqb query key); reflexivity.
  - destruct (bytes_eqb candidate key) eqn:candidate_key; cbn [negb entry_lookup].
    + rewrite IH. apply bytes_eqb_true_iff in candidate_key. subst candidate.
      destruct (bytes_eqb query key) eqn:query_key; [reflexivity|].
      rewrite (proj2 (bytes_eqb_false_iff key query)); [reflexivity|].
      intro same. subst. rewrite bytes_eqb_refl in query_key. discriminate query_key.
    + rewrite IH. destruct (bytes_eqb query key) eqn:query_key; [|reflexivity].
      apply bytes_eqb_true_iff in query_key. subst query. rewrite candidate_key. reflexivity.
Qed.

Lemma entry_lookup_insert :
  forall key claim entries query,
    entry_lookup entries key = None ->
    entry_lookup (entries_insert key claim entries) query =
      if bytes_eqb query key then Some claim else entry_lookup entries query.
Proof.
  intros key claim entries query absent.
  induction entries as [|[candidate value] rest IH]; cbn [entries_insert entry_lookup].
  - rewrite (bytes_eqb_sym key query). reflexivity.
  - cbn [entry_lookup] in absent.
    destruct (bytes_eqb candidate key) eqn:candidate_key; [discriminate absent|].
    destruct (ChannelImage.bytes_ltb key candidate); cbn [entry_lookup].
    + rewrite (bytes_eqb_sym key query). reflexivity.
    + rewrite (IH absent).
      destruct (bytes_eqb query key) eqn:query_key; [|reflexivity].
      apply bytes_eqb_true_iff in query_key. subst query. rewrite candidate_key. reflexivity.
Qed.

Lemma entry_lookup_update :
  forall entries key after query,
    entry_lookup (entries_update entries key after) query =
      if bytes_eqb query key then after else entry_lookup entries query.
Proof.
  intros entries key [claim|] query; unfold entries_update.
  - rewrite entry_lookup_insert.
    + rewrite entry_lookup_remove. destruct (bytes_eqb query key); reflexivity.
    + rewrite entry_lookup_remove, bytes_eqb_refl. reflexivity.
  - apply entry_lookup_remove.
Qed.

Lemma strongly_sorted_filter {A : Type} (R : A -> A -> Prop) (keep : A -> bool) :
  forall values, StronglySorted R values -> StronglySorted R (filter keep values).
Proof.
  induction values as [|value rest IH]; intro sorted; [constructor|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  cbn [filter]. destruct (keep value); [|exact (IH rest_sorted)].
  constructor; [exact (IH rest_sorted)|].
  rewrite Forall_forall in head_below |- *. intros other member.
  apply filter_In in member as [member _]. exact (head_below other member).
Qed.

Lemma entries_insert_member :
  forall key claim entries entry,
    In entry (entries_insert key claim entries) -> entry = (key, claim) \/ In entry entries.
Proof.
  intros key claim entries entry.
  induction entries as [|[candidate value] rest IH]; cbn [entries_insert].
  - intros [here|[]]. left. symmetry. exact here.
  - destruct (ChannelImage.bytes_ltb key candidate); intros [here|there].
    + left. symmetry. exact here.
    + right. exact there.
    + right. left. exact here.
    + destruct (IH there) as [inserted|old]; [left; exact inserted|right; right; exact old].
Qed.

Lemma entries_insert_sorted :
  forall key claim entries,
    StronglySorted key_before entries -> entry_lookup entries key = None ->
    StronglySorted key_before (entries_insert key claim entries).
Proof.
  intros key claim entries sorted absent.
  induction entries as [|[candidate value] rest IH]; cbn [entries_insert].
  - constructor; constructor.
  - apply StronglySorted_inv in sorted as [rest_sorted head_below].
    rewrite Forall_forall in head_below.
    cbn [entry_lookup] in absent.
    destruct (bytes_eqb candidate key) eqn:candidate_key; [discriminate absent|].
    destruct (ChannelImage.bytes_ltb key candidate) eqn:key_below.
    + constructor.
      * constructor; [exact rest_sorted|]. apply Forall_forall. exact head_below.
      * apply Forall_forall. intros [other other_value] [same|member].
        -- injection same as other_is _. subst other. exact key_below.
        -- pose proof (head_below (other, other_value) member) as candidate_below.
           unfold key_before in candidate_below |- *. cbn [fst] in candidate_below |- *.
           exact (bytes_ltb_transitive _ _ _ key_below candidate_below).
    + assert (different : key <> candidate).
      { intro same. subst candidate. rewrite bytes_eqb_refl in candidate_key.
        discriminate candidate_key. }
      assert (candidate_below : ChannelImage.bytes_ltb candidate key = true).
      { destruct (bytes_ltb_total key candidate different) as [forward|backward];
          [rewrite forward in key_below; discriminate key_below|exact backward]. }
      constructor.
      * exact (IH rest_sorted absent).
      * apply Forall_forall. intros [other other_value] member.
        destruct (entries_insert_member _ _ _ _ member) as [inserted|old].
        -- injection inserted as other_is _. subst other. exact candidate_below.
        -- exact (head_below (other, other_value) old).
Qed.

Lemma entries_update_sorted :
  forall width entries key after,
    StronglySorted key_before entries ->
    Forall (fun entry => length (fst entry) = width) entries ->
    length key = width ->
    StronglySorted key_before (entries_update entries key after) /\
    Forall (fun entry => length (fst entry) = width) (entries_update entries key after).
Proof.
  intros width entries key after sorted lengths key_length.
  assert (removed_sorted : StronglySorted key_before (entries_remove key entries))
    by (apply strongly_sorted_filter; exact sorted).
  assert (removed_lengths :
    Forall (fun entry => length (fst entry) = width) (entries_remove key entries)).
  { apply Forall_forall. intros entry member. unfold entries_remove in member.
    apply filter_In in member as [member _]. rewrite Forall_forall in lengths.
    exact (lengths entry member). }
  destruct after as [claim|]; unfold entries_update; [|split; assumption].
  split.
  - apply entries_insert_sorted; [exact removed_sorted|].
    rewrite entry_lookup_remove, bytes_eqb_refl. reflexivity.
  - apply Forall_forall. intros entry member.
    destruct (entries_insert_member _ _ _ _ member) as [inserted|old].
    + subst entry. exact key_length.
    + rewrite Forall_forall in removed_lengths. exact (removed_lengths entry old).
Qed.

Lemma entries_batch_sorted :
  forall width batch entries updated,
    StronglySorted key_before entries ->
    Forall (fun entry => length (fst entry) = width) entries ->
    Forall (fun mutation => length (fst (fst mutation)) = width) batch ->
    entries_batch entries batch = Some updated ->
    StronglySorted key_before updated /\ Forall (fun entry => length (fst entry) = width) updated.
Proof.
  intros width batch.
  induction batch as [|[[key before] after] rest IH];
    intros entries updated sorted lengths keys batched.
  - cbn [entries_batch] in batched. injection batched as updated_is. subst updated.
    split; assumption.
  - cbn [entries_batch entries_cas] in batched.
    destruct (endpoint_eqb (entry_lookup entries key) before); [|discriminate batched].
    pose proof (Forall_inv keys) as key_length. cbn [fst] in key_length.
    destruct (entries_update_sorted width entries key after sorted lengths key_length)
      as [next_sorted next_lengths].
    exact (IH _ _ next_sorted next_lengths (Forall_inv_tail keys) batched).
Qed.

(** ** Batches *)

Lemma edit_batch_spec :
  forall width batch tree entries,
    fixed_width width tree ->
    (forall key, tree_lookup tree key = entry_lookup entries key) ->
    Forall (fun mutation => length (fst (fst mutation)) = width) batch ->
    match edit_batch tree batch, entries_batch entries batch with
    | Some edited, Some updated =>
        fixed_width width edited /\
        (forall key, tree_lookup edited key = entry_lookup updated key)
    | None, None => True
    | _, _ => False
    end.
Proof.
  intros width batch.
  induction batch as [|[[key before] after] rest IH]; intros tree entries shaped same keys.
  - cbn [edit_batch entries_batch]. split; assumption.
  - cbn [edit_batch entries_batch entries_cas].
    pose proof (Forall_inv keys) as key_length. cbn [fst] in key_length.
    pose proof (edit_node_spec tree width key before after shaped key_length) as step.
    destruct (edit_node tree key before after) as [next|] eqn:stepped;
      cbv beta iota in step.
    + destruct step as (current & next_shape & next_lookup).
      rewrite <- same, current, (proj2 (endpoint_eqb_true_iff before before) eq_refl).
      cbv beta iota.
      apply IH; [exact next_shape| |exact (Forall_inv_tail keys)].
      intros query. rewrite next_lookup, entry_lookup_update, same. reflexivity.
    + destruct (endpoint_eqb (entry_lookup entries key) before) eqn:cas; [|exact I].
      apply endpoint_eqb_true_iff in cas. rewrite <- same in cas. contradiction.
Qed.

(** Capstone T2: on a canonical store, a batch of fixed-width CAS mutations
    either is refused, exactly when some CAS does not match the entries it
    meets, or yields the canonical rebuild of [E (+) batch]; the rebuild of no
    entries is the empty root. *)
Theorem radix_edit_batch_is_the_canonical_rebuild :
  forall width tree batch,
    canonical_root width tree ->
    Forall (fun mutation => length (fst (fst mutation)) = width) batch ->
    radix_edit tree batch =
      match entries_batch (tree_entries tree) batch with
      | Some updated => Some (partition_build width updated)
      | None => None
      end /\
    (forall updated,
       entries_batch (tree_entries tree) batch = Some updated ->
       canonical_root width (partition_build width updated) /\
       tree_entries (partition_build width updated) = updated /\
       (updated = [] -> partition_build width updated = empty_tree)).
Proof.
  intros width tree batch rooted keys.
  pose proof (canonical_root_fixed_width width tree rooted) as shaped.
  pose proof (fixed_width_entries tree width shaped) as [entries_sorted entries_lengths].
  assert (rebuilt : forall updated,
    entries_batch (tree_entries tree) batch = Some updated ->
    canonical_root width (partition_build width updated) /\
    tree_entries (partition_build width updated) = updated /\
    (updated = [] -> partition_build width updated = empty_tree)).
  { intros updated batched.
    destruct (entries_batch_sorted width batch (tree_entries tree) updated entries_sorted
                entries_lengths keys batched) as [updated_sorted updated_lengths].
    destruct (canonical_radix_exists_and_is_unique width updated updated_sorted
                updated_lengths) as (built_shape & built_entries & _).
    split; [exact built_shape|]. split; [exact built_entries|].
    intros empty. subst updated. apply partition_build_nil. }
  split; [|exact rebuilt].
  unfold radix_edit.
  destruct batch as [|mutation rest].
  - cbn [entries_batch]. f_equal.
    destruct (canonical_radix_exists_and_is_unique width (tree_entries tree) entries_sorted
                entries_lengths) as (_ & _ & unique).
    exact (unique tree rooted eq_refl).
  - pose proof (edit_batch_spec width (mutation :: rest) tree (tree_entries tree) shaped
      (tree_lookup_entries tree width shaped) keys) as spec.
    destruct (edit_batch tree (mutation :: rest)) as [edited|];
      destruct (entries_batch (tree_entries tree) (mutation :: rest)) as [updated|]
        eqn:batched; cbv beta iota in spec; try contradiction; [|reflexivity].
    destruct spec as [edited_shape edited_lookup].
    destruct (finalize_root_spec edited width edited_shape) as [final_shape final_lookup].
    destruct (rebuilt updated eq_refl) as (built_shape & built_entries & _).
    pose proof (canonical_root_fixed_width width _ built_shape) as built_fixed.
    f_equal. apply (canonical_root_lookup_unique width); [exact final_shape|exact built_shape|].
    intros key. rewrite final_lookup, edited_lookup.
    rewrite (tree_lookup_entries _ width built_fixed), built_entries. reflexivity.
Qed.

(** Negative control: a finalizer that does not prune empty children. *)
Fixpoint map_children (finalize : radix_tree -> radix_tree)
    (children : list (nat * radix_tree)) : list (nat * radix_tree) :=
  match children with
  | [] => []
  | (edge, child) :: rest => (edge, finalize child) :: map_children finalize rest
  end.

Definition unpruned_merge (prefix : list nat) (endpoint : option nat)
    (kept : list (nat * radix_tree)) : radix_tree :=
  match endpoint, kept with
  | None, [(edge, RadixTree child_prefix child_endpoint child_children)] =>
      RadixTree (prefix ++ edge :: child_prefix) child_endpoint child_children
  | _, _ => RadixTree prefix endpoint kept
  end.

Fixpoint unpruned_finalize (tree : radix_tree) : radix_tree :=
  match tree with
  | RadixTree prefix endpoint children =>
      unpruned_merge prefix endpoint (map_children unpruned_finalize children)
  end.

Definition unpruned_radix_edit (tree : radix_tree) (batch : list cas_mutation)
    : option radix_tree :=
  match batch with
  | [] => Some tree
  | _ :: _ =>
      match edit_batch tree batch with
      | Some edited => Some (unpruned_finalize edited)
      | None => None
      end
  end.

Definition two_leaf_left_key : list nat := 0 :: repeat 0 47.
Definition two_leaf_right_key : list nat := 1 :: repeat 0 47.

Definition two_leaf_sample : radix_tree :=
  RadixTree [] None
    [(0, RadixTree (repeat 0 47) (Some 50) []); (1, RadixTree (repeat 0 47) (Some 51) [])].

Definition unpruned_sample_result : radix_tree :=
  RadixTree [] None
    [(0, RadixTree (repeat 0 47) (Some 50) []); (1, RadixTree (repeat 0 47) None [])].

(** Capstone T2m (negative control): evicting the right leaf of the two-leaf
    root.  The editor returns the canonical rebuild of the remaining entry,
    the left leaf; a finalizer that does not prune keeps the emptied right
    leaf beside it, which is neither that rebuild nor a canonical root. *)
Theorem unpruned_empty_child_mutant_is_not_the_rebuild :
  canonical_root retention_key_len two_leaf_sample /\
  radix_edit two_leaf_sample [(two_leaf_right_key, Some 51, None)] =
    Some (partition_build retention_key_len [(two_leaf_left_key, 50)]) /\
  unpruned_radix_edit two_leaf_sample [(two_leaf_right_key, Some 51, None)] =
    Some unpruned_sample_result /\
  unpruned_sample_result <> partition_build retention_key_len [(two_leaf_left_key, 50)] /\
  ~ canonical_root retention_key_len unpruned_sample_result.
Proof.
  split.
  { assert (built : two_leaf_sample =
      partition_build retention_key_len [(two_leaf_left_key, 50); (two_leaf_right_key, 51)])
      by (vm_compute; reflexivity).
    rewrite built. apply canonical_radix_exists_and_is_unique.
    - apply SSorted_cons; [apply SSorted_cons; [apply SSorted_nil|apply Forall_nil]|].
      apply Forall_cons; [vm_compute; reflexivity|apply Forall_nil].
    - apply Forall_cons; [reflexivity|]. apply Forall_cons; [reflexivity|apply Forall_nil]. }
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; discriminate|].
  intros [empty|shape]; [discriminate empty|].
  cbn [canonical unpruned_sample_result] in shape.
  destruct shape as (_ & _ & _ & children_shape).
  cbn [all_children canonical] in children_shape.
  destruct children_shape as (_ & bad & _).
  destruct bad as (_ & wide & _). cbn [length] in wide. lia.
Qed.

(** * Path locality *)

Fixpoint children_subtrees (subtrees_of : radix_tree -> list radix_tree)
    (children : list (nat * radix_tree)) : list radix_tree :=
  match children with
  | [] => []
  | (_, child) :: rest => subtrees_of child ++ children_subtrees subtrees_of rest
  end.

(** [subtrees] lists the node objects of a radix image.  Nodes are
    content-addressed, so equal subtrees are one object and membership in
    this list is membership in the image's object set. *)
Fixpoint subtrees (tree : radix_tree) : list radix_tree :=
  match tree with
  | RadixTree _ _ children => tree :: children_subtrees subtrees children
  end.

Fixpoint path_children (path_of : radix_tree -> list radix_tree) (edge : nat)
    (children : list (nat * radix_tree)) : list radix_tree :=
  match children with
  | [] => []
  | (other, child) :: rest =>
      if Nat.eqb edge other then path_of child else path_children path_of edge rest
  end.

(** [search_path] lists the nodes a lookup of [key] visits: the root, and
    then the child selected by each next byte, while the prefixes match. *)
Fixpoint search_path (tree : radix_tree) (key : list nat) : list radix_tree :=
  match tree with
  | RadixTree prefix _ children =>
      tree ::
        match strip_prefix prefix key with
        | Some (edge :: rest) =>
            path_children (fun child => search_path child rest) edge children
        | _ => []
        end
  end.

Lemma children_subtrees_in :
  forall (subtrees_of : radix_tree -> list radix_tree) children node,
    In node (children_subtrees subtrees_of children) <->
    exists edge child, In (edge, child) children /\ In node (subtrees_of child).
Proof.
  intros subtrees_of children node.
  induction children as [|[edge child] rest IH]; cbn [children_subtrees].
  - split; [intros []|intros (edge & child & [] & _)].
  - rewrite in_app_iff, IH. split.
    + intros [here|(other & target & member & inside)].
      * exists edge, child. split; [left; reflexivity|exact here].
      * exists other, target. split; [right; exact member|exact inside].
    + intros (other & target & [same|member] & inside).
      * injection same as edge_is child_is. subst. left. exact inside.
      * right. exists other, target. split; [exact member|exact inside].
Qed.

Lemma subtrees_in :
  forall prefix endpoint children node,
    In node (subtrees (RadixTree prefix endpoint children)) <->
    node = RadixTree prefix endpoint children \/
    exists edge child, In (edge, child) children /\ In node (subtrees child).
Proof.
  intros prefix endpoint children node. cbn [subtrees].
  pose proof (children_subtrees_in subtrees children node) as members.
  split.
  - intros [here|there]; [left; symmetry; exact here|right; apply members; exact there].
  - intros [here|there]; [left; symmetry; exact here|right; apply members; exact there].
Qed.

Lemma path_children_cases :
  forall (path_of : radix_tree -> list radix_tree) edge children,
    path_children path_of edge children = [] \/
    exists child, In (edge, child) children /\
      path_children path_of edge children = path_of child.
Proof.
  intros path_of edge children.
  induction children as [|[other child] rest IH]; cbn [path_children]; [left; reflexivity|].
  destruct (Nat.eqb_spec edge other) as [same|different].
  - subst other. right. exists child. split; [left; reflexivity|reflexivity].
  - destruct IH as [none|(target & member & found)]; [left; exact none|].
    right. exists target. split; [right; exact member|exact found].
Qed.

Lemma path_children_member :
  forall (path_of : radix_tree -> list radix_tree) children edge child,
    edges_sorted children -> In (edge, child) children ->
    path_children path_of edge children = path_of child.
Proof.
  intros path_of children edge child sorted member.
  induction children as [|[other target] rest IH]; [destruct member|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  rewrite Forall_forall in head_below.
  cbn [path_children].
  destruct member as [same|member].
  - injection same as edge_is child_is. subst. rewrite Nat.eqb_refl. reflexivity.
  - pose proof (head_below (edge, child) member) as above. cbn [fst] in above.
    destruct (Nat.eqb_spec edge other) as [same|_]; [exfalso; lia|].
    exact (IH rest_sorted member).
Qed.

Lemma find_child_member :
  forall children edge child,
    edges_sorted children -> In (edge, child) children -> find_child edge children = Some child.
Proof.
  intros children edge child sorted member.
  induction children as [|[other target] rest IH]; [destruct member|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  rewrite Forall_forall in head_below.
  cbn [find_child].
  destruct member as [same|member].
  - injection same as edge_is child_is. subst. rewrite Nat.eqb_refl. reflexivity.
  - pose proof (head_below (edge, child) member) as above. cbn [fst] in above.
    destruct (Nat.eqb_spec edge other) as [same|_]; [exfalso; lia|].
    exact (IH rest_sorted member).
Qed.

Lemma search_path_extension :
  forall prefix endpoint children edge rest,
    search_path (RadixTree prefix endpoint children) (prefix ++ edge :: rest) =
      RadixTree prefix endpoint children ::
        path_children (fun child => search_path child rest) edge children.
Proof.
  intros prefix endpoint children edge rest. cbn [search_path].
  rewrite strip_prefix_app. reflexivity.
Qed.

Lemma search_path_leaf :
  forall suffix claim,
    search_path (RadixTree suffix (Some claim) []) suffix = [RadixTree suffix (Some claim) []].
Proof.
  intros suffix claim. cbn [search_path]. rewrite strip_prefix_self. reflexivity.
Qed.

(** A lookup visits at most one node per consumed byte plus the root. *)
Lemma search_path_length :
  forall tree width key, fixed_width width tree -> length (search_path tree key) <= S width.
Proof.
  intros tree.
  induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width key shaped. cbn [fixed_width] in shaped.
  destruct shaped as (prefix_bound & full_leaf & _ & _ & child_shapes).
  rewrite all_children_iff in child_shapes.
  cbn [search_path length].
  destruct (strip_prefix prefix key) as [[|edge rest]|]; cbn [length]; [lia| |lia].
  destruct (path_children_cases (fun child => search_path child rest) edge children)
    as [none|(child & member & found)]; [rewrite none; cbn [length]; lia|].
  cbv beta in found. rewrite found.
  assert (short : length prefix < width).
  { destruct (Nat.lt_ge_cases (length prefix) width) as [below|above]; [exact below|].
    assert (no_children : children = []) by (apply full_leaf; lia).
    subst children. destruct member. }
  pose proof (IH (edge, child) member (width - S (length prefix)) rest
    (child_shapes edge child member)) as bound.
  cbn [snd] in bound. lia.
Qed.

Lemma replace_child_length :
  forall edge edited children, length (replace_child edge edited children) = length children.
Proof.
  intros edge edited children.
  induction children as [|[other child] rest IH]; cbn [replace_child]; [reflexivity|].
  destruct (Nat.eqb edge other); cbn [length]; [reflexivity|rewrite IH; reflexivity].
Qed.

Lemma insert_child_length :
  forall edge leaf children, length (insert_child edge leaf children) = S (length children).
Proof.
  intros edge leaf children.
  induction children as [|[other child] rest IH]; cbn [insert_child]; [reflexivity|].
  destruct (edge <? other); cbn [length]; [reflexivity|rewrite IH; reflexivity].
Qed.

Lemma replace_child_keeps :
  forall edge edited children other child,
    In (other, child) children -> other <> edge ->
    In (other, child) (replace_child edge edited children).
Proof.
  intros edge edited children other child member different.
  induction children as [|[next target] rest IH]; [destruct member|].
  cbn [replace_child].
  destruct (Nat.eqb_spec edge next) as [same|_].
  - subst next. destruct member as [here|there].
    + injection here as other_is _. congruence.
    + right. exact there.
  - destruct member as [here|there]; [left; exact here|right; exact (IH there)].
Qed.

Lemma insert_child_keeps :
  forall edge leaf children other child,
    In (other, child) children -> In (other, child) (insert_child edge leaf children).
Proof.
  intros edge leaf children other child member.
  induction children as [|[next target] rest IH]; [destruct member|].
  cbn [insert_child].
  destruct (edge <? next).
  - right. exact member.
  - destruct member as [here|there]; [left; exact here|right; exact (IH there)].
Qed.

(** One insertion into a canonical image: the editor's result is canonical;
    every object of the result is an old object, a node on the key's new
    search path, or the one node split off below the split point; every old
    object survives or lies on the key's old search path. *)
Lemma insert_edit_structure :
  forall small width key claim,
    canonical width small -> length key = width -> tree_lookup small key = None ->
    exists large,
      edit_node small key None (Some claim) = Some large /\
      canonical width large /\
      (exists sibling, length sibling <= 1 /\
         forall node, In node (subtrees large) ->
           In node (subtrees small) \/ In node (search_path large key) \/ In node sibling) /\
      (forall node, In node (subtrees small) ->
         In node (subtrees large) \/ In node (search_path small key)).
Proof.
  intros small.
  induction small as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width key claim shaped key_length absent.
  pose proof (canonical_fixed_width _ _ shaped) as fixed.
  cbn [fixed_width] in fixed. destruct fixed as (prefix_bound & _).
  cbn [edit_node].
  destruct (prefix_split prefix key) as [rest|shared old_edge old_suffix rest] eqn:split_is.
  - pose proof (prefix_split_matched _ _ _ split_is) as key_is. subst key.
    destruct rest as [|edge tail].
    + exfalso. rewrite app_nil_r in key_length, absent.
      destruct endpoint as [held|].
      * destruct children as [|first others]; cbn [canonical] in shaped; [|exact shaped].
        rewrite tree_lookup_node_below, strip_prefix_self in absent. cbn [node_below] in absent.
        discriminate absent.
      * cbn [canonical] in shaped. destruct shaped as (below & _). lia.
    + rewrite length_app in key_length. cbn [length] in key_length.
      assert (tail_length : length tail = width - S (length prefix)) by lia.
      destruct endpoint as [held|].
      { exfalso. destruct children as [|first others]; cbn [canonical] in shaped;
          [lia|exact shaped]. }
      cbn [canonical] in shaped.
      destruct shaped as (short & wide & sorted & child_shapes).
      rewrite all_children_iff in child_shapes.
      destruct (find_child edge children) as [old|] eqn:found.
      * pose proof (find_child_some _ _ _ found) as member.
        rewrite (edit_children_present _ _ _ old _ sorted member). cbv beta.
        rewrite (member_lookup _ _ _ _ _ _ sorted member) in absent.
        pose proof (IH (edge, old) member) as child_induction. cbn [snd] in child_induction.
        destruct (child_induction (width - S (length prefix)) tail claim
                    (child_shapes edge old member) tail_length absent)
          as (edited & edited_is & edited_shape & (sibling & sibling_length & forward)
              & backward).
        rewrite edited_is. cbv beta iota.
        exists (RadixTree prefix None (replace_child edge edited children)).
        split; [reflexivity|].
        split.
        { cbn [canonical]. split; [exact short|].
          split; [rewrite replace_child_length; exact wide|].
          split; [apply replace_child_sorted; exact sorted|].
          apply all_children_iff. intros other child member'.
          destruct (replace_child_member _ _ _ _ _ member')
            as [(other_is & child_is & _)|original].
          - subst child. exact edited_shape.
          - exact (child_shapes other child original). }
        assert (large_path :
          search_path (RadixTree prefix None (replace_child edge edited children))
            (prefix ++ edge :: tail) =
          RadixTree prefix None (replace_child edge edited children) ::
            search_path edited tail).
        { rewrite search_path_extension.
          rewrite (path_children_member _ _ edge edited (replace_child_sorted _ _ _ sorted)
                     (replace_child_in _ _ _ _ member)).
          reflexivity. }
        assert (small_path :
          search_path (RadixTree prefix None children) (prefix ++ edge :: tail) =
          RadixTree prefix None children :: search_path old tail).
        { rewrite search_path_extension, (path_children_member _ _ edge old sorted member).
          reflexivity. }
        split.
        { exists sibling. split; [exact sibling_length|].
          intros node inside.
          apply subtrees_in in inside as [here|(other & child & member' & inside)].
          - right. left. rewrite large_path. left. symmetry. exact here.
          - destruct (replace_child_member _ _ _ _ _ member')
              as [(other_is & child_is & _)|original].
            + subst child. destruct (forward node inside) as [in_old|[on_path|in_sibling]].
              * left. apply subtrees_in. right. exists edge, old.
                split; [exact member|exact in_old].
              * right. left. rewrite large_path. right. exact on_path.
              * right. right. exact in_sibling.
            + left. apply subtrees_in. right. exists other, child.
              split; [exact original|exact inside]. }
        intros node inside.
        apply subtrees_in in inside as [here|(other & child & member' & inside)].
        -- right. rewrite small_path. left. symmetry. exact here.
        -- destruct (Nat.eqb_spec other edge) as [same|different].
           ++ subst other.
              pose proof (find_child_member _ _ _ sorted member') as child_found.
              rewrite found in child_found. injection child_found as child_is. subst child.
              destruct (backward node inside) as [in_new|on_path].
              ** left. apply subtrees_in. right. exists edge, edited.
                 split; [exact (replace_child_in _ _ _ _ member)|exact in_new].
              ** right. rewrite small_path. right. exact on_path.
           ++ left. apply subtrees_in. right. exists other, child.
              split; [apply replace_child_keeps; [exact member'|exact different]|exact inside].
      * pose proof (find_child_none _ _ found) as missing.
        rewrite (edit_children_absent _ _ _ _ sorted missing). cbn [fresh_leaf].
        cbv beta iota.
        exists (RadixTree prefix None
                  (insert_child edge (RadixTree tail (Some claim) []) children)).
        split; [reflexivity|].
        split.
        { cbn [canonical]. split; [exact short|].
          split; [rewrite insert_child_length; lia|].
          split; [apply insert_child_sorted; [exact sorted|exact missing]|].
          apply all_children_iff. intros other child member'.
          destruct (insert_child_member _ _ _ _ _ member') as [(other_is & child_is)|original].
          - subst child. cbn [canonical]. exact tail_length.
          - exact (child_shapes other child original). }
        assert (large_path :
          search_path (RadixTree prefix None
                         (insert_child edge (RadixTree tail (Some claim) []) children))
            (prefix ++ edge :: tail) =
          [RadixTree prefix None (insert_child edge (RadixTree tail (Some claim) []) children);
           RadixTree tail (Some claim) []]).
        { rewrite search_path_extension.
          rewrite (path_children_member _ _ edge (RadixTree tail (Some claim) [])
                     (insert_child_sorted _ _ _ sorted missing) (insert_child_in _ _ _)).
          cbv beta. rewrite search_path_leaf. reflexivity. }
        split.
        { exists []. split; [cbn [length]; lia|].
          intros node inside.
          apply subtrees_in in inside as [here|(other & child & member' & inside)].
          - right. left. rewrite large_path. left. symmetry. exact here.
          - destruct (insert_child_member _ _ _ _ _ member')
              as [(other_is & child_is)|original].
            + subst child. cbn [subtrees children_subtrees] in inside.
              destruct inside as [here|[]].
              right. left. rewrite large_path. right. left. exact here.
            + left. apply subtrees_in. right. exists other, child.
              split; [exact original|exact inside]. }
        intros node inside.
        apply subtrees_in in inside as [here|(other & child & member' & inside)].
        -- right. rewrite search_path_extension. left. symmetry. exact here.
        -- left. apply subtrees_in. right. exists other, child.
           split; [apply insert_child_keeps; exact member'|exact inside].
  - destruct (prefix_split_diverged _ _ _ _ _ _ split_is) as (prefix_is & key_is & differs).
    subst prefix key.
    rewrite length_app in prefix_bound, key_length. cbn [length] in prefix_bound.
    destruct rest as [|next tail]; [exfalso; cbn [length] in key_length; lia|].
    cbn [length] in key_length.
    pose proof (differs next tail eq_refl) as next_differs.
    assert (absent_single : forall old,
      ~ In (next, old) [(old_edge, RadixTree old_suffix endpoint children)]).
    { intros old [same|[]]. injection same as same_edge _. congruence. }
    assert (singleton_sorted :
      edges_sorted [(old_edge, RadixTree old_suffix endpoint children)])
      by (constructor; constructor).
    unfold split_edit.
    rewrite (edit_children_absent _ _ _ _ singleton_sorted absent_single).
    cbn [fresh_leaf]. cbv beta iota.
    exists (RadixTree shared None
              (insert_child next (RadixTree tail (Some claim) [])
                 [(old_edge, RadixTree old_suffix endpoint children)])).
    split; [reflexivity|].
    assert (sibling_shape :
      canonical (width - S (length shared)) (RadixTree old_suffix endpoint children)).
    { destruct endpoint as [held|].
      - destruct children as [|first others]; cbn [canonical] in shaped |- *;
          [|exact shaped].
        rewrite length_app in shaped. cbn [length] in shaped. lia.
      - cbn [canonical] in shaped |- *.
        destruct shaped as (short & wide & sorted & child_shapes).
        rewrite length_app in short. cbn [length] in short.
        split; [lia|]. split; [exact wide|]. split; [exact sorted|].
        replace (width - S (length shared) - S (length old_suffix))
          with (width - S (length (shared ++ old_edge :: old_suffix)))
          by (rewrite length_app; cbn [length]; lia).
        exact child_shapes. }
    split.
    { cbn [canonical]. split; [lia|].
      split; [rewrite insert_child_length; cbn [length]; lia|].
      split; [apply insert_child_sorted; [exact singleton_sorted|exact absent_single]|].
      apply all_children_iff. intros other child member'.
      destruct (insert_child_member _ _ _ _ _ member') as [(other_is & child_is)|[original|[]]].
      - subst child. cbn [canonical]. lia.
      - injection original as other_is child_is. subst child. exact sibling_shape. }
    assert (large_path :
      search_path (RadixTree shared None
                     (insert_child next (RadixTree tail (Some claim) [])
                        [(old_edge, RadixTree old_suffix endpoint children)]))
        (shared ++ next :: tail) =
      [RadixTree shared None
         (insert_child next (RadixTree tail (Some claim) [])
            [(old_edge, RadixTree old_suffix endpoint children)]);
       RadixTree tail (Some claim) []]).
    { rewrite search_path_extension.
      rewrite (path_children_member _ _ next (RadixTree tail (Some claim) [])
                 (insert_child_sorted _ _ _ singleton_sorted absent_single)
                 (insert_child_in _ _ _)).
      cbv beta. rewrite search_path_leaf. reflexivity. }
    assert (small_path :
      search_path (RadixTree (shared ++ old_edge :: old_suffix) endpoint children)
        (shared ++ next :: tail) =
      [RadixTree (shared ++ old_edge :: old_suffix) endpoint children]).
    { cbn [search_path]. rewrite strip_prefix_app_same. cbn [strip_prefix].
      destruct (Nat.eqb_spec old_edge next) as [same|_]; [congruence|reflexivity]. }
    split.
    { exists [RadixTree old_suffix endpoint children]. split; [cbn [length]; lia|].
      intros node inside.
      apply subtrees_in in inside as [here|(other & child & member' & inside)].
      - right. left. rewrite large_path. left. symmetry. exact here.
      - destruct (insert_child_member _ _ _ _ _ member')
          as [(other_is & child_is)|[original|[]]].
        + subst child. cbn [subtrees children_subtrees] in inside.
          destruct inside as [here|[]].
          right. left. rewrite large_path. right. left. exact here.
        + injection original as other_is child_is. subst child.
          apply subtrees_in in inside as [here|(edge & child & member & inside)].
          * right. right. left. symmetry. exact here.
          * left. apply subtrees_in. right. exists edge, child.
            split; [exact member|exact inside]. }
    intros node inside.
    apply subtrees_in in inside as [here|(edge & child & member & inside)].
    + right. rewrite small_path. left. symmetry. exact here.
    + left. apply subtrees_in. right.
      exists old_edge, (RadixTree old_suffix endpoint children).
      split; [apply insert_child_keeps; left; reflexivity|].
      apply subtrees_in. right. exists edge, child. split; [exact member|exact inside].
Qed.

Lemma partition_build_single :
  forall fuel key claim, partition_build fuel [(key, claim)] = RadixTree key (Some claim) [].
Proof. intros [|fuel] key claim; reflexivity. Qed.

Lemma entries_remove_absent :
  forall key entries, entry_lookup entries key = None -> entries_remove key entries = entries.
Proof.
  intros key entries absent. unfold entries_remove.
  induction entries as [|[candidate claim] rest IH]; [reflexivity|].
  cbn [entry_lookup] in absent. cbn [filter fst].
  destruct (bytes_eqb candidate key) eqn:same; [discriminate absent|].
  cbn [negb]. f_equal. exact (IH absent).
Qed.

Lemma entries_remove_insert :
  forall key claim entries,
    entry_lookup entries key = None ->
    entries_remove key (entries_insert key claim entries) = entries.
Proof.
  intros key claim entries absent. unfold entries_remove.
  induction entries as [|[candidate value] rest IH]; cbn [entries_insert].
  - cbn [filter fst]. rewrite bytes_eqb_refl. reflexivity.
  - cbn [entry_lookup] in absent.
    destruct (bytes_eqb candidate key) eqn:same; [discriminate absent|].
    destruct (ChannelImage.bytes_ltb key candidate); cbn [filter fst].
    + rewrite bytes_eqb_refl, same. cbn [negb]. f_equal.
      fold (entries_remove key rest). apply entries_remove_absent. exact absent.
    + rewrite same. cbn [negb]. f_equal. exact (IH absent).
Qed.

Lemma entry_lookup_insert_self :
  forall key claim entries,
    entry_lookup entries key = None -> entry_lookup (entries_insert key claim entries) key = Some claim.
Proof.
  intros key claim entries absent. rewrite (entry_lookup_insert key claim entries key absent).
  rewrite bytes_eqb_refl. reflexivity.
Qed.

Lemma entries_insert_sorted_lengths :
  forall width key claim entries,
    StronglySorted key_before entries ->
    Forall (fun entry => length (fst entry) = width) entries ->
    length key = width -> entry_lookup entries key = None ->
    StronglySorted key_before (entries_insert key claim entries) /\
    Forall (fun entry => length (fst entry) = width) (entries_insert key claim entries).
Proof.
  intros width key claim entries sorted lengths key_length absent.
  split; [exact (entries_insert_sorted key claim entries sorted absent)|].
  apply Forall_forall. intros entry member.
  destruct (entries_insert_member _ _ _ _ member) as [inserted|old].
  - subst entry. exact key_length.
  - rewrite Forall_forall in lengths. exact (lengths entry old).
Qed.

(** ** Node bytes *)

(** The canonical length of a node object ([canonical_len_for_shape]): the
    32-byte envelope header, an 8-byte prefix length, the prefix, the endpoint
    tag ([Absent]: 1 byte; [Multiplicity]: 1 + 8 bytes), the branch tag and
    child count (1 + 8 bytes), and the child table of the branch class
    (Node4/Node16: 41 bytes per child; Node48: a 256-byte slot table and 40
    bytes per child; Node256: a 32-byte bitmap and 40 bytes per child). *)
Definition endpoint_bytes (endpoint : option nat) : nat :=
  match endpoint with
  | Some _ => 9
  | None => 1
  end.

Definition child_table_bytes (count : nat) : nat :=
  if Nat.eqb count 0 then 0
  else if count <=? 16 then 41 * count
  else if count <=? 48 then 256 + 40 * count
  else 32 + 40 * count.

Definition node_bytes (tree : radix_tree) : nat :=
  match tree with
  | RadixTree prefix endpoint children =>
      object_envelope_header_len + 8 + length prefix + endpoint_bytes endpoint + 9 +
        child_table_bytes (length children)
  end.

(** The largest node of a width-[width] image: an inner node with 256
    children (a Node256 table) and a prefix shorter than the width, that is
    [10322 + width] bytes; a leaf is [58 + width] bytes. *)
Definition radix_node_byte_bound (width : nat) : nat :=
  object_envelope_header_len + 8 + width + 1 + 9 + (32 + 40 * 256).

Definition byte_keys (entries : list (list nat * nat)) : Prop :=
  Forall (fun entry => Forall (fun byte => byte < 256) (fst entry)) entries.

Lemma child_table_bytes_bound :
  forall count, count <= 256 -> child_table_bytes count <= 32 + 40 * 256.
Proof.
  intros count bound. unfold child_table_bytes.
  destruct (Nat.eqb_spec count 0); [lia|].
  destruct (Nat.leb_spec count 16); [lia|].
  destruct (Nat.leb_spec count 48); lia.
Qed.

Lemma sorted_edges_nodup :
  forall children, edges_sorted children -> NoDup (map fst children).
Proof.
  intros children sorted.
  induction children as [|[edge child] rest IH]; cbn [map]; [constructor|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  rewrite Forall_forall in head_below.
  constructor; [|exact (IH rest_sorted)].
  intros member. apply in_map_iff in member as ([other target] & edge_is & member).
  cbn [fst] in edge_is. subst other.
  pose proof (head_below (edge, target) member) as below. cbn [fst] in below. lia.
Qed.

Lemma byte_edges_fan_out :
  forall children,
    edges_sorted children -> (forall edge child, In (edge, child) children -> edge < 256) ->
    length children <= 256.
Proof.
  intros children sorted bytes.
  rewrite <- (length_map fst children).
  replace 256 with (length (seq 0 256)) by apply length_seq.
  apply NoDup_incl_length; [exact (sorted_edges_nodup children sorted)|].
  intros edge member. apply in_map_iff in member as ([other child] & edge_is & member).
  cbn [fst] in edge_is. subst other. apply in_seq.
  pose proof (bytes edge child member). lia.
Qed.

Lemma children_entries_member :
  forall entries_of children edge child entry,
    In (edge, child) children -> In entry (entries_of child) ->
    In (prepend_entry [edge] entry) (children_entries entries_of children).
Proof.
  intros entries_of children edge child entry member inside.
  induction children as [|[other target] rest IH]; [destruct member|].
  cbn [children_entries]. apply in_or_app.
  destruct member as [same|member].
  - injection same as edge_is child_is. subst. left. apply in_map. exact inside.
  - right. exact (IH member).
Qed.

Lemma child_entry_in_parent :
  forall prefix endpoint children edge child key claim,
    In (edge, child) children -> In (key, claim) (tree_entries child) ->
    In (prefix ++ edge :: key, claim) (tree_entries (RadixTree prefix endpoint children)).
Proof.
  intros prefix endpoint children edge child key claim member inside.
  cbn [tree_entries]. apply in_or_app. right.
  pose proof (children_entries_member tree_entries children edge child (key, claim)
    member inside) as nested.
  apply (in_map (prepend_entry prefix)) in nested. exact nested.
Qed.

Lemma canonical_entry_exists :
  forall tree width, canonical width tree -> exists entry, In entry (tree_entries tree).
Proof.
  intros tree width shaped.
  destruct (tree_entries tree) as [|entry rest] eqn:entries.
  - exfalso. exact (canonical_entries_nonempty tree width shaped entries).
  - exists entry. left. reflexivity.
Qed.

(** Every node of a canonical image over byte keys fits the node bound. *)
Lemma canonical_node_bytes :
  forall tree width,
    canonical width tree -> byte_keys (tree_entries tree) ->
    forall node, In node (subtrees tree) -> node_bytes node <= radix_node_byte_bound width.
Proof.
  intros tree.
  induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros width shaped bytes node inside.
  unfold byte_keys in bytes. rewrite Forall_forall in bytes.
  assert (child_bytes : forall edge child,
    In (edge, child) children -> byte_keys (tree_entries child)).
  { intros edge child member. unfold byte_keys. apply Forall_forall.
    intros [key claim] entry_member.
    pose proof (bytes _ (child_entry_in_parent prefix endpoint children edge child key claim
      member entry_member)) as parent_bytes.
    cbn [fst] in parent_bytes |- *. apply Forall_app in parent_bytes as [_ parent_bytes].
    exact (Forall_inv_tail parent_bytes). }
  apply subtrees_in in inside as [here|(edge & child & member & inside)].
  - subst node. unfold radix_node_byte_bound, node_bytes.
    destruct endpoint as [held|].
    + destruct children as [|first others]; cbn [canonical] in shaped; [|destruct shaped].
      cbn [endpoint_bytes length child_table_bytes Nat.eqb]. lia.
    + cbn [canonical] in shaped. destruct shaped as (short & wide & sorted & child_shapes).
      rewrite all_children_iff in child_shapes.
      assert (fan_out : length children <= 256).
      { apply byte_edges_fan_out; [exact sorted|].
        intros edge child member.
        destruct (canonical_entry_exists child _ (child_shapes edge child member))
          as ([key claim] & entry_member).
        pose proof (bytes _ (child_entry_in_parent prefix None children edge child key claim
          member entry_member)) as parent_bytes.
        cbn [fst] in parent_bytes. apply Forall_app in parent_bytes as [_ parent_bytes].
        exact (Forall_inv parent_bytes). }
      pose proof (child_table_bytes_bound (length children) fan_out) as table.
      cbn [endpoint_bytes]. lia.
  - assert (child_shape : canonical (width - S (length prefix)) child).
    { destruct endpoint as [held|].
      - destruct children as [|first others]; cbn [canonical] in shaped;
          [destruct member|destruct shaped].
      - cbn [canonical] in shaped. destruct shaped as (_ & _ & _ & child_shapes).
        rewrite all_children_iff in child_shapes. exact (child_shapes edge child member). }
    pose proof (IH (edge, child) member (width - S (length prefix)) child_shape
      (child_bytes edge child member) node inside) as bound.
    cbn [snd] in bound. unfold radix_node_byte_bound in bound |- *. lia.
Qed.

Lemma canonical_root_node_bytes :
  forall tree width,
    canonical_root width tree -> byte_keys (tree_entries tree) ->
    forall node, In node (subtrees tree) -> node_bytes node <= radix_node_byte_bound width.
Proof.
  intros tree width [empty|shape] bytes node inside.
  - subst tree. cbn [subtrees children_subtrees empty_tree] in inside.
    destruct inside as [here|[]]. subst node. unfold radix_node_byte_bound, node_bytes.
    cbn [length endpoint_bytes child_table_bytes Nat.eqb]. lia.
  - exact (canonical_node_bytes tree width shape bytes node inside).
Qed.

Lemma byte_keys_insert :
  forall key claim entries,
    byte_keys entries -> Forall (fun byte => byte < 256) key ->
    byte_keys (entries_insert key claim entries).
Proof.
  intros key claim entries bytes key_bytes. unfold byte_keys in *.
  rewrite Forall_forall in bytes |- *. intros entry member.
  destruct (entries_insert_member _ _ _ _ member) as [inserted|old].
  - subst entry. exact key_bytes.
  - exact (bytes entry old).
Qed.

(** Capstone T3: an insertion or an eviction of one fixed-width key changes
    only the objects on the key's search paths and the one node split off
    (insert) or merged away (evict) beside them: at most [width + 2] new and
    [width + 1] removed node objects for an insertion, and the reverse for an
    eviction.  Each node is at most [radix_node_byte_bound width] bytes, so the
    written bytes are bounded by the width alone, whatever the history size. *)
Theorem radix_edit_changes_only_the_edited_paths :
  forall width entries key claim,
    StronglySorted key_before entries ->
    Forall (fun entry => length (fst entry) = width) entries ->
    length key = width -> entry_lookup entries key = None ->
    radix_edit (partition_build width entries) [(key, None, Some claim)] =
      Some (partition_build width (entries_insert key claim entries)) /\
    radix_edit (partition_build width (entries_insert key claim entries))
      [(key, Some claim, None)] = Some (partition_build width entries) /\
    length (search_path (partition_build width (entries_insert key claim entries)) key)
      <= S width /\
    length (search_path (partition_build width entries) key) <= S width /\
    (exists sibling, length sibling <= 1 /\
       forall node,
         In node (subtrees (partition_build width (entries_insert key claim entries))) ->
         ~ In node (subtrees (partition_build width entries)) ->
         In node (search_path (partition_build width (entries_insert key claim entries)) key) \/
         In node sibling) /\
    (forall node,
       In node (subtrees (partition_build width entries)) ->
       ~ In node (subtrees (partition_build width (entries_insert key claim entries))) ->
       In node (search_path (partition_build width entries) key)) /\
    (byte_keys entries -> Forall (fun byte => byte < 256) key ->
     forall node,
       In node (subtrees (partition_build width entries)) \/
       In node (subtrees (partition_build width (entries_insert key claim entries))) ->
       node_bytes node <= radix_node_byte_bound width).
Proof.
  intros width entries key claim sorted lengths key_length absent.
  destruct (entries_insert_sorted_lengths width key claim entries sorted lengths key_length
              absent) as [large_sorted large_lengths].
  destruct (canonical_radix_exists_and_is_unique width entries sorted lengths)
    as (small_shape & small_entries & _).
  destruct (canonical_radix_exists_and_is_unique width (entries_insert key claim entries)
              large_sorted large_lengths) as (large_shape & large_entries & _).
  pose proof (canonical_root_fixed_width _ _ small_shape) as small_fixed.
  pose proof (canonical_root_fixed_width _ _ large_shape) as large_fixed.
  assert (insert_keys :
    Forall (fun mutation => length (fst (fst mutation)) = width)
      [(key, @None nat, Some claim)])
    by (constructor; [exact key_length|constructor]).
  assert (evict_keys :
    Forall (fun mutation => length (fst (fst mutation)) = width)
      [(key, Some claim, @None nat)])
    by (constructor; [exact key_length|constructor]).
  assert (inserted :
    radix_edit (partition_build width entries) [(key, None, Some claim)] =
      Some (partition_build width (entries_insert key claim entries))).
  { destruct (radix_edit_batch_is_the_canonical_rebuild width _ _ small_shape insert_keys)
      as [edited _].
    rewrite edited, small_entries. cbn [entries_batch entries_cas].
    rewrite absent. cbn [endpoint_eqb]. cbv beta iota. unfold entries_update.
    rewrite (entries_remove_absent key entries absent). reflexivity. }
  assert (evicted :
    radix_edit (partition_build width (entries_insert key claim entries))
      [(key, Some claim, None)] = Some (partition_build width entries)).
  { destruct (radix_edit_batch_is_the_canonical_rebuild width _ _ large_shape evict_keys)
      as [edited _].
    rewrite edited, large_entries. cbn [entries_batch entries_cas].
    rewrite (entry_lookup_insert_self key claim entries absent). cbn [endpoint_eqb].
    rewrite Nat.eqb_refl. cbv beta iota. unfold entries_update.
    rewrite (entries_remove_insert key claim entries absent). reflexivity. }
  split; [exact inserted|]. split; [exact evicted|].
  split; [exact (search_path_length _ width key large_fixed)|].
  split; [exact (search_path_length _ width key small_fixed)|].
  assert (bytes_part :
    byte_keys entries -> Forall (fun byte => byte < 256) key ->
    forall node,
      In node (subtrees (partition_build width entries)) \/
      In node (subtrees (partition_build width (entries_insert key claim entries))) ->
      node_bytes node <= radix_node_byte_bound width).
  { intros bytes key_bytes node [in_small|in_large].
    - apply (canonical_root_node_bytes _ width small_shape); [|exact in_small].
      rewrite small_entries. exact bytes.
    - apply (canonical_root_node_bytes _ width large_shape); [|exact in_large].
      rewrite large_entries. exact (byte_keys_insert key claim entries bytes key_bytes). }
  destruct entries as [|first rest].
  - split; [|split; [|exact bytes_part]].
    + rewrite partition_build_nil. cbn [entries_insert]. rewrite partition_build_single.
      exists []. split; [cbn [length]; lia|].
      intros node inside _. cbn [subtrees children_subtrees] in inside.
      destruct inside as [here|[]]. left. rewrite search_path_leaf. left. exact here.
    + rewrite partition_build_nil. cbn [entries_insert]. rewrite partition_build_single.
      intros node inside _. cbn [subtrees children_subtrees empty_tree] in inside.
      destruct inside as [here|[]]. cbn [search_path empty_tree]. left. exact here.
  - assert (small_canonical : canonical width (partition_build width (first :: rest))).
    { destruct small_shape as [empty|shape]; [|exact shape].
      exfalso. rewrite empty in small_entries. cbn in small_entries.
      discriminate small_entries. }
    assert (small_absent : tree_lookup (partition_build width (first :: rest)) key = None).
    { rewrite (tree_lookup_entries _ width small_fixed), small_entries. exact absent. }
    destruct (insert_edit_structure _ width key claim small_canonical key_length small_absent)
      as (large & edited & large_canonical & (sibling & sibling_length & forward) & backward).
    assert (large_is :
      large = partition_build width (entries_insert key claim (first :: rest))).
    { assert (via_editor :
        radix_edit (partition_build width (first :: rest)) [(key, None, Some claim)] =
          Some large).
      { unfold radix_edit. cbn [edit_batch]. rewrite edited. cbv beta iota.
        unfold finalize_root.
        rewrite (finalize_node_canonical_identity large width large_canonical). reflexivity. }
      rewrite inserted in via_editor. injection via_editor as same. symmetry. exact same. }
    subst large.
    split.
    { exists sibling. split; [exact sibling_length|].
      intros node inside outside.
      destruct (forward node inside) as [in_small|[on_path|in_sibling]];
        [contradiction|left; exact on_path|right; exact in_sibling]. }
    split; [|exact bytes_part].
    intros node inside outside.
    destruct (backward node inside) as [in_large|on_path]; [contradiction|exact on_path].
Qed.

(** Negative control: the V1 writer re-encodes the whole flat manifest on every
    commit, [96 + 56 n] bytes for [n] historical entries
    ([generation-checkpoint-invariant-catalog-2026-08-28.md]). *)
Definition flat_manifest_bytes (historical : nat) : nat := 96 + 56 * historical.

Definition path_local_byte_bound (width : nat) : nat :=
  (width + 2) * radix_node_byte_bound width.

(** Capstone T3m (negative control): the flat manifest grows by 56 bytes per
    historical entry without bound, so once the history has as many entries
    as the path-local bound of capstone T3 has bytes, rewriting the manifest
    writes more than any path-local edit. *)
Theorem flat_manifest_mutant_rewrites_bytes_proportional_to_history :
  (forall historical,
     flat_manifest_bytes (S historical) = flat_manifest_bytes historical + 56) /\
  (forall bound, exists historical, bound < flat_manifest_bytes historical) /\
  (forall width historical, path_local_byte_bound width <= historical ->
     path_local_byte_bound width < flat_manifest_bytes historical).
Proof.
  split; [intros historical; unfold flat_manifest_bytes; lia|].
  split; [intros bound; exists bound; unfold flat_manifest_bytes; lia|].
  intros width historical large. unfold flat_manifest_bytes. lia.
Qed.

(** * Cold-reader admission of the canonical image *)

(** A node object is the [radix_node] of [RetentionV2Format] whose child
    references are the names of the child subtrees.  Names are content
    addresses: under an injective naming, equal subtrees are one object. *)
Definition encode_node (name : radix_tree -> nat) (tree : radix_tree) : radix_node :=
  match tree with
  | RadixTree prefix endpoint children =>
      RadixNode prefix endpoint (map (fun child => (fst child, name (snd child))) children)
  end.

Definition injective_name (name : radix_tree -> nat) : Prop :=
  forall left right, name left = name right -> left = right.

(** [store_holds]: the store returns the encoded node for the name of every
    node of the image; it may hold any other object as well. *)
Definition store_holds (name : radix_tree -> nat) (store : list (nat * radix_node))
    (tree : radix_tree) : Prop :=
  forall node, In node (subtrees tree) ->
    lookup_radix_node store (name node) = Some (encode_node name node).

(** The image store: one (name, node) object per node of the image. *)
Definition image_store (name : radix_tree -> nat) (tree : radix_tree)
    : list (nat * radix_node) :=
  map (fun node => (name node, encode_node name node)) (subtrees tree).

Definition leaf_entry (leaf : radix_leaf) : list nat * nat := (leaf_key leaf, leaf_claim leaf).

Lemma lookup_image_member :
  forall name nodes node,
    injective_name name -> In node nodes ->
    lookup_radix_node (map (fun node => (name node, encode_node name node)) nodes)
      (name node) = Some (encode_node name node).
Proof.
  intros name nodes node injective member.
  induction nodes as [|head rest IH]; [destruct member|].
  cbn [map lookup_radix_node].
  destruct (Nat.eqb (name head) (name node)) eqn:same.
  - apply Nat.eqb_eq in same. apply injective in same. subst head. reflexivity.
  - destruct member as [here|later].
    + subst head. rewrite Nat.eqb_refl in same. discriminate same.
    + exact (IH later).
Qed.

Lemma image_store_holds :
  forall name tree, injective_name name -> store_holds name (image_store name tree) tree.
Proof.
  intros name tree injective node member. unfold image_store.
  exact (lookup_image_member name (subtrees tree) node injective member).
Qed.

Lemma store_holds_root :
  forall name store tree, store_holds name store tree ->
    lookup_radix_node store (name tree) = Some (encode_node name tree).
Proof.
  intros name store [prefix endpoint children] holds. apply holds.
  cbn [subtrees]. left. reflexivity.
Qed.

Lemma store_holds_child :
  forall name store prefix endpoint children edge child,
    store_holds name store (RadixTree prefix endpoint children) ->
    In (edge, child) children -> store_holds name store child.
Proof.
  intros name store prefix endpoint children edge child holds member node inside.
  apply holds. apply subtrees_in. right. exists edge, child. split; assumption.
Qed.

Lemma edges_above_of_sorted :
  forall (name : radix_tree -> nat) children edge,
    edges_sorted children ->
    Forall (fun child => edge < fst child) children ->
    edges_above edge (map (fun child => (fst child, name (snd child))) children) = true.
Proof.
  intros name children.
  induction children as [|[next child] rest IH]; intros edge sorted above.
  - reflexivity.
  - cbn [map edges_above fst snd].
    apply StronglySorted_inv in sorted as [rest_sorted rest_above].
    pose proof (Forall_inv above) as edge_below. cbn [fst] in edge_below.
    apply andb_true_intro. split; [apply Nat.ltb_lt; exact edge_below|].
    apply IH; [exact rest_sorted|].
    eapply Forall_impl; [|exact rest_above]. intros other below. exact below.
Qed.

Lemma edges_strictly_increasing_of_sorted :
  forall (name : radix_tree -> nat) children,
    edges_sorted children ->
    edges_strictly_increasing (map (fun child => (fst child, name (snd child))) children)
      = true.
Proof.
  intros name [|[edge child] rest] sorted; [reflexivity|].
  cbn [map edges_strictly_increasing fst snd].
  apply StronglySorted_inv in sorted as [rest_sorted rest_above].
  apply edges_above_of_sorted; [exact rest_sorted|].
  eapply Forall_impl; [|exact rest_above]. intros other below. exact below.
Qed.

Lemma leaf_entries_prepend :
  forall prefix leaves,
    map leaf_entry (map (prepend_leaf prefix) leaves) =
      map (prepend_entry prefix) (map leaf_entry leaves).
Proof.
  intros prefix leaves. rewrite !map_map. apply map_ext. intros leaf. reflexivity.
Qed.

Lemma unfold_children_image :
  forall name store remaining fuel children,
    (forall edge child, In (edge, child) children ->
       exists leaves, radix_unfold fuel store remaining (name child) = Some leaves /\
         map leaf_entry leaves = tree_entries child) ->
    exists blocks,
      unfold_children_with (radix_unfold fuel store remaining)
        (map (fun child => (fst child, name (snd child))) children) = Some blocks /\
      map leaf_entry blocks = children_entries tree_entries children.
Proof.
  intros name store remaining fuel children.
  induction children as [|[edge child] rest IH]; intros every.
  - exists []. split; reflexivity.
  - destruct (every edge child (in_eq _ _)) as (leaves & unfolded & same).
    destruct IH as (later & later_unfolded & later_same).
    { intros other target member. exact (every other target (in_cons _ _ _ member)). }
    exists (map (prepend_leaf [edge]) leaves ++ later).
    cbn [map unfold_children_with fst snd].
    rewrite unfolded, later_unfolded. split; [reflexivity|].
    cbn [children_entries]. rewrite map_app, leaf_entries_prepend, same, later_same.
    reflexivity.
Qed.

(** Every node of a canonical image unfolds to the image's entries: the
    reader's shape checks (endpoint depth, absent inner endpoint, at least
    two strictly increasing edges) are exactly the canonical shape rules. *)
Lemma canonical_image_unfolds :
  forall tree name store width fuel,
    canonical width tree -> store_holds name store tree -> S width <= fuel ->
    exists leaves,
      radix_unfold fuel store width (name tree) = Some leaves /\
      map leaf_entry leaves = tree_entries tree.
Proof.
  intros tree.
  induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros name store width fuel shaped holds enough.
  destruct fuel as [|smaller]; [lia|].
  cbn [radix_unfold].
  rewrite (store_holds_root name store (RadixTree prefix endpoint children) holds).
  cbn [encode_node radix_prefix radix_claim radix_children].
  destruct endpoint as [claim|].
  - destruct children as [|first rest]; cbn [canonical] in shaped; [|destruct shaped].
    cbn [map].
    destruct (Nat.compare_spec (length prefix) width) as [_|below|above]; [|lia|lia].
    cbv beta iota.
    exists [RadixLeaf prefix claim (name (RadixTree prefix (Some claim) []))].
    split; reflexivity.
  - cbn [canonical] in shaped. destruct shaped as (short & wide & sorted & child_shapes).
    rewrite all_children_iff in child_shapes.
    destruct (Nat.compare_spec (length prefix) width) as [same|_|above]; [lia| |lia].
    cbv beta iota.
    rewrite (edges_strictly_increasing_of_sorted name children sorted), length_map,
      andb_true_l.
    destruct (Nat.leb_spec 2 (length children)) as [_|narrow]; [|lia].
    cbv beta iota.
    destruct (unfold_children_image name store (width - S (length prefix)) smaller children)
      as (blocks & blocks_unfolded & blocks_same).
    { intros edge child member.
      apply (IH (edge, child) member name store (width - S (length prefix)) smaller).
      - exact (child_shapes edge child member).
      - exact (store_holds_child name store prefix None children edge child holds member).
      - lia. }
    rewrite blocks_unfolded. cbv beta iota.
    exists (map (prepend_leaf prefix) blocks). split; [reflexivity|].
    rewrite leaf_entries_prepend, blocks_same. reflexivity.
Qed.

(** Capstone T5: the cold reader admits every canonical image.  The empty
    image is named exactly by the empty radix reference, which is the anchor
    rule (count 0 if and only if the root is the empty radix node); any other
    canonical image counts (by the plain and by the memoized count of the
    reader) exactly its entries and unfolds to them in key order.  With
    capstone T2 this covers every result of the editor. *)
Theorem canonical_radix_store_is_admitted_by_the_cold_reader :
  forall (name : radix_tree -> nat) width tree store fuel,
    injective_name name ->
    canonical_root width tree ->
    store_holds name store tree ->
    S width <= fuel ->
    (name tree = name empty_tree <-> tree_entries tree = []) /\
    (tree_entries tree = [] ->
       lookup_radix_node store (name tree) = Some (RadixNode [] None [])) /\
    (tree_entries tree <> [] ->
       radix_count fuel store width (name tree) = Some (length (tree_entries tree)) /\
       (exists memo,
          memoized_radix_count fuel store [] width (name tree) =
            Some (length (tree_entries tree), memo) /\ memo_sound store memo) /\
       (exists leaves,
          radix_unfold fuel store width (name tree) = Some leaves /\
          map leaf_entry leaves = tree_entries tree /\
          StronglySorted leaf_before leaves)).
Proof.
  intros name width tree store fuel injective shape holds enough.
  assert (empty_iff : tree = empty_tree <-> tree_entries tree = []).
  { split; [intros ->; reflexivity|].
    intros no_entries. destruct shape as [empty|canonical_tree]; [exact empty|].
    exfalso. exact (canonical_entries_nonempty tree width canonical_tree no_entries). }
  split.
  { split.
    - intros same_name. apply empty_iff. apply injective. exact same_name.
    - intros no_entries. apply empty_iff in no_entries. rewrite no_entries. reflexivity. }
  split.
  { intros no_entries. apply empty_iff in no_entries. subst tree.
    exact (store_holds_root name store empty_tree holds). }
  intros has_entries.
  assert (canonical_tree : canonical width tree).
  { destruct shape as [empty|canonical_tree]; [|exact canonical_tree].
    exfalso. apply has_entries. apply empty_iff. exact empty. }
  destruct (canonical_image_unfolds tree name store width fuel canonical_tree holds enough)
    as (leaves & unfolded & same).
  destruct (radix_count_denotes_entry_occurrences fuel store width (name tree) leaves
              unfolded) as (counted & _ & sorted & _).
  assert (count_is : length leaves = length (tree_entries tree))
    by (rewrite <- same, length_map; reflexivity).
  rewrite count_is in counted.
  split; [exact counted|].
  split.
  - assert (empty_memo : memo_sound store []).
    { intros object remaining count found. discriminate found. }
    pose proof (memoized_radix_count_agrees_with_radix_count fuel store [] width (name tree)
                  empty_memo enough) as agrees.
    rewrite (remaining_width_is_sufficient_fuel (S width) fuel store width (name tree)
               (le_n _) enough), counted in agrees.
    destruct (memoized_radix_count fuel store [] width (name tree)) as [[count memo]|];
      cbn [agrees_with_count] in agrees; [|discriminate agrees].
    destruct agrees as [same_count sound]. injection same_count as count_eq. subst count.
    exists memo. split; [reflexivity|exact sound].
  - exists leaves. split; [exact unfolded|]. split; [exact same|exact sorted].
Qed.

(** Negative control: a finalizer that merges unary nodes below the root but
    not at the root itself. *)
Definition unmerged_root_finalize (tree : radix_tree) : radix_tree :=
  match tree with
  | RadixTree prefix endpoint children =>
      match endpoint, finalize_children finalize_node children with
      | None, [] => empty_tree
      | kept_endpoint, kept => RadixTree prefix kept_endpoint kept
      end
  end.

Definition unmerged_root_radix_edit (tree : radix_tree) (batch : list cas_mutation)
    : option radix_tree :=
  match batch with
  | [] => Some tree
  | _ :: _ =>
      match edit_batch tree batch with
      | Some edited => Some (unmerged_root_finalize edited)
      | None => None
      end
  end.

Definition unmerged_root_sample : radix_tree :=
  RadixTree [] None [(0, RadixTree (repeat 0 47) (Some 50) [])].

Definition merged_sample_result : radix_tree := RadixTree two_leaf_left_key (Some 50) [].

(** Capstone T5m (negative control): evicting the right leaf of the two-leaf
    root leaves a unary root under the mutant.  It denotes the same entry as
    the faithful leaf, but whatever store holds it, the cold reader's plain,
    memoized and enumerating passes all refuse it, while the faithful result
    counts one entry. *)
Theorem unmerged_root_mutant_is_refused_by_the_cold_reader :
  forall (name : radix_tree -> nat) store fuel,
    radix_edit two_leaf_sample [(two_leaf_right_key, Some 51, None)] =
      Some merged_sample_result /\
    unmerged_root_radix_edit two_leaf_sample [(two_leaf_right_key, Some 51, None)] =
      Some unmerged_root_sample /\
    tree_entries unmerged_root_sample = tree_entries merged_sample_result /\
    ~ canonical_root retention_key_len unmerged_root_sample /\
    (store_holds name store merged_sample_result ->
       radix_count (S fuel) store retention_key_len (name merged_sample_result) = Some 1) /\
    (store_holds name store unmerged_root_sample ->
       radix_count fuel store retention_key_len (name unmerged_root_sample) = None /\
       radix_unfold fuel store retention_key_len (name unmerged_root_sample) = None /\
       memoized_radix_count fuel store [] retention_key_len (name unmerged_root_sample) =
         None).
Proof.
  intros name store fuel.
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split.
  { intros [empty|shape]; [discriminate empty|].
    cbn [canonical unmerged_root_sample] in shape.
    destruct shape as (_ & wide & _). cbn [length] in wide. lia. }
  split.
  { intros holds. cbn [radix_count].
    rewrite (store_holds_root name store merged_sample_result holds). reflexivity. }
  intros holds.
  pose proof (store_holds_root name store unmerged_root_sample holds) as root_node.
  destruct fuel as [|smaller]; [split; [reflexivity|split; reflexivity]|].
  cbn [radix_count radix_unfold memoized_radix_count memo_lookup].
  rewrite root_node.
  split; [reflexivity|split; reflexivity].
Qed.

(** * The anchor step *)

Import IncrementalReachability CheckedLedgerTrial CheckedDomainComposition.

(** The V2 anchor: the verbatim current entry, the exact history count and the
    historical radix root.  [anchor_denotation] is the [retention_v2] that the
    cold reader authenticates; [RetentionV2Format.read_v2] reads it back as a
    V1 manifest. *)
Record anchor_v2 := AnchorV2 {
  anchor_current : retained_state;
  anchor_count : nat;
  anchor_root : radix_tree
}.

Definition anchor_denotation (anchor : anchor_v2) : retention_v2 :=
  RetentionV2 (anchor_current anchor) (anchor_count anchor) (tree_entries (anchor_root anchor)).

(** The anchor of a V1 manifest: its history as the canonical radix of the
    history entries. *)
Definition anchor_of (manifest : retention_v1) : anchor_v2 :=
  AnchorV2 (v1_current manifest) (length (v1_historical manifest))
    (partition_build retention_key_len (map history_entry (v1_historical manifest))).

(** The count rule of the editor's caller: [Absent -> Multiplicity] adds one,
    [Multiplicity -> Absent] removes one with checked subtraction, and every
    other outcome keeps the count. *)
Definition cas_count_step (count : nat) (mutation : cas_mutation) : option nat :=
  match mutation with
  | (_, None, Some _) => Some (S count)
  | (_, Some _, None) =>
      match count with
      | 0 => None
      | S smaller => Some smaller
      end
  | _ => Some count
  end.

Fixpoint cas_count (count : nat) (batch : list cas_mutation) : option nat :=
  match batch with
  | [] => Some count
  | mutation :: rest =>
      match cas_count_step count mutation with
      | Some next => cas_count next rest
      | None => None
      end
  end.

Definition commit_mutation (state : retained_state) : cas_mutation :=
  (retention_key (state_identity state), None, Some (state_claim state)).

Definition evict_mutation (victim : retained_state) : cas_mutation :=
  (retention_key (state_identity victim), Some (state_claim victim), None).

(** A commit moves the current entry into the history with one insertion CAS
    and installs the target as the current entry; an eviction removes one
    historical entry with one removal CAS.  The count comes from the CAS
    outcomes. *)
Definition anchor_commit (anchor : anchor_v2) (target : retained_state) : option anchor_v2 :=
  match radix_edit (anchor_root anchor) [commit_mutation (anchor_current anchor)],
        cas_count (anchor_count anchor) [commit_mutation (anchor_current anchor)] with
  | Some root, Some count => Some (AnchorV2 target count root)
  | _, _ => None
  end.

Definition anchor_evict (anchor : anchor_v2) (victim : retained_state) : option anchor_v2 :=
  match radix_edit (anchor_root anchor) [evict_mutation victim],
        cas_count (anchor_count anchor) [evict_mutation victim] with
  | Some root, Some count => Some (AnchorV2 (anchor_current anchor) count root)
  | _, _ => None
  end.

Definition remove_state (victim : retained_state) (states : list retained_state)
    : list retained_state :=
  filter (fun state => negb (bytes_eqb (retention_key (state_identity state))
                                       (retention_key (state_identity victim)))) states.

Definition commit_manifest (manifest : retention_v1) (target : retained_state)
    : retention_v1 :=
  RetentionV1 target (v1_historical manifest ++ [v1_current manifest]).

Definition evict_manifest (manifest : retention_v1) (victim : retained_state)
    : retention_v1 :=
  RetentionV1 (v1_current manifest) (remove_state victim (v1_historical manifest)).

Definition manifest_id_injective (manifest_id : list nat -> nat -> nat) : Prop :=
  forall left_digest left_len right_digest right_len,
    manifest_id left_digest left_len = manifest_id right_digest right_len ->
    left_digest = right_digest /\ left_len = right_len.

(** ** Key order facts *)

Lemma bytes_ltb_eqb_false :
  forall left right, ChannelImage.bytes_ltb left right = true -> bytes_eqb left right = false.
Proof.
  intros left right below. apply bytes_eqb_false_iff. intros same. subst right.
  rewrite ChannelImage.bytes_ltb_irreflexive in below. discriminate below.
Qed.

Lemma bytes_ltb_asymmetric :
  forall left right,
    ChannelImage.bytes_ltb left right = true -> ChannelImage.bytes_ltb right left = false.
Proof.
  intros left right below.
  destruct (ChannelImage.bytes_ltb right left) eqn:above; [|reflexivity].
  pose proof (bytes_ltb_transitive left right left below above) as self.
  rewrite ChannelImage.bytes_ltb_irreflexive in self. discriminate self.
Qed.

Lemma filter_keeps_all {A : Type} (keep : A -> bool) :
  forall values, (forall value, In value values -> keep value = true) ->
    filter keep values = values.
Proof.
  induction values as [|head rest IH]; intros every; [reflexivity|].
  cbn [filter]. rewrite (every head (in_eq _ _)). f_equal. apply IH.
  intros value member. exact (every value (in_cons _ _ _ member)).
Qed.

Lemma forall_filter {A : Type} (P : A -> Prop) (keep : A -> bool) :
  forall values, Forall P values -> Forall P (filter keep values).
Proof.
  intros values every. rewrite Forall_forall in every |- *. intros value member.
  apply filter_In in member as [member _]. exact (every value member).
Qed.

Lemma entries_insert_length :
  forall key claim entries, length (entries_insert key claim entries) = S (length entries).
Proof.
  intros key claim entries. induction entries as [|[candidate value] rest IH]; [reflexivity|].
  cbn [entries_insert]. destruct (ChannelImage.bytes_ltb key candidate); cbn [length];
    [reflexivity|]. rewrite IH. reflexivity.
Qed.

Lemma entry_lookup_sorted_member :
  forall entries key claim,
    StronglySorted key_before entries -> In (key, claim) entries ->
    entry_lookup entries key = Some claim.
Proof.
  intros entries key claim sorted member.
  induction entries as [|[candidate value] rest IH]; [destruct member|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  cbn [entry_lookup].
  destruct member as [same|later].
  - injection same as candidate_is value_is. subst. rewrite bytes_eqb_refl. reflexivity.
  - rewrite Forall_forall in head_below.
    pose proof (head_below (key, claim) later) as below.
    unfold key_before in below. cbn [fst] in below.
    rewrite (bytes_ltb_eqb_false candidate key below). exact (IH rest_sorted later).
Qed.

Lemma entries_remove_present_length :
  forall entries key claim,
    StronglySorted key_before entries -> entry_lookup entries key = Some claim ->
    S (length (entries_remove key entries)) = length entries.
Proof.
  intros entries key claim sorted found. unfold entries_remove.
  induction entries as [|[candidate value] rest IH]; [discriminate found|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  assert (found_rest : bytes_eqb candidate key = false -> entry_lookup rest key = Some claim).
  { intros different. cbn [entry_lookup] in found. rewrite different in found. exact found. }
  cbn [filter fst].
  destruct (bytes_eqb candidate key) eqn:same.
  - apply bytes_eqb_true_iff in same. subst candidate. cbn [negb].
    rewrite (filter_keeps_all _ rest); [reflexivity|].
    intros [other other_value] member. cbv beta. cbn [fst].
    rewrite Forall_forall in head_below.
    pose proof (head_below (other, other_value) member) as below.
    unfold key_before in below. cbn [fst] in below.
    rewrite bytes_eqb_sym, (bytes_ltb_eqb_false key other below). reflexivity.
  - cbn [negb length]. rewrite (IH rest_sorted (found_rest eq_refl)). reflexivity.
Qed.

Lemma entries_update_strongly_sorted :
  forall entries key after,
    StronglySorted key_before entries ->
    StronglySorted key_before (entries_update entries key after).
Proof.
  intros entries key after sorted.
  assert (removed_sorted : StronglySorted key_before (entries_remove key entries))
    by (apply strongly_sorted_filter; exact sorted).
  destruct after as [claim|]; unfold entries_update; [|exact removed_sorted].
  apply entries_insert_sorted; [exact removed_sorted|].
  rewrite entry_lookup_remove, bytes_eqb_refl. reflexivity.
Qed.

(** The count rule tracks the entries exactly: on sorted entries, a batch that
    succeeds moves the count from the old entry count to the new one. *)
Lemma cas_count_tracks_entries :
  forall batch entries updated,
    StronglySorted key_before entries ->
    entries_batch entries batch = Some updated ->
    cas_count (length entries) batch = Some (length updated).
Proof.
  intros batch.
  induction batch as [|[[key before] after] rest IH]; intros entries updated sorted batched.
  - cbn [entries_batch] in batched. injection batched as updated_is. subst updated.
    reflexivity.
  - cbn [entries_batch entries_cas] in batched.
    destruct (endpoint_eqb (entry_lookup entries key) before) eqn:matches;
      [|discriminate batched].
    apply endpoint_eqb_true_iff in matches.
    pose proof (entries_update_strongly_sorted entries key after sorted) as next_sorted.
    pose proof (IH _ _ next_sorted batched) as counted.
    assert (step : cas_count_step (length entries) (key, before, after) =
                   Some (length (entries_update entries key after))).
    { destruct before as [old|]; destruct after as [claim|];
        cbn [cas_count_step entries_update].
      - rewrite entries_insert_length.
        rewrite (entries_remove_present_length entries key old sorted matches). reflexivity.
      - rewrite <- (entries_remove_present_length entries key old sorted matches).
        reflexivity.
      - rewrite (entries_remove_absent key entries matches), entries_insert_length.
        reflexivity.
      - rewrite (entries_remove_absent key entries matches). reflexivity. }
    cbn [cas_count]. rewrite step. exact counted.
Qed.

Lemma entry_lookup_all_below :
  forall entries key,
    (forall entry, In entry entries -> ChannelImage.bytes_ltb (fst entry) key = true) ->
    entry_lookup entries key = None.
Proof.
  intros entries key below.
  induction entries as [|[candidate value] rest IH]; [reflexivity|].
  cbn [entry_lookup].
  rewrite (bytes_ltb_eqb_false candidate key (below (candidate, value) (in_eq _ _))).
  apply IH. intros entry member. exact (below entry (in_cons _ _ _ member)).
Qed.

Lemma entries_insert_last :
  forall entries key claim,
    (forall entry, In entry entries -> ChannelImage.bytes_ltb (fst entry) key = true) ->
    entries_insert key claim entries = entries ++ [(key, claim)].
Proof.
  intros entries key claim below.
  induction entries as [|[candidate value] rest IH]; [reflexivity|].
  cbn [entries_insert app].
  rewrite (bytes_ltb_asymmetric candidate key (below (candidate, value) (in_eq _ _))).
  f_equal. apply IH. intros entry member. exact (below entry (in_cons _ _ _ member)).
Qed.

Lemma retained_key_length :
  forall state,
    retained_state_ok state = true ->
    length (retention_key (state_identity state)) = retention_key_len.
Proof.
  intros state ok. apply retention_key_length.
  destruct (proj1 (identity_ok_true_iff _) (retained_state_ok_identity state ok))
    as (_ & digest_length & _).
  exact digest_length.
Qed.

Lemma history_entry_lengths :
  forall states,
    Forall (fun state => retained_state_ok state = true) states ->
    Forall (fun entry => length (fst entry) = retention_key_len) (map history_entry states).
Proof.
  intros states all_ok. apply Forall_map. eapply Forall_impl; [|exact all_ok].
  intros state ok. cbv beta in ok |- *. unfold history_entry. cbn [fst].
  exact (retained_key_length state ok).
Qed.

Lemma commit_history_facts :
  forall current historical,
    retained_state_ok current = true ->
    Forall (fun state => retained_state_ok state = true) historical ->
    StronglySorted key_before (map history_entry historical) ->
    Forall (fun state => state_lt state current) historical ->
    Forall (fun state => retained_state_ok state = true) (historical ++ [current]) /\
    StronglySorted key_before (map history_entry (historical ++ [current])) /\
    (forall entry, In entry (map history_entry historical) ->
       ChannelImage.bytes_ltb (fst entry) (retention_key (state_identity current)) = true).
Proof.
  intros current historical current_ok historical_ok sorted below.
  assert (keys_below : forall entry, In entry (map history_entry historical) ->
            ChannelImage.bytes_ltb (fst entry) (retention_key (state_identity current)) = true).
  { intros entry member.
    apply in_map_iff in member as [state [entry_is state_member]]. subst entry.
    unfold history_entry. cbn [fst].
    rewrite Forall_forall in historical_ok, below.
    rewrite (retention_key_order_is_entry_order _ _
      (retained_state_ok_identity state (historical_ok state state_member))
      (retained_state_ok_identity current current_ok)).
    exact (below state state_member). }
  split.
  { apply Forall_app. split; [exact historical_ok|].
    constructor; [exact current_ok|constructor]. }
  split; [|exact keys_below].
  rewrite map_app. apply strongly_sorted_app; [exact sorted| |].
  - constructor; constructor.
  - intros left right left_member right_member.
    destruct right_member as [right_is|[]]. subst right.
    exact (keys_below left left_member).
Qed.

Lemma anchor_of_facts :
  forall manifest,
    Forall (fun state => retained_state_ok state = true) (v1_historical manifest) ->
    StronglySorted key_before (map history_entry (v1_historical manifest)) ->
    canonical_root retention_key_len (anchor_root (anchor_of manifest)) /\
    tree_entries (anchor_root (anchor_of manifest)) = map history_entry (v1_historical manifest) /\
    anchor_denotation (anchor_of manifest) = migrate_v1 manifest /\
    read_v2 (anchor_denotation (anchor_of manifest)) = Some manifest.
Proof.
  intros [current historical] all_ok sorted. cbn [v1_historical] in *.
  destruct (canonical_radix_exists_and_is_unique retention_key_len
              (map history_entry historical) sorted (history_entry_lengths historical all_ok))
    as (shape & same & _).
  unfold anchor_of, anchor_denotation, migrate_v1.
  cbn [anchor_root anchor_current anchor_count v1_current v1_historical].
  rewrite same. split; [exact shape|]. split; [reflexivity|]. split; [reflexivity|].
  unfold read_v2. cbn [v2_historical_count v2_history v2_current].
  rewrite length_map, Nat.eqb_refl. cbv beta iota.
  rewrite (decode_history_of_states historical all_ok). reflexivity.
Qed.

Lemma remove_state_cons :
  forall victim state rest,
    remove_state victim (state :: rest) =
      if bytes_eqb (retention_key (state_identity state)) (retention_key (state_identity victim))
      then remove_state victim rest else state :: remove_state victim rest.
Proof.
  intros victim state rest. unfold remove_state. cbn [filter].
  destruct (bytes_eqb (retention_key (state_identity state))
              (retention_key (state_identity victim))); reflexivity.
Qed.

Lemma entries_remove_history :
  forall victim states,
    entries_remove (retention_key (state_identity victim)) (map history_entry states) =
      map history_entry (remove_state victim states).
Proof.
  intros victim states.
  induction states as [|state rest IH]; [reflexivity|].
  rewrite remove_state_cons. unfold entries_remove in *. cbn [map filter].
  change (fst (history_entry state)) with (retention_key (state_identity state)).
  destruct (bytes_eqb (retention_key (state_identity state))
              (retention_key (state_identity victim))); cbn [negb]; [exact IH|].
  cbn [map]. f_equal. exact IH.
Qed.

Lemma composition_entry_eqb_key :
  forall (manifest_id : list nat -> nat -> nat) left right,
    manifest_id_injective manifest_id ->
    retained_state_ok left = true -> retained_state_ok right = true ->
    entry_eqb (composition_entry manifest_id left) (composition_entry manifest_id right) =
      bytes_eqb (retention_key (state_identity left)) (retention_key (state_identity right)).
Proof.
  intros manifest_id [[left_generation left_digest left_len] left_claim]
    [[right_generation right_digest right_len] right_claim] injective left_ok right_ok.
  pose proof (proj2 (proj2 retention_key_is_an_exact_48_byte_bijection)) as key_injective.
  pose proof (retained_state_ok_identity _ left_ok) as left_identity_ok.
  pose proof (retained_state_ok_identity _ right_ok) as right_identity_ok.
  cbn [state_identity] in left_identity_ok, right_identity_ok.
  unfold composition_entry, entry_eqb.
  cbn [state_identity identity_generation identity_digest identity_encoded_len
       entry_generation entry_manifest].
  destruct (Nat.eqb_spec left_generation right_generation)
    as [same_generation|different_generation];
  destruct (Nat.eqb_spec (manifest_id left_digest left_len)
              (manifest_id right_digest right_len)) as [same_manifest|different_manifest];
  cbn [andb].
  - destruct (injective _ _ _ _ same_manifest) as [same_digest same_len].
    subst. symmetry. apply bytes_eqb_refl.
  - symmetry. apply bytes_eqb_false_iff. intros same_key. apply different_manifest.
    pose proof (key_injective _ _ left_identity_ok right_identity_ok same_key) as same.
    injection same as same_generation' same_digest same_len. subst. reflexivity.
  - symmetry. apply bytes_eqb_false_iff. intros same_key. apply different_generation.
    pose proof (key_injective _ _ left_identity_ok right_identity_ok same_key) as same.
    injection same as same_generation same_digest same_len. exact same_generation.
  - symmetry. apply bytes_eqb_false_iff. intros same_key. apply different_generation.
    pose proof (key_injective _ _ left_identity_ok right_identity_ok same_key) as same.
    injection same as same_generation same_digest same_len. exact same_generation.
Qed.

Lemma remove_state_composition :
  forall (manifest_id : list nat -> nat -> nat) victim states,
    manifest_id_injective manifest_id ->
    retained_state_ok victim = true ->
    Forall (fun state => retained_state_ok state = true) states ->
    StronglySorted key_before (map history_entry states) ->
    In victim states ->
    map (composition_entry manifest_id) (remove_state victim states) =
      remove_entry (composition_entry manifest_id victim)
        (map (composition_entry manifest_id) states).
Proof.
  intros manifest_id victim states injective victim_ok.
  induction states as [|state rest IH]; intros all_ok sorted member; [destruct member|].
  cbn [map] in sorted. apply StronglySorted_inv in sorted as [rest_sorted head_below].
  pose proof (Forall_inv all_ok) as state_ok. cbv beta in state_ok.
  pose proof (Forall_inv_tail all_ok) as rest_ok.
  rewrite remove_state_cons. cbn [map remove_entry].
  rewrite (composition_entry_eqb_key manifest_id state victim injective state_ok victim_ok).
  destruct (bytes_eqb (retention_key (state_identity state))
              (retention_key (state_identity victim))) eqn:same_key.
  - apply bytes_eqb_true_iff in same_key.
    assert (kept : remove_state victim rest = rest).
    { unfold remove_state. apply filter_keeps_all. intros other other_member. cbv beta.
      rewrite Forall_forall in head_below.
      pose proof (head_below (history_entry other) (in_map _ _ _ other_member)) as below.
      unfold key_before, history_entry in below. cbn [fst] in below.
      rewrite <- same_key, bytes_eqb_sym, (bytes_ltb_eqb_false _ _ below). reflexivity. }
    rewrite kept. reflexivity.
  - cbn [map]. f_equal. apply IH; [exact rest_ok|exact rest_sorted|].
    destruct member as [here|later]; [|exact later].
    subst state. rewrite bytes_eqb_refl in same_key. discriminate same_key.
Qed.

(** Capstone T4: from the anchor of a valid V1 manifest, a commit (whose
    current key is above every historical key, R8) and an eviction of a
    historical entry are one CAS each.  The successor anchor is the anchor of
    the stepped manifest, the cold reader reads it back, its composition
    entries are [step_entries] of the composition entries, and the count is
    the CAS-outcome count.  The metadata root delta [[A']] in, [[A]] out is
    authorized and leaves exactly the successor anchor as the root. *)
Theorem anchor_step_realizes_the_retention_entry_step :
  forall (manifest_id : list nat -> nat -> nat) manifest,
    retention_v1_valid manifest ->
    read_v2 (anchor_denotation (anchor_of manifest)) = Some manifest /\
    (forall target,
       Forall (fun state => state_lt state (v1_current manifest)) (v1_historical manifest) ->
       anchor_commit (anchor_of manifest) target =
         Some (anchor_of (commit_manifest manifest target)) /\
       read_v2 (anchor_denotation (anchor_of (commit_manifest manifest target))) =
         Some (commit_manifest manifest target) /\
       composition_entries manifest_id (commit_manifest manifest target) =
         step_entries (composition_entries manifest_id manifest)
           (CommitStep (composition_entry manifest_id target))) /\
    (forall victim,
       manifest_id_injective manifest_id ->
       In victim (v1_historical manifest) ->
       anchor_evict (anchor_of manifest) victim =
         Some (anchor_of (evict_manifest manifest victim)) /\
       read_v2 (anchor_denotation (anchor_of (evict_manifest manifest victim))) =
         Some (evict_manifest manifest victim) /\
       composition_entries manifest_id (evict_manifest manifest victim) =
         step_entries (composition_entries manifest_id manifest)
           (EvictStep (composition_entry manifest_id victim))) /\
    (forall old_anchor new_anchor,
       delta_authorized (occurrences [old_anchor]) (RootDelta [new_anchor] [old_anchor]) = true /\
       (forall vertex,
          apply_delta (occurrences [old_anchor]) (RootDelta [new_anchor] [old_anchor]) vertex =
          occurrences [new_anchor] vertex)).
Proof.
  intros manifest_id manifest valid.
  destruct (v1_migration_preserves_entries_order_count_and_roots manifest valid)
    as (_ & _ & keys_sorted & _).
  destruct manifest as [current historical].
  destruct valid as (current_ok & historical_ok & _ & _).
  cbn [v1_current v1_historical] in *.
  assert (sorted : StronglySorted key_before (map history_entry historical))
    by exact keys_sorted.
  destruct (anchor_of_facts (RetentionV1 current historical) historical_ok sorted)
    as (shape & entries_same & _ & read_back).
  cbn [anchor_of anchor_root v1_historical] in shape, entries_same.
  split; [exact read_back|].
  split.
  - intros target below.
    destruct (commit_history_facts current historical current_ok historical_ok sorted below)
      as (successor_ok & successor_sorted & keys_below).
    assert (absent : entry_lookup (map history_entry historical)
                       (retention_key (state_identity current)) = None)
      by (apply entry_lookup_all_below; exact keys_below).
    destruct (radix_edit_changes_only_the_edited_paths retention_key_len
                (map history_entry historical) (retention_key (state_identity current))
                (state_claim current) sorted (history_entry_lengths historical historical_ok)
                (retained_key_length current current_ok) absent)
      as (inserted & _).
    rewrite (entries_insert_last _ _ _ keys_below) in inserted.
    assert (inserted_commit :
      radix_edit (partition_build retention_key_len (map history_entry historical))
        [commit_mutation current] =
      Some (partition_build retention_key_len
              (map history_entry historical ++
                 [(retention_key (state_identity current), state_claim current)])))
      by exact inserted.
    destruct (anchor_of_facts (RetentionV1 target (historical ++ [current])) successor_ok
                successor_sorted) as (_ & _ & _ & successor_read).
    unfold commit_manifest. cbn [v1_current v1_historical].
    split.
    + unfold anchor_commit, anchor_of.
      cbn [anchor_root anchor_current anchor_count v1_current v1_historical].
      rewrite inserted_commit. cbn [cas_count cas_count_step commit_mutation].
      rewrite map_app, length_app. cbn [map length]. rewrite Nat.add_1_r. reflexivity.
    + split; [exact successor_read|].
      unfold composition_entries, step_entries, commit_entries, all_entries.
      cbn [v1_current v1_historical current_entry historical_entries].
      rewrite map_app. reflexivity.
  - split.
    + intros victim injective member.
      assert (victim_ok : retained_state_ok victim = true).
      { rewrite Forall_forall in historical_ok. exact (historical_ok victim member). }
      assert (found : entry_lookup (map history_entry historical)
                        (retention_key (state_identity victim)) = Some (state_claim victim)).
      { apply entry_lookup_sorted_member; [exact sorted|].
        change (retention_key (state_identity victim), state_claim victim)
          with (history_entry victim).
        apply in_map. exact member. }
      assert (batched : entries_batch (map history_entry historical) [evict_mutation victim] =
                        Some (map history_entry (remove_state victim historical))).
      { unfold evict_mutation. cbn [entries_batch entries_cas].
        rewrite found. cbn [endpoint_eqb]. rewrite Nat.eqb_refl.
        cbn [entries_batch entries_update]. rewrite entries_remove_history. reflexivity. }
      assert (evict_keys :
        Forall (fun mutation => length (fst (fst mutation)) = retention_key_len)
          [evict_mutation victim])
        by (constructor; [exact (retained_key_length victim victim_ok)|constructor]).
      destruct (radix_edit_batch_is_the_canonical_rebuild retention_key_len _ _ shape
                  evict_keys) as [evicted _].
      rewrite entries_same, batched in evicted.
      pose proof (cas_count_tracks_entries [evict_mutation victim] _ _ sorted batched)
        as counted.
      rewrite !length_map in counted.
      assert (remaining_ok :
        Forall (fun state => retained_state_ok state = true) (remove_state victim historical))
        by (apply forall_filter; exact historical_ok).
      assert (remaining_sorted :
        StronglySorted key_before (map history_entry (remove_state victim historical))).
      { rewrite <- entries_remove_history. apply strongly_sorted_filter. exact sorted. }
      destruct (anchor_of_facts (RetentionV1 current (remove_state victim historical))
                  remaining_ok remaining_sorted) as (_ & _ & _ & remaining_read).
      unfold evict_manifest. cbn [v1_current v1_historical].
      split.
      * unfold anchor_evict, anchor_of.
        cbn [anchor_root anchor_current anchor_count v1_current v1_historical].
        rewrite evicted, counted. reflexivity.
      * split; [exact remaining_read|].
        unfold composition_entries, step_entries, evict_entry.
        cbn [v1_current v1_historical current_entry historical_entries].
        rewrite (remove_state_composition manifest_id victim historical injective victim_ok
                   historical_ok sorted member).
        reflexivity.
    + intros old_anchor new_anchor. split.
      * unfold delta_authorized. cbn [added_roots removed_roots].
        apply authorized_iff. intros vertex. unfold add_roots. lia.
      * intros vertex. unfold apply_delta, remove_roots, add_roots.
        cbn [added_roots removed_roots]. lia.
Qed.

(** Negative control: a count that adds the inserted leaf objects instead of
    the CAS outcomes.  A content-addressed store inserts an object only when it
    is new, so an inserted leaf equal to an existing leaf object adds nothing. *)
Fixpoint radix_tree_eqb (left right : radix_tree) {struct left} : bool :=
  match left, right with
  | RadixTree left_prefix left_endpoint left_children,
    RadixTree right_prefix right_endpoint right_children =>
      bytes_eqb left_prefix right_prefix && endpoint_eqb left_endpoint right_endpoint &&
      (fix children_eqb (lefts rights : list (nat * radix_tree)) {struct lefts} : bool :=
         match lefts, rights with
         | [], [] => true
         | (left_edge, left_child) :: left_rest, (right_edge, right_child) :: right_rest =>
             Nat.eqb left_edge right_edge && radix_tree_eqb left_child right_child &&
             children_eqb left_rest right_rest
         | _, _ => false
         end) left_children right_children
  end.

Definition tree_member (tree : radix_tree) (nodes : list radix_tree) : bool :=
  existsb (radix_tree_eqb tree) nodes.

Fixpoint distinct_trees (nodes : list radix_tree) : list radix_tree :=
  match nodes with
  | [] => []
  | node :: rest =>
      if tree_member node rest then distinct_trees rest else node :: distinct_trees rest
  end.

Definition full_leaf (tree : radix_tree) : bool :=
  match tree with
  | RadixTree _ (Some _) [] => true
  | _ => false
  end.

Definition inserted_leaf_objects (before after : radix_tree) : nat :=
  length (filter (fun node => full_leaf node && negb (tree_member node (subtrees before)))
            (distinct_trees (subtrees after))).

Definition inserted_leaf_count_mutant (count : nat) (before after : radix_tree) : nat :=
  count + inserted_leaf_objects before after.

Definition aliased_leaf : radix_tree := RadixTree (repeat 0 47) (Some 50) [].

Definition aliased_history : radix_tree :=
  RadixTree [] None [(0, aliased_leaf); (2, RadixTree (repeat 0 47) (Some 51) [])].

Definition aliased_insert : cas_mutation := (1 :: repeat 0 47, None, Some 50).

Definition aliased_result : radix_tree :=
  RadixTree [] None
    [(0, aliased_leaf); (1, aliased_leaf); (2, RadixTree (repeat 0 47) (Some 51) [])].

(** Capstone T4m (negative control): inserting an entry whose leaf equals an
    existing leaf object stages no new leaf, so the inserted-object count
    stays at two while the history has three entries; the CAS-outcome count
    is three. *)
Theorem inserted_object_count_mutant_undercounts_an_aliased_leaf :
  canonical_root retention_key_len aliased_history /\
  radix_edit aliased_history [aliased_insert] = Some aliased_result /\
  length (tree_entries aliased_history) = 2 /\
  cas_count 2 [aliased_insert] = Some (length (tree_entries aliased_result)) /\
  In aliased_leaf (subtrees aliased_history) /\
  inserted_leaf_objects aliased_history aliased_result = 0 /\
  inserted_leaf_count_mutant 2 aliased_history aliased_result <>
    length (tree_entries aliased_result).
Proof.
  split.
  { assert (built : aliased_history =
      partition_build retention_key_len [(0 :: repeat 0 47, 50); (2 :: repeat 0 47, 51)])
      by (vm_compute; reflexivity).
    rewrite built. apply canonical_radix_exists_and_is_unique.
    - apply SSorted_cons; [apply SSorted_cons; [apply SSorted_nil|apply Forall_nil]|].
      apply Forall_cons; [vm_compute; reflexivity|apply Forall_nil].
    - apply Forall_cons; [reflexivity|]. apply Forall_cons; [reflexivity|apply Forall_nil]. }
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split.
  { unfold aliased_history. apply subtrees_in. right. exists 0, aliased_leaf.
    split; [left; reflexivity|]. unfold aliased_leaf. cbn [subtrees]. left. reflexivity. }
  split; [vm_compute; reflexivity|].
  intro same. vm_compute in same. discriminate same.
Qed.

(** * One-time V1 migration *)

(** The migration inserts every history entry into the empty root, one
    insertion CAS per entry, in history order. *)
Definition migration_batch (states : list retained_state) : list cas_mutation :=
  map commit_mutation states.

Lemma strongly_sorted_app_before {A : Type} (R : A -> A -> Prop) :
  forall left right, StronglySorted R (left ++ right) ->
    forall x y, In x left -> In y right -> R x y.
Proof.
  induction left as [|head tail IH]; intros right sorted x y x_member y_member;
    [destruct x_member|].
  cbn [app] in sorted. apply StronglySorted_inv in sorted as [tail_sorted head_before].
  destruct x_member as [here|later].
  - subst x. rewrite Forall_forall in head_before. apply head_before.
    apply in_or_app. right. exact y_member.
  - exact (IH right tail_sorted x y later y_member).
Qed.

Lemma entries_cas_commit_absent :
  forall entries state,
    entry_lookup entries (retention_key (state_identity state)) = None ->
    entries_cas entries (commit_mutation state) =
      Some (entries_insert (retention_key (state_identity state)) (state_claim state) entries).
Proof.
  intros entries state absent. unfold entries_cas, commit_mutation. cbv beta iota.
  rewrite absent. cbn [endpoint_eqb entries_update].
  rewrite (entries_remove_absent _ _ absent). reflexivity.
Qed.

Lemma migration_batch_entries :
  forall states done,
    StronglySorted key_before (map history_entry (done ++ states)) ->
    entries_batch (map history_entry done) (migration_batch states) =
      Some (map history_entry (done ++ states)).
Proof.
  induction states as [|state rest IH]; intros done sorted.
  - cbn [migration_batch map entries_batch]. rewrite app_nil_r. reflexivity.
  - assert (keys_below : forall entry, In entry (map history_entry done) ->
              ChannelImage.bytes_ltb (fst entry) (retention_key (state_identity state)) = true).
    { intros entry member. rewrite map_app in sorted.
      exact (strongly_sorted_app_before key_before _ _ sorted entry (history_entry state)
               member (in_eq _ _)). }
    pose proof (entry_lookup_all_below _ _ keys_below) as absent.
    change (migration_batch (state :: rest)) with (commit_mutation state :: migration_batch rest).
    cbn [entries_batch].
    rewrite (entries_cas_commit_absent _ _ absent), (entries_insert_last _ _ _ keys_below).
    change [(retention_key (state_identity state), state_claim state)]
      with (map history_entry [state]).
    rewrite <- map_app. cbv beta iota.
    rewrite IH.
    + rewrite <- app_assoc. reflexivity.
    + rewrite <- app_assoc. exact sorted.
Qed.

Lemma migration_builds_the_canonical_radix :
  forall states,
    Forall (fun state => retained_state_ok state = true) states ->
    StronglySorted key_before (map history_entry states) ->
    radix_edit empty_tree (migration_batch states) =
      Some (partition_build retention_key_len (map history_entry states)) /\
    cas_count 0 (migration_batch states) = Some (length states).
Proof.
  intros states all_ok sorted.
  assert (keys : Forall (fun mutation => length (fst (fst mutation)) = retention_key_len)
                   (migration_batch states)).
  { unfold migration_batch. apply Forall_map. eapply Forall_impl; [|exact all_ok].
    intros state ok. cbv beta in ok |- *. unfold commit_mutation. cbn [fst].
    exact (retained_key_length state ok). }
  assert (batched : entries_batch [] (migration_batch states) = Some (map history_entry states))
    by exact (migration_batch_entries states [] sorted).
  destruct (radix_edit_batch_is_the_canonical_rebuild retention_key_len empty_tree
              (migration_batch states) (or_introl eq_refl) keys) as [edited _].
  split.
  - rewrite edited. cbn [tree_entries empty_tree endpoint_entries children_entries map app].
    rewrite batched. reflexivity.
  - assert (empty_sorted : StronglySorted key_before []) by constructor.
    pose proof (cas_count_tracks_entries (migration_batch states) [] _ empty_sorted batched)
      as counted.
    rewrite length_map in counted. exact counted.
Qed.

(** Capstone T8: the one-time migration of a valid V1 manifest inserts its
    history into the empty root and obtains the canonical radix of the
    history with the exact count; the migrated anchor denotes
    [migrate_v1 manifest], which reads back as the manifest (capstone 13 of
    [RetentionV2Format]).  Migrating at a commit inserts [historical ++
    [current]] and yields the same anchor as committing on the migrated
    anchor. *)
Theorem v1_migration_is_the_canonical_rebuild_of_its_history :
  forall manifest,
    retention_v1_valid manifest ->
    radix_edit empty_tree (migration_batch (v1_historical manifest)) =
      Some (anchor_root (anchor_of manifest)) /\
    cas_count 0 (migration_batch (v1_historical manifest)) =
      Some (anchor_count (anchor_of manifest)) /\
    canonical_root retention_key_len (anchor_root (anchor_of manifest)) /\
    anchor_denotation (anchor_of manifest) = migrate_v1 manifest /\
    read_v2 (anchor_denotation (anchor_of manifest)) = Some manifest /\
    (forall target,
       Forall (fun state => state_lt state (v1_current manifest)) (v1_historical manifest) ->
       radix_edit empty_tree (migration_batch (v1_historical (commit_manifest manifest target))) =
         Some (anchor_root (anchor_of (commit_manifest manifest target))) /\
       cas_count 0 (migration_batch (v1_historical (commit_manifest manifest target))) =
         Some (anchor_count (anchor_of (commit_manifest manifest target))) /\
       anchor_commit (anchor_of manifest) target =
         Some (anchor_of (commit_manifest manifest target))).
Proof.
  intros manifest valid.
  destruct (v1_migration_preserves_entries_order_count_and_roots manifest valid)
    as (read_back & _ & keys_sorted & _).
  pose proof valid as (current_ok & historical_ok & _ & _).
  assert (sorted : StronglySorted key_before (map history_entry (v1_historical manifest)))
    by exact keys_sorted.
  destruct (anchor_of_facts manifest historical_ok sorted) as (shape & _ & denotes & _).
  destruct (migration_builds_the_canonical_radix (v1_historical manifest) historical_ok sorted)
    as [migrated counted].
  split; [exact migrated|].
  split; [exact counted|].
  split; [exact shape|].
  split; [exact denotes|].
  split; [rewrite denotes; exact read_back|].
  intros target below.
  destruct (commit_history_facts (v1_current manifest) (v1_historical manifest) current_ok
              historical_ok sorted below) as (successor_ok & successor_sorted & _).
  destruct (migration_builds_the_canonical_radix _ successor_ok successor_sorted)
    as [successor_migrated successor_counted].
  destruct (anchor_step_realizes_the_retention_entry_step (fun _ _ => 0) manifest valid)
    as (_ & commit & _).
  destruct (commit target below) as (committed & _ & _).
  split; [exact successor_migrated|].
  split; [exact successor_counted|exact committed].
Qed.

(** Negative control: a migration that inserts the current entry into the
    history but keeps it as the anchor's current entry. *)
Definition current_in_history_migration_mutant (manifest : retention_v1) : anchor_v2 :=
  AnchorV2 (v1_current manifest) (S (length (v1_historical manifest)))
    (partition_build retention_key_len
       (map history_entry (v1_historical manifest ++ [v1_current manifest]))).

Definition sample_state : retained_state := RetainedState early_identity 50.

Definition sample_manifest : retention_v1 := RetentionV1 sample_state [].

(** Capstone T8m (negative control): the mutant anchor of a one-state manifest
    reads back with the current state also in the history, which the V1
    rules refuse, and it counts the current manifest twice among the retained
    roots instead of once. *)
Theorem current_in_history_migration_mutant_duplicates_the_current_entry :
  forall (manifest_id : list nat -> nat -> nat),
    retention_v1_valid sample_manifest /\
    read_v2 (anchor_denotation (anchor_of sample_manifest)) = Some sample_manifest /\
    read_v2 (anchor_denotation (current_in_history_migration_mutant sample_manifest)) =
      Some (RetentionV1 sample_state [sample_state]) /\
    ~ retention_v1_valid (RetentionV1 sample_state [sample_state]) /\
    retained_roots (composition_entries manifest_id sample_manifest)
      (entry_manifest (composition_entry manifest_id sample_state)) = 1 /\
    retained_roots (composition_entries manifest_id (RetentionV1 sample_state [sample_state]))
      (entry_manifest (composition_entry manifest_id sample_state)) = 2.
Proof.
  intros manifest_id.
  split.
  { unfold retention_v1_valid, sample_manifest. cbn [v1_current v1_historical].
    split; [vm_compute; reflexivity|]. split; [constructor|]. split; constructor. }
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split.
  { intros (_ & _ & _ & distinct). cbn [v1_historical v1_current] in distinct.
    apply Forall_inv in distinct. apply distinct. reflexivity. }
  unfold retained_roots, entry_manifests, all_entries, composition_entries, sample_manifest.
  cbn [v1_current v1_historical current_entry historical_entries map app occurrences].
  rewrite Nat.eqb_refl. split; reflexivity.
Qed.

(** * The metadata ledger *)

Import CheckedReachability.

(** The metadata object graph.  The metadata domain of an anchor holds the
    anchor object, whose only child reference is the historical radix root,
    and the node objects of the radix image, whose child references are the
    names of their child subtrees: the Rust [push_cursor] decodes an anchor
    to its historical root and a radix node to its child targets.  A trial
    environment realizes the metadata of an anchor when its graph returns
    exactly these references for the anchor and for every node name of the
    image; [stored_children_realize] shows that decoding a store that holds
    the image (the store the cold reader admits) returns them. *)
Definition node_children (tree : radix_tree) : list (nat * radix_tree) :=
  match tree with
  | RadixTree _ _ children => children
  end.

Definition child_names (name : radix_tree -> nat) (tree : radix_tree) : list nat :=
  map (fun child => name (snd child)) (node_children tree).

Definition stored_children (store : list (nat * radix_node)) (vertex : nat) : list nat :=
  match lookup_radix_node store vertex with
  | Some node => map snd (radix_children node)
  | None => []
  end.

Definition metadata_object (name : radix_tree -> nat) (anchor : nat) (root : radix_tree)
    : membership :=
  fun vertex =>
    Nat.eqb vertex anchor || existsb (fun node => Nat.eqb (name node) vertex) (subtrees root).

Definition realizes_metadata (name : radix_tree -> nat) (env : trial_environment)
    (anchor : nat) (root : radix_tree) : Prop :=
  In anchor (universe env) /\ graph env anchor = [name root] /\
  forall node, In node (subtrees root) ->
    In (name node) (universe env) /\ graph env (name node) = child_names name node.

Lemma stored_children_realize :
  forall name store root node,
    store_holds name store root -> In node (subtrees root) ->
    stored_children store (name node) = child_names name node.
Proof.
  intros name store root node holds member. unfold stored_children.
  rewrite (holds node member). destruct node as [prefix endpoint children].
  cbn [encode_node radix_children]. unfold child_names. cbn [node_children].
  rewrite map_map. reflexivity.
Qed.

Lemma bool_eq_of_iff :
  forall left right : bool, (left = true <-> right = true) -> left = right.
Proof.
  intros [|] [|] same; try reflexivity; exfalso; destruct same as [forward backward].
  - discriminate (forward eq_refl).
  - discriminate (backward eq_refl).
Qed.

Lemma subtrees_self : forall tree, In tree (subtrees tree).
Proof. intros [prefix endpoint children]. cbn [subtrees]. left. reflexivity. Qed.

Lemma subtrees_trans :
  forall root middle node,
    In middle (subtrees root) -> In node (subtrees middle) -> In node (subtrees root).
Proof.
  intros root.
  induction root as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros middle node middle_member node_member.
  apply subtrees_in in middle_member.
  destruct middle_member as [same|(edge & child & member & inside)].
  - subst middle. exact node_member.
  - apply subtrees_in. right. exists edge, child. split; [exact member|].
    exact (IH (edge, child) member middle node inside node_member).
Qed.

Lemma subtrees_child_closed :
  forall root node edge child,
    In node (subtrees root) -> In (edge, child) (node_children node) ->
    In child (subtrees root).
Proof.
  intros root node edge child node_member edge_member.
  apply (subtrees_trans root node child node_member).
  destruct node as [prefix endpoint children]. cbn [node_children] in edge_member.
  apply subtrees_in. right. exists edge, child.
  split; [exact edge_member|apply subtrees_self].
Qed.

Lemma reachable_roots_extensional :
  forall children vertices (left right : counter) vertex,
    (forall query, left query = right query) ->
    reachable children vertices left vertex -> reachable children vertices right vertex.
Proof.
  intros children vertices left right vertex same path.
  induction path as [vertex root|parent child path IH member edge].
  - apply (@reachable_root children vertices right vertex). rewrite <- same. exact root.
  - exact (@reachable_child children vertices right parent child IH member edge).
Qed.

Lemma metadata_subtrees_reachable :
  forall name env anchor root,
    realizes_metadata name env anchor root ->
    forall tree,
      (forall node, In node (subtrees tree) -> In node (subtrees root)) ->
      reachable (graph env) (universe env) (occurrences [anchor]) (name tree) ->
      forall node, In node (subtrees tree) ->
        reachable (graph env) (universe env) (occurrences [anchor]) (name node).
Proof.
  intros name env anchor root (_ & _ & node_edges) tree.
  induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros inside tree_path node member.
  apply subtrees_in in member.
  destruct member as [same|(edge & child & child_member & below)].
  - subst node. exact tree_path.
  - destruct (node_edges (RadixTree prefix endpoint children)
                (inside _ (subtrees_self _))) as (tree_universe & tree_edges).
    apply (IH (edge, child) child_member).
    + intros other other_member. apply inside. apply subtrees_in. right.
      exists edge, child. split; [exact child_member|exact other_member].
    + apply (@reachable_child (graph env) (universe env) (occurrences [anchor])
               (name (RadixTree prefix endpoint children)) (name child)).
      * exact tree_path.
      * exact tree_universe.
      * rewrite tree_edges. unfold child_names. cbn [node_children].
        exact (in_map (fun child => name (snd child)) children (edge, child) child_member).
    + exact below.
Qed.

(** The metadata objects of an anchor are exactly the objects reachable from
    the anchor root occurrence. *)
Lemma metadata_object_is_reachability :
  forall name env anchor root,
    realizes_metadata name env anchor root ->
    forall vertex,
      metadata_object name anchor root vertex = true <->
      reachable (graph env) (universe env) (occurrences [anchor]) vertex.
Proof.
  intros name env anchor root realized.
  pose proof realized as (anchor_member & anchor_edge & node_edges).
  assert (anchor_path : reachable (graph env) (universe env) (occurrences [anchor]) anchor).
  { apply (@reachable_root (graph env) (universe env) (occurrences [anchor]) anchor).
    cbn [occurrences]. rewrite Nat.eqb_refl. lia. }
  assert (root_path :
    reachable (graph env) (universe env) (occurrences [anchor]) (name root)).
  { apply (@reachable_child (graph env) (universe env) (occurrences [anchor]) anchor
             (name root) anchor_path anchor_member).
    rewrite anchor_edge. left. reflexivity. }
  intros vertex. split.
  - intros object. unfold metadata_object in object.
    apply orb_true_iff in object. destruct object as [is_anchor|is_node].
    + apply Nat.eqb_eq in is_anchor. subst vertex. exact anchor_path.
    + apply existsb_exists in is_node. destruct is_node as (node & member & named).
      apply Nat.eqb_eq in named. subst vertex.
      exact (metadata_subtrees_reachable name env anchor root realized root
               (fun other other_member => other_member) root_path node member).
  - intros path.
    induction path as [vertex root_count|parent child parent_path IH parent_member edge].
    + unfold metadata_object. cbn [occurrences] in root_count.
      destruct (Nat.eqb vertex anchor) eqn:same; [reflexivity|].
      rewrite ?same in root_count. cbn in root_count. lia.
    + unfold metadata_object in IH |- *.
      apply orb_true_iff in IH. destruct IH as [is_anchor|is_node].
      * apply Nat.eqb_eq in is_anchor. subst parent.
        rewrite anchor_edge in edge. destruct edge as [same|[]]. subst child.
        apply orb_true_iff. right. apply existsb_exists. exists root.
        split; [apply subtrees_self|apply Nat.eqb_refl].
      * apply existsb_exists in is_node. destruct is_node as (node & member & named).
        apply Nat.eqb_eq in named. subst parent.
        destruct (node_edges node member) as (_ & node_edge).
        rewrite node_edge in edge. unfold child_names in edge.
        apply in_map_iff in edge.
        destruct edge as ([edge_byte target] & target_named & target_member).
        cbn [snd] in target_named. subst child.
        apply orb_true_iff. right. apply existsb_exists. exists target.
        split; [exact (subtrees_child_closed root node edge_byte target member
                         target_member)
               |apply Nat.eqb_refl].
Qed.

(** The occurrence-count seed.  The count of an object is its root
    multiplicity plus its child occurrences across the distinct objects of
    the closure (the universe lists every object once), and the active set is
    the closure: what cold admission computes from the reader's node map,
    without further reads. *)
Definition seed_ledger (env : trial_environment) (roots : counter) (objects : membership)
    : ledger :=
  {| counts := fun vertex => roots vertex + incoming (graph env) (universe env) objects vertex;
     active := objects |}.

Definition metadata_seed (name : radix_tree -> nat) (env : trial_environment)
    (anchor : nat) (root : radix_tree) : ledger :=
  seed_ledger env (occurrences [anchor]) (metadata_object name anchor root).

Definition seed_predecessor (env : trial_environment) (roots : counter) (state : ledger)
    : predecessor :=
  Predecessor state roots (inventory (universe env) state unit_weight)
    (inventory (universe env) state (object_bytes env)).

Lemma seed_ledger_is_quiescent :
  forall env roots objects,
    (forall vertex,
       objects vertex = true <-> reachable (graph env) (universe env) roots vertex) ->
    coherent (seed_ledger env roots objects) /\
    quiescent (graph env) (universe env) roots (seed_ledger env roots objects).
Proof.
  intros env roots objects support. split.
  - intros vertex. cbn [seed_ledger counts active].
    apply bool_eq_of_iff. rewrite support, Nat.ltb_lt. split.
    + intros path. destruct path as [target root|parent target parent_path member edge].
      * lia.
      * assert (contribution : 0 < incoming (graph env) (universe env) objects target).
        { apply incoming_positive. exists parent.
          split; [exact member|]. split; [apply support; exact parent_path|exact edge]. }
        lia.
    + intros positive.
      destruct (Nat.lt_ge_cases 0 (roots vertex)) as [root|no_root].
      * apply (@reachable_root (graph env) (universe env) roots vertex). exact root.
      * assert (contribution : 0 < incoming (graph env) (universe env) objects vertex)
          by lia.
        apply incoming_positive in contribution.
        destruct contribution as (parent & member & enabled & edge).
        apply (@reachable_child (graph env) (universe env) roots parent vertex).
        -- apply support. exact enabled.
        -- exact member.
        -- exact edge.
  - intros vertex. reflexivity.
Qed.

(** Capstone T7: the occurrence-count seed of the metadata domain is the
    credited cold ledger.  Its support is the closure of the anchor, it is a
    credited cold build of the single metadata epoch (the anchor), every
    credited cold build of that epoch equals it, and as a predecessor it is a
    valid metadata base of the composed trial. *)
Theorem metadata_ledger_seed_is_the_cold_ledger :
  forall (name : radix_tree -> nat) env anchor root rank,
    edges_descend env rank ->
    realizes_metadata name env anchor root ->
    (forall vertex,
       active (metadata_seed name env anchor root) vertex = true <->
       reachable (graph env) (universe env) (occurrences [anchor]) vertex) /\
    CreditedClosureLedger.credited_build (graph env) (universe env) [[anchor]]
      (metadata_seed name env anchor root) /\
    (forall cold,
       CreditedClosureLedger.credited_build (graph env) (universe env) [[anchor]] cold ->
       forall vertex,
         counts (metadata_seed name env anchor root) vertex = counts cold vertex /\
         active (metadata_seed name env anchor root) vertex = active cold vertex) /\
    domain_base_valid env (occurrences [anchor])
      (seed_predecessor env (occurrences [anchor]) (metadata_seed name env anchor root)).
Proof.
  intros name env anchor root rank descending realized.
  pose proof (metadata_object_is_reachability name env anchor root realized) as support.
  destruct (seed_ledger_is_quiescent env (occurrences [anchor])
              (metadata_object name anchor root) support)
    as (seed_coherent & seed_quiescent).
  split; [exact support|].
  split.
  { apply (@CreditedClosureLedger.credited_build_epoch (graph env) (universe env) []
             [anchor] CreditedClosureLedger.empty_ledger (metadata_seed name env anchor root)).
    - apply CreditedClosureLedger.credited_build_start.
    - exact seed_coherent.
    - intros vertex. reflexivity. }
  split.
  { intros cold built vertex.
    destruct (@CreditedClosureLedger.credited_build_is_quiescent (graph env) (universe env)
                [[anchor]] cold built) as (cold_coherent & cold_quiescent).
    exact (@quiescent_ledger_is_unique (graph env) (universe env) (occurrences [anchor])
             (metadata_seed name env anchor root) cold rank seed_coherent seed_quiescent
             cold_coherent cold_quiescent descending vertex). }
  split; [exact seed_coherent|].
  split; [intros vertex; reflexivity|].
  split; [exact seed_quiescent|].
  split; reflexivity.
Qed.

(** Concrete metadata fixtures.  [metadata_fixture_name] names the radix
    fixtures of capstone T4m and the committed history below; the fixture
    graph is the metadata graph of those images under that naming (the
    realization is checked by [realizes_metadata_by_test]). *)
Definition committed_history : radix_tree :=
  RadixTree [] None
    [(0, aliased_leaf); (2, RadixTree (repeat 0 47) (Some 51) []);
     (3, RadixTree (repeat 0 47) (Some 52) [])].

Definition metadata_fixture_name (tree : radix_tree) : nat :=
  if radix_tree_eqb tree aliased_leaf then 1
  else if radix_tree_eqb tree (RadixTree (repeat 0 47) (Some 51) []) then 2
  else if radix_tree_eqb tree aliased_history then 3
  else if radix_tree_eqb tree aliased_result then 4
  else if radix_tree_eqb tree committed_history then 7
  else if radix_tree_eqb tree (RadixTree (repeat 0 47) (Some 52) []) then 8
  else 0.

Definition metadata_fixture_graph (vertex : nat) : list nat :=
  match vertex with
  | 3 => [1; 2]
  | 4 => [1; 1; 2]
  | 5 => [4]
  | 6 => [3]
  | 7 => [1; 2; 8]
  | 9 => [3]
  | 10 => [7]
  | _ => []
  end.

Definition metadata_fixture : trial_environment :=
  fixture metadata_fixture_graph [1; 2; 3; 4; 5; 6; 7; 8; 9; 10] no_reads.

Definition realizes_metadata_test (name : radix_tree -> nat) (env : trial_environment)
    (anchor : nat) (root : radix_tree) : bool :=
  existsb (Nat.eqb anchor) (universe env) &&
  bytes_eqb (graph env anchor) [name root] &&
  forallb (fun node => existsb (Nat.eqb (name node)) (universe env) &&
                       bytes_eqb (graph env (name node)) (child_names name node))
    (subtrees root).

Lemma in_of_existsb_eqb :
  forall vertex vertices, existsb (Nat.eqb vertex) vertices = true -> In vertex vertices.
Proof.
  intros vertex vertices found. apply existsb_exists in found.
  destruct found as (other & member & same). apply Nat.eqb_eq in same. subst other.
  exact member.
Qed.

Lemma realizes_metadata_by_test :
  forall name env anchor root,
    realizes_metadata_test name env anchor root = true ->
    realizes_metadata name env anchor root.
Proof.
  intros name env anchor root test. unfold realizes_metadata_test in test.
  apply andb_true_iff in test. destruct test as (head & nodes).
  apply andb_true_iff in head. destruct head as (anchor_member & anchor_edge).
  split; [exact (in_of_existsb_eqb anchor (universe env) anchor_member)|].
  split; [apply bytes_eqb_true_iff; exact anchor_edge|].
  intros node member. rewrite forallb_forall in nodes.
  pose proof (nodes node member) as node_test. apply andb_true_iff in node_test.
  destruct node_test as (node_member & node_edge).
  split; [exact (in_of_existsb_eqb (name node) (universe env) node_member)|].
  apply bytes_eqb_true_iff. exact node_edge.
Qed.

(** Negative control: a seed that counts the child references of a node as a
    set.  Content addressing lets one radix node refer to the same leaf
    object under two edges (an aliased leaf); the set seed counts that leaf
    once, so evicting one alias deactivates the leaf while the other edge
    still refers to it. *)
Fixpoint distinct_incoming (children : nat -> list nat) (vertices : list nat)
    (objects : membership) (query : nat) : nat :=
  match vertices with
  | [] => 0
  | parent :: rest =>
      (if objects parent && existsb (Nat.eqb query) (children parent) then 1 else 0) +
      distinct_incoming children rest objects query
  end.

Definition set_seed_ledger (env : trial_environment) (roots : counter) (objects : membership)
    : ledger :=
  {| counts := fun vertex =>
       roots vertex + distinct_incoming (graph env) (universe env) objects vertex;
     active := objects |}.

Definition shared_child_eviction : cas_mutation := (1 :: repeat 0 47, Some 50, None).

(** Capstone T7m (negative control): the eviction of one alias of the
    aliased leaf.  The occurrence seed counts the leaf twice and the trial
    keeps it, once, as the cold ledger of the successor does; the set seed
    counts it once and the trial deactivates it although the successor root
    still refers to it. *)
Theorem set_seed_mutant_drops_a_shared_radix_child :
  radix_edit aliased_result [shared_child_eviction] = Some aliased_history /\
  realizes_metadata metadata_fixture_name metadata_fixture 5 aliased_result /\
  realizes_metadata metadata_fixture_name metadata_fixture 6 aliased_history /\
  counts (metadata_seed metadata_fixture_name metadata_fixture 5 aliased_result) 1 = 2 /\
  counts (set_seed_ledger metadata_fixture (occurrences [5])
            (metadata_object metadata_fixture_name 5 aliased_result)) 1 = 1 /\
  match run_trial metadata_fixture Faithful None prefer_cursors
          (seed_predecessor metadata_fixture (occurrences [5])
             (metadata_seed metadata_fixture_name metadata_fixture 5 aliased_result))
          [6] [5] with
  | TrialResult _ (TrialPrepared evidence) =>
      counts (prepared_ledger evidence) 1 = 1 /\ active (prepared_ledger evidence) 1 = true
  | TrialResult _ (TrialRefused _) => False
  end /\
  match run_trial metadata_fixture Faithful None prefer_cursors
          (seed_predecessor metadata_fixture (occurrences [5])
             (set_seed_ledger metadata_fixture (occurrences [5])
                (metadata_object metadata_fixture_name 5 aliased_result)))
          [6] [5] with
  | TrialResult _ (TrialPrepared evidence) =>
      active (prepared_ledger evidence) 1 = false /\
      active (prepared_ledger evidence) 3 = true /\
      In 1 (graph metadata_fixture 3) /\
      active (metadata_seed metadata_fixture_name metadata_fixture 6 aliased_history) 1 = true
  | TrialResult _ (TrialRefused _) => False
  end.
Proof.
  split; [vm_compute; reflexivity|].
  split; [apply realizes_metadata_by_test; vm_compute; reflexivity|].
  split; [apply realizes_metadata_by_test; vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; split; reflexivity|].
  vm_compute. split; [reflexivity|]. split; [reflexivity|].
  split; [left; reflexivity|reflexivity].
Qed.

(** * Trial work of a metadata root change *)

(** The changed objects: the metadata objects of exactly one of the two
    anchors. *)
Definition changed_objects (name : radix_tree -> nat) (old_anchor : nat)
    (old_root : radix_tree) (new_anchor : nat) (new_root : radix_tree) : membership :=
  fun vertex =>
    xorb (metadata_object name old_anchor old_root vertex)
      (metadata_object name new_anchor new_root vertex).

(** The trial charges one expansion exactly when an activation moves a count
    from zero (and the live object counter up by one) or a deactivation
    moves a count to zero (and the live object counter down by one); cursor
    steps change neither.  [expansion_balance] is that invariant per
    phase. *)
Definition expansion_balance (mode : phase) (start current : trial_state) : Prop :=
  match mode with
  | Activation =>
      live_objects current + expansions (usage start) =
        live_objects start + expansions (usage current)
  | Deactivation =>
      live_objects current + expansions (usage current) =
        live_objects start + expansions (usage start)
  end.

Lemma faithful_attempt_keeps_expansion_balance :
  forall env mode prefer start current next,
    expansion_balance mode start current ->
    attempt_step env Faithful mode prefer current = AttemptStep next ->
    expansion_balance mode start next.
Proof.
  intros env mode prefer start current next balanced step.
  apply attempt_step_cases in step.
  destruct step as [(vertex & rest & _ & step)|(parent & remaining & suspended & _ & step)].
  - destruct mode; cbn [event_attempt] in step.
    + injection step as <-. cbn [expansion_balance live_objects usage] in *.
      unfold with_peaks, charge_expansion, activated_weight, unit_weight.
      destruct (counts (resident (workspace current)) vertex =? 0); cbn [expansions]; lia.
    + destruct (counts (resident (workspace current)) vertex =? 0); [discriminate step|].
      destruct (checked_sub (live_objects current)
                  (deactivated_weight (resident (workspace current)) vertex unit_weight))
        as [next_objects|] eqn:objects_left; [|discriminate step].
      destruct (checked_sub (live_bytes current)
                  (deactivated_weight (resident (workspace current)) vertex
                     (object_bytes env)))
        as [next_bytes|]; [|discriminate step].
      injection step as <-. cbn [expansion_balance live_objects usage] in *.
      apply checked_sub_is_exact in objects_left. destruct objects_left as (objects_left & _).
      unfold with_peaks, charge_expansion.
      unfold deactivated_weight, unit_weight in objects_left.
      destruct (counts (resident (workspace current)) vertex =? 1); cbn [expansions]; lia.
  - destruct remaining as [|child tail]; cbn [cursor_attempt transferred_events] in step;
      injection step as <-; destruct mode; cbn [expansion_balance live_objects usage] in *;
      unfold with_peaks; try unfold charge_reference; cbn [expansions]; lia.
Qed.

Lemma run_phase_keeps_expansion_balance :
  forall env budget schedule mode fuel index start final,
    run_phase env Faithful budget schedule mode fuel index start = PhaseDrained final ->
    expansion_balance mode start final.
Proof.
  intros env budget schedule mode fuel index start final drained_run.
  assert (initial : expansion_balance mode start start)
    by (destruct mode; cbn [expansion_balance]; reflexivity).
  pose proof (run_phase_invariant env Faithful budget schedule mode
                (expansion_balance mode start)
                (fun prefer current next balanced step _ =>
                   faithful_attempt_keeps_expansion_balance env mode prefer start current next
                     balanced step)
                fuel index start initial) as kept.
  rewrite drained_run in kept. exact kept.
Qed.

Lemma union_expansion_count :
  forall vertices (old new : membership) weight,
    selected_sum vertices (fun vertex => old vertex || new vertex) weight +
    selected_sum vertices (fun vertex => old vertex || new vertex) weight =
    selected_sum vertices old weight + selected_sum vertices new weight +
    selected_sum vertices (fun vertex => xorb (old vertex) (new vertex)) weight.
Proof.
  induction vertices as [|head tail IH]; intros old new weight; [reflexivity|].
  cbn [selected_sum]. specialize (IH old new weight).
  destruct (old head), (new head); cbn [orb xorb]; lia.
Qed.

Lemma changed_frontiers_partition_the_changed_objects :
  forall vertices (old new : membership) weight,
    selected_sum vertices (fun vertex => negb (old vertex) && (old vertex || new vertex))
      weight +
    selected_sum vertices (fun vertex => (old vertex || new vertex) && negb (new vertex))
      weight =
    selected_sum vertices (fun vertex => xorb (old vertex) (new vertex)) weight.
Proof.
  induction vertices as [|head tail IH]; intros old new weight; [reflexivity|].
  cbn [selected_sum]. specialize (IH old new weight).
  destruct (old head), (new head); cbn [orb andb negb xorb]; lia.
Qed.

Lemma selected_sum_unit_is_filter_length :
  forall vertices selected,
    selected_sum vertices selected unit_weight = length (filter selected vertices).
Proof.
  induction vertices as [|head tail IH]; intros selected; [reflexivity|].
  cbn [selected_sum filter]. rewrite IH. unfold unit_weight.
  destruct (selected head); cbn [length]; lia.
Qed.

Lemma selected_count_is_covered :
  forall vertices selected listed,
    NoDup vertices ->
    (forall vertex, In vertex vertices -> selected vertex = true -> In vertex listed) ->
    selected_sum vertices selected unit_weight <= length listed.
Proof.
  intros vertices selected listed unique covered.
  rewrite selected_sum_unit_is_filter_length.
  apply NoDup_incl_length.
  - apply NoDup_filter. exact unique.
  - intros vertex member. apply filter_In in member. destruct member as [inside chosen].
    exact (covered vertex inside chosen).
Qed.

Lemma xorb_true_cases :
  forall left right : bool,
    xorb left right = true -> (left = true /\ right = false) \/ (left = false /\ right = true).
Proof.
  intros [|] [|] same; cbn in same; try discriminate same; [left|right]; split; reflexivity.
Qed.

(** A changed object of one insertion or eviction is one of the two anchors
    or a node on the search path of the key in the larger or the smaller
    image, or the one merged sibling (capstone T3). *)
Lemma changed_objects_are_on_the_edited_paths :
  forall (name : radix_tree -> nat) old_anchor new_anchor small large sibling key vertex,
    (forall node, In node (subtrees large) -> ~ In node (subtrees small) ->
       In node (search_path large key) \/ In node sibling) ->
    (forall node, In node (subtrees small) -> ~ In node (subtrees large) ->
       In node (search_path small key)) ->
    forall old_root new_root,
      (old_root = small /\ new_root = large) \/ (old_root = large /\ new_root = small) ->
      xorb (metadata_object name old_anchor old_root vertex)
        (metadata_object name new_anchor new_root vertex) = true ->
      In vertex ([old_anchor; new_anchor] ++
                 map name (search_path large key ++ sibling ++ search_path small key)).
Proof.
  intros name old_anchor new_anchor small large sibling key vertex added_cover removed_cover
    old_root new_root pair changed.
  destruct (Nat.eqb vertex old_anchor) eqn:is_old.
  { apply Nat.eqb_eq in is_old. subst vertex. left. reflexivity. }
  destruct (Nat.eqb vertex new_anchor) eqn:is_new.
  { apply Nat.eqb_eq in is_new. subst vertex. right. left. reflexivity. }
  right. right.
  unfold metadata_object in changed. rewrite is_old, is_new in changed.
  cbn [orb] in changed.
  assert (one_side :
    (existsb (fun node => Nat.eqb (name node) vertex) (subtrees large) = true /\
     existsb (fun node => Nat.eqb (name node) vertex) (subtrees small) = false) \/
    (existsb (fun node => Nat.eqb (name node) vertex) (subtrees small) = true /\
     existsb (fun node => Nat.eqb (name node) vertex) (subtrees large) = false)).
  { apply xorb_true_cases in changed.
    destruct pair as [(old_is & new_is)|(old_is & new_is)]; subst old_root new_root;
      destruct changed as [(first & second)|(first & second)]; auto. }
  apply in_map_iff.
  destruct one_side as [(in_large & not_small)|(in_small & not_large)].
  - apply existsb_exists in in_large. destruct in_large as (node & member & named).
    exists node. apply Nat.eqb_eq in named. split; [exact named|].
    assert (outside : ~ In node (subtrees small)).
    { intros inside.
      assert (found :
        existsb (fun node => Nat.eqb (name node) vertex) (subtrees small) = true).
      { apply existsb_exists. exists node.
        split; [exact inside|apply Nat.eqb_eq; exact named]. }
      rewrite found in not_small. discriminate not_small. }
    apply in_app_iff. destruct (added_cover node member outside) as [on_path|beside].
    + left. exact on_path.
    + right. apply in_app_iff. left. exact beside.
  - apply existsb_exists in in_small. destruct in_small as (node & member & named).
    exists node. apply Nat.eqb_eq in named. split; [exact named|].
    assert (outside : ~ In node (subtrees large)).
    { intros inside.
      assert (found :
        existsb (fun node => Nat.eqb (name node) vertex) (subtrees large) = true).
      { apply existsb_exists. exists node.
        split; [exact inside|apply Nat.eqb_eq; exact named]. }
      rewrite found in not_large. discriminate not_large. }
    apply in_app_iff. right. apply in_app_iff. right.
    exact (removed_cover node member outside).
Qed.

(** Capstone T6: the metadata root change [[A']] in, [[A]] out, runs as one
    faithful ledger trial (activation before deactivation) from the
    occurrence seed of [A].  Its prepared ledger is the occurrence seed of
    [A'] (the cold ledger, [prepared_trial_agrees_with_cold_ledger]), its
    expansion counter is the number of changed objects, its cursor steps are
    the two root events plus the expansion credit of the changed objects
    ([root_delta_execution_has_exact_changed_frontier_work] per phase), and
    for one insertion or eviction of a fixed-width key the root events plus
    the expansions are at most [2 w + 7], independent of the history. *)
Theorem metadata_trial_work_is_the_changed_objects :
  forall (name : radix_tree -> nat) env budget schedule base old_anchor old_root
         new_anchor new_root kept evidence rank,
    trial_input_valid env base [new_anchor] [old_anchor] ->
    edges_descend env rank ->
    (forall vertex, base_roots base vertex = occurrences [old_anchor] vertex) ->
    realizes_metadata name env old_anchor old_root ->
    realizes_metadata name env new_anchor new_root ->
    run_trial env Faithful budget schedule base [new_anchor] [old_anchor] =
      TrialResult kept (TrialPrepared evidence) ->
    (forall vertex,
       counts (prepared_ledger evidence) vertex =
         counts (metadata_seed name env new_anchor new_root) vertex /\
       active (prepared_ledger evidence) vertex =
         active (metadata_seed name env new_anchor new_root) vertex) /\
    prepared_objects evidence =
      selected_sum (universe env) (metadata_object name new_anchor new_root) unit_weight /\
    expansions (prepared_usage evidence) =
      selected_sum (universe env)
        (changed_objects name old_anchor old_root new_anchor new_root) unit_weight /\
    (exists middle activation_steps deactivation_steps,
       measured_execution (graph env) Activation
         (Configuration (base_ledger base) [new_anchor] []) activation_steps
         (Configuration middle [] []) /\
       measured_execution (graph env) Deactivation
         (Configuration middle [old_anchor] []) deactivation_steps
         (Configuration (prepared_ledger evidence) [] []) /\
       activation_steps + deactivation_steps =
         2 + selected_sum (universe env)
               (changed_objects name old_anchor old_root new_anchor new_root)
               (expansion_credit (graph env))) /\
    (forall width entries key claim,
       StronglySorted key_before entries ->
       Forall (fun entry => length (fst entry) = width) entries ->
       length key = width -> entry_lookup entries key = None ->
       (old_root = partition_build width entries /\
        new_root = partition_build width (entries_insert key claim entries)) \/
       (old_root = partition_build width (entries_insert key claim entries) /\
        new_root = partition_build width entries) ->
       2 + expansions (prepared_usage evidence) <= 2 * width + 7).
Proof.
  intros name env budget schedule base old_anchor old_root new_anchor new_root kept evidence
    rank valid descending roots_are old_realized new_realized prepared.
  pose proof valid as (unique & closed & agreement & balance & (base_objects_exact & _) &
    added_domain & removed_domain).
  pose proof (metadata_object_is_reachability name env old_anchor old_root old_realized)
    as old_support.
  pose proof (metadata_object_is_reachability name env new_anchor new_root new_realized)
    as new_support.
  assert (base_support : forall vertex,
    active (base_ledger base) vertex = metadata_object name old_anchor old_root vertex).
  { intros vertex. apply bool_eq_of_iff.
    rewrite (@quiescent_support_is_exact_root_reachability (graph env) (universe env)
               (base_roots base) (base_ledger base) rank agreement balance descending vertex),
      old_support.
    split; apply reachable_roots_extensional; intros query; [exact (roots_are query)|].
    symmetry. exact (roots_are query). }
  assert (union_support : forall vertex,
    (metadata_object name old_anchor old_root vertex ||
     metadata_object name new_anchor new_root vertex) = true <->
    reachable (graph env) (universe env) (add_roots (base_roots base) [new_anchor]) vertex).
  { intros vertex.
    rewrite (@CreditedClosureLedger.reachable_add_roots (graph env) (universe env)
               (base_roots base) [new_anchor] vertex),
      orb_true_iff, old_support, new_support.
    split; intros [old_path|new_path];
      [left| right; exact new_path|left|right; exact new_path].
    - apply (reachable_roots_extensional (graph env) (universe env)
               (occurrences [old_anchor])); [|exact old_path].
      intros query. symmetry. exact (roots_are query).
    - apply (reachable_roots_extensional (graph env) (universe env) (base_roots base));
        [exact roots_are|exact old_path]. }
  destruct (seed_ledger_is_quiescent env (add_roots (base_roots base) [new_anchor])
              (fun vertex => metadata_object name old_anchor old_root vertex ||
                 metadata_object name new_anchor new_root vertex) union_support)
    as (union_coherent & union_quiescent).
  destruct (seed_ledger_is_quiescent env (occurrences [new_anchor])
              (metadata_object name new_anchor new_root) new_support)
    as (cold_coherent & _).
  assert (cold_balance : quiescent (graph env) (universe env)
    (remove_roots (add_roots (base_roots base) [new_anchor]) [old_anchor])
    (metadata_seed name env new_anchor new_root)).
  { intros vertex. unfold metadata_seed, seed_ledger, remove_roots, add_roots.
    cbn [counts active]. rewrite (roots_are vertex). lia. }
  pose proof (prepared_trial_agrees_with_cold_ledger env budget schedule base [new_anchor]
                [old_anchor] kept evidence rank (metadata_seed name env new_anchor new_root)
                valid descending prepared cold_coherent cold_balance)
    as (agrees & _ & objects_exact & _).
  pose proof (union_peak_is_the_add_phase_cold_inventory env budget schedule base
                [new_anchor] [old_anchor] kept evidence rank
                (seed_ledger env (add_roots (base_roots base) [new_anchor])
                   (fun vertex => metadata_object name old_anchor old_root vertex ||
                      metadata_object name new_anchor new_root vertex))
                valid descending prepared union_coherent union_quiescent)
    as (peak_exact & _).
  destruct (trial_prepared_inversion env budget schedule base [new_anchor] [old_anchor] kept
              evidence prepared) as (_ & final & core & evidence_is).
  destruct (core_prepared_inversion env Faithful budget schedule base [new_anchor]
              [old_anchor] final core)
    as (_ & _ & middle & activated & _ & deactivated & _).
  pose proof (run_phase_keeps_expansion_balance env budget schedule Activation
                (remaining_work (graph env) Activation (universe env)
                   (workspace (activation_start Faithful base [new_anchor]))) 0
                (activation_start Faithful base [new_anchor]) middle activated)
    as first_balance.
  pose proof (run_phase_keeps_expansion_balance env budget schedule Deactivation
                (remaining_work (graph env) Deactivation (universe env)
                   (workspace (deactivation_start Faithful middle [old_anchor]))) 0
                (deactivation_start Faithful middle [old_anchor]) final deactivated)
    as second_balance.
  cbn [expansion_balance activation_start deactivation_start live_objects usage expansions]
    in first_balance, second_balance.
  pose proof (activation_run_tracks_peak env budget schedule
                (remaining_work (graph env) Activation (universe env)
                   (workspace (activation_start Faithful base [new_anchor]))) 0
                (activation_start Faithful base [new_anchor]) eq_refl eq_refl)
    as middle_peaks.
  rewrite activated in middle_peaks. cbn [phase_result_state] in middle_peaks.
  destruct middle_peaks as (middle_peak_objects & middle_peak_bytes).
  assert (restart_objects :
    live_objects (deactivation_start Faithful middle [old_anchor]) <=
    peak_objects (usage (deactivation_start Faithful middle [old_anchor]))).
  { cbn [deactivation_start live_objects usage peak_objects]. lia. }
  assert (restart_bytes :
    live_bytes (deactivation_start Faithful middle [old_anchor]) <=
    peak_bytes (usage (deactivation_start Faithful middle [old_anchor]))).
  { cbn [deactivation_start live_bytes usage peak_bytes]. lia. }
  pose proof (deactivation_run_keeps_peak env budget schedule
                (remaining_work (graph env) Deactivation (universe env)
                   (workspace (deactivation_start Faithful middle [old_anchor]))) 0
                (deactivation_start Faithful middle [old_anchor]) restart_objects
                restart_bytes)
    as final_peaks.
  rewrite deactivated in final_peaks. cbn [phase_result_state] in final_peaks.
  destruct final_peaks as (final_peak_objects & _).
  cbn [deactivation_start usage peak_objects] in final_peak_objects.
  subst evidence.
  cbn [evidence_of prepared_usage prepared_objects prepared_ledger] in
    agrees, objects_exact, peak_exact |- *.
  assert (base_count : inventory (universe env) (base_ledger base) unit_weight =
    selected_sum (universe env) (metadata_object name old_anchor old_root) unit_weight).
  { unfold inventory. apply selected_sum_extensional. exact base_support. }
  pose proof (union_expansion_count (universe env) (metadata_object name old_anchor old_root)
                (metadata_object name new_anchor new_root) unit_weight) as doubled.
  unfold inventory in peak_exact, objects_exact.
  cbn [seed_ledger metadata_seed active] in peak_exact, objects_exact.
  assert (expansions_exact : expansions (usage final) =
    selected_sum (universe env)
      (changed_objects name old_anchor old_root new_anchor new_root) unit_weight).
  { unfold changed_objects. lia. }
  split; [exact agrees|].
  split; [exact objects_exact|].
  split; [exact expansions_exact|].
  split.
  - destruct (prepared_core_facts env budget schedule base [new_anchor] [old_anchor] final
                valid core)
      as (middle_run & first_execution & second_execution & allowed & _).
    pose proof (@activation_initial_configuration_is_valid (graph env) (universe env)
                  (base_roots base) (base_ledger base) [new_anchor] agreement balance
                  added_domain)
      as activation_valid.
    destruct (@drained_execution_has_exact_root_reachability (graph env) (universe env)
                (add_roots (base_roots base) [new_anchor]) Activation
                (Configuration (base_ledger base) [new_anchor] [])
                (resident (workspace middle_run)) rank unique closed descending
                activation_valid first_execution)
      as (middle_coherent & middle_quiescent & middle_reach).
    pose proof (@deactivation_initial_configuration_is_valid (graph env) (universe env)
                  (add_roots (base_roots base) [new_anchor]) (resident (workspace middle_run))
                  [old_anchor] middle_coherent middle_quiescent removed_domain allowed)
      as deactivation_valid.
    assert (middle_support : forall vertex,
      active (resident (workspace middle_run)) vertex =
        metadata_object name old_anchor old_root vertex ||
        metadata_object name new_anchor new_root vertex).
    { intros vertex. apply bool_eq_of_iff. rewrite middle_reach, union_support.
      reflexivity. }
    assert (final_support : forall vertex,
      active (resident (workspace final)) vertex =
        metadata_object name new_anchor new_root vertex).
    { intros vertex. rewrite (proj2 (agrees vertex)). reflexivity. }
    destruct (@every_execution_has_a_step_count (graph env) Activation
                (Configuration (base_ledger base) [new_anchor] [])
                (Configuration (resident (workspace middle_run)) [] []) first_execution)
      as (activation_steps & activation_measured).
    destruct (@every_execution_has_a_step_count (graph env) Deactivation
                (Configuration (resident (workspace middle_run)) [old_anchor] [])
                (Configuration (resident (workspace final)) [] []) second_execution)
      as (deactivation_steps & deactivation_measured).
    pose proof (@root_delta_execution_has_exact_changed_frontier_work (graph env)
                  (universe env) (add_roots (base_roots base) [new_anchor]) Activation
                  (base_ledger base) [new_anchor] activation_steps
                  (resident (workspace middle_run)) unique closed activation_valid
                  activation_measured)
      as activation_work.
    pose proof (@root_delta_execution_has_exact_changed_frontier_work (graph env)
                  (universe env)
                  (remove_roots (add_roots (base_roots base) [new_anchor]) [old_anchor])
                  Deactivation (resident (workspace middle_run)) [old_anchor]
                  deactivation_steps (resident (workspace final)) unique closed
                  deactivation_valid deactivation_measured)
      as deactivation_work.
    assert (activation_frontier :
      changed_expansion_work (graph env) Activation (universe env) (base_ledger base)
        (resident (workspace middle_run)) =
      selected_sum (universe env)
        (fun vertex => negb (metadata_object name old_anchor old_root vertex) &&
           (metadata_object name old_anchor old_root vertex ||
            metadata_object name new_anchor new_root vertex))
        (expansion_credit (graph env))).
    { unfold changed_expansion_work. apply selected_sum_extensional. intros vertex.
      cbn [unexpanded]. rewrite base_support, middle_support, negb_involutive.
      reflexivity. }
    assert (deactivation_frontier :
      changed_expansion_work (graph env) Deactivation (universe env)
        (resident (workspace middle_run)) (resident (workspace final)) =
      selected_sum (universe env)
        (fun vertex => (metadata_object name old_anchor old_root vertex ||
            metadata_object name new_anchor new_root vertex) &&
           negb (metadata_object name new_anchor new_root vertex))
        (expansion_credit (graph env))).
    { unfold changed_expansion_work. apply selected_sum_extensional. intros vertex.
      cbn [unexpanded]. rewrite middle_support, final_support. reflexivity. }
    pose proof (changed_frontiers_partition_the_changed_objects (universe env)
                  (metadata_object name old_anchor old_root)
                  (metadata_object name new_anchor new_root) (expansion_credit (graph env)))
      as frontiers.
    exists (resident (workspace middle_run)), activation_steps, deactivation_steps.
    split; [exact activation_measured|]. split; [exact deactivation_measured|].
    rewrite activation_work, deactivation_work, activation_frontier, deactivation_frontier.
    cbn [length]. unfold changed_objects. lia.
  - intros width entries key claim sorted lengths key_length absent pair.
    destruct (radix_edit_changes_only_the_edited_paths width entries key claim sorted lengths
                key_length absent)
      as (_ & _ & large_path & small_path & (sibling & sibling_short & added_cover) &
          removed_cover & _).
    pose proof (selected_count_is_covered (universe env)
                  (changed_objects name old_anchor old_root new_anchor new_root)
                  ([old_anchor; new_anchor] ++
                     map name (search_path (partition_build width
                                 (entries_insert key claim entries)) key ++
                               sibling ++ search_path (partition_build width entries) key))
                  unique
                  (fun vertex _ changed =>
                     changed_objects_are_on_the_edited_paths name old_anchor new_anchor
                       (partition_build width entries)
                       (partition_build width (entries_insert key claim entries))
                       sibling key vertex added_cover removed_cover old_root new_root pair
                       changed))
      as covered.
    rewrite length_app, length_map, !length_app in covered. cbn [length] in covered.
    rewrite expansions_exact. lia.
Qed.

(** Negative control: the same two phases in the opposite order.
    Deactivating the old anchor first drops every object of the old closure
    to zero, so the activation of the new anchor re-expands the whole shared
    history. *)
Definition deactivate_first_core (env : trial_environment) (budget : option trial_limits)
    (schedule : nat -> bool) (base : predecessor) (added removed : list nat) : core_result :=
  match run_phase env Faithful budget schedule Deactivation
          (remaining_work (graph env) Deactivation (universe env)
             (workspace (activation_start Faithful base removed))) 0
          (activation_start Faithful base removed) with
  | PhaseRefused why at_refusal => CoreRefused why at_refusal
  | PhaseDrained middle =>
      match run_phase env Faithful budget schedule Activation
              (remaining_work (graph env) Activation (universe env)
                 (workspace (deactivation_start Faithful middle added))) 0
              (deactivation_start Faithful middle added) with
      | PhaseRefused why at_refusal => CoreRefused why at_refusal
      | PhaseDrained final => CorePrepared final
      end
  end.

(** Capstone T6m (negative control): committing a third entry over a
    two-entry history.  The faithful trial expands the five changed objects;
    deactivating first expands both closures, nine objects, re-expanding the
    two shared history leaves twice. *)
Theorem deactivate_first_mutant_expands_the_shared_history :
  radix_edit aliased_history [(3 :: repeat 0 47, None, Some 52)] = Some committed_history /\
  realizes_metadata metadata_fixture_name metadata_fixture 9 aliased_history /\
  realizes_metadata metadata_fixture_name metadata_fixture 10 committed_history /\
  selected_sum (universe metadata_fixture)
    (changed_objects metadata_fixture_name 9 aliased_history 10 committed_history)
    unit_weight = 5 /\
  selected_sum (universe metadata_fixture)
    (metadata_object metadata_fixture_name 9 aliased_history) unit_weight +
  selected_sum (universe metadata_fixture)
    (metadata_object metadata_fixture_name 10 committed_history) unit_weight = 9 /\
  match run_trial metadata_fixture Faithful None prefer_cursors
          (seed_predecessor metadata_fixture (occurrences [9])
             (metadata_seed metadata_fixture_name metadata_fixture 9 aliased_history))
          [10] [9] with
  | TrialResult _ (TrialPrepared evidence) => expansions (prepared_usage evidence) = 5
  | TrialResult _ (TrialRefused _) => False
  end /\
  match deactivate_first_core metadata_fixture None prefer_cursors
          (seed_predecessor metadata_fixture (occurrences [9])
             (metadata_seed metadata_fixture_name metadata_fixture 9 aliased_history))
          [10] [9] with
  | CorePrepared final => expansions (usage final) = 9
  | CoreRefused _ _ => False
  end.
Proof.
  split; [vm_compute; reflexivity|].
  split; [apply realizes_metadata_by_test; vm_compute; reflexivity|].
  split; [apply realizes_metadata_by_test; vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; vm_compute; reflexivity.
Qed.

End RetentionV2Editor.

Print Assumptions RetentionV2Editor.canonical_radix_exists_and_is_unique.
Print Assumptions RetentionV2Editor.unary_inner_mutant_breaks_uniqueness.
Print Assumptions RetentionV2Editor.radix_edit_batch_is_the_canonical_rebuild.
Print Assumptions RetentionV2Editor.unpruned_empty_child_mutant_is_not_the_rebuild.
Print Assumptions RetentionV2Editor.radix_edit_changes_only_the_edited_paths.
Print Assumptions RetentionV2Editor.flat_manifest_mutant_rewrites_bytes_proportional_to_history.
Print Assumptions RetentionV2Editor.anchor_step_realizes_the_retention_entry_step.
Print Assumptions RetentionV2Editor.inserted_object_count_mutant_undercounts_an_aliased_leaf.
Print Assumptions RetentionV2Editor.canonical_radix_store_is_admitted_by_the_cold_reader.
Print Assumptions RetentionV2Editor.unmerged_root_mutant_is_refused_by_the_cold_reader.
Print Assumptions RetentionV2Editor.metadata_trial_work_is_the_changed_objects.
Print Assumptions RetentionV2Editor.deactivate_first_mutant_expands_the_shared_history.
Print Assumptions RetentionV2Editor.metadata_ledger_seed_is_the_cold_ledger.
Print Assumptions RetentionV2Editor.set_seed_mutant_drops_a_shared_radix_child.
Print Assumptions RetentionV2Editor.v1_migration_is_the_canonical_rebuild_of_its_history.
Print Assumptions RetentionV2Editor.current_in_history_migration_mutant_duplicates_the_current_entry.
