From Stdlib Require Import List Bool Arith Lia NArith.
From StackSafePDA Require Import EPM1 EPathMap PathMapCanonicalStreamPublicationBase.

Import ListNotations.
Module P := PathMapCanonicalStreamPublicationBase.

Definition act_magic : list nat := [65; 67; 84; 114; 101; 101; 48; 51].
Definition act_header_size : nat := 16.
Definition act_trailer_size : nat := 8.
Definition act_varint_bias : nat := 247.

Definition act_take (arena : list nat) (position count : nat)
  : option (list nat) :=
  if position + count <=? length arena then
    Some (firstn count (skipn position arena))
  else None.

Definition act_little_endian (digits : list nat) : nat :=
  fold_right (fun digit value => digit + 256 * value) 0 digits.

Definition act_bytes_eqb (left right : list nat) : bool :=
  if list_eq_dec Nat.eq_dec left right then true else false.

Definition act_read_varint (arena : list nat) (position : nat)
  : option (nat * nat) :=
  match nth_error arena position with
  | None => None
  | Some first =>
      if first <=? act_varint_bias then Some (first, S position)
      else if first <=? 255 then
        let width := first - act_varint_bias in
        if (0 <? width) && (width <=? 8) then
          match act_take arena (S position) width with
          | Some digits =>
              let value := act_little_endian digits in
              if (act_varint_bias <? value) &&
                 negb (Nat.eqb (last digits 0) 0) then
                Some (value, S position + width)
              else None
          | None => None
          end
        else None
      else None
  end.

Definition act_read_target (arena : list nat) (parent position : nat)
  : option (nat * nat) :=
  match act_read_varint arena position with
  | Some (offset, next) =>
      if (0 <? offset) && (offset <=? parent - act_header_size) then
        Some (parent - offset, next)
      else None
  | None => None
  end.

Definition act_read_value (arena : list nat) (present : bool) (position : nat)
  : option (option nat * nat) :=
  if present then
    match act_read_varint arena position with
    | Some (value, next) => Some (Some value, next)
    | None => None
    end
  else Some (None, position).

Fixpoint act_strictly_ascending (previous : option nat)
  (labels : list nat) : bool :=
  match labels with
  | [] => true
  | label :: rest =>
      (match previous with
       | None => true
       | Some earlier => earlier <? label
       end) && act_strictly_ascending (Some label) rest
  end.

Definition act_mask_bit (mask : list nat) (bit : nat) : bool :=
  match nth_error mask (bit / 8) with
  | Some word => Nat.testbit word (bit mod 8)
  | None => false
  end.

Definition act_dense_labels (mask : list nat) : list nat :=
  filter (act_mask_bit mask) (seq 0 256).

Definition act_read_labels (arena : list nat) (marker position : nat)
  : option (list nat * nat) :=
  if marker <? 32 then
    match act_take arena position marker with
    | Some labels =>
        if act_strictly_ascending None labels then
          Some (labels, position + marker)
        else None
    | None => None
    end
  else if marker =? 32 then
    match act_take arena position 32 with
    | Some mask =>
        let labels := act_dense_labels mask in
        if 32 <=? length labels then Some (labels, position + 32)
        else None
    | None => None
    end
  else None.

Inductive act_edge : Type :=
| ActBranch (labels : list nat) (first_child : option nat)
| ActLine (suffix : list nat) (line_start line_end : nat)
    (wrapper : option nat).

Record act_raw_node : Type := ActRawNode {
  act_node_id : nat;
  act_node_length : nat;
  act_node_value : option nat;
  act_node_edge : act_edge
}.

Definition act_read_raw_node (arena : list nat) (node_id : nat)
  : option act_raw_node :=
  match nth_error arena node_id with
  | None => None
  | Some header =>
      match act_read_value arena (64 <=? header mod 128) (S node_id) with
      | None => None
      | Some (value, after_value) =>
          if header <? 128 then
            let marker := header mod 64 in
            let first :=
              if marker =? 0 then Some (None, after_value)
              else match act_read_target arena node_id after_value with
                   | Some (id, next) => Some (Some id, next)
                   | None => None
                   end in
            match first with
            | None => None
            | Some (first_child, after_first) =>
                match act_read_labels arena marker after_first with
                | Some (labels, next) =>
                    if Bool.eqb (Nat.eqb (length labels) 0)
                                (match first_child with None => true
                                 | Some _ => false end) then
                      Some (ActRawNode node_id (next - node_id) value
                            (ActBranch labels first_child))
                    else None
                | None => None
                end
            end
          else if header mod 64 <=? 1 then
            let child :=
              if header mod 2 =? 0 then Some (None, after_value)
              else match act_read_target arena node_id after_value with
                   | Some (id, next) => Some (Some id, next)
                   | None => None
                   end in
            match child with
            | None => None
            | Some (wrapper, after_child) =>
                match act_read_target arena node_id after_child with
                | None => None
                | Some (line_start, next) =>
                    match act_read_varint arena line_start with
                    | None => None
                    | Some (line_length, line_body) =>
                        match act_take arena line_body line_length with
                        | None => None
                        | Some suffix =>
                            let line_end := line_body + line_length in
                            if (0 <? line_length) && (line_end <=? node_id) then
                              Some (ActRawNode node_id (next - node_id) value
                                    (ActLine suffix line_start line_end wrapper))
                            else None
                        end
                    end
                end
            end
          else None
      end
  end.

Fixpoint act_sibling_ids (arena : list nat) (remaining : list nat)
  (next_id parent : nat) : option (list nat) :=
  match remaining with
  | [] => Some []
  | _ :: tail =>
      if next_id <? parent then
        match act_read_raw_node arena next_id with
        | None => None
        | Some node =>
            let after_node := next_id + act_node_length node in
            if after_node <=? parent then
              match act_sibling_ids arena tail after_node parent with
              | Some rest => Some (next_id :: rest)
              | None => None
              end
            else None
        end
      else None
  end.

Definition act_children (arena : list nat) (node : act_raw_node)
  : option (list nat) :=
  match act_node_edge node with
  | ActBranch labels first_child =>
      match labels, first_child with
      | [], None => Some []
      | _ :: _, Some first =>
          act_sibling_ids arena labels first (act_node_id node)
      | _, _ => None
      end
  | ActLine _ _ _ wrapper =>
      match wrapper with None => Some [] | Some id => Some [id] end
  end.

Definition act_parsed_node : Type := act_raw_node * list nat.

Fixpoint act_parse_work (arena : list nat) (fuel : nat)
  (pending seen : list nat) (parsed : list act_parsed_node)
  : option (list act_parsed_node) :=
  match pending with
  | [] => Some parsed
  | node_id :: remaining =>
      match fuel with
      | 0 => None
      | S smaller =>
          if existsb (Nat.eqb node_id) seen then None
          else match act_read_raw_node arena node_id with
               | None => None
               | Some node =>
                   match act_children arena node with
                   | None => None
                   | Some children =>
                       if forallb (fun child => child <? node_id) children then
                         act_parse_work arena smaller
                           (children ++ remaining) (node_id :: seen)
                           ((node, children) :: parsed)
                       else None
                   end
               end
      end
  end.

Fixpoint act_find_node (node_id : nat) (parsed : list act_parsed_node)
  : option act_parsed_node :=
  match parsed with
  | [] => None
  | (node, children) :: rest =>
      if node_id =? act_node_id node then Some (node, children)
      else act_find_node node_id rest
  end.

Fixpoint act_expand_line (suffix : list nat) (value : option nat)
  (children : P.ByteForest) : P.ByteTrie :=
  match suffix with
  | [] => P.TrieNode (option_map N.of_nat value) children
  | byte :: rest =>
      P.TrieNode None
        (P.TrieForestCons byte (act_expand_line rest value children)
          P.TrieForestNil)
  end.

Definition act_line_cache : Type := list (nat * (nat * list nat)).

Fixpoint act_cached_line (hash : nat) (cache : act_line_cache)
  : option (nat * list nat) :=
  match cache with
  | [] => None
  | (key, line) :: rest =>
      if key =? hash then Some line else act_cached_line hash rest
  end.

Definition act_cache_line (hash line_id : nat) (suffix : list nat)
  (cache : act_line_cache) : act_line_cache :=
  (hash, (line_id, suffix)) ::
    filter (fun entry => negb (fst entry =? hash)) cache.

Definition act_replay_line (hash_fn : list nat -> nat)
  (cache : act_line_cache) (position line_start line_end : nat)
  (suffix : list nat) : option (nat * act_line_cache) :=
  let hash := hash_fn suffix in
  match act_cached_line hash cache with
  | Some (previous_id, previous_suffix) =>
      if act_bytes_eqb previous_suffix suffix then
        if line_start =? previous_id then Some (position, cache) else None
      else if line_start =? position then
        Some (line_end, act_cache_line hash line_start suffix cache)
      else None
  | None =>
      if line_start =? position then
        Some (line_end, act_cache_line hash line_start suffix cache)
      else None
  end.

Lemma act_bytes_eqb_true_iff :
  forall left right, act_bytes_eqb left right = true <-> left = right.
Proof.
  intros left right. unfold act_bytes_eqb.
  destruct (list_eq_dec Nat.eq_dec left right) as [equal|different];
    split; intro observed; try discriminate; congruence.
Qed.

Lemma act_bytes_eqb_false_iff :
  forall left right, act_bytes_eqb left right = false <-> left <> right.
Proof.
  intros left right. unfold act_bytes_eqb.
  destruct (list_eq_dec Nat.eq_dec left right) as [equal|different];
    split; intro observed; try discriminate; congruence.
Qed.

Inductive act_canonical_line_step (hash_fn : list nat -> nat)
  : act_line_cache -> nat -> nat -> nat -> list nat ->
    nat -> act_line_cache -> Prop :=
| ActFreshFirst : forall cache position line_start line_end suffix,
    act_cached_line (hash_fn suffix) cache = None ->
    line_start = position ->
    act_canonical_line_step hash_fn cache position line_start line_end suffix
      line_end (act_cache_line (hash_fn suffix) line_start suffix cache)
| ActFreshCollision : forall cache position line_start line_end suffix
    previous_id previous_suffix,
    act_cached_line (hash_fn suffix) cache =
      Some (previous_id, previous_suffix) ->
    previous_suffix <> suffix ->
    line_start = position ->
    act_canonical_line_step hash_fn cache position line_start line_end suffix
      line_end (act_cache_line (hash_fn suffix) line_start suffix cache)
| ActReuse : forall cache position line_start line_end suffix previous_id,
    act_cached_line (hash_fn suffix) cache = Some (previous_id, suffix) ->
    line_start = previous_id ->
    act_canonical_line_step hash_fn cache position line_start line_end suffix
      position cache.

Theorem act_replay_line_iff_canonical_line_step :
  forall hash_fn cache position line_start line_end suffix next next_cache,
    act_replay_line hash_fn cache position line_start line_end suffix =
      Some (next, next_cache) <->
    act_canonical_line_step hash_fn cache position line_start line_end
      suffix next next_cache.
Proof.
  intros hash_fn cache position line_start line_end suffix next next_cache.
  split.
  - unfold act_replay_line.
    destruct (act_cached_line (hash_fn suffix) cache)
      as [[previous_id previous_suffix]|] eqn:found.
    + destruct (act_bytes_eqb previous_suffix suffix) eqn:same.
      * apply act_bytes_eqb_true_iff in same. subst previous_suffix.
        destruct (line_start =? previous_id) eqn:at_previous;
          [|discriminate].
        apply Nat.eqb_eq in at_previous. intro accepted.
        inversion accepted; subst.
        eapply ActReuse; eauto.
      * apply act_bytes_eqb_false_iff in same.
        destruct (line_start =? position) eqn:at_position;
          [|discriminate].
        apply Nat.eqb_eq in at_position. intro accepted.
        inversion accepted; subst.
        eapply ActFreshCollision; eauto.
    + destruct (line_start =? position) eqn:at_position;
        [|discriminate].
      apply Nat.eqb_eq in at_position. intro accepted.
      inversion accepted; subst.
      eapply ActFreshFirst; eauto.
  - intro step. inversion step; subst; unfold act_replay_line;
      rewrite H; try rewrite H0; try rewrite H1.
    + now rewrite Nat.eqb_refl.
    + apply act_bytes_eqb_false_iff in H0. rewrite H0.
      now rewrite Nat.eqb_refl.
    + assert (same : act_bytes_eqb suffix suffix = true).
      { apply act_bytes_eqb_true_iff. reflexivity. }
      now rewrite same, Nat.eqb_refl.
Qed.

Fixpoint act_reconstruct (fuel : nat) (hash_fn : list nat -> nat)
  (parsed : list act_parsed_node) (node_id position : nat)
  (cache : act_line_cache)
  : option (nat * P.ByteTrie * act_line_cache) :=
  match fuel with
  | 0 => None
  | S smaller =>
      match act_find_node node_id parsed with
      | None => None
      | Some (node, children) =>
          match act_node_edge node with
          | ActBranch labels _ =>
              match act_reconstruct_forest smaller hash_fn parsed labels children
                      position cache with
              | None => None
              | Some (next, forest, next_cache) =>
                  if (next =? node_id) &&
                     negb ((Nat.eqb (length labels) 1) &&
                           (match act_node_value node with
                            | None => true | Some _ => false end)) then
                    Some (node_id + act_node_length node,
                          P.TrieNode (option_map N.of_nat (act_node_value node)) forest,
                          next_cache)
                  else None
              end
          | ActLine suffix line_start line_end wrapper =>
              match wrapper with
              | None =>
                  match act_replay_line hash_fn cache position line_start
                          line_end suffix with
                  | Some (next, next_cache) =>
                      if next =? node_id then
                        Some (node_id + act_node_length node,
                              act_expand_line suffix (act_node_value node)
                                P.TrieForestNil, next_cache)
                      else None
                  | None => None
                  end
              | Some wrapper_id =>
                  match act_find_node wrapper_id parsed with
                  | Some (wrapper_node, wrapper_children) =>
                      match act_node_edge wrapper_node with
                      | ActBranch wrapper_labels _ =>
                          match act_reconstruct_forest smaller hash_fn parsed
                                  wrapper_labels wrapper_children position cache with
                          | Some (next, forest, after_children) =>
                              if (match act_node_value node with
                                  | None => true | Some _ => false end) &&
                                 (0 <? length wrapper_labels) &&
                                 negb ((Nat.eqb (length wrapper_labels) 1) &&
                                   (match act_node_value wrapper_node with
                                    | None => true | Some _ => false end)) &&
                                 (wrapper_id + act_node_length wrapper_node =? node_id) then
                                match act_replay_line hash_fn after_children next
                                        line_start line_end suffix with
                                | Some (after_line, next_cache) =>
                                    if after_line =? wrapper_id then
                                      Some (node_id + act_node_length node,
                                            act_expand_line suffix
                                              (act_node_value wrapper_node) forest,
                                            next_cache)
                                    else None
                                | None => None
                                end
                              else None
                          | None => None
                          end
                      | ActLine _ _ _ _ => None
                      end
                  | None => None
                  end
              end
          end
      end
  end
with act_reconstruct_forest (fuel : nat) (hash_fn : list nat -> nat)
  (parsed : list act_parsed_node) (labels ids : list nat)
  (position : nat) (cache : act_line_cache)
  : option (nat * P.ByteForest * act_line_cache) :=
  match fuel with
  | 0 => None
  | S smaller =>
      match labels, ids with
      | [], [] => Some (position, P.TrieForestNil, cache)
      | byte :: label_tail, node_id :: id_tail =>
          match act_reconstruct smaller hash_fn parsed node_id position cache with
          | None => None
          | Some (after_child, child, child_cache) =>
              match act_reconstruct_forest smaller hash_fn parsed
                      label_tail id_tail after_child child_cache with
              | Some (after_forest, tail, next_cache) =>
                  Some (after_forest, P.TrieForestCons byte child tail,
                        next_cache)
              | None => None
              end
          end
      | _, _ => None
      end
  end.

Definition act_decode_arena (hash_fn : list nat -> nat)
  (arena : list nat) : option P.ByteTrie :=
  if forallb (fun byte => byte <=? 255) arena then
    match act_take arena 0 8, act_take arena 8 8 with
    | Some magic, Some root_slot =>
        let root_id := act_little_endian root_slot in
        let trailer := length arena - act_trailer_size in
        if act_bytes_eqb act_magic magic &&
           (act_header_size <=? root_id) &&
           (root_id <? trailer) &&
           (25 <=? length arena) &&
           act_bytes_eqb (skipn trailer arena) (repeat 0 act_trailer_size) then
          match act_parse_work arena (length arena) [root_id] [] [] with
          | None => None
          | Some parsed =>
              match act_reconstruct (length arena) hash_fn parsed root_id
                      act_header_size [] with
              | Some (end_position, trie, _) =>
                  if end_position =? trailer then Some trie else None
              | None => None
              end
          end
        else None
    | _, _ => None
    end
  else None.

Definition act_value_markers (trie : P.ByteTrie) : list N :=
  map snd (P.trie_entries trie).

Definition decode_epm1_actree03 (hash_fn : list nat -> nat)
  (encoded : list nat)
  : option (EPM1.epm1_snapshot * option P.ByteTrie) :=
  match EPM1.decode_epm1 encoded with
  | None => None
  | Some snapshot =>
      match EPM1.snapshot_mode snapshot with
      | Neutral => Some (snapshot, None)
      | _ =>
          match act_decode_arena hash_fn (EPM1.topology_arena snapshot) with
          | None => None
          | Some trie =>
              let markers := act_value_markers trie in
              if forallb (N.eqb 0%N) markers &&
                 (match EPM1.snapshot_mode snapshot with
                  | SetMode => true
                  | MapMode =>
                      length markers =? length (EPM1.value_bodies snapshot)
                  | Neutral => false
                  end) then Some (snapshot, Some trie)
              else None
          end
      end
  end.

Theorem decode_epm1_actree03_refines_epm1 :
  forall hash_fn encoded snapshot trie,
    decode_epm1_actree03 hash_fn encoded = Some (snapshot, trie) ->
    EPM1.decode_epm1 encoded = Some snapshot.
Proof.
  intros hash_fn encoded snapshot trie accepted.
  unfold decode_epm1_actree03 in accepted.
  destruct (EPM1.decode_epm1 encoded) as [read|] eqn:decoded;
    [|discriminate].
  destruct (EPM1.snapshot_mode read); simpl in accepted.
  - inversion accepted; subst; reflexivity.
  - destruct (act_decode_arena hash_fn (EPM1.topology_arena read));
      [|discriminate].
    destruct (forallb (N.eqb 0%N) (act_value_markers b) && true);
      inversion accepted; subst; reflexivity.
  - destruct (act_decode_arena hash_fn (EPM1.topology_arena read));
      [|discriminate].
    destruct (forallb (N.eqb 0%N) (act_value_markers b) &&
      (length (act_value_markers b) =? length (EPM1.value_bodies read)));
      inversion accepted; subst; reflexivity.
Qed.

Theorem decoded_map_has_exact_zero_markers :
  forall hash_fn encoded snapshot trie,
    decode_epm1_actree03 hash_fn encoded = Some (snapshot, Some trie) ->
    EPM1.snapshot_mode snapshot = MapMode ->
    Forall (fun marker => marker = 0%N) (act_value_markers trie) /\
    length (act_value_markers trie) = length (EPM1.value_bodies snapshot).
Proof.
  intros hash_fn encoded snapshot trie accepted mode.
  unfold decode_epm1_actree03 in accepted.
  destruct (EPM1.decode_epm1 encoded) as [read|]; [|discriminate].
  destruct (EPM1.snapshot_mode read) eqn:read_mode;
    try discriminate.
  - destruct (act_decode_arena hash_fn (EPM1.topology_arena read)) as [decoded|];
      [|discriminate].
    destruct (forallb (N.eqb 0%N) (act_value_markers decoded) && true)
      eqn:checked; [|discriminate].
    inversion accepted; subst.
    rewrite read_mode in mode. discriminate.
  - destruct (act_decode_arena hash_fn (EPM1.topology_arena read)) as [decoded|];
      [|discriminate].
    destruct (forallb (N.eqb 0%N) (act_value_markers decoded) &&
      (length (act_value_markers decoded) =? length (EPM1.value_bodies read)))
      eqn:checked; [|discriminate].
    inversion accepted; subst.
    apply andb_true_iff in checked as [zeros count].
    apply Nat.eqb_eq in count.
    split; [|exact count].
    apply Forall_forall.
    intros marker member.
    apply forallb_forall with (x := marker) in zeros; [|exact member].
    now apply N.eqb_eq in zeros.
Qed.

Print Assumptions act_replay_line_iff_canonical_line_step.
Print Assumptions decode_epm1_actree03_refines_epm1.
Print Assumptions decoded_map_has_exact_zero_markers.
