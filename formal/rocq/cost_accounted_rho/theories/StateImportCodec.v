From Stdlib Require Import List Arith Bool Lia.
From CostAccountedRho Require Import StateImportClosure.
Import ListNotations.

Definition import_wire_byte (value : nat) : Prop := value < 256.

Record ImportWireEdge : Type := {
  import_wire_slot : nat;
  import_wire_header : nat;
  import_wire_prefix : list nat;
  import_wire_hash : list nat
}.

Definition import_wire_edge_bytes (edge : ImportWireEdge) : list nat :=
  import_wire_slot edge :: import_wire_header edge ::
    (import_wire_prefix edge ++ import_wire_hash edge).

Definition import_wire_edge_bounds (edge : ImportWireEdge) : Prop :=
  import_wire_slot edge < 256 /\ import_wire_header edge < 256 /\
  length (import_wire_prefix edge) = import_wire_header edge mod 128 /\
  length (import_wire_hash edge) = 32.

Definition import_wire_edge_valid (edge : ImportWireEdge) : Prop :=
  import_wire_edge_bounds edge /\
  Forall import_wire_byte (import_wire_edge_bytes edge).

Definition import_take_exact_bytes (count : nat) (input : list nat)
    : option (list nat * list nat) :=
  if count <=? length input
  then Some (firstn count input, skipn count input)
  else None.

Lemma import_exact_bytes_partition : forall count input selected suffix,
  import_take_exact_bytes count input = Some (selected, suffix) ->
  length selected = count /\ input = selected ++ suffix.
Proof.
  intros count input selected suffix H. unfold import_take_exact_bytes in H.
  destruct (count <=? length input) eqn:E; try discriminate.
  apply Nat.leb_le in E. inversion H; subst. split.
  - rewrite length_firstn. lia.
  - symmetry. apply firstn_skipn.
Qed.

Definition import_parse_wire_edge (input : list nat)
    : option (ImportWireEdge * list nat) :=
  match input with
  | slot :: header :: rest =>
      if (slot <? 256) && (header <? 256) then
        match import_take_exact_bytes (header mod 128) rest with
        | None => None
        | Some (prefix, tail) =>
            match import_take_exact_bytes 32 tail with
            | None => None
            | Some (hash, suffix) =>
                Some ({| import_wire_slot := slot; import_wire_header := header;
                         import_wire_prefix := prefix; import_wire_hash := hash |}, suffix)
            end
        end
      else None
  | _ => None
  end.

Theorem import_parsed_edge_preserves_exact_bytes : forall input edge suffix,
  import_parse_wire_edge input = Some (edge, suffix) ->
  import_wire_edge_bounds edge /\ input = import_wire_edge_bytes edge ++ suffix.
Proof.
  intros [|slot [|header rest]] edge suffix H; try discriminate.
  unfold import_parse_wire_edge in H.
  destruct ((slot <? 256) && (header <? 256)) eqn:Bounds; try discriminate.
  apply andb_true_iff in Bounds. destruct Bounds as [Slot Header].
  apply Nat.ltb_lt in Slot. apply Nat.ltb_lt in Header.
  destruct (import_take_exact_bytes (header mod 128) rest)
    as [[prefix tail]|] eqn:Prefix; try discriminate.
  destruct (import_take_exact_bytes 32 tail)
    as [[hash remainder]|] eqn:Hash; try discriminate.
  apply import_exact_bytes_partition in Prefix.
  apply import_exact_bytes_partition in Hash.
  destruct Prefix as [PrefixLength Rest]. destruct Hash as [HashLength Tail].
  inversion H; subst. split.
  - unfold import_wire_edge_bounds. simpl. repeat split; assumption.
  - unfold import_wire_edge_bytes. simpl. now rewrite app_assoc.
Qed.

Theorem import_parsed_edge_consumes_a_complete_record : forall input edge suffix,
  import_parse_wire_edge input = Some (edge, suffix) ->
  length input = 34 + length (import_wire_prefix edge) + length suffix.
Proof.
  intros input edge suffix H.
  apply import_parsed_edge_preserves_exact_bytes in H.
  destruct H as [[Slot [Header [Prefix Hash]]] Bytes].
  rewrite Bytes, length_app. unfold import_wire_edge_bytes.
  simpl. rewrite length_app, Hash. lia.
Qed.

Fixpoint import_parse_wire_records (fuel : nat) (input used : list nat)
    : option (list ImportWireEdge) :=
  match input with
  | [] => Some []
  | _ => match fuel with
    | 0 => None
    | S remaining =>
        match import_parse_wire_edge input with
        | None => None
        | Some (edge, suffix) =>
            if existsb (Nat.eqb (import_wire_slot edge)) used then None
            else match import_parse_wire_records remaining suffix
                (import_wire_slot edge :: used) with
              | None => None
              | Some edges => Some (edge :: edges)
              end
        end
    end
  end.

Definition import_parse_wire_node (input : list nat) : option (list ImportWireEdge) :=
  if forallb (fun byte => byte <? 256) input
  then import_parse_wire_records 256 input []
  else None.

Lemma import_record_parse_reassembles_input : forall fuel input used edges,
  import_parse_wire_records fuel input used = Some edges ->
  input = concat (map import_wire_edge_bytes edges).
Proof.
  induction fuel as [|fuel IH]; intros [|byte rest] used edges H;
    cbn [import_parse_wire_records] in H; try discriminate; try (inversion H; reflexivity).
  destruct (import_parse_wire_edge (byte :: rest)) as [[edge suffix]|] eqn:Edge;
    try discriminate.
  destruct (existsb (Nat.eqb (import_wire_slot edge)) used) eqn:Fresh;
    try discriminate.
  destruct (import_parse_wire_records fuel suffix (import_wire_slot edge :: used))
    as [tail|] eqn:Tail; try discriminate.
  inversion H; subst edges. simpl.
  apply import_parsed_edge_preserves_exact_bytes in Edge.
  destruct Edge as [_ Bytes]. rewrite (IH _ _ _ Tail) in Bytes. exact Bytes.
Qed.

Lemma import_record_parse_preserves_record_bounds : forall fuel input used edges,
  import_parse_wire_records fuel input used = Some edges ->
  Forall import_wire_edge_bounds edges.
Proof.
  induction fuel as [|fuel IH]; intros [|byte rest] used edges H;
    cbn [import_parse_wire_records] in H; try discriminate; try (inversion H; constructor).
  destruct (import_parse_wire_edge (byte :: rest)) as [[edge suffix]|] eqn:Edge;
    try discriminate.
  destruct (existsb (Nat.eqb (import_wire_slot edge)) used); try discriminate.
  destruct (import_parse_wire_records fuel suffix (import_wire_slot edge :: used))
    as [tail|] eqn:Tail; try discriminate.
  inversion H; subst edges. constructor.
  - eapply import_parsed_edge_preserves_exact_bytes in Edge. tauto.
  - eapply IH; eauto.
Qed.

Lemma import_record_parse_count_is_bounded : forall fuel input used edges,
  import_parse_wire_records fuel input used = Some edges -> length edges <= fuel.
Proof.
  induction fuel as [|fuel IH]; intros [|byte rest] used edges H;
    cbn [import_parse_wire_records] in H; try discriminate; try (inversion H; simpl; lia).
  destruct (import_parse_wire_edge (byte :: rest)) as [[edge suffix]|]; try discriminate.
  destruct (existsb (Nat.eqb (import_wire_slot edge)) used); try discriminate.
  destruct (import_parse_wire_records fuel suffix (import_wire_slot edge :: used))
    as [tail|] eqn:Tail; try discriminate.
  inversion H; subst edges. simpl. specialize (IH _ _ _ Tail). lia.
Qed.

Lemma import_slot_absent_when_guard_is_false : forall slot used,
  existsb (Nat.eqb slot) used = false -> ~In slot used.
Proof.
  intros slot used E Present.
  assert (existsb (Nat.eqb slot) used = true) as Found.
  { apply existsb_exists. exists slot. split; auto. apply Nat.eqb_refl. }
  congruence.
Qed.

Lemma import_record_parse_retains_unique_slots : forall fuel input used edges,
  import_parse_wire_records fuel input used = Some edges ->
  NoDup (map import_wire_slot edges) /\
  Forall (fun slot => ~In slot used) (map import_wire_slot edges).
Proof.
  induction fuel as [|fuel IH]; intros [|byte rest] used edges H;
    cbn [import_parse_wire_records] in H; try discriminate; try (inversion H; split; constructor).
  destruct (import_parse_wire_edge (byte :: rest)) as [[edge suffix]|]; try discriminate.
  destruct (existsb (Nat.eqb (import_wire_slot edge)) used) eqn:Fresh;
    try discriminate.
  destruct (import_parse_wire_records fuel suffix (import_wire_slot edge :: used))
    as [tail|] eqn:Tail; try discriminate.
  inversion H; subst edges. simpl.
  specialize (IH _ _ _ Tail). destruct IH as [Unique Disjoint].
  apply import_slot_absent_when_guard_is_false in Fresh.
  split.
  - constructor; auto. intro Present.
    apply Forall_forall with (x := import_wire_slot edge) in Disjoint; auto.
    apply Disjoint. now left.
  - constructor; auto. eapply Forall_impl; [|exact Disjoint].
    intros slot NotIn Present. apply NotIn. now right.
Qed.

Theorem import_node_parse_consumes_all_bytes : forall input edges,
  import_parse_wire_node input = Some edges ->
  input = concat (map import_wire_edge_bytes edges).
Proof.
  intros input edges H. unfold import_parse_wire_node in H.
  destruct (forallb (fun byte => byte <? 256) input); try discriminate.
  eapply import_record_parse_reassembles_input; eauto.
Qed.

Theorem import_node_parse_has_unique_bounded_slots : forall input edges,
  import_parse_wire_node input = Some edges ->
  NoDup (map import_wire_slot edges) /\ length edges <= 256 /\
  Forall import_wire_edge_bounds edges.
Proof.
  intros input edges H. unfold import_parse_wire_node in H.
  destruct (forallb (fun byte => byte <? 256) input); try discriminate.
  split.
  - eapply import_record_parse_retains_unique_slots in H. tauto.
  - split.
    + eapply import_record_parse_count_is_bounded; eauto.
    + eapply import_record_parse_preserves_record_bounds; eauto.
Qed.

Theorem import_node_parse_preserves_byte_domain : forall input edges,
  import_parse_wire_node input = Some edges ->
  Forall (fun edge => Forall import_wire_byte (import_wire_edge_bytes edge)) edges.
Proof.
  intros input edges H. pose proof (import_node_parse_consumes_all_bytes _ _ H) as Bytes.
  unfold import_parse_wire_node in H.
  destruct (forallb (fun byte => byte <? 256) input) eqn:Domain; try discriminate.
  assert (Forall import_wire_byte input) as AllBytes.
  { apply Forall_forall. intros byte Present. apply forallb_forall with (x := byte) in Domain; auto.
    apply Nat.ltb_lt in Domain. exact Domain. }
  rewrite Bytes in AllBytes. clear H Bytes Domain input.
  induction edges as [|edge rest IH].
  - constructor.
  - change (Forall import_wire_byte (import_wire_edge_bytes edge ++
      concat (map import_wire_edge_bytes rest))) in AllBytes.
    apply Forall_app in AllBytes. destruct AllBytes as [Head Tail].
    constructor; auto.
Qed.

Theorem import_node_parse_returns_valid_records : forall input edges,
  import_parse_wire_node input = Some edges -> Forall import_wire_edge_valid edges.
Proof.
  intros input edges H.
  pose proof (import_node_parse_has_unique_bounded_slots _ _ H) as [_ [_ Bounds]].
  pose proof (import_node_parse_preserves_byte_domain _ _ H) as Bytes.
  apply Forall_forall. intros edge Present. split.
  - now apply Forall_forall with (x := edge) in Bounds.
  - now apply Forall_forall with (x := edge) in Bytes.
Qed.

Lemma import_valid_record_size_bounds : forall edge,
  import_wire_edge_bounds edge -> 34 <= length (import_wire_edge_bytes edge) <= 161.
Proof.
  intros edge [_ [_ [Prefix Hash]]]. unfold import_wire_edge_bytes.
  simpl. rewrite length_app, Prefix, Hash.
  pose proof (Nat.mod_upper_bound (import_wire_header edge) 128 ltac:(lia)). lia.
Qed.

Lemma import_record_stream_size_bound : forall edges,
  Forall import_wire_edge_bounds edges ->
  length (concat (map import_wire_edge_bytes edges)) <= 161 * length edges.
Proof.
  intros edges H. induction H as [|edge rest Edge Rest IH].
  - simpl. lia.
  - change (length (import_wire_edge_bytes edge ++
      concat (map import_wire_edge_bytes rest)) <= 161 * S (length rest)).
    rewrite length_app. pose proof (import_valid_record_size_bounds _ Edge). lia.
Qed.

Theorem import_node_encoded_length_is_bounded : forall input edges,
  import_parse_wire_node input = Some edges -> length input <= 256 * 161.
Proof.
  intros input edges H.
  pose proof (import_node_parse_has_unique_bounded_slots _ _ H) as [_ [Count Bounds]].
  pose proof (import_record_stream_size_bound _ Bounds) as Bytes.
  rewrite (import_node_parse_consumes_all_bytes _ _ H). lia.
Qed.

Lemma import_exact_bytes_reads_a_complete_prefix : forall selected suffix,
  import_take_exact_bytes (length selected) (selected ++ suffix) = Some (selected, suffix).
Proof.
  intros selected suffix. unfold import_take_exact_bytes.
  assert (length selected <=? length (selected ++ suffix) = true) as Enough.
  { apply Nat.leb_le. rewrite length_app. lia. }
  rewrite Enough.
  rewrite firstn_app, firstn_all, Nat.sub_diag. simpl. rewrite app_nil_r.
  rewrite skipn_app, skipn_all, Nat.sub_diag. reflexivity.
Qed.

Theorem import_valid_edge_parses_with_exact_suffix : forall edge suffix,
  import_wire_edge_bounds edge ->
  import_parse_wire_edge (import_wire_edge_bytes edge ++ suffix) = Some (edge, suffix).
Proof.
  intros [slot header prefix hash] suffix [Slot [Header [Prefix Hash]]].
  cbn [import_wire_slot import_wire_header import_wire_prefix import_wire_hash] in *.
  cbv beta iota zeta delta [import_wire_edge_bytes import_parse_wire_edge
    import_wire_slot import_wire_header import_wire_prefix import_wire_hash app].
  assert (slot <? 256 = true) as SlotOK by now apply Nat.ltb_lt.
  assert (header <? 256 = true) as HeaderOK by now apply Nat.ltb_lt.
  rewrite SlotOK, HeaderOK. cbv beta iota zeta delta [andb].
  fold (@app nat).
  rewrite <- app_assoc, <- Prefix.
  rewrite import_exact_bytes_reads_a_complete_prefix.
  rewrite <- Hash, import_exact_bytes_reads_a_complete_prefix. reflexivity.
Qed.

Lemma import_slot_absence_sets_guard_false : forall slot used,
  ~In slot used -> existsb (Nat.eqb slot) used = false.
Proof.
  intros slot used Absent.
  destruct (existsb (Nat.eqb slot) used) eqn:Found; auto.
  apply existsb_exists in Found. destruct Found as [other [Present Equal]].
  apply Nat.eqb_eq in Equal. subst. contradiction.
Qed.

Lemma import_record_parser_accepts_fresh_head : forall fuel edge suffix used,
  import_wire_edge_bounds edge -> ~In (import_wire_slot edge) used ->
  import_parse_wire_records (S fuel) (import_wire_edge_bytes edge ++ suffix) used =
  match import_parse_wire_records fuel suffix (import_wire_slot edge :: used) with
  | None => None
  | Some edges => Some (edge :: edges)
  end.
Proof.
  intros fuel edge suffix used Bounds Fresh.
  change ((match import_parse_wire_edge (import_wire_edge_bytes edge ++ suffix) with
    | None => None
    | Some (parsed, remainder) =>
        if existsb (Nat.eqb (import_wire_slot parsed)) used then None
        else match import_parse_wire_records fuel remainder (import_wire_slot parsed :: used) with
        | None => None
        | Some edges => Some (parsed :: edges)
        end
    end) = match import_parse_wire_records fuel suffix (import_wire_slot edge :: used) with
      | None => None
      | Some edges => Some (edge :: edges)
      end).
  rewrite import_valid_edge_parses_with_exact_suffix by exact Bounds.
  rewrite import_slot_absence_sets_guard_false by exact Fresh. reflexivity.
Qed.

Theorem import_record_parser_is_complete : forall edges fuel used,
  Forall import_wire_edge_bounds edges ->
  NoDup (map import_wire_slot edges) ->
  Forall (fun slot => ~In slot used) (map import_wire_slot edges) ->
  length edges <= fuel ->
  import_parse_wire_records fuel (concat (map import_wire_edge_bytes edges)) used = Some edges.
Proof.
  induction edges as [|edge rest IH]; intros fuel used Bounds Unique Fresh Count.
  { destruct fuel; reflexivity. }
  destruct fuel as [|fuel]; simpl in Count; try lia.
  change (import_parse_wire_records (S fuel)
    (import_wire_edge_bytes edge ++ concat (map import_wire_edge_bytes rest)) used =
    Some (edge :: rest)).
  inversion Bounds as [|? ? EdgeBounds RestBounds]; subst.
  inversion Unique as [|? ? NotInRest RestUnique]; subst.
  inversion Fresh as [|? ? NotInUsed RestFresh]; subst.
  rewrite import_record_parser_accepts_fresh_head; auto.
  rewrite IH; auto; try lia.
  apply Forall_forall. intros slot Present. simpl. intros [Same|Old].
  - subst. contradiction.
  - apply Forall_forall with (x := slot) in RestFresh; auto.
Qed.

Lemma import_unique_slots_derive_node_capacity : forall edges,
  Forall import_wire_edge_bounds edges ->
  NoDup (map import_wire_slot edges) -> length edges <= 256.
Proof.
  intros edges Bounds Unique.
  assert (incl (map import_wire_slot edges) (seq 0 256)) as Included.
  { intros slot Present. apply in_map_iff in Present.
    destruct Present as [edge [Same Present]]. subst slot.
    apply Forall_forall with (x := edge) in Bounds; auto.
    destruct Bounds as [Slot _]. apply in_seq. lia. }
  assert (length (map import_wire_slot edges) <= length (seq 0 256)) as Count.
  { eapply NoDup_incl_length; eauto. }
  rewrite length_map, length_seq in Count. exact Count.
Qed.

Theorem import_wire_node_round_trip_preserves_record_order : forall edges,
  Forall import_wire_edge_valid edges ->
  NoDup (map import_wire_slot edges) ->
  import_parse_wire_node (concat (map import_wire_edge_bytes edges)) = Some edges.
Proof.
  intros edges Valid Unique. unfold import_parse_wire_node.
  assert (Forall import_wire_edge_bounds edges) as Bounds.
  { eapply Forall_impl; [|exact Valid]. intros edge [H _]. exact H. }
  pose proof (import_unique_slots_derive_node_capacity _ Bounds Unique) as Count.
  assert (Forall import_wire_byte (concat (map import_wire_edge_bytes edges))) as Bytes.
  { clear Bounds Unique Count.
    induction Valid as [|edge rest [EdgeBounds EdgeBytes] Rest IH].
    - constructor.
    - change (Forall import_wire_byte (import_wire_edge_bytes edge ++
        concat (map import_wire_edge_bytes rest))).
      apply Forall_app. split; assumption. }
  assert (forallb (fun byte => byte <? 256)
    (concat (map import_wire_edge_bytes edges)) = true) as ByteGuard.
  { apply forallb_forall. intros byte Present. apply Nat.ltb_lt.
    now apply Forall_forall with (x := byte) in Bytes. }
  rewrite ByteGuard. apply import_record_parser_is_complete; auto.
  apply Forall_forall. intros slot Present. simpl. tauto.
Qed.

Theorem import_wire_node_acceptance_characterization : forall input edges,
  import_parse_wire_node input = Some edges <->
  Forall import_wire_edge_valid edges /\ NoDup (map import_wire_slot edges) /\
  input = concat (map import_wire_edge_bytes edges).
Proof.
  intros input edges. split.
  - intros Parsed. split.
    + eapply import_node_parse_returns_valid_records; eauto.
    + split.
      * pose proof (import_node_parse_has_unique_bounded_slots _ _ Parsed). tauto.
      * eapply import_node_parse_consumes_all_bytes; eauto.
  - intros [Valid [Unique Bytes]]. rewrite Bytes.
    now apply import_wire_node_round_trip_preserves_record_order.
Qed.

Theorem import_wire_header_reconstructs_kind_and_prefix : forall header,
  header < 256 ->
  header mod 128 + (if header <? 128 then 0 else 128) = header.
Proof.
  intros header Bound. destruct (header <? 128) eqn:Kind.
  - apply Nat.ltb_lt in Kind. rewrite Nat.mod_small by exact Kind. lia.
  - apply Nat.ltb_ge in Kind.
    assert (header - 128 = header mod 128) as Remainder.
    { apply Nat.mod_unique with (q := 1); lia. }
    rewrite <- Remainder. lia.
Qed.

Theorem import_valid_edge_header_matches_decoded_kind : forall edge,
  import_wire_edge_bounds edge ->
  length (import_wire_prefix edge) +
    (if import_wire_header edge <? 128 then 0 else 128) = import_wire_header edge.
Proof.
  intros edge [_ [Header [Prefix _]]]. rewrite Prefix.
  now apply import_wire_header_reconstructs_kind_and_prefix.
Qed.

Fixpoint import_wire_slot_lookup (slot : nat) (edges : list ImportWireEdge)
    : option ImportWireEdge :=
  match edges with
  | [] => None
  | edge :: rest =>
      if import_wire_slot edge =? slot then Some edge else import_wire_slot_lookup slot rest
  end.

Theorem import_wire_slot_lookup_is_sound : forall edges slot edge,
  import_wire_slot_lookup slot edges = Some edge ->
  In edge edges /\ import_wire_slot edge = slot.
Proof.
  induction edges as [|head rest IH]; intros slot edge H; simpl in H; try discriminate.
  destruct (import_wire_slot head =? slot) eqn:Same.
  - inversion H; subst. apply Nat.eqb_eq in Same. split; simpl; auto.
  - specialize (IH _ _ H). destruct IH as [Present AtSlot]. split; simpl; auto.
Qed.

Theorem import_wire_slot_lookup_is_complete : forall edges edge,
  NoDup (map import_wire_slot edges) -> In edge edges ->
  import_wire_slot_lookup (import_wire_slot edge) edges = Some edge.
Proof.
  induction edges as [|head rest IH]; intros edge Unique Present; simpl in Present;
    try contradiction.
  inversion Unique as [|? ? Fresh RestUnique]; subst.
  destruct Present as [Same|Present].
  - subst. simpl. now rewrite Nat.eqb_refl.
  - simpl. destruct (import_wire_slot head =? import_wire_slot edge) eqn:Same.
    + apply Nat.eqb_eq in Same. exfalso. apply Fresh.
      rewrite Same. now apply in_map.
    + now apply IH.
Qed.

Theorem import_missing_wire_slot_is_empty : forall edges slot,
  ~In slot (map import_wire_slot edges) -> import_wire_slot_lookup slot edges = None.
Proof.
  induction edges as [|head rest IH]; intros slot Absent; simpl; auto.
  destruct (import_wire_slot head =? slot) eqn:Same.
  - apply Nat.eqb_eq in Same. exfalso. apply Absent. simpl. auto.
  - apply IH. intros Present. apply Absent. simpl. auto.
Qed.

Definition import_wire_slot_projection (edges : list ImportWireEdge)
    : list (option ImportWireEdge) :=
  map (fun slot => import_wire_slot_lookup slot edges) (seq 0 256).

Theorem import_wire_projection_has_exactly_256_slots : forall edges,
  length (import_wire_slot_projection edges) = 256.
Proof. intros. unfold import_wire_slot_projection. now rewrite length_map, length_seq. Qed.

Theorem import_wire_projection_retains_every_record : forall input edges edge,
  import_parse_wire_node input = Some edges -> In edge edges ->
  In (Some edge) (import_wire_slot_projection edges).
Proof.
  intros input edges edge Parsed Present.
  pose proof (import_node_parse_has_unique_bounded_slots _ _ Parsed) as [Unique [_ Bounds]].
  apply Forall_forall with (x := edge) in Bounds; auto.
  destruct Bounds as [Slot _]. unfold import_wire_slot_projection.
  apply in_map_iff. exists (import_wire_slot edge). split.
  - now apply import_wire_slot_lookup_is_complete.
  - apply in_seq. lia.
Qed.

Theorem import_wire_lookup_ignores_unique_record_order : forall first second,
  NoDup (map import_wire_slot first) -> NoDup (map import_wire_slot second) ->
  (forall edge, In edge first <-> In edge second) ->
  forall slot, import_wire_slot_lookup slot first = import_wire_slot_lookup slot second.
Proof.
  intros first second FirstUnique SecondUnique Same slot.
  destruct (import_wire_slot_lookup slot first) as [edge|] eqn:First.
  - pose proof (import_wire_slot_lookup_is_sound _ _ _ First) as [Present AtSlot].
    apply Same in Present.
    pose proof (import_wire_slot_lookup_is_complete _ _ SecondUnique Present) as Found.
    rewrite AtSlot in Found. symmetry. exact Found.
  - destruct (import_wire_slot_lookup slot second) as [edge|] eqn:Second; auto.
    pose proof (import_wire_slot_lookup_is_sound _ _ _ Second) as [Present AtSlot].
    apply Same in Present.
    pose proof (import_wire_slot_lookup_is_complete _ _ FirstUnique Present) as Found.
    rewrite AtSlot in Found. congruence.
Qed.

Theorem import_wire_projection_ignores_unique_record_order : forall first second,
  NoDup (map import_wire_slot first) -> NoDup (map import_wire_slot second) ->
  (forall edge, In edge first <-> In edge second) ->
  import_wire_slot_projection first = import_wire_slot_projection second.
Proof.
  intros first second FirstUnique SecondUnique Same.
  unfold import_wire_slot_projection. apply map_ext. intros slot.
  now apply import_wire_lookup_ignores_unique_record_order.
Qed.

Record ImportDecodedWireEdge : Type := {
  import_decoded_slot : nat;
  import_decoded_is_history : bool;
  import_decoded_prefix : list nat;
  import_decoded_target : list nat
}.

Definition import_decode_wire_edge (edge : ImportWireEdge) : ImportDecodedWireEdge :=
  {| import_decoded_slot := import_wire_slot edge;
     import_decoded_is_history := negb (import_wire_header edge <? 128);
     import_decoded_prefix := import_wire_prefix edge;
     import_decoded_target := import_wire_hash edge |}.

Definition import_encode_decoded_wire_edge (edge : ImportDecodedWireEdge) : list nat :=
  import_decoded_slot edge ::
    (length (import_decoded_prefix edge) + (if import_decoded_is_history edge then 128 else 0)) ::
    (import_decoded_prefix edge ++ import_decoded_target edge).

Theorem import_decoded_edge_retains_original_record_bytes : forall edge,
  import_wire_edge_bounds edge ->
  import_encode_decoded_wire_edge (import_decode_wire_edge edge) = import_wire_edge_bytes edge.
Proof.
  intros edge Bounds.
  pose proof (import_valid_edge_header_matches_decoded_kind _ Bounds) as Header.
  unfold import_encode_decoded_wire_edge, import_decode_wire_edge.
  cbn [import_decoded_slot import_decoded_prefix import_decoded_is_history import_decoded_target].
  unfold import_wire_edge_bytes.
  destruct (import_wire_header edge <? 128); simpl in Header; simpl; now rewrite Header.
Qed.

Theorem import_decoded_record_order_retains_original_input : forall input edges,
  import_parse_wire_node input = Some edges ->
  concat (map (fun edge => import_encode_decoded_wire_edge (import_decode_wire_edge edge)) edges) = input.
Proof.
  intros input edges Parsed.
  pose proof (import_node_parse_has_unique_bounded_slots _ _ Parsed) as [_ [_ Bounds]].
  rewrite (import_node_parse_consumes_all_bytes _ _ Parsed).
  f_equal. apply map_ext_in. intros edge Present.
  apply Forall_forall with (x := edge) in Bounds; auto.
  now apply import_decoded_edge_retains_original_record_bytes.
Qed.

Theorem import_decoded_record_order_preserves_any_hash :
  forall (Hash : list nat -> nat) input edges,
  import_parse_wire_node input = Some edges ->
  Hash (concat (map (fun edge =>
    import_encode_decoded_wire_edge (import_decode_wire_edge edge)) edges)) = Hash input.
Proof.
  intros Hash input edges Parsed. f_equal.
  now apply import_decoded_record_order_retains_original_input.
Qed.

Example import_node_parser_rejects_partial_header : import_parse_wire_node [1] = None.
Proof. reflexivity. Qed.

Example import_node_parser_rejects_partial_hash : import_parse_wire_node [1; 0; 7] = None.
Proof. reflexivity. Qed.

Example import_node_parser_accepts_empty_node : import_parse_wire_node [] = Some [].
Proof. reflexivity. Qed.

Definition import_zero_prefix_wire_edge (slot : nat) : ImportWireEdge :=
  {| import_wire_slot := slot; import_wire_header := 0;
     import_wire_prefix := []; import_wire_hash := repeat 0 32 |}.

Example import_node_parser_rejects_duplicate_slot :
  import_parse_wire_node
    (import_wire_edge_bytes (import_zero_prefix_wire_edge 1) ++
     import_wire_edge_bytes (import_zero_prefix_wire_edge 1)) = None.
Proof. reflexivity. Qed.

Example import_node_parser_accepts_unique_out_of_order_slots :
  import_parse_wire_node
    (import_wire_edge_bytes (import_zero_prefix_wire_edge 2) ++
     import_wire_edge_bytes (import_zero_prefix_wire_edge 1)) =
  Some [import_zero_prefix_wire_edge 2; import_zero_prefix_wire_edge 1].
Proof. reflexivity. Qed.

Definition import_boundary_wire_edge (slot header : nat) : ImportWireEdge :=
  {| import_wire_slot := slot; import_wire_header := header;
     import_wire_prefix := repeat 0 (header mod 128); import_wire_hash := repeat 255 32 |}.

Example import_node_parser_accepts_maximum_leaf_prefix_and_slot :
  import_parse_wire_node (import_wire_edge_bytes (import_boundary_wire_edge 255 127)) =
    Some [import_boundary_wire_edge 255 127].
Proof. reflexivity. Qed.

Example import_node_parser_accepts_maximum_history_prefix :
  import_parse_wire_node (import_wire_edge_bytes (import_boundary_wire_edge 0 255)) =
    Some [import_boundary_wire_edge 0 255].
Proof. reflexivity. Qed.

Example import_node_parser_accepts_zero_history_prefix :
  import_parse_wire_node (import_wire_edge_bytes (import_boundary_wire_edge 255 128)) =
    Some [import_boundary_wire_edge 255 128].
Proof. reflexivity. Qed.

Example import_node_parser_rejects_partial_prefix : import_parse_wire_node [1; 2; 7] = None.
Proof. reflexivity. Qed.

Example import_node_parser_rejects_out_of_domain_prefix_byte :
  import_parse_wire_node ([1; 1; 256] ++ repeat 0 32) = None.
Proof. reflexivity. Qed.

Example import_node_parser_rejects_out_of_domain_target_byte :
  import_parse_wire_node ([1; 0; 256] ++ repeat 0 31) = None.
Proof. reflexivity. Qed.

Fixpoint import_key_to_nat (bytes : list nat) : nat :=
  match bytes with
  | [] => 0
  | byte :: rest => byte + 256 * import_key_to_nat rest
  end.

Fixpoint import_nat_to_key (width value : nat) : list nat :=
  match width with
  | 0 => []
  | S remaining => value mod 256 :: import_nat_to_key remaining (value / 256)
  end.

Theorem import_key_conversion_is_reversible : forall bytes,
  Forall import_wire_byte bytes ->
  import_nat_to_key (length bytes) (import_key_to_nat bytes) = bytes.
Proof.
  intros bytes Valid. induction Valid as [|byte rest Byte Rest IH].
  { reflexivity. }
  change ((byte + 256 * import_key_to_nat rest) mod 256 ::
    import_nat_to_key (length rest) ((byte + 256 * import_key_to_nat rest) / 256) = byte :: rest).
  assert (byte = (byte + 256 * import_key_to_nat rest) mod 256) as Remainder.
  { apply Nat.mod_unique with (q := import_key_to_nat rest); unfold import_wire_byte in Byte; lia. }
  assert (import_key_to_nat rest = (byte + 256 * import_key_to_nat rest) / 256) as Quotient.
  { apply Nat.div_unique with (r := byte); unfold import_wire_byte in Byte; lia. }
  rewrite <- Remainder, <- Quotient. now rewrite IH.
Qed.

Theorem import_key_conversion_preserves_exact_width : forall width value,
  length (import_nat_to_key width value) = width.
Proof. induction width; intros value; simpl; auto. Qed.

Theorem import_decoded_numeric_key_has_valid_bytes : forall width value,
  Forall import_wire_byte (import_nat_to_key width value).
Proof.
  induction width; intros value.
  - constructor.
  - change (Forall import_wire_byte
      (value mod 256 :: import_nat_to_key width (value / 256))).
    constructor; auto. unfold import_wire_byte. apply Nat.mod_upper_bound. lia.
Qed.

Theorem import_equal_width_key_conversion_has_no_aliases : forall first second,
  Forall import_wire_byte first -> Forall import_wire_byte second ->
  length first = length second -> import_key_to_nat first = import_key_to_nat second ->
  first = second.
Proof.
  intros first second First Second Width Same.
  pose proof (import_key_conversion_is_reversible _ First) as DecodeFirst.
  pose proof (import_key_conversion_is_reversible _ Second) as DecodeSecond.
  rewrite Width, Same in DecodeFirst. congruence.
Qed.

Theorem import_32_byte_storage_keys_do_not_alias : forall first second,
  Forall import_wire_byte first -> Forall import_wire_byte second ->
  length first = 32 -> length second = 32 ->
  import_key_to_nat first = import_key_to_nat second -> first = second.
Proof.
  intros first second First Second WidthFirst WidthSecond Same.
  eapply import_equal_width_key_conversion_has_no_aliases; eauto. lia.
Qed.

Definition import_codec_edge_to_closure (edge : ImportWireEdge) : ImportRadixEdge :=
  {| import_edge_index := import_wire_slot edge;
     import_edge_prefix := import_wire_prefix edge;
     import_edge_target := if import_wire_header edge <? 128
       then ImportRadixLeaf (import_key_to_nat (import_wire_hash edge))
       else ImportRadixNode (import_key_to_nat (import_wire_hash edge)) |}.

Definition import_closure_edge_to_decoded (edge : ImportRadixEdge) : ImportDecodedWireEdge :=
  {| import_decoded_slot := import_edge_index edge;
     import_decoded_is_history := match import_edge_target edge with
       | ImportRadixNode _ => true | ImportRadixLeaf _ => false end;
     import_decoded_prefix := import_edge_prefix edge;
     import_decoded_target := import_nat_to_key 32 (match import_edge_target edge with
       | ImportRadixNode key => key | ImportRadixLeaf key => key end) |}.

Lemma import_valid_wire_target_has_exact_byte_width : forall edge,
  import_wire_edge_valid edge ->
  length (import_wire_hash edge) = 32 /\ Forall import_wire_byte (import_wire_hash edge).
Proof.
  intros edge [[_ [_ [_ Width]]] Bytes]. split; auto.
  unfold import_wire_edge_bytes in Bytes.
  apply Forall_inv_tail in Bytes. apply Forall_inv_tail in Bytes.
  apply Forall_app in Bytes. tauto.
Qed.

Theorem import_closure_mapping_retains_decoded_edge : forall edge,
  import_wire_edge_valid edge ->
  import_closure_edge_to_decoded (import_codec_edge_to_closure edge) = import_decode_wire_edge edge.
Proof.
  intros edge Valid.
  pose proof (import_valid_wire_target_has_exact_byte_width _ Valid) as [Width Bytes].
  pose proof (import_key_conversion_is_reversible _ Bytes) as Reversible.
  rewrite Width in Reversible.
  unfold import_closure_edge_to_decoded, import_codec_edge_to_closure, import_decode_wire_edge.
  cbn [import_edge_index import_edge_prefix import_edge_target].
  destruct (import_wire_header edge <? 128); cbn [negb]; now rewrite Reversible.
Qed.

Theorem import_closure_mapping_retains_original_record_bytes : forall edge,
  import_wire_edge_valid edge ->
  import_encode_decoded_wire_edge
    (import_closure_edge_to_decoded (import_codec_edge_to_closure edge)) = import_wire_edge_bytes edge.
Proof.
  intros edge [Bounds Bytes].
  rewrite import_closure_mapping_retains_decoded_edge by now split.
  now apply import_decoded_edge_retains_original_record_bytes.
Qed.

Definition import_closure_records_to_bytes (edges : list ImportRadixEdge) : list nat :=
  concat (map (fun edge => import_encode_decoded_wire_edge
    (import_closure_edge_to_decoded edge)) edges).

Theorem import_closure_mapping_retains_original_input : forall input edges,
  import_parse_wire_node input = Some edges ->
  import_closure_records_to_bytes (map import_codec_edge_to_closure edges) = input.
Proof.
  intros input edges Parsed.
  pose proof (import_node_parse_returns_valid_records _ _ Parsed) as Valid.
  unfold import_closure_records_to_bytes. rewrite map_map.
  rewrite (import_node_parse_consumes_all_bytes _ _ Parsed).
  f_equal. apply map_ext_in. intros edge Present.
  apply Forall_forall with (x := edge) in Valid; auto.
  now apply import_closure_mapping_retains_original_record_bytes.
Qed.

Theorem import_raw_hash_factors_through_closure_mapping :
  forall (Hash : list nat -> nat) input edges,
  import_parse_wire_node input = Some edges ->
  Hash (import_closure_records_to_bytes (map import_codec_edge_to_closure edges)) = Hash input.
Proof.
  intros Hash input edges Parsed. f_equal.
  now apply import_closure_mapping_retains_original_input.
Qed.

Theorem import_history_mapping_retains_occurrence_context : forall edge context,
  128 <= import_wire_header edge ->
  import_edge_reference context (import_codec_edge_to_closure edge) =
    Some (ImportHistoryRef (import_key_to_nat (import_wire_hash edge))
      (context ++ import_wire_slot edge :: import_wire_prefix edge)).
Proof.
  intros edge context Kind.
  assert (import_wire_header edge <? 128 = false) as Header by now apply Nat.ltb_ge.
  unfold import_edge_reference, import_codec_edge_to_closure.
  rewrite Header. reflexivity.
Qed.

Theorem import_leaf_mapping_retains_occurrence_kind : forall edge context kind,
  import_wire_header edge < 128 ->
  import_kind_at_path (context ++ import_wire_slot edge :: import_wire_prefix edge) = Some kind ->
  import_edge_reference context (import_codec_edge_to_closure edge) =
    Some (ImportColdRef (import_key_to_nat (import_wire_hash edge)) kind).
Proof.
  intros edge context kind Kind Path.
  assert (import_wire_header edge <? 128 = true) as Header by now apply Nat.ltb_lt.
  unfold import_edge_reference, import_codec_edge_to_closure.
  rewrite Header. cbn [import_edge_index import_edge_prefix import_edge_target].
  now rewrite Path.
Qed.

Example import_variable_width_keys_require_a_width_premise :
  import_key_to_nat [0] = import_key_to_nat [0; 0] /\ [0] <> [0; 0].
Proof. split; [reflexivity|discriminate]. Qed.
