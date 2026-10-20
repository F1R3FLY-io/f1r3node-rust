(* D-F1 (DR-117): the canonical authority witness.

   A COMM charges its authority: the regions of its participants. Before
   D-F1 the native pipeline canonicalized those regions five times for each
   granted COMM:
   - V1 the merge of the participants (after a copy of every region);
   - V2 and V3 the instantiation of persistent regions, before and after the
     new identities;
   - V4 the native observation (replay) or the producer's reservation (play);
   - V5 the demand computation.
   Canonicalization checks each identity, validates each signature, sorts
   the regions by identity and rejects two different signatures with one
   identity. Its output is canonical, so V2, V4 and V5 return their input,
   and V3 only re-sorts by the new identities with the same conflict check.

   D-F1 keeps the output of V1 as a witness. It rekeys the witness without a
   new validation, and the observation, the reservation and the demand read
   the witness in place. This module proves that the value and the first
   error are unchanged, and that the charge does not grow.

   The model abstracts the signature type, the validation (which returns a
   canonical signature unchanged, or an error code), the lane of a signature
   and the identity map of the instantiation. The section variables become
   premises of every theorem, so no axiom is used. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Lia Sorting.Sorted.
Import ListNotations.

Inductive auth_error :=
| InvalidRegionIdentity
| MissingSignature
| SignatureError (code : nat)
| RegionIdentityConflict
| MissingAuthority.

Inductive outcome (A : Type) :=
| Ok (value : A)
| Err (error : auth_error).
Arguments Ok {A} value.
Arguments Err {A} error.

Definition bind {A B : Type} (result : outcome A) (next : A -> outcome B) : outcome B :=
  match result with
  | Ok value => next value
  | Err error => Err error
  end.

Section Witness.

Variable sig : Type.
Variable sig_eq_dec : forall left right : sig, {left = right} + {left <> right}.

(** The validation of a signature: [None] when it is canonical (and is then
    returned unchanged), else the error code. *)
Variable check : sig -> option nat.

(** A region: an identity ([None] when it does not have 32 bytes) and an
    optional signature. *)
Record region := {
  region_id : option nat;
  region_signature : option sig
}.

Definition entry := (nat * sig)%type.

Definition checked (item : entry) : Prop := check (snd item) = None.

Definition key_lt (left right : entry) : Prop := fst left < fst right.

Fixpoint find (key : nat) (entries : list entry) : option sig :=
  match entries with
  | [] => None
  | (key', value) :: rest => if Nat.eqb key key' then Some value else find key rest
  end.

Fixpoint place (key : nat) (value : sig) (entries : list entry) : list entry :=
  match entries with
  | [] => [(key, value)]
  | (key', value') :: rest =>
      if key <? key' then (key, value) :: entries else (key', value') :: place key value rest
  end.

(** [canonical_regions_metered] (authority.rs), in its check order. *)
Fixpoint canonical_from (entries : list entry) (regions : list region) : outcome (list entry) :=
  match regions with
  | [] => Ok entries
  | item :: rest =>
      match region_id item with
      | None => Err InvalidRegionIdentity
      | Some key =>
          match region_signature item with
          | None => Err MissingSignature
          | Some signature =>
              match check signature with
              | Some code => Err (SignatureError code)
              | None =>
                  match find key entries with
                  | Some existing =>
                      if sig_eq_dec existing signature then canonical_from entries rest
                      else Err RegionIdentityConflict
                  | None => canonical_from (place key signature entries) rest
                  end
              end
          end
      end
  end.

Definition canonical (regions : list region) : outcome (list entry) := canonical_from [] regions.

Definition regions_of (entries : list entry) : list region :=
  map (fun item => {| region_id := Some (fst item); region_signature := Some (snd item) |})
    entries.

Definition is_witness (entries : list entry) : Prop :=
  StronglySorted key_lt entries /\ Forall checked entries.

(** * Lemmas on the sorted association list *)

Lemma find_absent : forall key entries,
  Forall (fun item => fst item < key) entries -> find key entries = None.
Proof.
  intros key entries below. induction entries as [|[key' value] rest IH]; [reflexivity|].
  inversion below as [|? ? head tail]; subst. simpl in head |- *.
  destruct (Nat.eqb key key') eqn:same; [apply Nat.eqb_eq in same; lia|].
  now apply IH.
Qed.

Lemma place_at_end : forall key value entries,
  Forall (fun item => fst item < key) entries -> place key value entries = entries ++ [(key, value)].
Proof.
  intros key value entries below. induction entries as [|[key' value'] rest IH]; [reflexivity|].
  inversion below as [|? ? head tail]; subst. simpl in head |- *.
  destruct (key <? key') eqn:order; [apply Nat.ltb_lt in order; lia|].
  now rewrite IH.
Qed.

Lemma place_in : forall key value entries item,
  In item (place key value entries) -> item = (key, value) \/ In item entries.
Proof.
  intros key value entries item member.
  induction entries as [|[key' value'] rest IH]; simpl in member.
  - destruct member as [same|[]]. now left.
  - destruct (key <? key'); simpl in member.
    + destruct member as [same|member]; [now left|now right].
    + destruct member as [same|member]; [right; now left|].
      destruct (IH member) as [same|inner]; [now left|right; now right].
Qed.

Lemma place_sorted : forall key value entries,
  StronglySorted key_lt entries -> find key entries = None ->
  StronglySorted key_lt (place key value entries).
Proof.
  intros key value entries sorted absent.
  induction entries as [|[key' value'] rest IH]; simpl.
  - repeat constructor.
  - apply StronglySorted_inv in sorted as [sorted_rest above].
    simpl in absent. destruct (Nat.eqb key key') eqn:same; [discriminate|].
    apply Nat.eqb_neq in same.
    destruct (key <? key') eqn:order.
    + apply Nat.ltb_lt in order. constructor.
      * now constructor.
      * constructor; [exact order|].
        apply Forall_forall. intros item member.
        rewrite Forall_forall in above. specialize (above item member).
        unfold key_lt in *. simpl in *. lia.
    + apply Nat.ltb_ge in order. constructor.
      * now apply IH.
      * apply Forall_forall. intros item member.
        destruct (place_in _ _ _ _ member) as [same_item|inner].
        -- subst. unfold key_lt. simpl. lia.
        -- rewrite Forall_forall in above. now apply above.
Qed.

Lemma place_checked : forall key value entries,
  check value = None -> Forall checked entries -> Forall checked (place key value entries).
Proof.
  intros key value entries good all_checked.
  apply Forall_forall. intros item member.
  destruct (place_in _ _ _ _ member) as [same|inner].
  - subst. exact good.
  - rewrite Forall_forall in all_checked. now apply all_checked.
Qed.

Lemma canonical_from_witness : forall regions entries result,
  StronglySorted key_lt entries -> Forall checked entries ->
  canonical_from entries regions = Ok result -> is_witness result.
Proof.
  induction regions as [|item rest IH]; intros entries result sorted all_checked run; simpl in run.
  - injection run as <-. now split.
  - destruct (region_id item) as [key|]; [|discriminate].
    destruct (region_signature item) as [signature|]; [|discriminate].
    destruct (check signature) as [code|] eqn:good; [discriminate|].
    destruct (find key entries) as [existing|] eqn:found.
    + destruct (sig_eq_dec existing signature); [|discriminate].
      now apply (IH entries).
    + apply (IH (place key signature entries)); [| |exact run].
      * now apply place_sorted.
      * now apply place_checked.
Qed.

Lemma sorted_before : forall prefix item suffix,
  StronglySorted key_lt (prefix ++ item :: suffix) ->
  Forall (fun other => fst other < fst item) prefix.
Proof.
  induction prefix as [|head prefix IH]; intros item suffix sorted; [constructor|].
  simpl in sorted. apply StronglySorted_inv in sorted as [sorted above].
  constructor.
  - rewrite Forall_forall in above. apply above. apply in_or_app. right. now left.
  - now apply (IH item suffix).
Qed.

Lemma canonical_from_regions_of : forall entries prefix,
  StronglySorted key_lt (prefix ++ entries) -> Forall checked entries ->
  canonical_from prefix (regions_of entries) = Ok (prefix ++ entries).
Proof.
  induction entries as [|[key signature] rest IH]; intros prefix sorted all_checked; simpl.
  - now rewrite app_nil_r.
  - inversion all_checked as [|? ? good tail]; subst. unfold checked in good. simpl in good.
    rewrite good.
    pose proof (sorted_before prefix (key, signature) rest sorted) as below. simpl in below.
    rewrite (find_absent key prefix below).
    rewrite (place_at_end key signature prefix below).
    specialize (IH (prefix ++ [(key, signature)])).
    rewrite <- app_assoc in IH. simpl in IH.
    apply IH; [exact sorted|exact tail].
Qed.

(** * Theorems *)

(** A canonical output is a witness, and canonicalizing it again returns it
    unchanged. This is why V2, V4 and V5 can read the witness in place. *)
Theorem canonical_idempotent_on_outputs : forall regions result,
  canonical regions = Ok result -> is_witness result /\ canonical (regions_of result) = Ok result.
Proof.
  intros regions result run.
  assert (witness : is_witness result)
    by (apply (canonical_from_witness regions []); [constructor|constructor|exact run]).
  split; [exact witness|].
  destruct witness as [sorted all_checked].
  exact (canonical_from_regions_of result [] sorted all_checked).
Qed.

(** The merge read in place canonicalizes the same regions, in the same
    participant order, as the merge that copied them first. *)
Definition merge_copied (participants : list (list region)) : outcome (list entry) :=
  canonical (map (fun item => item) (concat participants)).

Definition merge_in_place (participants : list (list region)) : outcome (list entry) :=
  canonical (concat participants).

Theorem canonical_regions_eq_canonical_of_merged_copy : forall participants,
  merge_in_place participants = merge_copied participants.
Proof. intros participants. unfold merge_copied. now rewrite map_id. Qed.

(** The identity map of the instantiation: a fresh identity for a persistent
    region, the same identity otherwise. Collisions are allowed. *)
Variable new_key : nat -> nat.

Definition rekey_regions (entries : list entry) : list region :=
  map (fun item => {| region_id := Some (new_key (fst item)); region_signature := Some (snd item) |})
    entries.

(** [rekey_metered] (authority.rs): no validation, the same identity order
    and the same conflict check. *)
Fixpoint rekey_from (entries : list entry) (witness : list entry) : outcome (list entry) :=
  match witness with
  | [] => Ok entries
  | (key, signature) :: rest =>
      match find (new_key key) entries with
      | Some existing =>
          if sig_eq_dec existing signature then rekey_from entries rest
          else Err RegionIdentityConflict
      | None => rekey_from (place (new_key key) signature entries) rest
      end
  end.

Definition rekey (witness : list entry) : outcome (list entry) := rekey_from [] witness.

Lemma rekey_from_eq_canonical_from : forall witness entries,
  Forall checked witness -> rekey_from entries witness = canonical_from entries (rekey_regions witness).
Proof.
  induction witness as [|[key signature] rest IH]; intros entries all_checked; [reflexivity|].
  inversion all_checked as [|? ? good tail]; subst. unfold checked in good. simpl in good |- *.
  rewrite good.
  destruct (find (new_key key) entries) as [existing|].
  - destruct (sig_eq_dec existing signature); [now apply IH|reflexivity].
  - now apply IH.
Qed.

Theorem rekey_of_canonical_eq_canonical_of_rekey : forall witness,
  is_witness witness -> rekey witness = canonical (rekey_regions witness).
Proof.
  intros witness [_ all_checked]. exact (rekey_from_eq_canonical_from witness [] all_checked).
Qed.

(** The lane of a signature; [None] for the free signature. *)
Variable lane : sig -> option nat.

Definition demand (entries : list entry) : list nat :=
  flat_map (fun item => match lane (snd item) with Some value => [value] | None => [] end) entries.

Definition nonempty (entries : list entry) : outcome (list entry) :=
  match entries with
  | [] => Err MissingAuthority
  | _ => Ok entries
  end.

(** The legacy pipeline of a granted COMM: V1 to V5. *)
Definition legacy_pipeline (participants : list (list region)) : outcome (list entry * list nat) :=
  bind (merge_copied participants) (fun merged =>
  bind (canonical (regions_of merged)) (fun regions =>
  bind (canonical (rekey_regions regions)) (fun instantiated =>
  bind (canonical (regions_of instantiated)) (fun observed =>
  bind (nonempty observed) (fun authority =>
  bind (canonical (regions_of authority)) (fun demanded =>
  Ok (authority, demand demanded))))))).

(** The witness pipeline: V1, the rekey, the empty check and the demand. *)
Definition witness_pipeline (participants : list (list region)) : outcome (list entry * list nat) :=
  bind (merge_in_place participants) (fun merged =>
  bind (rekey merged) (fun instantiated =>
  bind (nonempty instantiated) (fun authority =>
  Ok (authority, demand authority)))).

Theorem witness_pipeline_eq_legacy_pipeline : forall participants,
  witness_pipeline participants = legacy_pipeline participants.
Proof.
  intros participants. unfold witness_pipeline, legacy_pipeline.
  rewrite <- canonical_regions_eq_canonical_of_merged_copy.
  destruct (merge_in_place participants) as [merged|error] eqn:merge_run; [|reflexivity].
  simpl.
  destruct (canonical_idempotent_on_outputs _ _ merge_run) as [merged_witness merged_again].
  rewrite merged_again. simpl.
  rewrite (rekey_of_canonical_eq_canonical_of_rekey merged merged_witness).
  destruct (canonical (rekey_regions merged)) as [instantiated|error] eqn:rekey_run;
    [|reflexivity].
  simpl.
  destruct (canonical_idempotent_on_outputs _ _ rekey_run) as [_ instantiated_again].
  rewrite instantiated_again. simpl.
  destruct (nonempty instantiated) as [authority|error] eqn:nonempty_run; simpl; [|reflexivity].
  assert (same : authority = instantiated).
  { destruct instantiated; simpl in nonempty_run; [discriminate|now injection nonempty_run]. }
  subst authority. rewrite instantiated_again. reflexivity.
Qed.

(** * Charges *)

(** The charge of a canonicalization of [regions]: the read of each region and
    the validation of each signature. *)
Variable c_region c_copy c_key c_lane : nat.
Variable c_check : sig -> nat.

Definition validation_charge (regions : list region) : nat :=
  fold_right
    (fun item total =>
      c_region + match region_signature item with Some value => c_check value | None => 0 end
      + total)
    0 regions.

(** The charge of a successful granted COMM with merged witness [merged] and
    instantiated authority [instantiated]: the legacy pipeline copies every
    region, validates at V1 to V5 and computes the lanes. *)
Definition legacy_charge (participants : list (list region)) (merged instantiated : list entry)
  : nat :=
  length (concat participants) * c_copy + validation_charge (concat participants)
  + validation_charge (regions_of merged) + length merged * c_key
  + validation_charge (rekey_regions merged)
  + validation_charge (regions_of instantiated) + validation_charge (regions_of instantiated)
  + length instantiated * c_lane.

(** The witness pipeline validates at V1, reads and rekeys each witness region
    once, and computes the lanes. *)
Definition witness_charge (participants : list (list region)) (merged instantiated : list entry)
  : nat :=
  validation_charge (concat participants) + length merged * (c_region + c_key)
  + length instantiated * c_lane.

Lemma region_reads_within_validation : forall entries,
  length entries * c_region <= validation_charge (regions_of entries).
Proof.
  induction entries as [|item rest IH]; simpl; [lia|].
  unfold validation_charge in *. simpl. lia.
Qed.

Theorem witness_charge_le_legacy_charge : forall participants merged instantiated,
  witness_charge participants merged instantiated <= legacy_charge participants merged instantiated.
Proof.
  intros participants merged instantiated. unfold witness_charge, legacy_charge.
  pose proof (region_reads_within_validation merged). lia.
Qed.

(** The number of validations of a canonicalization of [regions]. *)
Definition validations (regions : list region) : nat := length regions.

Definition legacy_validations (participants : list (list region))
    (merged instantiated : list entry) : nat :=
  validations (concat participants) + validations (regions_of merged)
  + validations (rekey_regions merged) + validations (regions_of instantiated)
  + validations (regions_of instantiated).

Definition witness_validations (participants : list (list region)) : nat :=
  validations (concat participants).

End Witness.

(** * Negative controls

    Each control shows a removed check or a changed rule on a concrete
    instance: natural-number signatures that are all canonical, with each
    signature as its own lane. *)

Definition nat_check (_ : nat) : option nat := None.

Definition one_region (key value : nat) : region nat :=
  {| region_id := Some key; region_signature := Some value |}.

(** C1: the legacy pipeline validates one region five times, and the
    witness pipeline once. *)
Theorem legacy_pipeline_validates_each_region_five_times :
  let participants := [[one_region 1 7]] in
  let merged := [(1, 7)] in
  canonical nat Nat.eq_dec nat_check (concat participants) = Ok merged /\
  legacy_validations nat (fun key => key) participants merged merged = 5 /\
  witness_validations nat participants = 1.
Proof. repeat split. Qed.

(** C2: a rekey without the conflict check accepts two different signatures
    that the instantiation maps to one identity. The canonicalization of the
    legacy instantiation rejects them, and so does [rekey]. *)
Fixpoint rekey_overwrite (new_key : nat -> nat) (entries : list (nat * nat))
    (witness : list (nat * nat)) : list (nat * nat) :=
  match witness with
  | [] => entries
  | (key, value) :: rest =>
      rekey_overwrite new_key
        ((new_key key, value) :: filter (fun item => negb (Nat.eqb (fst item) (new_key key))) entries)
        rest
  end.

Theorem rekey_without_conflict_check_accepts_a_collision :
  let witness := [(1, 7); (2, 8)] in
  let collapse := fun _ : nat => 0 in
  canonical nat Nat.eq_dec nat_check (rekey_regions nat collapse witness) = Err RegionIdentityConflict /\
  rekey nat Nat.eq_dec collapse witness = Err RegionIdentityConflict /\
  rekey_overwrite collapse [] witness = [(0, 8)].
Proof. repeat split. Qed.

(** C3: merging the participants in another order can report another first
    error, so the merge must keep the participant order. *)
Theorem reversed_merge_reports_another_error :
  let first := [{| region_id := None; region_signature := Some 1 |}] in
  let second := [{| region_id := Some 1; region_signature := None |}] in
  merge_in_place nat Nat.eq_dec nat_check [first; second] = Err InvalidRegionIdentity /\
  merge_in_place nat Nat.eq_dec nat_check [second; first] = Err MissingSignature.
Proof. repeat split. Qed.
