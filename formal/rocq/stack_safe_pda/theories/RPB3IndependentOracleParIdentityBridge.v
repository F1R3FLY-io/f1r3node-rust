(** * The two reducers compute the same Par identity

    Leaf L10 of the score-arms design.  The target's [par_identity_bytes]
    removes the byte span of the root tag-9 field (locally_free) from a Par
    body.  The oracle's [source_par_identity_bytes] re-serializes the typed
    root children other than tag 9 with canonical keys and lengths.  On a body
    the decoder accepts, the two agree.

    The proof has four parts:

    - [varint_groups_reencodes]: a parsed canonical varint re-encodes to its
      own bytes, so a key and a length delimiter re-serialize verbatim;
    - [framing_field_reencodes]: one accepted field re-serializes to its own
      byte span, for each of the four wire forms;
    - [region_spans_flat_map]: the spans of a parsed region partition it, so
      removing the tag-9 span is the same as removing that field;
    - [decode_fold_at_most_one_nine]: tag 9 (locally_free) is not repeated in
      the generated Par program, so it occurs at most once on a decoded body,
      which is why [find] (the first span) and [filter] (every span) agree.

    Citations: P is models/src/rust/rholang/protobuf_admission.rs.  The identity
    is the Par body minus its whole root tag-9 encoded field (P:1060-1069,
    P:1586-1615, canonical_par_identity.rs:1-7 and 39-54).  *)

From Stdlib Require Import List Bool Arith NArith ZArith Lia.
From StackSafePDA Require Import RPB3DescriptorProjectionData
  RPB3GeneratedFieldRefinement RPB3RawRegionTree RPB3SemanticReducer
  RPB3IndependentOracleCore RPB3IndependentOracleAdmission
  RPB3IndependentOracleRecursive RPB3IndependentOracleRecursiveProjection
  RPB3IndependentOracleProcessChildProvenance
  RPB3SemanticReducer RPB3IndependentOracleExprPayload.

Import ListNotations.

(** ** Byte slices *)

Lemma firstn_add_local :
  forall (A : Type) (n m : nat) (l : list A),
    firstn (n + m) l = firstn n l ++ firstn m (skipn n l).
Proof.
  intros A n.
  induction n as [|n IH]; intros m l; [reflexivity|].
  destruct l as [|head tail].
  - simpl. rewrite firstn_nil. reflexivity.
  - simpl. rewrite IH. reflexivity.
Qed.

Lemma slice_append :
  forall (bytes : list nat) first middle last,
    first <= middle -> middle <= last ->
    firstn (middle - first) (skipn first bytes) ++
      firstn (last - middle) (skipn middle bytes) =
    firstn (last - first) (skipn first bytes).
Proof.
  intros bytes first middle last ordered_first ordered_last.
  replace (last - first) with ((middle - first) + (last - middle)) by lia.
  rewrite firstn_add_local.
  rewrite skipn_skipn.
  replace (middle - first + first) with middle by lia.
  reflexivity.
Qed.

Lemma skipn_cons_of_nth_error :
  forall (bytes : list nat) offset byte,
    nth_error bytes offset = Some byte ->
    skipn offset bytes = byte :: skipn (S offset) bytes.
Proof.
  intros bytes.
  induction bytes as [|head tail IH]; intros [|offset] byte read;
    simpl in read; try discriminate read.
  - inversion read. reflexivity.
  - simpl. exact (IH offset byte read).
Qed.

(** ** Canonical varints re-encode *)

Lemma source_varint_groups_final :
  forall fuel low,
    low < 128 ->
    source_varint_groups (S fuel) (N.of_nat low) = [low].
Proof.
  intros fuel low small.
  cbn [source_varint_groups].
  assert (is_small : N.ltb (N.of_nat low) 128 = true).
  { apply N.ltb_lt. lia. }
  rewrite is_small, Nat2N.id.
  reflexivity.
Qed.

Lemma source_varint_groups_continues :
  forall fuel low high,
    low < 128 ->
    high <> 0%N ->
    source_varint_groups (S fuel) (N.of_nat low + 128 * high)%N =
    (low + 128) :: source_varint_groups fuel high.
Proof.
  intros fuel low high small nonzero.
  assert (not_small : N.ltb (N.of_nat low + 128 * high)%N 128 = false).
  { apply N.ltb_ge. lia. }
  assert (same_mod : N.modulo (N.of_nat low + 128 * high)%N 128 = N.of_nat low).
  { symmetry. apply (N.mod_unique _ 128 high (N.of_nat low)).
    - lia.
    - lia. }
  assert (same_div : N.div (N.of_nat low + 128 * high)%N 128 = high).
  { symmetry. apply (N.div_unique _ 128 high (N.of_nat low)).
    - lia.
    - lia. }
  cbn [source_varint_groups].
  rewrite not_small, same_mod, same_div, Nat2N.id.
  reflexivity.
Qed.

(** A successful parse from group [group] with accumulator [acc] has read the
    value [acc + suffix * 128 ^ group] where [suffix] is the integer written
    by the groups read, and those groups are the canonical re-encoding of
    [suffix] because the terminal group is never a non-minimal zero.  The last
    conjunct records that, past the first group, the suffix is nonzero. *)
Lemma varint_groups_reencodes :
  forall fuel bytes region_end offset group acc value next,
    Wire.varint_groups fuel bytes region_end offset group acc =
      Some (value, next) ->
    exists suffix,
      value = (acc + suffix * 128 ^ N.of_nat group)%N /\
      firstn (next - offset) (skipn offset bytes) =
        source_varint_groups fuel suffix /\
      offset < next /\
      next <= region_end /\
      (group <> 0 -> suffix <> 0%N).
Proof.
  induction fuel as [|rest IH];
    intros bytes region_end offset group acc value next parsed;
    cbn [Wire.varint_groups] in parsed; [discriminate parsed|].
  destruct (offset <? region_end) eqn:inside; [|discriminate parsed].
  destruct (nth_error bytes offset) as [byte|] eqn:read; [|discriminate parsed].
  destruct (Wire.byte_ok byte) eqn:byte_good; [|discriminate parsed].
  destruct ((group =? 9) && (1 <? byte)) eqn:overflow; [discriminate parsed|].
  destruct (byte <? 128) eqn:small.
  - destruct ((0 <? group) && (byte =? 0)) eqn:nonminimal;
      [discriminate parsed|].
    injection parsed as same_value same_next.
    (* [injection] simplifies the sides of the new equation and unfolds
       [Nat.modulo] on the literal modulus, so the folded form of the value is
       restored by conversion. *)
    assert (value_is :
      value = (acc + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N).
    { rewrite <- same_value. reflexivity. }
    subst next.
    apply Nat.ltb_lt in inside.
    apply Nat.ltb_lt in small.
    assert (low_same : byte mod 128 = byte) by (apply Nat.mod_small; exact small).
    exists (N.of_nat byte).
    split; [rewrite value_is, low_same; reflexivity|].
    split.
    { replace (S offset - offset) with 1 by lia.
      rewrite (skipn_cons_of_nth_error bytes offset byte read).
      rewrite (source_varint_groups_final rest byte small).
      reflexivity. }
    split; [lia|].
    split; [lia|].
    intro group_nonzero.
    assert (group_positive : (0 <? group) = true) by (apply Nat.ltb_lt; lia).
    rewrite group_positive in nonminimal.
    cbn [andb] in nonminimal.
    apply Nat.eqb_neq in nonminimal.
    lia.
  - unfold Wire.byte_ok in byte_good.
    apply Nat.ltb_lt in byte_good.
    apply Nat.ltb_ge in small.
    apply Nat.ltb_lt in inside.
    change (Wire.varint_groups rest bytes region_end (S offset) (S group)
      (acc + N.of_nat (byte mod 128) * 128 ^ N.of_nat group)%N =
      Some (value, next)) in parsed.
    destruct (IH bytes region_end (S offset) (S group) _ value next parsed)
      as [suffix [same_value [encoded [advance [bounded nonzero]]]]].
    assert (suffix_nonzero : suffix <> 0%N) by (apply nonzero; lia).
    assert (low_is : byte mod 128 = byte - 128).
    { symmetry. apply (Nat.mod_unique byte 128 1 (byte - 128)); lia. }
    assert (power_step :
      (128 ^ N.of_nat (S group) = 128 * 128 ^ N.of_nat group)%N).
    { rewrite Nat2N.inj_succ. rewrite N.pow_succ_r by lia. reflexivity. }
    exists (N.of_nat (byte - 128) + 128 * suffix)%N.
    split.
    { rewrite same_value, power_step, low_is. ring. }
    split.
    { replace (next - offset) with (S (next - S offset)) by lia.
      rewrite (skipn_cons_of_nth_error bytes offset byte read).
      change (firstn (S (next - S offset)) (byte :: skipn (S offset) bytes))
        with (byte :: firstn (next - S offset) (skipn (S offset) bytes)).
      rewrite encoded.
      rewrite (source_varint_groups_continues rest (byte - 128) suffix).
      - replace (byte - 128 + 128) with byte by lia. reflexivity.
      - lia.
      - exact suffix_nonzero. }
    split; [lia|].
    split; [lia|].
    intro group_nonzero.
    lia.
Qed.

Lemma canonical_varint_reencodes :
  forall bytes region_end offset value next,
    Wire.canonical_varint bytes region_end offset = Some (value, next) ->
    firstn (next - offset) (skipn offset bytes) = source_canonical_varint value /\
    offset < next /\
    next <= region_end.
Proof.
  intros bytes region_end offset value next parsed.
  unfold Wire.canonical_varint in parsed.
  destruct (varint_groups_reencodes _ _ _ _ _ _ _ _ parsed)
    as [suffix [same_value [encoded [advance [bounded _]]]]].
  assert (value_is : value = suffix).
  { rewrite same_value.
    change (N.of_nat 0) with 0%N.
    rewrite N.pow_0_r.
    lia. }
  rewrite value_is.
  unfold source_canonical_varint.
  split; [exact encoded|split; assumption].
Qed.

Lemma key_number_wire_recomposes :
  forall key code,
    N.to_nat (key mod 8)%N = code ->
    N.of_nat (N.to_nat (key / 8)%N * 8 + code) = key.
Proof.
  intros key code same_code.
  subst code.
  rewrite Nat2N.inj_add, Nat2N.inj_mul, !N2Nat.id.
  change (N.of_nat 8) with 8%N.
  rewrite N.mul_comm.
  symmetry.
  apply N.div_mod.
  discriminate.
Qed.

(** ** One field re-serializes to its own span *)

(** A raw field carrying the span's tag, wire and payload re-serializes to the
    bytes of the whole span. *)
Definition span_encodes (bytes : list nat) (span : Wire.field_span) : Prop :=
  forall raw,
    raw_tag raw = Wire.span_number span ->
    raw_wire raw = wire_kind_of_span span ->
    raw_body raw = firstn (Wire.span_end span - Wire.span_payload_start span)
      (skipn (Wire.span_payload_start span) bytes) ->
    source_encoded_field raw =
      firstn (Wire.span_end span - Wire.span_start span)
        (skipn (Wire.span_start span) bytes).

Lemma framing_field_reencodes :
  forall bytes region_end offset last ordered field,
    region_end <= length bytes ->
    Wire.framing_field bytes region_end offset last ordered = Some field ->
    forall raw,
      raw_tag raw = Wire.span_number field ->
      raw_wire raw = wire_kind_of_span field ->
      raw_body raw = firstn (Wire.span_end field - Wire.span_payload_start field)
        (skipn (Wire.span_payload_start field) bytes) ->
      source_encoded_field raw =
        firstn (Wire.span_end field - offset) (skipn offset bytes).
Proof.
  intros bytes region_end offset last ordered field within framed raw
    raw_tag_is raw_wire_is raw_body_is.
  unfold Wire.framing_field in framed.
  destruct (Wire.canonical_varint bytes region_end offset)
    as [[key key_end]|] eqn:key_read; [|discriminate framed].
  destruct (canonical_varint_reencodes _ _ _ _ _ key_read)
    as [key_bytes [key_advance key_bounded]].
  cbv zeta in framed.
  destruct ((0 <? key / 8)%N && Wire.field_number_within_limit (key / 8)%N)
    eqn:number_ok; [|discriminate framed].
  destruct (negb ordered || (last <=? N.to_nat (key / 8)%N))
    eqn:order_ok; [|discriminate framed].
  destruct (N.to_nat (key mod 8)%N) as [|[|[|[|[|[|code]]]]]] eqn:wire_code;
    try discriminate framed.
  - (* varint *)
    destruct (Wire.canonical_varint bytes region_end key_end)
      as [[payload_value value_end]|] eqn:value_read; [|discriminate framed].
    destruct (canonical_varint_reencodes _ _ _ _ _ value_read)
      as [_ [value_advance _]].
    injection framed as same_field.
    subst field.
    cbn [Wire.span_number Wire.span_end Wire.span_payload_start]
      in raw_tag_is, raw_body_is |- *.
    cbn [wire_kind_of_span Wire.span_wire] in raw_wire_is.
    unfold source_encoded_field.
    rewrite raw_tag_is, raw_wire_is.
    cbn [source_wire_code].
    rewrite (key_number_wire_recomposes key 0 wire_code).
    rewrite <- key_bytes, raw_body_is.
    apply slice_append; lia.
  - (* fixed 64 *)
    destruct (key_end + 8 <=? region_end) eqn:fits; [|discriminate framed].
    injection framed as same_field.
    subst field.
    cbn [Wire.span_number Wire.span_end Wire.span_payload_start]
      in raw_tag_is, raw_body_is |- *.
    cbn [wire_kind_of_span Wire.span_wire] in raw_wire_is.
    unfold source_encoded_field.
    rewrite raw_tag_is, raw_wire_is.
    cbn [source_wire_code].
    rewrite (key_number_wire_recomposes key 1 wire_code).
    rewrite <- key_bytes, raw_body_is.
    apply slice_append; lia.
  - (* length delimited *)
    destruct (Wire.canonical_varint bytes region_end key_end)
      as [[payload_length body_start]|] eqn:length_read; [|discriminate framed].
    destruct (canonical_varint_reencodes _ _ _ _ _ length_read)
      as [length_bytes [length_advance length_bounded]].
    destruct (N.leb payload_length (N.of_nat (region_end - body_start)))
      eqn:fits; [|discriminate framed].
    injection framed as same_field.
    subst field.
    cbn [Wire.span_number Wire.span_end Wire.span_payload_start]
      in raw_tag_is, raw_body_is |- *.
    cbn [wire_kind_of_span Wire.span_wire] in raw_wire_is.
    apply N.leb_le in fits.
    assert (body_length : length (raw_body raw) = N.to_nat payload_length).
    { rewrite raw_body_is, length_firstn, length_skipn. lia. }
    unfold source_encoded_field.
    rewrite raw_tag_is, raw_wire_is.
    cbn [source_wire_code].
    rewrite (key_number_wire_recomposes key 2 wire_code).
    rewrite body_length, N2Nat.id.
    rewrite <- key_bytes, <- length_bytes, raw_body_is.
    rewrite (slice_append bytes key_end body_start
      (body_start + N.to_nat payload_length)) by lia.
    apply slice_append; lia.
  - (* fixed 32 *)
    destruct (key_end + 4 <=? region_end) eqn:fits; [|discriminate framed].
    injection framed as same_field.
    subst field.
    cbn [Wire.span_number Wire.span_end Wire.span_payload_start]
      in raw_tag_is, raw_body_is |- *.
    cbn [wire_kind_of_span Wire.span_wire] in raw_wire_is.
    unfold source_encoded_field.
    rewrite raw_tag_is, raw_wire_is.
    cbn [source_wire_code].
    rewrite (key_number_wire_recomposes key 5 wire_code).
    rewrite <- key_bytes, raw_body_is.
    apply slice_append; lia.
Qed.

Lemma checked_field_facts :
  forall bytes region_end offset last ordered field,
    region_end <= length bytes ->
    Wire.checked_field bytes region_end offset last ordered = Some field ->
    Wire.span_start field = offset /\
    offset < Wire.span_end field /\
    Wire.span_end field <= region_end /\
    span_encodes bytes field.
Proof.
  intros bytes region_end offset last ordered field within checked.
  unfold Wire.checked_field in checked.
  destruct (Wire.framing_field bytes region_end offset last ordered)
    as [framed_field|] eqn:framed; [|discriminate checked].
  destruct ((Wire.span_start framed_field =? offset) &&
    (offset <? Wire.span_end framed_field) &&
    (Wire.span_end framed_field <=? region_end))
    eqn:guard; [|discriminate checked].
  injection checked as same_field.
  subst field.
  repeat rewrite andb_true_iff in guard.
  destruct guard as [[start advance] bounded].
  apply Nat.eqb_eq in start.
  apply Nat.ltb_lt in advance.
  apply Nat.leb_le in bounded.
  split; [exact start|].
  split; [exact advance|].
  split; [exact bounded|].
  intros raw tag_is wire_is body_is.
  rewrite start.
  exact (framing_field_reencodes bytes region_end offset last ordered
    framed_field within framed raw tag_is wire_is body_is).
Qed.

(** ** The spans of a parsed region *)

Fixpoint region_spans (bytes : list nat) (region_end offset : nat)
    (spans : list Wire.field_span) : Prop :=
  match spans with
  | [] => offset = region_end
  | span :: rest =>
      Wire.span_start span = offset /\
      offset < Wire.span_end span /\
      Wire.span_end span <= region_end /\
      span_encodes bytes span /\
      region_spans bytes region_end (Wire.span_end span) rest
  end.

Lemma reference_remaining_region_spans :
  forall fuel bytes region_end offset last ordered spans,
    region_end <= length bytes ->
    Wire.reference_remaining fuel bytes region_end offset last ordered =
      Wire.Parsed spans ->
    region_spans bytes region_end offset spans.
Proof.
  induction fuel as [|smaller IH];
    intros bytes region_end offset last ordered spans within parsed;
    cbn [Wire.reference_remaining] in parsed;
    destruct (offset =? region_end) eqn:finished.
  - injection parsed as same_spans.
    subst spans.
    cbn [region_spans].
    apply Nat.eqb_eq; exact finished.
  - destruct (Wire.checked_field bytes region_end offset last ordered);
      discriminate parsed.
  - injection parsed as same_spans.
    subst spans.
    cbn [region_spans].
    apply Nat.eqb_eq; exact finished.
  - destruct (Wire.checked_field bytes region_end offset last ordered)
      as [field|] eqn:checked; [|discriminate parsed].
    destruct (Wire.reference_remaining smaller bytes region_end
      (Wire.span_end field) (Wire.span_number field) ordered)
      as [suffix| |] eqn:tail; try discriminate parsed.
    injection parsed as same_spans.
    subst spans.
    destruct (checked_field_facts _ _ _ _ _ _ within checked)
      as [start [advance [bounded encodes]]].
    cbn [region_spans].
    split; [exact start|].
    split; [exact advance|].
    split; [exact bounded|].
    split; [exact encodes|].
    exact (IH bytes region_end (Wire.span_end field) (Wire.span_number field)
      ordered suffix within tail).
Qed.

Lemma region_spans_member_start :
  forall bytes region_end spans offset member,
    region_spans bytes region_end offset spans ->
    In member spans ->
    offset <= Wire.span_start member.
Proof.
  intros bytes region_end spans.
  induction spans as [|span rest IH]; intros offset member chain inside.
  - destruct inside.
  - destruct chain as [start [advance [bounded [encodes tail]]]].
    destruct inside as [same|later].
    + subst member. rewrite start. lia.
    + specialize (IH (Wire.span_end span) member tail later). lia.
Qed.

Lemma region_spans_encode :
  forall bytes region_end spans offset,
    region_spans bytes region_end offset spans ->
    Forall (span_encodes bytes) spans.
Proof.
  intros bytes region_end spans.
  induction spans as [|span rest IH]; intros offset chain; [constructor|].
  destruct chain as [start [advance [bounded [encodes tail]]]].
  constructor.
  - exact encodes.
  - exact (IH (Wire.span_end span) tail).
Qed.

(** ** Tag nine occurs at most once *)

Fixpoint at_most_one_nine (spans : list Wire.field_span) : Prop :=
  match spans with
  | [] => True
  | span :: rest =>
      ((Wire.span_number span =? 9) = true ->
        find (fun candidate => Wire.span_number candidate =? 9) rest = None) /\
      at_most_one_nine rest
  end.

Definition par_tag_nine_singular (program : generated_program) : bool :=
  match resolve_tag (program_fields program) 9 with
  | Some field => negb (descriptor_repeated field) && (descriptor_slot field =? 7)
  | None => false
  end.

Lemma par_program_tag_nine_singular :
  forall program,
    lookup_projected_program 1 [] [] = Some program ->
    par_tag_nine_singular program = true.
Proof.
  intros program looked_up.
  vm_compute in looked_up.
  injection looked_up as same_program.
  rewrite <- same_program.
  vm_compute.
  reflexivity.
Qed.

Lemma singular_nine_descriptor :
  forall program span field,
    par_tag_nine_singular program = true ->
    Wire.span_number span = 9 ->
    source_descriptor program span = Some field ->
    descriptor_repeated field = false /\ descriptor_slot field = 7.
Proof.
  intros program span field singular nine selected.
  unfold source_descriptor in selected.
  rewrite nine in selected.
  unfold par_tag_nine_singular in singular.
  destruct (resolve_tag (program_fields program) 9) as [candidate|] eqn:resolved;
    [|discriminate selected].
  destruct (wire_eqb (wire_kind_of_span span) (descriptor_wire candidate));
    [|discriminate selected].
  injection selected as same_candidate.
  subst candidate.
  destruct (descriptor_repeated field); [discriminate singular|].
  cbn [negb andb] in singular.
  apply Nat.eqb_eq in singular.
  split; [reflexivity|exact singular].
Qed.

Lemma decode_step_success :
  forall program decode_child current span next,
    decode_source_step program decode_child (Some current) span = Some next ->
    exists field node,
      source_descriptor program span = Some field /\
      slot_admissible program current field = true /\
      generated_source_node decode_child span field = Some node /\
      next = DecodeAccumulator (Some (descriptor_slot field))
        (descriptor_slot field :: decode_seen_slots current)
        (node :: decode_reversed current).
Proof.
  intros program decode_child current span next stepped.
  unfold decode_source_step in stepped.
  destruct (source_descriptor program span) as [field|] eqn:resolved;
    [|discriminate stepped].
  destruct (slot_admissible program current field) eqn:slot_permitted;
    [|discriminate stepped].
  destruct (generated_source_node decode_child span field) as [node|]
    eqn:generated; [|discriminate stepped].
  cbn [option_map] in stepped.
  injection stepped as same_next.
  exists field, node.
  split; [first [exact resolved|reflexivity]|].
  split; [first [exact slot_permitted|reflexivity]|].
  split; [first [exact generated|reflexivity]|].
  symmetry. exact same_next.
Qed.

(** Once the tag-nine slot is seen no later span carries tag nine: the
    generated Par program declares tag 9 non-repeated, in slot 7, so a second
    occurrence would be refused by [slot_admissible]. *)
Lemma decode_fold_at_most_one_nine :
  forall program decode_child,
    par_tag_nine_singular program = true ->
    forall spans initial final,
      fold_left (decode_source_step program decode_child) spans
        (Some initial) = Some final ->
      (In 7 (decode_seen_slots initial) ->
        find (fun candidate => Wire.span_number candidate =? 9) spans = None) /\
      at_most_one_nine spans.
Proof.
  intros program decode_child singular spans.
  induction spans as [|span rest IH]; intros initial final folded.
  - split; [intros _; reflexivity|exact I].
  - cbn [fold_left] in folded.
    destruct (decode_source_step program decode_child (Some initial) span)
      as [next|] eqn:stepped.
    + destruct (decode_step_success _ _ _ _ _ stepped)
        as [field [node [selected [slot_permitted [generated same_next]]]]].
      subst next.
      destruct (IH _ final folded) as [no_nine_after singular_rest].
      cbn [decode_seen_slots] in no_nine_after.
      destruct (admitted_source_slot_obeys_generated_order
        program initial field slot_permitted) as [_ [_ unique]].
      cbn [find at_most_one_nine].
      destruct (Wire.span_number span =? 9) eqn:is_nine.
      * apply Nat.eqb_eq in is_nine.
        destruct (singular_nine_descriptor program span field singular
          is_nine selected) as [not_repeated slot_seven].
        rewrite not_repeated, slot_seven in unique.
        cbn [orb] in unique.
        apply negb_true_iff in unique.
        split.
        -- intro seen_before.
           exfalso.
           assert (found :
             existsb (Nat.eqb 7) (decode_seen_slots initial) = true).
           { apply existsb_exists.
             exists 7.
             split; [exact seen_before|apply Nat.eqb_refl]. }
           rewrite found in unique.
           discriminate unique.
        -- split; [|exact singular_rest].
           intros _.
           apply no_nine_after.
           left.
           exact slot_seven.
      * split.
        -- intro seen_before.
           apply no_nine_after.
           right.
           exact seen_before.
        -- split; [intro absurd_eq; discriminate absurd_eq|exact singular_rest].
    + rewrite (fold_left_none_stays_none _ _
        (decode_source_step program decode_child) rest
        (fun member => eq_refl)) in folded.
      discriminate folded.
Qed.

(** ** The decoded spans *)

Lemma decode_region_spans :
  forall smaller bytes program region_start region_end nodes,
    decode_region (S smaller) bytes program region_start region_end = Some nodes ->
    exists spans final,
      Wire.reference_region bytes region_start region_end false = Wire.Parsed spans /\
      fold_left (decode_source_step program
        (fun (child_program : generated_program) (first last : nat) =>
          decode_region smaller bytes child_program first last))
        spans (Some (DecodeAccumulator None [] [])) = Some final /\
      nodes = rev (decode_reversed final).
Proof.
  intros smaller bytes program region_start region_end nodes decoded.
  simpl in decoded.
  destruct (Wire.worklist_region bytes region_start region_end false)
    as [spans| |] eqn:framed; try discriminate decoded.
  set (decode_child := fun (child_program : generated_program)
    (first last : nat) =>
      decode_region smaller bytes child_program first last).
  change (option_map (fun state => rev (decode_reversed state))
    (fold_left (decode_source_step program decode_child) spans
      (Some (DecodeAccumulator None [] []))) = Some nodes) in decoded.
  destruct (fold_left (decode_source_step program decode_child) spans
    (Some (DecodeAccumulator None [] []))) as [final|] eqn:folded;
    [|discriminate decoded].
  cbn [option_map] in decoded.
  injection decoded as same_nodes.
  exists spans, final.
  split.
  { rewrite <- Wire.worklist_and_reference_agree_on_every_finite_region.
    exact framed. }
  split; [exact folded|].
  symmetry. exact same_nodes.
Qed.

Lemma decode_fold_tracks_spans :
  forall program decode_child spans initial final,
    fold_left (decode_source_step program decode_child) spans
      (Some initial) = Some final ->
    map source_span (rev (decode_reversed final)) =
      map source_span (rev (decode_reversed initial)) ++ spans.
Proof.
  intros program decode_child spans.
  induction spans as [|span rest IH]; intros initial final folded.
  - cbn [fold_left] in folded.
    injection folded as same_final.
    subst final.
    rewrite app_nil_r.
    reflexivity.
  - cbn [fold_left] in folded.
    destruct (decode_source_step program decode_child (Some initial) span)
      as [next|] eqn:stepped.
    + rewrite (IH next final folded).
      destruct (decode_step_success _ _ _ _ _ stepped)
        as [field [node [_ [_ [generated same_next]]]]].
      subst next.
      destruct (generated_source_node_has_selected_constructor
        decode_child span field node generated) as [same_span _].
      cbn [decode_reversed rev].
      rewrite map_app.
      cbn [map].
      rewrite same_span.
      rewrite <- app_assoc.
      reflexivity.
    + rewrite (fold_left_none_stays_none _ _
        (decode_source_step program decode_child) rest
        (fun member => eq_refl)) in folded.
      discriminate folded.
Qed.

(** ** The target's find agrees with the spans' find *)

Lemma find_tag_nine_on_nodes :
  forall program nodes,
    Forall (source_node_matches_generated_descriptor program) nodes ->
    option_map source_span
      (find (fun node => descriptor_tag (source_field node) =? 9) nodes) =
    find (fun span => Wire.span_number span =? 9) (map source_span nodes).
Proof.
  intros program nodes matched.
  induction matched as [|node rest [selected _] tail IH]; [reflexivity|].
  cbn [map find].
  pose proof (resolved_source_descriptor_has_actual_tag_and_wire program
    (source_span node) (source_field node) selected) as [tag_is _].
  rewrite tag_is.
  destruct (Wire.span_number (source_span node) =? 9); [reflexivity|exact IH].
Qed.

(** ** Evaluated children retain their raw fields *)

Lemma evaluated_node_retains_source_raw :
  forall fuel hash_fn body program node child,
    source_evaluate_node
      (source_evaluate_message fuel hash_fn) hash_fn body program node =
      Some child ->
    raw_field_of_source body node = Some (source_summary_raw child).
Proof.
  intros fuel hash_fn body program node child evaluated.
  unfold source_evaluate_node in evaluated.
  destruct (raw_field_of_source body node) as [child_raw|] eqn:converted;
    [|discriminate].
  destruct node as [span field|span field nested|span field].
  - destruct (source_typed_scalar program child_raw) as [value|] eqn:typed;
      [|discriminate].
    inversion evaluated; subst child.
    change (Some child_raw = Some (source_value_raw value)).
    rewrite (accepted_source_typed_scalar_retains_raw program child_raw value
      typed).
    reflexivity.
  - rewrite (source_evaluate_message_retains_raw fuel hash_fn child_raw child
      evaluated).
    reflexivity.
  - destruct (descriptor_payload field) eqn:field_kind;
      try discriminate evaluated;
      repeat (match type of evaluated with
        | (match ?scrutinee with _ => _ end) = _ => destruct scrutinee
        end; try discriminate evaluated);
      inversion evaluated; subst child;
      reflexivity.
Qed.

(** ** Children and spans *)

Lemma region_spans_flat_map :
  forall bytes region_end spans offset,
    region_spans bytes region_end offset spans ->
    at_most_one_nine spans ->
    flat_map (fun span =>
      if Wire.span_number span =? 9 then []
      else firstn (Wire.span_end span - Wire.span_start span)
        (skipn (Wire.span_start span) bytes)) spans =
    match find (fun span => Wire.span_number span =? 9) spans with
    | None => firstn (region_end - offset) (skipn offset bytes)
    | Some nine =>
        firstn (Wire.span_start nine - offset) (skipn offset bytes) ++
        firstn (region_end - Wire.span_end nine) (skipn (Wire.span_end nine) bytes)
    end.
Proof.
  intros bytes region_end spans.
  induction spans as [|span rest IH]; intros offset chain singular.
  - cbn [region_spans] in chain.
    subst offset.
    cbn [flat_map find].
    rewrite Nat.sub_diag.
    reflexivity.
  - destruct chain as [start [advance [bounded [encodes tail]]]].
    destruct singular as [span_singular rest_singular].
    specialize (IH (Wire.span_end span) tail rest_singular).
    cbn [flat_map find].
    destruct (Wire.span_number span =? 9) eqn:is_nine.
    + (* the root locally_free field *)
      assert (no_later :
        find (fun candidate => Wire.span_number candidate =? 9) rest = None).
      { apply span_singular. first [reflexivity|exact is_nine]. }
      rewrite no_later in IH.
      rewrite IH.
      rewrite start, Nat.sub_diag.
      reflexivity.
    + rewrite IH.
      destruct (find (fun candidate => Wire.span_number candidate =? 9) rest)
        as [nine|] eqn:found.
      * apply find_some in found as [member _].
        pose proof (region_spans_member_start bytes region_end rest
          (Wire.span_end span) nine tail member) as after.
        rewrite start.
        rewrite app_assoc.
        rewrite (slice_append bytes offset (Wire.span_end span)
          (Wire.span_start nine)) by lia.
        reflexivity.
      * rewrite start.
        apply slice_append; lia.
Qed.

Lemma flat_map_children_encode :
  forall body nodes children,
    Forall2 (fun node child =>
      raw_field_of_source body node = Some (source_summary_raw child))
      nodes children ->
    Forall (span_encodes body) (map source_span nodes) ->
    flat_map (fun child =>
      if raw_tag (source_value_raw child) =? 9 then []
      else source_encoded_field (source_value_raw child))
      (map source_summary_value children) =
    flat_map (fun span =>
      if Wire.span_number span =? 9 then []
      else firstn (Wire.span_end span - Wire.span_start span)
        (skipn (Wire.span_start span) body))
      (map source_span nodes).
Proof.
  intros body nodes children converted.
  induction converted as [|node child nodes' children' same rest IH];
    intro encodes; [reflexivity|].
  cbn [map] in encodes.
  pose proof (Forall_inv encodes) as head_encodes.
  pose proof (Forall_inv_tail encodes) as tail_encodes.
  cbn [map flat_map].
  rewrite (IH tail_encodes).
  f_equal.
  change (source_value_raw (source_summary_value child))
    with (source_summary_raw child).
  destruct (raw_field_of_source_preserves_wire_and_body body node
    (source_summary_raw child) same)
    as [tag_is [wire_is [_ [body_is _]]]].
  rewrite tag_is.
  destruct (Wire.span_number (source_span node) =? 9); [reflexivity|].
  apply head_encodes.
  - exact tag_is.
  - exact wire_is.
  - exact body_is.
Qed.

(** ** The bridge *)

Lemma par_identity_bytes_decoded :
  forall body program nodes,
    lookup_projected_program 1 [] [] = Some program ->
    decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    par_identity_bytes body =
    match find (fun node => descriptor_tag (source_field node) =? 9) nodes with
    | None => body
    | Some node =>
        firstn (Wire.span_start (source_span node)) body ++
        skipn (Wire.span_end (source_span node)) body
    end.
Proof.
  intros body program nodes looked_up decoded.
  unfold par_identity_bytes.
  rewrite looked_up.
  cbv beta iota.
  rewrite decoded.
  reflexivity.
Qed.

Lemma source_par_identity_bytes_typed_message :
  forall raw children,
    source_par_identity_bytes (TypedSourceMessage raw children) =
    flat_map (fun child =>
      if raw_tag (source_value_raw child) =? 9 then []
      else source_encoded_field (source_value_raw child)) children.
Proof.
  intros raw children.
  reflexivity.
Qed.

Theorem par_identity_bytes_project :
  forall fuel hash_fn body program nodes children raw,
    lookup_projected_program 1 [] [] = Some program ->
    decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    sequence_source_results
      (map (source_evaluate_node
             (source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    par_identity_bytes body =
    source_par_identity_bytes
      (TypedSourceMessage raw (map source_summary_value children)).
Proof.
  intros fuel hash_fn body program nodes children raw
    looked_up decoded selected.
  destruct (decode_region_spans (length body) body program 0 (length body)
    nodes decoded) as [spans [final [framed [folded same_nodes]]]].
  assert (chain : region_spans body (length body) 0 spans).
  { unfold Wire.reference_region in framed.
    destruct (Wire.region_ok body 0 (length body)); [|discriminate framed].
    exact (reference_remaining_region_spans _ _ _ _ _ _ _
      (Nat.le_refl (length body)) framed). }
  assert (spans_are : map source_span nodes = spans).
  { rewrite same_nodes, (decode_fold_tracks_spans _ _ _ _ _ folded).
    reflexivity. }
  pose proof (par_program_tag_nine_singular program looked_up) as singular.
  destruct (decode_fold_at_most_one_nine program _ singular _ _ _ folded)
    as [_ one_nine].
  pose proof (selected_source_evaluator_outputs_are_positional _ _
    (source_evaluate_node (source_evaluate_message fuel hash_fn) hash_fn body
      program) nodes children selected) as positional.
  assert (converted : Forall2 (fun node child =>
      raw_field_of_source body node = Some (source_summary_raw child))
      nodes children).
  { eapply Forall2_impl; [|exact positional].
    intros node child evaluated.
    exact (evaluated_node_retains_source_raw fuel hash_fn body program node
      child evaluated). }
  assert (encodes : Forall (span_encodes body) (map source_span nodes)).
  { rewrite spans_are. exact (region_spans_encode _ _ _ _ chain). }
  pose proof (decoded_region_nodes_match_generated_source _ _ _ _ _ _ decoded)
    as generated.
  pose proof (find_tag_nine_on_nodes program nodes generated) as related.
  rewrite spans_are in related.
  assert (tail_all : forall offset,
    firstn (length body - offset) (skipn offset body) = skipn offset body).
  { intro offset.
    apply firstn_all2.
    rewrite length_skipn.
    lia. }
  rewrite (par_identity_bytes_decoded body program nodes looked_up decoded).
  rewrite source_par_identity_bytes_typed_message.
  rewrite (flat_map_children_encode body nodes children converted encodes).
  rewrite spans_are.
  rewrite (region_spans_flat_map body (length body) spans 0 chain one_nine).
  destruct (find (fun node => descriptor_tag (source_field node) =? 9) nodes)
    as [node|] eqn:found_node.
  - cbn [option_map] in related.
    rewrite <- related.
    cbv beta iota.
    rewrite tail_all, Nat.sub_0_r.
    reflexivity.
  - cbn [option_map] in related.
    rewrite <- related.
    cbv beta iota.
    rewrite tail_all.
    reflexivity.
Qed.

(** The form used by the ESet and EMap parents: the identity computed from a
    Par child's own raw body equals the identity read from its typed value. *)
Corollary par_identity_bytes_project_of_evaluated :
  forall fuel hash_fn raw summary,
    source_evaluate_message (S fuel) hash_fn raw = Some summary ->
    raw_program_type_id raw = Some 1 ->
    par_identity_bytes (raw_body raw) =
    source_par_identity_bytes (source_summary_value summary).
Proof.
  intros fuel hash_fn raw summary evaluated par_type.
  destruct (accepted_independent_message_has_generated_children fuel hash_fn
    raw summary evaluated)
    as [type_id [program [nodes [children
      [_ [family [looked_up [decoded [shape aligned]]]]]]]]].
  rewrite par_type in family.
  injection family as same_type.
  subst type_id.
  rewrite shape.
  exact (par_identity_bytes_project fuel hash_fn (raw_body raw) program nodes
    children raw looked_up decoded
    (aligned_evaluated_children_are_selected program
      (source_evaluate_node (source_evaluate_message fuel hash_fn) hash_fn
        (raw_body raw) program)
      nodes children aligned)).
Qed.

(** ** Red control

    A mutant that also strips nested locally_free fields: every field with tag
    9 is dropped at every depth of the decoded tree, and each enclosing
    message field is re-serialized with its shortened payload.  On a body
    whose bundle contains a nested locally_free, the mutant differs from the
    identity the oracle computes, so the bridge fails for the mutant. *)

Fixpoint mutant_strip_nested (bytes : list nat) (node : source_tree)
    {struct node} : list nat :=
  let span := source_span node in
  let tag := Wire.span_number span in
  if tag =? 9 then []
  else
    match node with
    | SourceMessage _ _ nested =>
        let inner := flat_map (mutant_strip_nested bytes) nested in
        source_canonical_varint (N.of_nat (tag * 8 + 2)) ++
        source_canonical_varint (N.of_nat (length inner)) ++ inner
    | _ =>
        firstn (Wire.span_end span - Wire.span_start span)
          (skipn (Wire.span_start span) bytes)
    end.

Definition mutant_par_identity_bytes (body : list nat) : list nat :=
  match lookup_projected_program 1 [] [] with
  | None => body
  | Some program =>
      match decode_region (S (length body)) body program 0 (length body) with
      | None => body
      | Some nodes => flat_map (mutant_strip_nested body) nodes
      end
  end.

(** [74;1;9] is the root locally_free (tag 9); [90;5;...] is bundles (tag 11)
    holding a Bundle whose body Par (tag 1) has its own locally_free [74;1;7]. *)
Definition control_body : list nat := [74; 1; 9; 90; 5; 10; 3; 74; 1; 7].

Definition control_oracle_identity : option (list nat) :=
  option_map (fun summary =>
    source_par_identity_bytes (source_summary_value summary))
    (source_evaluate_message (S (length control_body)) (fun _ => 0)
      (source_par_raw control_body)).

Example genuine_identity_keeps_nested_locally_free :
  par_identity_bytes control_body = [90; 5; 10; 3; 74; 1; 7] /\
  control_oracle_identity = Some [90; 5; 10; 3; 74; 1; 7].
Proof. vm_compute. repeat split. Qed.

Example nested_strip_mutant_changes_the_identity :
  mutant_par_identity_bytes control_body = [90; 2; 10; 0].
Proof. vm_compute. reflexivity. Qed.

Example nested_strip_mutant_fails_the_bridge :
  Some (mutant_par_identity_bytes control_body) <> control_oracle_identity.
Proof. vm_compute. intro same. discriminate same. Qed.

(** The bridge statement, for an arbitrary candidate identity function. *)
Definition par_identity_statement (candidate : list nat -> list nat) : Prop :=
  forall fuel hash_fn body program nodes children raw,
    lookup_projected_program 1 [] [] = Some program ->
    decode_region (S (length body)) body program 0 (length body) = Some nodes ->
    sequence_source_results
      (map (source_evaluate_node
             (source_evaluate_message fuel hash_fn) hash_fn body program) nodes)
      = Some children ->
    candidate body =
    source_par_identity_bytes
      (TypedSourceMessage raw (map source_summary_value children)).

Corollary genuine_par_identity_satisfies_the_bridge :
  par_identity_statement par_identity_bytes.
Proof. exact par_identity_bytes_project. Qed.

Definition par_control_fuel : nat := S (length control_body).

Definition par_control_hash_fn : list nat -> nat := fun _ => 0.

Definition par_control_program : generated_program :=
  match lookup_projected_program 1 [] [] with
  | Some program => program
  | None => empty_program
  end.

Definition par_control_nodes : list source_tree :=
  match decode_region (S (length control_body)) control_body
      par_control_program 0 (length control_body) with
  | Some nodes => nodes
  | None => []
  end.

Definition par_control_children : list source_exact_summary :=
  match sequence_source_results
      (map (source_evaluate_node
        (source_evaluate_message par_control_fuel par_control_hash_fn)
        par_control_hash_fn control_body par_control_program)
        par_control_nodes) with
  | Some children => children
  | None => []
  end.

Lemma par_control_lookup :
  lookup_projected_program 1 [] [] = Some par_control_program.
Proof. vm_compute. reflexivity. Qed.

Lemma par_control_decode :
  decode_region (S (length control_body)) control_body
    par_control_program 0 (length control_body) = Some par_control_nodes.
Proof. vm_compute. reflexivity. Qed.

Lemma par_control_evaluate :
  sequence_source_results
    (map (source_evaluate_node
      (source_evaluate_message par_control_fuel par_control_hash_fn)
      par_control_hash_fn control_body par_control_program)
      par_control_nodes) =
    Some par_control_children.
Proof. vm_compute. reflexivity. Qed.

(** The nested-strip mutant does not satisfy the bridge: the premises hold on
    the decoded [control_body], and the two identities differ there. *)
Theorem nested_strip_mutant_fails_the_statement :
  ~ par_identity_statement mutant_par_identity_bytes.
Proof.
  intro holds.
  pose proof (holds par_control_fuel par_control_hash_fn control_body
    par_control_program par_control_nodes par_control_children
    (source_par_raw control_body)
    par_control_lookup par_control_decode par_control_evaluate) as agreement.
  vm_compute in agreement.
  discriminate agreement.
Qed.

Print Assumptions firstn_add_local.
Print Assumptions slice_append.
Print Assumptions skipn_cons_of_nth_error.
Print Assumptions source_varint_groups_final.
Print Assumptions source_varint_groups_continues.
Print Assumptions varint_groups_reencodes.
Print Assumptions canonical_varint_reencodes.
Print Assumptions key_number_wire_recomposes.
Print Assumptions framing_field_reencodes.
Print Assumptions checked_field_facts.
Print Assumptions reference_remaining_region_spans.
Print Assumptions region_spans_member_start.
Print Assumptions region_spans_encode.
Print Assumptions par_program_tag_nine_singular.
Print Assumptions singular_nine_descriptor.
Print Assumptions decode_step_success.
Print Assumptions decode_fold_at_most_one_nine.
Print Assumptions decode_region_spans.
Print Assumptions decode_fold_tracks_spans.
Print Assumptions find_tag_nine_on_nodes.
Print Assumptions evaluated_node_retains_source_raw.
Print Assumptions region_spans_flat_map.
Print Assumptions flat_map_children_encode.
Print Assumptions par_identity_bytes_decoded.
Print Assumptions source_par_identity_bytes_typed_message.
Print Assumptions par_identity_bytes_project.
Print Assumptions par_identity_bytes_project_of_evaluated.
Print Assumptions genuine_identity_keeps_nested_locally_free.
Print Assumptions nested_strip_mutant_changes_the_identity.
Print Assumptions nested_strip_mutant_fails_the_bridge.
Print Assumptions genuine_par_identity_satisfies_the_bridge.
Print Assumptions par_control_lookup.
Print Assumptions par_control_decode.
Print Assumptions par_control_evaluate.
Print Assumptions nested_strip_mutant_fails_the_statement.
