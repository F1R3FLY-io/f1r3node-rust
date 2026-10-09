(* D-F2 (DR-118): the reducer's measurement of an introduction.

   An introduction is the produce of a datum or the consume of a
   continuation. Its byte charge is the prost length of its channels, its
   patterns and its value, plus fixed event bytes. Before D-F2 the observer
   walked the whole value to compute that length. The reducer had computed
   the lengths of the datum's terms, of the body and of the guard moments
   earlier, for the phlo charge of the substitution, and then dropped them.

   D-F2 registers that measurement with the introduction's authority. The
   observer composes the charge from the measured lengths and walks only the
   parts that the reducer did not measure: the channels, the patterns, the
   seal, the stack and the authority. This module proves three results.
   - Part 1 (framing): the composed length equals the walked prost length on
     every shape that the reducer builds, and the composition asks for the
     walk on every other shape. A negative control shows that a framing
     without prost's default-field rule counts bytes that prost omits.
   - Part 2 (read coverage): the premeasured observation reserves what it
     reads, and its charge does not depend on the measured sizes. Its charge
     is no greater than the walked charge when its field reads cost no more
     than the root block of the value. A negative control shows that the
     walked charge grows with the measured body.
   - Part 3 (registry): two different measurements of one identity conflict
     in either order, and otherwise the first record stays. Updates of
     different identities commute. A negative control shows that a registry
     without the measurement check accepts two lengths for one identity.

   The model counts lengths as natural numbers. The Rust property tests in
   byte_accounting.rs and observation_construction.rs check the
   correspondence with prost's derived lengths. The section variables of
   Part 3 become premises of its theorems, so no axiom is used. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia.
From CostAccountedRho Require ObservationReadCoverage.
Import ListNotations.

Module Coverage := ObservationReadCoverage.

(** * Part 1: framing *)

(** The length of a LEB128 varint: one byte for each started group of seven
    bits, and at least one byte. The fuel [n] suffices, because
    [n / 128 < n] when [n >= 128]. *)
Fixpoint varint_len_fuel (fuel n : nat) : nat :=
  match fuel with
  | 0 => 1
  | S fuel' => if n <? 128 then 1 else S (varint_len_fuel fuel' (n / 128))
  end.

Definition varint_len (n : nat) : nat := varint_len_fuel n n.

Theorem varint_len_boundaries :
  varint_len 0 = 1 /\ varint_len 127 = 1 /\ varint_len 128 = 2 /\
  varint_len (128 * 128 - 1) = 2 /\ varint_len (128 * 128) = 3.
Proof. repeat split; vm_compute; reflexivity. Qed.

(** The length of a field key. Every introduction field has a tag below 16,
    so its key has one byte. *)
Definition key_len (tag : nat) : nat := if tag <? 16 then 1 else 2.

(** A length-delimited field with [len] content bytes. *)
Definition field (tag len : nat) : nat := key_len tag + varint_len len + len.

(** prost omits an empty bytes field and an absent optional message. *)
Definition bytes_field (tag n : nat) : nat := if n =? 0 then 0 else field tag n.

Definition opt_field (tag : nat) (len : option nat) : nat :=
  match len with Some len => field tag len | None => 0 end.

Definition repeated (tag : nat) (lens : list nat) : nat :=
  fold_right (fun len total => field tag len + total) 0 lens.

(** A datum ([ListParWithRandom]) by the lengths of its fields. *)
Record datum := {
  pars : list nat;
  random_state : nat;
  seal : option nat;
  stack : option nat
}.

Definition datum_len (d : datum) : nat :=
  repeated 1 (pars d) + bytes_field 2 (random_state d)
  + opt_field 3 (seal d) + opt_field 4 (stack d).

(** The body of a continuation: a [ParWithRandom] (its optional body term and
    its random state), a body reference (by its encoded length), or none. *)
Inductive tagged :=
| ParBody (term : option nat) (state : nat)
| BodyRef (encoded : nat)
| NoBody.

Definition par_with_random_len (term : option nat) (state : nat) : nat :=
  opt_field 1 term + bytes_field 2 state.

Definition tagged_len (t : tagged) : nat :=
  match t with
  | ParBody term state => field 1 (par_with_random_len term state)
  | BodyRef encoded => key_len 2 + encoded
  | NoBody => 0
  end.

(** A continuation ([TaggedContinuation]) by the lengths of its fields. *)
Record continuation := {
  tagged_body : tagged;
  guard_len : option nat;
  authority_len : option nat
}.

Definition continuation_len (k : continuation) : nat :=
  tagged_len (tagged_body k) + opt_field 3 (guard_len k) + opt_field 4 (authority_len k).

Definition hash_bytes : nat := 32.

Definition sum (lens : list nat) : nat := fold_right Nat.add 0 lens.

(** The walked charges: [produce_introduction_charge] and
    [consume_introduction_charge]. *)
Definition produce_bytes (channel : nat) (d : datum) : nat :=
  channel + datum_len d + 2 * hash_bytes.

Definition consume_bytes (channels patterns : list nat) (k : continuation) : nat :=
  sum channels + sum patterns + continuation_len k
  + (hash_bytes + length channels * hash_bytes).

(** The reducer's measurements. [eval_send] frames the length of each
    substituted datum term. [eval_receive] composes the length of the
    continuation's [ParWithRandom] from the substituted body's length, and it
    measures the guard. *)
Definition measured_pars (d : datum) : nat := repeated 1 (pars d).

Definition par_with_random_bytes (term state : nat) : nat :=
  field 1 term + bytes_field 2 state.

Lemma par_with_random_bytes_eq : forall term state,
  par_with_random_bytes term state = par_with_random_len (Some term) state.
Proof. intros term state. reflexivity. Qed.

(** The premeasured charges: [produce_introduction_charge_premeasured] and
    [consume_introduction_charge_premeasured]. *)
Definition produce_bytes_premeasured (channel : nat) (d : datum) (measured : nat) : nat :=
  channel
  + (measured + bytes_field 2 (random_state d) + opt_field 3 (seal d) + opt_field 4 (stack d))
  + 2 * hash_bytes.

Definition same_presence (measured actual : option nat) : bool :=
  match measured, actual with
  | Some _, Some _ => true
  | None, None => true
  | _, _ => false
  end.

Definition consume_bytes_premeasured
    (channels patterns : list nat) (k : continuation) (body : nat) (guard : option nat)
    : option nat :=
  match tagged_body k with
  | ParBody _ _ =>
      if same_presence guard (guard_len k)
      then Some (sum channels + sum patterns
                 + (field 1 body + opt_field 3 guard + opt_field 4 (authority_len k))
                 + (hash_bytes + length channels * hash_bytes))
      else None
  | _ => None
  end.

Theorem premeasured_datum_eq_walked : forall channel d,
  produce_bytes_premeasured channel d (measured_pars d) = produce_bytes channel d.
Proof.
  intros channel d.
  unfold produce_bytes_premeasured, produce_bytes, datum_len, measured_pars. lia.
Qed.

Theorem premeasured_continuation_eq_walked : forall channels patterns k term state,
  tagged_body k = ParBody term state ->
  consume_bytes_premeasured channels patterns k (par_with_random_len term state) (guard_len k)
    = Some (consume_bytes channels patterns k).
Proof.
  intros channels patterns k term state shape.
  unfold consume_bytes_premeasured, consume_bytes, continuation_len.
  rewrite shape. unfold tagged_len.
  destruct (guard_len k) as [guard |]; simpl; f_equal; lia.
Qed.

Theorem premeasured_continuation_none_iff_mismatch : forall channels patterns k body guard,
  consume_bytes_premeasured channels patterns k body guard = None <->
  (forall term state, tagged_body k <> ParBody term state)
  \/ same_presence guard (guard_len k) = false.
Proof.
  intros channels patterns k body guard. unfold consume_bytes_premeasured.
  destruct (tagged_body k) as [term state | encoded |] eqn:shape.
  - destruct (same_presence guard (guard_len k)) eqn:presence.
    + split; [discriminate |].
      intros [not_body | mismatch].
      * exfalso. exact (not_body term state eq_refl).
      * discriminate.
    + split; intros _; [right; reflexivity | reflexivity].
  - split; intros _; [left; intros term state; discriminate | reflexivity].
  - split; intros _; [left; intros term state; discriminate | reflexivity].
Qed.

(** The body measurement of [eval_receive] is the length of the
    [ParWithRandom] that it builds with [Some] body. *)
Theorem premeasured_observation_eq_walked : forall channel d channels patterns k term state,
  tagged_body k = ParBody (Some term) state ->
  produce_bytes_premeasured channel d (measured_pars d) = produce_bytes channel d
  /\ consume_bytes_premeasured channels patterns k
       (par_with_random_bytes term state) (guard_len k)
     = Some (consume_bytes channels patterns k).
Proof.
  intros channel d channels patterns k term state shape. split.
  - apply premeasured_datum_eq_walked.
  - rewrite par_with_random_bytes_eq.
    exact (premeasured_continuation_eq_walked channels patterns k (Some term) state shape).
Qed.

(** Negative control: a framing that also frames an empty bytes field. *)
Definition naive_datum_len (d : datum) : nat :=
  repeated 1 (pars d) + field 2 (random_state d)
  + opt_field 3 (seal d) + opt_field 4 (stack d).

Theorem naive_framing_differs_from_prost : exists d, naive_datum_len d <> datum_len d.
Proof.
  exists {| pars := []; random_state := 0; seal := None; stack := None |}.
  vm_compute. discriminate.
Qed.

(** * Part 2: read coverage *)

(** The walk sizes of one introduction observation:
    - the self-metered work of its identity;
    - the inspection of its channels and of its patterns (zero for a
      produce);
    - the root block of its value (the datum or the continuation);
    - the walk of the measured part of the value (the datum's terms, or the
      body and the guard);
    - the walk of the unmeasured part (the seal and the stack, or the
      authority);
    - the inline field reads of the premeasured observation. *)
Record walks := {
  identity_work : nat;
  channel_walk : nat;
  pattern_walk : nat;
  value_root : nat;
  measured_walk : nat;
  unmeasured_walk : nat;
  field_reads : nat
}.

(** A block inspection of the whole value walks its root, its measured part
    and its unmeasured part. *)
Definition value_walk (w : walks) : nat := value_root w + measured_walk w + unmeasured_walk w.

(** The walked observation: the self-metered identity, the inspections, and
    then the unmetered prost lengths that they prepay. *)
Definition walked_trace (w : walks) : list Coverage.event :=
  Coverage.self_metered (identity_work w)
  ++ map Coverage.Reserve [channel_walk w; pattern_walk w; value_walk w]
  ++ map Coverage.Read [channel_walk w; pattern_walk w; value_walk w].

(** The premeasured observation reserves its field reads and inspects only
    the unmeasured parts. *)
Definition premeasured_trace (w : walks) : list Coverage.event :=
  Coverage.self_metered (identity_work w)
  ++ map Coverage.Reserve [field_reads w; channel_walk w; pattern_walk w; unmeasured_walk w]
  ++ map Coverage.Read [field_reads w; channel_walk w; pattern_walk w; unmeasured_walk w].

(** A continuation whose shape differs from its measurement: the shape check
    reads its fields, and then the observation walks the whole value. *)
Definition fallback_trace (w : walks) : list Coverage.event :=
  Coverage.self_metered (identity_work w)
  ++ Coverage.self_metered (field_reads w)
  ++ map Coverage.Reserve [channel_walk w; pattern_walk w; value_walk w]
  ++ map Coverage.Read [channel_walk w; pattern_walk w; value_walk w].

Lemma inspect_then_read_covered : forall slack amounts,
  Coverage.covered_from slack (map Coverage.Reserve amounts ++ map Coverage.Read amounts).
Proof.
  intros slack amounts.
  apply Coverage.covered_from_app.
  - apply Coverage.reserves_covered.
  - rewrite Coverage.reserve_reads. lia.
  - rewrite Coverage.reserve_reads, Coverage.reserve_total, Nat.sub_0_r.
    apply Coverage.reads_covered. lia.
Qed.

Lemma self_metered_then_covered : forall slack units rest,
  Coverage.covered_from slack rest ->
  Coverage.covered_from slack (Coverage.self_metered units ++ rest).
Proof.
  intros slack units rest covered_rest.
  apply Coverage.covered_from_app.
  - apply Coverage.self_metered_covered.
  - rewrite Coverage.self_metered_balanced. lia.
  - rewrite Coverage.self_metered_balanced, Nat.add_sub. exact covered_rest.
Qed.

Theorem walked_trace_covered : forall w, Coverage.covered (walked_trace w).
Proof.
  intros w. unfold Coverage.covered, walked_trace.
  apply self_metered_then_covered, inspect_then_read_covered.
Qed.

Theorem premeasured_trace_covered : forall w, Coverage.covered (premeasured_trace w).
Proof.
  intros w. unfold Coverage.covered, premeasured_trace.
  apply self_metered_then_covered, inspect_then_read_covered.
Qed.

Theorem fallback_trace_covered : forall w, Coverage.covered (fallback_trace w).
Proof.
  intros w. unfold Coverage.covered, fallback_trace.
  apply self_metered_then_covered, self_metered_then_covered, inspect_then_read_covered.
Qed.

Definition with_measured (w : walks) (measured : nat) : walks :=
  {| identity_work := identity_work w;
     channel_walk := channel_walk w;
     pattern_walk := pattern_walk w;
     value_root := value_root w;
     measured_walk := measured;
     unmeasured_walk := unmeasured_walk w;
     field_reads := field_reads w |}.

Theorem premeasured_charge_independent_of_body : forall w measured,
  premeasured_trace (with_measured w measured) = premeasured_trace w.
Proof. intros w measured. reflexivity. Qed.

Theorem premeasured_reserves_le_walked : forall w,
  field_reads w <= value_root w ->
  Coverage.total Coverage.reserved_units (premeasured_trace w)
    <= Coverage.total Coverage.reserved_units (walked_trace w).
Proof.
  intros w fits.
  unfold premeasured_trace, walked_trace, value_walk, Coverage.self_metered, Coverage.total.
  simpl. lia.
Qed.

(** Negative control: the walked charge grows by every extra byte of the
    measured part, while the premeasured charge stays equal. *)
Theorem walked_charge_grows_with_premeasured_body : forall w extra,
  Coverage.total Coverage.reserved_units (walked_trace (with_measured w (measured_walk w + extra)))
    = Coverage.total Coverage.reserved_units (walked_trace w) + extra
  /\ premeasured_trace (with_measured w (measured_walk w + extra)) = premeasured_trace w.
Proof.
  intros w extra. split.
  - unfold walked_trace, value_walk, Coverage.self_metered, Coverage.total. simpl. lia.
  - reflexivity.
Qed.

(** * Part 3: the registry *)

Section Registry.

Variable authority measurement key : Type.
Variable authority_eq_dec : forall left right : authority, {left = right} + {left <> right}.
Variable measurement_eq_dec : forall left right : measurement, {left = right} + {left <> right}.
Variable key_eq_dec : forall left right : key, {left = right} + {left <> right}.

Definition record : Type := (authority * option measurement)%type.

Inductive registration := Accepted (r : record) | Conflict.

Definition agree (recorded new : option measurement) : bool :=
  match recorded, new with
  | Some recorded_value, Some new_value =>
      if measurement_eq_dec recorded_value new_value then true else false
  | _, _ => true
  end.

(** [register_introduction]: a repeated registration must have the same
    authority and an agreeing measurement, and the first record stays. *)
Definition register (existing : option record) (a : authority) (m : option measurement)
    : registration :=
  match existing with
  | None => Accepted (a, m)
  | Some (recorded_authority, recorded) =>
      if authority_eq_dec recorded_authority a
      then if agree recorded m then Accepted (recorded_authority, recorded) else Conflict
      else Conflict
  end.

Theorem registry_conflict_rejects_in_both_orders : forall a first second,
  first <> second ->
  register (Some (a, Some first)) a (Some second) = Conflict
  /\ register (Some (a, Some second)) a (Some first) = Conflict.
Proof.
  intros a first second different. unfold register, agree.
  destruct (authority_eq_dec a a) as [_ | absurd]; [| contradiction absurd; reflexivity].
  destruct (measurement_eq_dec first second) as [same | _]; [contradiction |].
  destruct (measurement_eq_dec second first) as [same | _];
    [exfalso; apply different; symmetry; exact same |].
  split; reflexivity.
Qed.

Theorem registry_equal_measurements_accepted : forall a m,
  register (Some (a, Some m)) a (Some m) = Accepted (a, Some m).
Proof.
  intros a m. unfold register, agree.
  destruct (authority_eq_dec a a) as [_ | absurd]; [| contradiction absurd; reflexivity].
  destruct (measurement_eq_dec m m) as [_ | absurd]; [reflexivity | contradiction absurd; reflexivity].
Qed.

(** A later registration without a measurement keeps the measurement: this
    covers the peek re-produce, which passes none. *)
Theorem registry_first_measurement_persists : forall a m,
  register (Some (a, Some m)) a None = Accepted (a, Some m).
Proof.
  intros a m. unfold register, agree.
  destruct (authority_eq_dec a a) as [_ | absurd]; [reflexivity | contradiction absurd; reflexivity].
Qed.

(** A record without a measurement keeps none, so its observer walks. *)
Theorem registry_unmeasured_entry_stays_unmeasured : forall a new,
  register (Some (a, None)) a new = Accepted (a, None).
Proof.
  intros a new. unfold register, agree.
  destruct (authority_eq_dec a a) as [_ | absurd]; [| contradiction absurd; reflexivity].
  destruct new; reflexivity.
Qed.

Definition registry : Type := key -> option record.

Definition set (reg : registry) (k : key) (r : record) : registry :=
  fun k' => if key_eq_dec k k' then Some r else reg k'.

Definition update (reg : registry) (k : key) (a : authority) (m : option measurement)
    : option registry :=
  match register (reg k) a m with
  | Accepted r => Some (set reg k r)
  | Conflict => None
  end.

Definition then_update (reg : option registry) (k : key) (a : authority)
    (m : option measurement) : option registry :=
  match reg with Some reg => update reg k a m | None => None end.

(** Two outcomes are equal when both conflict, or when both registries map
    every key to the same entry. The pointwise form needs no functional
    extensionality. *)
Definition same_registry (first second : option registry) : Prop :=
  match first, second with
  | Some first, Some second => forall k, first k = second k
  | None, None => True
  | _, _ => False
  end.

Lemma set_other : forall reg k r k', k <> k' -> set reg k r k' = reg k'.
Proof.
  intros reg k r k' distinct. unfold set.
  destruct (key_eq_dec k k') as [same | _]; [contradiction | reflexivity].
Qed.

Lemma update_accepted : forall reg k a m r,
  register (reg k) a m = Accepted r -> update reg k a m = Some (set reg k r).
Proof. intros reg k a m r accepted. unfold update. rewrite accepted. reflexivity. Qed.

Lemma update_conflict : forall reg k a m,
  register (reg k) a m = Conflict -> update reg k a m = None.
Proof. intros reg k a m conflict. unfold update. rewrite conflict. reflexivity. Qed.

Theorem registry_updates_on_distinct_identities_commute : forall reg k1 a1 m1 k2 a2 m2,
  k1 <> k2 ->
  same_registry (then_update (update reg k1 a1 m1) k2 a2 m2)
                (then_update (update reg k2 a2 m2) k1 a1 m1).
Proof.
  intros reg k1 a1 m1 k2 a2 m2 distinct.
  assert (distinct' : k2 <> k1) by (intros same; apply distinct; symmetry; exact same).
  destruct (register (reg k1) a1 m1) as [r1 |] eqn:first;
    destruct (register (reg k2) a2 m2) as [r2 |] eqn:second.
  - rewrite (update_accepted _ _ _ _ _ first), (update_accepted _ _ _ _ _ second).
    cbn [then_update].
    rewrite (update_accepted (set reg k1 r1) k2 a2 m2 r2)
      by (rewrite (set_other reg k1 r1 k2 distinct); exact second).
    rewrite (update_accepted (set reg k2 r2) k1 a1 m1 r1)
      by (rewrite (set_other reg k2 r2 k1 distinct'); exact first).
    cbn [same_registry]. intros k. unfold set.
    destruct (key_eq_dec k2 k) as [at_second | not_second];
      destruct (key_eq_dec k1 k) as [at_first | not_first]; try reflexivity.
    exfalso. apply distinct. rewrite at_first, at_second. reflexivity.
  - rewrite (update_accepted _ _ _ _ _ first), (update_conflict _ _ _ _ second).
    cbn [then_update].
    rewrite (update_conflict (set reg k1 r1) k2 a2 m2)
      by (rewrite (set_other reg k1 r1 k2 distinct); exact second).
    exact I.
  - rewrite (update_conflict _ _ _ _ first), (update_accepted _ _ _ _ _ second).
    cbn [then_update].
    rewrite (update_conflict (set reg k2 r2) k1 a1 m1)
      by (rewrite (set_other reg k2 r2 k1 distinct'); exact first).
    exact I.
  - rewrite (update_conflict _ _ _ _ first), (update_conflict _ _ _ _ second).
    exact I.
Qed.

(** Negative control: a registration that checks only the authority. *)
Definition unchecked_register (existing : option record) (a : authority)
    (m : option measurement) : registration :=
  match existing with
  | None => Accepted (a, m)
  | Some (recorded_authority, recorded) =>
      if authority_eq_dec recorded_authority a
      then Accepted (recorded_authority, recorded)
      else Conflict
  end.

End Registry.

Theorem unchecked_registry_accepts_two_lengths :
  unchecked_register nat nat Nat.eq_dec (Some (0, Some 1)) 0 (Some 2)
    = Accepted nat nat (0, Some 1)
  /\ register nat nat Nat.eq_dec Nat.eq_dec (Some (0, Some 1)) 0 (Some 2)
    = Conflict nat nat.
Proof. split; reflexivity. Qed.

Print Assumptions varint_len_boundaries.
Print Assumptions premeasured_datum_eq_walked.
Print Assumptions premeasured_continuation_eq_walked.
Print Assumptions premeasured_continuation_none_iff_mismatch.
Print Assumptions premeasured_observation_eq_walked.
Print Assumptions naive_framing_differs_from_prost.
Print Assumptions walked_trace_covered.
Print Assumptions premeasured_trace_covered.
Print Assumptions fallback_trace_covered.
Print Assumptions premeasured_charge_independent_of_body.
Print Assumptions premeasured_reserves_le_walked.
Print Assumptions walked_charge_grows_with_premeasured_body.
Print Assumptions registry_conflict_rejects_in_both_orders.
Print Assumptions registry_equal_measurements_accepted.
Print Assumptions registry_first_measurement_persists.
Print Assumptions registry_unmeasured_entry_stays_unmeasured.
Print Assumptions registry_updates_on_distinct_identities_commute.
Print Assumptions unchecked_registry_accepts_two_lengths.
