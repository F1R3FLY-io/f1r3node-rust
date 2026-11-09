From Stdlib Require Import List Bool Arith Lia.

Import ListNotations.

Record located_cell := Cell {
  cell_byte : nat;
  cell_page : nat;
  cell_source : nat;
  cell_offset : nat
}.

Inductive source_handle :=
| EmptyHandle
| BoundedHandle (root start width : nat).

Record descriptor := Descriptor {
  descriptor_previous : source_handle;
  descriptor_suffix : list located_cell;
  descriptor_contents : list located_cell
}.

Record retained_window := RetainedWindow {
  window_handle : source_handle;
  window_contents : list located_cell
}.

Record arena_state := ArenaState {
  arena_descriptors : list descriptor;
  weak_cache_keys : list nat;
  live_windows : list retained_window;
  registered_sources : list retained_window;
  active_page : nat;
  next_page : nat;
  active_used : nat;
  page_capacity : nat
}.

Definition empty_arena (capacity : nat) : arena_state :=
  ArenaState [] [] [] [] 0 1 0 capacity.

Definition with_descriptors (state : arena_state) (descriptors : list descriptor)
    : arena_state :=
  ArenaState descriptors (weak_cache_keys state) (live_windows state)
    (registered_sources state) (active_page state) (next_page state)
    (active_used state) (page_capacity state).

Definition with_weak_and_live (state : arena_state) (keys : list nat)
    (windows : list retained_window) : arena_state :=
  ArenaState (arena_descriptors state) keys windows
    (registered_sources state) (active_page state) (next_page state)
    (active_used state) (page_capacity state).

Definition with_live (state : arena_state) (windows : list retained_window)
    : arena_state :=
  with_weak_and_live state (weak_cache_keys state) windows.

Definition with_tail (state : arena_state) (page next used : nat)
    : arena_state :=
  ArenaState (arena_descriptors state) (weak_cache_keys state)
    (live_windows state) (registered_sources state) page next used
    (page_capacity state).

Definition selected (state : arena_state) (handle : source_handle)
    : option (list located_cell) :=
  match handle with
  | EmptyHandle => Some []
  | BoundedHandle root start width =>
      match nth_error (arena_descriptors state) root with
      | None => None
      | Some descriptor =>
          if start + width <=? length (descriptor_contents descriptor) then
            Some (firstn width (skipn start (descriptor_contents descriptor)))
          else None
      end
  end.

Definition existing_handle (state : arena_state) (handle : source_handle)
    : Prop :=
  match handle with
  | EmptyHandle => True
  | BoundedHandle root _ _ => root < length (arena_descriptors state)
  end.

Definition weak_cache_capacity : nat := 256.

Definition metadata_bounded (state : arena_state) : Prop :=
  length (weak_cache_keys state) <= weak_cache_capacity.

Definition window_pins (window : retained_window) (page : nat) : bool :=
  existsb (fun cell => cell_page cell =? page) (window_contents window).

Definition pin_count (windows : list retained_window) (page : nat) : nat :=
  length (filter (fun window => window_pins window page) windows).

Definition strong_page_owners (state : arena_state) (page : nat) : nat :=
  1 + pin_count (registered_sources state) page +
    pin_count (live_windows state) page.

Definition weak_cache_strong_page_owners (_state : arena_state) (_page : nat)
    : nat := 0.

Definition tail_is_unique (state : arena_state) : bool :=
  strong_page_owners state (active_page state) =? 1.

Definition tail_can_extend (state : arena_state) : bool :=
  (active_used state <? page_capacity state) && tail_is_unique state.

Definition append_fork (state : arena_state) (previous : source_handle)
    (suffix : list located_cell) : option (arena_state * source_handle) :=
  match selected state previous with
  | None => None
  | Some prefix =>
      let contents := prefix ++ suffix in
      let next_id := length (arena_descriptors state) in
      Some (with_descriptors state
        (arena_descriptors state ++
          [Descriptor previous suffix contents]),
        BoundedHandle next_id 0 (length contents))
  end.

Lemma selected_existing_handle_survives_descriptor_extension :
  forall state added handle,
    existing_handle state handle ->
    selected (with_descriptors state (arena_descriptors state ++ [added]))
      handle = selected state handle.
Proof.
  intros [descriptors keys live registered page next used capacity] added
    [|root start width] old; simpl in *; [reflexivity|].
  now rewrite nth_error_app1 by exact old.
Qed.

Lemma selected_success_implies_existing_handle :
  forall state handle contents,
    selected state handle = Some contents -> existing_handle state handle.
Proof.
  intros [descriptors keys live registered page next used capacity]
    [|root start width] contents succeeds; simpl in *; [exact I|].
  destruct (nth_error descriptors root) as [descriptor|] eqn:found;
    [|discriminate].
  apply nth_error_Some. rewrite found. discriminate.
Qed.

Theorem arbitrary_historical_fork_preserves_every_old_handle :
  forall state previous suffix next new_handle old,
    append_fork state previous suffix = Some (next, new_handle) ->
    existing_handle state old -> selected next old = selected state old.
Proof.
  intros state previous suffix next new_handle old appended existing.
  unfold append_fork in appended.
  destruct (selected state previous) as [prefix|]; [|discriminate].
  inversion appended; subst next new_handle.
  now apply selected_existing_handle_survives_descriptor_extension.
Qed.

Theorem arbitrary_historical_fork_preserves_successful_selections :
  forall state previous suffix next new_handle old contents,
    append_fork state previous suffix = Some (next, new_handle) ->
    selected state old = Some contents -> selected next old = Some contents.
Proof.
  intros state previous suffix next new_handle old contents appended old_contents.
  rewrite (arbitrary_historical_fork_preserves_every_old_handle
    _ _ _ _ _ _ appended (selected_success_implies_existing_handle
      _ _ _ old_contents)).
  exact old_contents.
Qed.

Theorem forked_handle_has_exact_selected_predecessor_and_suffix :
  forall state previous suffix next new_handle prefix,
    selected state previous = Some prefix ->
    append_fork state previous suffix = Some (next, new_handle) ->
    selected next new_handle = Some (prefix ++ suffix).
Proof.
  intros [descriptors keys live registered page next_page_id used capacity]
    previous suffix next new_handle prefix selected_prefix appended.
  unfold append_fork in appended. rewrite selected_prefix in appended.
  inversion appended; subst next new_handle; clear appended.
  unfold selected; simpl.
  rewrite nth_error_app2 by lia.
  replace (length descriptors - length descriptors) with 0 by lia.
  simpl. rewrite Nat.leb_refl, firstn_all. reflexivity.
Qed.

Theorem append_fork_preserves_metadata_bound :
  forall state previous suffix next new_handle,
    append_fork state previous suffix = Some (next, new_handle) ->
    metadata_bounded state -> metadata_bounded next.
Proof.
  intros state previous suffix next new_handle appended bounded.
  unfold append_fork in appended.
  destruct (selected state previous) as [prefix|]; [|discriminate].
  inversion appended; subst next new_handle.
  unfold metadata_bounded in *. simpl. exact bounded.
Qed.

Definition append_generated (state : arena_state) (previous : source_handle)
    (byte source offset : nat) : option (arena_state * source_handle) :=
  let can_extend := tail_can_extend state in
  let page := if can_extend then active_page state else next_page state in
  match append_fork state previous [Cell byte page source offset] with
  | None => None
  | Some (extended, handle) =>
      if can_extend then
        Some (with_tail extended page (next_page state) (S (active_used state)), handle)
      else
        Some (with_tail extended page (S (next_page state)) 1, handle)
  end.

Theorem append_generated_preserves_every_old_handle :
  forall state previous byte source offset next new_handle old,
    append_generated state previous byte source offset = Some (next, new_handle) ->
    existing_handle state old -> selected next old = selected state old.
Proof.
  intros state previous byte source offset next new_handle old appended existing.
  unfold append_generated in appended.
  destruct (tail_can_extend state) eqn:reuse; simpl in appended.
  - destruct (append_fork state previous
      [Cell byte (active_page state) source offset])
      as [[extended handle]|] eqn:forked; [|discriminate].
    inversion appended; subst next new_handle; simpl.
    now apply (arbitrary_historical_fork_preserves_every_old_handle
      _ _ _ _ _ _ forked).
  - destruct (append_fork state previous
      [Cell byte (next_page state) source offset])
      as [[extended handle]|] eqn:forked; [|discriminate].
    inversion appended; subst next new_handle; simpl.
    now apply (arbitrary_historical_fork_preserves_every_old_handle
      _ _ _ _ _ _ forked).
Qed.

Definition weak_key_after_export (state : arena_state)
    (handle : source_handle) : list nat :=
  match handle with
  | EmptyHandle => weak_cache_keys state
  | BoundedHandle root _ _ =>
      firstn weak_cache_capacity
        (root :: filter (fun id => negb (id =? root)) (weak_cache_keys state))
  end.

Definition publish (state : arena_state) (handle : source_handle)
    : option (arena_state * retained_window) :=
  match selected state handle with
  | None => None
  | Some contents =>
      let window := RetainedWindow handle contents in
      Some (with_weak_and_live state (weak_key_after_export state handle)
        (window :: live_windows state), window)
  end.

Definition drop_latest (state : arena_state) : arena_state :=
  match live_windows state with
  | [] => state
  | _ :: rest => with_live state rest
  end.

Definition register_source_window (state : arena_state)
    (source : retained_window) : arena_state :=
  ArenaState (arena_descriptors state) (weak_cache_keys state)
    (live_windows state) (source :: registered_sources state)
    (active_page state) (next_page state) (active_used state)
    (page_capacity state).

Theorem weak_keys_contribute_no_persistent_strong_page_owners :
  forall state page,
    weak_cache_strong_page_owners state page = 0 /\
    strong_page_owners state page =
      1 + pin_count (registered_sources state) page +
        pin_count (live_windows state) page.
Proof. intros; split; reflexivity. Qed.

Theorem weak_key_metadata_is_bounded_after_export :
  forall state handle next window,
    publish state handle = Some (next, window) ->
    metadata_bounded state -> metadata_bounded next.
Proof.
  intros state handle next window published bounded.
  unfold publish in published.
  destruct (selected state handle) as [contents|] eqn:selection;
    [|discriminate].
  inversion published; subst next window; clear published.
  unfold metadata_bounded in *.
  destruct handle as [|root start width].
  - cbn [with_weak_and_live weak_cache_keys weak_key_after_export].
    exact bounded.
  - cbn [with_weak_and_live weak_cache_keys weak_key_after_export].
    rewrite length_firstn. lia.
Qed.

Theorem publish_returns_exact_selected_window_and_preserves_history :
  forall state handle next window old,
    publish state handle = Some (next, window) ->
    selected state handle = Some (window_contents window) /\
    selected next old = selected state old.
Proof.
  intros state handle next window old published.
  unfold publish in published.
  destruct (selected state handle) as [contents|] eqn:selection;
    [|discriminate].
  inversion published; subst next window; simpl.
  split; reflexivity.
Qed.

Theorem refused_export_does_not_publish_or_mutate :
  forall state handle,
    selected state handle = None -> publish state handle = None.
Proof. intros state handle refusal; unfold publish; now rewrite refusal. Qed.

Theorem returned_window_is_the_only_new_persistent_page_owner :
  forall state handle next window page,
    publish state handle = Some (next, window) ->
    strong_page_owners next page = strong_page_owners state page +
      (if window_pins window page then 1 else 0).
Proof.
  intros state handle next window page published.
  unfold publish in published.
  destruct (selected state handle) as [contents|] eqn:selection;
    [|discriminate].
  inversion published; subst next window; clear published.
  unfold strong_page_owners, pin_count; simpl.
  destruct (window_pins (RetainedWindow handle contents) page);
    simpl; lia.
Qed.

Theorem dropping_returned_window_restores_pre_export_page_owners :
  forall state handle next window page,
    publish state handle = Some (next, window) ->
    strong_page_owners (drop_latest next) page =
      strong_page_owners state page.
Proof.
  intros state handle next window page published.
  unfold publish in published.
  destruct (selected state handle) as [contents|] eqn:selection;
    [|discriminate].
  inversion published; subst next window; clear published.
  unfold drop_latest, strong_page_owners; simpl. reflexivity.
Qed.

Theorem dropping_any_live_window_removes_exactly_its_page_owner :
  forall state before window after page,
    live_windows state = before ++ window :: after ->
    strong_page_owners state page =
      strong_page_owners (with_live state (before ++ after)) page +
        (if window_pins window page then 1 else 0).
Proof.
  intros state before window after page membership.
  unfold strong_page_owners, pin_count; simpl.
  rewrite membership.
  repeat rewrite filter_app.
  repeat rewrite length_app.
  simpl.
  destruct (window_pins window page); simpl; lia.
Qed.

Theorem dropping_returned_window_restores_unique_tail_eligibility :
  forall state handle next window,
    publish state handle = Some (next, window) ->
    tail_can_extend (drop_latest next) = tail_can_extend state.
Proof.
  intros state handle next window published.
  unfold publish in published.
  destruct (selected state handle) as [contents|] eqn:selection;
    [|discriminate].
  inversion published; subst next window; clear published.
  unfold drop_latest, tail_can_extend, tail_is_unique,
    strong_page_owners; simpl. reflexivity.
Qed.

Theorem registering_a_source_is_an_explicit_page_owner :
  forall state source page,
    strong_page_owners (register_source_window state source) page =
      strong_page_owners state page +
        (if window_pins source page then 1 else 0).
Proof.
  intros state source page.
  unfold strong_page_owners, pin_count, register_source_window;
    simpl.
  destruct (window_pins source page); simpl; lia.
Qed.

Definition workspace_page_owners (state : arena_state)
    (handle : source_handle) (page : nat) : nat :=
  match selected state handle with
  | None => 0
  | Some contents =>
      if existsb (fun cell => cell_page cell =? page) contents then 1 else 0
  end.

Definition old_strong_cache_page_owners (old_roots : list retained_window)
    (state : arena_state) (page : nat) : nat :=
  strong_page_owners state page + pin_count old_roots page.

Definition old_tail_is_unique (old_roots : list retained_window)
    (state : arena_state) : bool :=
  old_strong_cache_page_owners old_roots state (active_page state) =? 1.

Definition three_forks : option
    (arena_state * source_handle * source_handle * source_handle) :=
  match append_generated (empty_arena 4) EmptyHandle 7 41 100 with
  | None => None
  | Some (first, old) =>
      match append_generated first old 8 42 200 with
      | None => None
      | Some (second, linear) =>
          match append_generated second old 8 43 300 with
          | None => None
          | Some (third, forked) => Some (third, old, linear, forked)
          end
      end
  end.

Definition three_exports : option
    (arena_state * retained_window * retained_window * retained_window *
      source_handle * source_handle * source_handle) :=
  match three_forks with
  | None => None
  | Some (state, old, linear, forked) =>
      match publish state old with
      | None => None
      | Some (first, old_window) =>
          match publish first linear with
          | None => None
          | Some (second, linear_window) =>
              match publish second forked with
              | None => None
              | Some (third, fork_window) =>
                  Some (third, old_window, linear_window, fork_window,
                    old, linear, forked)
              end
          end
      end
  end.

Example historical_forks_are_exact_after_all_weak_exports :
  match three_exports with
  | None => False
  | Some (state, old_window, linear_window, fork_window,
      old, linear, forked) =>
      selected state old = Some [Cell 7 0 41 100] /\
      selected state linear = Some [Cell 7 0 41 100; Cell 8 0 42 200] /\
      selected state forked = Some [Cell 7 0 41 100; Cell 8 0 43 300] /\
      window_contents old_window = [Cell 7 0 41 100] /\
      window_contents linear_window = [Cell 7 0 41 100; Cell 8 0 42 200] /\
      window_contents fork_window = [Cell 7 0 41 100; Cell 8 0 43 300] /\
      length (weak_cache_keys state) <= length (arena_descriptors state) /\
      weak_cache_strong_page_owners state 0 = 0
  end.
Proof. vm_compute. repeat split; try reflexivity; lia. Qed.

Example old_every_root_strong_cache_seals_the_tail_after_drop :
  match three_exports with
  | None => False
  | Some (exported, old_window, linear_window, fork_window, _, _, _) =>
      let released := drop_latest (drop_latest (drop_latest exported)) in
      let old_roots := [old_window; linear_window; fork_window] in
      length old_roots = 3 /\
      strong_page_owners released 0 = 1 /\
      tail_can_extend released = true /\
      old_strong_cache_page_owners old_roots released 0 = 4 /\
      old_tail_is_unique old_roots released = false
  end.
Proof. vm_compute. repeat split; reflexivity. Qed.

Print Assumptions arbitrary_historical_fork_preserves_every_old_handle.
Print Assumptions forked_handle_has_exact_selected_predecessor_and_suffix.
Print Assumptions append_generated_preserves_every_old_handle.
Print Assumptions weak_keys_contribute_no_persistent_strong_page_owners.
Print Assumptions dropping_returned_window_restores_pre_export_page_owners.
Print Assumptions dropping_any_live_window_removes_exactly_its_page_owner.
Print Assumptions dropping_returned_window_restores_unique_tail_eligibility.
Print Assumptions historical_forks_are_exact_after_all_weak_exports.
Print Assumptions old_every_root_strong_cache_seals_the_tail_after_drop.
