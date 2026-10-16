From Stdlib Require Import Lists.List Arith.PeanoNat Lia.
Import ListNotations.

Module RecordPrefixComposition.

Inductive DecodeError := UnderlyingError (code : nat)
  | InvalidConsumed | CursorOverflow | UnsupportedEpoch.
Inductive Result (A : Type) := Ok (value : A) | Err (error : DecodeError).
Arguments Ok {A} _.
Arguments Err {A} _.

Definition checked_advance (bound size cursor consumed : nat) : Result nat :=
  if cursor <=? size then
    if consumed <=? size - cursor then
      if cursor + consumed <=? bound then Ok (cursor + consumed)
      else Err CursorOverflow
    else Err InvalidConsumed
  else Err InvalidConsumed.

Theorem checked_advance_success : forall bound size cursor consumed next,
  checked_advance bound size cursor consumed = Ok next <->
  cursor <= size /\ consumed <= size - cursor /\
  cursor + consumed <= bound /\ next = cursor + consumed.
Proof.
  intros. unfold checked_advance.
  destruct (cursor <=? size) eqn:Hcursor;
    [apply Nat.leb_le in Hcursor|apply Nat.leb_gt in Hcursor].
  - destruct (consumed <=? size - cursor) eqn:Hused;
      [apply Nat.leb_le in Hused|apply Nat.leb_gt in Hused].
    + destruct (cursor + consumed <=? bound) eqn:Hbound;
        [apply Nat.leb_le in Hbound|apply Nat.leb_gt in Hbound].
      * split; intro H; [inversion H; subst; auto|destruct H as [_ [_ [_ ->]]]; reflexivity].
      * split; [discriminate|intros [_ [_ [H _]]]; lia].
    + split; [discriminate|intros [_ [H _]]; lia].
  - split; [discriminate|intros [H _]; lia].
Qed.

Theorem cursor_outside_input_is_rejected : forall bound size cursor consumed,
  size < cursor -> checked_advance bound size cursor consumed = Err InvalidConsumed.
Proof.
  intros. unfold checked_advance.
  destruct (cursor <=? size) eqn:Hcursor; [apply Nat.leb_le in Hcursor; lia|reflexivity].
Qed.

Theorem consumed_length_outside_suffix_is_rejected : forall bound size cursor consumed,
  size - cursor < consumed ->
  checked_advance bound size cursor consumed = Err InvalidConsumed.
Proof.
  intros. unfold checked_advance. destruct (cursor <=? size); [|reflexivity].
  destruct (consumed <=? size - cursor) eqn:Hused;
    [apply Nat.leb_le in Hused; lia|reflexivity].
Qed.

Theorem cursor_integer_overflow_is_rejected : forall bound size cursor consumed,
  cursor <= size -> consumed <= size - cursor -> bound < cursor + consumed ->
  checked_advance bound size cursor consumed = Err CursorOverflow.
Proof.
  intros bound size cursor consumed Hcursor Hused Hoverflow.
  unfold checked_advance.
  apply Nat.leb_le in Hcursor. apply Nat.leb_le in Hused.
  rewrite Hcursor, Hused.
  destruct (cursor + consumed <=? bound) eqn:Hbound;
    [apply Nat.leb_le in Hbound; lia|reflexivity].
Qed.

Theorem checked_cursor_addition_composes : forall bound size cursor first middle second finish,
  checked_advance bound size cursor first = Ok middle ->
  checked_advance bound size middle second = Ok finish ->
  checked_advance bound size cursor (first + second) = Ok finish.
Proof.
  intros. apply checked_advance_success in H. apply checked_advance_success in H0.
  apply checked_advance_success. destruct H as [? [? [? ?]]].
  destruct H0 as [? [? [? ?]]]. repeat split; lia.
Qed.

Theorem checked_prefix_suffix_partition : forall bound (bytes : list nat) consumed next,
  checked_advance bound (length bytes) 0 consumed = Ok next ->
  next = consumed /\ length (firstn consumed bytes) = consumed /\
  firstn consumed bytes ++ skipn next bytes = bytes.
Proof.
  intros bound bytes consumed next H.
  apply checked_advance_success in H. simpl in H.
  destruct H as [_ [Hused [_ Hnext]]]. subst next.
  split; [reflexivity|]. split.
  - rewrite firstn_length. apply Nat.min_l. lia.
  - apply firstn_skipn.
Qed.

Definition prefix_at {Epoch Value : Type}
  (decode : Epoch -> list nat -> Result (Value * nat))
  (epoch : Epoch) (bytes : list nat) (bound cursor : nat) : Result (Value * nat) :=
  if cursor <=? length bytes then
    match decode epoch (skipn cursor bytes) with
    | Err error => Err error
    | Ok (value, consumed) =>
        match checked_advance bound (length bytes) cursor consumed with
        | Err error => Err error
        | Ok next => Ok (value, next)
        end
    end
  else Err InvalidConsumed.

Theorem prefix_at_success : forall Epoch Value decode (epoch : Epoch) bytes bound cursor
  (value : Value) next,
  prefix_at decode epoch bytes bound cursor = Ok (value, next) ->
  exists consumed,
    decode epoch (skipn cursor bytes) = Ok (value, consumed) /\
    checked_advance bound (length bytes) cursor consumed = Ok next.
Proof.
  intros Epoch Value decode epoch bytes bound cursor value next H.
  unfold prefix_at in H. destruct (cursor <=? length bytes); [|discriminate].
  destruct (decode epoch (skipn cursor bytes)) as [[v used]|error] eqn:Hd; [|discriminate].
  destruct (checked_advance bound (length bytes) cursor used) as [pos|error] eqn:Ha;
    [|discriminate]. inversion H; subst. exists used. auto.
Qed.

Fixpoint counted_prefix {Epoch Value : Type}
  (decode : Epoch -> list nat -> Result (Value * nat))
  (epoch : Epoch) (bytes : list nat) (bound count cursor : nat)
  : Result (list Value * nat) :=
  match count with
  | 0 =>
      match checked_advance bound (length bytes) cursor 0 with
      | Ok next => Ok ([], next)
      | Err error => Err error
      end
  | S rest =>
      match prefix_at decode epoch bytes bound cursor with
      | Err error => Err error
      | Ok (value, next) =>
          match counted_prefix decode epoch bytes bound rest next with
          | Err error => Err error
          | Ok (values, finish) => Ok (value :: values, finish)
          end
      end
  end.

Fixpoint ordered_prefixes {Epoch Value : Type}
  (decode : Epoch -> list nat -> Result (Value * nat))
  (epoch : Epoch) (bytes : list nat) (bound cursor : nat)
  (values : list Value) (finish : nat) : Prop :=
  match values with
  | [] => cursor <= length bytes /\ cursor <= bound /\ finish = cursor
  | value :: rest => exists consumed,
      decode epoch (skipn cursor bytes) = Ok (value, consumed) /\
      cursor <= length bytes /\ consumed <= length bytes - cursor /\
      cursor + consumed <= bound /\
      ordered_prefixes decode epoch bytes bound (cursor + consumed) rest finish
  end.

Theorem counted_prefix_keeps_count_order_and_epoch :
  forall Epoch Value decode (epoch : Epoch) bytes bound count cursor (values : list Value) finish,
  counted_prefix decode epoch bytes bound count cursor = Ok (values, finish) ->
  length values = count /\ ordered_prefixes decode epoch bytes bound cursor values finish.
Proof.
  intros Epoch Value decode epoch bytes bound count.
  induction count as [|rest IH]; intros cursor values finish H; simpl in H.
  - destruct (checked_advance bound (length bytes) cursor 0) as [next|error] eqn:Ha;
      [|discriminate]. inversion H; subst. simpl.
    apply checked_advance_success in Ha. rewrite Nat.add_0_r in Ha.
    destruct Ha as [Hcursor [_ [Hbound Hfinish]]]. auto.
  - destruct (prefix_at decode epoch bytes bound cursor) as [[value next]|error] eqn:Hp;
      [|discriminate].
    destruct (counted_prefix decode epoch bytes bound rest next) as [[tail final]|error] eqn:Hr;
      [|discriminate]. inversion H; subst. specialize (IH _ _ _ Hr).
    destruct IH as [Hlength Horder]. simpl. split; [now rewrite Hlength|].
    apply prefix_at_success in Hp. destruct Hp as [used [Hd Ha]].
    apply checked_advance_success in Ha. destruct Ha as [Hcursor [Hused [Hbound Hnext]]].
    exists used. subst next. auto.
Qed.

Fixpoint decode_rows {Epoch Row Value : Type} (supported : Epoch -> bool)
  (decode : Epoch -> Row -> Result Value) (epoch : Epoch) (rows : list Row)
  : Result (list Value) :=
  match rows with
  | [] => Ok []
  | row :: rest =>
      if supported epoch then
        match decode epoch row with
        | Err error => Err error
        | Ok value =>
            match decode_rows supported decode epoch rest with
            | Err error => Err error
            | Ok values => Ok (value :: values)
            end
        end
      else Err UnsupportedEpoch
  end.

Definition decode_history_rows {Epoch Row Value : Type} supported decode (epoch : Epoch)
  (history : Result (list Row)) : Result (list Value) :=
  match history with
  | Err error => Err error
  | Ok rows => decode_rows supported decode epoch rows
  end.

Theorem empty_history_never_invokes_typed_decode : forall Epoch Row Value supported decode (epoch : Epoch),
  @decode_history_rows Epoch Row Value supported decode epoch (Ok []) = Ok [].
Proof. reflexivity. Qed.

Theorem history_lookup_error_is_not_empty : forall Epoch Row Value supported decode (epoch : Epoch) error,
  @decode_history_rows Epoch Row Value supported decode epoch (Err error) = Err error.
Proof. reflexivity. Qed.

Theorem present_unsupported_row_refuses_before_typed_decode :
  forall Epoch Row Value supported decode (epoch : Epoch) (row : Row) rest,
  supported epoch = false ->
  @decode_history_rows Epoch Row Value supported decode epoch (Ok (row :: rest)) = Err UnsupportedEpoch.
Proof. intros. unfold decode_history_rows. simpl. now rewrite H. Qed.

Theorem supported_epoch_matches_ungated_traversal :
  forall Epoch Row Value supported decode (epoch : Epoch) rows,
  supported epoch = true ->
  @decode_rows Epoch Row Value supported decode epoch rows =
  @decode_rows Epoch Row Value (fun _ => true) decode epoch rows.
Proof.
  intros Epoch Row Value supported decode epoch rows Hsupported.
  induction rows as [|row rest IH]; [reflexivity|].
  simpl. rewrite Hsupported.
  destruct (decode epoch row) as [value|error].
  - now rewrite IH.
  - reflexivity.
Qed.

Print Assumptions checked_advance_success.
Print Assumptions cursor_outside_input_is_rejected.
Print Assumptions consumed_length_outside_suffix_is_rejected.
Print Assumptions cursor_integer_overflow_is_rejected.
Print Assumptions checked_cursor_addition_composes.
Print Assumptions checked_prefix_suffix_partition.
Print Assumptions prefix_at_success.
Print Assumptions counted_prefix_keeps_count_order_and_epoch.
Print Assumptions empty_history_never_invokes_typed_decode.
Print Assumptions history_lookup_error_is_not_empty.
Print Assumptions present_unsupported_row_refuses_before_typed_decode.
Print Assumptions supported_epoch_matches_ungated_traversal.

End RecordPrefixComposition.

Module EpmDriverReuse.

Inductive Result (A : Type) := Ok (value : A) | Err (error : nat).
Arguments Ok {A} _.
Arguments Err {A} _.

Section BodyDriver.
Context {Value : Type}.
Variable read_body : nat -> Result (Value * nat).

Fixpoint reference (remaining cursor : nat) : Result (list Value * nat) :=
  match remaining with
  | 0 => Ok ([], cursor)
  | S rest =>
      match read_body cursor with
      | Err error => Err error
      | Ok (value, next) =>
          match reference rest next with
          | Err error => Err error
          | Ok (values, final) => Ok (value :: values, final)
          end
      end
  end.

Fixpoint drive (remaining cursor : nat) (reversed : list Value)
  : Result (list Value * nat) :=
  match remaining with
  | 0 => Ok (rev reversed, cursor)
  | S rest =>
      match read_body cursor with
      | Err error => Err error
      | Ok (value, next) => drive rest next (value :: reversed)
      end
  end.

Theorem driver_preserves_order_cursor_and_error : forall remaining cursor reversed,
  drive remaining cursor reversed =
  match reference remaining cursor with
  | Ok (values, final) => Ok (rev reversed ++ values, final)
  | Err error => Err error
  end.
Proof.
  induction remaining as [|rest IH]; intros cursor reversed; simpl.
  - now rewrite app_nil_r.
  - destruct (read_body cursor) as [[value next]|error]; [|reflexivity].
    rewrite IH. destruct (reference rest next) as [[values final]|error];
      [|reflexivity].
    simpl. now rewrite <- app_assoc.
Qed.

Corollary empty_driver_matches_reference : forall remaining cursor,
  drive remaining cursor [] = reference remaining cursor.
Proof.
  intros. rewrite driver_preserves_order_cursor_and_error.
  destruct (reference remaining cursor) as [[values final]|error]; reflexivity.
Qed.

Theorem successful_body_count : forall remaining cursor values final,
  reference remaining cursor = Ok (values, final) -> length values = remaining.
Proof.
  induction remaining as [|rest IH]; intros cursor values final H; simpl in H.
  - inversion H. reflexivity.
  - destruct (read_body cursor) as [[value next]|error] eqn:Hread; [|discriminate].
    destruct (reference rest next) as [[tail last]|error] eqn:Htail; [|discriminate].
    inversion H; subst. simpl. f_equal. eapply IH. exact Htail.
Qed.

Theorem driver_preserves_expected_ordinal_count : forall remaining cursor reversed values final,
  drive remaining cursor reversed = Ok (values, final) ->
  length values = length reversed + remaining.
Proof.
  intros remaining cursor reversed values final H.
  rewrite driver_preserves_order_cursor_and_error in H.
  destruct (reference remaining cursor) as [[tail last]|error] eqn:Href;
    [|discriminate].
  inversion H; subst. rewrite app_length, rev_length.
  now rewrite (successful_body_count _ _ _ _ Href).
Qed.

Definition finish_result {Output : Type}
  (finish : list Value -> nat -> Result Output) (decoded : Result (list Value * nat)) :=
  match decoded with
  | Err error => Err error
  | Ok (values, final) => finish values final
  end.

Theorem shared_finish_preserves_decoding : forall Output
  (finish : list Value -> nat -> Result Output) remaining cursor,
  finish_result finish (drive remaining cursor []) =
  finish_result finish (reference remaining cursor).
Proof. intros. now rewrite empty_driver_matches_reference. Qed.

Theorem failed_body_never_runs_finish : forall Output
  (finish : list Value -> nat -> Result Output) remaining cursor error,
  reference remaining cursor = Err error ->
  finish_result finish (drive remaining cursor []) = Err error.
Proof. intros. rewrite empty_driver_matches_reference, H. reflexivity. Qed.

End BodyDriver.

Example eager_framing_changes_error_precedence :
  @finish_result nat nat (fun _ _ => Err 2)
    (drive (fun _ => @Err (nat * nat) 1) 1 0 []) = Err 1 /\
  @Err nat 2 <> Err 1.
Proof. split; [reflexivity|discriminate]. Qed.

Print Assumptions driver_preserves_order_cursor_and_error.
Print Assumptions empty_driver_matches_reference.
Print Assumptions successful_body_count.
Print Assumptions driver_preserves_expected_ordinal_count.
Print Assumptions shared_finish_preserves_decoding.
Print Assumptions failed_body_never_runs_finish.
Print Assumptions eager_framing_changes_error_precedence.

End EpmDriverReuse.

Module EpmEndpointDispatch.

Definition path := list nat.

Definition prefix (ancestor descendant : path) : Prop :=
  exists suffix, descendant = ancestor ++ suffix.

Definition strict_prefix (ancestor descendant : path) : Prop :=
  exists byte suffix, descendant = ancestor ++ byte :: suffix.

Lemma prefix_reflexive : forall p, prefix p p.
Proof. intros p. exists []. now rewrite app_nil_r. Qed.

Lemma prefix_empty_only : forall p, prefix p [] -> p = [].
Proof.
  intros p [suffix Hequal]. destruct p; [reflexivity|discriminate Hequal].
Qed.

Lemma strict_prefix_has_first_child : forall ancestor descendant,
  strict_prefix ancestor descendant -> exists byte, prefix (ancestor ++ [byte]) descendant.
Proof.
  intros ancestor descendant [byte [suffix Hequal]]. exists byte, suffix.
  rewrite Hequal, <- app_assoc. reflexivity.
Qed.

Section Values.
Context {Value : Type}.

Definition endpoint := (path * option Value)%type.

Record semantic_state := SemanticState {
  represented_topology : path -> Prop;
  represented_value : path -> option Value
}.

Definition state_equivalent (left right : semantic_state) : Prop :=
  forall query,
    (represented_topology left query <-> represented_topology right query) /\
    represented_value left query = represented_value right query.

Definition semantic_create (state : semantic_state) (target : path) : semantic_state :=
  SemanticState
    (fun query => represented_topology state query \/ (target <> [] /\ prefix query target))
    (represented_value state).

Definition semantic_insert (state : semantic_state) (target : path) (value : Value) : semantic_state :=
  SemanticState
    (fun query => represented_topology state query \/ prefix query target)
    (fun query => if list_eq_dec Nat.eq_dec query target then Some value else represented_value state query).

Definition old_dispatch (state : semantic_state) (entry : endpoint) : semantic_state :=
  let created := semantic_create state (fst entry) in
  match snd entry with
  | Some value => semantic_insert created (fst entry) value
  | None => created
  end.

Definition new_dispatch (state : semantic_state) (entry : endpoint) : semantic_state :=
  match snd entry with
  | Some value => semantic_insert state (fst entry) value
  | None => semantic_create state (fst entry)
  end.

Lemma state_equivalent_reflexive : forall state, state_equivalent state state.
Proof. intros state query. split; [tauto|reflexivity]. Qed.

Lemma state_equivalent_transitive : forall first middle last,
  state_equivalent first middle -> state_equivalent middle last -> state_equivalent first last.
Proof.
  intros first middle last Hfirst Hlast query.
  destruct (Hfirst query) as [Htopology Hvalue].
  destruct (Hlast query) as [Htopology' Hvalue']. split; [tauto|congruence].
Qed.

Theorem create_preserves_equivalence : forall left right target,
  state_equivalent left right ->
  state_equivalent (semantic_create left target) (semantic_create right target).
Proof.
  intros left right target Hequivalent query.
  destruct (Hequivalent query) as [Htopology Hvalue].
  cbv beta iota zeta delta [semantic_create represented_topology represented_value].
  split; [tauto|exact Hvalue].
Qed.

Theorem insert_preserves_equivalence : forall left right target value,
  state_equivalent left right ->
  state_equivalent (semantic_insert left target value) (semantic_insert right target value).
Proof.
  intros left right target value Hequivalent query.
  destruct (Hequivalent query) as [Htopology Hvalue].
  cbv beta iota zeta delta [semantic_insert represented_topology represented_value].
  split; [tauto|]. destruct (list_eq_dec Nat.eq_dec query target); [reflexivity|exact Hvalue].
Qed.

Theorem direct_insert_equals_create_then_insert : forall state target value,
  state_equivalent (semantic_insert state target value)
    (semantic_insert (semantic_create state target) target value).
Proof.
  intros state target value query.
  cbv beta iota zeta delta [semantic_create semantic_insert represented_topology represented_value].
  split; [tauto|reflexivity].
Qed.

Theorem root_create_is_identity : forall state,
  state_equivalent (semantic_create state []) state.
Proof.
  intros state query.
  cbv beta iota zeta delta [semantic_create represented_topology represented_value].
  split; [|reflexivity]. split.
  - intros [Hpresent|[Hnonempty Hprefix]]; [exact Hpresent|].
    exfalso. apply Hnonempty. reflexivity.
  - intro Hpresent. now left.
Qed.

Theorem root_insert_sets_root_value : forall state value,
  represented_value (semantic_insert state [] value) [] = Some value.
Proof.
  intros state value.
  cbv beta iota zeta delta [semantic_insert represented_value].
  destruct (list_eq_dec Nat.eq_dec [] []) as [Hequal|Hdifferent]; [reflexivity|].
  exfalso. apply Hdifferent. reflexivity.
Qed.

Theorem root_insert_preserves_nonempty_topology : forall state value query,
  query <> [] ->
  (represented_topology (semantic_insert state [] value) query <-> represented_topology state query).
Proof.
  intros state value query Hnonempty.
  cbv beta iota zeta delta [semantic_insert represented_topology]. split.
  - intros [Hpresent|Hprefix]; [exact Hpresent|].
    apply prefix_empty_only in Hprefix. contradiction.
  - intro Hpresent. now left.
Qed.

Theorem endpoint_dispatch_equivalence : forall state entry,
  state_equivalent (new_dispatch state entry) (old_dispatch state entry).
Proof.
  intros state [target [value|]];
    cbv beta iota zeta delta [new_dispatch old_dispatch fst snd].
  - apply direct_insert_equals_create_then_insert.
  - apply state_equivalent_reflexive.
Qed.

Theorem new_dispatch_preserves_equivalence : forall left right entry,
  state_equivalent left right -> state_equivalent (new_dispatch left entry) (new_dispatch right entry).
Proof.
  intros left right [target [value|]] Hequivalent;
    cbv beta iota zeta delta [new_dispatch fst snd].
  - now apply insert_preserves_equivalence.
  - now apply create_preserves_equivalence.
Qed.

Fixpoint run_endpoints (dispatch : semantic_state -> endpoint -> semantic_state)
    (state : semantic_state) (entries : list endpoint) : semantic_state :=
  match entries with
  | [] => state
  | entry :: rest => run_endpoints dispatch (dispatch state entry) rest
  end.

Theorem endpoint_batches_preserve_equivalence : forall entries left right,
  state_equivalent left right ->
  state_equivalent (run_endpoints new_dispatch left entries) (run_endpoints old_dispatch right entries).
Proof.
  induction entries as [|entry rest IH]; intros left right Hequivalent; cbn [run_endpoints].
  - exact Hequivalent.
  - apply IH. eapply state_equivalent_transitive.
    + apply new_dispatch_preserves_equivalence. exact Hequivalent.
    + apply endpoint_dispatch_equivalence.
Qed.

Corollary endpoint_batch_equivalence : forall entries state,
  state_equivalent (run_endpoints new_dispatch state entries) (run_endpoints old_dispatch state entries).
Proof. intros. apply endpoint_batches_preserve_equivalence, state_equivalent_reflexive. Qed.

Theorem run_endpoints_append : forall dispatch before after state,
  run_endpoints dispatch state (before ++ after) =
    run_endpoints dispatch (run_endpoints dispatch state before) after.
Proof.
  intros dispatch before. induction before as [|entry rest IH]; intros after state;
    cbn [run_endpoints app]; [reflexivity|apply IH].
Qed.

Theorem new_dispatch_topology_cases : forall state entry query,
  represented_topology (new_dispatch state entry) query ->
  represented_topology state query \/ prefix query (fst entry).
Proof.
  intros state [target [value|]] query;
    cbv beta iota zeta delta [new_dispatch semantic_insert semantic_create represented_topology fst snd]; tauto.
Qed.

Theorem new_dispatch_preserves_topology : forall state entry query,
  represented_topology state query -> represented_topology (new_dispatch state entry) query.
Proof.
  intros state [target [value|]] query Hpresent;
    cbv beta iota zeta delta [new_dispatch semantic_insert semantic_create represented_topology fst snd];
    now left.
Qed.

Theorem new_batch_preserves_topology : forall entries state query,
  represented_topology state query -> represented_topology (run_endpoints new_dispatch state entries) query.
Proof.
  induction entries as [|entry rest IH]; intros state query Hpresent; cbn [run_endpoints].
  - exact Hpresent.
  - apply IH. now apply new_dispatch_preserves_topology.
Qed.

Theorem new_dispatch_preserves_other_values : forall state entry query,
  query <> fst entry -> represented_value (new_dispatch state entry) query = represented_value state query.
Proof.
  intros state [target [value|]] query Hother;
    cbv beta iota zeta delta [new_dispatch semantic_insert semantic_create represented_value fst snd] in *.
  - destruct (list_eq_dec Nat.eq_dec query target); [contradiction|reflexivity].
  - reflexivity.
Qed.

Theorem new_batch_preserves_other_values : forall entries state query,
  Forall (fun entry => query <> fst entry) entries ->
  represented_value (run_endpoints new_dispatch state entries) query = represented_value state query.
Proof.
  induction entries as [|entry rest IH]; intros state query Hother; cbn [run_endpoints].
  - reflexivity.
  - inversion Hother as [|head tail Hhead Htail]; subst.
    rewrite (IH (new_dispatch state entry) query Htail).
    now apply new_dispatch_preserves_other_values.
Qed.

Definition visit_act_prefix_coverage (tree : path -> Prop) (entries : list endpoint) : Prop :=
  forall entry query, In entry entries -> prefix query (fst entry) -> tree query.

Definition visit_act_leaf_premise (tree : path -> Prop) (entries : list endpoint) : Prop :=
  forall target, In (target, None) entries -> forall byte, ~ tree (target ++ [byte]).

Theorem valueless_endpoint_has_no_strict_extension : forall tree entries,
  visit_act_prefix_coverage tree entries -> visit_act_leaf_premise tree entries ->
  forall target entry, In (target, None) entries -> In entry entries -> ~ strict_prefix target (fst entry).
Proof.
  intros tree entries Hcoverage Hleaf target entry Htarget Hentry Hextension.
  destruct (strict_prefix_has_first_child _ _ Hextension) as [byte Hprefix].
  apply (Hleaf target Htarget byte). eapply Hcoverage; eassumption.
Qed.

Theorem no_future_strict_extension_of_created_leaf : forall tree before target after,
  visit_act_prefix_coverage tree (before ++ (target, None) :: after) ->
  visit_act_leaf_premise tree (before ++ (target, None) :: after) ->
  forall entry, In entry after -> ~ strict_prefix target (fst entry).
Proof.
  intros tree before target after Hcoverage Hleaf entry Hentry.
  eapply valueless_endpoint_has_no_strict_extension; [exact Hcoverage|exact Hleaf| |].
  - apply in_or_app. right. now left.
  - apply in_or_app. right. now right.
Qed.

Theorem new_batch_stays_within_visited_tree : forall entries tree state,
  (forall query, represented_topology state query -> tree query) ->
  visit_act_prefix_coverage tree entries ->
  forall query, represented_topology (run_endpoints new_dispatch state entries) query -> tree query.
Proof.
  induction entries as [|entry rest IH]; intros tree state Hstate Hcoverage query Hpresent;
    cbn [run_endpoints] in Hpresent.
  - now apply Hstate.
  - eapply IH; [| |exact Hpresent].
    + intros next Hnext. apply new_dispatch_topology_cases in Hnext.
      destruct Hnext as [Hold|Hprefix]; [now apply Hstate|].
      apply (Hcoverage entry next); [now left|exact Hprefix].
    + intros next candidate Hin Hprefix.
      apply (Hcoverage next candidate); [now right|exact Hprefix].
Qed.

Theorem leaf_absence_preserved_by_batch : forall tree entries state target,
  (forall query, represented_topology state query -> tree query) ->
  visit_act_prefix_coverage tree entries ->
  (forall byte, ~ tree (target ++ [byte])) ->
  forall byte, ~ represented_topology (run_endpoints new_dispatch state entries) (target ++ [byte]).
Proof.
  intros tree entries state target Hstate Hcoverage Hleaf byte Hpresent.
  apply (Hleaf byte). eapply new_batch_stays_within_visited_tree; eassumption.
Qed.

Lemma distinct_future_endpoint_paths : forall (before : list endpoint) (entry : endpoint) (after : list endpoint),
  NoDup (map fst (before ++ entry :: after)) ->
  Forall (fun next => fst entry <> fst next) after.
Proof.
  induction before as [|head rest IH]; intros entry after Hdistinct; cbn [app map] in Hdistinct.
  - inversion Hdistinct as [|head tail Habsent Htail]; subst.
    apply Forall_forall. intros next Hin Hequal. apply Habsent.
    apply in_map_iff. exists next. split; [symmetry; exact Hequal|exact Hin].
  - inversion Hdistinct as [|first tail Habsent Htail]; subst.
    now apply IH.
Qed.

Theorem dangling_endpoint_preserved_by_suffix : forall before target after state,
  NoDup (map fst (before ++ (target, None) :: after)) ->
  target <> [] -> represented_value state target = None ->
  represented_topology (run_endpoints new_dispatch (semantic_create state target) after) target /\
  represented_value (run_endpoints new_dispatch (semantic_create state target) after) target = None.
Proof.
  intros before target after state Hdistinct Hnonempty Hvalue. split.
  - apply new_batch_preserves_topology.
    cbv beta iota zeta delta [semantic_create represented_topology].
    right. split; [exact Hnonempty|apply prefix_reflexive].
  - rewrite new_batch_preserves_other_values.
    + exact Hvalue.
    + exact (distinct_future_endpoint_paths before (target, None) after Hdistinct).
Qed.

Section SuccessfulAPIRefinement.
Context {Representation : Type}.
Variable observe : Representation -> semantic_state.
Variable create_api : Representation -> path -> option Representation.
Variable insert_api : Representation -> path -> Value -> option Representation.

Hypothesis create_api_topology : forall before after target,
  create_api before target = Some after -> forall query,
  represented_topology (observe after) query <->
    represented_topology (observe before) query \/ (target <> [] /\ prefix query target).
Hypothesis create_api_values : forall before after target,
  create_api before target = Some after -> forall query,
  represented_value (observe after) query = represented_value (observe before) query.
Hypothesis insert_api_topology : forall before after target value,
  insert_api before target value = Some after -> forall query,
  represented_topology (observe after) query <->
    represented_topology (observe before) query \/ prefix query target.
Hypothesis insert_api_values : forall before after target value,
  insert_api before target value = Some after -> forall query,
  represented_value (observe after) query =
    if list_eq_dec Nat.eq_dec query target then Some value else represented_value (observe before) query.

Theorem successful_api_direct_insert_equivalence : forall before created old_result new_result target value,
  create_api before target = Some created ->
  insert_api created target value = Some old_result ->
  insert_api before target value = Some new_result ->
  state_equivalent (observe new_result) (observe old_result).
Proof.
  intros before created old_result new_result target value Hcreate Hold Hnew query. split.
  - rewrite (insert_api_topology _ _ _ _ Hnew), (insert_api_topology _ _ _ _ Hold),
      (create_api_topology _ _ _ Hcreate). tauto.
  - rewrite (insert_api_values _ _ _ _ Hnew), (insert_api_values _ _ _ _ Hold).
    destruct (list_eq_dec Nat.eq_dec query target); [reflexivity|].
    now rewrite (create_api_values _ _ _ Hcreate).
Qed.

End SuccessfulAPIRefinement.

Lemma one_byte_edge_remaining_suffix_length : forall (parent : path) byte,
  length (parent ++ [byte]) - (length parent + 1) = 0.
Proof. intros parent byte. rewrite app_length. cbn [length]. lia. Qed.

Lemma one_byte_edge_is_strict_child : forall (parent : path) byte,
  strict_prefix parent (parent ++ [byte]).
Proof. intros parent byte. exists byte, []. reflexivity. Qed.

Theorem one_byte_dangling_endpoint_is_created : forall state parent byte,
  represented_topology (new_dispatch state (parent ++ [byte], None)) (parent ++ [byte]).
Proof.
  intros state parent byte.
  change (represented_topology state (parent ++ [byte]) \/
    ((parent ++ [byte]) <> [] /\ prefix (parent ++ [byte]) (parent ++ [byte]))).
  right. split; [|apply prefix_reflexive].
  intro Hempty. apply app_eq_nil in Hempty. destruct Hempty as [_ Hlast]. discriminate.
Qed.

Theorem one_byte_dangling_endpoint_cannot_be_skipped : forall state parent byte,
  ~ represented_topology state (parent ++ [byte]) ->
  ~ state_equivalent (new_dispatch state (parent ++ [byte], None)) state.
Proof.
  intros state parent byte Habsent Hequivalent.
  destruct (Hequivalent (parent ++ [byte])) as [Htopology Hvalue].
  apply Habsent, (proj1 Htopology). apply one_byte_dangling_endpoint_is_created.
Qed.

End Values.

Print Assumptions direct_insert_equals_create_then_insert.
Print Assumptions root_create_is_identity.
Print Assumptions root_insert_sets_root_value.
Print Assumptions root_insert_preserves_nonempty_topology.
Print Assumptions endpoint_dispatch_equivalence.
Print Assumptions endpoint_batches_preserve_equivalence.
Print Assumptions endpoint_batch_equivalence.
Print Assumptions run_endpoints_append.
Print Assumptions new_batch_preserves_topology.
Print Assumptions new_batch_preserves_other_values.
Print Assumptions valueless_endpoint_has_no_strict_extension.
Print Assumptions no_future_strict_extension_of_created_leaf.
Print Assumptions new_batch_stays_within_visited_tree.
Print Assumptions leaf_absence_preserved_by_batch.
Print Assumptions distinct_future_endpoint_paths.
Print Assumptions dangling_endpoint_preserved_by_suffix.
Print Assumptions successful_api_direct_insert_equivalence.
Print Assumptions one_byte_edge_remaining_suffix_length.
Print Assumptions one_byte_edge_is_strict_child.
Print Assumptions one_byte_dangling_endpoint_is_created.
Print Assumptions one_byte_dangling_endpoint_cannot_be_skipped.

End EpmEndpointDispatch.
