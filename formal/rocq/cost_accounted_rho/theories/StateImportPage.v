From Stdlib Require Import List Arith Lia.
Import ListNotations.

Inductive ImportExportEntry : Type :=
| ImportExportHistory : nat -> list nat -> ImportExportEntry
| ImportExportLeaf : nat -> ImportExportEntry.

Fixpoint import_attach_last_cursor (keys cursor : list nat) : list ImportExportEntry :=
  match keys with
  | [] => []
  | [key] => [ImportExportHistory key cursor]
  | key :: rest => ImportExportHistory key [] :: import_attach_last_cursor rest cursor
  end.

Definition import_export_page_entries (leaves history cursor : list nat)
    : list ImportExportEntry :=
  map ImportExportLeaf leaves ++ import_attach_last_cursor history cursor.

Fixpoint import_export_leaf_keys (entries : list ImportExportEntry) : list nat :=
  match entries with
  | [] => []
  | ImportExportLeaf key :: rest => key :: import_export_leaf_keys rest
  | ImportExportHistory _ _ :: rest => import_export_leaf_keys rest
  end.

Fixpoint import_export_history_keys (entries : list ImportExportEntry) : list nat :=
  match entries with
  | [] => []
  | ImportExportLeaf _ :: rest => import_export_history_keys rest
  | ImportExportHistory key _ :: rest => key :: import_export_history_keys rest
  end.

Lemma import_export_leaf_keys_app : forall first second,
  import_export_leaf_keys (first ++ second) =
  import_export_leaf_keys first ++ import_export_leaf_keys second.
Proof.
  induction first as [|entry rest IH]; intros second; simpl; auto.
  destruct entry; simpl; rewrite IH; reflexivity.
Qed.

Lemma import_export_history_keys_app : forall first second,
  import_export_history_keys (first ++ second) =
  import_export_history_keys first ++ import_export_history_keys second.
Proof.
  induction first as [|entry rest IH]; intros second; simpl; auto.
  destruct entry; simpl; rewrite IH; reflexivity.
Qed.

Lemma import_export_leaf_map_preserves_keys : forall leaves,
  import_export_leaf_keys (map ImportExportLeaf leaves) = leaves.
Proof. induction leaves; simpl; congruence. Qed.

Lemma import_export_leaf_map_has_no_history : forall leaves,
  import_export_history_keys (map ImportExportLeaf leaves) = [].
Proof. induction leaves; simpl; auto. Qed.

Lemma import_cursor_attachment_has_no_leaves : forall history cursor,
  import_export_leaf_keys (import_attach_last_cursor history cursor) = [].
Proof.
  induction history as [|key rest IH]; intros cursor; simpl; auto.
  destruct rest as [|next tail]; simpl; auto.
  exact (IH cursor).
Qed.

Lemma import_cursor_attachment_preserves_history : forall history cursor,
  import_export_history_keys (import_attach_last_cursor history cursor) = history.
Proof.
  induction history as [|key rest IH]; intros cursor; simpl; auto.
  destruct rest as [|next tail]; simpl; auto.
  specialize (IH cursor). simpl in IH. rewrite IH. reflexivity.
Qed.

Theorem import_page_preserves_all_leaf_occurrences : forall leaves history cursor,
  import_export_leaf_keys (import_export_page_entries leaves history cursor) = leaves.
Proof.
  intros. unfold import_export_page_entries. rewrite import_export_leaf_keys_app.
  rewrite import_export_leaf_map_preserves_keys, import_cursor_attachment_has_no_leaves.
  apply app_nil_r.
Qed.

Theorem import_page_preserves_all_history_occurrences : forall leaves history cursor,
  import_export_history_keys (import_export_page_entries leaves history cursor) = history.
Proof.
  intros. unfold import_export_page_entries. rewrite import_export_history_keys_app.
  rewrite import_export_leaf_map_has_no_history, import_cursor_attachment_preserves_history.
  reflexivity.
Qed.

Theorem import_leaf_only_page_remains_nonempty : forall key leaves cursor,
  import_export_page_entries (key :: leaves) [] cursor <> [].
Proof. intros. discriminate. Qed.

Theorem import_page_preserves_history_budget : forall leaves history cursor budget,
  length history <= budget ->
  length (import_export_history_keys
    (import_export_page_entries leaves history cursor)) <= budget.
Proof. intros. rewrite import_page_preserves_all_history_occurrences. assumption. Qed.

Theorem import_page_composition_preserves_leaves : forall l1 h1 c1 l2 h2 c2,
  import_export_leaf_keys
    (import_export_page_entries l1 h1 c1 ++ import_export_page_entries l2 h2 c2) =
  l1 ++ l2.
Proof.
  intros. rewrite import_export_leaf_keys_app.
  rewrite !import_page_preserves_all_leaf_occurrences. reflexivity.
Qed.

Theorem import_page_composition_preserves_history : forall l1 h1 c1 l2 h2 c2,
  import_export_history_keys
    (import_export_page_entries l1 h1 c1 ++ import_export_page_entries l2 h2 c2) =
  h1 ++ h2.
Proof.
  intros. rewrite import_export_history_keys_app.
  rewrite !import_page_preserves_all_history_occurrences. reflexivity.
Qed.

Record ImportPageParts : Type := {
  import_page_leaves : list nat;
  import_page_history : list nat;
  import_page_cursor : list nat
}.

Definition import_page_parts_entries (parts : ImportPageParts) :=
  import_export_page_entries (import_page_leaves parts)
    (import_page_history parts) (import_page_cursor parts).

Theorem import_arbitrary_page_composition_preserves_leaves : forall pages,
  import_export_leaf_keys (concat (map import_page_parts_entries pages)) =
  concat (map import_page_leaves pages).
Proof.
  induction pages as [|parts rest IH]; simpl; auto.
  rewrite import_export_leaf_keys_app, IH.
  unfold import_page_parts_entries.
  rewrite import_page_preserves_all_leaf_occurrences. reflexivity.
Qed.

Theorem import_arbitrary_page_composition_preserves_history : forall pages,
  import_export_history_keys (concat (map import_page_parts_entries pages)) =
  concat (map import_page_history pages).
Proof.
  induction pages as [|parts rest IH]; simpl; auto.
  rewrite import_export_history_keys_app, IH.
  unfold import_page_parts_entries.
  rewrite import_page_preserves_all_history_occurrences. reflexivity.
Qed.

Theorem import_arbitrary_pages_preserve_each_history_budget : forall pages budget,
  Forall (fun parts => length (import_page_history parts) <= budget) pages ->
  Forall (fun parts => length (import_export_history_keys
    (import_page_parts_entries parts)) <= budget) pages.
Proof.
  intros pages budget H. induction H; constructor; auto.
  unfold import_page_parts_entries.
  rewrite import_page_preserves_all_history_occurrences. assumption.
Qed.

Definition import_old_leaf_only_adapter (leaves : list nat) : list ImportExportEntry :=
  removelast (map ImportExportLeaf leaves).

Example import_old_adapter_loses_singleton_leaf :
  import_export_leaf_keys (import_old_leaf_only_adapter [7]) = [].
Proof. reflexivity. Qed.

Example import_old_adapter_loses_final_leaf :
  import_export_leaf_keys (import_old_leaf_only_adapter [7; 9]) = [7].
Proof. reflexivity. Qed.

Example import_page_retains_duplicate_leaf_occurrences :
  import_export_leaf_keys (import_export_page_entries [7; 7] [] []) = [7; 7].
Proof. reflexivity. Qed.

Example import_page_retains_leaf_only_tail_after_exact_budget : forall budget cursor,
  length (import_export_history_keys
    (import_export_page_entries [] (seq 0 budget) cursor)) = budget /\
  import_export_leaf_keys (import_export_page_entries [7] [] []) = [7].
Proof.
  intros. rewrite import_page_preserves_all_history_occurrences, length_seq.
  split; reflexivity.
Qed.

Fixpoint import_take_history_occurrences (budget : nat) (trace : list ImportExportEntry)
    : list ImportExportEntry * list ImportExportEntry :=
  match budget, trace with
  | 0, _ => ([], trace)
  | _, [] => ([], [])
  | S remaining, entry :: rest =>
      let next_budget := match entry with
        | ImportExportHistory _ _ => remaining
        | ImportExportLeaf _ => S remaining
        end in
      let '(selected, suffix) := import_take_history_occurrences next_budget rest in
      (entry :: selected, suffix)
  end.

Theorem import_history_page_split_preserves_every_occurrence : forall trace budget,
  let '(selected, suffix) := import_take_history_occurrences budget trace in
  selected ++ suffix = trace.
Proof.
  induction trace as [|entry rest IH]; intros [|budget]; simpl; auto.
  destruct entry; simpl.
  - specialize (IH budget).
    destruct (import_take_history_occurrences budget rest) as [selected suffix] eqn:E.
    simpl. f_equal. exact IH.
  - specialize (IH (S budget)).
    destruct (import_take_history_occurrences (S budget) rest) as [selected suffix] eqn:E.
    simpl. f_equal. exact IH.
Qed.

Theorem import_history_page_split_respects_budget : forall trace budget,
  length (import_export_history_keys
    (fst (import_take_history_occurrences budget trace))) <= budget.
Proof.
  induction trace as [|entry rest IH]; intros [|budget]; simpl; try lia.
  destruct entry; simpl.
  - specialize (IH budget).
    destruct (import_take_history_occurrences budget rest) as [selected suffix] eqn:E.
    simpl in *. lia.
  - specialize (IH (S budget)).
    destruct (import_take_history_occurrences (S budget) rest) as [selected suffix] eqn:E.
    simpl in *. exact IH.
Qed.

Theorem import_positive_history_budget_makes_progress : forall entry rest budget,
  0 < budget ->
  fst (import_take_history_occurrences budget (entry :: rest)) <> [].
Proof.
  intros entry rest [|budget] H; try lia.
  destruct entry; simpl;
    destruct (import_take_history_occurrences _ rest); discriminate.
Qed.

Theorem import_history_page_remainder_requires_full_occurrence_budget :
  forall trace budget selected suffix,
  import_take_history_occurrences budget trace = (selected, suffix) ->
  suffix <> [] -> length (import_export_history_keys selected) = budget.
Proof.
  induction trace as [|entry rest IH]; intros [|budget] selected suffix E H;
    simpl in E; try (inversion E; subst; simpl; congruence).
  destruct entry; simpl in E.
  - destruct (import_take_history_occurrences budget rest) as [chosen tail] eqn:Step.
    inversion E; subst. simpl. f_equal. eapply IH; eauto.
  - destruct (import_take_history_occurrences (S budget) rest) as [chosen tail] eqn:Step.
    inversion E; subst. simpl. eapply IH; eauto.
Qed.

Theorem import_history_split_preserves_selected_and_remaining_leaves :
  forall trace budget selected suffix,
  import_take_history_occurrences budget trace = (selected, suffix) ->
  import_export_leaf_keys selected ++ import_export_leaf_keys suffix =
    import_export_leaf_keys trace.
Proof.
  intros trace budget selected suffix E.
  pose proof (import_history_page_split_preserves_every_occurrence trace budget) as H.
  rewrite E in H. rewrite <- import_export_leaf_keys_app. now rewrite H.
Qed.

Theorem import_history_split_preserves_selected_and_remaining_history :
  forall trace budget selected suffix,
  import_take_history_occurrences budget trace = (selected, suffix) ->
  import_export_history_keys selected ++ import_export_history_keys suffix =
    import_export_history_keys trace.
Proof.
  intros trace budget selected suffix E.
  pose proof (import_history_page_split_preserves_every_occurrence trace budget) as H.
  rewrite E in H. rewrite <- import_export_history_keys_app. now rewrite H.
Qed.

Example import_exact_history_budget_leaves_a_leaf_only_suffix :
  import_take_history_occurrences 1 [ImportExportHistory 4 []; ImportExportLeaf 7] =
    ([ImportExportHistory 4 []], [ImportExportLeaf 7]).
Proof. reflexivity. Qed.

Example import_resumed_leaf_only_suffix_preserves_all_leaves :
  import_take_history_occurrences 1 [ImportExportLeaf 7; ImportExportLeaf 9] =
    ([ImportExportLeaf 7; ImportExportLeaf 9], []).
Proof. reflexivity. Qed.

Example import_shared_history_occurrences_each_consume_budget :
  import_take_history_occurrences 2
    [ImportExportHistory 4 [1]; ImportExportHistory 4 [2]; ImportExportLeaf 7] =
    ([ImportExportHistory 4 [1]; ImportExportHistory 4 [2]], [ImportExportLeaf 7]).
Proof. reflexivity. Qed.
