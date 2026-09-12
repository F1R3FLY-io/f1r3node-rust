From Stdlib Require Import List String Arith Lia.
From StackSafePDA Require Import StackSafePDA RPB3Admission.

Import ListNotations.
Open Scope string_scope.
Open Scope list_scope.
Set Implicit Arguments.

Module CanonicalSexprProjection.

Definition tokens := list string.

Record label := Label {
  opening : tokens;
  separator : tokens;
  closing : tokens
}.

Definition subject := @tree label.
Definition subjects := @forest label.

Fixpoint join_tokens (sep : tokens) (parts : list tokens) : tokens :=
  match parts with
  | [] => []
  | first :: rest => first ++
      match rest with [] => [] | _ => sep ++ join_tokens sep rest end
  end.

Definition algebra (shape : label) (parts : list tokens) : tokens :=
  opening shape ++ join_tokens (separator shape) parts ++ closing shape.

Fixpoint reference_tree (value : subject) : tokens :=
  match value with
  | Node shape children =>
      opening shape ++ reference_forest (separator shape) children ++ closing shape
  end
with reference_forest (sep : tokens) (values : subjects) : tokens :=
  match values with
  | FNil => []
  | FCons first rest => reference_tree first ++
      match rest with FNil => [] | _ => sep ++ reference_forest sep rest end
  end.

Theorem recursive_reference_is_the_existing_fold :
  (forall value, reference_tree value = fold_tree algebra value) /\
  (forall values sep,
      reference_forest sep values = join_tokens sep (fold_forest algebra values)).
Proof.
  apply tree_forest_induction.
  - intros shape children IH. simpl. unfold algebra. now rewrite IH.
  - intros sep. reflexivity.
  - intros first IHfirst rest IHrest sep.
    simpl. rewrite IHfirst. destruct rest; simpl; auto.
    specialize (IHrest sep). simpl in IHrest. now rewrite IHrest.
Qed.

Theorem existing_fold_machine_has_exact_reference_tokens value :
  run algebra (compile_tree value) [] = Some [reference_tree value].
Proof.
  rewrite pda_fold_equivalent_to_recursive_fold.
  now rewrite (proj1 recursive_reference_is_the_existing_fold).
Qed.

Inductive job :=
| Visit : subject -> job
| Siblings : tokens -> subjects -> job
| Emit : tokens -> job.

Definition job_tokens (work : job) : tokens :=
  match work with
  | Visit value => reference_tree value
  | Siblings sep values => reference_forest sep values
  | Emit value => value
  end.

Fixpoint pending_tokens (work : list job) : tokens :=
  match work with
  | [] => []
  | first :: rest => job_tokens first ++ pending_tokens rest
  end.

Inductive executes : list job -> tokens -> Prop :=
| ExecuteDone : executes [] []
| ExecuteEmit : forall value rest out,
    executes rest out -> executes (Emit value :: rest) (value ++ out)
| ExecuteVisit : forall shape children rest out,
    executes (Emit (opening shape) :: Siblings (separator shape) children ::
      Emit (closing shape) :: rest) out ->
    executes (Visit (Node shape children) :: rest) out
| ExecuteEmpty : forall sep rest out,
    executes rest out -> executes (Siblings sep FNil :: rest) out
| ExecuteLast : forall sep first rest out,
    executes (Visit first :: rest) out ->
    executes (Siblings sep (FCons first FNil) :: rest) out
| ExecuteMore : forall sep first second tail rest out,
    executes (Visit first :: Emit sep :: Siblings sep (FCons second tail) :: rest) out ->
    executes (Siblings sep (FCons first (FCons second tail)) :: rest) out.

Theorem continuation_machine_is_sound work out :
  executes work out -> out = pending_tokens work.
Proof.
  intro H. induction H; simpl in *; rewrite ?IHexecutes;
    repeat rewrite app_nil_r; repeat rewrite app_assoc; reflexivity.
Qed.

Theorem continuation_machine_constructively_completes :
  (forall value rest out, executes rest out ->
    executes (Visit value :: rest) (reference_tree value ++ out)) /\
  (forall values sep rest out, executes rest out ->
    executes (Siblings sep values :: rest) (reference_forest sep values ++ out)).
Proof.
  apply tree_forest_induction.
  - intros shape children IH rest out H. simpl.
    apply ExecuteVisit.
    repeat rewrite <- app_assoc.
    apply ExecuteEmit. apply IH. now apply ExecuteEmit.
  - intros sep rest out H. simpl. now apply ExecuteEmpty.
  - intros first IHfirst tail IHtail sep rest out H.
    destruct tail as [|second more].
    + simpl. rewrite app_nil_r. apply ExecuteLast. now apply IHfirst.
    + change (executes (Siblings sep (FCons first (FCons second more)) :: rest)
        ((reference_tree first ++ (sep ++ reference_forest sep (FCons second more))) ++ out)).
      repeat rewrite <- app_assoc. apply ExecuteMore.
      apply IHfirst. apply ExecuteEmit. now apply IHtail.
Qed.

Theorem streaming_and_recursive_reference_are_exact value out :
  executes [Visit value] out <-> out = reference_tree value.
Proof.
  split.
  - intro H. apply continuation_machine_is_sound in H.
    simpl in H. now rewrite app_nil_r in H.
  - intros ->.
    pose proof (proj1 continuation_machine_constructively_completes
      value [] [] ExecuteDone) as H. now rewrite app_nil_r in H.
Qed.

Theorem streaming_execution_is_deterministic work first second :
  executes work first -> executes work second -> first = second.
Proof.
  intros H1 H2. apply continuation_machine_is_sound in H1.
  apply continuation_machine_is_sound in H2. congruence.
Qed.

Definition leaf (value : tokens) : subject := Node (Label value [] []) FNil.
Definition group (open sep close : tokens) (children : list subject) : subject :=
  Node (Label open sep close) (forest_of_list children).

Lemma reference_leaf value : reference_tree (leaf value) = value.
Proof. simpl. now rewrite app_nil_r. Qed.

Lemma forest_fold_of_list values :
  fold_forest algebra (forest_of_list values) = map (fold_tree algebra) values.
Proof. induction values; simpl; congruence. Qed.

Theorem reference_group open sep close values :
  reference_tree (group open sep close values) =
    open ++ join_tokens sep (map reference_tree values) ++ close.
Proof.
  rewrite (proj1 recursive_reference_is_the_existing_fold).
  unfold group. simpl. unfold algebra. rewrite forest_fold_of_list.
  assert (H : map (fold_tree algebra) values = map reference_tree values).
  { apply map_ext. intro x. symmetry.
    apply (proj1 recursive_reference_is_the_existing_fold). }
  now rewrite H.
Qed.

Fixpoint occurrences (entries : list (subject * nat)) : list subject :=
  match entries with
  | [] => []
  | (value, count) :: rest => repeat value count ++ occurrences rest
  end.

Fixpoint rendered_occurrences (entries : list (subject * nat)) : list tokens :=
  match entries with
  | [] => []
  | (value, count) :: rest => repeat (reference_tree value) count ++ rendered_occurrences rest
  end.

Lemma map_repeat_exact {A B} (f : A -> B) value count :
  map f (repeat value count) = repeat (f value) count.
Proof. induction count; simpl; congruence. Qed.

Theorem render_once_then_repeat_is_exact entries :
  map reference_tree (occurrences entries) = rendered_occurrences entries.
Proof.
  induction entries as [|[value count] rest IH]; simpl; auto.
  now rewrite map_app, map_repeat_exact, IH.
Qed.

Theorem repeated_output_preserves_every_separator entries sep :
  join_tokens sep (map reference_tree (occurrences entries)) =
    join_tokens sep (rendered_occurrences entries).
Proof. now rewrite render_once_then_repeat_is_exact. Qed.

Inductive cardinality := Zero | One | Many.

Definition classify (count : nat) : cardinality :=
  match count with 0 => Zero | 1 => One | _ => Many end.

Definition combine (left right : cardinality) : cardinality :=
  match left, right with
  | Zero, value | value, Zero => value
  | _, _ => Many
  end.

Fixpoint classify_entries (entries : list (subject * nat)) : cardinality :=
  match entries with
  | [] => Zero
  | (_, count) :: rest => combine (classify count) (classify_entries rest)
  end.

Lemma classify_add left right :
  classify (left + right) = combine (classify left) (classify right).
Proof. destruct left as [|[|left]], right as [|[|right]]; reflexivity. Qed.

Theorem three_state_classification_matches_exact_occurrences entries :
  classify_entries entries = classify (List.length (occurrences entries)).
Proof.
  induction entries as [|[value count] rest IH]; simpl; auto.
  rewrite length_app, repeat_length, classify_add. now rewrite IH.
Qed.

Definition project_par (values : list subject) : subject :=
  match values with
  | [] => leaf ["Nil"]
  | [value] => value
  | _ => group ["(par "] [" "] [")"] values
  end.

Theorem par_zero_one_many_are_distinct value next rest :
  reference_tree (project_par []) = ["Nil"] /\
  reference_tree (project_par [value]) = reference_tree value /\
  reference_tree (project_par (value :: next :: rest)) =
    ["(par "] ++ join_tokens [" "] (map reference_tree (value :: next :: rest)) ++ [")"].
Proof. split; [apply reference_leaf|]. split; [reflexivity|apply reference_group]. Qed.

Record family_observation := Families {
  sends : list (subject * nat);
  receives : list (subject * nat);
  news : list (subject * nat);
  expressions : list (subject * nat);
  bundles : list (subject * nat);
  matches : list (subject * nat);
  unforgeables : list (subject * nat);
  connectives : list (subject * nat);
  conditionals : list (subject * nat)
}.

Definition selected_entries (families : family_observation) : list (subject * nat) :=
  sends families ++ receives families ++ news families ++ expressions families ++ bundles families.

Definition project_families (families : family_observation) : subject :=
  project_par (occurrences (selected_entries families)).

Theorem ignored_families_change_neither_rendering_nor_arity s r n e b m u c i m' u' c' i' :
  let left := Families s r n e b m u c i in
  let right := Families s r n e b m' u' c' i' in
  reference_tree (project_families left) = reference_tree (project_families right) /\
  classify_entries (selected_entries left) = classify_entries (selected_entries right).
Proof. split; reflexivity. Qed.

Theorem ignored_only_families_render_nil m u c i :
  reference_tree (project_families (Families [] [] [] [] [] m u c i)) = ["Nil"].
Proof. reflexivity. Qed.

Theorem family_occurrences_preserve_historical_order families :
  occurrences (selected_entries families) =
    occurrences (sends families) ++ occurrences (receives families) ++
    occurrences (news families) ++ occurrences (expressions families) ++
    occurrences (bundles families).
Proof.
  assert (H : forall left right,
    occurrences (left ++ right) = occurrences left ++ occurrences right).
  { induction left as [|[value count] rest IH]; intro right; simpl; auto.
    rewrite IH. now rewrite app_assoc. }
  unfold selected_entries. now repeat rewrite H.
Qed.

Inductive constructor_obligation :=
| ParZero | ParOne | ParMany
| SendCase | ReceiveCase | BindCase | NewCase | BundleCase
| BoolCase | IntCase | StringCase | UriCase | BytesCase
| ListCase | TupleCase | SetCase | MapCase | PairCase
| BoundCase | FreeCase | WildcardCase | MissingVarCase
| NegCase | NotCase | MultCase | DivCase | PlusCase | MinusCase | MethodCase
| MissingExprCase | FallbackExprCase | IgnoredFamilyCase | MissingParCase.

Definition constructor_catalog : list constructor_obligation :=
  [ParZero; ParOne; ParMany; SendCase; ReceiveCase; BindCase; NewCase; BundleCase;
   BoolCase; IntCase; StringCase; UriCase; BytesCase; ListCase; TupleCase; SetCase;
   MapCase; PairCase; BoundCase; FreeCase; WildcardCase; MissingVarCase;
   NegCase; NotCase; MultCase; DivCase; PlusCase; MinusCase; MethodCase;
   MissingExprCase; FallbackExprCase; IgnoredFamilyCase; MissingParCase].

Record field_observation := Fields {
  first_child : subject;
  second_child : subject;
  repeated_children : list subject;
  scalar_text : string;
  binders : list subject
}.

Definition view (kind : constructor_obligation) (fields : field_observation) : subject :=
  let first := first_child fields in
  let second := second_child fields in
  let rest := repeated_children fields in
  let scalar := scalar_text fields in
  let joined := group [] [" "] [] rest in
  match kind with
  | ParZero | MissingExprCase | MissingParCase => leaf ["Nil"]
  | ParOne => first
  | ParMany => group ["(par "] [" "] [")"] (first :: second :: rest)
  | SendCase => group ["(! "] [] [")"] [first; leaf [" "]; joined]
  | ReceiveCase => group ["(for ("] [] [")"] [joined; leaf [") "]; first]
  | BindCase => group ["(bind <- "] [] [")"] [first]
  | NewCase => group ["(new "] [] [")"]
      [group [] [" "] [] (binders fields); leaf [" "]; first]
  | BundleCase => group ["(bundle "] [] [")"] [first]
  | BoolCase | IntCase => leaf [scalar]
  | StringCase => leaf [""""; scalar; """"]
  | UriCase => leaf ["`"; scalar; "`"]
  | BytesCase => leaf ["0x"; scalar]
  | ListCase => group ["["] [" "] ["]"] rest
  | TupleCase => group ["(tuple "] [" "] [")"] rest
  | SetCase => group ["(set "] [" "] [")"] rest
  | MapCase => group ["(map "] [" "] [")"] rest
  | PairCase => group ["("] [" : "] [")"] [first; second]
  | BoundCase => leaf ["_"; scalar]
  | FreeCase => leaf ["$"; scalar]
  | WildcardCase => leaf ["_"]
  | MissingVarCase => leaf ["var"]
  | NegCase => group ["(- "] [] [")"] [first]
  | NotCase => group ["(not "] [] [")"] [first]
  | MultCase => group ["(* "] [" "] [")"] [first; second]
  | DivCase => group ["(/ "] [" "] [")"] [first; second]
  | PlusCase => group ["(+ "] [" "] [")"] [first; second]
  | MinusCase => group ["(- "] [" "] [")"] [first; second]
  | MethodCase => group ["("] [] [")"]
      [first; leaf ["."; scalar; " "]; joined]
  | FallbackExprCase => leaf ["(expr)"]
  | IgnoredFamilyCase => leaf []
  end.

Theorem constructor_catalog_is_exhaustive_and_unique :
  NoDup constructor_catalog /\
  (forall kind, In kind constructor_catalog) /\
  List.length constructor_catalog = 33.
Proof.
  split.
  - unfold constructor_catalog. repeat constructor; simpl; intuition discriminate.
  - split; [intro kind; destruct kind; unfold constructor_catalog; simpl; tauto|reflexivity].
Qed.

Theorem every_constructor_observation_has_exact_streaming_tokens kind fields out :
  executes [Visit (view kind fields)] out <-> out = reference_tree (view kind fields).
Proof. apply streaming_and_recursive_reference_are_exact. Qed.

Theorem ignored_fallback_and_absence_observations_are_distinct fields :
  reference_tree (view IgnoredFamilyCase fields) = [] /\
  reference_tree (view FallbackExprCase fields) = ["(expr)"] /\
  reference_tree (view MissingExprCase fields) = ["Nil"] /\
  reference_tree (view MissingParCase fields) = ["Nil"] /\
  reference_tree (view MissingVarCase fields) = ["var"] /\
  reference_tree (view WildcardCase fields) = ["_"].
Proof. repeat split; apply reference_leaf. Qed.

Definition empty_fields : field_observation := Fields (leaf ["Nil"]) (leaf ["Nil"]) [] "" [].

Theorem empty_shape_controls :
  reference_tree (view ListCase empty_fields) = ["["; "]"] /\
  reference_tree (view TupleCase empty_fields) = ["(tuple "; ")"] /\
  reference_tree (view SendCase empty_fields) = ["(! "; "Nil"; " "; ")"] /\
  reference_tree (view NewCase empty_fields) = ["(new "; " "; "Nil"; ")"] /\
  reference_tree (view MethodCase empty_fields) = ["("; "Nil"; "."; ""; " "; ")"].
Proof. repeat split; reflexivity. Qed.

Definition native_field_view_obligation {Bytes Typed : Type}
  (resident : Bytes -> Prop) (decode : Bytes -> option Typed)
  (typed_projection : Typed -> subject) (byte_projection : Bytes -> option subject) : Prop :=
  forall bytes typed, resident bytes -> decode bytes = Some typed ->
    byte_projection bytes = Some (typed_projection typed).

Module FieldViews.

Module Windows := IndexedWindowTransport.

Inductive slot := FirstChild | SecondChild | RepeatedChildren | Scalar | BinderCount
  | ParSends | ParReceives | ParNews | ParExpressions | ParBundles | IgnoredMetadata.

Definition slot_eq_dec : forall (left right : slot), {left = right} + {left <> right}.
Proof. decide equality. Defined.

Inductive payload := ScalarObservation : tokens -> payload
  | ChildRegion : Windows.window -> payload.

Record field := Field { field_slot : slot; field_payload : payload }.

Fixpoint collect (wanted : slot) (fields : list field) : list payload :=
  match fields with
  | [] => []
  | event :: rest =>
      if slot_eq_dec (field_slot event) wanted
      then field_payload event :: collect wanted rest
      else collect wanted rest
  end.

Fixpoint decoder_repeated (wanted : slot) (fields : list field)
  (accumulated : list payload) : list payload :=
  match fields with
  | [] => accumulated
  | event :: rest => decoder_repeated wanted rest
      (if slot_eq_dec (field_slot event) wanted
       then accumulated ++ [field_payload event] else accumulated)
  end.

Fixpoint decoder_singular (wanted : slot) (fields : list field)
  (current : option payload) : option payload :=
  match fields with
  | [] => current
  | event :: rest => decoder_singular wanted rest
      (if slot_eq_dec (field_slot event) wanted
       then Some (field_payload event) else current)
  end.

Fixpoint cursor_first (wanted : slot) (fields : list field) : option payload :=
  match fields with
  | [] => None
  | event :: rest =>
      if slot_eq_dec (field_slot event) wanted
      then Some (field_payload event) else cursor_first wanted rest
  end.

Fixpoint cursor_run (wanted : slot) (fields : list field) : list payload :=
  match fields with
  | [] => []
  | event :: rest =>
      if slot_eq_dec (field_slot event) wanted
      then field_payload event :: cursor_run wanted rest else []
  end.

Fixpoint cursor_seek (wanted : slot) (fields : list field) : list payload :=
  match fields with
  | [] => []
  | event :: rest =>
      if slot_eq_dec (field_slot event) wanted
      then field_payload event :: cursor_run wanted rest else cursor_seek wanted rest
  end.

Definition with_default (fallback selected : option payload) : option payload :=
  match selected with None => fallback | Some value => Some value end.

Fixpoint last_write (values : list payload) (current : option payload) : option payload :=
  match values with [] => current | first :: rest => last_write rest (Some first) end.

Lemma decoder_repeated_collects_in_source_order wanted fields accumulated :
  decoder_repeated wanted fields accumulated = accumulated ++ collect wanted fields.
Proof.
  revert accumulated. induction fields as [|event rest IH]; intros accumulated; simpl.
  - now rewrite app_nil_r.
  - destruct (slot_eq_dec (field_slot event) wanted); rewrite IH; auto.
    now rewrite <- app_assoc.
Qed.

Lemma decoder_singular_is_last_selected_write wanted fields current :
  decoder_singular wanted fields current = last_write (collect wanted fields) current.
Proof.
  revert current. induction fields as [|event rest IH]; intro current; simpl; auto.
  destruct (slot_eq_dec (field_slot event) wanted); simpl; apply IH.
Qed.

Lemma cursor_first_is_first_selected_value wanted fields :
  cursor_first wanted fields = hd_error (collect wanted fields).
Proof.
  induction fields as [|event rest IH]; simpl; auto.
  destruct (slot_eq_dec (field_slot event) wanted); simpl; auto.
Qed.

Theorem unique_singular_cursor_equals_decoder wanted fields fallback :
  List.length (collect wanted fields) <= 1 ->
  decoder_singular wanted fields fallback = with_default fallback (cursor_first wanted fields).
Proof.
  rewrite decoder_singular_is_last_selected_write, cursor_first_is_first_selected_value.
  destruct (collect wanted fields) as [|first [|second rest]]; simpl; intros; try lia; reflexivity.
Qed.

Definition excludes (wanted : slot) (fields : list field) : Prop :=
  Forall (fun event => field_slot event <> wanted) fields.

Definition selected (wanted : slot) (fields : list field) : Prop :=
  Forall (fun event => field_slot event = wanted) fields.

Definition canonical_run (wanted : slot) (fields : list field) : Prop :=
  exists before body after,
    fields = before ++ body ++ after /\
    excludes wanted before /\ selected wanted body /\ excludes wanted after.

Lemma collect_append wanted left right :
  collect wanted (left ++ right) = collect wanted left ++ collect wanted right.
Proof.
  induction left as [|event rest IH]; simpl; auto.
  destruct (slot_eq_dec (field_slot event) wanted); simpl; now rewrite IH.
Qed.

Lemma excluded_fields_have_no_selected_values wanted fields :
  excludes wanted fields -> collect wanted fields = [].
Proof.
  intro H. induction H; simpl; auto.
  destruct (slot_eq_dec (field_slot x) wanted); congruence.
Qed.

Lemma cursor_seek_excludes_prefix wanted prefix rest :
  excludes wanted prefix -> cursor_seek wanted (prefix ++ rest) = cursor_seek wanted rest.
Proof.
  intro H. induction H; simpl; auto.
  destruct (slot_eq_dec (field_slot x) wanted); congruence.
Qed.

Lemma cursor_seek_excludes_all wanted fields :
  excludes wanted fields -> cursor_seek wanted fields = [].
Proof.
  intro H. pose proof (@cursor_seek_excludes_prefix wanted fields [] H) as E.
  now rewrite app_nil_r in E.
Qed.

Lemma cursor_run_selected_then_excluded wanted body after :
  selected wanted body -> excludes wanted after ->
  cursor_run wanted (body ++ after) = collect wanted body.
Proof.
  intros H. induction H; intro Hafter; simpl.
  - destruct after as [|event rest]; simpl; auto.
    inversion Hafter; subst. destruct (slot_eq_dec (field_slot event) wanted); congruence.
  - destruct (slot_eq_dec (field_slot x) wanted); [simpl; now rewrite IHForall|congruence].
Qed.

Lemma cursor_seek_selected_then_excluded wanted body after :
  selected wanted body -> excludes wanted after ->
  cursor_seek wanted (body ++ after) = collect wanted body.
Proof.
  intros H Hafter. destruct body as [|event rest].
  - simpl. now apply cursor_seek_excludes_all.
  - inversion H; subst. simpl.
    destruct (slot_eq_dec (field_slot event) (field_slot event)); [|congruence].
    f_equal. now apply cursor_run_selected_then_excluded.
Qed.

Theorem ordered_run_cursor_equals_repeated_decoder wanted fields :
  canonical_run wanted fields ->
  cursor_seek wanted fields = decoder_repeated wanted fields [].
Proof.
  intros (before & body & after & -> & Hbefore & Hbody & Hafter).
  rewrite cursor_seek_excludes_prefix by exact Hbefore.
  rewrite cursor_seek_selected_then_excluded by assumption.
  rewrite decoder_repeated_collects_in_source_order. simpl.
  repeat rewrite collect_append.
  rewrite (excluded_fields_have_no_selected_values Hbefore),
    (excluded_fields_have_no_selected_values Hafter). simpl. now rewrite app_nil_r.
Qed.

Theorem absent_singular_preserves_the_decoder_default wanted fields fallback :
  excludes wanted fields ->
  decoder_singular wanted fields fallback = fallback /\ cursor_first wanted fields = None.
Proof.
  intro H. rewrite decoder_singular_is_last_selected_write,
    cursor_first_is_first_selected_value,
    (excluded_fields_have_no_selected_values H). split; reflexivity.
Qed.

Theorem metadata_insertion_preserves_selected_values wanted prefix suffix value :
  wanted <> IgnoredMetadata ->
  collect wanted (prefix ++ Field IgnoredMetadata value :: suffix) =
    collect wanted (prefix ++ suffix).
Proof.
  intro H. repeat rewrite collect_append.
  destruct wanted; simpl in *; congruence.
Qed.

Theorem metadata_is_neutral_for_decoder_registers wanted rest value singular repeated :
  wanted <> IgnoredMetadata ->
  decoder_singular wanted (Field IgnoredMetadata value :: rest) singular =
    decoder_singular wanted rest singular /\
  decoder_repeated wanted (Field IgnoredMetadata value :: rest) repeated =
    decoder_repeated wanted rest repeated.
Proof. intro H. destruct wanted; simpl in *; intuition congruence. Qed.

Definition family_slot (wanted : slot) : bool :=
  match wanted with
  | ParSends | ParReceives | ParNews | ParExpressions | ParBundles => true
  | _ => false
  end.

Definition ordered_family_slots :=
  [ParSends; ParReceives; ParNews; ParExpressions; ParBundles].

Theorem selected_family_mapping_is_exact :
  NoDup ordered_family_slots /\
  (forall wanted, family_slot wanted = true <-> In wanted ordered_family_slots).
Proof.
  split.
  - unfold ordered_family_slots. repeat constructor; simpl; intuition discriminate.
  - intro wanted. destruct wanted; unfold ordered_family_slots; simpl; intuition discriminate.
Qed.

Definition payload_bytes (root : CanonicalKeyProtocol.region) (value : payload)
  : list CanonicalKeyProtocol.located_byte :=
  match value with
  | ScalarObservation _ => []
  | ChildRegion region => Windows.window_bytes root region
  end.

Theorem repeated_source_slice_order_is_preserved wanted fields root :
  canonical_run wanted fields ->
  flat_map (payload_bytes root) (cursor_seek wanted fields) =
    flat_map (payload_bytes root) (decoder_repeated wanted fields []).
Proof. intro H. now rewrite (ordered_run_cursor_equals_repeated_decoder H). Qed.

Theorem selected_child_window_composes_with_exact_source_slice
  root parent limit start stop child :
  Windows.window_valid root parent ->
  Windows.checked_subwindow limit parent start stop = Some child ->
  Windows.window_valid root child /\
  Windows.window_owner child = Windows.window_owner parent /\
  payload_bytes root (ChildRegion child) =
    firstn (stop - start) (skipn start (payload_bytes root (ChildRegion parent))).
Proof.
  intros Hvalid Hchild.
  pose proof (@Windows.nested_windows_preserve_owner_bounds_and_exact_slice
    limit root parent start stop child Hvalid Hchild)
    as (valid & owner & bound & bytes).
  split; [exact valid|]. split; [exact owner|exact bytes].
Qed.

Definition sample (value : string) := ScalarObservation [value].

Theorem field_view_nonvacuity_and_invalid_input_controls :
  cursor_first Scalar [Field Scalar (sample "first"); Field Scalar (sample "last")] =
    Some (sample "first") /\
  decoder_singular Scalar [Field Scalar (sample "first"); Field Scalar (sample "last")] None =
    Some (sample "last") /\
  cursor_seek RepeatedChildren
    [Field RepeatedChildren (sample "a"); Field IgnoredMetadata (sample "m");
     Field RepeatedChildren (sample "b")] = [sample "a"] /\
  decoder_repeated RepeatedChildren
    [Field RepeatedChildren (sample "a"); Field IgnoredMetadata (sample "m");
     Field RepeatedChildren (sample "b")] [] = [sample "a"; sample "b"] /\
  canonical_run RepeatedChildren
    [Field Scalar (sample "s"); Field RepeatedChildren (sample "a");
     Field RepeatedChildren (sample "b"); Field IgnoredMetadata (sample "m")].
Proof.
  repeat split; try reflexivity.
  exists [Field Scalar (sample "s")],
    [Field RepeatedChildren (sample "a"); Field RepeatedChildren (sample "b")],
    [Field IgnoredMetadata (sample "m")].
  repeat split; repeat constructor; discriminate.
Qed.

End FieldViews.

End CanonicalSexprProjection.

Print Assumptions CanonicalSexprProjection.recursive_reference_is_the_existing_fold.
Print Assumptions CanonicalSexprProjection.existing_fold_machine_has_exact_reference_tokens.
Print Assumptions CanonicalSexprProjection.continuation_machine_is_sound.
Print Assumptions CanonicalSexprProjection.continuation_machine_constructively_completes.
Print Assumptions CanonicalSexprProjection.streaming_and_recursive_reference_are_exact.
Print Assumptions CanonicalSexprProjection.streaming_execution_is_deterministic.
Print Assumptions CanonicalSexprProjection.reference_group.
Print Assumptions CanonicalSexprProjection.render_once_then_repeat_is_exact.
Print Assumptions CanonicalSexprProjection.repeated_output_preserves_every_separator.
Print Assumptions CanonicalSexprProjection.three_state_classification_matches_exact_occurrences.
Print Assumptions CanonicalSexprProjection.par_zero_one_many_are_distinct.
Print Assumptions CanonicalSexprProjection.ignored_families_change_neither_rendering_nor_arity.
Print Assumptions CanonicalSexprProjection.ignored_only_families_render_nil.
Print Assumptions CanonicalSexprProjection.family_occurrences_preserve_historical_order.
Print Assumptions CanonicalSexprProjection.constructor_catalog_is_exhaustive_and_unique.
Print Assumptions CanonicalSexprProjection.every_constructor_observation_has_exact_streaming_tokens.
Print Assumptions CanonicalSexprProjection.ignored_fallback_and_absence_observations_are_distinct.
Print Assumptions CanonicalSexprProjection.empty_shape_controls.
Print Assumptions CanonicalSexprProjection.FieldViews.unique_singular_cursor_equals_decoder.
Print Assumptions CanonicalSexprProjection.FieldViews.ordered_run_cursor_equals_repeated_decoder.
Print Assumptions CanonicalSexprProjection.FieldViews.absent_singular_preserves_the_decoder_default.
Print Assumptions CanonicalSexprProjection.FieldViews.metadata_insertion_preserves_selected_values.
Print Assumptions CanonicalSexprProjection.FieldViews.metadata_is_neutral_for_decoder_registers.
Print Assumptions CanonicalSexprProjection.FieldViews.selected_family_mapping_is_exact.
Print Assumptions CanonicalSexprProjection.FieldViews.repeated_source_slice_order_is_preserved.
Print Assumptions CanonicalSexprProjection.FieldViews.selected_child_window_composes_with_exact_source_slice.
Print Assumptions CanonicalSexprProjection.FieldViews.field_view_nonvacuity_and_invalid_input_controls.
