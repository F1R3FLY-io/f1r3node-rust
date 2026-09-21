From Stdlib Require Import List Bool Arith Lia.
Import ListNotations.
Set Implicit Arguments.

Module CollectionCollisionElision.

Section Ordering.
Context {A : Type}.
Variable before : A -> A -> bool.

Fixpoint insert_ordered (x : A) (xs : list A) : list A :=
  match xs with
  | [] => [x]
  | y :: ys => if before x y then x :: xs else y :: insert_ordered x ys
  end.

Fixpoint order (xs : list A) : list A :=
  match xs with [] => [] | x :: rest => insert_ordered x (order rest) end.
End Ordering.

Section Sets.
Context {A : Type}.
Variable equal before : A -> A -> bool.

Definition set_insert (resident : list A) (x : A) : list A :=
  if existsb (equal x) resident then resident else resident ++ [x].

Definition set_reference (xs : list A) : list A :=
  order before (fold_left set_insert xs []).

Definition set_elided (xs : list A) : list A :=
  match xs with [] => [] | [x] => [x] | _ => set_reference xs end.

Theorem set_empty_exact : set_reference [] = [].
Proof. reflexivity. Qed.

Theorem set_singleton_exact : forall x, set_reference [x] = [x].
Proof. reflexivity. Qed.

Theorem set_dispatch_refines : forall xs, set_elided xs = set_reference xs.
Proof. intros [|x [|y rest]]; reflexivity. Qed.

Theorem set_multi_entry_unchanged : forall x y rest,
  set_elided (x :: y :: rest) = set_reference (x :: y :: rest).
Proof. reflexivity. Qed.

Theorem set_observables_unchanged : forall (B : Type) (observe : list A -> B) xs,
  observe (set_elided xs) = observe (set_reference xs).
Proof. intros; rewrite set_dispatch_refines; reflexivity. Qed.

Theorem set_first_representative : forall first alias,
  equal alias first = true -> set_reference [first; alias] = [first].
Proof.
  intros first alias H.
  change (order before (if equal alias first || false then [first] else [first; alias]) = [first]).
  rewrite H; reflexivity.
Qed.
End Sets.

Section Maps.
Context {K V : Type}.
Variable equal : K -> K -> bool.
Variable greater : V -> V -> bool.
Variable before : (K * V) -> (K * V) -> bool.

Fixpoint map_insert (resident : list (K * V)) (incoming : K * V) : list (K * V) :=
  match resident with
  | [] => [incoming]
  | (key, value) :: rest =>
      if equal (fst incoming) key then
        (key, if greater (snd incoming) value then snd incoming else value) :: rest
      else (key, value) :: map_insert rest incoming
  end.

Definition map_reference (xs : list (K * V)) : list (K * V) :=
  order before (fold_left map_insert xs []).

Definition map_elided (xs : list (K * V)) : list (K * V) :=
  match xs with [] => [] | [entry] => [entry] | _ => map_reference xs end.

Theorem map_empty_exact : map_reference [] = [].
Proof. reflexivity. Qed.

Theorem map_singleton_exact : forall key value,
  map_reference [(key, value)] = [(key, value)].
Proof. reflexivity. Qed.

Theorem map_dispatch_refines : forall xs, map_elided xs = map_reference xs.
Proof. intros [|[k v] [|entry rest]]; reflexivity. Qed.

Theorem map_multi_entry_unchanged : forall x y rest,
  map_elided (x :: y :: rest) = map_reference (x :: y :: rest).
Proof. reflexivity. Qed.

Theorem map_observables_unchanged : forall (B : Type) (observe : list (K * V) -> B) xs,
  observe (map_elided xs) = observe (map_reference xs).
Proof. intros; rewrite map_dispatch_refines; reflexivity. Qed.

Theorem map_first_key_and_strict_winner : forall key alias old incoming,
  equal alias key = true ->
  map_reference [(key, old); (alias, incoming)] =
  [(key, if greater incoming old then incoming else old)].
Proof.
  intros key alias old incoming H.
  change (order before
    (if equal alias key then [(key, if greater incoming old then incoming else old)]
     else [(key, old); (alias, incoming)]) =
    [(key, if greater incoming old then incoming else old)]).
  rewrite H; reflexivity.
Qed.
End Maps.

Section Inspection.
Context {A : Type}.
Variable size : A -> nat.

Definition reference_identity_bytes (xs : list A) := fold_right Nat.add 0 (map size xs).
Definition elided_identity_bytes (xs : list A) :=
  match xs with [] => 0 | [_] => 0 | _ => reference_identity_bytes xs end.

Theorem empty_identity_work_zero : elided_identity_bytes [] = 0.
Proof. reflexivity. Qed.

Theorem singleton_identity_work_zero : forall x, elided_identity_bytes [x] = 0.
Proof. reflexivity. Qed.

Theorem singleton_removed_work_exact : forall x, reference_identity_bytes [x] = size x.
Proof. intros; cbn [reference_identity_bytes map fold_right]; lia. Qed.

Theorem multi_entry_identity_work_unchanged : forall x y rest,
  elided_identity_bytes (x :: y :: rest) = reference_identity_bytes (x :: y :: rest).
Proof. reflexivity. Qed.

Theorem identity_work_never_increases : forall xs,
  elided_identity_bytes xs <= reference_identity_bytes xs.
Proof. intros [|x [|y rest]]; cbn [elided_identity_bytes reference_identity_bytes]; lia. Qed.

Theorem nested_singleton_identity_work_zero : forall xs,
  fold_right Nat.add 0 (map (fun x => elided_identity_bytes [x]) xs) = 0.
Proof. induction xs; cbn [map elided_identity_bytes fold_right]; auto. Qed.
End Inspection.

Example rejecting_duplicate_set_dispatch_mutant :
  set_reference Nat.eqb Nat.leb [7; 7] <> [7; 7].
Proof. discriminate. Qed.

Example rejecting_map_winner_dispatch_mutant :
  map_reference Nat.eqb (fun incoming old => Nat.ltb old incoming)
    (fun p q => Nat.leb (fst p) (fst q))
    [(7, 10); (7, 30)] <> [(7, 10)].
Proof. discriminate. Qed.

Print Assumptions set_empty_exact.
Print Assumptions set_singleton_exact.
Print Assumptions set_dispatch_refines.
Print Assumptions set_multi_entry_unchanged.
Print Assumptions set_observables_unchanged.
Print Assumptions set_first_representative.
Print Assumptions map_empty_exact.
Print Assumptions map_singleton_exact.
Print Assumptions map_dispatch_refines.
Print Assumptions map_multi_entry_unchanged.
Print Assumptions map_observables_unchanged.
Print Assumptions map_first_key_and_strict_winner.
Print Assumptions empty_identity_work_zero.
Print Assumptions singleton_identity_work_zero.
Print Assumptions singleton_removed_work_exact.
Print Assumptions multi_entry_identity_work_unchanged.
Print Assumptions identity_work_never_increases.
Print Assumptions nested_singleton_identity_work_zero.
Print Assumptions rejecting_duplicate_set_dispatch_mutant.
Print Assumptions rejecting_map_winner_dispatch_mutant.

End CollectionCollisionElision.
