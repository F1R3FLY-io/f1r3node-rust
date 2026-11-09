(** * The score-class table versus the reducers' behaviour

    Leaf L14 of the score-arms design, rpb3-score-class-behaviour.

    RPB3ScoreClass assigns every generated program type a score class.  This
    theory proves, parametrically in the reducer, that the table matches what
    the reducers actually do.  The check is a function of an evidence function,
    so the same check can be run against a mutant reducer:

    - [class_witness_ok evidence entry] reads the class of the entry's type in
      [score_class_of] and asks [evidence] about the entry: NoStandalone and
      Opaque types must answer [ScoreNotDerived], [Unclassified] is never
      acceptable, every other class must answer [ExactScore];
    - [score_class_witnesses] holds one row per scored type (a generated type
      whose class is neither NoStandalone nor Opaque), and one row each for the
      Connective arms And, Or and VarRef, so that a partial arm is reported;
      [missing_score_arms evidence] lists the type ids whose row fails;
    - [score_class_behaviour] says that [score_evidence_for] fails no row;
    - [no_score_classes_never_score] covers the sixteen NoStandalone and
      Opaque ids universally over completeness and children, on both reducers.

    The oracle is checked in parallel: [source_score_class_witnesses] renders
    the same rows over source children, [missing_source_score_arms] is the
    same check over [option source_score], [source_score_class_behaviour] is
    [source_message_score]'s verdict, and
    [source_score_class_witnesses_project] shows that the two tables are the
    same data through [project_source_summary].

    Witnesses are not arbitrary.  [witness_rows] uses the child summaries that
    a reduction produces for those fields, and
    [score_class_witnesses_are_reachable] with its oracle twin shows that each
    row's children pass the generated program's scan and that the row's
    completeness is the one the reducer computes for those children.

    Only the kind of each answer (exact or underived, [Some] or [None]) is
    checked, never a concrete score value, so nothing here depends on how Par
    family scores are ordered.

    Red controls: [score_class_behaviour_red_control] removes arm 28,
    [connective_arm_red_controls] removes single Connective arms,
    [source_score_class_behaviour_red_control] is the oracle's arm 28, and
    [class_witness_ok_rejects_forged_no_score_evidence] and
    [class_witness_ok_rejects_unclassified_types] exercise the branches of the
    check that the witness table itself never reaches. *)

From Stdlib Require Import List Bool Arith NArith.
From StackSafePDA Require Import RPB3ScoreClass RPB3SemanticReducer
  RPB3GeneratedFieldRefinement RPB3IndependentOracleCore.
From StackSafePDA Require EPM1 RPB3AuxiliaryTypedReference
  RPB3IndependentOracleAdmission RPB3IndependentOracleProcess
  RPB3IndependentOracleRecursive RPB3IndependentOracleProjection.

Import ListNotations.

(** ** Witness children

    A row's children are given by shape, so that the target table and the
    oracle table are two renderings of the same rows.  Each shape is the child
    that a reduction produces for that field, read from the child's own arm of
    the corresponding reducer:

    - [WitnessPar tag]: an empty Par field; its score is the empty Par score
      and a Par has no completeness of its own;
    - [WitnessVar]: an empty Var at tag 1, the operand of an EVar; a Var is
      always complete and an absent oneof scores the integer 0;
    - [WitnessConnectiveBody tag]: a ConnectiveBody of two empty Par operands
      at tag 1 or 2 (ConnAndBody or ConnOrBody), carrying the constructor 0
      forest of the two operand scores;
    - [WitnessVarRef]: a VarRef with default index and depth at tag 4;
    - [WitnessPathMapEntries]: the auxiliary snapshot field of an EPathMap at
      tag 9, whose evidence is the raw snapshot bytes;
    - [WitnessEPathMap]: an EPathMap message at tag 1 of an EZipper. *)
Inductive witness_child : Type :=
| WitnessPar (tag : nat)
| WitnessVar
| WitnessConnectiveBody (tag : nat)
| WitnessVarRef
| WitnessPathMapEntries
| WitnessEPathMap.

(** The snapshot of the EPathMap witnesses is the canonical empty map of
    RPB3AuxiliaryTypedReference, so its bytes are the actual wire encoding. *)
Definition witness_snapshot : EPM1.epm1_snapshot :=
  RPB3AuxiliaryTypedReference.empty_pathmap_snapshot.

Definition witness_snapshot_bytes : list nat :=
  EPM1.encode_epm1 witness_snapshot.

Definition witness_epathmap_body : list nat :=
  RPB3AuxiliaryTypedReference.empty_pathmap_wire.

(** *** Rendering a witness child for the target *)
Definition target_witness_child (child : witness_child) : semantic_summary :=
  match child with
  | WitnessPar tag => auxiliary_empty_par_child tag
  | WitnessVar =>
      SemanticSummary 1 LengthDelimited Message 6 [] false (Some true)
        (ExactScore (ScoreInteger 0)) false false
  | WitnessConnectiveBody tag =>
      SemanticSummary tag LengthDelimited Message 57 [10; 0; 10; 0] false None
        (ExactScore (score_constructor 0 [empty_par_score; empty_par_score]))
        false false
  | WitnessVarRef =>
      SemanticSummary 4 LengthDelimited Message 56 [] false None
        (ExactScore (score_constructor 403 [ScoreInteger 0; ScoreInteger 0]))
        false false
  | WitnessPathMapEntries =>
      SemanticSummary 9 LengthDelimited PathMap 0 witness_snapshot_bytes false
        (Some true) (ExactScore (ScoreBytes witness_snapshot_bytes)) false false
  | WitnessEPathMap =>
      SemanticSummary 1 LengthDelimited Message 31 witness_epathmap_body false
        (Some true)
        (ExactScore (score_constructor 13
          [ScoreBytes witness_snapshot_bytes; ScoreInteger 0]))
        false false
  end.

(** *** Rendering a witness child for the oracle *)
Definition source_witness_message (tag type_id : nat) (body : list nat)
    (children : list typed_source_value) : typed_source_value :=
  TypedSourceMessage
    (RawField tag LengthDelimited Message body (length body)
      (Some type_id) [] [])
    children.

Definition source_witness_pathmap : typed_source_value :=
  TypedSourcePathMap
    (RawField 9 LengthDelimited PathMap witness_snapshot_bytes
      (length witness_snapshot_bytes) None [] [])
    witness_snapshot [].

Definition source_witness_analysis (complete : option bool)
    (score : source_score) : source_family_analysis :=
  SourceFamilyAnalysis complete (Some score) false false false.

Definition source_witness_child (child : witness_child)
    : source_exact_summary :=
  match child with
  | WitnessPar tag =>
      SourceExactSummary (source_witness_message tag 1 [] [])
        (source_witness_analysis None
          RPB3IndependentOracleProcess.source_empty_par_score)
        (Some true)
  | WitnessVar =>
      SourceExactSummary (source_witness_message 1 6 [] [])
        (source_witness_analysis (Some true) (SourceInteger 0))
        None
  | WitnessConnectiveBody tag =>
      SourceExactSummary
        (source_witness_message tag 57 [10; 0; 10; 0]
          [source_witness_message 1 1 [] [];
           source_witness_message 1 1 [] []])
        (source_witness_analysis None
          (source_score_constructor 0
            [RPB3IndependentOracleProcess.source_empty_par_score;
             RPB3IndependentOracleProcess.source_empty_par_score]))
        None
  | WitnessVarRef =>
      SourceExactSummary (source_witness_message 4 56 [] [])
        (source_witness_analysis None
          (source_score_constructor 403 [SourceInteger 0; SourceInteger 0]))
        None
  | WitnessPathMapEntries =>
      SourceExactSummary source_witness_pathmap
        (source_witness_analysis (Some true)
          (SourceBytes witness_snapshot_bytes))
        None
  | WitnessEPathMap =>
      SourceExactSummary
        (source_witness_message 1 31 witness_epathmap_body
          [source_witness_pathmap])
        (source_witness_analysis (Some true)
          (source_score_constructor 13
            [SourceBytes witness_snapshot_bytes; SourceInteger 0]))
        None
  end.

(** ** The witness rows

    A row is a type id, the completeness that the reducers compute for its
    children, and the children.  The completeness is [None] for the families
    that have no completeness notion (Par, the Cost parts, ReceiveBind,
    MatchCase and the carriers 56 and 57) and [Some true] for every other row,
    whose children are complete under the generated program. *)
Definition binary_children : list witness_child :=
  [WitnessPar 1; WitnessPar 2].

Definition witness_rows : list (nat * option bool * list witness_child) :=
  [ (1, None, []);                                  (* Par *)
    (6, Some true, []);                             (* Var *)
    (8, Some true, [WitnessPar 1]);                 (* Bundle *)
    (9, None, []);                                  (* CostSignature *)
    (10, None, []);                                 (* CostSignatureAtom *)
    (11, None, []);                                 (* CostBoundSignature *)
    (12, None, []);                                 (* CostRegion *)
    (13, Some true, []);                            (* CostTokenStack *)
    (15, Some true, [WitnessPar 1]);                (* Send *)
    (16, None, [WitnessPar 2]);                     (* ReceiveBind *)
    (19, Some true, [WitnessPar 2]);                (* Receive *)
    (20, Some true, [WitnessPar 2]);                (* New *)
    (21, None, [WitnessPar 1; WitnessPar 2]);       (* MatchCase *)
    (22, Some true, [WitnessPar 1]);                (* Match *)
    (23, Some true,
      [WitnessPar 1; WitnessPar 2; WitnessPar 3]);  (* If *)
    (24, Some true, []);                            (* Expr *)
    (27, Some true, []);                            (* EList *)
    (28, Some true, []);                            (* ETuple *)
    (29, Some true, []);                            (* ESet *)
    (30, Some true, []);                            (* EMap *)
    (31, Some true, [WitnessPathMapEntries]);       (* EPathMap *)
    (32, Some true, [WitnessEPathMap]);             (* EZipper *)
    (33, Some true, [WitnessPar 2]);                (* EMethod *)
    (34, Some true, binary_children);               (* KeyValuePair carrier *)
    (35, Some true, [WitnessVar]);                  (* EVar *)
    (36, Some true, [WitnessPar 1]);                (* ENot *)
    (37, Some true, [WitnessPar 1]);                (* ENeg *)
    (38, Some true, binary_children);               (* EMult *)
    (39, Some true, binary_children);               (* EDiv *)
    (40, Some true, binary_children);               (* EMod *)
    (41, Some true, binary_children);               (* EPlus *)
    (42, Some true, binary_children);               (* EMinus *)
    (43, Some true, binary_children);               (* ELt *)
    (44, Some true, binary_children);               (* ELte *)
    (45, Some true, binary_children);               (* EGt *)
    (46, Some true, binary_children);               (* EGte *)
    (47, Some true, binary_children);               (* EEq *)
    (48, Some true, binary_children);               (* ENeq *)
    (49, Some true, binary_children);               (* EAnd *)
    (50, Some true, binary_children);               (* EOr *)
    (51, Some true, binary_children);               (* EMatches *)
    (52, Some true, binary_children);               (* EPercentPercent *)
    (53, Some true, binary_children);               (* EPlusPlus *)
    (54, Some true, binary_children);               (* EMinusMinus *)
    (55, Some true, []);                            (* Connective, absent *)
    (55, Some true, [WitnessConnectiveBody 1]);     (* Connective And *)
    (55, Some true, [WitnessConnectiveBody 2]);     (* Connective Or *)
    (55, Some true, [WitnessVarRef]);               (* Connective VarRef *)
    (56, None, []);                                 (* VarRef carrier *)
    (57, None, [WitnessPar 1; WitnessPar 1]);       (* ConnectiveBody carrier *)
    (60, Some true, [])                             (* GUnforgeable, absent *)
  ].

Definition render_witness_rows {child : Type} (render : witness_child -> child)
    (rows : list (nat * option bool * list witness_child))
    : list (nat * option bool * list child) :=
  map (fun row : nat * option bool * list witness_child =>
    (fst (fst row), snd (fst row), map render (snd row))) rows.

Definition score_class_witnesses
    : list (nat * option bool * list semantic_summary) :=
  render_witness_rows target_witness_child witness_rows.

Definition source_score_class_witnesses
    : list (nat * option bool * list source_exact_summary) :=
  render_witness_rows source_witness_child witness_rows.

(** ** The check, parametric in the reducer

    Both tables are read through [class_accepts]: the verdict of the table
    entry's class on whether the reducer's answer for the entry was exact. *)
Definition class_accepts (class : score_class) (exact : bool) : bool :=
  match class with
  | ClassNoStandalone | ClassOpaque => negb exact
  | Unclassified => false
  | _ => exact
  end.

Definition class_witness_ok
    (evidence : nat -> option bool -> list semantic_summary -> score_evidence)
    (entry : nat * option bool * list semantic_summary) : bool :=
  let '(type_id, complete, children) := entry in
  class_accepts (score_class_of type_id)
    (match evidence type_id complete children with
     | ExactScore _ => true
     | ScoreNotDerived => false
     end).

Definition entry_type {child : Type}
    (entry : nat * option bool * list child) : nat :=
  fst (fst entry).

Definition missing_score_arms
    (evidence : nat -> option bool -> list semantic_summary -> score_evidence)
    : list nat :=
  map entry_type
    (filter (fun entry => negb (class_witness_ok evidence entry))
      score_class_witnesses).

(** The oracle's parallel check, over [option source_score]. *)
Definition source_witness_ok
    (oracle : nat -> option bool -> list source_exact_summary ->
      option source_score)
    (entry : nat * option bool * list source_exact_summary) : bool :=
  let '(type_id, complete, children) := entry in
  class_accepts (score_class_of type_id)
    (match oracle type_id complete children with
     | Some _ => true
     | None => false
     end).

Definition missing_source_score_arms
    (oracle : nat -> option bool -> list source_exact_summary ->
      option source_score)
    : list nat :=
  map entry_type
    (filter (fun entry => negb (source_witness_ok oracle entry))
      source_score_class_witnesses).

(** ** The table's classes *)

(** The sixteen ids with no score at all: NoStandalone [7; 25; 26; 61; 62; 63;
    64] and Opaque [2; 3; 4; 5; 14; 17; 18; 58; 59]. *)
Definition unscored_ids : list nat :=
  [7; 25; 26; 61; 62; 63; 64; 2; 3; 4; 5; 14; 17; 18; 58; 59].

Definition class_is_scored (class : score_class) : bool :=
  match class with
  | ClassNoStandalone | ClassOpaque => false
  | _ => true
  end.

Definition scored_type_ids : list nat :=
  filter (fun type_id => class_is_scored (score_class_of type_id))
    generated_type_ids.

Theorem unscored_ids_are_the_no_score_classes :
  unscored_ids =
    classified_ids ClassNoStandalone ++ classified_ids ClassOpaque /\
  length unscored_ids = 16.
Proof. vm_compute. split; reflexivity. Qed.

Theorem scored_type_ids_are_the_48 :
  scored_type_ids =
    [1; 6; 8; 9; 10; 11; 12; 13; 15; 16; 19; 20; 21; 22; 23; 24;
     27; 28; 29; 30; 31; 32; 33; 34; 35; 36; 37; 38; 39; 40; 41; 42; 43; 44;
     45; 46; 47; 48; 49; 50; 51; 52; 53; 54; 55; 56; 57; 60] /\
  length scored_type_ids = 48.
Proof. vm_compute. split; reflexivity. Qed.

(** The table holds exactly the scored types, one row each, plus the three
    extra Connective arm rows. *)
Theorem score_class_witnesses_cover_the_scored_types :
  nodup Nat.eq_dec (map entry_type score_class_witnesses) = scored_type_ids /\
  length score_class_witnesses = 51.
Proof. vm_compute. split; reflexivity. Qed.

(** The Connective rows are the absent oneof and the And, Or and VarRef arms,
    selected by the child's tag. *)
Theorem score_class_witnesses_connective_arms :
  map (fun entry : nat * option bool * list semantic_summary =>
        map sem_tag (snd entry))
    (filter (fun entry : nat * option bool * list semantic_summary =>
        entry_type entry =? 55) score_class_witnesses) =
  [[]; [1]; [2]; [4]].
Proof. vm_compute. reflexivity. Qed.

(** ** The behaviour theorems *)

(** Every row, including the partial arms, is answered as its class demands. *)
Theorem score_class_behaviour :
  missing_score_arms score_evidence_for = [].
Proof. vm_compute. reflexivity. Qed.

(** The oracle's verdict over the parallel table. *)
Theorem source_score_class_behaviour :
  missing_source_score_arms
    RPB3IndependentOracleRecursive.source_message_score = [].
Proof. vm_compute. reflexivity. Qed.

(** A sixteen-case split over the listed ids; each case reduces by computation
    to the default arm of every dispatch, for every completeness and all
    children, on both reducers. *)
Theorem no_score_classes_never_score :
  forall t complete children source_children,
    In t unscored_ids ->
    score_evidence_for t complete children = ScoreNotDerived /\
    RPB3IndependentOracleRecursive.source_message_score t complete
      source_children = None.
Proof.
  intros t complete children source_children member.
  unfold unscored_ids in member.
  repeat (destruct member as [same|member];
    [subst t; destruct complete as [[|]|]; split; reflexivity|]).
  exfalso. exact member.
Qed.

(** An empty report means that every row was accepted, on both sides.

    The fact is proved once over an abstract list of rows, and each table
    instantiates it as a whole function.  That shape is what keeps [Qed] fast:
    the kernel compares the generic statement against the report with the
    report on the side it unfolds first, so [map] and [filter] then meet with
    equal arguments.  The other way round, a hypothesis
    [missing_score_arms evidence = []] used at the expected type
    [map entry_type (filter F W) = []] makes the kernel unfold the concrete
    table and compare both branches of every row's [if], each stuck on the
    free [evidence], which is exponential in the rows: measured on prefixes of
    the table, every two extra rows cost a factor of about 4, and a [Qed] of
    that shape over all the rows did not finish in 56 minutes. *)
Lemma rows_accepted_of_empty_report :
  forall (row : Type) (accepted : row -> bool) (row_id : row -> nat)
    (rows : list row),
    map row_id (filter (fun entry => negb (accepted entry)) rows) = [] ->
    forall entry, In entry rows -> accepted entry = true.
Proof.
  intros row accepted row_id rows none entry member.
  apply map_eq_nil in none.
  destruct (accepted entry) eqn:verdict; [reflexivity|].
  exfalso.
  assert (kept : In entry
      (filter (fun candidate => negb (accepted candidate)) rows)).
  { apply filter_In. split.
    - exact member.
    - change (negb (accepted entry) = true).
      rewrite verdict. reflexivity. }
  rewrite none in kept.
  exact kept.
Qed.

Lemma missing_score_arms_nil_means_all_rows_ok :
  forall evidence,
    missing_score_arms evidence = [] ->
    forall entry, In entry score_class_witnesses ->
      class_witness_ok evidence entry = true.
Proof.
  intro evidence.
  exact (rows_accepted_of_empty_report _ (class_witness_ok evidence)
    entry_type score_class_witnesses).
Qed.

Lemma missing_source_score_arms_nil_means_all_rows_ok :
  forall oracle,
    missing_source_score_arms oracle = [] ->
    forall entry, In entry source_score_class_witnesses ->
      source_witness_ok oracle entry = true.
Proof.
  intro oracle.
  exact (rows_accepted_of_empty_report _ (source_witness_ok oracle)
    entry_type source_score_class_witnesses).
Qed.

(** ** The two tables are the same data

    Rendering a row for the oracle and projecting its children yields the row
    rendered for the target.  Only the children are compared, not any answer. *)
Definition project_witness_row
    (row : nat * option bool * list source_exact_summary)
    : nat * option bool * list semantic_summary :=
  (fst (fst row), snd (fst row),
   map RPB3IndependentOracleProjection.project_source_summary (snd row)).

Theorem source_score_class_witnesses_project :
  map project_witness_row source_score_class_witnesses =
  score_class_witnesses.
Proof. vm_compute. reflexivity. Qed.

(** ** The witnesses are reachable

    For each row, the children pass the scan of the row's generated program
    and the row's completeness is the one the reducer computes for those
    children; the oracle's admission scan and completeness agree on the
    parallel table. *)
Definition option_bool_eqb (left right : option bool) : bool :=
  match left, right with
  | Some a, Some b => Bool.eqb a b
  | None, None => true
  | _, _ => false
  end.

Definition row_is_reachable
    (entry : nat * option bool * list semantic_summary) : bool :=
  let '(type_id, complete, children) := entry in
  match lookup_projected_program type_id [] [] with
  | None => false
  | Some program =>
      let scanned := fold_left (scan_child program) children empty_scan in
      scan_valid scanned &&
      option_bool_eqb (component_completeness type_id scanned children) complete
  end.

Definition source_row_is_reachable
    (entry : nat * option bool * list source_exact_summary) : bool :=
  let '(type_id, complete, children) := entry in
  match lookup_projected_program type_id [] [] with
  | None => false
  | Some program =>
      let scanned :=
        RPB3IndependentOracleAdmission.source_scan_children program children in
      RPB3IndependentOracleAdmission.source_scan_valid scanned &&
      option_bool_eqb
        (RPB3IndependentOracleRecursive.source_message_completeness type_id
          scanned children) complete
  end.

Theorem score_class_witnesses_are_reachable :
  forallb row_is_reachable score_class_witnesses = true.
Proof. vm_compute. reflexivity. Qed.

Theorem source_score_class_witnesses_are_reachable :
  forallb source_row_is_reachable source_score_class_witnesses = true.
Proof. vm_compute. reflexivity. Qed.

(** ** Red controls *)

(** Removing arm 28 from the target: exactly the ETuple row is reported. *)
Definition score_evidence_without_28 (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : score_evidence :=
  if type_id =? 28 then ScoreNotDerived
  else score_evidence_for type_id complete children.

Theorem score_class_behaviour_red_control :
  missing_score_arms score_evidence_without_28 = [28].
Proof. vm_compute. reflexivity. Qed.

(** Removing one Connective arm: only that arm's row fails, so the table
    reports the partial arm; [[1; 2; 4]] are And, Or and VarRef. *)
Definition score_evidence_without_connective_arm (arm_tag : nat)
    (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : score_evidence :=
  if (type_id =? 55) &&
     existsb (fun child => sem_tag child =? arm_tag) children
  then ScoreNotDerived
  else score_evidence_for type_id complete children.

Theorem connective_arm_red_controls :
  map (fun arm_tag =>
    missing_score_arms (score_evidence_without_connective_arm arm_tag))
    [1; 2; 4] =
  [[55]; [55]; [55]].
Proof. vm_compute. reflexivity. Qed.

(** Removing arm 28 from the oracle: exactly the ETuple row is reported. *)
Definition source_message_score_without_28 (type_id : nat)
    (complete : option bool) (children : list source_exact_summary)
    : option source_score :=
  if type_id =? 28 then None
  else RPB3IndependentOracleRecursive.source_message_score type_id complete
    children.

Theorem source_score_class_behaviour_red_control :
  missing_source_score_arms source_message_score_without_28 = [28].
Proof. vm_compute. reflexivity. Qed.

(** The witness table has no row for a no-score or an unclassified type, so
    these two controls exercise the remaining branches of the check.  A forged
    score on a NoStandalone or Opaque type is rejected; an unclassified type
    is rejected whatever the reducer answers. *)
Definition forged_score_evidence (type_id : nat) (complete : option bool)
    (children : list semantic_summary) : score_evidence :=
  ExactScore (ScoreInteger 0).

Example class_witness_ok_rejects_forged_no_score_evidence :
  class_witness_ok forged_score_evidence (7, None, []) = false /\
  class_witness_ok forged_score_evidence (2, Some true, []) = false /\
  class_witness_ok score_evidence_for (7, None, []) = true.
Proof. vm_compute. repeat split; reflexivity. Qed.

Example class_witness_ok_rejects_unclassified_types :
  class_witness_ok forged_score_evidence (65, Some true, []) = false /\
  class_witness_ok score_evidence_for (0, None, []) = false.
Proof. vm_compute. split; reflexivity. Qed.

Print Assumptions unscored_ids_are_the_no_score_classes.
Print Assumptions scored_type_ids_are_the_48.
Print Assumptions score_class_witnesses_cover_the_scored_types.
Print Assumptions score_class_witnesses_connective_arms.
Print Assumptions score_class_behaviour.
Print Assumptions source_score_class_behaviour.
Print Assumptions no_score_classes_never_score.
Print Assumptions rows_accepted_of_empty_report.
Print Assumptions missing_score_arms_nil_means_all_rows_ok.
Print Assumptions missing_source_score_arms_nil_means_all_rows_ok.
Print Assumptions source_score_class_witnesses_project.
Print Assumptions score_class_witnesses_are_reachable.
Print Assumptions source_score_class_witnesses_are_reachable.
Print Assumptions score_class_behaviour_red_control.
Print Assumptions connective_arm_red_controls.
Print Assumptions source_score_class_behaviour_red_control.
Print Assumptions class_witness_ok_rejects_forged_no_score_evidence.
Print Assumptions class_witness_ok_rejects_unclassified_types.
