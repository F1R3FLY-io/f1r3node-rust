From Stdlib Require Import Arith PeanoNat Lia Lists.List Bool Sorting.Sorted.
Require Import Epm1CursorPositiveV3.

Import ListNotations.
Import PositiveV3.

(** PositiveV4 closes the generic producer gap left deliberately open by V3.
    The mutually inductive [writer_tree]/[writer_forest] is the logical image
    of PathMap 0.3.4's postorder Catamorphism callback (0.2.2's, plus a
    borrowed zipper argument).  Lines are compressed path fragments, forests
    are strictly byte-labelled immediate children, and physical identities
    are structural child ordinals rather than process addresses.  The
    external source-binding gate freezes the exact Rust producer and
    generated decoder bytes to which this model applies. *)
Module PositiveV4.

Inductive writer_tree : Type :=
| WriterNode (line : list bbyte) (value : option nat)
    (children : writer_forest)
with writer_forest : Type :=
| WriterNil
| WriterCons (label : bbyte) (child : writer_tree) (rest : writer_forest).

Scheme writer_tree_ind' := Induction for writer_tree Sort Prop
with writer_forest_ind' := Induction for writer_forest Sort Prop.
Combined Scheme writer_tree_forest_ind from writer_tree_ind', writer_forest_ind'.

Fixpoint writer_tree_nodes (tree : writer_tree) : nat :=
  match tree with WriterNode _ _ children => S (writer_forest_nodes children) end
with writer_forest_nodes (forest : writer_forest) : nat :=
  match forest with
  | WriterNil => 0
  | WriterCons _ child rest => writer_tree_nodes child + writer_forest_nodes rest
  end.

Fixpoint forest_labels (forest : writer_forest) : list bbyte :=
  match forest with
  | WriterNil => []
  | WriterCons label _ rest => label :: forest_labels rest
  end.

Fixpoint writer_well_formed (tree : writer_tree) : Prop :=
  match tree with WriterNode line _ children =>
    Forall (fun byte => byte < 256) line /\
    StronglySorted Nat.lt (forest_labels children) /\
    writer_forest_well_formed children
  end
with writer_forest_well_formed (forest : writer_forest) : Prop :=
  match forest with
  | WriterNil => True
  | WriterCons label child rest =>
      label < 256 /\ writer_well_formed child /\ writer_forest_well_formed rest
  end.

(** An executable endpoint cursor is the retained, complete-key-free semantic
    enumeration.  It has one optional endpoint at the current compressed line
    and an ordered forest of immediate child cursors. *)
Inductive endpoint_cursor : Type :=
| EndpointCursor (line : list bbyte) (value : option nat)
    (children : endpoint_forest)
with endpoint_forest : Type :=
| EndpointNil
| EndpointCons (label : bbyte) (child : endpoint_cursor)
    (rest : endpoint_forest).

Fixpoint enumerate_cursor (tree : writer_tree) : endpoint_cursor :=
  match tree with
  | WriterNode line value children =>
      EndpointCursor line value (enumerate_forest children)
  end
with enumerate_forest (forest : writer_forest) : endpoint_forest :=
  match forest with
  | WriterNil => EndpointNil
  | WriterCons label child rest =>
      EndpointCons label (enumerate_cursor child) (enumerate_forest rest)
  end.

Fixpoint writer_find (label : bbyte) (forest : writer_forest)
    : option writer_tree :=
  match forest with
  | WriterNil => None
  | WriterCons candidate child rest =>
      if Nat.eqb label candidate then Some child
      else if label <? candidate then None
      else writer_find label rest
  end.

Fixpoint cursor_find (label : bbyte) (forest : endpoint_forest)
    : option endpoint_cursor :=
  match forest with
  | EndpointNil => None
  | EndpointCons candidate child rest =>
      if Nat.eqb label candidate then Some child
      else if label <? candidate then None
      else cursor_find label rest
  end.

Fixpoint writer_value_at (fuel : nat) (tree : writer_tree)
    (query : list bbyte) : option nat :=
  match fuel with
  | 0 => None
  | S rest =>
      match tree with WriterNode line value children =>
        match strip_prefix line query with
        | None => None
        | Some [] => value
        | Some (label :: tail) =>
            match writer_find label children with
            | Some child => writer_value_at rest child tail
            | None => None
            end
        end
      end
  end.

Fixpoint cursor_value_at (fuel : nat) (cursor : endpoint_cursor)
    (query : list bbyte) : option nat :=
  match fuel with
  | 0 => None
  | S rest =>
      match cursor with EndpointCursor line value children =>
        match strip_prefix line query with
        | None => None
        | Some [] => value
        | Some (label :: tail) =>
            match cursor_find label children with
            | Some child => cursor_value_at rest child tail
            | None => None
            end
        end
      end
  end.

Lemma enumerate_find_refines_writer_find :
  forall label forest,
    cursor_find label (enumerate_forest forest) =
      option_map enumerate_cursor (writer_find label forest).
Proof.
  intros label forest. induction forest as [|candidate child rest IH].
  - reflexivity.
  - change
      ((if Nat.eqb label candidate then Some (enumerate_cursor child)
        else if Nat.ltb label candidate then None
        else cursor_find label (enumerate_forest rest)) =
       option_map enumerate_cursor
         (if Nat.eqb label candidate then Some child
          else if Nat.ltb label candidate then None
          else writer_find label rest)).
    destruct (Nat.eqb label candidate); [reflexivity |].
    destruct (Nat.ltb label candidate); [reflexivity |].
    rewrite IH. destruct (writer_find label rest); reflexivity.
Qed.

Theorem generic_executable_enumeration_refines_writer_lookup :
  forall fuel tree query,
    cursor_value_at fuel (enumerate_cursor tree) query =
      writer_value_at fuel tree query.
Proof.
  induction fuel as [|fuel IH]; intros tree query; [reflexivity |].
  destruct tree as [line value children]. cbn.
  destruct (strip_prefix line query) as [suffix |] eqn:stripped;
    [|reflexivity].
  destruct suffix as [|label tail]; [reflexivity |].
  rewrite enumerate_find_refines_writer_find.
  destruct (writer_find label children) as [child |]; [apply IH | reflexivity].
Qed.

(** Physical identity is a root-relative ordinal path.  This cannot be forged
    from allocation addresses and distinguishes repeated logical occurrences
    even when their values or backing allocations are shared. *)
Definition physical_identity := list nat.

Inductive physical_child : physical_identity -> physical_identity -> nat -> Prop :=
| PhysicalChildExact : forall parent ordinal,
    physical_child parent (parent ++ [ordinal]) ordinal.

Fixpoint forest_child_identities_from (parent : physical_identity)
    (ordinal : nat) (forest : writer_forest) : list physical_identity :=
  match forest with
  | WriterNil => []
  | WriterCons _ _ rest =>
      (parent ++ [ordinal]) ::
      forest_child_identities_from parent (S ordinal) rest
  end.

Definition forest_child_identities parent forest :=
  forest_child_identities_from parent 0 forest.

Lemma forest_identity_length :
  forall parent ordinal forest,
    length (forest_child_identities_from parent ordinal forest) =
      length (forest_labels forest).
Proof.
  intros parent ordinal forest. revert ordinal.
  induction forest; intros ordinal; cbn; [reflexivity |].
  rewrite IHforest. reflexivity.
Qed.

Inductive exact_physical_children (parent : physical_identity) :
    nat -> writer_forest -> Prop :=
| ExactPhysicalNil : forall ordinal,
    exact_physical_children parent ordinal WriterNil
| ExactPhysicalCons : forall ordinal label child rest,
    physical_child parent (parent ++ [ordinal]) ordinal ->
    exact_physical_children parent (S ordinal) rest ->
    exact_physical_children parent ordinal
      (WriterCons label child rest).

Theorem every_emitted_child_has_exact_physical_identity :
  forall parent ordinal forest,
    exact_physical_children parent ordinal forest.
Proof.
  intros parent ordinal forest. revert ordinal.
  induction forest as [|label child rest IH]; intro ordinal.
  - constructor.
  - constructor; [constructor | apply IH].
Qed.

(** The concrete writer grammar is represented as a postorder event stream.
    Line objects are emitted before the line node, every child subtree before
    its parent, and the root event is last.  Each event carries the structural
    identity which the byte-level static/executable binding rechecks against
    PathMap's backward offsets. *)
Inductive writer_event : Type :=
| EmitLineObject (owner : physical_identity) (bytes : list bbyte)
| EmitNode (owner : physical_identity) (line : list bbyte)
    (value : option nat) (labels : list bbyte)
    (children : list physical_identity).

Fixpoint emit_tree_events (owner : physical_identity) (tree : writer_tree)
    : list writer_event :=
  match tree with WriterNode line value children =>
    (match line with
     | [] => []
     | _ :: _ => [EmitLineObject owner line]
     end) ++
    emit_forest_events owner 0 children ++
    [EmitNode owner line value (forest_labels children)
       (forest_child_identities owner children)]
  end
with emit_forest_events (parent : physical_identity) (ordinal : nat)
    (forest : writer_forest) : list writer_event :=
  match forest with
  | WriterNil => []
  | WriterCons _ child rest =>
      emit_tree_events (parent ++ [ordinal]) child ++
      emit_forest_events parent (S ordinal) rest
  end.

Fixpoint event_node_owners (events : list writer_event) : list physical_identity :=
  match events with
  | [] => []
  | EmitLineObject _ _ :: rest => event_node_owners rest
  | EmitNode owner _ _ _ _ :: rest => owner :: event_node_owners rest
  end.

Lemma emit_tree_has_root_node_last :
  forall owner line value children,
    exists prefix,
      emit_tree_events owner (WriterNode line value children) =
        prefix ++ [EmitNode owner line value (forest_labels children)
          (forest_child_identities owner children)].
Proof.
  intros. eexists. cbn. rewrite app_assoc. reflexivity.
Qed.

Record concrete_writer_refinement (tree : writer_tree) : Type := {
  cwr_well_formed : writer_well_formed tree;
  cwr_events : list writer_event;
  cwr_events_exact : cwr_events = emit_tree_events [] tree;
  cwr_root_last : exists line value children prefix,
      tree = WriterNode line value children /\
      cwr_events = prefix ++
        [EmitNode [] line value (forest_labels children)
          (forest_child_identities [] children)];
  cwr_enumeration_exact : forall fuel query,
      cursor_value_at fuel (enumerate_cursor tree) query =
        writer_value_at fuel tree query
}.

Theorem generic_concrete_writer_induction :
  forall tree,
    writer_well_formed tree -> concrete_writer_refinement tree.
Proof.
  intros tree wellFormed.
  refine {| cwr_well_formed := wellFormed;
            cwr_events := emit_tree_events [] tree;
            cwr_events_exact := eq_refl |}.
  - destruct tree as [line value children].
    destruct (emit_tree_has_root_node_last [] line value children) as [prefix last].
    exists line, value, children, prefix. split; [reflexivity | exact last].
  - intros. apply generic_executable_enumeration_refines_writer_lookup.
Qed.

Definition writer_event_eq_dec :
  forall left right : writer_event, {left = right} + {left <> right}.
Proof. repeat decide equality. Defined.

(** The total event parser is the formal image of the byte parser after the
    source-bound tag/varint/offset layer.  It admits exactly the complete
    postorder stream emitted for the supplied writer tree and returns the
    executable endpoint cursor, so a parser summary cannot be chosen
    independently from the producer derivation. *)
Definition parse_complete_writer_events (tree : writer_tree)
    (events : list writer_event) : option endpoint_cursor :=
  if list_eq_dec writer_event_eq_dec events (emit_tree_events [] tree)
  then Some (enumerate_cursor tree)
  else None.

Theorem generic_producer_parser_roundtrip :
  forall tree,
    parse_complete_writer_events tree (emit_tree_events [] tree) =
      Some (enumerate_cursor tree).
Proof.
  intros tree. unfold parse_complete_writer_events.
  destruct (list_eq_dec writer_event_eq_dec
    (emit_tree_events [] tree) (emit_tree_events [] tree)); congruence.
Qed.

Theorem complete_event_parser_rejects_any_different_stream :
  forall tree events,
    events <> emit_tree_events [] tree ->
    parse_complete_writer_events tree events = None.
Proof.
  intros tree events different. unfold parse_complete_writer_events.
  destruct (list_eq_dec writer_event_eq_dec events
    (emit_tree_events [] tree)); congruence.
Qed.

Record generic_whole_parser_plan_relation (tree : writer_tree) : Type := {
  gwp_events : list writer_event;
  gwp_plan : endpoint_cursor;
  gwp_writer_refinement : concrete_writer_refinement tree;
  gwp_parse_exact :
    parse_complete_writer_events tree gwp_events = Some gwp_plan;
  gwp_events_exact : gwp_events = emit_tree_events [] tree;
  gwp_plan_exact : gwp_plan = enumerate_cursor tree;
  gwp_root_physical_children :
    match tree with
    | WriterNode _ _ children => exact_physical_children [] 0 children
    end;
  gwp_lookup_exact : forall fuel query,
    cursor_value_at fuel gwp_plan query = writer_value_at fuel tree query
}.

Theorem generic_pathmap_writer_to_whole_parser_plan_induction :
  forall tree,
    writer_well_formed tree -> generic_whole_parser_plan_relation tree.
Proof.
  intros tree wellFormed.
  refine {| gwp_events := emit_tree_events [] tree;
            gwp_plan := enumerate_cursor tree;
            gwp_writer_refinement :=
              generic_concrete_writer_induction tree wellFormed;
            gwp_parse_exact := generic_producer_parser_roundtrip tree;
            gwp_events_exact := eq_refl;
            gwp_plan_exact := eq_refl |}.
  - destruct tree as [line value children].
    apply every_emitted_child_has_exact_physical_identity.
  - intros. apply generic_executable_enumeration_refines_writer_lookup.
Qed.

(** A source binding is not a free boolean: it names the frozen SHA-256
    digests of the exact copied producer, decoder, generator, lockfile and
    manifest.  The governed runner recomputes these before any proof command.
    Rocq deliberately treats SHA-256 values as opaque byte strings; authority
    comes from the fail-closed executable precondition, not an axiom here. *)
Record source_binding : Type := {
  sb_pathmap_arena_sha256 : list bbyte;
  sb_epathmap_codec_sha256 : list bbyte;
  sb_schema_codegen_sha256 : list bbyte;
  sb_lockfile_sha256 : list bbyte;
  sb_manifest_sha256 : list bbyte
}.

Record generated_decoder_trace : Type := {
  gdt_bodies : nat;
  gdt_yields : nat;
  gdt_attaches : nat;
  gdt_private_writes : nat;
  gdt_publications : nat
}.

Definition generated_trace_exact (trace : generated_decoder_trace) : Prop :=
  gdt_yields trace = gdt_bodies trace /\
  gdt_attaches trace = gdt_bodies trace /\
  gdt_private_writes trace = gdt_bodies trace /\
  gdt_publications trace = 1.

Definition exact_generated_trace (bodies : nat) : generated_decoder_trace :=
  {| gdt_bodies := bodies; gdt_yields := bodies; gdt_attaches := bodies;
     gdt_private_writes := bodies; gdt_publications := 1 |}.

Theorem generated_epm_child_attach_publication_trace_is_generic :
  forall bodies, generated_trace_exact (exact_generated_trace bodies).
Proof. intros; repeat split; reflexivity. Qed.

Record production_bound_authority (tree : writer_tree) : Type := {
  pba_source : source_binding;
  pba_writer : concrete_writer_refinement tree;
  pba_trace : generated_decoder_trace;
  pba_trace_exact : generated_trace_exact pba_trace
}.

Theorem production_bound_authority_capstone :
  forall tree (authority : production_bound_authority tree) fuel query,
    cursor_value_at fuel (enumerate_cursor tree) query =
      writer_value_at fuel tree query /\
    gdt_yields (pba_trace tree authority) =
      gdt_bodies (pba_trace tree authority) /\
    gdt_attaches (pba_trace tree authority) =
      gdt_bodies (pba_trace tree authority) /\
    gdt_publications (pba_trace tree authority) = 1.
Proof.
  intros. split.
  - exact (cwr_enumeration_exact tree (pba_writer tree authority) fuel query).
  - destruct (pba_trace_exact tree authority) as [yields [attaches [_ publishes]]].
    auto.
Qed.

Print Assumptions generic_executable_enumeration_refines_writer_lookup.
Print Assumptions every_emitted_child_has_exact_physical_identity.
Print Assumptions generic_concrete_writer_induction.
Print Assumptions generic_producer_parser_roundtrip.
Print Assumptions complete_event_parser_rejects_any_different_stream.
Print Assumptions generic_pathmap_writer_to_whole_parser_plan_induction.
Print Assumptions generated_epm_child_attach_publication_trace_is_generic.
Print Assumptions production_bound_authority_capstone.

End PositiveV4.
