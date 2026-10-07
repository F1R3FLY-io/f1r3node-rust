From Stdlib Require Import Arith.PeanoNat Lists.List Bool.Bool Lia.
From CostAccountedRho Require Import SignedPhloWire.
Import ListNotations.

Record phlo_class_record := {
  class_record_identity : list nat;
  class_record_unit : list nat;
  class_record_measurement : list nat;
  class_record_valuation : list nat;
  class_record_weight : nat
}.

Definition phlo_class_fields word_width record :=
  [class_record_identity record; class_record_unit record;
   class_record_measurement record; class_record_valuation record;
   phlo_word word_width (class_record_weight record)].

Definition phlo_class_bytes field_width word_width record :=
  phlo_fields field_width (phlo_class_fields word_width record).

Definition phlo_fields_fit field_width maximum (fields : list (list nat)) :=
  Forall (fun payload => length payload < 256 ^ field_width /\ length payload <= maximum) fields.

Definition phlo_class_fits field_width word_width maximum record :=
  class_record_weight record < 256 ^ word_width /\
  phlo_fields_fit field_width maximum (phlo_class_fields word_width record).

Theorem phlo_class_fields_preserve_all_components : forall word_width first second,
  class_record_weight first < 256 ^ word_width ->
  class_record_weight second < 256 ^ word_width ->
  phlo_class_fields word_width first = phlo_class_fields word_width second -> first = second.
Proof.
  intros word_width [identity unit measurement valuation weight]
    [other_identity other_unit other_measurement other_valuation other_weight] fits other_fits same.
  cbn [phlo_class_fields class_record_identity class_record_unit class_record_measurement
    class_record_valuation class_record_weight] in *.
  injection same as identities units measurements valuations weights.
  apply phlo_word_encoding_is_injective in weights; auto. subst. reflexivity.
Qed.

Theorem phlo_class_wire_encoding_is_injective : forall field_width word_width maximum first second,
  0 < field_width -> phlo_class_fits field_width word_width maximum first ->
  phlo_class_fits field_width word_width maximum second ->
  phlo_class_bytes field_width word_width first = phlo_class_bytes field_width word_width second ->
  first = second.
Proof.
  intros field_width word_width maximum first second positive [first_weight first_fields]
    [second_weight second_fields] same.
  apply phlo_class_fields_preserve_all_components with (word_width := word_width); auto.
  eapply phlo_field_sequences_cannot_alias; eauto.
Qed.

Definition phlo_class_list_fields field_width word_width count_width records :=
  phlo_word count_width (length records) :: map (phlo_class_bytes field_width word_width) records.

Definition phlo_class_list_bytes field_width word_width count_width records :=
  phlo_fields field_width (phlo_class_list_fields field_width word_width count_width records).

Lemma phlo_class_map_preserves_records : forall field_width word_width maximum first second,
  0 < field_width ->
  Forall (phlo_class_fits field_width word_width maximum) first ->
  Forall (phlo_class_fits field_width word_width maximum) second ->
  map (phlo_class_bytes field_width word_width) first =
    map (phlo_class_bytes field_width word_width) second -> first = second.
Proof.
  intros field_width word_width maximum first. induction first as [|head tail IH];
    intros [|other rest] positive first_valid second_valid same; try discriminate; [reflexivity|].
  apply Forall_cons_iff in first_valid. destruct first_valid as [head_valid tail_valid].
  apply Forall_cons_iff in second_valid. destruct second_valid as [other_valid rest_valid].
  cbn in same. injection same as heads tails.
  assert (head = other) by (eapply phlo_class_wire_encoding_is_injective; eauto).
  subst other. f_equal. eapply IH; eauto.
Qed.

Theorem phlo_class_list_wire_preserves_order_and_count :
  forall field_width word_width count_width maximum first second,
  0 < field_width ->
  Forall (phlo_class_fits field_width word_width maximum) first ->
  Forall (phlo_class_fits field_width word_width maximum) second ->
  phlo_fields_fit field_width maximum (phlo_class_list_fields field_width word_width count_width first) ->
  phlo_fields_fit field_width maximum (phlo_class_list_fields field_width word_width count_width second) ->
  phlo_class_list_bytes field_width word_width count_width first =
    phlo_class_list_bytes field_width word_width count_width second -> first = second.
Proof.
  intros field_width word_width count_width maximum first second positive first_valid second_valid
    first_fields second_fields same.
  unfold phlo_class_list_bytes in same.
  apply (phlo_field_sequences_cannot_alias field_width maximum) in same; auto.
  unfold phlo_class_list_fields in same. injection same as counts records.
  eapply phlo_class_map_preserves_records; eauto.
Qed.

Record phlo_schedule_record := {
  schedule_record_protocol : nat;
  schedule_record_network : list nat;
  schedule_record_shard : list nat;
  schedule_record_asset : list nat;
  schedule_record_unit : list nat;
  schedule_record_scale : nat;
  schedule_record_classes : list phlo_class_record;
  schedule_record_price : nat;
  schedule_record_compatibility : list nat
}.

Definition schedule_policy_fields record :=
  (schedule_record_protocol record, schedule_record_network record,
   schedule_record_shard record, schedule_record_asset record,
   schedule_record_unit record, schedule_record_scale record,
   schedule_record_classes record, schedule_record_compatibility record).

Definition schedule_at_offer policy price : phlo_schedule_record := {|
  schedule_record_protocol := schedule_record_protocol policy;
  schedule_record_network := schedule_record_network policy;
  schedule_record_shard := schedule_record_shard policy;
  schedule_record_asset := schedule_record_asset policy;
  schedule_record_unit := schedule_record_unit policy;
  schedule_record_scale := schedule_record_scale policy;
  schedule_record_classes := schedule_record_classes policy;
  schedule_record_price := price;
  schedule_record_compatibility := schedule_record_compatibility policy
|}.

Theorem offered_schedule_keeps_complete_policy : forall policy price,
  schedule_policy_fields (schedule_at_offer policy price) = schedule_policy_fields policy.
Proof. reflexivity. Qed.

Theorem offered_schedule_has_exact_price : forall policy price,
  schedule_record_price (schedule_at_offer policy price) = price.
Proof. reflexivity. Qed.

Theorem policy_and_offer_determine_complete_schedule : forall selected policy price,
  schedule_policy_fields selected = schedule_policy_fields policy ->
  schedule_record_price selected = price -> selected = schedule_at_offer policy price.
Proof.
  intros [sp sn ss sa su sd sc sv sk] [pp pn ps pa pu pd pc pv pk] price same offered.
  cbn in same, offered. inversion same. subst. reflexivity.
Qed.

Theorem changed_policy_cannot_be_authorized_by_price : forall selected policy price,
  schedule_policy_fields selected <> schedule_policy_fields policy ->
  selected <> schedule_at_offer policy price.
Proof.
  intros selected policy price different same. subst selected.
  apply different. apply offered_schedule_keeps_complete_policy.
Qed.

Print Assumptions offered_schedule_keeps_complete_policy.
Print Assumptions offered_schedule_has_exact_price.
Print Assumptions policy_and_offer_determine_complete_schedule.
Print Assumptions changed_policy_cannot_be_authorized_by_price.

Definition genesis_policy_read (genesis queried : nat)
    (storage : nat -> option phlo_schedule_record) :=
  if Nat.eqb genesis queried then storage genesis else None.

Theorem genesis_policy_read_has_no_local_fallback : forall genesis storage,
  storage genesis = None -> genesis_policy_read genesis genesis storage = None.
Proof. intros. unfold genesis_policy_read. rewrite Nat.eqb_refl. assumption. Qed.

Theorem genesis_policy_read_rejects_another_root : forall genesis queried storage,
  genesis <> queried -> genesis_policy_read genesis queried storage = None.
Proof.
  intros. unfold genesis_policy_read.
  apply Nat.eqb_neq in H. rewrite H. reflexivity.
Qed.

Theorem genesis_policy_read_agrees_across_validators : forall genesis left right,
  left genesis = right genesis ->
  genesis_policy_read genesis genesis left = genesis_policy_read genesis genesis right.
Proof. intros. unfold genesis_policy_read. rewrite Nat.eqb_refl. assumption. Qed.

Theorem genesis_policy_read_preserves_history : forall genesis before after,
  before genesis = after genesis -> forall queried,
  genesis_policy_read genesis queried before = genesis_policy_read genesis queried after.
Proof. intros. unfold genesis_policy_read. destruct (Nat.eqb genesis queried); auto. Qed.

Theorem genesis_policy_canonical_price_is_not_an_offer : forall policy first second,
  schedule_at_offer (schedule_at_offer policy first) 0 =
  schedule_at_offer (schedule_at_offer policy second) 0.
Proof. reflexivity. Qed.

Definition phlo_schedule_fields domain field_width word_width count_width scale_width record :=
  [domain; phlo_word word_width (schedule_record_protocol record);
   schedule_record_network record; schedule_record_shard record;
   schedule_record_asset record; schedule_record_unit record;
   phlo_word scale_width (schedule_record_scale record);
   phlo_class_list_bytes field_width word_width count_width (schedule_record_classes record);
   phlo_word word_width (schedule_record_price record); phlo_word word_width 1;
   schedule_record_compatibility record].

Definition phlo_schedule_bytes domain field_width word_width count_width scale_width record :=
  phlo_fields field_width (phlo_schedule_fields domain field_width word_width count_width scale_width record).

Definition phlo_schedule_fits domain field_width word_width count_width scale_width maximum record :=
  schedule_record_protocol record < 256 ^ word_width /\
  schedule_record_scale record < 256 ^ scale_width /\
  schedule_record_price record < 256 ^ word_width /\
  Forall (phlo_class_fits field_width word_width maximum) (schedule_record_classes record) /\
  phlo_fields_fit field_width maximum
    (phlo_class_list_fields field_width word_width count_width (schedule_record_classes record)) /\
  phlo_fields_fit field_width maximum
    (phlo_schedule_fields domain field_width word_width count_width scale_width record).

Theorem phlo_schedule_wire_binds_every_component :
  forall domain field_width word_width count_width scale_width maximum first second,
  0 < field_width ->
  phlo_schedule_fits domain field_width word_width count_width scale_width maximum first ->
  phlo_schedule_fits domain field_width word_width count_width scale_width maximum second ->
  phlo_schedule_bytes domain field_width word_width count_width scale_width first =
    phlo_schedule_bytes domain field_width word_width count_width scale_width second -> first = second.
Proof.
  intros domain field_width word_width count_width scale_width maximum first second positive
    [first_protocol [first_scale [first_price [first_classes [first_class_fields first_fields]]]]]
    [second_protocol [second_scale [second_price [second_classes [second_class_fields second_fields]]]]] same.
  unfold phlo_schedule_bytes in same.
  apply (phlo_field_sequences_cannot_alias field_width maximum) in same; auto.
  destruct first as [protocol network shard asset unit scale classes price compatibility].
  destruct second as [other_protocol other_network other_shard other_asset other_unit other_scale
    other_classes other_price other_compatibility].
  cbn [phlo_schedule_fields schedule_record_protocol schedule_record_network schedule_record_shard
    schedule_record_asset schedule_record_unit schedule_record_scale schedule_record_classes
    schedule_record_price schedule_record_compatibility] in *.
  injection same as protocols networks shards assets units scales class_tables prices compatibilities.
  apply phlo_word_encoding_is_injective in protocols; auto.
  apply phlo_word_encoding_is_injective in scales; auto.
  apply phlo_word_encoding_is_injective in prices; auto.
  assert (classes = other_classes) by (eapply phlo_class_list_wire_preserves_order_and_count; eauto).
  subst. reflexivity.
Qed.

Theorem phlo_schedule_format_domains_do_not_alias :
  forall first_domain second_domain field_width word_width count_width scale_width maximum first second,
  0 < field_width ->
  phlo_fields_fit field_width maximum
    (phlo_schedule_fields first_domain field_width word_width count_width scale_width first) ->
  phlo_fields_fit field_width maximum
    (phlo_schedule_fields second_domain field_width word_width count_width scale_width second) ->
  phlo_schedule_bytes first_domain field_width word_width count_width scale_width first =
    phlo_schedule_bytes second_domain field_width word_width count_width scale_width second ->
  first_domain = second_domain.
Proof.
  intros first_domain second_domain field_width word_width count_width scale_width maximum first second
    positive first_valid second_valid same.
  unfold phlo_schedule_bytes in same.
  apply (phlo_field_sequences_cannot_alias field_width maximum) in same; auto.
  exact (f_equal (fun fields => hd [] fields) same).
Qed.

Print Assumptions phlo_schedule_format_domains_do_not_alias.
Print Assumptions phlo_class_fields_preserve_all_components.
Print Assumptions phlo_class_wire_encoding_is_injective.
Print Assumptions phlo_class_map_preserves_records.
Print Assumptions phlo_class_list_wire_preserves_order_and_count.
Print Assumptions phlo_schedule_wire_binds_every_component.

Definition native_rule_fields_match expected offered :=
  if list_eq_dec Nat.eq_dec (class_record_unit expected) (class_record_unit offered) then
    if list_eq_dec Nat.eq_dec (class_record_measurement expected) (class_record_measurement offered) then
      if list_eq_dec Nat.eq_dec (class_record_valuation expected) (class_record_valuation offered)
      then true else false
    else false
  else false.

Theorem native_rule_match_preserves_interpretation : forall expected offered,
  native_rule_fields_match expected offered = true <->
  class_record_unit expected = class_record_unit offered /\
  class_record_measurement expected = class_record_measurement offered /\
  class_record_valuation expected = class_record_valuation offered.
Proof.
  intros. unfold native_rule_fields_match.
  repeat destruct (list_eq_dec _ _ _); intuition congruence.
Qed.

Definition native_class_supported registry offered :=
  existsb (fun expected => native_rule_fields_match expected offered) registry.

Definition accept_native_schedule registry compatibility schedule :=
  if list_eq_dec Nat.eq_dec compatibility (schedule_record_compatibility schedule) then
    if forallb (native_class_supported registry) (schedule_record_classes schedule)
    then Some schedule else None
  else None.

Theorem accepted_native_schedule_keeps_every_field : forall registry compatibility schedule accepted,
  accept_native_schedule registry compatibility schedule = Some accepted -> accepted = schedule.
Proof.
  intros. unfold accept_native_schedule in H.
  destruct (list_eq_dec _ _ _); [destruct (forallb _ _)|]; inversion H; reflexivity.
Qed.

Theorem accepted_native_schedule_has_supported_rules : forall registry compatibility schedule accepted offered,
  accept_native_schedule registry compatibility schedule = Some accepted ->
  In offered (schedule_record_classes accepted) ->
  exists expected, In expected registry /\
    class_record_unit expected = class_record_unit offered /\
    class_record_measurement expected = class_record_measurement offered /\
    class_record_valuation expected = class_record_valuation offered.
Proof.
  intros registry compatibility schedule accepted offered checked included.
  pose proof (accepted_native_schedule_keeps_every_field _ _ _ _ checked) as same.
  subst accepted. unfold accept_native_schedule in checked.
  destruct (list_eq_dec _ _ _); [|discriminate].
  destruct (forallb _ _) eqn:supported; [|discriminate].
  apply forallb_forall with (x := offered) in supported; auto.
  unfold native_class_supported in supported. apply existsb_exists in supported.
  destruct supported as [expected [present matched]].
  exists expected. split; auto. now apply native_rule_match_preserves_interpretation.
Qed.

Theorem accepted_native_schedule_keeps_compatibility : forall registry compatibility schedule accepted,
  accept_native_schedule registry compatibility schedule = Some accepted ->
  schedule_record_compatibility accepted = compatibility.
Proof.
  intros. unfold accept_native_schedule in H.
  destruct (list_eq_dec _ _ _) as [same|different]; [|discriminate].
  destruct (forallb _ _); inversion H; subst; auto.
Qed.

Theorem unsupported_native_rule_cannot_be_activated : forall registry compatibility schedule offered,
  In offered (schedule_record_classes schedule) ->
  native_class_supported registry offered = false ->
  accept_native_schedule registry compatibility schedule = None.
Proof.
  intros registry compatibility schedule offered included unsupported.
  unfold accept_native_schedule. destruct (list_eq_dec _ _ _); [|reflexivity].
  destruct (forallb _ _) eqn:supported; [|reflexivity].
  apply forallb_forall with (x := offered) in supported; auto. congruence.
Qed.

Print Assumptions native_rule_match_preserves_interpretation.
Print Assumptions accepted_native_schedule_keeps_every_field.
Print Assumptions accepted_native_schedule_has_supported_rules.
Print Assumptions accepted_native_schedule_keeps_compatibility.
Print Assumptions unsupported_native_rule_cannot_be_activated.

Definition phlo_class_record_eq_dec : forall (left right : phlo_class_record),
  {left = right} + {left <> right}.
Proof. decide equality; try apply Nat.eq_dec; apply list_eq_dec; apply Nat.eq_dec. Defined.

Definition phlo_schedule_record_eq_dec : forall (left right : phlo_schedule_record),
  {left = right} + {left <> right}.
Proof.
  decide equality; try apply Nat.eq_dec; apply list_eq_dec;
    first [apply Nat.eq_dec | apply phlo_class_record_eq_dec].
Defined.

Definition compatible_acquisition_terms policy original :=
  if phlo_schedule_record_eq_dec (schedule_at_offer original 0) (schedule_at_offer policy 0)
  then Some original else None.

Theorem compatible_acquisition_keeps_original : forall policy original accepted,
  compatible_acquisition_terms policy original = Some accepted -> accepted = original.
Proof.
  intros. unfold compatible_acquisition_terms in H.
  destruct (phlo_schedule_record_eq_dec _ _); inversion H; reflexivity.
Qed.

Theorem compatible_acquisition_iff_complete_policy : forall policy original,
  compatible_acquisition_terms policy original = Some original <->
  schedule_policy_fields original = schedule_policy_fields policy.
Proof.
  intros. unfold compatible_acquisition_terms.
  destruct (phlo_schedule_record_eq_dec _ _) as [same|different].
  - split; auto. intros _.
    apply (f_equal schedule_policy_fields) in same.
    repeat rewrite offered_schedule_keeps_complete_policy in same. exact same.
  - split; [discriminate|]. intros same. exfalso. apply different.
    apply policy_and_offer_determine_complete_schedule.
    + now rewrite offered_schedule_keeps_complete_policy.
    + reflexivity.
Qed.

Theorem compatible_acquisition_does_not_reprice : forall policy original accepted,
  compatible_acquisition_terms policy original = Some accepted ->
  schedule_record_price accepted = schedule_record_price original.
Proof.
  intros. now rewrite (compatible_acquisition_keeps_original _ _ _ H).
Qed.

Theorem acquisition_compatibility_is_independent_of_purchase_price : forall policy original price,
  compatible_acquisition_terms policy (schedule_at_offer original price) =
  option_map (fun accepted => schedule_at_offer accepted price)
    (compatible_acquisition_terms policy original).
Proof.
  intros. unfold compatible_acquisition_terms.
  rewrite genesis_policy_canonical_price_is_not_an_offer with (second := schedule_record_price original).
  assert (schedule_at_offer original (schedule_record_price original) = original) as same
    by (destruct original; reflexivity).
  rewrite same. destruct (phlo_schedule_record_eq_dec _ _); reflexivity.
Qed.

Theorem incompatible_acquisition_cannot_supply_a_checked_record : forall policy original,
  schedule_policy_fields original <> schedule_policy_fields policy ->
  compatible_acquisition_terms policy original = None.
Proof.
  intros policy original different.
  destruct (compatible_acquisition_terms policy original) as [accepted|] eqn:checked; auto.
  pose proof (compatible_acquisition_keeps_original _ _ _ checked) as same. subst accepted.
  apply compatible_acquisition_iff_complete_policy in checked. contradiction.
Qed.

Print Assumptions phlo_class_record_eq_dec.
Print Assumptions phlo_schedule_record_eq_dec.
Print Assumptions compatible_acquisition_keeps_original.
Print Assumptions compatible_acquisition_iff_complete_policy.
Print Assumptions compatible_acquisition_does_not_reprice.
Print Assumptions acquisition_compatibility_is_independent_of_purchase_price.
Print Assumptions incompatible_acquisition_cannot_supply_a_checked_record.
