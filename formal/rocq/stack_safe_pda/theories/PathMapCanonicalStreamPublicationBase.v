(**
  Canonical, stack-safe publication for byte-keyed PathMap roots.

  Lists in this development are ghost observations of already ordered trie
  cursors.  They are not an implementation prescription to flatten PathMap,
  allocate a [Vec], or sort a projection.  The physical refinement target is a
  forward source/removal/replacement cursor and one moving destination zipper.
*)

From Stdlib Require Import List Arith Bool Lia PeanoNat NArith.

Import ListNotations.
Open Scope nat_scope.

Definition Byte : Type := nat.
Definition byte_valid (byte : Byte) : bool := byte <=? 255.
Definition Path : Type := list Byte.
Definition path_valid (path : Path) : bool := forallb byte_valid path.

Lemma byte_valid_spec :
  forall byte, byte_valid byte = true <-> byte <= 255.
Proof.
  intro byte. unfold byte_valid. apply Nat.leb_le.
Qed.
Inductive path_lex_lt : Path -> Path -> Prop :=
| PathLexNil : forall byte tail, path_lex_lt [] (byte :: tail)
| PathLexHead : forall left right left_tail right_tail,
    left < right ->
    path_lex_lt (left :: left_tail) (right :: right_tail)
| PathLexTail : forall byte left_tail right_tail,
    path_lex_lt left_tail right_tail ->
    path_lex_lt (byte :: left_tail) (byte :: right_tail).

Theorem path_lex_lt_irreflexive :
  forall path, ~ path_lex_lt path path.
Proof.
  induction path as [|byte tail IH]; intro Hlt; inversion Hlt; subst.
  - lia.
  - apply IH. assumption.
Qed.

Theorem path_lex_lt_transitive :
  forall left middle right,
    path_lex_lt left middle ->
    path_lex_lt middle right ->
    path_lex_lt left right.
Proof.
  intros left middle right Hleft.
  revert right.
  induction Hleft; intros right_path Hright; inversion Hright; subst.
  - apply PathLexNil.
  - apply PathLexNil.
  - apply PathLexHead. lia.
  - apply PathLexHead. assumption.
  - apply PathLexHead. assumption.
  - apply PathLexTail. eapply IHHleft. eassumption.
Qed.

Theorem path_lex_order_total :
  forall left right,
    left = right \/ path_lex_lt left right \/ path_lex_lt right left.
Proof.
  induction left as [|left_byte left_tail IH];
    destruct right as [|right_byte right_tail].
  - left. reflexivity.
  - right. left. apply PathLexNil.
  - right. right. apply PathLexNil.
  - destruct (Nat.lt_trichotomy left_byte right_byte)
      as [Hlt | [Heq | Hgt]].
    + right. left. apply PathLexHead. exact Hlt.
    + subst right_byte.
      specialize (IH right_tail).
      destruct IH as [Heq_tail | [Hlt_tail | Hgt_tail]].
      * left. now subst right_tail.
      * right. left. apply PathLexTail. exact Hlt_tail.
      * right. right. apply PathLexTail. exact Hgt_tail.
    + right. right. apply PathLexHead. exact Hgt.
Qed.

Fixpoint path_compare (left right : Path) : comparison :=
  match left, right with
  | [], [] => Eq
  | [], _ :: _ => Lt
  | _ :: _, [] => Gt
  | left_byte :: left_tail, right_byte :: right_tail =>
      match Nat.compare left_byte right_byte with
      | Eq => path_compare left_tail right_tail
      | Lt => Lt
      | Gt => Gt
      end
  end.

Definition path_ltb (left right : Path) : bool :=
  match path_compare left right with
  | Lt => true
  | _ => false
  end.

Fixpoint path_eqb (left right : Path) : bool :=
  match left, right with
  | [], [] => true
  | left_byte :: left_tail, right_byte :: right_tail =>
      (left_byte =? right_byte) && path_eqb left_tail right_tail
  | _, _ => false
  end.

Lemma path_compare_reflexive :
  forall path, path_compare path path = Eq.
Proof.
  induction path as [|byte tail IH]; simpl.
  - reflexivity.
  - rewrite Nat.compare_refl. exact IH.
Qed.

Lemma path_eqb_reflexive :
  forall path, path_eqb path path = true.
Proof.
  induction path as [|byte tail IH]; simpl.
  - reflexivity.
  - rewrite Nat.eqb_refl, IH. reflexivity.
Qed.

Lemma path_eqb_spec :
  forall left right, path_eqb left right = true <-> left = right.
Proof.
  induction left as [|left_byte left_tail IH];
    destruct right as [|right_byte right_tail]; simpl.
  - split; reflexivity.
  - split; discriminate.
  - split; discriminate.
  - rewrite andb_true_iff, Nat.eqb_eq, IH.
    split.
    + intros [Hhead Htail]. now subst right_byte right_tail.
    + intro Heq. inversion Heq. auto.
Qed.

Lemma path_compare_eq_iff :
  forall left right, path_compare left right = Eq <-> left = right.
Proof.
  induction left as [|left_byte left_tail IH];
    destruct right as [|right_byte right_tail]; simpl.
  - split; reflexivity.
  - split; discriminate.
  - split; discriminate.
  - destruct (Nat.compare left_byte right_byte) eqn:Hhead; simpl.
    + apply Nat.compare_eq in Hhead. subst right_byte.
      rewrite IH. split; intro H;
        [now subst right_tail | inversion H; reflexivity].
    + split.
      * discriminate.
      * intro Heq. inversion Heq. subst right_byte.
        rewrite Nat.compare_refl in Hhead. discriminate.
    + split.
      * discriminate.
      * intro Heq. inversion Heq. subst right_byte.
        rewrite Nat.compare_refl in Hhead. discriminate.
Qed.

Lemma path_compare_lt_iff :
  forall left right, path_compare left right = Lt <-> path_lex_lt left right.
Proof.
  induction left as [|left_byte left_tail IH];
    destruct right as [|right_byte right_tail]; simpl.
  - split; intro H; [discriminate | inversion H].
  - split; intro H; [apply PathLexNil | reflexivity].
  - split; intro H; [discriminate | inversion H].
  - destruct (Nat.compare left_byte right_byte) eqn:Hhead.
    + apply Nat.compare_eq in Hhead. subst right_byte.
      rewrite IH. split; intro H.
      * apply PathLexTail. exact H.
      * inversion H; subst; try lia; assumption.
    + apply Nat.compare_lt_iff in Hhead.
      split; intro H.
      * apply PathLexHead. exact Hhead.
      * reflexivity.
    + apply Nat.compare_gt_iff in Hhead.
      split; intro H.
      * discriminate.
      * inversion H; subst; lia.
Qed.

Lemma path_compare_gt_iff :
  forall left right, path_compare left right = Gt <-> path_lex_lt right left.
Proof.
  induction left as [|left_byte left_tail IH];
    destruct right as [|right_byte right_tail]; simpl.
  - split; intro H; [discriminate | inversion H].
  - split; intro H; [discriminate | inversion H].
  - split; intro H; [apply PathLexNil | reflexivity].
  - destruct (Nat.compare left_byte right_byte) eqn:Hhead.
    + apply Nat.compare_eq in Hhead. subst right_byte.
      rewrite IH. split; intro H.
      * apply PathLexTail. exact H.
      * inversion H; subst; try lia; assumption.
    + apply Nat.compare_lt_iff in Hhead.
      split; intro H.
      * discriminate.
      * inversion H; subst; lia.
    + apply Nat.compare_gt_iff in Hhead.
      split; intro H.
      * apply PathLexHead. exact Hhead.
      * reflexivity.
Qed.

Corollary path_ltb_spec :
  forall left right, path_ltb left right = true <-> path_lex_lt left right.
Proof.
  intros left right. unfold path_ltb.
  destruct (path_compare left right) eqn:Hcompare; simpl.
  - split; intro H; [discriminate |].
    exfalso. apply (path_lex_lt_irreflexive right).
    apply path_compare_eq_iff in Hcompare. now subst left.
  - split; intro H.
    + now apply path_compare_lt_iff.
    + reflexivity.
  - split; intro H; [discriminate |].
    exfalso.
    pose proof (path_lex_order_total left right) as Htotal.
    destruct Htotal as [Heq | [Hlt | Hgt]].
    + subst right. exact (path_lex_lt_irreflexive _ H).
    + pose proof (proj2 (path_compare_lt_iff left right) Hlt) as Hcompare_lt.
      rewrite Hcompare in Hcompare_lt. discriminate.
    + exact (path_lex_lt_irreflexive _
        (path_lex_lt_transitive _ _ _ H Hgt)).
Qed.

Fixpoint path_prefixb (prefix path : Path) : bool :=
  match prefix, path with
  | [], _ => true
  | _ :: _, [] => false
  | prefix_byte :: prefix_tail, path_byte :: path_tail =>
      (prefix_byte =? path_byte) && path_prefixb prefix_tail path_tail
  end.

Lemma path_prefixb_reflexive :
  forall path, path_prefixb path path = true.
Proof.
  induction path as [|byte tail IH]; simpl.
  - reflexivity.
  - rewrite Nat.eqb_refl, IH. reflexivity.
Qed.

Inductive PrefixPosition : Type :=
| PrefixBelow
| PrefixInside
| PrefixAbove.

(** Direct classification has no computed upper bound: below is strict
    lexicographic order, inside is [prefix-of] including equality, and every
    remaining key is above. *)
Definition classify_prefix (prefix key : Path) : PrefixPosition :=
  if path_ltb key prefix then PrefixBelow
  else if path_prefixb prefix key then PrefixInside
  else PrefixAbove.

Theorem empty_prefix_contains_every_path :
  forall key, classify_prefix [] key = PrefixInside.
Proof.
  destruct key; reflexivity.
Qed.

Theorem a_path_is_inside_its_own_prefix :
  forall path, classify_prefix path path = PrefixInside.
Proof.
  intro path. unfold classify_prefix, path_ltb.
  rewrite path_compare_reflexive. simpl.
  rewrite path_prefixb_reflexive. reflexivity.
Qed.

Example all_ff_prefix_contains_itself :
  classify_prefix [255; 255] [255; 255] = PrefixInside.
Proof. reflexivity. Qed.

Example all_ff_prefix_contains_descendants :
  classify_prefix [255; 255] [255; 255; 0; 255] = PrefixInside.
Proof. reflexivity. Qed.

Example all_ff_prefix_classifies_a_valid_divergence_below :
  classify_prefix [255; 255] [255; 254; 255] = PrefixBelow.
Proof. reflexivity. Qed.

Theorem valid_path_is_never_above_single_ff_prefix :
  forall key,
    path_valid key = true ->
    classify_prefix [255] key <> PrefixAbove.
Proof.
  destruct key as [|byte tail]; intro Hvalid; simpl.
  - discriminate.
  - apply andb_true_iff in Hvalid as [Hbyte _].
    apply byte_valid_spec in Hbyte.
    destruct (Nat.compare byte 255) eqn:Hcompare.
    + apply Nat.compare_eq in Hcompare. subst byte.
      destruct tail; discriminate.
    + unfold classify_prefix, path_ltb. simpl. rewrite Hcompare. discriminate.
    + apply Nat.compare_gt_iff in Hcompare. lia.
Qed.

Definition Multiplicity : Type := N.
Definition multiplicity_max : Multiplicity := N.ones 64.

Definition multiplicity_valid (value : Multiplicity) : bool :=
  N.ltb (0%N) value && N.leb value multiplicity_max.

Definition checked_add_multiplicity
    (left right : Multiplicity) : option Multiplicity :=
  let sum := N.add left right in
  if N.ltb (0%N) sum && N.leb sum multiplicity_max then Some sum else None.

Definition checked_subtract_multiplicity
    (available removed : Multiplicity) : option Multiplicity :=
  if N.leb removed available then
    if N.eqb removed available
    then Some 0%N
    else Some (N.sub available removed)
  else None.

Definition Entry : Type := (Path * Multiplicity)%type.
Definition entry_path (entry : Entry) : Path := fst entry.
Definition entry_multiplicity (entry : Entry) : Multiplicity := snd entry.

Fixpoint entries_eq_dec (left right : list Entry) : {left = right} + {left <> right}.
Proof.
  repeat decide equality.
Defined.

Inductive canonical_entries : list Entry -> Prop :=
| CanonicalEntriesNil : canonical_entries []
| CanonicalEntriesCons : forall path multiplicity tail,
    path_valid path = true ->
    multiplicity_valid multiplicity = true ->
    Forall (fun entry => path_lex_lt path (entry_path entry)) tail ->
    canonical_entries tail ->
    canonical_entries ((path, multiplicity) :: tail).

Lemma canonical_entries_tail :
  forall head tail, canonical_entries (head :: tail) -> canonical_entries tail.
Proof.
  intros head tail Hcanonical. inversion Hcanonical; subst. assumption.
Qed.

Lemma canonical_entries_head_positive :
  forall path multiplicity tail,
    canonical_entries ((path, multiplicity) :: tail) ->
    multiplicity_valid multiplicity = true.
Proof.
  intros path multiplicity tail Hcanonical. inversion Hcanonical; assumption.
Qed.

Lemma canonical_entries_have_valid_paths :
  forall entries,
    canonical_entries entries ->
    Forall (fun entry => path_valid (entry_path entry) = true) entries.
Proof.
  intros entries Hcanonical.
  induction Hcanonical; constructor; assumption.
Qed.

Fixpoint lookup_count (path : Path) (entries : list Entry) : Multiplicity :=
  match entries with
  | [] => 0%N
  | entry :: tail =>
      if path_eqb path (entry_path entry)
      then entry_multiplicity entry
      else lookup_count path tail
  end.

Fixpoint checked_cardinality_from
    (accumulated : Multiplicity) (entries : list Entry)
    : option Multiplicity :=
  match entries with
  | [] => Some accumulated
  | entry :: tail =>
      match checked_add_multiplicity accumulated (entry_multiplicity entry) with
      | None => None
      | Some next => checked_cardinality_from next tail
      end
  end.

Definition checked_cardinality (entries : list Entry) : option Multiplicity :=
  checked_cardinality_from 0%N entries.

Record Metadata : Type := {
  distinct_entries : nat;
  total_cardinality : Multiplicity
}.

Definition checked_metadata (entries : list Entry) : option Metadata :=
  match checked_cardinality entries with
  | Some cardinality =>
      Some {| distinct_entries := length entries;
              total_cardinality := cardinality |}
  | None => None
  end.

Lemma checked_add_result_is_bounded :
  forall left right result,
    checked_add_multiplicity left right = Some result ->
    N.le result multiplicity_max.
Proof.
  intros left right result Hchecked.
  unfold checked_add_multiplicity in Hchecked.
  destruct
    (N.ltb (0%N) (N.add left right) &&
     N.leb (N.add left right) multiplicity_max)
    eqn:Hvalid; [|discriminate].
  inversion Hchecked. subst result.
  apply andb_true_iff in Hvalid as [_ Hbound].
  now apply N.leb_le.
Qed.

Lemma checked_cardinality_from_is_bounded :
  forall entries accumulated result,
    N.le accumulated multiplicity_max ->
    checked_cardinality_from accumulated entries = Some result ->
    N.le result multiplicity_max.
Proof.
  induction entries as [|entry tail IH];
    intros accumulated result Haccumulated Hchecked; simpl in Hchecked.
  - injection Hchecked as Hresult. subst result. exact Haccumulated.
  - destruct (checked_add_multiplicity accumulated (entry_multiplicity entry))
      as [next|] eqn:Hnext.
    + eapply IH.
      * eapply checked_add_result_is_bounded.
        exact Hnext.
      * exact Hchecked.
    + discriminate.
Qed.

Theorem checked_metadata_is_bounded_and_exact :
  forall entries metadata,
    checked_metadata entries = Some metadata ->
    distinct_entries metadata = length entries /\
    N.le (total_cardinality metadata) multiplicity_max.
Proof.
  intros entries metadata Hmetadata.
  unfold checked_metadata in Hmetadata.
  destruct (checked_cardinality entries) as [cardinality|] eqn:Hcardinality;
    [|discriminate].
  inversion Hmetadata.
  subst metadata.
  simpl.
  split.
  - reflexivity.
  - unfold checked_cardinality in Hcardinality.
    eapply checked_cardinality_from_is_bounded.
    + apply N.le_0_l.
    + exact Hcardinality.
Qed.

Example checked_subtraction_rejects_underflow :
  checked_subtract_multiplicity (1%N) (2%N) = None.
Proof. reflexivity. Qed.

Example checked_addition_rejects_u64_overflow :
  checked_add_multiplicity multiplicity_max (1%N) = None.
Proof. reflexivity. Qed.

(** A byte trie is represented structurally, including the canonical empty
    root.  The forest is a distinct mutually inductive type so every recursive
    traversal is structural and accepted without a depth bound. *)
Inductive ByteTrie : Type :=
| TrieNode : option Multiplicity -> ByteForest -> ByteTrie
with ByteForest : Type :=
| TrieForestNil : ByteForest
| TrieForestCons : Byte -> ByteTrie -> ByteForest -> ByteForest.

Scheme ByteTrie_ind' := Induction for ByteTrie Sort Prop
with ByteForest_ind' := Induction for ByteForest Sort Prop.
Combined Scheme ByteTrieForest_ind
  from ByteTrie_ind', ByteForest_ind'.

Inductive canonical_trie : ByteTrie -> Prop :=
| CanonicalTrieNode : forall value children,
    (match value with
     | None => True
     | Some multiplicity => multiplicity_valid multiplicity = true
     end) ->
    canonical_forest None children ->
    canonical_trie (TrieNode value children)
with canonical_forest : option Byte -> ByteForest -> Prop :=
| CanonicalForestNil : forall previous,
    canonical_forest previous TrieForestNil
| CanonicalForestCons : forall previous byte child tail,
    byte_valid byte = true ->
    (match previous with None => True | Some prior => prior < byte end) ->
    canonical_trie child ->
    canonical_forest (Some byte) tail ->
    canonical_forest previous (TrieForestCons byte child tail).

Scheme canonical_trie_ind' := Induction for canonical_trie Sort Prop
with canonical_forest_ind' := Induction for canonical_forest Sort Prop.
Combined Scheme canonical_trie_forest_ind
  from canonical_trie_ind', canonical_forest_ind'.

Definition empty_trie : ByteTrie := TrieNode None TrieForestNil.

Theorem empty_trie_is_canonical : canonical_trie empty_trie.
Proof.
  unfold empty_trie. constructor; constructor.
Qed.

Fixpoint trie_entries_at (prefix : Path) (trie : ByteTrie) : list Entry :=
  match trie with
  | TrieNode value children =>
      match value with
      | None => forest_entries_at prefix children
      | Some multiplicity =>
          (prefix, multiplicity) :: forest_entries_at prefix children
      end
  end
with forest_entries_at (prefix : Path) (forest : ByteForest) : list Entry :=
  match forest with
  | TrieForestNil => []
  | TrieForestCons byte child tail =>
      trie_entries_at (prefix ++ [byte]) child ++
      forest_entries_at prefix tail
  end.

Definition trie_entries (trie : ByteTrie) : list Entry :=
  trie_entries_at [] trie.

Inductive ActToken : Type :=
| ActNodeStart
| ActNodeValue (multiplicity : Multiplicity)
| ActEdge (byte : Byte)
| ActNodeEnd.

Fixpoint structural_act_image (trie : ByteTrie) : list ActToken :=
  match trie with
  | TrieNode value children =>
      ActNodeStart ::
      (match value with None => [] | Some count => [ActNodeValue count] end) ++
      structural_act_forest_image children ++
      [ActNodeEnd]
  end
with structural_act_forest_image (forest : ByteForest) : list ActToken :=
  match forest with
  | TrieForestNil => []
  | TrieForestCons byte child tail =>
      ActEdge byte :: structural_act_image child ++
      structural_act_forest_image tail
  end.

Theorem structural_act_image_is_deterministic :
  forall left right,
    left = right ->
    structural_act_image left = structural_act_image right.
Proof. intros left right Heq. now subst right. Qed.

Theorem canonical_empty_root_has_a_structural_act_image :
  structural_act_image empty_trie = [ActNodeStart; ActNodeEnd].
Proof. reflexivity. Qed.

(** The compact observation is structural as well: it contains the ACT node
    image and checked metadata, never a flattened key/value byte sequence. *)
Definition structural_compact_observation (trie : ByteTrie)
    : list ActToken * option Metadata :=
  (structural_act_image trie, checked_metadata (trie_entries trie)).

(** Cursor events are causal observations of a small-step zipper machine.
    There is intentionally no rewind event.  Accounting is computed from the
    trace rather than asserted in a record. *)
Inductive StreamEvent : Type :=
| SourceAdvance (entry : Entry)
| RemovalAdvance (entry : Entry)
| ExactDeleteAdvance (path : Path)
| ReplacementAdvance (entry : Entry)
| DestinationEmit (entry : Entry).

Fixpoint source_event_count (trace : list StreamEvent) : nat :=
  match trace with
  | [] => 0
  | SourceAdvance _ :: tail => S (source_event_count tail)
  | _ :: tail => source_event_count tail
  end.

Fixpoint removal_event_count (trace : list StreamEvent) : nat :=
  match trace with
  | [] => 0
  | RemovalAdvance _ :: tail => S (removal_event_count tail)
  | _ :: tail => removal_event_count tail
  end.

Fixpoint replacement_event_count (trace : list StreamEvent) : nat :=
  match trace with
  | [] => 0
  | ReplacementAdvance _ :: tail => S (replacement_event_count tail)
  | _ :: tail => replacement_event_count tail
  end.

Fixpoint emitted_trace (trace : list StreamEvent) : list Entry :=
  match trace with
  | [] => []
  | DestinationEmit entry :: tail => entry :: emitted_trace tail
  | _ :: tail => emitted_trace tail
  end.

Fixpoint source_projection (trace : list StreamEvent) : list Entry :=
  match trace with
  | [] => []
  | SourceAdvance entry :: tail => entry :: source_projection tail
  | _ :: tail => source_projection tail
  end.

Fixpoint removal_projection (trace : list StreamEvent) : list Entry :=
  match trace with
  | [] => []
  | RemovalAdvance entry :: tail => entry :: removal_projection tail
  | _ :: tail => removal_projection tail
  end.

Fixpoint replacement_projection (trace : list StreamEvent) : list Entry :=
  match trace with
  | [] => []
  | ReplacementAdvance entry :: tail => entry :: replacement_projection tail
  | _ :: tail => replacement_projection tail
  end.

Fixpoint deletion_projection (trace : list StreamEvent) : list Path :=
  match trace with
  | [] => []
  | ExactDeleteAdvance path :: tail => path :: deletion_projection tail
  | _ :: tail => deletion_projection tail
  end.

Fixpoint deletion_event_count (trace : list StreamEvent) : nat :=
  match trace with
  | [] => 0
  | ExactDeleteAdvance _ :: tail => S (deletion_event_count tail)
  | _ :: tail => deletion_event_count tail
  end.

Lemma deletion_event_count_is_projection_length :
  forall trace,
    deletion_event_count trace = length (deletion_projection trace).
Proof.
  induction trace as [|event tail IH]; simpl; [reflexivity|].
  destruct event; simpl; rewrite IH; reflexivity.
Qed.

Lemma source_event_count_is_projection_length :
  forall trace, source_event_count trace = length (source_projection trace).
Proof.
  induction trace as [|event tail IH]; simpl.
  - reflexivity.
  - destruct event; simpl; lia.
Qed.

Lemma removal_event_count_is_projection_length :
  forall trace, removal_event_count trace = length (removal_projection trace).
Proof.
  induction trace as [|event tail IH]; simpl.
  - reflexivity.
  - destruct event; simpl; lia.
Qed.

Lemma replacement_event_count_is_projection_length :
  forall trace,
    replacement_event_count trace = length (replacement_projection trace).
Proof.
  induction trace as [|event tail IH]; simpl.
  - reflexivity.
  - destruct event; simpl; lia.
Qed.

Definition destination_event_count (trace : list StreamEvent) : nat :=
  length (emitted_trace trace).

Lemma destination_event_count_is_projection_length :
  forall trace, destination_event_count trace = length (emitted_trace trace).
Proof. reflexivity. Qed.

Record ExactCursorState : Type := {
  exact_source_remaining : list Entry;
  exact_removals_remaining : list Entry;
  exact_destination_rev : list Entry
}.

Inductive ExactStepResult : Type :=
| ExactProgress (next : ExactCursorState) (events : list StreamEvent)
| ExactFinished
| ExactFault.

Definition exact_cursor_step (state : ExactCursorState) : ExactStepResult :=
  match exact_source_remaining state, exact_removals_remaining state with
  | [], [] => ExactFinished
  | [], removal :: removal_tail =>
      ExactProgress
        {| exact_source_remaining := [];
           exact_removals_remaining := removal_tail;
           exact_destination_rev := exact_destination_rev state |}
        [RemovalAdvance removal]
  | source :: source_tail, [] =>
      ExactProgress
        {| exact_source_remaining := source_tail;
           exact_removals_remaining := [];
           exact_destination_rev := source :: exact_destination_rev state |}
        [SourceAdvance source; DestinationEmit source]
  | source :: source_tail, removal :: removal_tail =>
      match path_compare (entry_path source) (entry_path removal) with
      | Lt =>
          ExactProgress
            {| exact_source_remaining := source_tail;
               exact_removals_remaining := removal :: removal_tail;
               exact_destination_rev := source :: exact_destination_rev state |}
            [SourceAdvance source; DestinationEmit source]
      | Gt =>
          ExactProgress
            {| exact_source_remaining := source :: source_tail;
               exact_removals_remaining := removal_tail;
               exact_destination_rev := exact_destination_rev state |}
            [RemovalAdvance removal]
      | Eq =>
          match checked_subtract_multiplicity
                  (entry_multiplicity source)
                  (entry_multiplicity removal) with
          | None => ExactFault
          | Some (0%N) =>
              ExactProgress
                {| exact_source_remaining := source_tail;
                   exact_removals_remaining := removal_tail;
                   exact_destination_rev := exact_destination_rev state |}
                [SourceAdvance source; RemovalAdvance removal]
          | Some remaining =>
              let output := (entry_path source, remaining) in
              ExactProgress
                {| exact_source_remaining := source_tail;
                   exact_removals_remaining := removal_tail;
                   exact_destination_rev := output :: exact_destination_rev state |}
                [SourceAdvance source; RemovalAdvance removal;
                 DestinationEmit output]
          end
      end
  end.

Inductive CursorRunStatus : Type :=
| CursorComplete
| CursorFault
| CursorFuelExhausted.

Fixpoint run_exact_cursor
    (fuel : nat) (state : ExactCursorState)
    : ExactCursorState * list StreamEvent * CursorRunStatus :=
  match fuel with
  | 0 => (state, [], CursorFuelExhausted)
  | S remaining_fuel =>
      match exact_cursor_step state with
      | ExactFinished => (state, [], CursorComplete)
      | ExactFault => (state, [], CursorFault)
      | ExactProgress next events =>
          let '(final_state, later_events, status) :=
            run_exact_cursor remaining_fuel next in
          (final_state, events ++ later_events, status)
      end
  end.

Definition initial_exact_state
    (source removals : list Entry) : ExactCursorState :=
  {| exact_source_remaining := source;
     exact_removals_remaining := removals;
     exact_destination_rev := [] |}.

Definition exact_cursor_fuel (source removals : list Entry) : nat :=
  S (length source + length removals).

Definition exact_cursor_run (source removals : list Entry) :=
  run_exact_cursor (exact_cursor_fuel source removals)
    (initial_exact_state source removals).

Definition cursor_run_trace
    (run : ExactCursorState * list StreamEvent * CursorRunStatus)
    : list StreamEvent :=
  let '(_, trace, _) := run in trace.

Definition cursor_run_status
    (run : ExactCursorState * list StreamEvent * CursorRunStatus)
    : CursorRunStatus :=
  let '(_, _, status) := run in status.

Theorem exact_step_is_total :
  forall state,
    exact_cursor_step state = ExactFinished \/
    exact_cursor_step state = ExactFault \/
    exists next events,
      exact_cursor_step state = ExactProgress next events.
Proof.
  intro state. destruct (exact_cursor_step state) eqn:Hstep.
  - right. right. eauto.
  - left. reflexivity.
  - right. left. reflexivity.
Qed.

(** Each nonterminal exact step consumes at least one input endpoint. *)
Theorem exact_progress_decreases_remaining :
  forall state next events,
    exact_cursor_step state = ExactProgress next events ->
    length (exact_source_remaining next) +
      length (exact_removals_remaining next) <
    length (exact_source_remaining state) +
      length (exact_removals_remaining state).
Proof.
  intros [source removals destination] next events Hstep.
  unfold exact_cursor_step in Hstep.
  destruct source as [|source source_tail];
    destruct removals as [|removal removal_tail];
    cbn in Hstep.
  - discriminate.
  - inversion Hstep. subst. simpl. lia.
  - inversion Hstep. subst. simpl. lia.
  - destruct (path_compare (entry_path source) (entry_path removal))
      eqn:Hcompare.
    + destruct (checked_subtract_multiplicity
        (entry_multiplicity source) (entry_multiplicity removal))
        as [remaining|] eqn:Hsubtract.
      * destruct remaining as [|positive].
        -- try rewrite Hcompare in Hstep.
           try rewrite Hsubtract in Hstep.
           cbn [exact_cursor_step] in Hstep.
           inversion Hstep. subst.
           clear Hcompare Hsubtract. simpl.
           rewrite Nat.add_succ_r.
           apply Nat.lt_trans with
             (m := S (length source_tail + length removal_tail));
             apply Nat.lt_succ_diag_r.
        -- try rewrite Hcompare in Hstep.
           try rewrite Hsubtract in Hstep.
           cbn [exact_cursor_step] in Hstep.
           inversion Hstep. subst.
           clear Hcompare Hsubtract. simpl.
           rewrite Nat.add_succ_r.
           apply Nat.lt_trans with
             (m := S (length source_tail + length removal_tail));
             apply Nat.lt_succ_diag_r.
      * try rewrite Hcompare in Hstep.
        try rewrite Hsubtract in Hstep.
        cbn [exact_cursor_step] in Hstep. discriminate.
    + try rewrite Hcompare in Hstep.
      cbn [exact_cursor_step] in Hstep.
      inversion Hstep. subst. simpl. lia.
    + try rewrite Hcompare in Hstep.
      cbn [exact_cursor_step] in Hstep.
      inversion Hstep. subst. simpl. lia.
Qed.

Inductive PrefixPhase : Type :=
| PrefixScanning
| PrefixEmittingReplacement
| PrefixCopyingSuffix.

Record PrefixCursorState : Type := {
  prefix_key : Path;
  prefix_source_remaining : list Entry;
  prefix_replacements_remaining : list Entry;
  prefix_destination_rev : list Entry;
  prefix_phase : PrefixPhase
}.

Inductive PrefixStepResult : Type :=
| PrefixProgress (next : PrefixCursorState) (events : list StreamEvent)
| PrefixFinished.

Definition prefix_cursor_step (state : PrefixCursorState) : PrefixStepResult :=
  match prefix_phase state with
  | PrefixScanning =>
      match prefix_source_remaining state with
      | [] =>
          PrefixProgress
            {| prefix_key := prefix_key state;
               prefix_source_remaining := [];
               prefix_replacements_remaining :=
                 prefix_replacements_remaining state;
               prefix_destination_rev := prefix_destination_rev state;
               prefix_phase := PrefixEmittingReplacement |}
            []
      | source :: source_tail =>
          match classify_prefix (prefix_key state) (entry_path source) with
          | PrefixBelow =>
              PrefixProgress
                {| prefix_key := prefix_key state;
                   prefix_source_remaining := source_tail;
                   prefix_replacements_remaining :=
                     prefix_replacements_remaining state;
                   prefix_destination_rev :=
                     source :: prefix_destination_rev state;
                   prefix_phase := PrefixScanning |}
                [SourceAdvance source; DestinationEmit source]
          | PrefixInside =>
              PrefixProgress
                {| prefix_key := prefix_key state;
                   prefix_source_remaining := source_tail;
                   prefix_replacements_remaining :=
                     prefix_replacements_remaining state;
                   prefix_destination_rev := prefix_destination_rev state;
                   prefix_phase := PrefixScanning |}
                [SourceAdvance source]
          | PrefixAbove =>
              PrefixProgress
                {| prefix_key := prefix_key state;
                   prefix_source_remaining := source :: source_tail;
                   prefix_replacements_remaining :=
                     prefix_replacements_remaining state;
                   prefix_destination_rev := prefix_destination_rev state;
                   prefix_phase := PrefixEmittingReplacement |}
                []
          end
      end
  | PrefixEmittingReplacement =>
      match prefix_replacements_remaining state with
      | replacement :: replacement_tail =>
          PrefixProgress
            {| prefix_key := prefix_key state;
               prefix_source_remaining := prefix_source_remaining state;
               prefix_replacements_remaining := replacement_tail;
               prefix_destination_rev :=
                 replacement :: prefix_destination_rev state;
               prefix_phase := PrefixEmittingReplacement |}
            [ReplacementAdvance replacement; DestinationEmit replacement]
      | [] =>
          PrefixProgress
            {| prefix_key := prefix_key state;
               prefix_source_remaining := prefix_source_remaining state;
               prefix_replacements_remaining := [];
               prefix_destination_rev := prefix_destination_rev state;
               prefix_phase := PrefixCopyingSuffix |}
            []
      end
  | PrefixCopyingSuffix =>
      match prefix_source_remaining state with
      | [] => PrefixFinished
      | source :: source_tail =>
          PrefixProgress
            {| prefix_key := prefix_key state;
               prefix_source_remaining := source_tail;
               prefix_replacements_remaining := [];
               prefix_destination_rev :=
                 source :: prefix_destination_rev state;
               prefix_phase := PrefixCopyingSuffix |}
            [SourceAdvance source; DestinationEmit source]
      end
  end.

Fixpoint run_prefix_cursor
    (fuel : nat) (state : PrefixCursorState)
    : PrefixCursorState * list StreamEvent * CursorRunStatus :=
  match fuel with
  | 0 => (state, [], CursorFuelExhausted)
  | S remaining_fuel =>
      match prefix_cursor_step state with
      | PrefixFinished => (state, [], CursorComplete)
      | PrefixProgress next events =>
          let '(final_state, later_events, status) :=
            run_prefix_cursor remaining_fuel next in
          (final_state, events ++ later_events, status)
      end
  end.

Definition initial_prefix_state
    (prefix : Path) (source replacements : list Entry) : PrefixCursorState :=
  {| prefix_key := prefix;
     prefix_source_remaining := source;
     prefix_replacements_remaining := replacements;
     prefix_destination_rev := [];
     prefix_phase := PrefixScanning |}.

Definition prefix_cursor_fuel
    (source replacements : list Entry) : nat :=
  3 + length source + length replacements.

Definition prefix_cursor_run
    (prefix : Path) (source replacements : list Entry) :=
  run_prefix_cursor (prefix_cursor_fuel source replacements)
    (initial_prefix_state prefix source replacements).

Theorem prefix_step_is_total :
  forall state,
    prefix_cursor_step state = PrefixFinished \/
    exists next events,
      prefix_cursor_step state = PrefixProgress next events.
Proof.
  intro state. destruct (prefix_cursor_step state) eqn:Hstep.
  - right. eauto.
  - left. reflexivity.
Qed.

Definition prefix_phase_rank (phase : PrefixPhase) : nat :=
  match phase with
  | PrefixScanning => 2
  | PrefixEmittingReplacement => 1
  | PrefixCopyingSuffix => 0
  end.

Definition prefix_state_measure (state : PrefixCursorState) : nat :=
  length (prefix_source_remaining state) +
  length (prefix_replacements_remaining state) +
  prefix_phase_rank (prefix_phase state).

Theorem prefix_progress_decreases_measure :
  forall state next events,
    prefix_cursor_step state = PrefixProgress next events ->
    prefix_state_measure next < prefix_state_measure state.
Proof.
  intros [prefix source replacements destination phase] next events Hstep.
  unfold prefix_cursor_step in Hstep.
  destruct phase; cbn in Hstep.
  - destruct source as [|source source_tail].
    + inversion Hstep. subst. unfold prefix_state_measure. simpl. lia.
    + destruct (classify_prefix prefix (entry_path source)) eqn:Hclassify;
        try rewrite Hclassify in Hstep; cbn in Hstep;
        inversion Hstep; subst; unfold prefix_state_measure; simpl; lia.
  - destruct replacements as [|replacement replacement_tail];
      inversion Hstep; subst; unfold prefix_state_measure; simpl; lia.
  - destruct source as [|source source_tail]; [discriminate |].
    inversion Hstep. subst. unfold prefix_state_measure. simpl. lia.
Qed.

Lemma prefix_step_emits_at_most_two_events :
  forall state next events,
    prefix_cursor_step state = PrefixProgress next events ->
    length events <= 2.
Proof.
  intros [prefix source replacements destination phase] next events Hstep.
  unfold prefix_cursor_step in Hstep.
  destruct phase; cbn in Hstep.
  - destruct source as [|source source_tail].
    + inversion Hstep. subst. simpl. lia.
    + destruct (classify_prefix prefix (entry_path source)) eqn:Hclassify;
        try rewrite Hclassify in Hstep; cbn in Hstep;
        inversion Hstep; subst; simpl; lia.
  - destruct replacements as [|replacement replacement_tail];
      inversion Hstep; subst; simpl; lia.
  - destruct source as [|source source_tail]; [discriminate |].
    inversion Hstep. subst. simpl. lia.
Qed.

Theorem run_prefix_trace_is_linear_in_fuel :
  forall fuel state final trace status,
    run_prefix_cursor fuel state = (final, trace, status) ->
    length trace <= 2 * fuel.
Proof.
  induction fuel as [|fuel IH]; intros state final trace status Hrun; simpl in Hrun.
  - inversion Hrun. simpl. lia.
  - destruct (prefix_cursor_step state) as [next events|] eqn:Hstep.
    + destruct (run_prefix_cursor fuel next)
        as [[later later_trace] later_status] eqn:Hlater.
      inversion Hrun. subst final trace status.
      rewrite length_app.
      pose proof (prefix_step_emits_at_most_two_events _ _ _ Hstep) as Hbound.
      pose proof (IH _ _ _ _ Hlater) as Hlater_bound. lia.
    + inversion Hrun. simpl. lia.
Qed.

Theorem exact_run_cannot_exhaust_measure_derived_fuel :
  forall fuel state final trace status,
    length (exact_source_remaining state) +
      length (exact_removals_remaining state) < fuel ->
    run_exact_cursor fuel state = (final, trace, status) ->
    status <> CursorFuelExhausted.
Proof.
  induction fuel as [|fuel IH];
    intros state final trace status Hmeasure Hrun; [lia |].
  simpl in Hrun.
  destruct (exact_cursor_step state) as [next events| |] eqn:Hstep.
  - destruct (run_exact_cursor fuel next)
      as [[later later_trace] later_status] eqn:Hlater.
    inversion Hrun. subst final trace status.
    eapply IH; [|exact Hlater].
    pose proof (exact_progress_decreases_remaining _ _ _ Hstep). lia.
  - inversion Hrun. discriminate.
  - inversion Hrun. discriminate.
Qed.

Theorem prefix_run_cannot_exhaust_measure_derived_fuel :
  forall fuel state final trace status,
    prefix_state_measure state < fuel ->
    run_prefix_cursor fuel state = (final, trace, status) ->
    status <> CursorFuelExhausted.
Proof.
  induction fuel as [|fuel IH];
    intros state final trace status Hmeasure Hrun; [lia |].
  simpl in Hrun.
  destruct (prefix_cursor_step state) as [next events|] eqn:Hstep.
  - destruct (run_prefix_cursor fuel next)
      as [[later later_trace] later_status] eqn:Hlater.
    inversion Hrun. subst final trace status.
    eapply IH; [|exact Hlater].
    pose proof (prefix_progress_decreases_measure _ _ _ Hstep). lia.
  - inversion Hrun. discriminate.
Qed.

(** Logical stream functions are the extensional specification.  The physical
    cursor traces above refine them; neither function sorts or materializes a
    PathMap projection. *)
Fixpoint exact_stream_transform
    (source removals : list Entry) : option (list Entry) :=
  match source with
  | [] => Some []
  | source_entry :: source_tail =>
      match checked_subtract_multiplicity
              (entry_multiplicity source_entry)
              (lookup_count (entry_path source_entry) removals) with
      | None => None
      | Some remaining =>
          match exact_stream_transform source_tail removals with
          | None => None
          | Some output_tail =>
              if N.eqb remaining (0%N) then Some output_tail
              else Some ((entry_path source_entry, remaining) :: output_tail)
          end
      end
  end.

Definition prefix_position_eqb
    (left right : PrefixPosition) : bool :=
  match left, right with
  | PrefixBelow, PrefixBelow
  | PrefixInside, PrefixInside
  | PrefixAbove, PrefixAbove => true
  | _, _ => false
  end.

Lemma prefix_position_eqb_spec :
  forall left right,
    prefix_position_eqb left right = true <-> left = right.
Proof.
  destruct left, right; simpl; split; intro H; try reflexivity; discriminate.
Qed.

Fixpoint select_prefix_position
    (position : PrefixPosition)
    (prefix : Path) (source : list Entry) : list Entry :=
  match source with
  | [] => []
  | entry :: tail =>
      if prefix_position_eqb
           (classify_prefix prefix (entry_path entry)) position
      then entry :: select_prefix_position position prefix tail
      else select_prefix_position position prefix tail
  end.

Definition prefix_filter_stream
    (prefix : Path) (source : list Entry) : list Entry :=
  select_prefix_position PrefixBelow prefix source ++
  select_prefix_position PrefixAbove prefix source.

Definition prefix_below_stream := select_prefix_position PrefixBelow.
Definition prefix_above_stream := select_prefix_position PrefixAbove.

Definition prefix_splice_stream
    (prefix : Path) (source replacements : list Entry) : list Entry :=
  prefix_below_stream prefix source ++
  replacements ++
  prefix_above_stream prefix source.

Lemma checked_subtract_value :
  forall available removed remaining,
    checked_subtract_multiplicity available removed = Some remaining ->
    remaining = N.sub available removed.
Proof.
  intros available removed remaining Hsubtract.
  unfold checked_subtract_multiplicity in Hsubtract.
  destruct (N.leb removed available) eqn:Hle; [|discriminate].
  destruct (N.eqb removed available) eqn:Heq.
  - apply N.eqb_eq in Heq. subst removed.
    inversion Hsubtract. symmetry. apply N.sub_diag.
  - inversion Hsubtract. reflexivity.
Qed.

Lemma checked_subtract_positive_valid :
  forall available removed remaining,
    multiplicity_valid available = true ->
    checked_subtract_multiplicity available removed = Some remaining ->
    remaining <> (0%N) ->
    multiplicity_valid remaining = true.
Proof.
  intros available removed remaining Havailable Hsubtract Hpositive.
  unfold checked_subtract_multiplicity in Hsubtract.
  destruct (N.leb removed available) eqn:Hle; [|discriminate].
  destruct (N.eqb removed available) eqn:Heq.
  - inversion Hsubtract. subst remaining. exfalso. apply Hpositive. reflexivity.
  - inversion Hsubtract. subst remaining.
    unfold multiplicity_valid in *.
    apply andb_true_iff in Havailable as [Havailable_positive Havailable_max].
    apply N.leb_le in Hle.
    apply N.eqb_neq in Heq.
    apply andb_true_iff. split.
    + apply N.ltb_lt. nia.
    + apply N.leb_le. apply N.leb_le in Havailable_max. nia.
Qed.

Lemma ordered_head_is_absent_from_tail :
  forall path tail,
    Forall (fun entry => path_lex_lt path (entry_path entry)) tail ->
    lookup_count path tail = 0%N.
Proof.
  intros path tail Hordered. induction Hordered as [|entry tail Hlt Htail IH].
  - reflexivity.
  - simpl. destruct (path_eqb path (entry_path entry)) eqn:Heq.
    + apply path_eqb_spec in Heq. subst.
      exfalso. exact (path_lex_lt_irreflexive _ Hlt).
    + exact IH.
Qed.

Lemma exact_transform_preserves_path_forall :
  forall (predicate : Path -> Prop) source removals output,
    Forall (fun entry => predicate (entry_path entry)) source ->
    exact_stream_transform source removals = Some output ->
    Forall (fun entry => predicate (entry_path entry)) output.
Proof.
  intros predicate source.
  induction source as [|source_entry source_tail IH];
    intros removals output Hforall Htransform; simpl in Htransform.
  - inversion Htransform. constructor.
  - inversion Hforall as [|? ? Hhead Htail]; subst.
    destruct (checked_subtract_multiplicity
      (entry_multiplicity source_entry)
      (lookup_count (entry_path source_entry) removals))
      as [remaining|] eqn:Hsubtract; [|discriminate].
    destruct (exact_stream_transform source_tail removals)
      as [output_tail|] eqn:Htail_transform; [|discriminate].
    destruct (N.eqb remaining (0%N)) eqn:Hzero.
    + inversion Htransform. subst output. eapply IH; eauto.
    + inversion Htransform. subst output. constructor.
      * exact Hhead.
      * eapply IH; eauto.
Qed.

Theorem exact_stream_transform_preserves_canonicality :
  forall source removals output,
    canonical_entries source ->
    exact_stream_transform source removals = Some output ->
    canonical_entries output.
Proof.
  intros source.
  induction source as [|source_entry source_tail IH];
    intros removals output Hcanonical Htransform; simpl in Htransform.
  - inversion Htransform. constructor.
  - destruct source_entry as [source_path source_count].
    cbn in Htransform.
    inversion Hcanonical as
      [|? ? ? Hpath Hcount Hordered Htail_canonical].
    destruct (checked_subtract_multiplicity
      source_count
      (lookup_count source_path removals))
      as [remaining|] eqn:Hsubtract; [|discriminate].
    destruct (exact_stream_transform source_tail removals)
      as [output_tail|] eqn:Htail_transform; [|discriminate].
    destruct (N.eqb remaining 0%N) eqn:Hzero.
    + inversion Htransform. subst output.
      eapply IH; eauto.
    + inversion Htransform. subst output.
      apply CanonicalEntriesCons.
      * exact Hpath.
      * eapply checked_subtract_positive_valid; eauto.
        now apply N.eqb_neq.
      * eapply exact_transform_preserves_path_forall; eauto.
      * eapply IH; eauto.
Qed.

Theorem exact_stream_transform_is_extensionally_correct :
  forall source removals output queried_path,
    canonical_entries source ->
    exact_stream_transform source removals = Some output ->
    lookup_count queried_path output =
      N.sub
        (lookup_count queried_path source)
        (lookup_count queried_path removals).
Proof.
  intros source.
  induction source as [|source_entry source_tail IH];
    intros removals output queried_path Hcanonical Htransform;
    simpl in Htransform.
  - inversion Htransform. reflexivity.
  - destruct source_entry as [source_path source_count].
    cbn in Htransform.
    inversion Hcanonical as
      [|? ? ? Hpath Hcount Hordered Htail_canonical].
    destruct (checked_subtract_multiplicity
      source_count
      (lookup_count source_path removals))
      as [remaining|] eqn:Hsubtract; [|discriminate].
    destruct (exact_stream_transform source_tail removals)
      as [output_tail|] eqn:Htail_transform; [|discriminate].
    destruct (path_eqb queried_path source_path) eqn:Hquery.
    + apply path_eqb_spec in Hquery. subst queried_path.
      cbn [lookup_count].
      rewrite path_eqb_reflexive.
      destruct (N.eqb remaining (0%N)) eqn:Hzero.
      * inversion Htransform. subst output.
        assert (Houtput_ordered :
          Forall
            (fun entry => path_lex_lt source_path (entry_path entry))
            output_tail).
        { eapply exact_transform_preserves_path_forall; eauto. }
        rewrite ordered_head_is_absent_from_tail by exact Houtput_ordered.
        apply N.eqb_eq in Hzero. subst remaining.
        pose proof (checked_subtract_value _ _ _ Hsubtract) as Hvalue.
        exact Hvalue.
      * inversion Htransform. subst output. simpl.
        rewrite path_eqb_reflexive.
        pose proof (checked_subtract_value _ _ _ Hsubtract) as Hvalue.
        exact Hvalue.
    + destruct (N.eqb remaining (0%N)) eqn:Hzero;
        inversion Htransform; subst output; simpl; rewrite Hquery;
        eapply IH; eauto.
Qed.

Lemma select_prefix_position_preserves_path_forall :
  forall (predicate : Path -> Prop) position prefix source,
    Forall (fun entry => predicate (entry_path entry)) source ->
    Forall (fun entry => predicate (entry_path entry))
      (select_prefix_position position prefix source).
Proof.
  intros predicate position prefix source Hforall.
  induction Hforall as [|entry tail Hhead Htail IH]; simpl.
  - constructor.
  - destruct (prefix_position_eqb
      (classify_prefix prefix (entry_path entry)) position).
    + constructor; assumption.
    + exact IH.
Qed.

Theorem select_prefix_position_preserves_canonicality :
  forall position prefix source,
    canonical_entries source ->
    canonical_entries (select_prefix_position position prefix source).
Proof.
  intros position prefix source Hcanonical.
  induction Hcanonical as
    [|path multiplicity tail Hpath Hmultiplicity Hordered Htail IH]; simpl.
  - constructor.
  - destruct (prefix_position_eqb (classify_prefix prefix path) position).
    + apply CanonicalEntriesCons; try assumption.
      eapply select_prefix_position_preserves_path_forall. exact Hordered.
    + exact IH.
Qed.

Definition ordered_before (left right : list Entry) : Prop :=
  Forall
    (fun left_entry =>
      Forall
        (fun right_entry =>
          path_lex_lt (entry_path left_entry) (entry_path right_entry))
        right)
    left.

Lemma ordered_before_tail :
  forall head tail right,
    ordered_before (head :: tail) right -> ordered_before tail right.
Proof.
  intros head tail right Hordered. inversion Hordered. assumption.
Qed.

Theorem canonical_append :
  forall left right,
    canonical_entries left ->
    canonical_entries right ->
    ordered_before left right ->
    canonical_entries (left ++ right).
Proof.
  intros left right Hleft.
  induction Hleft as
    [|path multiplicity tail Hpath Hmultiplicity Htail_order Htail_canonical IH];
    intros Hright Hcross; simpl.
  - exact Hright.
  - inversion Hcross as [|left_entry left_tail Hhead_cross Htail_cross]; subst.
    apply CanonicalEntriesCons; try assumption.
    + apply Forall_app. split; assumption.
    + apply IH; assumption.
Qed.

Record PrefixSplicePreconditions
    (prefix : Path) (source replacements : list Entry) : Prop := {
  prefix_source_is_canonical : canonical_entries source;
  prefix_replacements_are_canonical : canonical_entries replacements;
  prefix_replacements_are_inside :
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      replacements;
  prefix_below_before_replacements :
    ordered_before (prefix_below_stream prefix source) replacements;
  prefix_below_before_above :
    ordered_before
      (prefix_below_stream prefix source)
      (prefix_above_stream prefix source);
  prefix_replacements_before_above :
    ordered_before replacements (prefix_above_stream prefix source)
}.

Lemma ordered_before_append_left :
  forall first second right,
    ordered_before first right ->
    ordered_before second right ->
    ordered_before (first ++ second) right.
Proof.
  intros first second right Hfirst Hsecond.
  unfold ordered_before in *. now apply Forall_app.
Qed.

Theorem prefix_splice_stream_preserves_canonicality :
  forall prefix source replacements,
    PrefixSplicePreconditions prefix source replacements ->
    canonical_entries (prefix_splice_stream prefix source replacements).
Proof.
  intros prefix source replacements Hpreconditions.
  destruct Hpreconditions as
    [Hsource Hreplacements Hinside Hbelow_replacements
     Hbelow_above Hreplacements_above].
  unfold prefix_splice_stream.
  rewrite app_assoc.
  apply canonical_append.
  - apply canonical_append.
    + apply select_prefix_position_preserves_canonicality. exact Hsource.
    + exact Hreplacements.
    + exact Hbelow_replacements.
  - apply select_prefix_position_preserves_canonicality. exact Hsource.
  - apply ordered_before_append_left; assumption.
Qed.

Theorem lookup_selected_position :
  forall queried position prefix source,
    lookup_count queried
      (select_prefix_position position prefix source) =
    if prefix_position_eqb (classify_prefix prefix queried) position
    then lookup_count queried source
    else 0%N.
Proof.
  intros queried position prefix source.
  induction source as [|entry tail IH]; simpl.
  - destruct (prefix_position_eqb
      (classify_prefix prefix queried) position); reflexivity.
  - destruct (prefix_position_eqb
      (classify_prefix prefix (entry_path entry)) position)
      eqn:Hentry; simpl;
    destruct (path_eqb queried (entry_path entry)) eqn:Hquery.
    + apply path_eqb_spec in Hquery. subst queried. now rewrite Hentry.
    + rewrite IH. reflexivity.
    + apply path_eqb_spec in Hquery. subst queried.
      rewrite IH. now rewrite Hentry.
    + rewrite IH. reflexivity.
Qed.

Definition paths_absent (queried : Path) (entries : list Entry) : Prop :=
  Forall (fun entry => entry_path entry <> queried) entries.

Lemma paths_absent_lookup_zero :
  forall queried entries,
    paths_absent queried entries -> lookup_count queried entries = 0%N.
Proof.
  intros queried entries Habsent.
  induction Habsent as [|entry tail Hhead Htail IH]; simpl.
  - reflexivity.
  - destruct (path_eqb queried (entry_path entry)) eqn:Heq.
    + apply path_eqb_spec in Heq. symmetry in Heq. contradiction.
    + exact IH.
Qed.

Lemma lookup_append_absent_left :
  forall queried left right,
    paths_absent queried left ->
    lookup_count queried (left ++ right) = lookup_count queried right.
Proof.
  intros queried left right Habsent.
  induction Habsent as [|entry tail Hhead Htail IH]; simpl.
  - reflexivity.
  - destruct (path_eqb queried (entry_path entry)) eqn:Heq.
    + apply path_eqb_spec in Heq. symmetry in Heq. contradiction.
    + exact IH.
Qed.

Lemma lookup_append_absent_right :
  forall queried left right,
    paths_absent queried right ->
    lookup_count queried (left ++ right) = lookup_count queried left.
Proof.
  intros queried left right Habsent.
  induction left as [|entry tail IH]; simpl.
  - now apply paths_absent_lookup_zero.
  - destruct (path_eqb queried (entry_path entry)); [reflexivity | exact IH].
Qed.

Lemma selected_other_position_is_absent :
  forall queried selected prefix source,
    classify_prefix prefix queried <> selected ->
    paths_absent queried (select_prefix_position selected prefix source).
Proof.
  intros queried selected prefix source Hdifferent.
  induction source as [|entry tail IH]; simpl.
  - constructor.
  - destruct (prefix_position_eqb
      (classify_prefix prefix (entry_path entry)) selected) eqn:Hselected.
    + constructor.
      * intro Heq. subst queried.
        apply prefix_position_eqb_spec in Hselected. contradiction.
      * exact IH.
    + exact IH.
Qed.

Lemma inside_replacements_are_absent_for_other_positions :
  forall queried prefix replacements,
    classify_prefix prefix queried <> PrefixInside ->
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      replacements ->
    paths_absent queried replacements.
Proof.
  intros queried prefix replacements Houtside Hinside.
  induction Hinside as [|entry tail Hentry Htail IH].
  - constructor.
  - constructor.
    + intro Heq. subst queried. contradiction.
    + exact IH.
Qed.

Theorem prefix_splice_stream_is_extensionally_correct :
  forall prefix source replacements queried,
    Forall
      (fun entry => classify_prefix prefix (entry_path entry) = PrefixInside)
      replacements ->
    lookup_count queried (prefix_splice_stream prefix source replacements) =
      match classify_prefix prefix queried with
      | PrefixInside => lookup_count queried replacements
      | PrefixBelow | PrefixAbove => lookup_count queried source
      end.
Proof.
  intros prefix source replacements queried Hinside.
  unfold prefix_splice_stream.
  destruct (classify_prefix prefix queried) eqn:Hposition.
  - assert (Hreplacement_absent : paths_absent queried replacements).
    { eapply inside_replacements_are_absent_for_other_positions; eauto.
      rewrite Hposition. discriminate. }
    assert (Habove_absent :
      paths_absent queried (prefix_above_stream prefix source)).
    { apply selected_other_position_is_absent. rewrite Hposition. discriminate. }
    rewrite lookup_append_absent_right.
    + unfold prefix_below_stream.
      rewrite lookup_selected_position, Hposition. reflexivity.
    + apply Forall_app. split; assumption.
  - assert (Hbelow_absent :
      paths_absent queried (prefix_below_stream prefix source)).
    { apply selected_other_position_is_absent. rewrite Hposition. discriminate. }
    assert (Habove_absent :
      paths_absent queried (prefix_above_stream prefix source)).
    { apply selected_other_position_is_absent. rewrite Hposition. discriminate. }
    rewrite lookup_append_absent_left by exact Hbelow_absent.
    rewrite lookup_append_absent_right by exact Habove_absent.
    reflexivity.
  - assert (Hbelow_absent :
      paths_absent queried (prefix_below_stream prefix source)).
    { apply selected_other_position_is_absent. rewrite Hposition. discriminate. }
    assert (Hreplacement_absent : paths_absent queried replacements).
    { eapply inside_replacements_are_absent_for_other_positions; eauto.
      rewrite Hposition. discriminate. }
    rewrite lookup_append_absent_left by exact Hbelow_absent.
    rewrite lookup_append_absent_left by exact Hreplacement_absent.
    unfold prefix_above_stream.
    rewrite lookup_selected_position, Hposition. reflexivity.
Qed.

(** Complete paths, not prefix relations, decide exact subtraction.  These
    witnesses cover the canonical empty/root key, a strict ancestor, a strict
    descendant, and a sibling in one stream. *)
Definition exact_separation_source : list Entry :=
  [([], 1%N); ([1], 2%N); ([1; 2], 3%N); ([1; 3], 4%N)].

Example exact_merge_keeps_root_ancestor_descendant_and_sibling_separate :
  exact_stream_transform exact_separation_source [([1], 2%N)] =
  Some [([], 1%N); ([1; 2], 3%N); ([1; 3], 4%N)].
Proof. reflexivity. Qed.

Example exact_merge_removing_descendant_preserves_its_ancestor :
  exact_stream_transform exact_separation_source [([1; 2], 3%N)] =
  Some [([], 1%N); ([1], 2%N); ([1; 3], 4%N)].
Proof. reflexivity. Qed.

Example exact_merge_removing_root_preserves_every_nonroot_path :
  exact_stream_transform exact_separation_source [([], 1%N)] =
  Some [([1], 2%N); ([1; 2], 3%N); ([1; 3], 4%N)].
Proof. reflexivity. Qed.

Example empty_prefix_filters_every_source_path_before_splice :
  prefix_splice_stream [] exact_separation_source [([9], 1%N)] = [([9], 1%N)].
Proof. reflexivity. Qed.

Example exact_byte_cursor_completes_without_fault_or_fuel_exhaustion :
  cursor_run_status
    (exact_cursor_run exact_separation_source [([1],2%N)]) = CursorComplete.
Proof. reflexivity. Qed.

Example exact_byte_cursor_trace_refines_the_logical_output :
  emitted_trace
    (cursor_run_trace
      (exact_cursor_run exact_separation_source [([1],2%N)])) =
  [([],1%N);([1;2],3%N);([1;3],4%N)].
Proof. reflexivity. Qed.

Definition RootIdentity : Type := nat.

Fixpoint maximum_identity (identities : list RootIdentity) : RootIdentity :=
  match identities with
  | [] => 0
  | identity :: tail => Nat.max identity (maximum_identity tail)
  end.

Lemma member_is_bounded_by_maximum_identity :
  forall identities identity,
    In identity identities -> identity <= maximum_identity identities.
Proof.
  induction identities as [|head tail IH]; intros identity Hin; simpl in *.
  - contradiction.
  - destruct Hin as [Heq | Hin].
    + subst identity. apply Nat.le_max_l.
    + eapply Nat.le_trans; [apply IH; exact Hin | apply Nat.le_max_r].
Qed.

Definition fresh_identity (footprint : list RootIdentity) : RootIdentity :=
  S (maximum_identity footprint).

Theorem fresh_identity_is_outside_the_complete_footprint :
  forall footprint, ~ In (fresh_identity footprint) footprint.
Proof.
  intros footprint Hin.
  pose proof (member_is_bounded_by_maximum_identity
    footprint (fresh_identity footprint) Hin) as Hbounded.
  unfold fresh_identity in Hbounded. lia.
Qed.

Record RootAuthority : Type := {
  authority_identity : RootIdentity;
  authority_trie : ByteTrie;
  authority_trie_canonical : canonical_trie authority_trie
}.

Definition authority_entries (authority : RootAuthority) : list Entry :=
  trie_entries (authority_trie authority).

Definition authority_act (authority : RootAuthority) : list ActToken :=
  structural_act_image (authority_trie authority).

Definition authority_compact (authority : RootAuthority)
    : list ActToken * option Metadata :=
  structural_compact_observation (authority_trie authority).

Theorem structural_compact_is_independent_of_root_identity :
  forall left_identity right_identity trie canonical_left canonical_right,
    authority_compact
      {| authority_identity := left_identity;
         authority_trie := trie;
         authority_trie_canonical := canonical_left |} =
    authority_compact
      {| authority_identity := right_identity;
         authority_trie := trie;
         authority_trie_canonical := canonical_right |}.
Proof. reflexivity. Qed.

Record PreparedCandidate : Type := {
  prepared_entries : list Entry;
  prepared_entries_canonical : canonical_entries prepared_entries;
  prepared_trie : ByteTrie;
  prepared_trie_canonical : canonical_trie prepared_trie;
  prepared_trie_refines_entries : trie_entries prepared_trie = prepared_entries;
  prepared_trace : list StreamEvent;
  prepared_trace_refines_entries : emitted_trace prepared_trace = prepared_entries;
  prepared_metadata : Metadata;
  prepared_metadata_checked :
    checked_metadata prepared_entries = Some prepared_metadata
}.

Record StagedCandidate : Type := {
  staged_identity : RootIdentity;
  staged_parent_identity : RootIdentity;
  staged_prepared : PreparedCandidate
}.

Record TransactionWorld : Type := {
  world_live : RootAuthority;
  world_aliases : list RootAuthority;
  world_staged : option StagedCandidate
}.

Definition world_footprint (world : TransactionWorld) : list RootIdentity :=
  authority_identity (world_live world) ::
  map authority_identity (world_aliases world) ++
  match world_staged world with
  | None => []
  | Some staged => [staged_identity staged]
  end.

Definition stage_prepared_candidate
    (world : TransactionWorld) (prepared : PreparedCandidate)
    : TransactionWorld :=
  let identity := fresh_identity (world_footprint world) in
  {| world_live := world_live world;
     world_aliases := world_aliases world;
     world_staged :=
       Some
         {| staged_identity := identity;
            staged_parent_identity := authority_identity (world_live world);
            staged_prepared := prepared |} |}.

Theorem staged_candidate_is_disjoint_from_live_and_every_alias :
  forall world prepared staged,
    world_staged (stage_prepared_candidate world prepared) = Some staged ->
    ~ In (staged_identity staged) (world_footprint world).
Proof.
  intros world prepared staged Hstaged.
  unfold stage_prepared_candidate in Hstaged.
  inversion Hstaged. subst staged.
  apply fresh_identity_is_outside_the_complete_footprint.
Qed.

Theorem staging_preserves_live_authority_and_all_aliases :
  forall world prepared,
    world_live (stage_prepared_candidate world prepared) = world_live world /\
    world_aliases (stage_prepared_candidate world prepared) = world_aliases world.
Proof. split; reflexivity. Qed.

Definition prepared_as_authority
    (staged : StagedCandidate) : RootAuthority :=
  {| authority_identity := staged_identity staged;
     authority_trie := prepared_trie (staged_prepared staged);
     authority_trie_canonical :=
       prepared_trie_canonical (staged_prepared staged) |}.

Definition publish_staged (world : TransactionWorld) : TransactionWorld :=
  match world_staged world with
  | None => world
  | Some staged =>
      {| world_live := prepared_as_authority staged;
         world_aliases := world_live world :: world_aliases world;
         world_staged := None |}
  end.

Definition abort_staged (world : TransactionWorld) : TransactionWorld :=
  {| world_live := world_live world;
     world_aliases := world_aliases world;
     world_staged := None |}.

Definition fault_staged := abort_staged.

Theorem abort_is_strongly_atomic_for_live_and_aliases :
  forall world,
    world_live (abort_staged world) = world_live world /\
    world_aliases (abort_staged world) = world_aliases world.
Proof. split; reflexivity. Qed.

Theorem fault_unwind_is_strongly_atomic_for_live_and_aliases :
  forall world,
    world_live (fault_staged world) = world_live world /\
    world_aliases (fault_staged world) = world_aliases world.
Proof. split; reflexivity. Qed.

Theorem abort_preserves_every_strong_observation :
  forall world,
    authority_entries (world_live (abort_staged world)) =
      authority_entries (world_live world) /\
    authority_act (world_live (abort_staged world)) =
      authority_act (world_live world) /\
    authority_compact (world_live (abort_staged world)) =
      authority_compact (world_live world) /\
    world_aliases (abort_staged world) = world_aliases world.
Proof. repeat split; reflexivity. Qed.

Theorem fault_preserves_every_strong_observation :
  forall world,
    authority_entries (world_live (fault_staged world)) =
      authority_entries (world_live world) /\
    authority_act (world_live (fault_staged world)) =
      authority_act (world_live world) /\
    authority_compact (world_live (fault_staged world)) =
      authority_compact (world_live world) /\
    world_aliases (fault_staged world) = world_aliases world.
Proof. repeat split; reflexivity. Qed.

Fixpoint drop_alias_identity
    (identity : RootIdentity) (aliases : list RootAuthority)
    : list RootAuthority :=
  match aliases with
  | [] => []
  | alias :: tail =>
      if authority_identity alias =? identity
      then drop_alias_identity identity tail
      else alias :: drop_alias_identity identity tail
  end.

Definition drop_alias
    (identity : RootIdentity) (world : TransactionWorld) : TransactionWorld :=
  {| world_live := world_live world;
     world_aliases := drop_alias_identity identity (world_aliases world);
     world_staged := world_staged world |}.

Theorem dropping_an_alias_never_mutates_live_authority :
  forall identity world,
    world_live (drop_alias identity world) = world_live world.
Proof. reflexivity. Qed.

Theorem dropping_an_alias_never_mutates_a_staged_candidate :
  forall identity world,
    world_staged (drop_alias identity world) = world_staged world.
Proof. reflexivity. Qed.

Lemma drop_alias_identities_commute :
  forall first second aliases,
    drop_alias_identity first (drop_alias_identity second aliases) =
    drop_alias_identity second (drop_alias_identity first aliases).
Proof.
  intros first second aliases.
  induction aliases as [|alias tail IH]; simpl.
  - reflexivity.
  - destruct (authority_identity alias =? second) eqn:Hsecond;
      destruct (authority_identity alias =? first) eqn:Hfirst; simpl;
      rewrite ?Hfirst, ?Hsecond, IH; reflexivity.
Qed.

Theorem shared_alias_drop_order_is_observationally_irrelevant :
  forall first second world,
    drop_alias first (drop_alias second world) =
    drop_alias second (drop_alias first world).
Proof.
  intros first second [live aliases staged].
  unfold drop_alias. simpl.
  f_equal. apply drop_alias_identities_commute.
Qed.

Inductive TransactionAction : Type :=
| ActionStage (prepared : PreparedCandidate)
| ActionPublish
| ActionAbort
| ActionFault
| ActionDropAlias (identity : RootIdentity).

Definition transaction_transition
    (action : TransactionAction) (world : TransactionWorld) : TransactionWorld :=
  match action with
  | ActionStage prepared => stage_prepared_candidate world prepared
  | ActionPublish => publish_staged world
  | ActionAbort => abort_staged world
  | ActionFault => fault_staged world
  | ActionDropAlias identity => drop_alias identity world
  end.

Inductive ZipperEvent : Type :=
| ZipperOpenFreshRoot (identity : RootIdentity)
| ZipperDescend (byte : Byte)
| ZipperSetEndpoint (multiplicity : Multiplicity)
| ZipperAscend
| ZipperSealRoot.

Fixpoint zipper_path_events (path : Path) : list ZipperEvent :=
  match path with
  | [] => []
  | byte :: tail => ZipperDescend byte :: zipper_path_events tail
  end.

Fixpoint zipper_ascents (path : Path) : list ZipperEvent :=
  match path with
  | [] => []
  | _ :: tail => ZipperAscend :: zipper_ascents tail
  end.

Definition zipper_endpoint_events (entry : Entry) : list ZipperEvent :=
  zipper_path_events (entry_path entry) ++
  [ZipperSetEndpoint (entry_multiplicity entry)] ++
  zipper_ascents (entry_path entry).

Fixpoint zipper_events_for_entries (entries : list Entry) : list ZipperEvent :=
  match entries with
  | [] => []
  | entry :: tail =>
      zipper_endpoint_events entry ++ zipper_events_for_entries tail
  end.

Definition topology_preserving_update_protocol
    (identity : RootIdentity) (entries : list Entry) : list ZipperEvent :=
  ZipperOpenFreshRoot identity ::
  zipper_events_for_entries entries ++
  [ZipperSealRoot].

Definition topology_protocol_for_prepared
    (identity : RootIdentity) (prepared : PreparedCandidate)
    : list ZipperEvent :=
  topology_preserving_update_protocol identity (prepared_entries prepared).

Theorem topology_protocol_uses_a_distinct_fresh_root :
  forall world prepared,
    hd_error
      (topology_protocol_for_prepared
        (fresh_identity (world_footprint world)) prepared) =
    Some (ZipperOpenFreshRoot (fresh_identity (world_footprint world))) /\
    ~ In (fresh_identity (world_footprint world)) (world_footprint world).
Proof.
  intros world prepared. split.
  - reflexivity.
  - apply fresh_identity_is_outside_the_complete_footprint.
Qed.

(** Exact and prefix machines share cursor utilities, while topology replay is
    a distinct protocol.  This separation prevents physical pruning history
    from changing logical endpoint semantics. *)
Theorem topology_replay_depends_only_on_canonical_output :
  forall identity left right,
    left = right ->
    topology_preserving_update_protocol identity left =
    topology_preserving_update_protocol identity right.
Proof. intros identity left right Heq. now subst right. Qed.

Lemma exact_step_emits_at_most_three_events :
  forall state next events,
    exact_cursor_step state = ExactProgress next events ->
    length events <= 3.
Proof.
  intros [source removals destination] next events Hstep.
  unfold exact_cursor_step in Hstep.
  destruct source as [|source source_tail];
    destruct removals as [|removal removal_tail]; cbn in Hstep.
  - discriminate.
  - inversion Hstep. simpl. lia.
  - inversion Hstep. simpl. lia.
  - destruct (path_compare (entry_path source) (entry_path removal))
      eqn:Hcompare.
    + destruct (checked_subtract_multiplicity
        (entry_multiplicity source) (entry_multiplicity removal))
        as [remaining|] eqn:Hsubtract.
      * destruct remaining as [|positive];
          inversion Hstep; subst; simpl; lia.
      * discriminate.
    + inversion Hstep. simpl. lia.
    + inversion Hstep. simpl. lia.
Qed.

Theorem run_exact_trace_is_linear_in_fuel :
  forall fuel state final trace status,
    run_exact_cursor fuel state = (final, trace, status) ->
    length trace <= 3 * fuel.
Proof.
  induction fuel as [|fuel IH]; intros state final trace status Hrun; simpl in Hrun.
  - inversion Hrun. simpl. lia.
  - destruct (exact_cursor_step state) as [next events| |] eqn:Hstep.
    + destruct (run_exact_cursor fuel next)
        as [[later later_trace] later_status] eqn:Hlater.
      inversion Hrun. subst final trace status.
      rewrite length_app.
      pose proof (exact_step_emits_at_most_three_events _ _ _ Hstep) as Hbound.
      pose proof (IH _ _ _ _ Hlater) as Hlater_bound. lia.
    + inversion Hrun. simpl. lia.
    + inversion Hrun. simpl. lia.
Qed.

Theorem exact_fuel_has_no_path_depth_cap :
  forall source removals source' removals',
    length source = length source' ->
    length removals = length removals' ->
    exact_cursor_fuel source removals = exact_cursor_fuel source' removals'.
Proof.
  intros source removals source' removals' Hsource Hremovals.
  unfold exact_cursor_fuel. now rewrite Hsource, Hremovals.
Qed.

Theorem prefix_fuel_has_no_path_depth_cap :
  forall source replacements source' replacements',
    length source = length source' ->
    length replacements = length replacements' ->
    prefix_cursor_fuel source replacements =
    prefix_cursor_fuel source' replacements'.
Proof.
  intros source replacements source' replacements' Hsource Hreplacements.
  unfold prefix_cursor_fuel. now rewrite Hsource, Hreplacements.
Qed.

Definition exact_endpoint_availability
    (source removals : list Entry) : Prop :=
  Forall
    (fun source_entry =>
      N.le
        (lookup_count (entry_path source_entry) removals)
        (entry_multiplicity source_entry))
    source /\
  Forall
    (fun removal_entry =>
      N.le
        (entry_multiplicity removal_entry)
        (lookup_count (entry_path removal_entry) source))
    removals.

Lemma checked_subtract_is_total_when_available :
  forall available removed,
    N.le removed available ->
    exists remaining,
      checked_subtract_multiplicity available removed = Some remaining.
Proof.
  intros available removed Havailable.
  unfold checked_subtract_multiplicity.
  assert (Havailableb : N.leb removed available = true).
  { apply N.leb_le. exact Havailable. }
  rewrite Havailableb.
  destruct (N.eqb removed available); eauto.
Qed.

Theorem exact_stream_transform_is_total :
  forall source removals,
    exact_endpoint_availability source removals ->
    exists output, exact_stream_transform source removals = Some output.
Proof.
  intros source removals [Hsource_availability Hremoval_availability].
  clear Hremoval_availability.
  induction source as [|source_entry source_tail IH].
  - exists []. reflexivity.
  - inversion Hsource_availability as [|source_entry' source_tail' Hhead Htail].
    subst source_entry' source_tail'.
    destruct (IH Htail) as [output_tail Houtput_tail].
    destruct (checked_subtract_is_total_when_available
      (entry_multiplicity source_entry)
      (lookup_count (entry_path source_entry) removals) Hhead)
      as [remaining Hremaining].
    destruct (N.eqb remaining (0%N)) eqn:Hzero.
    + exists output_tail. simpl.
      rewrite Hremaining, Houtput_tail, Hzero. reflexivity.
    + exists ((entry_path source_entry, remaining) :: output_tail). simpl.
      rewrite Hremaining, Houtput_tail, Hzero. reflexivity.
Qed.

Theorem prefix_splice_stream_is_total :
  forall prefix source replacements,
    exists output,
      prefix_splice_stream prefix source replacements = output.
Proof. intros. eexists. reflexivity. Qed.

Definition empty_root_authority (identity : RootIdentity) : RootAuthority :=
  {| authority_identity := identity;
     authority_trie := empty_trie;
     authority_trie_canonical := empty_trie_is_canonical |}.

Definition canonical_empty_world : TransactionWorld :=
  {| world_live := empty_root_authority 0;
     world_aliases := [];
     world_staged := None |}.

Theorem canonical_empty_world_has_one_empty_live_root :
  authority_entries (world_live canonical_empty_world) = [] /\
  authority_act (world_live canonical_empty_world) =
    [ActNodeStart; ActNodeEnd] /\
  world_aliases canonical_empty_world = [] /\
  world_staged canonical_empty_world = None.
Proof. repeat split; reflexivity. Qed.

Definition path_prefix (prefix path : Path) : Prop :=
  exists suffix, path = prefix ++ suffix.

Lemma path_prefixb_spec :
  forall prefix path, path_prefixb prefix path = true <-> path_prefix prefix path.
Proof.
  induction prefix as [|prefix_byte prefix_tail IH];
    destruct path as [|path_byte path_tail]; simpl.
  - split.
    + intro Htrue.
      exists ([] : Path).
      reflexivity.
    + intro Hprefix.
      reflexivity.
  - split.
    + intro Htrue.
      exists (path_byte :: path_tail).
      reflexivity.
    + intro Hprefix.
      reflexivity.
  - split.
    + intro Hfalse.
      discriminate.
    + intro Hprefix.
      destruct Hprefix as [suffix Hsuffix].
      discriminate.
  - rewrite andb_true_iff, Nat.eqb_eq, IH.
    split.
    + intro Hcombined.
      destruct Hcombined as [Hbyte Hprefix].
      destruct Hprefix as [suffix Hsuffix].
      subst path_byte.
      exists suffix.
      simpl.
      now rewrite Hsuffix.
    + intro Hprefix.
      destruct Hprefix as [suffix Hsuffix].
      inversion Hsuffix.
      subst path_byte.
      split.
      * reflexivity.
      * exists suffix.
        reflexivity.
Qed.

Lemma path_lex_lt_prefix_extension :
  forall prefix byte suffix,
    path_lex_lt prefix (prefix ++ byte :: suffix).
Proof.
  induction prefix as [|head tail IH]; intros byte suffix; simpl.
  - apply PathLexNil.
  - apply PathLexTail. apply IH.
Qed.

Lemma prefix_path_is_not_below_its_prefix :
  forall prefix path,
    path_prefix prefix path -> ~ path_lex_lt path prefix.
Proof.
  intros prefix path [suffix Hsuffix]. subst path.
  destruct suffix as [|byte suffix].
  - rewrite app_nil_r. apply path_lex_lt_irreflexive.
  - intro Hreverse.
    pose proof (path_lex_lt_prefix_extension prefix byte suffix) as Hforward.
    exact (path_lex_lt_irreflexive _
      (path_lex_lt_transitive _ _ _ Hforward Hreverse)).
Qed.

Theorem classify_prefix_below_iff :
  forall prefix key,
    classify_prefix prefix key = PrefixBelow <-> path_lex_lt key prefix.
Proof.
  intros prefix key. unfold classify_prefix.
  destruct (path_ltb key prefix) eqn:Hbelow; simpl.
  - split.
    + intro Hclassification.
      now apply path_ltb_spec.
    + intro Horder.
      reflexivity.
  - destruct (path_prefixb prefix key) eqn:Hprefix; simpl.
    + split.
      * intro Himpossible.
        discriminate.
      * intro Hlt.
        apply path_ltb_spec in Hlt.
        rewrite Hlt in Hbelow.
        discriminate.
    + split.
      * intro Himpossible.
        discriminate.
      * intro Hlt.
        apply path_ltb_spec in Hlt.
        rewrite Hlt in Hbelow.
        discriminate.
Qed.

Theorem classify_prefix_inside_iff :
  forall prefix key,
    classify_prefix prefix key = PrefixInside <-> path_prefix prefix key.
Proof.
  intros prefix key. unfold classify_prefix.
  destruct (path_ltb key prefix) eqn:Hbelow; simpl.
  - split.
    + intro Himpossible.
      discriminate.
    + intro Hprefix.
      apply path_ltb_spec in Hbelow.
      exfalso.
      exact (prefix_path_is_not_below_its_prefix _ _ Hprefix Hbelow).
  - destruct (path_prefixb prefix key) eqn:Hprefix; simpl.
    + split.
      * intro Hclassification.
        now apply path_prefixb_spec.
      * intro Hrelation.
        reflexivity.
    + split.
      * intro Himpossible.
        discriminate.
      * intro Hrelation.
        apply path_prefixb_spec in Hrelation.
        rewrite Hrelation in Hprefix.
        discriminate.
Qed.

Theorem classify_prefix_above_iff :
  forall prefix key,
    classify_prefix prefix key = PrefixAbove <->
    path_lex_lt prefix key /\ ~ path_prefix prefix key.
Proof.
  intros prefix key. split.
  - intro Habove.
    assert (Hnot_below : ~ path_lex_lt key prefix).
    { intro H. apply classify_prefix_below_iff in H. congruence. }
    assert (Hnot_inside : ~ path_prefix prefix key).
    { intro H. apply classify_prefix_inside_iff in H. congruence. }
    split.
    + destruct (path_lex_order_total prefix key) as [Heq | [Hlt | Hgt]].
      * subst key.
        exfalso.
        apply Hnot_inside.
        exists ([] : Path).
        now rewrite app_nil_r.
      * exact Hlt.
      * contradiction.
    + exact Hnot_inside.
  - intros [Habove Hnot_prefix].
    unfold classify_prefix.
    destruct (path_ltb key prefix) eqn:Hbelow.
    + apply path_ltb_spec in Hbelow.
      exfalso. exact (path_lex_lt_irreflexive _
        (path_lex_lt_transitive _ _ _ Habove Hbelow)).
    + destruct (path_prefixb prefix key) eqn:Hinside.
      * apply path_prefixb_spec in Hinside.
        exfalso.
        apply Hnot_prefix.
        exact Hinside.
      * reflexivity.
Qed.

Lemma prefix_below_is_before_every_prefix_extension :
  forall prefix below inside,
    classify_prefix prefix below = PrefixBelow ->
    path_prefix prefix inside ->
    path_lex_lt below inside.
Proof.
  intros prefix below inside Hbelow [suffix Hinside].
  apply classify_prefix_below_iff in Hbelow.
  subst inside.
  destruct suffix as [|byte suffix].
  - rewrite app_nil_r.
    exact Hbelow.
  - eapply path_lex_lt_transitive.
    + exact Hbelow.
    + apply path_lex_lt_prefix_extension.
Qed.

(** Extending a prefix cannot cross a lexicographically-above path which has
    already diverged from that prefix.  This byte-structural lemma states the
    exact comparator fact used by the symmetric region-order proof. *)
Lemma path_compare_prefix_extension_before_nonprefix_above :
  forall prefix above suffix,
    path_compare prefix above = Lt ->
    path_prefixb prefix above = false ->
    path_compare (prefix ++ suffix) above = Lt.
Proof.
  induction prefix as [|prefix_byte prefix_tail IH];
    intros above suffix Hcompare Hnot_prefix.
  - simpl in Hnot_prefix.
    discriminate.
  - destruct above as [|above_byte above_tail].
    + simpl in Hcompare.
      discriminate.
    + simpl in Hcompare, Hnot_prefix |- *.
      destruct (Nat.compare prefix_byte above_byte) eqn:Hhead.
      * apply Nat.compare_eq in Hhead.
        subst above_byte.
        rewrite Nat.eqb_refl in Hnot_prefix.
        simpl in Hnot_prefix.
        eapply IH.
        -- exact Hcompare.
        -- exact Hnot_prefix.
      * reflexivity.
      * discriminate.
Qed.

Lemma every_prefix_extension_is_before_prefix_above :
  forall prefix inside above,
    path_prefix prefix inside ->
    classify_prefix prefix above = PrefixAbove ->
    path_lex_lt inside above.
Proof.
  intros prefix inside above [suffix Hinside] Habove.
  apply classify_prefix_above_iff in Habove as [Hprefix_above Hnot_prefix].
  subst inside.
  apply (proj1 (path_compare_lt_iff (prefix ++ suffix) above)).
  eapply path_compare_prefix_extension_before_nonprefix_above.
  - apply (proj2 (path_compare_lt_iff prefix above)).
    exact Hprefix_above.
  - destruct (path_prefixb prefix above) eqn:Hprefixb.
    + exfalso.
      apply Hnot_prefix.
      apply (proj1 (path_prefixb_spec prefix above)).
      exact Hprefixb.
    + reflexivity.
Qed.

Theorem prefix_regions_are_monotone :
  forall prefix left right,
    path_lex_lt left right ->
    (classify_prefix prefix left = PrefixInside ->
       classify_prefix prefix right <> PrefixBelow) /\
    (classify_prefix prefix left = PrefixAbove ->
       classify_prefix prefix right = PrefixAbove) /\
    (classify_prefix prefix right = PrefixBelow ->
       classify_prefix prefix left = PrefixBelow) /\
    (classify_prefix prefix right = PrefixInside ->
       classify_prefix prefix left <> PrefixAbove).
Proof.
  intros prefix left right Horder. repeat split.
  - intros Hinside Hbelow.
    apply classify_prefix_inside_iff in Hinside.
    apply classify_prefix_below_iff in Hbelow.
    apply (prefix_path_is_not_below_its_prefix prefix left Hinside).
    eapply path_lex_lt_transitive; eauto.
  - intros Hleft_above.
    destruct (classify_prefix prefix right) eqn:Hright.
    + apply classify_prefix_below_iff in Hright.
      apply classify_prefix_above_iff in Hleft_above as [Hprefix_left _].
      exfalso. exact (path_lex_lt_irreflexive _
        (path_lex_lt_transitive _ _ _
          (path_lex_lt_transitive _ _ _ Hprefix_left Horder) Hright)).
    + apply classify_prefix_inside_iff in Hright.
      pose proof
        (every_prefix_extension_is_before_prefix_above
          prefix right left Hright Hleft_above) as Hreverse.
      exfalso. exact (path_lex_lt_irreflexive _
        (path_lex_lt_transitive _ _ _ Horder Hreverse)).
    + reflexivity.
  - intro Hright_below.
    apply classify_prefix_below_iff.
    apply classify_prefix_below_iff in Hright_below.
    eapply path_lex_lt_transitive; eauto.
  - intros Hright_inside Hleft_above.
    apply classify_prefix_inside_iff in Hright_inside.
    pose proof
      (every_prefix_extension_is_before_prefix_above
        prefix right left Hright_inside Hleft_above) as Hreverse.
    exact (path_lex_lt_irreflexive _
      (path_lex_lt_transitive _ _ _ Horder Hreverse)).
Qed.

Fixpoint path_lcp (left right : Path) : Path :=
  match left, right with
  | left_byte :: left_tail, right_byte :: right_tail =>
      if left_byte =? right_byte
      then left_byte :: path_lcp left_tail right_tail
      else []
  | _, _ => []
  end.

Fixpoint path_drop (count : nat) (path : Path) : Path :=
  match count, path with
  | 0, _ => path
  | S remaining, _ :: tail => path_drop remaining tail
  | _, [] => []
  end.

Lemma path_lcp_length_left :
  forall left right, length (path_lcp left right) <= length left.
Proof.
  induction left as [|left_byte left_tail IH];
    destruct right as [|right_byte right_tail]; simpl; try lia.
  destruct (left_byte =? right_byte); simpl; specialize (IH right_tail); lia.
Qed.

Lemma path_lcp_length_right :
  forall left right, length (path_lcp left right) <= length right.
Proof.
  induction left as [|left_byte left_tail IH];
    destruct right as [|right_byte right_tail]; simpl; try lia.
  destruct (left_byte =? right_byte); simpl; specialize (IH right_tail); lia.
Qed.

Inductive CompressedImageToken : Type :=
| CompressedRootStart
| CompressedAscend
| CompressedDescend (byte : Byte)
| CompressedValue (multiplicity : Multiplicity)
| CompressedRootEnd.

Definition lcp_zipper_move (previous next : Path)
    : list CompressedImageToken :=
  let common := path_lcp previous next in
  repeat CompressedAscend (length previous - length common) ++
  map CompressedDescend (path_drop (length common) next).

Fixpoint canonical_compressed_body
    (previous : Path) (entries : list Entry) : list CompressedImageToken :=
  match entries with
  | [] => repeat CompressedAscend (length previous)
  | entry :: tail =>
      lcp_zipper_move previous (entry_path entry) ++
      CompressedValue (entry_multiplicity entry) ::
      canonical_compressed_body (entry_path entry) tail
  end.

Definition canonical_compressed_image (entries : list Entry)
    : list CompressedImageToken :=
  CompressedRootStart :: canonical_compressed_body [] entries ++
  [CompressedRootEnd].

Theorem equal_canonical_entries_have_equal_compressed_images :
  forall left right,
    canonical_entries left ->
    canonical_entries right ->
    left = right ->
    canonical_compressed_image left = canonical_compressed_image right.
Proof. intros left right _ _ Heq. now subst right. Qed.

Theorem distinct_build_histories_with_equal_entries_have_one_canonical_image :
  forall first_history second_history entries,
    emitted_trace first_history = entries ->
    emitted_trace second_history = entries ->
    canonical_compressed_image (emitted_trace first_history) =
    canonical_compressed_image (emitted_trace second_history).
Proof. intros first second entries Hfirst Hsecond. now rewrite Hfirst, Hsecond. Qed.

Theorem lcp_zipper_never_ascends_past_root :
  forall previous next,
    length previous - length (path_lcp previous next) <= length previous.
Proof. intros; lia. Qed.

(** Exact endpoint operations have deliberately different failure contracts.
    Strict subtraction is transactional: an absent endpoint and an
    insufficient multiplicity both fail preflight.  Idempotent deletion is a
    set-like erase: an absent endpoint is a successful no-op. *)
Fixpoint strict_removal_preflightb
  (source removals : list Entry) : bool :=
  match removals with
  | [] => true
  | removal :: tail =>
      N.leb (entry_multiplicity removal)
        (lookup_count (entry_path removal) source)
      && strict_removal_preflightb source tail
  end.

Definition strict_exact_remove
  (source removals : list Entry) : option (list Entry) :=
  if strict_removal_preflightb source removals
  then exact_stream_transform source removals
  else None.

Fixpoint path_memberb (path : Path) (paths : list Path) : bool :=
  match paths with
  | [] => false
  | candidate :: tail =>
      path_eqb path candidate || path_memberb path tail
  end.

Fixpoint idempotent_exact_delete
  (source : list Entry) (deletions : list Path) : list Entry :=
  match source with
  | [] => []
  | entry :: tail =>
      if path_memberb (entry_path entry) deletions
      then idempotent_exact_delete tail deletions
      else entry :: idempotent_exact_delete tail deletions
  end.

Lemma path_eqb_symmetric :
  forall left right,
    path_eqb left right = path_eqb right left.
Proof.
  intros left right.
  destruct (path_eqb left right) eqn:Hleft.
  - apply (proj1 (path_eqb_spec left right)) in Hleft.
    subst right.
    rewrite path_eqb_reflexive.
    reflexivity.
  - destruct (path_eqb right left) eqn:Hright.
    + apply (proj1 (path_eqb_spec right left)) in Hright.
      subst right.
      rewrite path_eqb_reflexive in Hleft.
      discriminate.
    + reflexivity.
Qed.

Theorem strict_exact_remove_rejects_underflow :
  forall source path amount,
    (lookup_count path source < amount)%N ->
    strict_exact_remove source [(path, amount)] = None.
Proof.
  intros source path amount Hunderflow.
  unfold strict_exact_remove; simpl.
  assert (Hpreflight_false :
    N.leb amount (lookup_count path source) = false).
  { apply N.leb_gt. exact Hunderflow. }
  rewrite Hpreflight_false.
  reflexivity.
Qed.

Theorem strict_exact_remove_rejects_absent_positive :
  forall source path amount,
    lookup_count path source = 0%N ->
    (0 < amount)%N ->
    strict_exact_remove source [(path, amount)] = None.
Proof.
  intros source path amount Habsent Hpositive.
  apply strict_exact_remove_rejects_underflow.
  rewrite Habsent. exact Hpositive.
Qed.

Theorem idempotent_exact_delete_no_matching_endpoint :
  forall source deletions,
    (forall entry,
        In entry source ->
        path_memberb (entry_path entry) deletions = false) ->
    idempotent_exact_delete source deletions = source.
Proof.
  induction source as [|entry tail IH]; intros deletions Hnone; simpl.
  - reflexivity.
  - rewrite Hnone by (left; reflexivity).
    f_equal. apply IH.
    intros candidate Hin.
    apply Hnone. right. exact Hin.
Qed.

Theorem idempotent_exact_delete_is_extensionally_exact :
  forall source deletions queried,
    lookup_count queried (idempotent_exact_delete source deletions) =
      if path_memberb queried deletions
      then 0%N
      else lookup_count queried source.
Proof.
  induction source as [|entry tail IH]; intros deletions queried; simpl.
  - destruct (path_memberb queried deletions); reflexivity.
  - destruct (path_memberb (entry_path entry) deletions) eqn:Hentry.
    + rewrite IH.
      destruct (path_memberb queried deletions) eqn:Hqueried.
      * reflexivity.
      * destruct (path_eqb queried (entry_path entry)) eqn:Hequal.
        -- apply path_eqb_spec in Hequal. subst queried. congruence.
        -- reflexivity.
    + simpl. rewrite IH.
      destruct (path_memberb queried deletions) eqn:Hqueried; simpl.
      * destruct (path_eqb queried (entry_path entry)) eqn:Hequal.
        -- apply path_eqb_spec in Hequal.
           subst queried.
           rewrite Hentry in Hqueried.
           discriminate.
        -- reflexivity.
      * destruct (path_eqb queried (entry_path entry)) eqn:Hequal.
        -- apply path_eqb_spec in Hequal. subst queried. congruence.
        -- reflexivity.
Qed.

Lemma idempotent_exact_delete_preserves_path_forall :
  forall predicate source deletions,
    Forall (fun entry => predicate (entry_path entry)) source ->
    Forall (fun entry => predicate (entry_path entry))
      (idempotent_exact_delete source deletions).
Proof.
  intros predicate source deletions Hforall.
  induction Hforall as [|entry tail Hentry Htail IH]; simpl.
  - constructor.
  - destruct (path_memberb (entry_path entry) deletions).
    + exact IH.
    + constructor; assumption.
Qed.

Theorem idempotent_exact_delete_preserves_canonicality :
  forall source deletions,
    canonical_entries source ->
    canonical_entries (idempotent_exact_delete source deletions).
Proof.
  intros source deletions Hcanonical.
  induction Hcanonical as
    [|path count tail Hpath Hcount Horder Htail IH]; simpl.
  - constructor.
  - destruct (path_memberb path deletions) eqn:Hmember.
    + exact IH.
    + apply CanonicalEntriesCons; try assumption.
      apply idempotent_exact_delete_preserves_path_forall. exact Horder.
Qed.

(** A simultaneous checked merge is the logical contract for exact add/upsert.
    It compares complete paths and never confuses the root, an ancestor, and a
    descendant.  [fuel] is a proof device over list cells, not a path-depth
    limit and not a production allocation. *)
Fixpoint checked_upsert_merge_fuel
  (fuel : nat) (source additions : list Entry) : option (list Entry) :=
  match fuel with
  | O =>
      match source, additions with
      | [], [] => Some []
      | _, _ => None
      end
  | S fuel' =>
      match source, additions with
      | [], [] => Some []
      | source_head :: source_tail, [] => Some (source_head :: source_tail)
      | [], addition :: addition_tail =>
          match checked_add_multiplicity 0%N (entry_multiplicity addition),
                checked_upsert_merge_fuel fuel' [] addition_tail with
          | Some count, Some tail =>
              Some ((entry_path addition, count) :: tail)
          | _, _ => None
          end
      | source_head :: source_tail, addition :: addition_tail =>
          match path_compare (entry_path source_head) (entry_path addition) with
          | Lt =>
              match checked_upsert_merge_fuel fuel' source_tail additions with
              | Some tail => Some (source_head :: tail)
              | None => None
              end
          | Eq =>
              match checked_add_multiplicity (entry_multiplicity source_head)
                                  (entry_multiplicity addition),
                    checked_upsert_merge_fuel fuel' source_tail addition_tail with
              | Some count, Some tail =>
                  Some ((entry_path source_head, count) :: tail)
              | _, _ => None
              end
          | Gt =>
              match checked_add_multiplicity 0%N (entry_multiplicity addition),
                    checked_upsert_merge_fuel fuel' source addition_tail with
              | Some count, Some tail =>
                  Some ((entry_path addition, count) :: tail)
              | _, _ => None
              end
          end
      end
  end.

Definition checked_exact_upsert
  (source additions : list Entry) : option (list Entry) :=
  checked_upsert_merge_fuel (S (length source + length additions))
    source additions.

Definition checked_upsert_preflightb
  (source additions : list Entry) : bool :=
  match checked_exact_upsert source additions with
  | Some _ => true
  | None => false
  end.

Theorem checked_upsert_is_total_after_complete_preflight :
  forall source additions,
    checked_upsert_preflightb source additions = true ->
    exists output, checked_exact_upsert source additions = Some output.
Proof.
  intros source additions Hpreflight.
  unfold checked_upsert_preflightb in Hpreflight.
  destruct (checked_exact_upsert source additions) as [output|] eqn:Hresult.
  - exists output.
    reflexivity.
  - discriminate.
Qed.

Lemma checked_add_zero_is_identity :
  forall count result,
    checked_add_multiplicity 0%N count = Some result ->
    result = count.
Proof.
  intros count result Hchecked.
  unfold checked_add_multiplicity in Hchecked. simpl in Hchecked.
  destruct (N.ltb 0 count && N.leb count multiplicity_max);
    inversion Hchecked. reflexivity.
Qed.

Lemma successful_checked_add_is_valid :
  forall left right result,
    checked_add_multiplicity left right = Some result ->
    multiplicity_valid result = true.
Proof.
  intros left right result Hchecked.
  unfold checked_add_multiplicity in Hchecked.
  destruct (N.ltb 0 (left + right) &&
            N.leb (left + right) multiplicity_max) eqn:Hvalid;
    try discriminate.
  inversion Hchecked; subst result.
  exact Hvalid.
Qed.

Lemma successful_checked_add_is_exact :
  forall left right result,
    checked_add_multiplicity left right = Some result ->
    result = (left + right)%N.
Proof.
  intros left right result Hchecked.
  unfold checked_add_multiplicity in Hchecked.
  destruct (N.ltb 0 (left + right) &&
            N.leb (left + right) multiplicity_max);
    try discriminate.
  inversion Hchecked. reflexivity.
Qed.

Lemma checked_upsert_merge_preserves_path_forall :
  forall fuel predicate source additions output,
    Forall (fun entry => predicate (entry_path entry)) source ->
    Forall (fun entry => predicate (entry_path entry)) additions ->
    checked_upsert_merge_fuel fuel source additions = Some output ->
    Forall (fun entry => predicate (entry_path entry)) output.
Proof.
  induction fuel as [|fuel IH]; intros predicate source additions output
    Hsource Hadditions Hmerge.
  - destruct source, additions; simpl in Hmerge; try discriminate.
    inversion Hmerge. constructor.
  - destruct source as [|source_head source_tail],
      additions as [|addition addition_tail]; simpl in Hmerge.
    + inversion Hmerge. constructor.
    + inversion Hadditions as [|? ? Haddition Haddition_tail].
      destruct (checked_add_multiplicity 0%N
        (entry_multiplicity addition)) as [count|] eqn:Hcount;
        [|discriminate].
      destruct (checked_upsert_merge_fuel fuel [] addition_tail)
        as [merged_tail|] eqn:Htail; [|discriminate].
      inversion Hmerge; subst output. constructor.
      * exact Haddition.
      * eapply (IH predicate [] addition_tail merged_tail).
        -- constructor.
        -- exact Haddition_tail.
        -- exact Htail.
    + inversion Hmerge; subst output. exact Hsource.
    + inversion Hsource as [|? ? Hsource_head Hsource_tail].
      inversion Hadditions as [|? ? Haddition Haddition_tail].
      destruct (path_compare (entry_path source_head)
        (entry_path addition)) eqn:Hcompare.
      * destruct (checked_add_multiplicity
          (entry_multiplicity source_head)
          (entry_multiplicity addition)) as [count|] eqn:Hcount;
          [|discriminate].
        destruct (checked_upsert_merge_fuel fuel source_tail addition_tail)
          as [merged_tail|] eqn:Htail; [|discriminate].
        inversion Hmerge; subst output. constructor.
        -- exact Hsource_head.
        -- eapply (IH predicate source_tail addition_tail merged_tail).
           ++ exact Hsource_tail.
           ++ exact Haddition_tail.
           ++ exact Htail.
      * destruct (checked_upsert_merge_fuel fuel source_tail
          (addition :: addition_tail)) as [tail|] eqn:Htail;
          [|discriminate].
        inversion Hmerge; subst output. constructor.
        -- exact Hsource_head.
        -- eapply (IH predicate source_tail
             (addition :: addition_tail) tail).
           ++ exact Hsource_tail.
           ++ constructor; assumption.
           ++ exact Htail.
      * destruct (checked_add_multiplicity 0%N
          (entry_multiplicity addition)) as [count|] eqn:Hcount;
          [|discriminate].
        destruct (checked_upsert_merge_fuel fuel
          (source_head :: source_tail) addition_tail)
          as [tail|] eqn:Htail; [|discriminate].
        inversion Hmerge; subst output. constructor.
        -- exact Haddition.
        -- eapply (IH predicate (source_head :: source_tail)
             addition_tail tail).
           ++ constructor; assumption.
           ++ exact Haddition_tail.
           ++ exact Htail.
Qed.

Theorem checked_exact_upsert_preserves_path_forall :
  forall predicate source additions output,
    Forall (fun entry => predicate (entry_path entry)) source ->
    Forall (fun entry => predicate (entry_path entry)) additions ->
    checked_exact_upsert source additions = Some output ->
    Forall (fun entry => predicate (entry_path entry)) output.
Proof.
  intros predicate source additions output Hsource Hadditions Hupsert.
  unfold checked_exact_upsert in Hupsert.
  eapply (checked_upsert_merge_preserves_path_forall
    (S (length source + length additions))
    predicate source additions output).
  - exact Hsource.
  - exact Hadditions.
  - exact Hupsert.
Qed.

Lemma path_order_transports_over_ordered_tail :
  forall lower middle entries,
    path_lex_lt lower middle ->
    Forall (fun entry => path_lex_lt middle (entry_path entry)) entries ->
    Forall (fun entry => path_lex_lt lower (entry_path entry)) entries.
Proof.
  intros lower middle entries Hlower Hordered.
  induction Hordered; constructor; eauto using path_lex_lt_transitive.
Qed.

Lemma checked_upsert_merge_preserves_lower_bound :
  forall fuel lower source additions output,
    Forall (fun entry => path_lex_lt lower (entry_path entry)) source ->
    Forall (fun entry => path_lex_lt lower (entry_path entry)) additions ->
    checked_upsert_merge_fuel fuel source additions = Some output ->
    Forall (fun entry => path_lex_lt lower (entry_path entry)) output.
Proof.
  intros fuel lower source additions output Hsource Hadditions Hmerge.
  eapply (checked_upsert_merge_preserves_path_forall fuel
    (fun path => path_lex_lt lower path) source additions output).
  - exact Hsource.
  - exact Hadditions.
  - exact Hmerge.
Qed.

Lemma checked_upsert_merge_preserves_lower_bound_with_left_head :
  forall fuel lower head_path head_count tail additions output,
    path_lex_lt lower head_path ->
    Forall (fun entry => path_lex_lt head_path (entry_path entry)) tail ->
    Forall (fun entry => path_lex_lt lower (entry_path entry)) additions ->
    checked_upsert_merge_fuel fuel
      ((head_path, head_count) :: tail) additions = Some output ->
    Forall (fun entry => path_lex_lt lower (entry_path entry)) output.
Proof.
  intros fuel lower head_path head_count tail additions output
    Hlower Htail Hadditions Hmerge.
  eapply (checked_upsert_merge_preserves_lower_bound fuel lower
    ((head_path, head_count) :: tail) additions output).
  - constructor.
    + exact Hlower.
    + eapply path_order_transports_over_ordered_tail; eauto.
  - exact Hadditions.
  - exact Hmerge.
Qed.

Lemma checked_upsert_merge_preserves_lower_bound_with_right_head :
  forall fuel lower source head_path head_count tail output,
    Forall (fun entry => path_lex_lt lower (entry_path entry)) source ->
    path_lex_lt lower head_path ->
    Forall (fun entry => path_lex_lt head_path (entry_path entry)) tail ->
    checked_upsert_merge_fuel fuel
      source ((head_path, head_count) :: tail) = Some output ->
    Forall (fun entry => path_lex_lt lower (entry_path entry)) output.
Proof.
  intros fuel lower source head_path head_count tail output
    Hsource Hlower Htail Hmerge.
  eapply (checked_upsert_merge_preserves_lower_bound fuel lower source
    ((head_path, head_count) :: tail) output).
  - exact Hsource.
  - constructor.
    + exact Hlower.
    + eapply path_order_transports_over_ordered_tail; eauto.
  - exact Hmerge.
Qed.

Lemma checked_upsert_empty_left_branch_preserves_canonicality :
  forall fuel addition_path addition_count addition_tail count canonical_tail output,
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    checked_add_multiplicity 0%N addition_count = Some count ->
    checked_upsert_merge_fuel fuel [] addition_tail = Some canonical_tail ->
    canonical_entries canonical_tail ->
    output = (addition_path, count) :: canonical_tail ->
    canonical_entries output.
Proof.
  intros fuel addition_path addition_count addition_tail count canonical_tail output
    Hadditions Hcount Htail Hcanonical Houtput.
  inversion Hadditions as
    [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
  subst output.
  apply CanonicalEntriesCons.
  - exact Haddition_path.
  - eapply successful_checked_add_is_valid. exact Hcount.
  - eapply (checked_upsert_merge_preserves_lower_bound fuel
      addition_path [] addition_tail canonical_tail).
    + constructor.
    + exact Haddition_order.
    + exact Htail.
  - exact Hcanonical.
Qed.

Lemma checked_upsert_equal_head_branch_preserves_canonicality :
  forall fuel path source_count source_tail addition_count addition_tail
         count canonical_tail output,
    canonical_entries ((path, source_count) :: source_tail) ->
    canonical_entries ((path, addition_count) :: addition_tail) ->
    checked_add_multiplicity source_count addition_count = Some count ->
    checked_upsert_merge_fuel fuel source_tail addition_tail = Some canonical_tail ->
    canonical_entries canonical_tail ->
    output = (path, count) :: canonical_tail ->
    canonical_entries output.
Proof.
  intros fuel path source_count source_tail addition_count addition_tail
    count canonical_tail output Hsource Hadditions Hcount Htail Hcanonical Houtput.
  inversion Hsource as
    [|? ? ? Hsource_path Hsource_count Hsource_order Hsource_tail].
  inversion Hadditions as
    [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
  subst output.
  apply CanonicalEntriesCons.
  - exact Hsource_path.
  - eapply successful_checked_add_is_valid. exact Hcount.
  - eapply (checked_upsert_merge_preserves_lower_bound fuel
      path source_tail addition_tail canonical_tail).
    + exact Hsource_order.
    + exact Haddition_order.
    + exact Htail.
  - exact Hcanonical.
Qed.

Lemma checked_upsert_left_head_branch_preserves_canonicality :
  forall fuel source_path source_count source_tail
         addition_path addition_count addition_tail canonical_tail output,
    canonical_entries ((source_path, source_count) :: source_tail) ->
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    path_lex_lt source_path addition_path ->
    checked_upsert_merge_fuel fuel source_tail
      ((addition_path, addition_count) :: addition_tail) = Some canonical_tail ->
    canonical_entries canonical_tail ->
    output = (source_path, source_count) :: canonical_tail ->
    canonical_entries output.
Proof.
  intros fuel source_path source_count source_tail
    addition_path addition_count addition_tail canonical_tail output
    Hsource Hadditions Hsource_addition Htail Hcanonical Houtput.
  inversion Hsource as
    [|? ? ? Hsource_path Hsource_count Hsource_order Hsource_tail].
  inversion Hadditions as
    [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
  subst output.
  apply CanonicalEntriesCons.
  - exact Hsource_path.
  - exact Hsource_count.
  - eapply (checked_upsert_merge_preserves_lower_bound_with_right_head
      fuel source_path source_tail addition_path addition_count
      addition_tail canonical_tail).
    + exact Hsource_order.
    + exact Hsource_addition.
    + exact Haddition_order.
    + exact Htail.
  - exact Hcanonical.
Qed.

Lemma checked_upsert_right_head_branch_preserves_canonicality :
  forall fuel source_path source_count source_tail
         addition_path addition_count addition_tail count canonical_tail output,
    canonical_entries ((source_path, source_count) :: source_tail) ->
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    path_lex_lt addition_path source_path ->
    checked_add_multiplicity 0%N addition_count = Some count ->
    checked_upsert_merge_fuel fuel
      ((source_path, source_count) :: source_tail) addition_tail = Some canonical_tail ->
    canonical_entries canonical_tail ->
    output = (addition_path, count) :: canonical_tail ->
    canonical_entries output.
Proof.
  intros fuel source_path source_count source_tail
    addition_path addition_count addition_tail count canonical_tail output
    Hsource Hadditions Haddition_source Hcount Htail Hcanonical Houtput.
  inversion Hsource as
    [|? ? ? Hsource_path Hsource_count Hsource_order Hsource_tail].
  inversion Hadditions as
    [|? ? ? Haddition_path Haddition_count Haddition_order Haddition_tail].
  subst output.
  apply CanonicalEntriesCons.
  - exact Haddition_path.
  - eapply successful_checked_add_is_valid. exact Hcount.
  - eapply (checked_upsert_merge_preserves_lower_bound_with_left_head
      fuel addition_path source_path source_count source_tail
      addition_tail canonical_tail).
    + exact Haddition_source.
    + exact Hsource_order.
    + exact Haddition_order.
    + exact Htail.
  - exact Hcanonical.
Qed.

Lemma checked_upsert_equal_case_from_recursive_canonicality :
  forall fuel path source_count source_tail addition_count addition_tail
         count canonical_tail output,
    canonical_entries ((path, source_count) :: source_tail) ->
    canonical_entries ((path, addition_count) :: addition_tail) ->
    checked_add_multiplicity source_count addition_count = Some count ->
    checked_upsert_merge_fuel fuel source_tail addition_tail = Some canonical_tail ->
    (forall recursive_source recursive_additions recursive_output,
      canonical_entries recursive_source ->
      canonical_entries recursive_additions ->
      checked_upsert_merge_fuel fuel recursive_source recursive_additions =
        Some recursive_output ->
      canonical_entries recursive_output) ->
    output = (path, count) :: canonical_tail ->
    canonical_entries output.
Proof.
  intros fuel path source_count source_tail addition_count addition_tail
    count canonical_tail output Hsource Hadditions Hcount Htail
    Hrecursive Houtput.
  eapply (checked_upsert_equal_head_branch_preserves_canonicality
    fuel path source_count source_tail addition_count addition_tail
    count canonical_tail output).
  - exact Hsource.
  - exact Hadditions.
  - exact Hcount.
  - exact Htail.
  - eapply Hrecursive.
    + exact (canonical_entries_tail _ _ Hsource).
    + exact (canonical_entries_tail _ _ Hadditions).
    + exact Htail.
  - exact Houtput.
Qed.

Lemma checked_upsert_left_case_from_recursive_canonicality :
  forall fuel source_path source_count source_tail
         addition_path addition_count addition_tail canonical_tail output,
    canonical_entries ((source_path, source_count) :: source_tail) ->
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    path_lex_lt source_path addition_path ->
    checked_upsert_merge_fuel fuel source_tail
      ((addition_path, addition_count) :: addition_tail) = Some canonical_tail ->
    (forall recursive_source recursive_additions recursive_output,
      canonical_entries recursive_source ->
      canonical_entries recursive_additions ->
      checked_upsert_merge_fuel fuel recursive_source recursive_additions =
        Some recursive_output ->
      canonical_entries recursive_output) ->
    output = (source_path, source_count) :: canonical_tail ->
    canonical_entries output.
Proof.
  intros fuel source_path source_count source_tail
    addition_path addition_count addition_tail canonical_tail output
    Hsource Hadditions Hsource_addition Htail Hrecursive Houtput.
  eapply (checked_upsert_left_head_branch_preserves_canonicality
    fuel source_path source_count source_tail
    addition_path addition_count addition_tail canonical_tail output).
  - exact Hsource.
  - exact Hadditions.
  - exact Hsource_addition.
  - exact Htail.
  - eapply Hrecursive.
    + exact (canonical_entries_tail _ _ Hsource).
    + exact Hadditions.
    + exact Htail.
  - exact Houtput.
Qed.

Lemma checked_upsert_right_case_from_recursive_canonicality :
  forall fuel source_path source_count source_tail
         addition_path addition_count addition_tail count canonical_tail output,
    canonical_entries ((source_path, source_count) :: source_tail) ->
    canonical_entries ((addition_path, addition_count) :: addition_tail) ->
    path_lex_lt addition_path source_path ->
    checked_add_multiplicity 0%N addition_count = Some count ->
    checked_upsert_merge_fuel fuel
      ((source_path, source_count) :: source_tail) addition_tail = Some canonical_tail ->
    (forall recursive_source recursive_additions recursive_output,
      canonical_entries recursive_source ->
      canonical_entries recursive_additions ->
      checked_upsert_merge_fuel fuel recursive_source recursive_additions =
        Some recursive_output ->
      canonical_entries recursive_output) ->
    output = (addition_path, count) :: canonical_tail ->
    canonical_entries output.
Proof.
  intros fuel source_path source_count source_tail
    addition_path addition_count addition_tail count canonical_tail output
    Hsource Hadditions Haddition_source Hcount Htail Hrecursive Houtput.
  eapply (checked_upsert_right_head_branch_preserves_canonicality
    fuel source_path source_count source_tail
    addition_path addition_count addition_tail count canonical_tail output).
  - exact Hsource.
  - exact Hadditions.
  - exact Haddition_source.
  - exact Hcount.
  - exact Htail.
  - eapply Hrecursive.
    + exact Hsource.
    + exact (canonical_entries_tail _ _ Hadditions).
    + exact Htail.
  - exact Houtput.
Qed.
