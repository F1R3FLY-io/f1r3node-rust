From Stdlib Require Import List NArith Arith Bool Lia.
Import ListNotations.
Open Scope N_scope.

Definition wire := list N.
Definition modulus (width : nat) := 256 ^ N.of_nat width.

Fixpoint little (width : nat) (value : N) : wire :=
  match width with
  | O => []
  | S rest => value mod 256 :: little rest (value / 256)
  end.

Fixpoint read_little (bytes : wire) : N :=
  match bytes with
  | [] => 0
  | head :: rest => head + 256 * read_little rest
  end.

Definition big width value := rev (little width value).
Definition read_big bytes := read_little (rev bytes).

Lemma little_length : forall width value, length (little width value) = width.
Proof. induction width; intros; simpl; auto. Qed.

Lemma big_length : forall width value, length (big width value) = width.
Proof. intros. unfold big. rewrite length_rev. apply little_length. Qed.

Lemma little_roundtrip : forall width value,
  value < modulus width -> read_little (little width value) = value.
Proof.
  induction width as [|width IH]; intros value H.
  - unfold modulus in H. simpl in H. simpl. lia.
  - unfold modulus in H. rewrite Nat2N.inj_succ, N.pow_succ_r in H by lia.
    change (value mod 256 + 256 * read_little (little width (value / 256)) = value).
    rewrite IH.
    + pose proof (N.div_mod value 256 ltac:(lia)). lia.
    + unfold modulus. apply N.Private_NDivProp.div_lt_upper_bound; lia.
Qed.

Theorem big_roundtrip : forall width value,
  value < modulus width -> read_big (big width value) = value.
Proof. intros. unfold read_big, big. rewrite rev_involutive. apply little_roundtrip. exact H. Qed.

Record codec (A : Type) := {
  emit : A -> wire;
  parse : wire -> option (A * wire);
  valid : A -> Prop;
  roundtrip : forall value suffix, valid value ->
    parse (emit value ++ suffix) = Some (value, suffix)
}.
Arguments emit {A} _ _.
Arguments parse {A} _ _.
Arguments valid {A} _ _.
Arguments roundtrip {A} _ _ _ _.

Definition uint (width : nat) : codec N.
Proof.
  refine {| emit := big width;
    parse := fun bytes => if Nat.leb width (length bytes)
      then Some (read_big (firstn width bytes), skipn width bytes) else None;
    valid := fun value => value < modulus width |}.
  intros value suffix H.
  assert (Nat.leb width (length (big width value ++ suffix)) = true) as Hsize.
  { apply Nat.leb_le. rewrite length_app, big_length. lia. }
  rewrite Hsize.
  assert (firstn width (big width value ++ suffix) = big width value) as Hfirst.
  { rewrite firstn_app, firstn_all2 by (rewrite big_length; lia).
    rewrite big_length, Nat.sub_diag. simpl. apply app_nil_r. }
  assert (skipn width (big width value ++ suffix) = suffix) as Hskip.
  { rewrite skipn_app, skipn_all2 by (rewrite big_length; lia).
    rewrite big_length, Nat.sub_diag. reflexivity. }
  rewrite Hfirst, Hskip, big_roundtrip by exact H. reflexivity.
Defined.

Definition pair_codec {A B} (left : codec A) (right : codec B) : codec (A * B).
Proof.
  refine {| emit := fun value => emit left (fst value) ++ emit right (snd value);
    parse := fun bytes => match parse left bytes with
      | Some (a, rest) => match parse right rest with
          | Some (b, tail) => Some ((a, b), tail) | None => None end
      | None => None end;
    valid := fun value => valid left (fst value) /\ valid right (snd value) |}.
  intros [a b] suffix [Ha Hb]. simpl. rewrite <- app_assoc.
  rewrite (roundtrip left a _ Ha), (roundtrip right b _ Hb). reflexivity.
Defined.

Fixpoint emit_rows {A} (item : codec A) (rows : list A) : wire :=
  match rows with [] => [] | head :: rest => emit item head ++ emit_rows item rest end.

Fixpoint parse_rows {A} (item : codec A) (count : nat) (bytes : wire)
  : option (list A * wire) :=
  match count with
  | O => Some ([], bytes)
  | S rest => match parse item bytes with
      | Some (head, tail) => match parse_rows item rest tail with
          | Some (rows, suffix) => Some (head :: rows, suffix) | None => None end
      | None => None end
  end.

Lemma rows_roundtrip : forall A (item : codec A) rows suffix,
  Forall (valid item) rows ->
  parse_rows item (length rows) (emit_rows item rows ++ suffix) = Some (rows, suffix).
Proof.
  intros A item rows. induction rows as [|head rest IH]; intros suffix H; simpl; [reflexivity|].
  inversion H; subst. rewrite <- app_assoc, roundtrip by assumption.
  rewrite IH by assumption. reflexivity.
Qed.

Definition fixed_rows {A} (count : nat) (item : codec A) : codec (list A).
Proof.
  refine {| emit := emit_rows item; parse := parse_rows item count;
    valid := fun rows => length rows = count /\ Forall (valid item) rows |}.
  intros rows suffix [Hlen Hvalid]. subst. apply rows_roundtrip. exact Hvalid.
Defined.

Definition list_codec {A} (item : codec A) : codec (list A).
Proof.
  refine {| emit := fun rows => big 8 (N.of_nat (length rows)) ++ emit_rows item rows;
    parse := fun bytes => match parse (uint 8) bytes with
      | Some (count, rest) => parse_rows item (N.to_nat count) rest | None => None end;
    valid := fun rows => N.of_nat (length rows) < modulus 8 /\ Forall (valid item) rows |}.
  intros rows suffix [Hlen Hvalid]. rewrite <- app_assoc.
  change (match parse (uint 8) (emit (uint 8) (N.of_nat (length rows)) ++ (emit_rows item rows ++ suffix)) with
    | Some (count, rest) => parse_rows item (N.to_nat count) rest | None => None end = Some (rows, suffix)).
  rewrite roundtrip by exact Hlen. rewrite Nat2N.id. apply rows_roundtrip. exact Hvalid.
Defined.

Definition blob : codec wire.
Proof.
  refine {| emit := fun bytes => big 8 (N.of_nat (length bytes)) ++ bytes;
    parse := fun bytes => match parse (uint 8) bytes with
      | Some (size, rest) => if Nat.leb (N.to_nat size) (length rest)
        then Some (firstn (N.to_nat size) rest, skipn (N.to_nat size) rest) else None
      | None => None end;
    valid := fun bytes => N.of_nat (length bytes) < modulus 8 /\ Forall (fun b => b < 256) bytes |}.
  intros bytes suffix [Hlen Hbytes]. rewrite <- app_assoc.
  change (match parse (uint 8) (emit (uint 8) (N.of_nat (length bytes)) ++ (bytes ++ suffix)) with
    | Some (size, rest) => if Nat.leb (N.to_nat size) (length rest)
        then Some (firstn (N.to_nat size) rest, skipn (N.to_nat size) rest) else None
    | None => None end = Some (bytes, suffix)).
  rewrite roundtrip by exact Hlen. rewrite Nat2N.id.
  assert (Nat.leb (length bytes) (length (bytes ++ suffix)) = true) as Hsize.
  { apply Nat.leb_le. rewrite length_app. lia. }
  rewrite Hsize, firstn_app, skipn_app, firstn_all, skipn_all, Nat.sub_diag.
  simpl. rewrite app_nil_r. reflexivity.
Defined.

Definition boolean : codec bool.
Proof.
  refine {| emit := fun (b : bool) => [if b then 1 else 0];
    parse := fun bytes => match bytes with
      | 0 :: rest => Some (false, rest) | 1 :: rest => Some (true, rest) | _ => None end;
    valid := fun _ => True |}.
  intros [] suffix _; reflexivity.
Defined.

Definition optional {A} (item : codec A) : codec (option A).
Proof.
  refine {| emit := fun value => match value with None => [0] | Some x => 1 :: emit item x end;
    parse := fun bytes => match bytes with
      | 0 :: rest => Some (None, rest)
      | 1 :: rest => match parse item rest with Some (x, tail) => Some (Some x, tail) | None => None end
      | _ => None end;
    valid := fun value => match value with None => True | Some x => valid item x end |}.
  intros [value|] suffix H; simpl; [rewrite roundtrip by exact H|]; reflexivity.
Defined.

Definition mapped {A B} (base : codec A) (pack : A -> B) (unpack : B -> A)
  (inverse : forall value, pack (unpack value) = value) : codec B.
Proof.
  refine {| emit := fun value => emit base (unpack value);
    parse := fun bytes => match parse base bytes with Some (value, rest) => Some (pack value, rest) | None => None end;
    valid := fun value => valid base (unpack value) |}.
  intros value suffix H. rewrite roundtrip by exact H. rewrite inverse. reflexivity.
Defined.

Theorem codec_injective : forall A (c : codec A) a b,
  valid c a -> valid c b -> emit c a = emit c b -> a = b.
Proof.
  intros A c a b Ha Hb Heq.
  pose proof (roundtrip c a [] Ha) as H.
  rewrite Heq, (roundtrip c b [] Hb) in H. inversion H. reflexivity.
Qed.

Definition literal (prefix : wire) : codec unit.
Proof.
  refine {| emit := fun _ => prefix;
    parse := fun bytes => if list_eq_dec N.eq_dec (firstn (length prefix) bytes) prefix
      then Some (tt, skipn (length prefix) bytes) else None;
    valid := fun _ => True |}.
  intros [] suffix _. rewrite firstn_app, skipn_app, firstn_all, skipn_all, Nat.sub_diag.
  simpl. rewrite app_nil_r. destruct (list_eq_dec N.eq_dec prefix prefix); congruence.
Defined.

Definition prefixed {A} (prefix : wire) (item : codec A) : codec A :=
  mapped (pair_codec (literal prefix) item) snd (fun value => (tt, value)) (fun _ => eq_refl).

From Stdlib Require Import Sorting.Sorted Sorting.Permutation.

Theorem ordered_permutation_unique : forall A (R : A -> A -> Prop),
  (forall x y : A, {x = y} + {x <> y}) ->
  (forall x y, R x y -> R y x -> False) ->
  forall left right, StronglySorted R left -> StronglySorted R right ->
  Permutation left right -> left = right.
Proof.
  intros A R decide asym left. induction left as [|head rest IH]; intros right Hl Hr Hp.
  - symmetry. apply Permutation_nil. exact Hp.
  - destruct right as [|other tail].
    + apply Permutation_sym, Permutation_nil in Hp. discriminate.
    + inversion Hl as [|? ? Hrest Hall]; subst.
      inversion Hr as [|? ? Htail Hother]; subst.
      destruct (decide head other) as [Heq|Hneq].
      * subst. f_equal. apply IH; [exact Hrest|exact Htail|].
        apply Permutation_cons_inv with (a := other). exact Hp.
      * assert (In head (other :: tail)) as Hinhead.
        { eapply Permutation_in; [exact Hp|]. left. reflexivity. }
        assert (In other (head :: rest)) as Hinother.
        { eapply Permutation_in; [apply Permutation_sym; exact Hp|]. left. reflexivity. }
        destruct Hinhead as [Heq|Hinhead]; [congruence|].
        destruct Hinother as [Heq|Hinother]; [congruence|].
        apply Forall_forall with (x := other) in Hall; [|exact Hinother].
        apply Forall_forall with (x := head) in Hother; [|exact Hinhead].
        exfalso. eapply asym; eauto.
Qed.
