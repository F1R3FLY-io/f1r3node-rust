From Stdlib Require Import Bool List Arith Lia.
From Stdlib Require Import Sorting.Permutation Sorting.Sorted.
From StackSafePDA Require ChannelImage.
From StackSafePDA Require Import RetentionV2Format RetentionV2Editor.

Import ListNotations.

(**
  The exact current-state shape summary of a trusted successor.

  The hot policy of a trusted incremental successor (engineering journal
  J-067) checks state-shape ceilings on logical nodes, endpoints, logical path
  bytes and payload bytes without walking the successor state.  This theory
  derives each of these quantities exactly from the predecessor's summary, the
  CAS batch and the rewrite's counters, and shows that the derived summary is
  the cold semantic walk of the successor.

  - The cold walk ([walk_trie]) visits a node once per occurrence, counts one
    logical node per visit, observes the path length at every visit and counts
    one endpoint per present claim; [logical_nodes], [path_maximum] and
    [tree_entries] are those measures.
  - An admitted trie has no empty leaf, so the largest path length the walk
    observes is the length of the longest key; over the three namespaces it is
    the maximum of all key lengths.
  - A successful CAS batch satisfies one permutation identity on entries, so
    every additive weight of the entries (endpoint existence, the entry count
    of one namespace, payload bytes) moves by exactly the weights of the
    batch's after-images minus those of its before-images.  The payload of a
    content claim is its encoded length minus the 64-byte blob envelope.
  - A multiset of key lengths, with checked removal of each before-image
    length, tracks the batch exactly; its largest nonempty bucket is the
    longest key.  A running maximum kept without the multiset keeps a deleted
    longest key.
  - The counted rewrite refines the editor's [radix_edit] on admitted roots
    and moves logical positions by the identity
    final + loaded = root + placed, where loaded counts the root load, each
    expanded reference and each merge load, and placed counts each staged
    node.  An emitted node is placed or absorbed by a compression merge, so a
    delta of emitted minus loaded nodes miscounts a merge, and a count of the
    distinct inserted objects undercounts equal leaves.

  The rewrite's entry denotation, that the successor root lists the entries
  of the CAS batch applied to the predecessor's entries, is an explicit
  argument of the composed theorem; the Rust differential tests compare the
  rewrite with an independent full rebuild.  The four variants named mutant
  are negative controls, each refuted by a closed counterexample theorem.
*)

Module ExactStateShapeSummary.

Import RetentionV2Format.
Import RetentionV2Editor.

(** * The cold walk's measures *)

(** [walk_trie] pops one visit per node occurrence, so a subtree reached from
    two parents is visited twice.  A visit counts one logical node, observes
    the path length (the parent path, the edge byte and the node prefix) and
    counts one endpoint when the node carries a claim. *)
Fixpoint children_sum (measure : radix_tree -> nat) (children : list (nat * radix_tree))
    : nat :=
  match children with
  | [] => 0
  | (_, child) :: rest => measure child + children_sum measure rest
  end.

Fixpoint logical_nodes (tree : radix_tree) : nat :=
  match tree with
  | RadixTree _ _ children => S (children_sum logical_nodes children)
  end.

Fixpoint children_maximum (measure : radix_tree -> nat)
    (children : list (nat * radix_tree)) : nat :=
  match children with
  | [] => 0
  | (_, child) :: rest => Nat.max (S (measure child)) (children_maximum measure rest)
  end.

(** The largest path length observed at or below a node, measured from the
    node's entry point: the node prefix, then for each child its edge byte and
    the child's own maximum. *)
Fixpoint path_maximum (tree : radix_tree) : nat :=
  match tree with
  | RadixTree prefix _ children => length prefix + children_maximum path_maximum children
  end.

Definition key_lengths (entries : list (list nat * nat)) : list nat :=
  map (fun entry => length (fst entry)) entries.

Definition longest_key (entries : list (list nat * nat)) : nat :=
  list_max (key_lengths entries).

(** * The cold reader's shape rules *)

(** A stored node has strictly increasing edges, a node without a claim has at
    least two children ([EmptyNode] and [CompressibleUnaryNode] are refused),
    and every child is a stored node.  The root is a stored node or the empty
    node. *)
Fixpoint admitted_node (tree : radix_tree) : Prop :=
  match tree with
  | RadixTree _ endpoint children =>
      edges_sorted children /\ (endpoint = None -> 2 <= length children) /\
      all_children admitted_node children
  end.

Definition admitted_root (tree : radix_tree) : Prop := tree = empty_tree \/ admitted_node tree.

Fixpoint no_empty_leaf (tree : radix_tree) : Prop :=
  match tree with
  | RadixTree _ endpoint children =>
      match children with
      | [] => endpoint <> None
      | _ :: _ => all_children no_empty_leaf children
      end
  end.

(** Capstone: every leaf of an admitted trie carries a claim. *)
Theorem admitted_tries_have_no_empty_leaf :
  forall tree, admitted_node tree -> no_empty_leaf tree.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  intros shaped. cbn [admitted_node] in shaped.
  destruct shaped as (_ & branching & child_shapes).
  rewrite Forall_forall in IH. rewrite all_children_iff in child_shapes.
  destruct children as [|[edge child] rest]; cbn [no_empty_leaf].
  - intros absent. specialize (branching absent). cbn [length] in branching. lia.
  - apply all_children_iff. intros other target member.
    exact (IH (other, target) member (child_shapes other target member)).
Qed.

Lemma list_max_cons :
  forall value rest, list_max (value :: rest) = Nat.max value (list_max rest).
Proof. reflexivity. Qed.

Lemma list_max_map_add :
  forall offset values, values <> [] ->
    list_max (map (fun value => offset + value) values) = offset + list_max values.
Proof.
  intros offset values nonempty.
  induction values as [|value rest IH]; [contradiction|].
  cbn [map]. rewrite !list_max_cons.
  destruct rest as [|next later].
  - cbn [map list_max fold_right]. lia.
  - rewrite IH by discriminate. lia.
Qed.

Lemma key_lengths_app :
  forall left right, key_lengths (left ++ right) = key_lengths left ++ key_lengths right.
Proof. intros left right. unfold key_lengths. apply map_app. Qed.

Lemma key_lengths_prepend :
  forall bytes entries,
    key_lengths (map (prepend_entry bytes) entries) =
      map (fun value => length bytes + value) (key_lengths entries).
Proof.
  intros bytes entries. unfold key_lengths. rewrite !map_map.
  apply map_ext. intros [key claim]. unfold prepend_entry. cbn [fst]. apply length_app.
Qed.

Lemma no_empty_leaf_lists_an_entry :
  forall tree, no_empty_leaf tree -> tree_entries tree <> [].
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  intros shaped. destruct children as [|[edge child] rest].
  - cbn [no_empty_leaf] in shaped. destruct endpoint as [claim|].
    + cbn [tree_entries endpoint_entries app]. discriminate.
    + exfalso. apply shaped. reflexivity.
  - cbn [no_empty_leaf all_children] in shaped. destruct shaped as [child_shaped _].
    pose proof (Forall_inv IH) as child_IH. cbn [snd] in child_IH.
    pose proof (child_IH child_shaped) as child_nonempty.
    cbn [tree_entries children_entries]. intros empty.
    apply app_eq_nil in empty as [_ empty].
    apply map_eq_nil in empty. apply app_eq_nil in empty as [empty _].
    apply map_eq_nil in empty. exact (child_nonempty empty).
Qed.

Lemma children_longest_key :
  forall children,
    (forall edge child, In (edge, child) children ->
       path_maximum child = longest_key (tree_entries child) /\ tree_entries child <> []) ->
    children_maximum path_maximum children = longest_key (children_entries tree_entries children).
Proof.
  induction children as [|[edge child] rest IH]; intros each; [reflexivity|].
  cbn [children_maximum children_entries].
  destruct (each edge child (or_introl eq_refl)) as [child_maximum child_nonempty].
  assert (rest_maximum : children_maximum path_maximum rest =
    longest_key (children_entries tree_entries rest)).
  { apply IH. intros other target member. exact (each other target (or_intror member)). }
  unfold longest_key in *.
  rewrite key_lengths_app, list_max_app, key_lengths_prepend.
  rewrite list_max_map_add.
  2: { unfold key_lengths. intros empty. apply map_eq_nil in empty. exact (child_nonempty empty). }
  rewrite <- child_maximum, <- rest_maximum. cbn [length]. lia.
Qed.

Lemma admitted_node_longest_key :
  forall tree, admitted_node tree ->
    path_maximum tree = longest_key (tree_entries tree) /\ tree_entries tree <> [].
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  intros shaped.
  split; [|exact (no_empty_leaf_lists_an_entry _ (admitted_tries_have_no_empty_leaf _ shaped))].
  cbn [admitted_node] in shaped. destruct shaped as (_ & branching & child_shapes).
  rewrite all_children_iff in child_shapes. rewrite Forall_forall in IH.
  assert (children_facts : forall edge child, In (edge, child) children ->
    path_maximum child = longest_key (tree_entries child) /\ tree_entries child <> []).
  { intros edge child member. exact (IH (edge, child) member (child_shapes edge child member)). }
  pose proof (children_longest_key children children_facts) as children_max.
  cbn [path_maximum tree_entries]. unfold longest_key in *.
  rewrite key_lengths_app, list_max_app, key_lengths_prepend.
  destruct children as [|[edge child] rest].
  - destruct endpoint as [claim|].
    + cbn [children_maximum children_entries endpoint_entries key_lengths map list_max
        fold_right fst].
      lia.
    + specialize (branching eq_refl). cbn [length] in branching. lia.
  - rewrite list_max_map_add.
    2: { unfold key_lengths. intros empty. apply map_eq_nil in empty.
         cbn [children_entries] in empty. apply app_eq_nil in empty as [empty _].
         apply map_eq_nil in empty.
         exact (proj2 (children_facts edge child (or_introl eq_refl)) empty). }
    rewrite <- children_max.
    destruct endpoint as [claim|]; cbn [endpoint_entries key_lengths map list_max fold_right fst];
      lia.
Qed.

(** Capstone: the largest logical path the cold walk observes in an admitted
    root is the length of its longest key (zero for the empty root). *)
Theorem maximum_logical_path_is_the_longest_key :
  forall tree, admitted_root tree -> path_maximum tree = longest_key (tree_entries tree).
Proof.
  intros tree [empty|shaped].
  - subst tree. reflexivity.
  - exact (proj1 (admitted_node_longest_key tree shaped)).
Qed.

Lemma admitted_entries_sorted :
  forall tree, admitted_node tree -> StronglySorted key_before (tree_entries tree).
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  rewrite Forall_forall in IH.
  intros shaped. cbn [admitted_node] in shaped.
  destruct shaped as (sorted & _ & child_shapes).
  rewrite all_children_iff in child_shapes.
  cbn [tree_entries].
  apply strongly_sorted_app.
  - destruct endpoint; repeat constructor.
  - apply (strongly_sorted_map key_before key_before (prepend_entry prefix));
      [intros x y; apply prepend_preserves_key_order|].
    apply children_entries_sorted; [exact sorted|].
    intros edge child member. exact (IH (edge, child) member (child_shapes edge child member)).
  - intros x y x_member y_member.
    destruct endpoint as [claim|]; [|destruct x_member].
    destruct x_member as [x_is|[]]. subst x.
    apply in_map_iff in y_member as [inner [y_is inner_member]]. subst y.
    apply children_entries_in in inner_member
      as [edge [child [deeper [_ [inner_is _]]]]]. subst inner.
    unfold prepend_entry. cbn [fst snd app].
    apply prefix_entry_before_extension.
Qed.

Lemma admitted_root_entries_sorted :
  forall tree, admitted_root tree -> StronglySorted key_before (tree_entries tree).
Proof.
  intros tree [empty|shaped].
  - subst tree. apply SSorted_nil.
  - exact (admitted_entries_sorted tree shaped).
Qed.

(** * Entry deltas of a CAS batch *)

Lemma entries_insert_permutation :
  forall key claim entries,
    Permutation (entries_insert key claim entries) ((key, claim) :: entries).
Proof.
  intros key claim entries.
  induction entries as [|[candidate value] rest IH]; cbn [entries_insert];
    [apply Permutation_refl|].
  destruct (ChannelImage.bytes_ltb key candidate); [apply Permutation_refl|].
  eapply perm_trans; [apply perm_skip; exact IH|]. apply perm_swap.
Qed.

Lemma remove_restores_entries :
  forall key entries,
    StronglySorted key_before entries ->
    Permutation entries
      (endpoint_entries key (entry_lookup entries key) ++ entries_remove key entries).
Proof.
  intros key entries sorted.
  induction entries as [|[candidate value] rest IH]; [apply Permutation_refl|].
  apply StronglySorted_inv in sorted as [rest_sorted head_below].
  unfold entries_remove. cbn [entry_lookup filter fst].
  destruct (bytes_eqb candidate key) eqn:same; cbn [negb].
  - apply bytes_eqb_true_iff in same. subst candidate.
    cbn [endpoint_entries app].
    rewrite (filter_keeps_all _ rest); [apply Permutation_refl|].
    intros [other other_value] member. cbv beta. cbn [fst].
    rewrite Forall_forall in head_below.
    pose proof (head_below (other, other_value) member) as below.
    unfold key_before in below. cbn [fst] in below.
    rewrite bytes_eqb_sym, (bytes_ltb_eqb_false key other below). reflexivity.
  - fold (entries_remove key rest).
    eapply perm_trans; [apply perm_skip; exact (IH rest_sorted)|].
    apply Permutation_middle.
Qed.

(** One CAS step: the updated entries plus the before-image are a permutation
    of the old entries plus the after-image. *)
Lemma entries_update_permutation :
  forall entries key before after,
    StronglySorted key_before entries ->
    entry_lookup entries key = before ->
    Permutation (entries_update entries key after ++ endpoint_entries key before)
      (entries ++ endpoint_entries key after).
Proof.
  intros entries key before after sorted found.
  pose proof (remove_restores_entries key entries sorted) as restored.
  rewrite found in restored.
  destruct after as [claim|]; unfold entries_update; cbn [endpoint_entries].
  - eapply perm_trans.
    { apply Permutation_app_tail. apply entries_insert_permutation. }
    cbn [app].
    eapply perm_trans; [|apply Permutation_cons_append].
    apply perm_skip.
    eapply perm_trans; [apply Permutation_app_comm|].
    apply Permutation_sym. exact restored.
  - rewrite app_nil_r.
    eapply perm_trans; [apply Permutation_app_comm|].
    apply Permutation_sym. exact restored.
Qed.

Fixpoint removed_entries (batch : list cas_mutation) : list (list nat * nat) :=
  match batch with
  | [] => []
  | (key, before, _) :: rest => endpoint_entries key before ++ removed_entries rest
  end.

Fixpoint added_entries (batch : list cas_mutation) : list (list nat * nat) :=
  match batch with
  | [] => []
  | (key, _, after) :: rest => endpoint_entries key after ++ added_entries rest
  end.

Lemma batch_permutation :
  forall batch entries final,
    StronglySorted key_before entries ->
    entries_batch entries batch = Some final ->
    Permutation (final ++ removed_entries batch) (entries ++ added_entries batch).
Proof.
  intros batch.
  induction batch as [|[[key before] after] rest IH]; intros entries final sorted batched.
  - cbn [entries_batch] in batched. injection batched as final_is. subst final.
    apply Permutation_refl.
  - cbn [entries_batch entries_cas] in batched.
    destruct (endpoint_eqb (entry_lookup entries key) before) eqn:matches;
      [|discriminate batched].
    apply endpoint_eqb_true_iff in matches.
    pose proof (entries_update_strongly_sorted entries key after sorted) as next_sorted.
    pose proof (IH _ _ next_sorted batched) as later.
    pose proof (entries_update_permutation entries key before after sorted matches) as step.
    cbn [removed_entries added_entries].
    eapply perm_trans.
    { apply Permutation_app_head. apply Permutation_app_comm. }
    rewrite app_assoc.
    eapply perm_trans; [apply Permutation_app_tail; exact later|].
    rewrite <- app_assoc.
    eapply perm_trans; [apply Permutation_app_head; apply Permutation_app_comm|].
    rewrite app_assoc.
    eapply perm_trans; [apply Permutation_app_tail; exact step|].
    rewrite <- app_assoc. apply Permutation_refl.
Qed.

Fixpoint entry_weight (weight : list nat * nat -> nat) (entries : list (list nat * nat))
    : nat :=
  match entries with
  | [] => 0
  | entry :: rest => weight entry + entry_weight weight rest
  end.

Lemma entry_weight_app :
  forall weight left right,
    entry_weight weight (left ++ right) = entry_weight weight left + entry_weight weight right.
Proof.
  intros weight left right.
  induction left as [|entry rest IH]; cbn [app entry_weight]; [reflexivity|].
  rewrite IH. lia.
Qed.

Lemma entry_weight_permutation :
  forall weight left right,
    Permutation left right -> entry_weight weight left = entry_weight weight right.
Proof.
  intros weight left right same.
  induction same as [|entry first second _ IH|first second rest|first middle last _ IH1 _ IH2];
    cbn [entry_weight]; lia.
Qed.

Lemma entry_weight_ext_in :
  forall left right entries,
    (forall entry, In entry entries -> left entry = right entry) ->
    entry_weight left entries = entry_weight right entries.
Proof.
  intros left right entries same.
  induction entries as [|entry rest IH]; cbn [entry_weight]; [reflexivity|].
  rewrite (same entry (in_eq _ _)), IH; [reflexivity|].
  intros other member. exact (same other (in_cons _ _ _ member)).
Qed.

(** The checked delta of the Rust summary: add, then subtract with refusal of
    an underflow. *)
Definition checked_delta (value added removed : nat) : option nat :=
  if removed <=? value + added then Some (value + added - removed) else None.

Lemma checked_delta_exact :
  forall value added removed final,
    final + removed = value + added -> checked_delta value added removed = Some final.
Proof.
  intros value added removed final balance. unfold checked_delta.
  destruct (Nat.leb_spec removed (value + added)) as [fits|overflows]; [|lia].
  f_equal. lia.
Qed.

(** Capstone: every additive weight of the entries moves by exactly the weight
    of the batch's after-images minus that of its before-images. *)
Theorem batch_weight_delta_is_exact :
  forall weight batch entries final,
    StronglySorted key_before entries ->
    entries_batch entries batch = Some final ->
    checked_delta (entry_weight weight entries) (entry_weight weight (added_entries batch))
      (entry_weight weight (removed_entries batch)) = Some (entry_weight weight final).
Proof.
  intros weight batch entries final sorted batched.
  apply checked_delta_exact.
  pose proof (entry_weight_permutation weight _ _
    (batch_permutation batch entries final sorted batched)) as same.
  rewrite !entry_weight_app in same. exact same.
Qed.

(** Capstone: the endpoint count moves by the number of present after-images
    minus the number of present before-images, which is the editor caller's
    per-step count rule. *)
Theorem endpoint_existence_delta_is_exact :
  forall batch entries final,
    StronglySorted key_before entries ->
    entries_batch entries batch = Some final ->
    checked_delta (length entries) (length (added_entries batch))
      (length (removed_entries batch)) = Some (length final) /\
    cas_count (length entries) batch = Some (length final).
Proof.
  intros batch entries final sorted batched. split.
  - apply checked_delta_exact.
    pose proof (Permutation_length (batch_permutation batch entries final sorted batched))
      as same.
    rewrite !length_app in same. exact same.
  - exact (cas_count_tracks_entries batch entries final sorted batched).
Qed.

(** A canonical blob is the 32-byte envelope header (domain, format version,
    body length) followed by its body: the 16-byte blob kind, the payload
    version and the payload length (8 bytes each), and the payload. *)
Definition canonical_blob_envelope_len : nat := 64.

Definition canonical_blob_len (payload : nat) : nat :=
  object_envelope_header_len + (16 + 8 + 8 + payload).

Lemma canonical_blob_payload :
  forall payload, canonical_blob_len payload - canonical_blob_envelope_len = payload.
Proof.
  intros payload. unfold canonical_blob_len, canonical_blob_envelope_len,
    object_envelope_header_len. lia.
Qed.

Definition reference_payload (encoded_len : nat -> nat) (claim : nat) : nat :=
  encoded_len claim - canonical_blob_envelope_len.

(** Every claim the batch names is a reference whose exact encoded length is
    the canonical blob length of its payload, which the cold reader enforces
    ([load_reference] compares the length, and the blob parser refuses
    trailing bytes). *)
Definition batch_claims_are_blobs (encoded_len payload : nat -> nat)
    (batch : list cas_mutation) : Prop :=
  forall key before after claim,
    In (key, before, after) batch -> before = Some claim \/ after = Some claim ->
    encoded_len claim = canonical_blob_len (payload claim).

Lemma removed_entries_in :
  forall batch entry, In entry (removed_entries batch) ->
    exists before after, In (fst entry, before, after) batch /\ before = Some (snd entry).
Proof.
  intros batch entry.
  induction batch as [|[[key before] after] rest IH]; cbn [removed_entries]; [intros []|].
  intros member. apply in_app_or in member as [here|later].
  - destruct before as [claim|]; [|destruct here].
    destruct here as [entry_is|[]]. subst entry. cbn [fst snd].
    exists (Some claim), after. split; [left; reflexivity|reflexivity].
  - destruct (IH later) as [before' [after' [mutation claim_is]]].
    exists before', after'. split; [right; exact mutation|exact claim_is].
Qed.

Lemma added_entries_in :
  forall batch entry, In entry (added_entries batch) ->
    exists before after, In (fst entry, before, after) batch /\ after = Some (snd entry).
Proof.
  intros batch entry.
  induction batch as [|[[key before] after] rest IH]; cbn [added_entries]; [intros []|].
  intros member. apply in_app_or in member as [here|later].
  - destruct after as [claim|]; [|destruct here].
    destruct here as [entry_is|[]]. subst entry. cbn [fst snd].
    exists before, (Some claim). split; [left; reflexivity|reflexivity].
  - destruct (IH later) as [before' [after' [mutation claim_is]]].
    exists before', after'. split; [right; exact mutation|exact claim_is].
Qed.

(** Capstone: the payload bytes move by the reference payloads of the batch's
    after-images minus those of its before-images, each the encoded length
    minus the blob envelope. *)
Theorem payload_delta_is_exact :
  forall (encoded_len payload : nat -> nat) batch entries final,
    StronglySorted key_before entries ->
    entries_batch entries batch = Some final ->
    batch_claims_are_blobs encoded_len payload batch ->
    checked_delta (entry_weight (fun entry => payload (snd entry)) entries)
      (entry_weight (fun entry => reference_payload encoded_len (snd entry))
         (added_entries batch))
      (entry_weight (fun entry => reference_payload encoded_len (snd entry))
         (removed_entries batch)) =
      Some (entry_weight (fun entry => payload (snd entry)) final).
Proof.
  intros encoded_len payload batch entries final sorted batched blobs.
  rewrite (entry_weight_ext_in _ (fun entry => payload (snd entry)) (added_entries batch)).
  2: { intros entry member.
       destruct (added_entries_in batch entry member) as [before [after [mutation claim_is]]].
       unfold reference_payload.
       rewrite (blobs (fst entry) before after (snd entry) mutation (or_intror claim_is)).
       apply canonical_blob_payload. }
  rewrite (entry_weight_ext_in _ (fun entry => payload (snd entry)) (removed_entries batch)).
  2: { intros entry member.
       destruct (removed_entries_in batch entry member) as [before [after [mutation claim_is]]].
       unfold reference_payload.
       rewrite (blobs (fst entry) before after (snd entry) mutation (or_introl claim_is)).
       apply canonical_blob_payload. }
  exact (batch_weight_delta_is_exact _ batch entries final sorted batched).
Qed.

(** * The key-length multiset *)

Definition occurrences (lengths : list nat) (value : nat) : nat :=
  count_occ Nat.eq_dec lengths value.

Lemma occurrences_app :
  forall left right value,
    occurrences (left ++ right) value = occurrences left value + occurrences right value.
Proof. intros left right value. apply count_occ_app. Qed.

Lemma permutation_occurrences :
  forall left right,
    Permutation left right <-> (forall value, occurrences left value = occurrences right value).
Proof. intros left right. apply Permutation_count_occ. Qed.

(** Checked removal of one occurrence; an absent length is refused. *)
Fixpoint remove_length (target : nat) (lengths : list nat) : option (list nat) :=
  match lengths with
  | [] => None
  | head :: rest =>
      if Nat.eqb head target then Some rest
      else
        match remove_length target rest with
        | Some later => Some (head :: later)
        | None => None
        end
  end.

Lemma remove_length_member :
  forall target lengths, In target lengths ->
    exists rest, remove_length target lengths = Some rest /\
      Permutation lengths (target :: rest).
Proof.
  intros target lengths. induction lengths as [|head tail IH]; intros member; [destruct member|].
  cbn [remove_length].
  destruct (Nat.eqb_spec head target) as [same|different].
  - subst head. exists tail. split; [reflexivity|apply Permutation_refl].
  - destruct member as [same|later]; [contradiction|].
    destruct (IH later) as [rest [removed restored]].
    rewrite removed. exists (head :: rest). split; [reflexivity|].
    eapply perm_trans; [apply perm_skip; exact restored|]. apply perm_swap.
Qed.

Definition endpoint_lengths (key : list nat) (endpoint : option nat) : list nat :=
  key_lengths (endpoint_entries key endpoint).

(** One CAS step on the multiset: a present before-image removes its key
    length (refused when absent) and a present after-image adds it. *)
Definition apply_length_mutation (lengths : list nat) (mutation : cas_mutation)
    : option (list nat) :=
  match mutation with
  | (key, before, after) =>
      match before with
      | Some _ =>
          match remove_length (length key) lengths with
          | Some rest => Some (endpoint_lengths key after ++ rest)
          | None => None
          end
      | None => Some (endpoint_lengths key after ++ lengths)
      end
  end.

Fixpoint apply_lengths (lengths : list nat) (batch : list cas_mutation) : option (list nat) :=
  match batch with
  | [] => Some lengths
  | mutation :: rest =>
      match apply_length_mutation lengths mutation with
      | Some next => apply_lengths next rest
      | None => None
      end
  end.

Lemma entry_lookup_member :
  forall entries key claim, entry_lookup entries key = Some claim -> In (key, claim) entries.
Proof.
  intros entries key claim.
  induction entries as [|[candidate value] rest IH]; cbn [entry_lookup]; [discriminate|].
  destruct (bytes_eqb candidate key) eqn:same.
  - intros found. injection found as value_is. subst value.
    apply bytes_eqb_true_iff in same. subst candidate. left. reflexivity.
  - intros found. right. exact (IH found).
Qed.

Lemma key_lengths_present :
  forall key claim, key_lengths (endpoint_entries key (Some claim)) = [length key].
Proof. reflexivity. Qed.

Lemma key_lengths_absent : forall key, key_lengths (endpoint_entries key None) = [].
Proof. reflexivity. Qed.

(** [others] are the key lengths of the namespaces the batch does not touch:
    the Rust summary keeps one multiset for all three namespaces. *)
Lemma length_step_tracks_entries :
  forall entries lengths others key before after,
    StronglySorted key_before entries ->
    entry_lookup entries key = before ->
    Permutation lengths (key_lengths entries ++ others) ->
    exists next,
      apply_length_mutation lengths (key, before, after) = Some next /\
      Permutation next (key_lengths (entries_update entries key after) ++ others).
Proof.
  intros entries lengths others key before after sorted found tracked.
  pose proof (Permutation_map (fun entry => length (fst entry))
    (entries_update_permutation entries key before after sorted found)) as step.
  change (Permutation
    (key_lengths (entries_update entries key after ++ endpoint_entries key before))
    (key_lengths (entries ++ endpoint_entries key after))) in step.
  rewrite !key_lengths_app in step.
  rewrite permutation_occurrences in step, tracked.
  destruct before as [claim|].
  - rewrite key_lengths_present in step.
    assert (listed : In (length key) (key_lengths entries)).
    { unfold key_lengths.
      exact (in_map (fun entry => length (fst entry)) entries (key, claim)
        (entry_lookup_member entries key claim found)). }
    assert (present : In (length key) lengths).
    { apply (count_occ_In Nat.eq_dec). apply (count_occ_In Nat.eq_dec) in listed.
      fold (occurrences lengths (length key)).
      fold (occurrences (key_lengths entries) (length key)) in listed.
      rewrite tracked, occurrences_app. lia. }
    destruct (remove_length_member (length key) lengths present) as [rest [removed restored]].
    exists (endpoint_lengths key after ++ rest). split.
    + cbn [apply_length_mutation]. rewrite removed. reflexivity.
    + apply permutation_occurrences. intros value.
      rewrite permutation_occurrences in restored.
      specialize (step value). specialize (tracked value). specialize (restored value).
      change (length key :: rest) with ([length key] ++ rest) in restored.
      unfold endpoint_lengths. rewrite !occurrences_app in *. lia.
  - rewrite key_lengths_absent, app_nil_r in step.
    exists (endpoint_lengths key after ++ lengths). split; [reflexivity|].
    apply permutation_occurrences. intros value.
    specialize (step value). specialize (tracked value).
    unfold endpoint_lengths. rewrite !occurrences_app in *. lia.
Qed.

(** Capstone: the multiset of key lengths, updated per CAS with checked
    removal, stays the multiset of the successor's key lengths. *)
Theorem key_length_multiset_tracks_the_batch :
  forall batch entries final lengths others,
    StronglySorted key_before entries ->
    entries_batch entries batch = Some final ->
    Permutation lengths (key_lengths entries ++ others) ->
    exists updated,
      apply_lengths lengths batch = Some updated /\
      Permutation updated (key_lengths final ++ others).
Proof.
  intros batch.
  induction batch as [|[[key before] after] rest IH];
    intros entries final lengths others sorted batched tracked.
  - cbn [entries_batch] in batched. injection batched as final_is. subst final.
    exists lengths. split; [reflexivity|exact tracked].
  - cbn [entries_batch entries_cas] in batched.
    destruct (endpoint_eqb (entry_lookup entries key) before) eqn:matches;
      [|discriminate batched].
    apply endpoint_eqb_true_iff in matches.
    destruct (length_step_tracks_entries entries lengths others key before after sorted matches
      tracked) as [next [stepped next_tracked]].
    destruct (IH (entries_update entries key after) final next others
      (entries_update_strongly_sorted entries key after sorted) batched next_tracked)
      as [updated [applied updated_tracked]].
    exists updated. split; [|exact updated_tracked].
    cbn [apply_lengths]. rewrite stepped. exact applied.
Qed.

Lemma list_max_member : forall lengths, lengths <> [] -> In (list_max lengths) lengths.
Proof.
  induction lengths as [|head rest IH]; intros nonempty; [contradiction|].
  rewrite list_max_cons.
  destruct rest as [|next later].
  - left. cbn [list_max fold_right]. lia.
  - destruct (Nat.le_ge_cases head (list_max (next :: later))) as [below|above].
    + rewrite (Nat.max_r _ _ below). right. apply IH. discriminate.
    + rewrite (Nat.max_l _ _ above). left. reflexivity.
Qed.

Lemma member_le_list_max : forall lengths value, In value lengths -> value <= list_max lengths.
Proof.
  intros lengths value member.
  pose proof (proj1 (list_max_le lengths (list_max lengths)) (le_n _)) as bounded.
  rewrite Forall_forall in bounded. exact (bounded value member).
Qed.

Lemma list_max_permutation :
  forall left right, Permutation left right -> list_max left = list_max right.
Proof.
  intros left right same.
  induction same as [|value first second _ IH|first second rest|first middle last _ IH1 _ IH2].
  - reflexivity.
  - rewrite !list_max_cons, IH. reflexivity.
  - rewrite !list_max_cons. lia.
  - rewrite IH1, IH2. reflexivity.
Qed.

(** Capstone: every nonempty bucket of the multiset is at most its maximum,
    the maximum's bucket is nonempty, and the maximum is the larger of the
    namespace's longest key and the other namespaces' maximum. *)
Theorem largest_nonempty_bucket_is_the_longest_key :
  forall lengths entries others,
    Permutation lengths (key_lengths entries ++ others) ->
    (forall bucket, 0 < occurrences lengths bucket -> bucket <= list_max lengths) /\
    (lengths <> [] -> 0 < occurrences lengths (list_max lengths)) /\
    list_max lengths = Nat.max (longest_key entries) (list_max others).
Proof.
  intros lengths entries others tracked. split; [|split].
  - intros bucket nonempty. apply member_le_list_max.
    apply (count_occ_In Nat.eq_dec). exact nonempty.
  - intros nonempty. apply (count_occ_In Nat.eq_dec). exact (list_max_member lengths nonempty).
  - rewrite (list_max_permutation _ _ tracked), list_max_app. reflexivity.
Qed.

(** * The counted rewrite *)

(** An edited trie during a rewrite: [StoredTree] is a child still held by
    reference, [EditedNode] a node the editor holds in memory.  Opening a
    stored node loads it once; its children stay references. *)
Inductive edit_tree : Type :=
| StoredTree (stored : radix_tree)
| EditedNode (prefix : list nat) (endpoint : option nat) (children : list (nat * edit_tree)).

Section EditTreeInduction.
Variable holds : edit_tree -> Prop.
Hypothesis stored_case : forall stored, holds (StoredTree stored).
Hypothesis edited_case :
  forall prefix endpoint children,
    Forall (fun child => holds (snd child)) children ->
    holds (EditedNode prefix endpoint children).

Fixpoint edit_tree_induction (tree : edit_tree) : holds tree :=
  match tree with
  | StoredTree stored => stored_case stored
  | EditedNode prefix endpoint children =>
      edited_case prefix endpoint children
        ((fix slots_induction (rest : list (nat * edit_tree))
            : Forall (fun child => holds (snd child)) rest :=
            match rest with
            | [] => Forall_nil _
            | (edge, child) :: later =>
                @Forall_cons _ (fun child => holds (snd child)) (edge, child) later
                  (edit_tree_induction child) (slots_induction later)
            end) children)
  end.
End EditTreeInduction.

Lemma forall_slots_member :
  forall (holds : edit_tree -> Prop) (children : list (nat * edit_tree)) edge child,
    Forall (fun child => holds (snd child)) children -> In (edge, child) children -> holds child.
Proof.
  intros holds children edge child every member.
  rewrite Forall_forall in every. exact (every (edge, child) member).
Qed.

Fixpoint reference_children (children : list (nat * radix_tree)) : list (nat * edit_tree) :=
  match children with
  | [] => []
  | (edge, child) :: rest => (edge, StoredTree child) :: reference_children rest
  end.

Fixpoint image_children (image : edit_tree -> radix_tree) (children : list (nat * edit_tree))
    : list (nat * radix_tree) :=
  match children with
  | [] => []
  | (edge, child) :: rest => (edge, image child) :: image_children image rest
  end.

Fixpoint edit_image (tree : edit_tree) : radix_tree :=
  match tree with
  | StoredTree stored => stored
  | EditedNode prefix endpoint children =>
      RadixTree prefix endpoint (image_children edit_image children)
  end.

Fixpoint slot_sum (measure : edit_tree -> nat) (children : list (nat * edit_tree)) : nat :=
  match children with
  | [] => 0
  | (_, child) :: rest => measure child + slot_sum measure rest
  end.

(** The logical positions still held by reference. *)
Fixpoint stored_positions (tree : edit_tree) : nat :=
  match tree with
  | StoredTree stored => logical_nodes stored
  | EditedNode _ _ children => slot_sum stored_positions children
  end.

Definition fresh_edit_leaf (rest : list nat) (before after : option nat) : option edit_tree :=
  match before with
  | None => Some (EditedNode rest after [])
  | Some _ => None
  end.

(** The editor's slot search over an edited node's children, with the loads
    of the descent. *)
Fixpoint edit_slots (edit : edit_tree -> option (edit_tree * nat)) (fresh : option edit_tree)
    (edge : nat) (children : list (nat * edit_tree))
    : option (list (nat * edit_tree) * nat) :=
  match children with
  | [] =>
      match fresh with
      | Some leaf => Some ([(edge, leaf)], 0)
      | None => None
      end
  | (other, child) :: rest =>
      if edge <? other then
        match fresh with
        | Some leaf => Some ((edge, leaf) :: (other, child) :: rest, 0)
        | None => None
        end
      else if Nat.eqb edge other then
        match edit child with
        | Some (edited, loads) => Some ((other, edited) :: rest, loads)
        | None => None
        end
      else
        match edit_slots edit fresh edge rest with
        | Some (later, loads) => Some ((other, child) :: later, loads)
        | None => None
        end
  end.

(** The same search over a just-opened stored node, whose children are
    references. *)
Fixpoint stored_slots (edit : radix_tree -> option (edit_tree * nat)) (fresh : option edit_tree)
    (edge : nat) (children : list (nat * radix_tree))
    : option (list (nat * edit_tree) * nat) :=
  match children with
  | [] =>
      match fresh with
      | Some leaf => Some ([(edge, leaf)], 0)
      | None => None
      end
  | (other, child) :: rest =>
      if edge <? other then
        match fresh with
        | Some leaf => Some ((edge, leaf) :: reference_children ((other, child) :: rest), 0)
        | None => None
        end
      else if Nat.eqb edge other then
        match edit child with
        | Some (edited, loads) => Some ((other, edited) :: reference_children rest, loads)
        | None => None
        end
      else
        match stored_slots edit fresh edge rest with
        | Some (later, loads) => Some ((other, StoredTree child) :: later, loads)
        | None => None
        end
  end.

Definition split_slots (shared : list nat) (old_edge : nat) (old_node : edit_tree)
    (rest : list nat) (before after : option nat) : option (edit_tree * nat) :=
  match rest with
  | [] =>
      if endpoint_eqb None before
      then Some (EditedNode shared after [(old_edge, old_node)], 0)
      else None
  | edge :: tail =>
      match edit_slots (fun _ => None) (fresh_edit_leaf tail before after) edge
              [(old_edge, old_node)] with
      | Some (children, loads) => Some (EditedNode shared None children, loads)
      | None => None
      end
  end.

(** One CAS step at a stored node: the node is loaded (one load) before the
    walk splits it, descends through it or compares its endpoint, exactly as
    [rewrite_root] loads the root and [expand_child] loads a reference child. *)
Fixpoint edit_stored (tree : radix_tree) (key : list nat) (before after : option nat)
    {struct tree} : option (edit_tree * nat) :=
  match tree with
  | RadixTree prefix endpoint children =>
      match prefix_split prefix key with
      | PrefixMatched [] =>
          if endpoint_eqb endpoint before
          then Some (EditedNode prefix after (reference_children children), 1)
          else None
      | PrefixMatched (edge :: rest) =>
          match stored_slots (fun child => edit_stored child rest before after)
                  (fresh_edit_leaf rest before after) edge children with
          | Some (edited, loads) => Some (EditedNode prefix endpoint edited, S loads)
          | None => None
          end
      | PrefixDiverged shared old_edge old_suffix rest =>
          match split_slots shared old_edge
                  (EditedNode old_suffix endpoint (reference_children children))
                  rest before after with
          | Some (split, loads) => Some (split, S loads)
          | None => None
          end
      end
  end.

Fixpoint edit_edited (tree : edit_tree) (key : list nat) (before after : option nat)
    {struct tree} : option (edit_tree * nat) :=
  match tree with
  | StoredTree stored => edit_stored stored key before after
  | EditedNode prefix endpoint children =>
      match prefix_split prefix key with
      | PrefixMatched [] =>
          if endpoint_eqb endpoint before
          then Some (EditedNode prefix after children, 0)
          else None
      | PrefixMatched (edge :: rest) =>
          match edit_slots (fun child => edit_edited child rest before after)
                  (fresh_edit_leaf rest before after) edge children with
          | Some (edited, loads) => Some (EditedNode prefix endpoint edited, loads)
          | None => None
          end
      | PrefixDiverged shared old_edge old_suffix rest =>
          split_slots shared old_edge (EditedNode old_suffix endpoint children) rest before after
      end
  end.

Fixpoint edit_batch_counted (tree : edit_tree) (batch : list cas_mutation)
    : option (edit_tree * nat) :=
  match batch with
  | [] => Some (tree, 0)
  | (key, before, after) :: rest =>
      match edit_edited tree key before after with
      | Some (next, loads) =>
          match edit_batch_counted next rest with
          | Some (final, later) => Some (final, loads + later)
          | None => None
          end
      | None => None
      end
  end.

(** ** Finalization with counters *)

(** A finalized child is a kept reference ([reused_child_references]) or a
    rebuilt node ([build_final]). *)
Inductive resolved : Type :=
| KeptReference (stored : radix_tree)
| RebuiltNode (node : radix_tree).

Definition resolved_tree (child : resolved) : radix_tree :=
  match child with
  | KeptReference stored => stored
  | RebuiltNode node => node
  end.

Definition rebuilt_count (child : resolved) : nat :=
  match child with
  | KeptReference _ => 0
  | RebuiltNode _ => 1
  end.

(** [emitted_nodes] counts [build_final], [absorbed_nodes] the rebuilt
    children a compression merge consumes, [merge_loads] the reference
    children a merge loads, and [placed_nodes] lists every [stage_final] call
    in order. *)
Record finalize_report : Type := FinalizeReport {
  emitted_nodes : nat;
  absorbed_nodes : nat;
  merge_loads : nat;
  placed_nodes : list radix_tree
}.

Definition no_work : finalize_report := FinalizeReport 0 0 0 [].

Definition merge_reports (left right : finalize_report) : finalize_report :=
  FinalizeReport (emitted_nodes left + emitted_nodes right)
    (absorbed_nodes left + absorbed_nodes right)
    (merge_loads left + merge_loads right)
    (placed_nodes left ++ placed_nodes right).

Fixpoint finalize_slots (finalize : edit_tree -> option resolved * finalize_report)
    (children : list (nat * edit_tree)) : list (nat * resolved) * finalize_report :=
  match children with
  | [] => ([], no_work)
  | (edge, child) :: rest =>
      match finalize child, finalize_slots finalize rest with
      | (Some kept, report), (later, rest_report) =>
          ((edge, kept) :: later, merge_reports report rest_report)
      | (None, report), (later, rest_report) => (later, merge_reports report rest_report)
      end
  end.

Fixpoint rebuilt_children (children : list (nat * resolved)) : list radix_tree :=
  match children with
  | [] => []
  | (_, KeptReference _) :: rest => rebuilt_children rest
  | (_, RebuiltNode node) :: rest => node :: rebuilt_children rest
  end.

Fixpoint resolved_children (children : list (nat * resolved)) : list (nat * radix_tree) :=
  match children with
  | [] => []
  | (edge, child) :: rest => (edge, resolved_tree child) :: resolved_children rest
  end.

Definition merged_node (prefix : list nat) (edge : nat) (child : radix_tree) : radix_tree :=
  match child with
  | RadixTree child_prefix child_endpoint child_children =>
      RadixTree (prefix ++ edge :: child_prefix) child_endpoint child_children
  end.

(** [finalize_node] of the Rust editor: an [Absent] node without children is
    empty; an [Absent] node with one child merges with it, absorbing a rebuilt
    child or loading a reference child; any other node stages its rebuilt
    children and is emitted. *)
Definition finalize_contents (prefix : list nat) (endpoint : option nat)
    (kept : list (nat * resolved)) : option resolved * finalize_report :=
  match endpoint, kept with
  | None, [] => (None, no_work)
  | None, [(edge, KeptReference stored)] =>
      (Some (RebuiltNode (merged_node prefix edge stored)), FinalizeReport 1 0 1 [])
  | None, [(edge, RebuiltNode node)] =>
      (Some (RebuiltNode (merged_node prefix edge node)), FinalizeReport 1 1 0 [])
  | _, _ =>
      (Some (RebuiltNode (RadixTree prefix endpoint (resolved_children kept))),
       FinalizeReport 1 0 0 (rebuilt_children kept))
  end.

Fixpoint finalize_counted (tree : edit_tree) : option resolved * finalize_report :=
  match tree with
  | StoredTree stored => (Some (KeptReference stored), no_work)
  | EditedNode prefix endpoint children =>
      match finalize_slots finalize_counted children with
      | (kept, inner) =>
          match finalize_contents prefix endpoint kept with
          | (result, own) => (result, merge_reports inner own)
          end
      end
  end.

(** [finish]: an empty root is built as the empty node, and the root is
    staged last. *)
Definition finalize_root_counted (tree : edit_tree) : radix_tree * finalize_report :=
  match finalize_counted tree with
  | (None, report) => (empty_tree, merge_reports report (FinalizeReport 1 0 0 [empty_tree]))
  | (Some (RebuiltNode node), report) => (node, merge_reports report (FinalizeReport 0 0 0 [node]))
  | (Some (KeptReference stored), report) => (stored, report)
  end.

(** [rewrite_root] with its counters: the number of opened stored nodes and
    the finalization report. *)
Definition counted_rewrite (root : radix_tree) (batch : list cas_mutation)
    : option (radix_tree * nat * finalize_report) :=
  match batch with
  | [] => Some (root, 0, no_work)
  | _ :: _ =>
      match edit_batch_counted (StoredTree root) batch with
      | Some (edited, opens) =>
          match finalize_root_counted edited with
          | (final, report) => Some (final, opens, report)
          end
      | None => None
      end
  end.

(** ** Logical positions through editing *)

Lemma reference_children_positions :
  forall children,
    slot_sum stored_positions (reference_children children) = children_sum logical_nodes children.
Proof.
  induction children as [|[edge child] rest IH];
    cbn [reference_children slot_sum stored_positions children_sum]; [reflexivity|].
  rewrite IH. reflexivity.
Qed.

Lemma fresh_edit_leaf_positions :
  forall rest before after leaf,
    fresh_edit_leaf rest before after = Some leaf -> stored_positions leaf = 0.
Proof.
  intros rest [claim|] after leaf fresh; cbn [fresh_edit_leaf] in fresh; [discriminate fresh|].
  injection fresh as leaf_is. subst leaf. reflexivity.
Qed.

Lemma edit_slots_positions :
  forall (edit : edit_tree -> option (edit_tree * nat)) fresh edge children updated loads,
    (forall other child edited child_loads, In (other, child) children ->
       edit child = Some (edited, child_loads) ->
       stored_positions edited + child_loads = stored_positions child) ->
    (forall leaf, fresh = Some leaf -> stored_positions leaf = 0) ->
    edit_slots edit fresh edge children = Some (updated, loads) ->
    slot_sum stored_positions updated + loads = slot_sum stored_positions children.
Proof.
  intros edit fresh edge children.
  induction children as [|[other child] rest IH]; intros updated loads agree fresh_empty edited;
    cbn [edit_slots] in edited.
  - destruct fresh as [leaf|]; [|discriminate edited].
    injection edited as updated_is loads_is. subst updated loads.
    cbn [slot_sum]. rewrite (fresh_empty leaf eq_refl). reflexivity.
  - destruct (edge <? other).
    + destruct fresh as [leaf|]; [|discriminate edited].
      injection edited as updated_is loads_is. subst updated loads.
      cbn [slot_sum]. rewrite (fresh_empty leaf eq_refl). lia.
    + destruct (Nat.eqb edge other).
      * destruct (edit child) as [[child_edited child_loads]|] eqn:child_edit;
          [|discriminate edited].
        injection edited as updated_is loads_is. subst updated loads.
        cbn [slot_sum].
        pose proof (agree other child child_edited child_loads (or_introl eq_refl) child_edit).
        lia.
      * destruct (edit_slots edit fresh edge rest) as [[later later_loads]|] eqn:later_edit;
          [|discriminate edited].
        injection edited as updated_is loads_is. subst updated loads.
        cbn [slot_sum].
        assert (rest_agree : forall other' child' edited' loads', In (other', child') rest ->
          edit child' = Some (edited', loads') ->
          stored_positions edited' + loads' = stored_positions child').
        { intros other' child' edited' loads' member.
          exact (agree other' child' edited' loads' (or_intror member)). }
        pose proof (IH later later_loads rest_agree fresh_empty eq_refl). lia.
Qed.

Lemma stored_slots_positions :
  forall (edit : radix_tree -> option (edit_tree * nat)) fresh edge children updated loads,
    (forall other child edited child_loads, In (other, child) children ->
       edit child = Some (edited, child_loads) ->
       stored_positions edited + child_loads = logical_nodes child) ->
    (forall leaf, fresh = Some leaf -> stored_positions leaf = 0) ->
    stored_slots edit fresh edge children = Some (updated, loads) ->
    slot_sum stored_positions updated + loads = children_sum logical_nodes children.
Proof.
  intros edit fresh edge children.
  induction children as [|[other child] rest IH]; intros updated loads agree fresh_empty edited;
    cbn [stored_slots] in edited.
  - destruct fresh as [leaf|]; [|discriminate edited].
    injection edited as updated_is loads_is. subst updated loads.
    cbn [slot_sum children_sum]. rewrite (fresh_empty leaf eq_refl). reflexivity.
  - destruct (edge <? other).
    + destruct fresh as [leaf|]; [|discriminate edited].
      injection edited as updated_is loads_is. subst updated loads.
      cbn [slot_sum children_sum reference_children stored_positions].
      rewrite (fresh_empty leaf eq_refl), reference_children_positions. lia.
    + destruct (Nat.eqb edge other).
      * destruct (edit child) as [[child_edited child_loads]|] eqn:child_edit;
          [|discriminate edited].
        injection edited as updated_is loads_is. subst updated loads.
        cbn [slot_sum children_sum].
        rewrite reference_children_positions.
        pose proof (agree other child child_edited child_loads (or_introl eq_refl) child_edit).
        lia.
      * destruct (stored_slots edit fresh edge rest) as [[later later_loads]|] eqn:later_edit;
          [|discriminate edited].
        injection edited as updated_is loads_is. subst updated loads.
        cbn [slot_sum children_sum stored_positions].
        assert (rest_agree : forall other' child' edited' loads', In (other', child') rest ->
          edit child' = Some (edited', loads') ->
          stored_positions edited' + loads' = logical_nodes child').
        { intros other' child' edited' loads' member.
          exact (agree other' child' edited' loads' (or_intror member)). }
        pose proof (IH later later_loads rest_agree fresh_empty eq_refl). lia.
Qed.

Lemma split_slots_positions :
  forall shared old_edge old_node rest before after split loads,
    split_slots shared old_edge old_node rest before after = Some (split, loads) ->
    stored_positions split + loads = stored_positions old_node.
Proof.
  intros shared old_edge old_node [|edge tail] before after split loads splitting;
    cbn [split_slots] in splitting.
  - destruct (endpoint_eqb None before); [|discriminate splitting].
    injection splitting as split_is loads_is. subst split loads.
    cbn [stored_positions slot_sum]. lia.
  - destruct (edit_slots (fun _ : edit_tree => @None (edit_tree * nat))
      (fresh_edit_leaf tail before after) edge [(old_edge, old_node)])
      as [[children child_loads]|] eqn:slots; [|discriminate splitting].
    injection splitting as split_is loads_is. subst split loads.
    cbn [stored_positions].
    assert (agree : forall other child (edited : edit_tree) child_loads',
      In (other, child) [(old_edge, old_node)] ->
      (fun _ : edit_tree => @None (edit_tree * nat)) child = Some (edited, child_loads') ->
      stored_positions edited + child_loads' = stored_positions child).
    { intros other child edited child_loads' _ none. discriminate none. }
    pose proof (edit_slots_positions (fun _ : edit_tree => @None (edit_tree * nat))
      (fresh_edit_leaf tail before after) edge [(old_edge, old_node)] children child_loads
      agree (fresh_edit_leaf_positions tail before after) slots) as counted.
    cbn [slot_sum] in counted. lia.
Qed.

Lemma edit_stored_positions :
  forall tree key before after edited loads,
    edit_stored tree key before after = Some (edited, loads) ->
    stored_positions edited + loads = logical_nodes tree.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  intros key before after edited loads editing. cbn [edit_stored] in editing.
  cbn [logical_nodes].
  destruct (prefix_split prefix key) as [[|edge rest]|shared old_edge old_suffix rest].
  - destruct (endpoint_eqb endpoint before); [|discriminate editing].
    injection editing as edited_is loads_is. subst edited loads.
    cbn [stored_positions]. rewrite reference_children_positions. lia.
  - destruct (stored_slots (fun child => edit_stored child rest before after)
      (fresh_edit_leaf rest before after) edge children) as [[updated child_loads]|] eqn:slots;
      [|discriminate editing].
    injection editing as edited_is loads_is. subst edited loads.
    cbn [stored_positions].
    assert (agree : forall other child edited' child_loads', In (other, child) children ->
      edit_stored child rest before after = Some (edited', child_loads') ->
      stored_positions edited' + child_loads' = logical_nodes child).
    { intros other child edited' child_loads' member.
      rewrite Forall_forall in IH.
      exact (IH (other, child) member rest before after edited' child_loads'). }
    pose proof (stored_slots_positions _ _ edge children updated child_loads agree
      (fresh_edit_leaf_positions rest before after) slots).
    lia.
  - destruct (split_slots shared old_edge
      (EditedNode old_suffix endpoint (reference_children children)) rest before after)
      as [[split split_loads]|] eqn:splitting; [|discriminate editing].
    injection editing as edited_is loads_is. subst edited loads.
    pose proof (split_slots_positions _ _ _ _ _ _ _ _ splitting) as counted.
    cbn [stored_positions] in counted. rewrite reference_children_positions in counted. lia.
Qed.

Lemma edit_edited_positions :
  forall tree key before after edited loads,
    edit_edited tree key before after = Some (edited, loads) ->
    stored_positions edited + loads = stored_positions tree.
Proof.
  intros tree. induction tree as [stored|prefix endpoint children IH] using edit_tree_induction;
    intros key before after edited loads editing; cbn [edit_edited] in editing.
  - cbn [stored_positions]. exact (edit_stored_positions stored key before after edited loads editing).
  - cbn [stored_positions].
    destruct (prefix_split prefix key) as [[|edge rest]|shared old_edge old_suffix rest].
    + destruct (endpoint_eqb endpoint before); [|discriminate editing].
      injection editing as edited_is loads_is. subst edited loads. cbn [stored_positions]. lia.
    + destruct (edit_slots (fun child => edit_edited child rest before after)
        (fresh_edit_leaf rest before after) edge children) as [[updated child_loads]|] eqn:slots;
        [|discriminate editing].
      injection editing as edited_is loads_is. subst edited loads. cbn [stored_positions].
      assert (agree : forall other child edited' child_loads', In (other, child) children ->
        edit_edited child rest before after = Some (edited', child_loads') ->
        stored_positions edited' + child_loads' = stored_positions child).
      { intros other child edited' child_loads' member.
        rewrite Forall_forall in IH.
        exact (IH (other, child) member rest before after edited' child_loads'). }
      pose proof (edit_slots_positions _ _ edge children updated child_loads agree
        (fresh_edit_leaf_positions rest before after) slots).
      lia.
    + pose proof (split_slots_positions _ _ _ _ _ _ _ _ editing) as counted.
      cbn [stored_positions] in counted. exact counted.
Qed.

Lemma edit_batch_counted_positions :
  forall batch tree edited loads,
    edit_batch_counted tree batch = Some (edited, loads) ->
    stored_positions edited + loads = stored_positions tree.
Proof.
  induction batch as [|[[key before] after] rest IH]; intros tree edited loads editing;
    cbn [edit_batch_counted] in editing.
  - injection editing as edited_is loads_is. subst edited loads. lia.
  - destruct (edit_edited tree key before after) as [[next next_loads]|] eqn:step;
      [|discriminate editing].
    destruct (edit_batch_counted next rest) as [[final later]|] eqn:later_edit;
      [|discriminate editing].
    injection editing as edited_is loads_is. subst edited loads.
    pose proof (edit_edited_positions tree key before after next next_loads step).
    pose proof (IH next final later later_edit). lia.
Qed.

(** ** Logical positions through finalization *)

Fixpoint kept_sum (children : list (nat * resolved)) : nat :=
  match children with
  | [] => 0
  | (_, child) :: rest => logical_nodes (resolved_tree child) + kept_sum rest
  end.

Lemma resolved_children_sum :
  forall kept, children_sum logical_nodes (resolved_children kept) = kept_sum kept.
Proof.
  induction kept as [|[edge child] rest IH]; cbn [resolved_children children_sum kept_sum];
    [reflexivity|].
  rewrite IH. reflexivity.
Qed.

Lemma merged_node_logical :
  forall prefix edge child, logical_nodes (merged_node prefix edge child) = logical_nodes child.
Proof. intros prefix edge [child_prefix child_endpoint child_children]. reflexivity. Qed.

Lemma rebuilt_children_cons :
  forall edge child rest,
    length (rebuilt_children ((edge, child) :: rest)) =
      rebuilt_count child + length (rebuilt_children rest).
Proof. intros edge [stored|node] rest; reflexivity. Qed.

Definition finalize_sound (tree : edit_tree) : Prop :=
  (forall report, finalize_counted tree = (None, report) ->
     stored_positions tree = 0 /\ report = no_work) /\
  (forall result report, finalize_counted tree = (Some result, report) ->
     logical_nodes (resolved_tree result) + merge_loads report =
       stored_positions tree + length (placed_nodes report) + rebuilt_count result /\
     emitted_nodes report =
       length (placed_nodes report) + absorbed_nodes report + rebuilt_count result).

Lemma finalize_slots_sound :
  forall children kept report,
    Forall (fun child => finalize_sound (snd child)) children ->
    finalize_slots finalize_counted children = (kept, report) ->
    kept_sum kept + merge_loads report =
      slot_sum stored_positions children + length (placed_nodes report) +
        length (rebuilt_children kept) /\
    emitted_nodes report =
      length (placed_nodes report) + absorbed_nodes report + length (rebuilt_children kept) /\
    (kept = [] -> slot_sum stored_positions children = 0 /\ report = no_work).
Proof.
  induction children as [|[edge child] rest IH]; intros kept report every slots.
  - cbn [finalize_slots] in slots. injection slots as kept_is report_is. subst kept report.
    cbn. split; [reflexivity|]. split; [reflexivity|]. intros _. split; reflexivity.
  - cbn [finalize_slots] in slots.
    destruct (finalize_slots finalize_counted rest) as [later rest_report] eqn:rest_slots.
    destruct (IH later rest_report (Forall_inv_tail every) eq_refl)
      as (rest_count & rest_emitted & rest_empty).
    destruct (finalize_counted child) as [[result|] child_report] eqn:child_counted.
    + pose proof (Forall_inv every) as child_sound. cbn [snd] in child_sound.
      unfold finalize_sound in child_sound. destruct child_sound as [_ some_sound].
      destruct (some_sound result child_report child_counted) as [child_count child_emitted].
      cbv beta iota in slots. injection slots as kept_is report_is. subst kept report.
      rewrite rebuilt_children_cons.
      cbn [kept_sum slot_sum merge_reports merge_loads placed_nodes emitted_nodes absorbed_nodes].
      rewrite length_app.
      split; [lia|]. split; [lia|]. intros empty. discriminate empty.
    + pose proof (Forall_inv every) as child_sound. cbn [snd] in child_sound.
      unfold finalize_sound in child_sound. destruct child_sound as [none_sound _].
      destruct (none_sound child_report child_counted) as [child_empty child_report_is].
      cbv beta iota in slots. injection slots as kept_is report_is.
      subst kept report child_report.
      cbn [slot_sum merge_reports merge_loads placed_nodes emitted_nodes absorbed_nodes no_work app].
      split; [lia|]. split; [lia|].
      intros later_empty. destruct (rest_empty later_empty) as [rest_zero rest_report_is].
      subst rest_report. split; [lia|reflexivity].
Qed.

Lemma finalize_contents_sound :
  forall prefix endpoint kept result own,
    finalize_contents prefix endpoint kept = (result, own) ->
    match result with
    | None => endpoint = None /\ kept = [] /\ own = no_work
    | Some rebuilt =>
        rebuilt_count rebuilt = 1 /\
        logical_nodes (resolved_tree rebuilt) + merge_loads own + length (rebuilt_children kept) =
          kept_sum kept + length (placed_nodes own) + 1 /\
        length (rebuilt_children kept) + emitted_nodes own =
          length (placed_nodes own) + absorbed_nodes own + 1
    end.
Proof.
  intros prefix endpoint kept result own contents.
  assert (general : forall endpoint',
    rebuilt_count (RebuiltNode (RadixTree prefix endpoint' (resolved_children kept))) = 1 /\
    logical_nodes (resolved_tree (RebuiltNode (RadixTree prefix endpoint'
        (resolved_children kept)))) +
      merge_loads (FinalizeReport 1 0 0 (rebuilt_children kept)) +
      length (rebuilt_children kept) =
      kept_sum kept + length (placed_nodes (FinalizeReport 1 0 0 (rebuilt_children kept))) + 1 /\
    length (rebuilt_children kept) + emitted_nodes (FinalizeReport 1 0 0 (rebuilt_children kept)) =
      length (placed_nodes (FinalizeReport 1 0 0 (rebuilt_children kept))) +
      absorbed_nodes (FinalizeReport 1 0 0 (rebuilt_children kept)) + 1).
  { intros endpoint'.
    cbn [rebuilt_count resolved_tree logical_nodes merge_loads placed_nodes emitted_nodes
      absorbed_nodes].
    rewrite resolved_children_sum. lia. }
  destruct endpoint as [claim|].
  - cbn [finalize_contents] in contents. injection contents as result_is own_is.
    subst result own. exact (general (Some claim)).
  - destruct kept as [|[edge [stored|node]] [|second later]];
      cbn [finalize_contents] in contents; injection contents as result_is own_is;
      subst result own.
    + split; [reflexivity|]. split; reflexivity.
    + cbn [rebuilt_count resolved_tree merge_loads placed_nodes emitted_nodes absorbed_nodes
        rebuilt_children length kept_sum].
      rewrite merged_node_logical. lia.
    + exact (general None).
    + cbn [rebuilt_count resolved_tree merge_loads placed_nodes emitted_nodes absorbed_nodes
        rebuilt_children length kept_sum].
      rewrite merged_node_logical. lia.
    + exact (general None).
Qed.

Lemma finalize_counted_sound : forall tree, finalize_sound tree.
Proof.
  intros tree. induction tree as [stored|prefix endpoint children IH] using edit_tree_induction.
  - split.
    + intros report counted. cbn [finalize_counted] in counted. discriminate counted.
    + intros result report counted. cbn [finalize_counted] in counted.
      injection counted as result_is report_is. subst result report.
      cbn [resolved_tree rebuilt_count stored_positions merge_loads placed_nodes emitted_nodes
        absorbed_nodes no_work length].
      lia.
  - split.
    + intros report counted. cbn [finalize_counted stored_positions] in counted |- *.
      destruct (finalize_slots finalize_counted children) as [kept inner] eqn:slots.
      destruct (finalize_contents prefix endpoint kept) as [result own] eqn:contents.
      cbv beta iota in counted. injection counted as result_is report_is.
      subst result report.
      pose proof (finalize_contents_sound prefix endpoint kept None own contents)
        as contents_facts.
      cbv beta iota in contents_facts. destruct contents_facts as (_ & kept_empty & own_empty).
      destruct (finalize_slots_sound children kept inner IH slots) as (_ & _ & slots_empty).
      destruct (slots_empty kept_empty) as [zero inner_empty].
      subst own inner. split; [exact zero|reflexivity].
    + intros result report counted. cbn [finalize_counted stored_positions] in counted |- *.
      destruct (finalize_slots finalize_counted children) as [kept inner] eqn:slots.
      destruct (finalize_contents prefix endpoint kept) as [content_result own] eqn:contents.
      cbv beta iota in counted. injection counted as result_is report_is.
      subst content_result report.
      pose proof (finalize_contents_sound prefix endpoint kept (Some result) own contents)
        as contents_facts.
      cbv beta iota in contents_facts.
      destruct contents_facts as (rebuilt & content_count & content_emitted).
      destruct (finalize_slots_sound children kept inner IH slots)
        as (slot_count & slot_emitted & _).
      cbn [merge_reports merge_loads placed_nodes emitted_nodes absorbed_nodes].
      rewrite length_app, rebuilt. lia.
Qed.

Lemma finalize_root_sound :
  forall tree final report,
    finalize_root_counted tree = (final, report) ->
    logical_nodes final + merge_loads report =
      stored_positions tree + length (placed_nodes report) /\
    emitted_nodes report = length (placed_nodes report) + absorbed_nodes report.
Proof.
  intros tree final report rooted. unfold finalize_root_counted in rooted.
  destruct (finalize_counted tree) as [[[stored|node]|] inner] eqn:counted;
    injection rooted as final_is report_is; subst final report;
    destruct (finalize_counted_sound tree) as [none_sound some_sound].
  - destruct (some_sound (KeptReference stored) inner counted) as [count emitted].
    cbn [resolved_tree rebuilt_count] in count, emitted. lia.
  - destruct (some_sound (RebuiltNode node) inner counted) as [count emitted].
    cbn [resolved_tree rebuilt_count] in count, emitted.
    cbn [merge_reports merge_loads placed_nodes emitted_nodes absorbed_nodes].
    rewrite length_app. cbn [length]. lia.
  - destruct (none_sound inner counted) as [zero inner_is]. subst inner.
    cbn. lia.
Qed.

(** Capstone: the counted rewrite moves logical positions exactly; the final
    root's logical nodes plus the loads (opens and merge loads) equal the old
    root's logical nodes plus the placements, and every emitted node is placed
    or absorbed by a compression merge. *)
Theorem rewrite_moves_logical_positions_exactly :
  forall root batch final opens report,
    counted_rewrite root batch = Some (final, opens, report) ->
    logical_nodes final + (opens + merge_loads report) =
      logical_nodes root + length (placed_nodes report) /\
    emitted_nodes report = length (placed_nodes report) + absorbed_nodes report.
Proof.
  intros root batch final opens report rewriting.
  destruct batch as [|mutation rest]; cbn [counted_rewrite] in rewriting.
  - injection rewriting as final_is opens_is report_is. subst final opens report.
    cbn. lia.
  - destruct (edit_batch_counted (StoredTree root) (mutation :: rest))
      as [[edited loads]|] eqn:editing; [|discriminate rewriting].
    destruct (finalize_root_counted edited) as [rooted_final rooted_report] eqn:rooted.
    cbv beta iota in rewriting.
    injection rewriting as final_is opens_is report_is. subst final opens report.
    destruct (finalize_root_sound edited rooted_final rooted_report rooted) as [count emitted].
    pose proof (edit_batch_counted_positions (mutation :: rest) (StoredTree root) edited loads
      editing) as positions.
    cbn [stored_positions] in positions. split; lia.
Qed.

(** Capstone: summed over the three semantic namespaces, as the Rust report's
    [absorb] sums them, the same identity holds. *)
Theorem absorbed_reports_move_positions_exactly :
  forall data continuations joins data_batch continuation_batch join_batch
         data_final continuation_final join_final data_opens continuation_opens join_opens
         data_report continuation_report join_report,
    counted_rewrite data data_batch = Some (data_final, data_opens, data_report) ->
    counted_rewrite continuations continuation_batch =
      Some (continuation_final, continuation_opens, continuation_report) ->
    counted_rewrite joins join_batch = Some (join_final, join_opens, join_report) ->
    logical_nodes data_final + logical_nodes continuation_final + logical_nodes join_final +
      ((data_opens + merge_loads data_report) +
       (continuation_opens + merge_loads continuation_report) +
       (join_opens + merge_loads join_report)) =
    logical_nodes data + logical_nodes continuations + logical_nodes joins +
      (length (placed_nodes data_report) + length (placed_nodes continuation_report) +
       length (placed_nodes join_report)).
Proof.
  intros data continuations joins data_batch continuation_batch join_batch
    data_final continuation_final join_final data_opens continuation_opens join_opens
    data_report continuation_report join_report data_rewrite continuation_rewrite join_rewrite.
  destruct (rewrite_moves_logical_positions_exactly _ _ _ _ _ data_rewrite) as [data_count _].
  destruct (rewrite_moves_logical_positions_exactly _ _ _ _ _ continuation_rewrite)
    as [continuation_count _].
  destruct (rewrite_moves_logical_positions_exactly _ _ _ _ _ join_rewrite) as [join_count _].
  lia.
Qed.

(** ** The counted rewrite is the editor's rewrite *)

Lemma image_reference_children :
  forall children, image_children edit_image (reference_children children) = children.
Proof.
  induction children as [|[edge child] rest IH];
    cbn [reference_children image_children edit_image]; [reflexivity|].
  rewrite IH. reflexivity.
Qed.

Definition result_image (result : option (edit_tree * nat)) : option radix_tree :=
  match result with
  | Some (tree, _) => Some (edit_image tree)
  | None => None
  end.

Definition slots_image (result : option (list (nat * edit_tree) * nat))
    : option (list (nat * radix_tree)) :=
  match result with
  | Some (children, _) => Some (image_children edit_image children)
  | None => None
  end.

Lemma fresh_image :
  forall rest before after,
    option_map edit_image (fresh_edit_leaf rest before after) = fresh_leaf rest before after.
Proof. intros rest [claim|] after; reflexivity. Qed.

Lemma edit_slots_image :
  forall (edit : edit_tree -> option (edit_tree * nat)) (reference : radix_tree -> option radix_tree)
         fresh edge children,
    (forall other child, In (other, child) children ->
       result_image (edit child) = reference (edit_image child)) ->
    slots_image (edit_slots edit fresh edge children) =
      edit_children reference (option_map edit_image fresh) edge
        (image_children edit_image children).
Proof.
  intros edit reference fresh edge children.
  induction children as [|[other child] rest IH]; intros agree;
    cbn [edit_slots image_children edit_children].
  - destruct fresh as [leaf|]; reflexivity.
  - destruct (edge <? other).
    + destruct fresh as [leaf|]; reflexivity.
    + destruct (Nat.eqb edge other).
      * rewrite <- (agree other child (or_introl eq_refl)).
        destruct (edit child) as [[edited loads]|]; reflexivity.
      * rewrite <- IH.
        2: { intros other' child' member. exact (agree other' child' (or_intror member)). }
        destruct (edit_slots edit fresh edge rest) as [[later loads]|]; reflexivity.
Qed.

Lemma stored_slots_image :
  forall (edit : radix_tree -> option (edit_tree * nat)) (reference : radix_tree -> option radix_tree)
         fresh edge children,
    (forall other child, In (other, child) children ->
       result_image (edit child) = reference child) ->
    slots_image (stored_slots edit fresh edge children) =
      edit_children reference (option_map edit_image fresh) edge children.
Proof.
  intros edit reference fresh edge children.
  induction children as [|[other child] rest IH]; intros agree;
    cbn [stored_slots edit_children].
  - destruct fresh as [leaf|]; reflexivity.
  - destruct (edge <? other).
    + destruct fresh as [leaf|]; [|reflexivity].
      cbn [slots_image option_map image_children reference_children edit_image].
      rewrite image_reference_children. reflexivity.
    + destruct (Nat.eqb edge other).
      * rewrite <- (agree other child (or_introl eq_refl)).
        destruct (edit child) as [[edited loads]|]; [|reflexivity].
        cbn [slots_image result_image image_children].
        rewrite image_reference_children. reflexivity.
      * rewrite <- IH.
        2: { intros other' child' member. exact (agree other' child' (or_intror member)). }
        destruct (stored_slots edit fresh edge rest) as [[later loads]|]; reflexivity.
Qed.

Lemma split_slots_image :
  forall shared old_edge old_node rest before after,
    result_image (split_slots shared old_edge old_node rest before after) =
      split_edit shared old_edge (edit_image old_node) rest before after.
Proof.
  intros shared old_edge old_node [|edge tail] before after; cbn [split_slots split_edit].
  - destruct (endpoint_eqb None before); reflexivity.
  - assert (none_agree : forall other child, In (other, child) [(old_edge, old_node)] ->
      result_image ((fun _ : edit_tree => @None (edit_tree * nat)) child) =
        (fun _ : radix_tree => @None radix_tree) (edit_image child))
      by (intros other child _; reflexivity).
    pose proof (edit_slots_image (fun _ : edit_tree => @None (edit_tree * nat))
      (fun _ : radix_tree => @None radix_tree) (fresh_edit_leaf tail before after) edge
      [(old_edge, old_node)] none_agree) as same.
    cbn [image_children] in same. rewrite fresh_image in same.
    rewrite <- same.
    destruct (edit_slots (fun _ : edit_tree => @None (edit_tree * nat))
      (fresh_edit_leaf tail before after) edge [(old_edge, old_node)])
      as [[children loads]|]; reflexivity.
Qed.

Lemma edit_stored_image :
  forall tree key before after,
    result_image (edit_stored tree key before after) = edit_node tree key before after.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  intros key before after. cbn [edit_stored edit_node].
  destruct (prefix_split prefix key) as [[|edge rest]|shared old_edge old_suffix rest].
  - destruct (endpoint_eqb endpoint before); [|reflexivity].
    cbn [result_image edit_image]. rewrite image_reference_children. reflexivity.
  - rewrite <- fresh_image.
    rewrite <- (stored_slots_image (fun child => edit_stored child rest before after)
      (fun child => edit_node child rest before after) (fresh_edit_leaf rest before after) edge
      children).
    2: { intros other child member. rewrite Forall_forall in IH.
         exact (IH (other, child) member rest before after). }
    destruct (stored_slots (fun child => edit_stored child rest before after)
      (fresh_edit_leaf rest before after) edge children) as [[edited loads]|]; reflexivity.
  - pose proof (split_slots_image shared old_edge
      (EditedNode old_suffix endpoint (reference_children children)) rest before after) as same.
    cbn [edit_image] in same. rewrite image_reference_children in same.
    rewrite <- same.
    destruct (split_slots shared old_edge
      (EditedNode old_suffix endpoint (reference_children children)) rest before after)
      as [[split loads]|]; reflexivity.
Qed.

Lemma edit_edited_image :
  forall tree key before after,
    result_image (edit_edited tree key before after) = edit_node (edit_image tree) key before after.
Proof.
  intros tree. induction tree as [stored|prefix endpoint children IH] using edit_tree_induction;
    intros key before after.
  - cbn [edit_edited edit_image]. apply edit_stored_image.
  - cbn [edit_edited edit_image edit_node].
    destruct (prefix_split prefix key) as [[|edge rest]|shared old_edge old_suffix rest].
    + destruct (endpoint_eqb endpoint before); reflexivity.
    + rewrite <- fresh_image.
      rewrite <- (edit_slots_image (fun child => edit_edited child rest before after)
        (fun child => edit_node child rest before after) (fresh_edit_leaf rest before after) edge
        children).
      2: { intros other child member. rewrite Forall_forall in IH.
           exact (IH (other, child) member rest before after). }
      destruct (edit_slots (fun child => edit_edited child rest before after)
        (fresh_edit_leaf rest before after) edge children) as [[edited loads]|]; reflexivity.
    + pose proof (split_slots_image shared old_edge (EditedNode old_suffix endpoint children)
        rest before after) as same.
      cbn [edit_image] in same. exact same.
Qed.

Lemma edit_batch_counted_image :
  forall batch tree, result_image (edit_batch_counted tree batch) = edit_batch (edit_image tree) batch.
Proof.
  induction batch as [|[[key before] after] rest IH]; intros tree;
    cbn [edit_batch_counted edit_batch].
  - reflexivity.
  - rewrite <- edit_edited_image.
    destruct (edit_edited tree key before after) as [[next loads]|]; cbn [result_image];
      [|reflexivity].
    rewrite <- IH. destruct (edit_batch_counted next rest) as [[final later]|]; reflexivity.
Qed.

(** Stored subtrees of an edited trie are admitted nodes. *)
Fixpoint slots_hold (holds : edit_tree -> Prop) (children : list (nat * edit_tree)) : Prop :=
  match children with
  | [] => True
  | (_, child) :: rest => holds child /\ slots_hold holds rest
  end.

Fixpoint stored_admitted (tree : edit_tree) : Prop :=
  match tree with
  | StoredTree stored => admitted_node stored
  | EditedNode _ _ children => slots_hold stored_admitted children
  end.

Definition edit_ready (tree : edit_tree) : Prop :=
  match tree with
  | StoredTree stored => admitted_root stored
  | EditedNode _ _ children => slots_hold stored_admitted children
  end.

Lemma stored_admitted_ready : forall tree, stored_admitted tree -> edit_ready tree.
Proof. intros [stored|prefix endpoint children] admitted; [right|]; exact admitted. Qed.

Lemma slots_hold_member :
  forall holds children edge child,
    slots_hold holds children -> In (edge, child) children -> holds child.
Proof.
  intros holds children edge child.
  induction children as [|[other target] rest IH]; [intros _ []|].
  cbn [slots_hold]. intros [here later] [same|member].
  - injection same as edge_is child_is. subst target. exact here.
  - exact (IH later member).
Qed.

Lemma reference_children_admitted :
  forall children, all_children admitted_node children ->
    slots_hold stored_admitted (reference_children children).
Proof.
  induction children as [|[edge child] rest IH];
    cbn [all_children reference_children slots_hold stored_admitted]; [intros _; exact I|].
  intros [here later]. split; [exact here|exact (IH later)].
Qed.

Lemma admitted_root_children :
  forall prefix endpoint children,
    admitted_root (RadixTree prefix endpoint children) -> all_children admitted_node children.
Proof.
  intros prefix endpoint children [empty|shaped].
  - unfold empty_tree in empty. injection empty as prefix_is endpoint_is children_is.
    subst children. exact I.
  - cbn [admitted_node] in shaped. exact (proj2 (proj2 shaped)).
Qed.

Lemma fresh_edit_leaf_admitted :
  forall rest before after leaf,
    fresh_edit_leaf rest before after = Some leaf -> stored_admitted leaf.
Proof.
  intros rest [claim|] after leaf fresh; cbn [fresh_edit_leaf] in fresh; [discriminate fresh|].
  injection fresh as leaf_is. subst leaf. exact I.
Qed.

Lemma edit_slots_admitted :
  forall (edit : edit_tree -> option (edit_tree * nat)) fresh edge children updated loads,
    slots_hold stored_admitted children ->
    (forall other child edited child_loads, In (other, child) children ->
       edit child = Some (edited, child_loads) -> stored_admitted edited) ->
    (forall leaf, fresh = Some leaf -> stored_admitted leaf) ->
    edit_slots edit fresh edge children = Some (updated, loads) ->
    slots_hold stored_admitted updated.
Proof.
  intros edit fresh edge children.
  induction children as [|[other child] rest IH];
    intros updated loads admitted agree fresh_admitted edited; cbn [edit_slots] in edited.
  - destruct fresh as [leaf|]; [|discriminate edited].
    injection edited as updated_is _. subst updated.
    cbn [slots_hold]. split; [exact (fresh_admitted leaf eq_refl)|exact I].
  - cbn [slots_hold] in admitted. destruct admitted as [child_admitted rest_admitted].
    destruct (edge <? other).
    + destruct fresh as [leaf|]; [|discriminate edited].
      injection edited as updated_is _. subst updated.
      cbn [slots_hold]. split; [exact (fresh_admitted leaf eq_refl)|].
      split; assumption.
    + destruct (Nat.eqb edge other).
      * destruct (edit child) as [[child_edited child_loads]|] eqn:child_edit;
          [|discriminate edited].
        injection edited as updated_is _. subst updated.
        cbn [slots_hold]. split; [|exact rest_admitted].
        exact (agree other child child_edited child_loads (or_introl eq_refl) child_edit).
      * destruct (edit_slots edit fresh edge rest) as [[later later_loads]|] eqn:later_edit;
          [|discriminate edited].
        injection edited as updated_is _. subst updated.
        cbn [slots_hold]. split; [exact child_admitted|].
        apply (IH later later_loads rest_admitted); [|exact fresh_admitted|exact eq_refl].
        intros other' child' edited' loads' member.
        exact (agree other' child' edited' loads' (or_intror member)).
Qed.

Lemma stored_slots_admitted :
  forall (edit : radix_tree -> option (edit_tree * nat)) fresh edge children updated loads,
    all_children admitted_node children ->
    (forall other child edited child_loads, In (other, child) children ->
       edit child = Some (edited, child_loads) -> stored_admitted edited) ->
    (forall leaf, fresh = Some leaf -> stored_admitted leaf) ->
    stored_slots edit fresh edge children = Some (updated, loads) ->
    slots_hold stored_admitted updated.
Proof.
  intros edit fresh edge children.
  induction children as [|[other child] rest IH];
    intros updated loads admitted agree fresh_admitted edited; cbn [stored_slots] in edited.
  - destruct fresh as [leaf|]; [|discriminate edited].
    injection edited as updated_is _. subst updated.
    cbn [slots_hold]. split; [exact (fresh_admitted leaf eq_refl)|exact I].
  - cbn [all_children] in admitted. destruct admitted as [child_admitted rest_admitted].
    destruct (edge <? other).
    + destruct fresh as [leaf|]; [|discriminate edited].
      injection edited as updated_is _. subst updated.
      cbn [slots_hold reference_children stored_admitted].
      split; [exact (fresh_admitted leaf eq_refl)|].
      split; [exact child_admitted|exact (reference_children_admitted rest rest_admitted)].
    + destruct (Nat.eqb edge other).
      * destruct (edit child) as [[child_edited child_loads]|] eqn:child_edit;
          [|discriminate edited].
        injection edited as updated_is _. subst updated.
        cbn [slots_hold]. split; [|exact (reference_children_admitted rest rest_admitted)].
        exact (agree other child child_edited child_loads (or_introl eq_refl) child_edit).
      * destruct (stored_slots edit fresh edge rest) as [[later later_loads]|] eqn:later_edit;
          [|discriminate edited].
        injection edited as updated_is _. subst updated.
        cbn [slots_hold stored_admitted]. split; [exact child_admitted|].
        apply (IH later later_loads rest_admitted); [|exact fresh_admitted|exact eq_refl].
        intros other' child' edited' loads' member.
        exact (agree other' child' edited' loads' (or_intror member)).
Qed.

Lemma split_slots_admitted :
  forall shared old_edge old_node rest before after split loads,
    stored_admitted old_node ->
    split_slots shared old_edge old_node rest before after = Some (split, loads) ->
    stored_admitted split.
Proof.
  intros shared old_edge old_node [|edge tail] before after split loads old_admitted splitting;
    cbn [split_slots] in splitting.
  - destruct (endpoint_eqb None before); [|discriminate splitting].
    injection splitting as split_is _. subst split.
    cbn [stored_admitted slots_hold]. split; [exact old_admitted|exact I].
  - destruct (edit_slots (fun _ : edit_tree => @None (edit_tree * nat))
      (fresh_edit_leaf tail before after) edge [(old_edge, old_node)])
      as [[children child_loads]|] eqn:slots; [|discriminate splitting].
    injection splitting as split_is _. subst split.
    cbn [stored_admitted].
    apply (edit_slots_admitted (fun _ : edit_tree => @None (edit_tree * nat))
      (fresh_edit_leaf tail before after) edge [(old_edge, old_node)] children child_loads).
    + cbn [slots_hold]. split; [exact old_admitted|exact I].
    + intros other child edited child_loads' _ none. discriminate none.
    + exact (fresh_edit_leaf_admitted tail before after).
    + exact slots.
Qed.

Lemma edit_stored_admitted :
  forall tree key before after edited loads,
    admitted_root tree -> edit_stored tree key before after = Some (edited, loads) ->
    stored_admitted edited.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  intros key before after edited loads admitted editing.
  pose proof (admitted_root_children prefix endpoint children admitted) as child_shapes.
  cbn [edit_stored] in editing.
  destruct (prefix_split prefix key) as [[|edge rest]|shared old_edge old_suffix rest].
  - destruct (endpoint_eqb endpoint before); [|discriminate editing].
    injection editing as edited_is _. subst edited.
    cbn [stored_admitted]. exact (reference_children_admitted children child_shapes).
  - destruct (stored_slots (fun child => edit_stored child rest before after)
      (fresh_edit_leaf rest before after) edge children) as [[updated child_loads]|] eqn:slots;
      [|discriminate editing].
    injection editing as edited_is _. subst edited.
    cbn [stored_admitted].
    apply (stored_slots_admitted (fun child => edit_stored child rest before after)
      (fresh_edit_leaf rest before after) edge children updated child_loads child_shapes).
    + intros other child edited' child_loads' member child_edit.
      rewrite Forall_forall in IH.
      apply (IH (other, child) member rest before after edited' child_loads');
        [|exact child_edit].
      right. rewrite all_children_iff in child_shapes. exact (child_shapes other child member).
    + exact (fresh_edit_leaf_admitted rest before after).
    + exact slots.
  - destruct (split_slots shared old_edge
      (EditedNode old_suffix endpoint (reference_children children)) rest before after)
      as [[split split_loads]|] eqn:splitting; [|discriminate editing].
    injection editing as edited_is _. subst edited.
    apply (split_slots_admitted shared old_edge
      (EditedNode old_suffix endpoint (reference_children children)) rest before after split
      split_loads); [|exact splitting].
    cbn [stored_admitted]. exact (reference_children_admitted children child_shapes).
Qed.

Lemma edit_edited_admitted :
  forall tree key before after edited loads,
    edit_ready tree -> edit_edited tree key before after = Some (edited, loads) ->
    stored_admitted edited.
Proof.
  intros tree. induction tree as [stored|prefix endpoint children IH] using edit_tree_induction;
    intros key before after edited loads ready editing; cbn [edit_edited] in editing.
  - exact (edit_stored_admitted stored key before after edited loads ready editing).
  - cbn [edit_ready] in ready.
    destruct (prefix_split prefix key) as [[|edge rest]|shared old_edge old_suffix rest].
    + destruct (endpoint_eqb endpoint before); [|discriminate editing].
      injection editing as edited_is _. subst edited. exact ready.
    + destruct (edit_slots (fun child => edit_edited child rest before after)
        (fresh_edit_leaf rest before after) edge children) as [[updated child_loads]|] eqn:slots;
        [|discriminate editing].
      injection editing as edited_is _. subst edited.
      cbn [stored_admitted].
      apply (edit_slots_admitted (fun child => edit_edited child rest before after)
        (fresh_edit_leaf rest before after) edge children updated child_loads ready).
      * intros other child edited' child_loads' member child_edit.
        rewrite Forall_forall in IH.
        apply (IH (other, child) member rest before after edited' child_loads');
          [|exact child_edit].
        apply stored_admitted_ready. exact (slots_hold_member _ _ other child ready member).
      * exact (fresh_edit_leaf_admitted rest before after).
      * exact slots.
    + apply (split_slots_admitted shared old_edge (EditedNode old_suffix endpoint children)
        rest before after edited loads); [|exact editing].
      exact ready.
Qed.

Lemma edit_batch_counted_admitted :
  forall batch tree edited loads,
    stored_admitted tree -> edit_batch_counted tree batch = Some (edited, loads) ->
    stored_admitted edited.
Proof.
  induction batch as [|[[key before] after] rest IH]; intros tree edited loads admitted editing;
    cbn [edit_batch_counted] in editing.
  - injection editing as edited_is _. subst edited. exact admitted.
  - destruct (edit_edited tree key before after) as [[next next_loads]|] eqn:step;
      [|discriminate editing].
    destruct (edit_batch_counted next rest) as [[final later]|] eqn:later_edit;
      [|discriminate editing].
    injection editing as edited_is _. subst edited.
    apply (IH next final later); [|exact later_edit].
    exact (edit_edited_admitted tree key before after next next_loads
      (stored_admitted_ready tree admitted) step).
Qed.

Lemma counted_batch_admitted :
  forall root mutation rest edited loads,
    admitted_root root ->
    edit_batch_counted (StoredTree root) (mutation :: rest) = Some (edited, loads) ->
    stored_admitted edited.
Proof.
  intros root [[key before] after] rest edited loads admitted editing.
  cbn [edit_batch_counted] in editing.
  destruct (edit_edited (StoredTree root) key before after) as [[next next_loads]|] eqn:step;
    [|discriminate editing].
  destruct (edit_batch_counted next rest) as [[final later]|] eqn:later_edit;
    [|discriminate editing].
  injection editing as edited_is _. subst edited.
  apply (edit_batch_counted_admitted rest next final later); [|exact later_edit].
  exact (edit_edited_admitted (StoredTree root) key before after next next_loads admitted step).
Qed.

Lemma admitted_node_finalizes_to_itself :
  forall tree, admitted_node tree -> finalize_node tree = Some tree.
Proof.
  intros tree. induction tree as [prefix endpoint children IH] using radix_tree_induction.
  intros shaped. cbn [admitted_node] in shaped. destruct shaped as (_ & branching & child_shapes).
  rewrite all_children_iff in child_shapes. rewrite Forall_forall in IH.
  assert (kept : finalize_children finalize_node children = children).
  { apply finalize_children_identity. intros edge child member.
    exact (IH (edge, child) member (child_shapes edge child member)). }
  cbn [finalize_node]. rewrite kept.
  destruct endpoint as [claim|]; [reflexivity|].
  destruct children as [|[edge child] [|second later]].
  - specialize (branching eq_refl). cbn [length] in branching. lia.
  - specialize (branching eq_refl). cbn [length] in branching. lia.
  - destruct child as [child_prefix child_endpoint child_children]. reflexivity.
Qed.

Lemma finalize_slots_image :
  forall children kept report,
    Forall (fun child => forall result report, stored_admitted (snd child) ->
      finalize_counted (snd child) = (result, report) ->
      option_map resolved_tree result = finalize_node (edit_image (snd child))) children ->
    slots_hold stored_admitted children ->
    finalize_slots finalize_counted children = (kept, report) ->
    resolved_children kept = finalize_children finalize_node (image_children edit_image children).
Proof.
  induction children as [|[edge child] rest IH]; intros kept report every admitted slots.
  - cbn [finalize_slots] in slots. injection slots as kept_is _. subst kept. reflexivity.
  - cbn [finalize_slots] in slots. cbn [slots_hold] in admitted.
    destruct admitted as [child_admitted rest_admitted].
    cbn [image_children finalize_children].
    destruct (finalize_slots finalize_counted rest) as [later rest_report] eqn:rest_slots.
    pose proof (IH later rest_report (Forall_inv_tail every) rest_admitted eq_refl) as rest_image.
    destruct (finalize_counted child) as [child_result child_report] eqn:child_counted.
    pose proof (Forall_inv every) as child_image. cbn [snd] in child_image.
    rewrite <- (child_image child_result child_report child_admitted child_counted).
    destruct child_result as [result|]; cbv beta iota in slots;
      injection slots as kept_is _; subst kept;
      cbn [option_map resolved_children]; rewrite rest_image; reflexivity.
Qed.

Lemma finalize_contents_image :
  forall prefix endpoint kept result own,
    finalize_contents prefix endpoint kept = (result, own) ->
    option_map resolved_tree result = merge_unary prefix endpoint (resolved_children kept).
Proof.
  intros prefix endpoint kept result own contents.
  destruct endpoint as [claim|].
  - cbn [finalize_contents] in contents. injection contents as result_is own_is.
    subst result. reflexivity.
  - destruct kept as [|[edge [stored|node]] [|second later]];
      cbn [finalize_contents] in contents; injection contents as result_is own_is;
      subst result.
    + reflexivity.
    + destruct stored as [child_prefix child_endpoint child_children]. reflexivity.
    + destruct stored as [child_prefix child_endpoint child_children].
      destruct second as [second_edge second_child]. reflexivity.
    + destruct node as [child_prefix child_endpoint child_children]. reflexivity.
    + destruct node as [child_prefix child_endpoint child_children].
      destruct second as [second_edge second_child]. reflexivity.
Qed.

Lemma finalize_counted_image :
  forall tree result report,
    stored_admitted tree -> finalize_counted tree = (result, report) ->
    option_map resolved_tree result = finalize_node (edit_image tree).
Proof.
  intros tree. induction tree as [stored|prefix endpoint children IH] using edit_tree_induction;
    intros result report admitted counted.
  - cbn [finalize_counted] in counted. injection counted as result_is _. subst result.
    cbn [stored_admitted] in admitted. cbn [option_map resolved_tree edit_image].
    symmetry. exact (admitted_node_finalizes_to_itself stored admitted).
  - cbn [finalize_counted] in counted. cbn [stored_admitted] in admitted.
    cbn [edit_image finalize_node].
    destruct (finalize_slots finalize_counted children) as [kept inner] eqn:slots.
    destruct (finalize_contents prefix endpoint kept) as [content_result own] eqn:contents.
    cbv beta iota in counted. injection counted as result_is _. subst result.
    rewrite (finalize_contents_image prefix endpoint kept content_result own contents).
    f_equal. exact (finalize_slots_image children kept inner IH admitted slots).
Qed.

Lemma finalize_root_image :
  forall tree final report,
    stored_admitted tree -> finalize_root_counted tree = (final, report) ->
    finalize_root (edit_image tree) = final.
Proof.
  intros tree final report admitted rooted. unfold finalize_root_counted in rooted.
  unfold finalize_root.
  destruct (finalize_counted tree) as [result inner] eqn:counted.
  rewrite <- (finalize_counted_image tree result inner admitted counted).
  destruct result as [[stored|node]|]; cbv beta iota in rooted;
    injection rooted as final_is _; cbn [option_map resolved_tree]; exact final_is.
Qed.

(** Capstone: on an admitted root, the counted rewrite computes the editor's
    [radix_edit] and refuses exactly when it refuses. *)
Theorem counted_rewrite_is_the_radix_edit :
  forall root batch,
    admitted_root root ->
    match counted_rewrite root batch with
    | Some (final, _, _) => radix_edit root batch = Some final
    | None => radix_edit root batch = None
    end.
Proof.
  intros root batch admitted.
  destruct batch as [|mutation rest]; [reflexivity|].
  pose proof (edit_batch_counted_image (mutation :: rest) (StoredTree root)) as image.
  cbn [edit_image] in image.
  cbn [counted_rewrite radix_edit].
  destruct (edit_batch_counted (StoredTree root) (mutation :: rest))
    as [[edited opens]|] eqn:editing.
  - cbn [result_image] in image. rewrite <- image.
    destruct (finalize_root_counted edited) as [final report] eqn:rooted.
    cbv beta iota. f_equal.
    exact (finalize_root_image edited final report
      (counted_batch_admitted root mutation rest edited opens admitted editing) rooted).
  - cbn [result_image] in image. rewrite <- image. cbv beta iota. reflexivity.
Qed.

(** * The trusted summary is the cold walk *)

(** The per-namespace counters of the Rust summary.  The key-length multiset
    is shared by the three namespaces and is tracked separately. *)
Record namespace_summary : Type := NamespaceSummary {
  summary_logical_nodes : nat;
  summary_endpoints : nat;
  summary_payload_bytes : nat
}.

Definition cold_namespace_summary (payload : nat -> nat) (tree : radix_tree) : namespace_summary :=
  NamespaceSummary (logical_nodes tree) (length (tree_entries tree))
    (entry_weight (fun entry => payload (snd entry)) (tree_entries tree)).

Definition apply_namespace (encoded_len : nat -> nat) (summary : namespace_summary)
    (batch : list cas_mutation) (loaded placed : nat) : option namespace_summary :=
  match checked_delta (summary_logical_nodes summary) placed loaded,
        checked_delta (summary_endpoints summary) (length (added_entries batch))
          (length (removed_entries batch)),
        checked_delta (summary_payload_bytes summary)
          (entry_weight (fun entry => reference_payload encoded_len (snd entry))
             (added_entries batch))
          (entry_weight (fun entry => reference_payload encoded_len (snd entry))
             (removed_entries batch)) with
  | Some logical, Some endpoints, Some payload_bytes =>
      Some (NamespaceSummary logical endpoints payload_bytes)
  | _, _, _ => None
  end.

(** Capstone: the trusted successor's summary, derived from the predecessor's
    cold summary, the batch and the rewrite's loads and placements, is the
    cold summary of the successor; the shared key-length multiset tracks the
    successor's keys beside the other namespaces' keys, and its maximum is the
    walk's maximum.  The rewrite's entry denotation is an explicit argument. *)
Theorem trusted_shape_summary_equals_the_cold_walk :
  forall (encoded_len payload : nat -> nat) root batch final opens report after lengths others,
    admitted_root root ->
    counted_rewrite root batch = Some (final, opens, report) ->
    entries_batch (tree_entries root) batch = Some after ->
    admitted_root final ->
    Permutation (tree_entries final) after ->
    batch_claims_are_blobs encoded_len payload batch ->
    Permutation lengths (key_lengths (tree_entries root) ++ others) ->
    apply_namespace encoded_len (cold_namespace_summary payload root) batch
      (opens + merge_loads report) (length (placed_nodes report)) =
      Some (cold_namespace_summary payload final) /\
    exists updated,
      apply_lengths lengths batch = Some updated /\
      Permutation updated (key_lengths (tree_entries final) ++ others) /\
      list_max updated = Nat.max (path_maximum final) (list_max others).
Proof.
  intros encoded_len payload root batch final opens report after lengths others
    root_admitted rewriting batched final_admitted denotes blobs tracked.
  pose proof (admitted_root_entries_sorted root root_admitted) as sorted.
  destruct (rewrite_moves_logical_positions_exactly root batch final opens report rewriting)
    as [positions _].
  destruct (endpoint_existence_delta_is_exact batch (tree_entries root) after sorted batched)
    as [endpoints _].
  pose proof (payload_delta_is_exact encoded_len payload batch (tree_entries root) after
    sorted batched blobs) as payloads.
  assert (logical : checked_delta (logical_nodes root) (length (placed_nodes report))
    (opens + merge_loads report) = Some (logical_nodes final))
    by (apply checked_delta_exact; lia).
  split.
  - unfold apply_namespace, cold_namespace_summary.
    cbn [summary_logical_nodes summary_endpoints summary_payload_bytes].
    rewrite logical, endpoints, payloads.
    rewrite (Permutation_length denotes), (entry_weight_permutation _ _ _ denotes).
    reflexivity.
  - destruct (key_length_multiset_tracks_the_batch batch (tree_entries root) after lengths others
      sorted batched tracked) as [updated [applied updated_tracked]].
    assert (final_tracked : Permutation updated (key_lengths (tree_entries final) ++ others)).
    { eapply perm_trans; [exact updated_tracked|]. apply Permutation_app_tail.
      unfold key_lengths. apply Permutation_map. apply Permutation_sym. exact denotes. }
    exists updated. split; [exact applied|]. split; [exact final_tracked|].
    rewrite (list_max_permutation _ _ final_tracked), list_max_app.
    rewrite (maximum_logical_path_is_the_longest_key final final_admitted). reflexivity.
Qed.

(** Capstone: over the three namespaces the walk's maximum logical path is the
    maximum of all key lengths. *)
Theorem combined_maximum_is_the_walk_maximum :
  forall data joins continuations,
    admitted_root data -> admitted_root joins -> admitted_root continuations ->
    Nat.max (path_maximum data) (Nat.max (path_maximum joins) (path_maximum continuations)) =
      list_max (key_lengths (tree_entries data ++ tree_entries joins ++
        tree_entries continuations)).
Proof.
  intros data joins continuations data_admitted joins_admitted continuations_admitted.
  rewrite (maximum_logical_path_is_the_longest_key data data_admitted),
    (maximum_logical_path_is_the_longest_key joins joins_admitted),
    (maximum_logical_path_is_the_longest_key continuations continuations_admitted).
  unfold longest_key. rewrite !key_lengths_app, !list_max_app. reflexivity.
Qed.

(** * Negative controls *)

(** A compression merge: deleting one child of an [Absent] root and updating a
    grandchild under the other leaves the root with one child, which absorbs
    its rebuilt child. *)
Definition merge_sample : radix_tree :=
  RadixTree [] None
    [(1, RadixTree [] (Some 1) []);
     (2, RadixTree [] (Some 1) [(3, RadixTree [] (Some 1) [])])].

Definition merge_batch : list cas_mutation := [([1], Some 1, None); ([2; 3], Some 1, Some 2)].

Definition merge_result : radix_tree := RadixTree [2] (Some 1) [(3, RadixTree [] (Some 2) [])].

(** The mutant derives the logical delta from emitted minus loaded nodes. *)
Definition emitted_minus_loaded_mutant (root final : radix_tree) (loaded emitted : nat) : Prop :=
  logical_nodes final + loaded = logical_nodes root + emitted.

(** Capstone (negative control): an absorbed node is emitted but never placed,
    so emitted minus loaded nodes miscounts the merge while placed minus
    loaded nodes is exact. *)
Theorem emitted_minus_loaded_mutant_miscounts_a_compression_merge :
  admitted_root merge_sample /\
  counted_rewrite merge_sample merge_batch =
    Some (merge_result, 4, FinalizeReport 3 1 0 [RadixTree [] (Some 2) []; merge_result]) /\
  radix_edit merge_sample merge_batch = Some merge_result /\
  logical_nodes merge_result + (4 + 0) = logical_nodes merge_sample + 2 /\
  ~ emitted_minus_loaded_mutant merge_sample merge_result (4 + 0) 3.
Proof.
  split.
  { right. cbn [merge_sample admitted_node all_children length]. unfold edges_sorted.
    repeat split; try (intros absent; discriminate absent); try (intros _; lia);
      repeat constructor. }
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  unfold emitted_minus_loaded_mutant. intro same. vm_compute in same. discriminate same.
Qed.

(** Two equal leaves inserted under the empty root are two placements of one
    content-addressed object. *)
Definition equal_leaf : radix_tree := RadixTree [] (Some 5) [].

Definition equal_leaves_batch : list cas_mutation := [([1], None, Some 5); ([2], None, Some 5)].

Definition equal_leaves_result : radix_tree :=
  RadixTree [] None [(1, equal_leaf); (2, equal_leaf)].

(** The mutant counts the distinct placed objects, as [inserted_objects]
    does after [put_if_absent_or_equal] deduplicates by digest. *)
Definition digest_dedup_counter_mutant (root final : radix_tree) (loaded : nat)
    (placed : list radix_tree) : Prop :=
  logical_nodes final + loaded = logical_nodes root + length (distinct_trees placed).

(** Capstone (negative control): a digest-deduplicating counter undercounts
    equal leaves; the placement count is exact. *)
Theorem digest_dedup_counter_mutant_undercounts_equal_leaves :
  admitted_root empty_tree /\
  counted_rewrite empty_tree equal_leaves_batch =
    Some (equal_leaves_result, 1,
      FinalizeReport 3 0 0 [equal_leaf; equal_leaf; equal_leaves_result]) /\
  radix_edit empty_tree equal_leaves_batch = Some equal_leaves_result /\
  logical_nodes equal_leaves_result + (1 + 0) = logical_nodes empty_tree + 3 /\
  length (distinct_trees [equal_leaf; equal_leaf; equal_leaves_result]) = 2 /\
  ~ digest_dedup_counter_mutant empty_tree equal_leaves_result (1 + 0)
      [equal_leaf; equal_leaf; equal_leaves_result].
Proof.
  split; [left; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  unfold digest_dedup_counter_mutant. intro same. vm_compute in same. discriminate same.
Qed.

(** Deleting the longest key. *)
Definition running_sample : radix_tree := RadixTree [1] (Some 1) [(2, RadixTree [] (Some 1) [])].

Definition running_batch : list cas_mutation := [([1; 2], Some 1, None)].

Definition running_result : radix_tree := RadixTree [1] (Some 1) [].

(** The mutant keeps a running maximum without the multiset: additions can
    raise it, deletions cannot lower it. *)
Definition running_maximum_mutant (previous : nat) (batch : list cas_mutation) : nat :=
  list_max (previous :: key_lengths (added_entries batch)).

(** Capstone (negative control): a maximum kept without a multiset keeps a
    deleted longest key, while the multiset's maximum is the walk's. *)
Theorem running_maximum_mutant_keeps_a_deleted_longest_key :
  admitted_root running_sample /\
  radix_edit running_sample running_batch = Some running_result /\
  entries_batch (tree_entries running_sample) running_batch = Some (tree_entries running_result) /\
  path_maximum running_sample = 2 /\
  path_maximum running_result = 1 /\
  apply_lengths (key_lengths (tree_entries running_sample)) running_batch = Some [1] /\
  list_max [1] = path_maximum running_result /\
  running_maximum_mutant (path_maximum running_sample) running_batch <>
    path_maximum running_result.
Proof.
  split.
  { right. cbn [running_sample admitted_node all_children length]. unfold edges_sorted.
    repeat split; try (intros absent; discriminate absent); try (intros _; lia);
      repeat constructor. }
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  split; [vm_compute; reflexivity|].
  intro same. vm_compute in same. discriminate same.
Qed.

(** An empty leaf below an [Absent] root: the shape the cold reader refuses. *)
Definition empty_leaf_sample : radix_tree :=
  RadixTree [] None [(1, RadixTree [] (Some 1) []); (2, RadixTree [7; 7] None [])].

(** Capstone (negative control): without the no-empty-leaf rule the walk's
    maximum logical path exceeds the longest key. *)
Theorem empty_leaf_mutant_breaks_the_path_maximum :
  ~ no_empty_leaf empty_leaf_sample /\
  ~ admitted_root empty_leaf_sample /\
  longest_key (tree_entries empty_leaf_sample) = 1 /\
  path_maximum empty_leaf_sample = 3.
Proof.
  assert (empty_leaf : ~ no_empty_leaf empty_leaf_sample).
  { unfold empty_leaf_sample. cbn [no_empty_leaf all_children].
    intros (_ & absent & _). apply absent. reflexivity. }
  split; [exact empty_leaf|].
  split.
  { intros [same|shaped].
    - unfold empty_leaf_sample, empty_tree in same. discriminate same.
    - exact (empty_leaf (admitted_tries_have_no_empty_leaf _ shaped)). }
  split; vm_compute; reflexivity.
Qed.

End ExactStateShapeSummary.

Print Assumptions ExactStateShapeSummary.admitted_tries_have_no_empty_leaf.
Print Assumptions ExactStateShapeSummary.maximum_logical_path_is_the_longest_key.
Print Assumptions ExactStateShapeSummary.batch_weight_delta_is_exact.
Print Assumptions ExactStateShapeSummary.endpoint_existence_delta_is_exact.
Print Assumptions ExactStateShapeSummary.payload_delta_is_exact.
Print Assumptions ExactStateShapeSummary.key_length_multiset_tracks_the_batch.
Print Assumptions ExactStateShapeSummary.largest_nonempty_bucket_is_the_longest_key.
Print Assumptions ExactStateShapeSummary.rewrite_moves_logical_positions_exactly.
Print Assumptions ExactStateShapeSummary.absorbed_reports_move_positions_exactly.
Print Assumptions ExactStateShapeSummary.counted_rewrite_is_the_radix_edit.
Print Assumptions ExactStateShapeSummary.trusted_shape_summary_equals_the_cold_walk.
Print Assumptions ExactStateShapeSummary.combined_maximum_is_the_walk_maximum.
Print Assumptions ExactStateShapeSummary.emitted_minus_loaded_mutant_miscounts_a_compression_merge.
Print Assumptions ExactStateShapeSummary.digest_dedup_counter_mutant_undercounts_equal_leaves.
Print Assumptions ExactStateShapeSummary.running_maximum_mutant_keeps_a_deleted_longest_key.
Print Assumptions ExactStateShapeSummary.empty_leaf_mutant_breaks_the_path_maximum.
