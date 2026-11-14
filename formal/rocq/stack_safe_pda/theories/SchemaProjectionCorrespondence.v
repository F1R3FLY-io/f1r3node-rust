(** * Table-driven matcher query projection

    This theory proves that the ordered query compiler
    [compile_ordered_nested_query] in
    [rholang/src/rust/interpreter/matcher/match.rs], which reads the
    projection rule table [PROJECTION_RULES] of
    [rholang/src/rust/interpreter/matcher/projection_rules.rs], never makes an
    RSpace candidate index reject a value that the authoritative spatial
    matcher accepts.

    ** The rule table

    [projection_rule] mirrors the Rust enum [ProjectionRule] constructor for
    constructor, and [projection_rules] is its table, generated row for row
    from the Rust source; [scripts/check-spatial-matcher-obligations.py]
    refuses any difference between the two. [rule_lookup] returns the first
    row of a (type, field) pair. The keys are unique
    ([projection_rules_respect_authority]), so that row is the only one, the
    row the Rust [binary_search_by] finds in the sorted table. [declines] is the
    Rust [declines]: a type declines when one of its rows is [Decline].

    ** The authority facts

    Two facts about the authoritative spatial matcher are the source-audit
    seam of this theory. They are stated here, not derived from Rust:
    - (A1) the matcher never compares the [connective_used], [cost_region] or
      [locally_free] field of any type ([authority_ignored_fields]);
    - (A2) the matcher compares a node of every type outside
      [authority_nonstructural_types] structurally: the target is a node of the
      same type whose fields match pairwise, except the (A1) fields. The nine
      listed types carry associative-commutative, binding, connective or
      variable semantics, so a pattern of such a type may accept any target.
      Types that occur only beneath them, such as MatchCase and KeyValuePair,
      are never reached by the compiler's walk.
    [spatial_term] is the acceptance relation these facts determine. It
    contains every acceptance of the matcher, which is the direction a
    soundness theorem needs.

    ** The compiler model

    [term] is the generated cursor's view of one value. A node has a type
    identifier, an index into [schema_type_names], and named fields in cursor
    order. A field holds a scalar, the RMP2 summary of a ground nested Par's
    component bag, or an ordered sequence of child values whose header
    abstracts the sequence, presence or oneof bytes. [WildcardTerm] is a
    pattern hole: a free variable, a wildcard, or a Par that the compiler
    classifies as dynamic, with the capture levels its scan records.
    [UnsupportedTerm] is a value on which the compiler returns [None] for a
    reason not expressed otherwise, such as a Par root that is not a
    single-expression carrier. Captures are linear, so a capture accepts
    exactly what a wildcard accepts; a repeated level is refused by the shared
    capture scan of [SpatialMatcher.v].

    [project_term] is the query that the walk emits. A field whose rule is
    [Ignored] becomes a wildcard after its exact field-start event, a ground
    nested Par becomes its exact RMP2 token ([RMP2.v]), and every other event
    is exact. A declining node becomes a wildcard in the projection, while
    [term_effects] gives it an unsupported effect, so the walk itself refuses:
    [term_effects] is the walk's committed-branch trace in the effect
    vocabulary of [SpatialMatcher.v], and [atom_effects] adds the final refusal
    of a root that is neither [Send] nor holds a hole. Bytes are abstracted to
    tokens, and the RMP2 version byte is omitted. [term_conforms] says that
    every field of a pattern has a table row; the Rust test
    [every_generated_field_has_exactly_one_projection_rule] establishes it for
    every generated value.

    ** The index filters

    [CandidatePool::positions_matching_query] in
    [rspace++/src/rspace/data_index.rs] and
    [ListTargetIndex::group_mask_matching] in
    [rholang/src/rust/interpreter/matcher/spatial_matcher_pda.rs] are each
    modelled as a filter by [matches_structure]. The list-target filter first
    requires [retained_prefix], the query's exact prefix up to its first
    wildcard, and selects every group when that prefix is empty (the Rust
    prefix of length one). A [BindPattern] atom reaches the compiler only as a
    Par root that is not a single-expression carrier, which the compiler
    refuses, so in the reviewed Rust [CandidatePool] receives no table-driven
    query; the filter theorems hold for every query.

    ** Theorems
    - [projection_rules_respect_authority]: the table has 156 rows with unique
      keys, marks every (A1) field [Ignored], declines every (A2) type, and
      has no [UnorderedSeq] row.
    - [table_projection_preserves_spatial_success]: for a conforming pattern,
      spatial success implies that the projected query accepts and that the
      prefilter keeps the target.
    - [ignored_field_changes_never_change_acceptance]: changing [Ignored]
      fields leaves the pattern's projection unchanged and never changes the
      prefilter's decision.
    - [declined_root_never_prunes]: a declining root compiles to no query, the
      prefilter keeps every target, and every query holding it is refused.
    - [index_filters_preserve_first_admissible_binding] and
      [table_query_filters_preserve_first_admissible_binding]: both index
      filters keep every admissible candidate in order, so the first
      admissible binding is unchanged.
    - [locally_free_exact_mutant_is_a_false_negative],
      [cost_region_exact_mutant_is_a_false_negative] and
      [extended_retained_prefix_mutant_is_a_false_negative]: making
      [locally_free] or [cost_region] exact, or extending the retained prefix
      past its first wildcard, rejects a value that the matcher accepts. *)

From Stdlib Require Import String.
From Stdlib Require Import List Arith Bool.
From StackSafePDA Require Import SpatialMatcher RMP2 RSpaceCommit.
From StackSafePDA Require ConcreteNewQueryCorrespondence.

Import ListNotations.

Module GeneratedNewProjection :=
  StackSafePDA.ConcreteNewQueryCorrespondence.ConcreteNewQueryCorrespondence.

#[local] Arguments emit_opaque_par_token : simpl never.

(** ** The rule table *)

Inductive projection_rule : Type :=
| Exact
| Observe
| Ignored
| UnorderedSeq
| OrderedSeq
| ParEqSeq
| Decline.

Record projection_row : Type := ProjectionRow {
  row_type : string;
  row_field : string;
  row_rule : projection_rule
}.

Section ProjectionTable.

Local Open Scope string_scope.

Definition projection_rules : list projection_row := [
  ProjectionRow "BindPattern" "free_count" Exact;
  ProjectionRow "BindPattern" "patterns" OrderedSeq;
  ProjectionRow "BindPattern" "remainder" Observe;
  ProjectionRow "Bundle" "body" Observe;
  ProjectionRow "Bundle" "read_flag" Exact;
  ProjectionRow "Bundle" "write_flag" Exact;
  ProjectionRow "Connective" "connective_instance" Decline;
  ProjectionRow "ConnectiveBody" "ps" OrderedSeq;
  ProjectionRow "CostBoundSignature" "index" Exact;
  ProjectionRow "CostClauseRegion" "bind_index" Exact;
  ProjectionRow "CostClauseRegion" "region" Observe;
  ProjectionRow "CostRegion" "end_column" Exact;
  ProjectionRow "CostRegion" "end_line" Exact;
  ProjectionRow "CostRegion" "ordinal" Exact;
  ProjectionRow "CostRegion" "signer" Observe;
  ProjectionRow "CostRegion" "start_column" Exact;
  ProjectionRow "CostRegion" "start_line" Exact;
  ProjectionRow "CostRegion" "version" Exact;
  ProjectionRow "CostSignature" "atoms" OrderedSeq;
  ProjectionRow "CostSignatureAtom" "form" Observe;
  ProjectionRow "CostTokenStack" "layers" OrderedSeq;
  ProjectionRow "DeployId" "sig" Exact;
  ProjectionRow "DeployerId" "public_key" Exact;
  ProjectionRow "EAnd" "p1" Observe;
  ProjectionRow "EAnd" "p2" Observe;
  ProjectionRow "EDiv" "p1" Observe;
  ProjectionRow "EDiv" "p2" Observe;
  ProjectionRow "EEq" "p1" Observe;
  ProjectionRow "EEq" "p2" Observe;
  ProjectionRow "EGt" "p1" Observe;
  ProjectionRow "EGt" "p2" Observe;
  ProjectionRow "EGte" "p1" Observe;
  ProjectionRow "EGte" "p2" Observe;
  ProjectionRow "EList" "connective_used" Ignored;
  ProjectionRow "EList" "locally_free" Ignored;
  ProjectionRow "EList" "ps" OrderedSeq;
  ProjectionRow "EList" "remainder" Observe;
  ProjectionRow "ELt" "p1" Observe;
  ProjectionRow "ELt" "p2" Observe;
  ProjectionRow "ELte" "p1" Observe;
  ProjectionRow "ELte" "p2" Observe;
  ProjectionRow "EMap" "connective_used" Ignored;
  ProjectionRow "EMap" "kvs" Decline;
  ProjectionRow "EMap" "locally_free" Ignored;
  ProjectionRow "EMap" "remainder" Decline;
  ProjectionRow "EMatches" "pattern" Observe;
  ProjectionRow "EMatches" "target" Observe;
  ProjectionRow "EMethod" "arguments" OrderedSeq;
  ProjectionRow "EMethod" "connective_used" Ignored;
  ProjectionRow "EMethod" "locally_free" Ignored;
  ProjectionRow "EMethod" "method_name" Exact;
  ProjectionRow "EMethod" "target" Observe;
  ProjectionRow "EMinus" "p1" Observe;
  ProjectionRow "EMinus" "p2" Observe;
  ProjectionRow "EMinusMinus" "p1" Observe;
  ProjectionRow "EMinusMinus" "p2" Observe;
  ProjectionRow "EMod" "p1" Observe;
  ProjectionRow "EMod" "p2" Observe;
  ProjectionRow "EMult" "p1" Observe;
  ProjectionRow "EMult" "p2" Observe;
  ProjectionRow "ENeg" "p" Observe;
  ProjectionRow "ENeq" "p1" Observe;
  ProjectionRow "ENeq" "p2" Observe;
  ProjectionRow "ENot" "p" Observe;
  ProjectionRow "EOr" "p1" Observe;
  ProjectionRow "EOr" "p2" Observe;
  ProjectionRow "EPathMap" "connective_used" Ignored;
  ProjectionRow "EPathMap" "entries" Decline;
  ProjectionRow "EPathMap" "locally_free" Ignored;
  ProjectionRow "EPathMap" "remainder" Decline;
  ProjectionRow "EPercentPercent" "p1" Observe;
  ProjectionRow "EPercentPercent" "p2" Observe;
  ProjectionRow "EPlus" "p1" Observe;
  ProjectionRow "EPlus" "p2" Observe;
  ProjectionRow "EPlusPlus" "p1" Observe;
  ProjectionRow "EPlusPlus" "p2" Observe;
  ProjectionRow "ESet" "connective_used" Ignored;
  ProjectionRow "ESet" "locally_free" Ignored;
  ProjectionRow "ESet" "ps" Decline;
  ProjectionRow "ESet" "remainder" Decline;
  ProjectionRow "ETuple" "connective_used" Ignored;
  ProjectionRow "ETuple" "locally_free" Ignored;
  ProjectionRow "ETuple" "ps" OrderedSeq;
  ProjectionRow "EVar" "v" Observe;
  ProjectionRow "EZipper" "connective_used" Ignored;
  ProjectionRow "EZipper" "current_path" Exact;
  ProjectionRow "EZipper" "cursor_kind" Exact;
  ProjectionRow "EZipper" "is_write_zipper" Exact;
  ProjectionRow "EZipper" "locally_free" Ignored;
  ProjectionRow "EZipper" "pathmap" Observe;
  ProjectionRow "Expr" "expr_instance" Observe;
  ProjectionRow "GBigRational" "denominator" Exact;
  ProjectionRow "GBigRational" "numerator" Exact;
  ProjectionRow "GDeployId" "sig" Exact;
  ProjectionRow "GDeployerId" "public_key" Exact;
  ProjectionRow "GFixedPoint" "scale" Exact;
  ProjectionRow "GFixedPoint" "unscaled" Exact;
  ProjectionRow "GPrivate" "id" Exact;
  ProjectionRow "GUnforgeable" "unf_instance" Observe;
  ProjectionRow "If" "condition" Observe;
  ProjectionRow "If" "connective_used" Ignored;
  ProjectionRow "If" "if_false" Observe;
  ProjectionRow "If" "if_true" Observe;
  ProjectionRow "If" "locally_free" Ignored;
  ProjectionRow "KeyValuePair" "key" Observe;
  ProjectionRow "KeyValuePair" "value" Observe;
  ProjectionRow "ListBindPatterns" "patterns" OrderedSeq;
  ProjectionRow "ListParWithRandom" "cost_region" Ignored;
  ProjectionRow "ListParWithRandom" "pars" OrderedSeq;
  ProjectionRow "ListParWithRandom" "random_state" Exact;
  ProjectionRow "Match" "cases" Decline;
  ProjectionRow "Match" "connective_used" Ignored;
  ProjectionRow "Match" "locally_free" Ignored;
  ProjectionRow "Match" "target" Decline;
  ProjectionRow "MatchCase" "free_count" Exact;
  ProjectionRow "MatchCase" "guard" Observe;
  ProjectionRow "MatchCase" "pattern" Observe;
  ProjectionRow "MatchCase" "source" Observe;
  ProjectionRow "New" "bind_count" Decline;
  ProjectionRow "New" "injections" Decline;
  ProjectionRow "New" "locally_free" Ignored;
  ProjectionRow "New" "p" Decline;
  ProjectionRow "New" "uri" Decline;
  ProjectionRow "PCost" "cost" Exact;
  ProjectionRow "Par" "components" ParEqSeq;
  ProjectionRow "Par" "connective_used" Ignored;
  ProjectionRow "Par" "locally_free" Ignored;
  ProjectionRow "ParWithRandom" "body" Observe;
  ProjectionRow "ParWithRandom" "random_state" Exact;
  ProjectionRow "Receive" "bind_count" Decline;
  ProjectionRow "Receive" "binds" Decline;
  ProjectionRow "Receive" "body" Decline;
  ProjectionRow "Receive" "condition" Decline;
  ProjectionRow "Receive" "connective_used" Ignored;
  ProjectionRow "Receive" "cost_region" Ignored;
  ProjectionRow "Receive" "locally_free" Ignored;
  ProjectionRow "Receive" "peek" Decline;
  ProjectionRow "Receive" "persistent" Decline;
  ProjectionRow "ReceiveBind" "cost_region" Ignored;
  ProjectionRow "ReceiveBind" "free_count" Decline;
  ProjectionRow "ReceiveBind" "patterns" Decline;
  ProjectionRow "ReceiveBind" "remainder" Decline;
  ProjectionRow "ReceiveBind" "source" Decline;
  ProjectionRow "Send" "chan" Observe;
  ProjectionRow "Send" "connective_used" Ignored;
  ProjectionRow "Send" "cost_region" Ignored;
  ProjectionRow "Send" "data" OrderedSeq;
  ProjectionRow "Send" "locally_free" Ignored;
  ProjectionRow "Send" "persistent" Exact;
  ProjectionRow "TaggedContinuation" "clause_regions" OrderedSeq;
  ProjectionRow "TaggedContinuation" "cost_region" Ignored;
  ProjectionRow "TaggedContinuation" "guard" Observe;
  ProjectionRow "TaggedContinuation" "tagged_cont" Observe;
  ProjectionRow "Var" "var_instance" Decline;
  ProjectionRow "VarRef" "depth" Exact;
  ProjectionRow "VarRef" "index" Exact
].

Definition authority_ignored_fields : list string :=
  ["connective_used"; "cost_region"; "locally_free"].

Definition authority_nonstructural_types : list string :=
  ["Connective"; "EMap"; "EPathMap"; "ESet"; "Match"; "New"; "Receive";
   "ReceiveBind"; "Var"].

End ProjectionTable.

Definition authority_ignores (field : string) : bool :=
  existsb (String.eqb field) authority_ignored_fields.

Definition authority_nonstructural (owner : string) : bool :=
  existsb (String.eqb owner) authority_nonstructural_types.

Definition rule_is_ignored (rule : projection_rule) : bool :=
  match rule with
  | Ignored => true
  | _ => false
  end.

Definition rule_is_decline (rule : projection_rule) : bool :=
  match rule with
  | Decline => true
  | _ => false
  end.

Definition rule_is_unordered (rule : projection_rule) : bool :=
  match rule with
  | UnorderedSeq => true
  | _ => false
  end.

Fixpoint rule_lookup (rows : list projection_row) (owner field : string)
    : option projection_rule :=
  match rows with
  | [] => None
  | row :: rest =>
      if String.eqb (row_type row) owner && String.eqb (row_field row) field
      then Some (row_rule row)
      else rule_lookup rest owner field
  end.

Definition declines (rows : list projection_row) (owner : string) : bool :=
  existsb (fun row =>
    String.eqb (row_type row) owner && rule_is_decline (row_rule row)) rows.

Definition field_is_ignored (rows : list projection_row) (owner field : string)
    : bool :=
  match rule_lookup rows owner field with
  | Some Ignored => true
  | _ => false
  end.

Fixpoint keys_unique (rows : list projection_row) : bool :=
  match rows with
  | [] => true
  | row :: rest =>
      forallb (fun other =>
        negb (String.eqb (row_type row) (row_type other) &&
              String.eqb (row_field row) (row_field other))) rest &&
      keys_unique rest
  end.

Definition rows_respect_authority_ignoredb (rows : list projection_row) : bool :=
  forallb (fun row =>
    negb (authority_ignores (row_field row)) || rule_is_ignored (row_rule row))
    rows.

Definition rows_decline_authority_nonstructuralb (rows : list projection_row)
    : bool :=
  forallb (declines rows) authority_nonstructural_types.

Definition make_field_exact (field : string) (rows : list projection_row)
    : list projection_row :=
  map (fun row =>
    if String.eqb (row_field row) field
    then ProjectionRow (row_type row) (row_field row) Exact
    else row) rows.

Fixpoint distinct_adjacent (names : list string) : list string :=
  match names with
  | [] => []
  | name :: rest =>
      match distinct_adjacent rest with
      | [] => [name]
      | next :: tail =>
          if String.eqb name next then next :: tail else name :: next :: tail
      end
  end.

Definition schema_type_names : list string :=
  distinct_adjacent (map row_type projection_rules).

Definition type_name_of_id (type_id : nat) : string :=
  nth type_id schema_type_names EmptyString.

Fixpoint position_of (name : string) (names : list string) : nat :=
  match names with
  | [] => 0
  | candidate :: rest =>
      if String.eqb candidate name then 0 else S (position_of name rest)
  end.

Definition type_id_of (name : string) : nat :=
  position_of name schema_type_names.

#[local] Arguments type_name_of_id : simpl never.
#[local] Arguments declines : simpl never.
#[local] Arguments field_is_ignored : simpl never.

(** ** Byte strings *)

Fixpoint bytes_eqb (left_bytes right_bytes : bytes) : bool :=
  match left_bytes, right_bytes with
  | [], [] => true
  | left_byte :: left_rest, right_byte :: right_rest =>
      Nat.eqb left_byte right_byte && bytes_eqb left_rest right_rest
  | _, _ => false
  end.

Lemma bytes_eqb_refl : forall value, bytes_eqb value value = true.
Proof.
  induction value as [| byte_value rest induction_hypothesis];
    cbn [bytes_eqb].
  - reflexivity.
  - rewrite Nat.eqb_refl. exact induction_hypothesis.
Qed.

Lemma bytes_eqb_eq :
  forall left_bytes right_bytes,
    bytes_eqb left_bytes right_bytes = true -> left_bytes = right_bytes.
Proof.
  induction left_bytes as [| left_byte left_rest induction_hypothesis];
    intros [| right_byte right_rest] same; cbn [bytes_eqb] in same;
    try discriminate same.
  - reflexivity.
  - apply andb_true_iff in same.
    destruct same as [same_byte same_rest].
    apply Nat.eqb_eq in same_byte. subst right_byte.
    rewrite (induction_hypothesis right_rest same_rest). reflexivity.
Qed.

(** ** Values and queries *)

Inductive term : Type :=
| WildcardTerm : list nat -> term
| UnsupportedTerm : term
| NodeTerm : nat -> fields -> term
with fields : Type :=
| NoFields : fields
| FieldCons : string -> payload -> fields -> fields
with payload : Type :=
| ScalarPayload : bytes -> payload
| OpaqueParPayload : opaque_par_summary -> payload
| ChildrenPayload : bytes -> terms -> payload
with terms : Type :=
| NoTerms : terms
| TermCons : term -> terms -> terms.

Inductive query : Type :=
| WildcardQuery : query
| NodeQuery : nat -> field_queries -> query
with field_queries : Type :=
| NoFieldQueries : field_queries
| FieldQueryCons : string -> field_query -> field_queries -> field_queries
with field_query : Type :=
| IgnoredFieldQuery : field_query
| ScalarQuery : bytes -> field_query
| OpaqueParQuery : opaque_par_query -> field_query
| ChildrenQuery : bytes -> queries -> field_query
with queries : Type :=
| NoQueries : queries
| QueryCons : query -> queries -> queries.

Scheme query_mutual := Induction for query Sort Prop
with field_queries_mutual := Induction for field_queries Sort Prop
with field_query_mutual := Induction for field_query Sort Prop
with queries_mutual := Induction for queries Sort Prop.

Combined Scheme query_structure_mutual from
  query_mutual, field_queries_mutual, field_query_mutual, queries_mutual.

Fixpoint query_accepts (compiled : query) (value : term) {struct compiled}
    : bool :=
  match compiled with
  | WildcardQuery => true
  | NodeQuery type_id compiled_fields =>
      match value with
      | NodeTerm value_type_id value_fields =>
          Nat.eqb value_type_id type_id &&
          field_queries_accept compiled_fields value_fields
      | _ => false
      end
  end
with field_queries_accept (compiled : field_queries) (value : fields)
    {struct compiled} : bool :=
  match compiled with
  | NoFieldQueries =>
      match value with
      | NoFields => true
      | FieldCons _ _ _ => false
      end
  | FieldQueryCons compiled_name compiled_field compiled_rest =>
      match value with
      | NoFields => false
      | FieldCons name field_value rest =>
          String.eqb name compiled_name &&
          field_query_accepts compiled_field field_value &&
          field_queries_accept compiled_rest rest
      end
  end
with field_query_accepts (compiled : field_query) (value : payload)
    {struct compiled} : bool :=
  match compiled with
  | IgnoredFieldQuery => true
  | ScalarQuery expected =>
      match value with
      | ScalarPayload actual => bytes_eqb expected actual
      | _ => false
      end
  | OpaqueParQuery (ExactOpaqueToken expected) =>
      match value with
      | OpaqueParPayload actual =>
          bytes_eqb expected (emit_opaque_par_token actual)
      | _ => false
      end
  | OpaqueParQuery WholeOpaqueTokenWildcard =>
      match value with
      | OpaqueParPayload _ => true
      | _ => false
      end
  | ChildrenQuery header compiled_children =>
      match value with
      | ChildrenPayload value_header children =>
          bytes_eqb header value_header &&
          queries_accept compiled_children children
      | _ => false
      end
  end
with queries_accept (compiled : queries) (value : terms) {struct compiled}
    : bool :=
  match compiled with
  | NoQueries =>
      match value with
      | NoTerms => true
      | TermCons _ _ => false
      end
  | QueryCons compiled_term compiled_rest =>
      match value with
      | NoTerms => false
      | TermCons term_value rest =>
          query_accepts compiled_term term_value &&
          queries_accept compiled_rest rest
      end
  end.

Definition root_is_send (value : term) : bool :=
  match value with
  | NodeTerm type_id _ => String.eqb (type_name_of_id type_id) "Send"%string
  | _ => false
  end.

Definition projection_site : capture_site := GeneratedCursorSite 0 [].

(** ** The table-driven walk *)

Section Projection.

Variable rows : list projection_row.

Fixpoint project_term (value : term) {struct value} : query :=
  match value with
  | WildcardTerm _ => WildcardQuery
  | UnsupportedTerm => WildcardQuery
  | NodeTerm type_id value_fields =>
      if declines rows (type_name_of_id type_id)
      then WildcardQuery
      else NodeQuery type_id (project_fields (type_name_of_id type_id) value_fields)
  end
with project_fields (owner : string) (value : fields) {struct value}
    : field_queries :=
  match value with
  | NoFields => NoFieldQueries
  | FieldCons name field_value rest =>
      FieldQueryCons name
        (if field_is_ignored rows owner name
         then IgnoredFieldQuery
         else project_payload field_value)
        (project_fields owner rest)
  end
with project_payload (value : payload) {struct value} : field_query :=
  match value with
  | ScalarPayload scalar => ScalarQuery scalar
  | OpaqueParPayload summary =>
      OpaqueParQuery (ExactOpaqueToken (emit_opaque_par_token summary))
  | ChildrenPayload header children =>
      ChildrenQuery header (project_terms children)
  end
with project_terms (value : terms) {struct value} : queries :=
  match value with
  | NoTerms => NoQueries
  | TermCons term_value rest =>
      QueryCons (project_term term_value) (project_terms rest)
  end.

Fixpoint term_effects (value : term) {struct value}
    : list source_query_effect :=
  match value with
  | WildcardTerm levels =>
      map (SourceCheckedCapture GeneratedEnterNodeRecorder projection_site)
        levels ++ [SourceWildcardEffect projection_site]
  | UnsupportedTerm => [SourceUnsupportedEffect projection_site]
  | NodeTerm type_id value_fields =>
      if declines rows (type_name_of_id type_id)
      then [SourceUnsupportedEffect projection_site]
      else fields_effects (type_name_of_id type_id) value_fields
  end
with fields_effects (owner : string) (value : fields) {struct value}
    : list source_query_effect :=
  match value with
  | NoFields => []
  | FieldCons name field_value rest =>
      (if field_is_ignored rows owner name
       then [SourceWildcardEffect projection_site]
       else payload_effects field_value) ++ fields_effects owner rest
  end
with payload_effects (value : payload) {struct value}
    : list source_query_effect :=
  match value with
  | ScalarPayload _ => []
  | OpaqueParPayload _ => []
  | ChildrenPayload _ children => terms_effects children
  end
with terms_effects (value : terms) {struct value}
    : list source_query_effect :=
  match value with
  | NoTerms => []
  | TermCons term_value rest => term_effects term_value ++ terms_effects rest
  end.

Fixpoint term_walks_hole (value : term) {struct value} : bool :=
  match value with
  | WildcardTerm _ => true
  | UnsupportedTerm => false
  | NodeTerm type_id value_fields =>
      fields_walk_hole (type_name_of_id type_id) value_fields
  end
with fields_walk_hole (owner : string) (value : fields) {struct value} : bool :=
  match value with
  | NoFields => false
  | FieldCons name field_value rest =>
      (if field_is_ignored rows owner name
       then false
       else payload_walks_hole field_value) || fields_walk_hole owner rest
  end
with payload_walks_hole (value : payload) {struct value} : bool :=
  match value with
  | ScalarPayload _ => false
  | OpaqueParPayload _ => false
  | ChildrenPayload _ children => terms_walk_hole children
  end
with terms_walk_hole (value : terms) {struct value} : bool :=
  match value with
  | NoTerms => false
  | TermCons term_value rest => term_walks_hole term_value || terms_walk_hole rest
  end.

Definition atom_effects (value : term) : list source_query_effect :=
  term_effects value ++
  (if term_walks_hole value || root_is_send value
   then []
   else [SourceUnsupportedEffect projection_site]).

Definition compile_atom (value : term) : option query :=
  match scan_source_query_effects initial_cursor_scan (atom_effects value) with
  | Some _ => Some (project_term value)
  | None => None
  end.

Definition compile_query (patterns : list term) : option (list query) :=
  match scan_source_query_effects initial_cursor_scan
          (flat_map atom_effects patterns) with
  | Some _ => Some (map project_term patterns)
  | None => None
  end.

Definition table_prefilter_keeps (pattern target : term) : bool :=
  source_query_prefilter_keeps
    (fun _ => query_accepts (project_term pattern) target)
    (atom_effects pattern).

Fixpoint term_conforms (value : term) {struct value} : Prop :=
  match value with
  | WildcardTerm _ => True
  | UnsupportedTerm => True
  | NodeTerm type_id value_fields =>
      fields_conform (type_name_of_id type_id) value_fields
  end
with fields_conform (owner : string) (value : fields) {struct value} : Prop :=
  match value with
  | NoFields => True
  | FieldCons name field_value rest =>
      rule_lookup rows owner name <> None /\
      payload_conforms field_value /\
      fields_conform owner rest
  end
with payload_conforms (value : payload) {struct value} : Prop :=
  match value with
  | ScalarPayload _ => True
  | OpaqueParPayload _ => True
  | ChildrenPayload _ children => terms_conform children
  end
with terms_conform (value : terms) {struct value} : Prop :=
  match value with
  | NoTerms => True
  | TermCons term_value rest => term_conforms term_value /\ terms_conform rest
  end.

Fixpoint patterns_conform (patterns : list term) : Prop :=
  match patterns with
  | [] => True
  | pattern :: rest => term_conforms pattern /\ patterns_conform rest
  end.

End Projection.

(** ** The authority relation *)

Inductive spatial_term : term -> term -> Prop :=
| SpatialWildcard :
    forall target levels, spatial_term target (WildcardTerm levels)
| SpatialUnsupported :
    forall target, spatial_term target UnsupportedTerm
| SpatialNonstructural :
    forall target type_id pattern_fields,
      authority_nonstructural (type_name_of_id type_id) = true ->
      spatial_term target (NodeTerm type_id pattern_fields)
| SpatialNode :
    forall type_id target_fields pattern_fields,
      spatial_fields target_fields pattern_fields ->
      spatial_term (NodeTerm type_id target_fields) (NodeTerm type_id pattern_fields)
with spatial_fields : fields -> fields -> Prop :=
| SpatialNoFields : spatial_fields NoFields NoFields
| SpatialIgnoredField :
    forall name target_value pattern_value target_rest pattern_rest,
      authority_ignores name = true ->
      spatial_fields target_rest pattern_rest ->
      spatial_fields (FieldCons name target_value target_rest)
        (FieldCons name pattern_value pattern_rest)
| SpatialKeptField :
    forall name target_value pattern_value target_rest pattern_rest,
      spatial_payload target_value pattern_value ->
      spatial_fields target_rest pattern_rest ->
      spatial_fields (FieldCons name target_value target_rest)
        (FieldCons name pattern_value pattern_rest)
with spatial_payload : payload -> payload -> Prop :=
| SpatialScalar :
    forall value, spatial_payload (ScalarPayload value) (ScalarPayload value)
| SpatialOpaquePar :
    forall expected actual,
      spatial_opaque_par_accepts ExactOpaqueShell expected actual ->
      spatial_payload (OpaqueParPayload actual) (OpaqueParPayload expected)
| SpatialChildren :
    forall header target_children pattern_children,
      spatial_terms target_children pattern_children ->
      spatial_payload (ChildrenPayload header target_children)
        (ChildrenPayload header pattern_children)
with spatial_terms : terms -> terms -> Prop :=
| SpatialNoTerms : spatial_terms NoTerms NoTerms
| SpatialTermCons :
    forall target pattern target_rest pattern_rest,
      spatial_term target pattern ->
      spatial_terms target_rest pattern_rest ->
      spatial_terms (TermCons target target_rest) (TermCons pattern pattern_rest).

Scheme spatial_term_mutual := Induction for spatial_term Sort Prop
with spatial_fields_mutual := Induction for spatial_fields Sort Prop
with spatial_payload_mutual := Induction for spatial_payload Sort Prop
with spatial_terms_mutual := Induction for spatial_terms Sort Prop.

Combined Scheme spatial_mutual from
  spatial_term_mutual, spatial_fields_mutual, spatial_payload_mutual,
  spatial_terms_mutual.

(** ** Spatial success implies projected acceptance *)

Lemma rule_lookup_found :
  forall rows owner field rule,
    rule_lookup rows owner field = Some rule ->
    In (ProjectionRow owner field rule) rows.
Proof.
  induction rows as [| [row_owner row_field_name row_rule_value] rest
    induction_hypothesis]; intros owner field rule found;
    cbn [rule_lookup row_type row_field row_rule] in found.
  - discriminate found.
  - destruct (String.eqb row_owner owner && String.eqb row_field_name field)
      eqn:matched.
    + apply andb_true_iff in matched.
      destruct matched as [same_owner same_field].
      apply String.eqb_eq in same_owner.
      apply String.eqb_eq in same_field.
      injection found as same_rule.
      subst row_owner row_field_name row_rule_value.
      left. reflexivity.
    + right. exact (induction_hypothesis owner field rule found).
Qed.

Lemma authority_ignored_field_is_projected_ignored :
  forall rows owner field,
    rows_respect_authority_ignoredb rows = true ->
    rule_lookup rows owner field <> None ->
    authority_ignores field = true ->
    field_is_ignored rows owner field = true.
Proof.
  intros rows owner field respected found ignored.
  unfold field_is_ignored.
  destruct (rule_lookup rows owner field) as [rule |] eqn:lookup.
  - apply rule_lookup_found in lookup.
    assert (every_row : forall row, In row rows ->
      negb (authority_ignores (row_field row)) || rule_is_ignored (row_rule row)
        = true).
    { apply forallb_forall. exact respected. }
    pose proof (every_row _ lookup) as listed.
    cbn [row_field row_rule] in listed.
    rewrite ignored in listed.
    destruct rule; cbn in listed; try discriminate listed; reflexivity.
  - exfalso. exact (found eq_refl).
Qed.

Lemma declines_authority_nonstructural :
  forall rows owner,
    rows_decline_authority_nonstructuralb rows = true ->
    authority_nonstructural owner = true ->
    declines rows owner = true.
Proof.
  intros rows owner declined nonstructural.
  unfold authority_nonstructural in nonstructural.
  apply existsb_exists in nonstructural.
  destruct nonstructural as [name [listed same_name]].
  apply String.eqb_eq in same_name.
  subst name.
  assert (every_type : forall name, In name authority_nonstructural_types ->
    declines rows name = true).
  { apply forallb_forall. exact declined. }
  exact (every_type owner listed).
Qed.

Lemma field_is_ignored_of_lookup :
  forall rows owner name,
    rule_lookup rows owner name = Some Ignored ->
    field_is_ignored rows owner name = true.
Proof.
  intros rows owner name found.
  unfold field_is_ignored. rewrite found. reflexivity.
Qed.

Lemma projection_preserves_spatial_success_mutual :
  forall rows,
    rows_respect_authority_ignoredb rows = true ->
    rows_decline_authority_nonstructuralb rows = true ->
    (forall target pattern, spatial_term target pattern ->
       term_conforms rows pattern ->
       query_accepts (project_term rows pattern) target = true) /\
    (forall target_fields pattern_fields,
       spatial_fields target_fields pattern_fields ->
       forall owner, fields_conform rows owner pattern_fields ->
       field_queries_accept (project_fields rows owner pattern_fields)
         target_fields = true) /\
    (forall target_payload pattern_payload,
       spatial_payload target_payload pattern_payload ->
       payload_conforms rows pattern_payload ->
       field_query_accepts (project_payload rows pattern_payload)
         target_payload = true) /\
    (forall target_terms pattern_terms,
       spatial_terms target_terms pattern_terms ->
       terms_conform rows pattern_terms ->
       queries_accept (project_terms rows pattern_terms) target_terms = true).
Proof.
  intros rows respected declined.
  apply (spatial_mutual
    (fun target pattern _ => term_conforms rows pattern ->
       query_accepts (project_term rows pattern) target = true)
    (fun target_fields pattern_fields _ =>
       forall owner, fields_conform rows owner pattern_fields ->
       field_queries_accept (project_fields rows owner pattern_fields)
         target_fields = true)
    (fun target_payload pattern_payload _ =>
       payload_conforms rows pattern_payload ->
       field_query_accepts (project_payload rows pattern_payload)
         target_payload = true)
    (fun target_terms pattern_terms _ =>
       terms_conform rows pattern_terms ->
       queries_accept (project_terms rows pattern_terms) target_terms = true));
    cbv beta.
  - intros target levels _. reflexivity.
  - intros target _. reflexivity.
  - intros target type_id pattern_fields nonstructural _.
    cbn [project_term].
    rewrite (declines_authority_nonstructural rows (type_name_of_id type_id)
      declined nonstructural).
    reflexivity.
  - intros type_id target_fields pattern_fields fields_spatial fields_induction
      conform.
    cbn [project_term term_conforms] in conform |- *.
    destruct (declines rows (type_name_of_id type_id)); [reflexivity |].
    pose proof (fields_induction (type_name_of_id type_id) conform)
      as fields_accepted.
    assert (body_superset : forall actual expected,
      expected = pattern_fields /\ actual = target_fields ->
      field_queries_accept (project_fields rows (type_name_of_id type_id) expected)
        actual = true).
    { intros actual expected [same_expected same_actual].
      rewrite same_expected, same_actual. exact fields_accepted. }
    assert (same_node : GeneratedNewProjection.spatial_new_accepts
      (fun actual expected => expected = pattern_fields /\ actual = target_fields)
      (GeneratedNewProjection.GeneratedNew fields unit unit unit
         type_id target_fields tt tt tt)
      (GeneratedNewProjection.GeneratedNew fields unit unit unit
         type_id pattern_fields tt tt tt)).
    { split; [reflexivity | split; reflexivity]. }
    exact (GeneratedNewProjection.generated_new_projection_preserves_spatial_success
      fields unit unit unit
      (fun actual expected => expected = pattern_fields /\ actual = target_fields)
      (fun actual expected =>
         field_queries_accept
           (project_fields rows (type_name_of_id type_id) expected) actual)
      (GeneratedNewProjection.GeneratedNew fields unit unit unit
         type_id target_fields tt tt tt)
      (GeneratedNewProjection.GeneratedNew fields unit unit unit
         type_id pattern_fields tt tt tt)
      body_superset same_node).
  - intros owner _. reflexivity.
  - intros name target_value pattern_value target_rest pattern_rest ignored
      rest_spatial rest_induction owner conform.
    cbn [fields_conform] in conform.
    destruct conform as [found [_ rest_conform]].
    cbn [project_fields].
    rewrite (authority_ignored_field_is_projected_ignored rows owner name
      respected found ignored).
    cbn [field_queries_accept field_query_accepts].
    rewrite String.eqb_refl.
    exact (rest_induction owner rest_conform).
  - intros name target_value pattern_value target_rest pattern_rest
      payload_spatial payload_induction rest_spatial rest_induction owner conform.
    cbn [fields_conform] in conform.
    destruct conform as [_ [payload_conform rest_conform]].
    cbn [project_fields].
    destruct (field_is_ignored rows owner name);
      cbn [field_queries_accept field_query_accepts];
      rewrite String.eqb_refl.
    + exact (rest_induction owner rest_conform).
    + rewrite (payload_induction payload_conform).
      exact (rest_induction owner rest_conform).
  - intros value _.
    cbn [project_payload field_query_accepts].
    apply bytes_eqb_refl.
  - intros expected actual accepted _.
    pose proof (@compiled_opaque_query_has_no_false_negative
      ExactOpaqueShell expected actual
      (ExactOpaqueToken (emit_opaque_par_token expected)) eq_refl accepted)
      as token_equal.
    cbn [opaque_par_query_accepts] in token_equal.
    cbn [project_payload field_query_accepts].
    rewrite token_equal.
    apply bytes_eqb_refl.
  - intros header target_children pattern_children children_spatial
      children_induction conform.
    cbn [payload_conforms] in conform.
    cbn [project_payload field_query_accepts].
    rewrite bytes_eqb_refl.
    exact (children_induction conform).
  - intros _. reflexivity.
  - intros target pattern target_rest pattern_rest term_spatial term_induction
      rest_spatial rest_induction conform.
    cbn [terms_conform] in conform.
    destruct conform as [term_conform rest_conform].
    cbn [project_terms queries_accept].
    rewrite (term_induction term_conform), (rest_induction rest_conform).
    reflexivity.
Qed.

(** ** Changing an ignored field *)

Section IgnoredVariants.

Variable rows : list projection_row.

Inductive ignored_variant_term : term -> term -> Prop :=
| VariantTermSame : forall value, ignored_variant_term value value
| VariantTermNode :
    forall type_id value_fields changed_fields,
      ignored_variant_fields (type_name_of_id type_id) value_fields changed_fields ->
      ignored_variant_term (NodeTerm type_id value_fields)
        (NodeTerm type_id changed_fields)
with ignored_variant_fields : string -> fields -> fields -> Prop :=
| VariantNoFields : forall owner, ignored_variant_fields owner NoFields NoFields
| VariantIgnoredField :
    forall owner name value changed_value rest changed_rest,
      rule_lookup rows owner name = Some Ignored ->
      ignored_variant_fields owner rest changed_rest ->
      ignored_variant_fields owner (FieldCons name value rest)
        (FieldCons name changed_value changed_rest)
| VariantKeptField :
    forall owner name value changed_value rest changed_rest,
      ignored_variant_payload value changed_value ->
      ignored_variant_fields owner rest changed_rest ->
      ignored_variant_fields owner (FieldCons name value rest)
        (FieldCons name changed_value changed_rest)
with ignored_variant_payload : payload -> payload -> Prop :=
| VariantPayloadSame : forall value, ignored_variant_payload value value
| VariantChildren :
    forall header children changed_children,
      ignored_variant_terms children changed_children ->
      ignored_variant_payload (ChildrenPayload header children)
        (ChildrenPayload header changed_children)
with ignored_variant_terms : terms -> terms -> Prop :=
| VariantNoTerms : ignored_variant_terms NoTerms NoTerms
| VariantTermCons :
    forall value changed_value rest changed_rest,
      ignored_variant_term value changed_value ->
      ignored_variant_terms rest changed_rest ->
      ignored_variant_terms (TermCons value rest)
        (TermCons changed_value changed_rest).

End IgnoredVariants.

Scheme ignored_variant_term_mutual :=
  Induction for ignored_variant_term Sort Prop
with ignored_variant_fields_mutual :=
  Induction for ignored_variant_fields Sort Prop
with ignored_variant_payload_mutual :=
  Induction for ignored_variant_payload Sort Prop
with ignored_variant_terms_mutual :=
  Induction for ignored_variant_terms Sort Prop.

Combined Scheme ignored_variant_mutual from
  ignored_variant_term_mutual, ignored_variant_fields_mutual,
  ignored_variant_payload_mutual, ignored_variant_terms_mutual.

Lemma ignored_variant_preserves_projection_mutual :
  forall rows,
    (forall value changed, ignored_variant_term rows value changed ->
       project_term rows value = project_term rows changed) /\
    (forall owner value changed, ignored_variant_fields rows owner value changed ->
       project_fields rows owner value = project_fields rows owner changed) /\
    (forall value changed, ignored_variant_payload rows value changed ->
       project_payload rows value = project_payload rows changed) /\
    (forall value changed, ignored_variant_terms rows value changed ->
       project_terms rows value = project_terms rows changed).
Proof.
  intros rows.
  apply (ignored_variant_mutual rows
    (fun value changed _ => project_term rows value = project_term rows changed)
    (fun owner value changed _ =>
       project_fields rows owner value = project_fields rows owner changed)
    (fun value changed _ =>
       project_payload rows value = project_payload rows changed)
    (fun value changed _ =>
       project_terms rows value = project_terms rows changed));
    cbv beta.
  - intros value. reflexivity.
  - intros type_id value_fields changed_fields _ fields_equal.
    cbn [project_term]. rewrite fields_equal. reflexivity.
  - intros owner. reflexivity.
  - intros owner name value changed_value rest changed_rest ignored _
      rest_equal.
    cbn [project_fields].
    rewrite (field_is_ignored_of_lookup rows owner name ignored), rest_equal.
    reflexivity.
  - intros owner name value changed_value rest changed_rest _ payload_equal _
      rest_equal.
    cbn [project_fields]. rewrite payload_equal, rest_equal. reflexivity.
  - intros value. reflexivity.
  - intros header children changed_children _ children_equal.
    cbn [project_payload]. rewrite children_equal. reflexivity.
  - reflexivity.
  - intros value changed_value rest changed_rest _ value_equal _ rest_equal.
    cbn [project_terms]. rewrite value_equal, rest_equal. reflexivity.
Qed.

Lemma ignored_variant_preserves_effects_mutual :
  forall rows,
    (forall value changed, ignored_variant_term rows value changed ->
       term_effects rows value = term_effects rows changed /\
       term_walks_hole rows value = term_walks_hole rows changed) /\
    (forall owner value changed, ignored_variant_fields rows owner value changed ->
       fields_effects rows owner value = fields_effects rows owner changed /\
       fields_walk_hole rows owner value = fields_walk_hole rows owner changed) /\
    (forall value changed, ignored_variant_payload rows value changed ->
       payload_effects rows value = payload_effects rows changed /\
       payload_walks_hole rows value = payload_walks_hole rows changed) /\
    (forall value changed, ignored_variant_terms rows value changed ->
       terms_effects rows value = terms_effects rows changed /\
       terms_walk_hole rows value = terms_walk_hole rows changed).
Proof.
  intros rows.
  apply (ignored_variant_mutual rows
    (fun value changed _ =>
       term_effects rows value = term_effects rows changed /\
       term_walks_hole rows value = term_walks_hole rows changed)
    (fun owner value changed _ =>
       fields_effects rows owner value = fields_effects rows owner changed /\
       fields_walk_hole rows owner value = fields_walk_hole rows owner changed)
    (fun value changed _ =>
       payload_effects rows value = payload_effects rows changed /\
       payload_walks_hole rows value = payload_walks_hole rows changed)
    (fun value changed _ =>
       terms_effects rows value = terms_effects rows changed /\
       terms_walk_hole rows value = terms_walk_hole rows changed));
    cbv beta.
  - intros value. split; reflexivity.
  - intros type_id value_fields changed_fields _ [effects_equal walks_equal].
    cbn [term_effects term_walks_hole].
    rewrite effects_equal, walks_equal. split; reflexivity.
  - intros owner. split; reflexivity.
  - intros owner name value changed_value rest changed_rest ignored _
      [effects_equal walks_equal].
    cbn [fields_effects fields_walk_hole].
    rewrite (field_is_ignored_of_lookup rows owner name ignored), effects_equal,
      walks_equal.
    split; reflexivity.
  - intros owner name value changed_value rest changed_rest _
      [payload_effects_equal payload_walks_equal] _ [effects_equal walks_equal].
    cbn [fields_effects fields_walk_hole].
    rewrite payload_effects_equal, payload_walks_equal, effects_equal,
      walks_equal.
    split; reflexivity.
  - intros value. split; reflexivity.
  - intros header children changed_children _ [effects_equal walks_equal].
    cbn [payload_effects payload_walks_hole].
    rewrite effects_equal, walks_equal. split; reflexivity.
  - split; reflexivity.
  - intros value changed_value rest changed_rest _
      [value_effects_equal value_walks_equal] _ [effects_equal walks_equal].
    cbn [terms_effects terms_walk_hole].
    rewrite value_effects_equal, value_walks_equal, effects_equal, walks_equal.
    split; reflexivity.
Qed.

Lemma ignored_variant_preserves_acceptance_mutual :
  forall rows,
    (forall value changed, ignored_variant_term rows value changed ->
       forall pattern,
         query_accepts (project_term rows pattern) value =
         query_accepts (project_term rows pattern) changed) /\
    (forall owner value changed, ignored_variant_fields rows owner value changed ->
       forall pattern_fields,
         field_queries_accept (project_fields rows owner pattern_fields) value =
         field_queries_accept (project_fields rows owner pattern_fields) changed) /\
    (forall value changed, ignored_variant_payload rows value changed ->
       forall pattern_payload,
         field_query_accepts (project_payload rows pattern_payload) value =
         field_query_accepts (project_payload rows pattern_payload) changed) /\
    (forall value changed, ignored_variant_terms rows value changed ->
       forall pattern_terms,
         queries_accept (project_terms rows pattern_terms) value =
         queries_accept (project_terms rows pattern_terms) changed).
Proof.
  intros rows.
  apply (ignored_variant_mutual rows
    (fun value changed _ => forall pattern,
       query_accepts (project_term rows pattern) value =
       query_accepts (project_term rows pattern) changed)
    (fun owner value changed _ => forall pattern_fields,
       field_queries_accept (project_fields rows owner pattern_fields) value =
       field_queries_accept (project_fields rows owner pattern_fields) changed)
    (fun value changed _ => forall pattern_payload,
       field_query_accepts (project_payload rows pattern_payload) value =
       field_query_accepts (project_payload rows pattern_payload) changed)
    (fun value changed _ => forall pattern_terms,
       queries_accept (project_terms rows pattern_terms) value =
       queries_accept (project_terms rows pattern_terms) changed));
    cbv beta.
  - intros value pattern. reflexivity.
  - intros type_id value_fields changed_fields _ fields_equal pattern.
    destruct pattern as [levels | | pattern_type_id pattern_fields];
      cbn [project_term]; [reflexivity | reflexivity |].
    destruct (declines rows (type_name_of_id pattern_type_id)); [reflexivity |].
    cbn [query_accepts].
    destruct (Nat.eqb type_id pattern_type_id) eqn:same_type; [| reflexivity].
    apply Nat.eqb_eq in same_type. subst pattern_type_id.
    cbn [andb].
    apply fields_equal.
  - intros owner pattern_fields. reflexivity.
  - intros owner name value changed_value rest changed_rest ignored _
      rest_equal pattern_fields.
    destruct pattern_fields as [| pattern_name pattern_value pattern_rest];
      cbn [project_fields field_queries_accept]; [reflexivity |].
    destruct (String.eqb name pattern_name) eqn:same_name; [| reflexivity].
    apply String.eqb_eq in same_name. subst pattern_name.
    rewrite (field_is_ignored_of_lookup rows owner name ignored).
    cbn [field_query_accepts andb].
    apply rest_equal.
  - intros owner name value changed_value rest changed_rest _ payload_equal _
      rest_equal pattern_fields.
    destruct pattern_fields as [| pattern_name pattern_value pattern_rest];
      cbn [project_fields field_queries_accept]; [reflexivity |].
    destruct (String.eqb name pattern_name) eqn:same_name; [| reflexivity].
    destruct (field_is_ignored rows owner pattern_name);
      cbn [field_query_accepts andb].
    + apply rest_equal.
    + rewrite (payload_equal pattern_value), (rest_equal pattern_rest).
      reflexivity.
  - intros value pattern_payload. reflexivity.
  - intros header children changed_children _ children_equal pattern_payload.
    destruct pattern_payload as [pattern_scalar | pattern_summary |
      pattern_header pattern_children];
      cbn [project_payload field_query_accepts]; [reflexivity | reflexivity |].
    rewrite (children_equal pattern_children). reflexivity.
  - intros pattern_terms. reflexivity.
  - intros value changed_value rest changed_rest _ value_equal _ rest_equal
      pattern_terms.
    destruct pattern_terms as [| pattern_term pattern_rest];
      cbn [project_terms queries_accept]; [reflexivity |].
    rewrite (value_equal pattern_term), (rest_equal pattern_rest).
    reflexivity.
Qed.

Lemma ignored_variant_preserves_root_kind :
  forall rows value changed,
    ignored_variant_term rows value changed ->
    root_is_send value = root_is_send changed.
Proof.
  intros rows value changed variant.
  destruct variant; reflexivity.
Qed.

(** ** Tokens, retained prefixes and the index filters *)

Inductive token : Type :=
| NodeStartToken : nat -> token
| NodeEndToken : token
| FieldStartToken : string -> token
| FieldEndToken : token
| ScalarToken : bytes -> token
| ParBagToken : bytes -> token
| ChildrenStartToken : bytes -> token
| ChildrenEndToken : token
| HoleToken : token
| FrameEndToken : token.

Definition token_eqb (left_token right_token : token) : bool :=
  match left_token, right_token with
  | NodeStartToken left_id, NodeStartToken right_id => Nat.eqb left_id right_id
  | NodeEndToken, NodeEndToken => true
  | FieldStartToken left_name, FieldStartToken right_name =>
      String.eqb left_name right_name
  | FieldEndToken, FieldEndToken => true
  | ScalarToken left_bytes, ScalarToken right_bytes =>
      bytes_eqb left_bytes right_bytes
  | ParBagToken left_bytes, ParBagToken right_bytes =>
      bytes_eqb left_bytes right_bytes
  | ChildrenStartToken left_bytes, ChildrenStartToken right_bytes =>
      bytes_eqb left_bytes right_bytes
  | ChildrenEndToken, ChildrenEndToken => true
  | HoleToken, HoleToken => true
  | FrameEndToken, FrameEndToken => true
  | _, _ => false
  end.

Lemma token_eqb_refl : forall value, token_eqb value value = true.
Proof.
  intros [type_id | | name | | scalar | par_bag | header | | |];
    cbn [token_eqb].
  - apply Nat.eqb_refl.
  - reflexivity.
  - apply String.eqb_refl.
  - reflexivity.
  - apply bytes_eqb_refl.
  - apply bytes_eqb_refl.
  - apply bytes_eqb_refl.
  - reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

Inductive part : Type :=
| PartToken : token -> part
| PartWildcard : part.

Fixpoint query_parts (compiled : query) {struct compiled} : list part :=
  match compiled with
  | WildcardQuery => [PartWildcard]
  | NodeQuery type_id compiled_fields =>
      PartToken (NodeStartToken type_id) ::
      field_queries_parts compiled_fields ++ [PartToken NodeEndToken]
  end
with field_queries_parts (compiled : field_queries) {struct compiled}
    : list part :=
  match compiled with
  | NoFieldQueries => []
  | FieldQueryCons name compiled_field compiled_rest =>
      PartToken (FieldStartToken name) ::
      field_query_parts compiled_field ++
      [PartToken FieldEndToken] ++ field_queries_parts compiled_rest
  end
with field_query_parts (compiled : field_query) {struct compiled} : list part :=
  match compiled with
  | IgnoredFieldQuery => [PartWildcard]
  | ScalarQuery expected => [PartToken (ScalarToken expected)]
  | OpaqueParQuery (ExactOpaqueToken expected) =>
      [PartToken (ParBagToken expected)]
  | OpaqueParQuery WholeOpaqueTokenWildcard => [PartWildcard]
  | ChildrenQuery header compiled_children =>
      PartToken (ChildrenStartToken header) ::
      queries_parts compiled_children ++ [PartToken ChildrenEndToken]
  end
with queries_parts (compiled : queries) {struct compiled} : list part :=
  match compiled with
  | NoQueries => []
  | QueryCons compiled_term compiled_rest =>
      query_parts compiled_term ++ queries_parts compiled_rest
  end.

Fixpoint term_tokens (value : term) {struct value} : list token :=
  match value with
  | WildcardTerm _ => [HoleToken]
  | UnsupportedTerm => [HoleToken]
  | NodeTerm type_id value_fields =>
      NodeStartToken type_id :: fields_tokens value_fields ++ [NodeEndToken]
  end
with fields_tokens (value : fields) {struct value} : list token :=
  match value with
  | NoFields => []
  | FieldCons name field_value rest =>
      FieldStartToken name ::
      payload_tokens field_value ++ [FieldEndToken] ++ fields_tokens rest
  end
with payload_tokens (value : payload) {struct value} : list token :=
  match value with
  | ScalarPayload scalar => [ScalarToken scalar]
  | OpaqueParPayload summary => [ParBagToken (emit_opaque_par_token summary)]
  | ChildrenPayload header children =>
      ChildrenStartToken header :: terms_tokens children ++ [ChildrenEndToken]
  end
with terms_tokens (value : terms) {struct value} : list token :=
  match value with
  | NoTerms => []
  | TermCons term_value rest => term_tokens term_value ++ terms_tokens rest
  end.

Inductive image_of : list part -> list token -> Prop :=
| ImageNil : image_of [] []
| ImageToken :
    forall expected parts tokens,
      image_of parts tokens ->
      image_of (PartToken expected :: parts) (expected :: tokens)
| ImageWildcard :
    forall skipped parts tokens,
      image_of parts tokens ->
      image_of (PartWildcard :: parts) (skipped ++ tokens).

Lemma image_token : forall expected, image_of [PartToken expected] [expected].
Proof.
  intros expected. apply ImageToken. apply ImageNil.
Qed.

Lemma image_wildcard : forall tokens, image_of [PartWildcard] tokens.
Proof.
  intros tokens.
  rewrite <- (app_nil_r tokens).
  apply ImageWildcard. apply ImageNil.
Qed.

Lemma image_app :
  forall left_parts left_tokens right_parts right_tokens,
    image_of left_parts left_tokens ->
    image_of right_parts right_tokens ->
    image_of (left_parts ++ right_parts) (left_tokens ++ right_tokens).
Proof.
  intros left_parts left_tokens right_parts right_tokens left_image right_image.
  induction left_image as [| expected rest_parts rest_tokens rest_image
    induction_hypothesis | skipped rest_parts rest_tokens rest_image
    induction_hypothesis].
  - exact right_image.
  - cbn [app]. apply ImageToken. exact induction_hypothesis.
  - cbn [app]. rewrite <- app_assoc. apply ImageWildcard.
    exact induction_hypothesis.
Qed.

Lemma accepted_query_has_token_image_mutual :
  (forall compiled value, query_accepts compiled value = true ->
     image_of (query_parts compiled) (term_tokens value)) /\
  (forall compiled value, field_queries_accept compiled value = true ->
     image_of (field_queries_parts compiled) (fields_tokens value)) /\
  (forall compiled value, field_query_accepts compiled value = true ->
     image_of (field_query_parts compiled) (payload_tokens value)) /\
  (forall compiled value, queries_accept compiled value = true ->
     image_of (queries_parts compiled) (terms_tokens value)).
Proof.
  apply (query_structure_mutual
    (fun compiled => forall value, query_accepts compiled value = true ->
       image_of (query_parts compiled) (term_tokens value))
    (fun compiled => forall value, field_queries_accept compiled value = true ->
       image_of (field_queries_parts compiled) (fields_tokens value))
    (fun compiled => forall value, field_query_accepts compiled value = true ->
       image_of (field_query_parts compiled) (payload_tokens value))
    (fun compiled => forall value, queries_accept compiled value = true ->
       image_of (queries_parts compiled) (terms_tokens value)));
    cbv beta.
  - intros value _. cbn [query_parts]. apply image_wildcard.
  - intros type_id compiled_fields fields_image value accepted.
    destruct value as [levels | | value_type_id value_fields];
      cbn [query_accepts] in accepted; try discriminate accepted.
    apply andb_true_iff in accepted.
    destruct accepted as [same_type fields_accepted].
    apply Nat.eqb_eq in same_type. subst value_type_id.
    cbn [query_parts term_tokens].
    apply ImageToken.
    apply image_app;
      [exact (fields_image value_fields fields_accepted) | apply image_token].
  - intros value accepted.
    destruct value as [| name field_value rest];
      cbn [field_queries_accept] in accepted; try discriminate accepted.
    cbn [field_queries_parts fields_tokens]. apply ImageNil.
  - intros compiled_name compiled_field field_image compiled_rest rest_image
      value accepted.
    destruct value as [| name field_value rest];
      cbn [field_queries_accept] in accepted; try discriminate accepted.
    apply andb_true_iff in accepted.
    destruct accepted as [head_accepted rest_accepted].
    apply andb_true_iff in head_accepted.
    destruct head_accepted as [same_name value_accepted].
    apply String.eqb_eq in same_name. subst name.
    cbn [field_queries_parts fields_tokens].
    apply ImageToken.
    apply image_app; [exact (field_image field_value value_accepted) |].
    apply image_app; [apply image_token | exact (rest_image rest rest_accepted)].
  - intros value _. cbn [field_query_parts]. apply image_wildcard.
  - intros expected value accepted.
    destruct value as [actual | summary | header children];
      cbn [field_query_accepts] in accepted; try discriminate accepted.
    apply bytes_eqb_eq in accepted. subst actual.
    cbn [field_query_parts payload_tokens]. apply image_token.
  - intros [expected |] value accepted;
      destruct value as [actual | summary | header children];
      cbn [field_query_accepts] in accepted; try discriminate accepted.
    + apply bytes_eqb_eq in accepted. subst expected.
      cbn [field_query_parts payload_tokens]. apply image_token.
    + cbn [field_query_parts]. apply image_wildcard.
  - intros header compiled_children children_image value accepted.
    destruct value as [actual | summary | value_header children];
      cbn [field_query_accepts] in accepted; try discriminate accepted.
    apply andb_true_iff in accepted.
    destruct accepted as [same_header children_accepted].
    apply bytes_eqb_eq in same_header. subst value_header.
    cbn [field_query_parts payload_tokens].
    apply ImageToken.
    apply image_app;
      [exact (children_image children children_accepted) | apply image_token].
  - intros value accepted.
    destruct value as [| term_value rest];
      cbn [queries_accept] in accepted; try discriminate accepted.
    cbn [queries_parts terms_tokens]. apply ImageNil.
  - intros compiled_term term_image compiled_rest rest_image value accepted.
    destruct value as [| term_value rest];
      cbn [queries_accept] in accepted; try discriminate accepted.
    apply andb_true_iff in accepted.
    destruct accepted as [term_accepted rest_accepted].
    cbn [queries_parts terms_tokens].
    apply image_app;
      [exact (term_image term_value term_accepted) |
       exact (rest_image rest rest_accepted)].
Qed.

Fixpoint exact_prefix (parts : list part) : list token :=
  match parts with
  | [] => []
  | PartToken expected :: rest => expected :: exact_prefix rest
  | PartWildcard :: _ => []
  end.

Fixpoint extended_exact_prefix (parts : list part) : list token :=
  match parts with
  | [] => []
  | PartToken expected :: rest => expected :: extended_exact_prefix rest
  | PartWildcard :: rest => exact_prefix rest
  end.

Lemma image_prefix :
  forall parts tokens,
    image_of parts tokens -> exists rest, tokens = exact_prefix parts ++ rest.
Proof.
  intros parts tokens image.
  induction image as [| expected rest_parts rest_tokens rest_image
    induction_hypothesis | skipped rest_parts rest_tokens rest_image
    induction_hypothesis].
  - exists []. reflexivity.
  - destruct induction_hypothesis as [rest same].
    exists rest. cbn [exact_prefix app]. rewrite same. reflexivity.
  - exists (skipped ++ rest_tokens). reflexivity.
Qed.

Fixpoint token_prefixb (expected_tokens tokens : list token) : bool :=
  match expected_tokens, tokens with
  | [], _ => true
  | expected :: expected_rest, actual :: tokens_rest =>
      token_eqb expected actual && token_prefixb expected_rest tokens_rest
  | _ :: _, [] => false
  end.

Lemma token_prefixb_app :
  forall expected_tokens rest,
    token_prefixb expected_tokens (expected_tokens ++ rest) = true.
Proof.
  induction expected_tokens as [| expected expected_rest induction_hypothesis];
    intros rest.
  - destruct rest; reflexivity.
  - cbn [token_prefixb app].
    rewrite token_eqb_refl.
    exact (induction_hypothesis rest).
Qed.

Definition atom_parts (compiled : query) : list part :=
  query_parts compiled ++ [PartToken FrameEndToken].

Fixpoint compiled_parts (compiled : list query) : list part :=
  match compiled with
  | [] => []
  | atom :: rest => atom_parts atom ++ compiled_parts rest
  end.

Definition retained_prefix (compiled : list query) : list token :=
  exact_prefix (compiled_parts compiled).

Definition extended_retained_prefix (compiled : list query) : list token :=
  extended_exact_prefix (compiled_parts compiled).

Definition datum_tokens (datum : term) : list token :=
  term_tokens datum ++ [FrameEndToken].

Fixpoint structure_tokens (structure : list term) : list token :=
  match structure with
  | [] => []
  | datum :: rest => datum_tokens datum ++ structure_tokens rest
  end.

Fixpoint matches_structure
    (accepts_remainder : bool) (compiled : list query) (structure : list term)
    : bool :=
  match compiled, structure with
  | [], [] => true
  | [], _ :: _ => accepts_remainder
  | atom :: compiled_rest, datum :: structure_rest =>
      query_accepts atom datum &&
      matches_structure accepts_remainder compiled_rest structure_rest
  | _ :: _, [] => false
  end.

Lemma matches_structure_has_token_image :
  forall accepts_remainder compiled structure,
    matches_structure accepts_remainder compiled structure = true ->
    exists covered uncovered,
      structure_tokens structure = covered ++ uncovered /\
      image_of (compiled_parts compiled) covered.
Proof.
  destruct accepted_query_has_token_image_mutual as [query_image _].
  intros accepts_remainder compiled.
  induction compiled as [| atom compiled_rest induction_hypothesis];
    intros [| datum structure_rest] matched;
    cbn [matches_structure] in matched.
  - exists [], []. split; [reflexivity | apply ImageNil].
  - exists [], (structure_tokens (datum :: structure_rest)).
    split; [reflexivity | apply ImageNil].
  - discriminate matched.
  - apply andb_true_iff in matched.
    destruct matched as [datum_matched rest_matched].
    destruct (induction_hypothesis structure_rest rest_matched)
      as [covered [uncovered [rest_tokens rest_image]]].
    exists (datum_tokens datum ++ covered), uncovered.
    split.
    + cbn [structure_tokens]. rewrite rest_tokens. apply app_assoc.
    + cbn [compiled_parts].
      apply image_app; [| exact rest_image].
      unfold atom_parts, datum_tokens.
      apply image_app;
        [exact (query_image atom datum datum_matched) | apply image_token].
Qed.

Lemma matches_structure_has_retained_prefix :
  forall accepts_remainder compiled structure,
    matches_structure accepts_remainder compiled structure = true ->
    token_prefixb (retained_prefix compiled) (structure_tokens structure) = true.
Proof.
  intros accepts_remainder compiled structure matched.
  destruct (matches_structure_has_token_image accepts_remainder compiled
    structure matched) as [covered [uncovered [split_tokens covered_image]]].
  destruct (image_prefix _ _ covered_image) as [rest prefix_split].
  unfold retained_prefix.
  rewrite split_tokens, prefix_split, <- app_assoc.
  apply token_prefixb_app.
Qed.

Definition candidate_pool_filter {Binding : Type}
    (accepts_remainder : bool) (compiled : list query)
    (candidates : list (@matched_candidate (list term) Binding))
    : list (@matched_candidate (list term) Binding) :=
  filter (fun candidate =>
    matches_structure accepts_remainder compiled (fst candidate)) candidates.

Definition list_target_group_keep
    (accepts_remainder : bool) (compiled : list query) (structure : list term)
    : bool :=
  match retained_prefix compiled with
  | [] => true
  | exact_tokens =>
      token_prefixb exact_tokens (structure_tokens structure) &&
      matches_structure accepts_remainder compiled structure
  end.

Definition list_target_group_filter {Binding : Type}
    (accepts_remainder : bool) (compiled : list query)
    (candidates : list (@matched_candidate (list term) Binding))
    : list (@matched_candidate (list term) Binding) :=
  filter (fun candidate =>
    list_target_group_keep accepts_remainder compiled (fst candidate)) candidates.

Definition extended_group_keep
    (accepts_remainder : bool) (compiled : list query) (structure : list term)
    : bool :=
  match extended_retained_prefix compiled with
  | [] => true
  | exact_tokens =>
      token_prefixb exact_tokens (structure_tokens structure) &&
      matches_structure accepts_remainder compiled structure
  end.

Definition extended_group_filter {Binding : Type}
    (accepts_remainder : bool) (compiled : list query)
    (candidates : list (@matched_candidate (list term) Binding))
    : list (@matched_candidate (list term) Binding) :=
  filter (fun candidate =>
    extended_group_keep accepts_remainder compiled (fst candidate)) candidates.

Lemma list_target_group_keep_accepts :
  forall accepts_remainder compiled structure,
    matches_structure accepts_remainder compiled structure = true ->
    list_target_group_keep accepts_remainder compiled structure = true.
Proof.
  intros accepts_remainder compiled structure matched.
  pose proof (matches_structure_has_retained_prefix accepts_remainder compiled
    structure matched) as prefix_fact.
  unfold list_target_group_keep.
  revert prefix_fact.
  destruct (retained_prefix compiled) as [| first_token rest_tokens].
  - intros _. reflexivity.
  - intros prefix_fact.
    exact (@andb_true_intro _ _ (conj prefix_fact matched)).
Qed.

Lemma filter_keeps_every_admissible :
  forall (Candidate : Type) (admissible keep : Candidate -> bool) candidates,
    (forall candidate, admissible candidate = true -> keep candidate = true) ->
    filter admissible (filter keep candidates) = filter admissible candidates.
Proof.
  intros Candidate admissible keep candidates keeps.
  induction candidates as [| candidate rest induction_hypothesis];
    cbn [filter].
  - reflexivity.
  - destruct (admissible candidate) eqn:eligible.
    + rewrite (keeps candidate eligible).
      cbn [filter].
      rewrite eligible, induction_hypothesis.
      reflexivity.
    + destruct (keep candidate); cbn [filter].
      * rewrite eligible. exact induction_hypothesis.
      * exact induction_hypothesis.
Qed.

Lemma first_admissible_selects_admissible :
  forall (Datum Binding : Type)
    (admissible : @matched_candidate Datum Binding -> bool) candidates selected,
    first_admissible admissible candidates = Some selected ->
    admissible selected = true.
Proof.
  intros Datum Binding admissible candidates selected.
  induction candidates as [| candidate rest induction_hypothesis];
    cbn [first_admissible].
  - intros none. discriminate none.
  - destruct (admissible candidate) eqn:eligible.
    + intros found. injection found as same. subst selected. exact eligible.
    + exact induction_hypothesis.
Qed.

Lemma first_admissible_none_after_filter :
  forall (Datum Binding : Type)
    (admissible keep : @matched_candidate Datum Binding -> bool) candidates,
    first_admissible admissible candidates = None ->
    first_admissible admissible (filter keep candidates) = None.
Proof.
  intros Datum Binding admissible keep candidates.
  induction candidates as [| candidate rest induction_hypothesis];
    cbn [first_admissible filter].
  - intros _. reflexivity.
  - destruct (admissible candidate) eqn:eligible.
    + intros found. discriminate found.
    + intros none. destruct (keep candidate); cbn [first_admissible].
      * rewrite eligible. exact (induction_hypothesis none).
      * exact (induction_hypothesis none).
Qed.

Lemma admissible_preserving_filter :
  forall (Datum Binding : Type)
    (admissible keep : @matched_candidate Datum Binding -> bool) candidates,
    (forall candidate, admissible candidate = true -> keep candidate = true) ->
    first_admissible admissible (filter keep candidates) =
      first_admissible admissible candidates.
Proof.
  intros Datum Binding admissible keep candidates keeps.
  destruct (first_admissible admissible candidates) as [selected |] eqn:first.
  - apply (@replay_subsequence_preserves_first_admissible_bindings
      Datum Binding admissible keep candidates selected first).
    apply keeps.
    exact (first_admissible_selects_admissible Datum Binding admissible
      candidates selected first).
  - exact (first_admissible_none_after_filter Datum Binding admissible keep
      candidates first).
Qed.

Fixpoint spatial_sequence
    (accepts_remainder : bool) (structure patterns : list term) {struct patterns}
    : Prop :=
  match patterns, structure with
  | [], [] => True
  | [], _ :: _ => accepts_remainder = true
  | pattern :: patterns_rest, datum :: structure_rest =>
      spatial_term datum pattern /\
      spatial_sequence accepts_remainder structure_rest patterns_rest
  | _ :: _, [] => False
  end.

Lemma compile_query_projects :
  forall rows patterns compiled,
    compile_query rows patterns = Some compiled ->
    compiled = map (project_term rows) patterns.
Proof.
  intros rows patterns compiled.
  unfold compile_query.
  destruct (scan_source_query_effects initial_cursor_scan
    (flat_map (atom_effects rows) patterns)) as [state |].
  - intros compiled_query. injection compiled_query as same.
    symmetry. exact same.
  - intros compiled_query. discriminate compiled_query.
Qed.

(** ** The capstones *)

Theorem projection_rules_respect_authority :
  Datatypes.length projection_rules = 156 /\
  keys_unique projection_rules = true /\
  rows_respect_authority_ignoredb projection_rules = true /\
  rows_decline_authority_nonstructuralb projection_rules = true /\
  forallb (fun row => negb (rule_is_unordered (row_rule row))) projection_rules
    = true.
Proof.
  refine (conj _ (conj _ (conj _ (conj _ _)))); vm_compute; reflexivity.
Qed.

Lemma table_respects_authority_ignored :
  rows_respect_authority_ignoredb projection_rules = true.
Proof.
  exact (proj1 (proj2 (proj2 projection_rules_respect_authority))).
Qed.

Lemma table_declines_authority_nonstructural :
  rows_decline_authority_nonstructuralb projection_rules = true.
Proof.
  exact (proj1 (proj2 (proj2 (proj2 projection_rules_respect_authority)))).
Qed.

Theorem table_projection_preserves_spatial_success :
  forall target pattern,
    term_conforms projection_rules pattern ->
    spatial_term target pattern ->
    query_accepts (project_term projection_rules pattern) target = true /\
    table_prefilter_keeps projection_rules pattern target = true.
Proof.
  intros target pattern conform spatial.
  destruct (projection_preserves_spatial_success_mutual projection_rules
    table_respects_authority_ignored table_declines_authority_nonstructural)
    as [term_case _].
  pose proof (term_case target pattern spatial conform) as accepted.
  split; [exact accepted |].
  unfold table_prefilter_keeps.
  exact (@source_effect_prefilter_preserves_spatial_success
    (atom_effects projection_rules pattern)
    (fun _ => query_accepts (project_term projection_rules pattern) target)
    true eq_refl (fun _ _ => accepted)).
Qed.

Theorem ignored_field_changes_never_change_acceptance :
  forall pattern changed_pattern target changed_target,
    ignored_variant_term projection_rules pattern changed_pattern ->
    ignored_variant_term projection_rules target changed_target ->
    project_term projection_rules changed_pattern =
      project_term projection_rules pattern /\
    table_prefilter_keeps projection_rules changed_pattern changed_target =
      table_prefilter_keeps projection_rules pattern target.
Proof.
  intros pattern changed_pattern target changed_target pattern_variant
    target_variant.
  destruct (ignored_variant_preserves_projection_mutual projection_rules)
    as [projection_case _].
  destruct (ignored_variant_preserves_effects_mutual projection_rules)
    as [effects_case _].
  destruct (ignored_variant_preserves_acceptance_mutual projection_rules)
    as [acceptance_case _].
  pose proof (projection_case pattern changed_pattern pattern_variant)
    as same_projection.
  destruct (effects_case pattern changed_pattern pattern_variant)
    as [same_effects same_walks].
  pose proof (ignored_variant_preserves_root_kind projection_rules pattern
    changed_pattern pattern_variant) as same_root.
  pose proof (acceptance_case target changed_target target_variant pattern)
    as same_acceptance.
  split; [symmetry; exact same_projection |].
  unfold table_prefilter_keeps, atom_effects.
  rewrite <- same_projection, <- same_effects, <- same_walks, <- same_root,
    <- same_acceptance.
  reflexivity.
Qed.

Theorem declined_root_never_prunes :
  forall type_id root_fields patterns target,
    declines projection_rules (type_name_of_id type_id) = true ->
    compile_atom projection_rules (NodeTerm type_id root_fields) = None /\
    table_prefilter_keeps projection_rules (NodeTerm type_id root_fields) target
      = true /\
    compile_query projection_rules (NodeTerm type_id root_fields :: patterns)
      = None.
Proof.
  intros type_id root_fields patterns target declined.
  assert (declined_scan : forall rest,
    scan_source_query_effects initial_cursor_scan
      (atom_effects projection_rules (NodeTerm type_id root_fields) ++ rest)
      = None).
  { intros rest. unfold atom_effects. cbn [term_effects].
    rewrite declined. reflexivity. }
  pose proof (declined_scan []) as root_scan.
  rewrite app_nil_r in root_scan.
  split; [unfold compile_atom; rewrite root_scan; reflexivity |].
  split.
  - unfold table_prefilter_keeps.
    exact (@source_duplicate_or_unsupported_fallback_never_prunes
      (atom_effects projection_rules (NodeTerm type_id root_fields))
      (fun _ => query_accepts
         (project_term projection_rules (NodeTerm type_id root_fields)) target)
      root_scan).
  - unfold compile_query. cbn [flat_map].
    rewrite declined_scan. reflexivity.
Qed.

Theorem index_filters_preserve_first_admissible_binding :
  forall (Binding : Type)
    (admissible : @matched_candidate (list term) Binding -> bool)
    (accepts_remainder : bool) (compiled : list query)
    (candidates : list (@matched_candidate (list term) Binding)),
    (forall candidate, admissible candidate = true ->
       matches_structure accepts_remainder compiled (fst candidate) = true) ->
    filter admissible (candidate_pool_filter accepts_remainder compiled candidates)
      = filter admissible candidates /\
    first_admissible admissible
      (candidate_pool_filter accepts_remainder compiled candidates)
      = first_admissible admissible candidates /\
    filter admissible
      (list_target_group_filter accepts_remainder compiled candidates)
      = filter admissible candidates /\
    first_admissible admissible
      (list_target_group_filter accepts_remainder compiled candidates)
      = first_admissible admissible candidates.
Proof.
  intros Binding admissible accepts_remainder compiled candidates accepted.
  assert (group_keeps : forall candidate, admissible candidate = true ->
    list_target_group_keep accepts_remainder compiled (fst candidate) = true).
  { intros candidate eligible.
    apply list_target_group_keep_accepts.
    exact (accepted candidate eligible). }
  unfold candidate_pool_filter, list_target_group_filter.
  split; [exact (filter_keeps_every_admissible _ admissible _ candidates
    accepted) |].
  split; [exact (admissible_preserving_filter _ _ admissible _ candidates
    accepted) |].
  split; [exact (filter_keeps_every_admissible _ admissible _ candidates
    group_keeps) |].
  exact (admissible_preserving_filter _ _ admissible _ candidates group_keeps).
Qed.

Lemma spatial_sequence_matches_structure :
  forall accepts_remainder patterns structure,
    patterns_conform projection_rules patterns ->
    spatial_sequence accepts_remainder structure patterns ->
    matches_structure accepts_remainder
      (map (project_term projection_rules) patterns) structure = true.
Proof.
  intros accepts_remainder patterns.
  induction patterns as [| pattern patterns_rest induction_hypothesis];
    intros [| datum structure_rest] conform spatial;
    cbn [spatial_sequence] in spatial.
  - reflexivity.
  - cbn [map matches_structure]. exact spatial.
  - destruct spatial.
  - cbn [patterns_conform] in conform.
    destruct conform as [pattern_conform rest_conform].
    destruct spatial as [datum_spatial rest_spatial].
    cbn [map matches_structure].
    rewrite (proj1 (table_projection_preserves_spatial_success datum pattern
      pattern_conform datum_spatial)).
    exact (induction_hypothesis structure_rest rest_conform rest_spatial).
Qed.

Theorem table_query_filters_preserve_first_admissible_binding :
  forall (Binding : Type)
    (admissible : @matched_candidate (list term) Binding -> bool)
    (accepts_remainder : bool) (patterns : list term) (compiled : list query)
    (candidates : list (@matched_candidate (list term) Binding)),
    patterns_conform projection_rules patterns ->
    compile_query projection_rules patterns = Some compiled ->
    (forall candidate, admissible candidate = true ->
       spatial_sequence accepts_remainder (fst candidate) patterns) ->
    filter admissible (candidate_pool_filter accepts_remainder compiled candidates)
      = filter admissible candidates /\
    first_admissible admissible
      (candidate_pool_filter accepts_remainder compiled candidates)
      = first_admissible admissible candidates /\
    filter admissible
      (list_target_group_filter accepts_remainder compiled candidates)
      = filter admissible candidates /\
    first_admissible admissible
      (list_target_group_filter accepts_remainder compiled candidates)
      = first_admissible admissible candidates.
Proof.
  intros Binding admissible accepts_remainder patterns compiled candidates
    conform compiled_query spatial.
  rewrite (compile_query_projects projection_rules patterns compiled
    compiled_query).
  apply index_filters_preserve_first_admissible_binding.
  intros candidate eligible.
  exact (spatial_sequence_matches_structure accepts_remainder patterns
    (fst candidate) conform (spatial candidate eligible)).
Qed.

(** ** Three false-negative mutants *)

Section Counterexamples.

Local Open Scope string_scope.

Definition send_id : nat := type_id_of "Send".
Definition par_id : nat := type_id_of "Par".
Definition cost_region_id : nat := type_id_of "CostRegion".

Definition channel_summary : opaque_par_summary :=
  OpaqueParSummary 1 1 (repeat 0 32).

Definition ground_channel : term :=
  NodeTerm par_id
    (FieldCons "components" (OpaqueParPayload channel_summary)
      (FieldCons "locally_free" (ScalarPayload [])
        (FieldCons "connective_used" (ScalarPayload [0]) NoFields))).

Definition counterexample_send
    (channel : term) (connective_used locally_free : bytes)
    (cost_region : payload) : term :=
  NodeTerm send_id
    (FieldCons "chan" (ChildrenPayload [] (TermCons channel NoTerms))
      (FieldCons "data" (ChildrenPayload [0] NoTerms)
        (FieldCons "persistent" (ScalarPayload [0])
          (FieldCons "locally_free" (ScalarPayload locally_free)
            (FieldCons "connective_used" (ScalarPayload connective_used)
              (FieldCons "cost_region" cost_region NoFields)))))).

Definition counterexample_pattern : term :=
  counterexample_send (WildcardTerm [0]) [1] [] (ScalarPayload []).

Definition locally_free_target : term :=
  counterexample_send ground_channel [0] [1] (ScalarPayload []).

Definition cost_region_node : term :=
  NodeTerm cost_region_id
    (FieldCons "version" (ScalarPayload [1])
      (FieldCons "start_line" (ScalarPayload [1])
        (FieldCons "start_column" (ScalarPayload [1])
          (FieldCons "end_line" (ScalarPayload [1])
            (FieldCons "end_column" (ScalarPayload [2])
              (FieldCons "signer" (ScalarPayload [])
                (FieldCons "ordinal" (ScalarPayload [0]) NoFields))))))).

Definition cost_region_target : term :=
  counterexample_send ground_channel [0] []
    (ChildrenPayload [] (TermCons cost_region_node NoTerms)).

Definition prefix_target : term :=
  counterexample_send ground_channel [0] [] (ScalarPayload []).

Definition locally_free_exact_rules : list projection_row :=
  make_field_exact "locally_free" projection_rules.

Definition cost_region_exact_rules : list projection_row :=
  make_field_exact "cost_region" projection_rules.

End Counterexamples.

Definition counterexample_query : query :=
  project_term projection_rules counterexample_pattern.

Definition prefix_candidates : list (@matched_candidate (list term) nat) :=
  [([prefix_target], 0)].

Lemma counterexample_pattern_conforms :
  term_conforms projection_rules counterexample_pattern.
Proof.
  vm_compute.
  repeat split; intro absent; discriminate absent.
Qed.

Lemma counterexample_pattern_accepts_every_variant :
  forall channel connective_used locally_free cost_region,
    spatial_term (counterexample_send channel connective_used locally_free
      cost_region) counterexample_pattern.
Proof.
  intros channel connective_used locally_free cost_region.
  unfold counterexample_pattern, counterexample_send.
  apply SpatialNode.
  apply SpatialKeptField.
  { apply SpatialChildren.
    apply SpatialTermCons; [apply SpatialWildcard | apply SpatialNoTerms]. }
  apply SpatialKeptField; [apply SpatialChildren; apply SpatialNoTerms |].
  apply SpatialKeptField; [apply SpatialScalar |].
  apply SpatialIgnoredField; [reflexivity |].
  apply SpatialIgnoredField; [reflexivity |].
  apply SpatialIgnoredField; [reflexivity |].
  apply SpatialNoFields.
Qed.

Theorem locally_free_exact_mutant_is_a_false_negative :
  spatial_term locally_free_target counterexample_pattern /\
  table_prefilter_keeps projection_rules counterexample_pattern
    locally_free_target = true /\
  rows_respect_authority_ignoredb locally_free_exact_rules = false /\
  table_prefilter_keeps locally_free_exact_rules counterexample_pattern
    locally_free_target = false.
Proof.
  pose proof (counterexample_pattern_accepts_every_variant ground_channel [0] [1]
    (ScalarPayload [])) as spatial.
  split; [exact spatial |].
  split; [exact (proj2 (table_projection_preserves_spatial_success
    locally_free_target counterexample_pattern counterexample_pattern_conforms
    spatial)) |].
  split; vm_compute; reflexivity.
Qed.

Theorem cost_region_exact_mutant_is_a_false_negative :
  spatial_term cost_region_target counterexample_pattern /\
  table_prefilter_keeps projection_rules counterexample_pattern
    cost_region_target = true /\
  rows_respect_authority_ignoredb cost_region_exact_rules = false /\
  table_prefilter_keeps cost_region_exact_rules counterexample_pattern
    cost_region_target = false.
Proof.
  pose proof (counterexample_pattern_accepts_every_variant ground_channel [0] []
    (ChildrenPayload [] (TermCons cost_region_node NoTerms))) as spatial.
  split; [exact spatial |].
  split; [exact (proj2 (table_projection_preserves_spatial_success
    cost_region_target counterexample_pattern counterexample_pattern_conforms
    spatial)) |].
  split; vm_compute; reflexivity.
Qed.

Theorem extended_retained_prefix_mutant_is_a_false_negative :
  spatial_term prefix_target counterexample_pattern /\
  compile_query projection_rules [counterexample_pattern] =
    Some [counterexample_query] /\
  list_target_group_filter false [counterexample_query] prefix_candidates =
    prefix_candidates /\
  extended_group_filter false [counterexample_query] prefix_candidates = [] /\
  first_admissible (fun _ => true) prefix_candidates =
    Some ([prefix_target], 0) /\
  first_admissible (fun _ => true)
    (extended_group_filter false [counterexample_query] prefix_candidates)
    = None.
Proof.
  split; [exact (counterexample_pattern_accepts_every_variant ground_channel [0]
    [] (ScalarPayload [])) |].
  split; [reflexivity |].
  split; [reflexivity |].
  split; [reflexivity |].
  split; reflexivity.
Qed.

Print Assumptions projection_rules_respect_authority.
Print Assumptions table_projection_preserves_spatial_success.
Print Assumptions ignored_field_changes_never_change_acceptance.
Print Assumptions declined_root_never_prunes.
Print Assumptions index_filters_preserve_first_admissible_binding.
Print Assumptions table_query_filters_preserve_first_admissible_binding.
Print Assumptions locally_free_exact_mutant_is_a_false_negative.
Print Assumptions cost_region_exact_mutant_is_a_false_negative.
Print Assumptions extended_retained_prefix_mutant_is_a_false_negative.
