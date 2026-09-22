From Stdlib Require Import List Arith Bool.
Import ListNotations.
Set Implicit Arguments.

Module EpmPayloadRepresentation.

Definition bindings (V : Type) := list (nat * V).
Definition erase {V} key (items : bindings V) : bindings V :=
  filter (fun item => negb (Nat.eqb (fst item) key)) items.
Definition put {V} key (value : V) items := (key, value) :: erase key items.
Definition denote {V W} (f : V -> W) (items : bindings V) : bindings W :=
  map (fun item => (fst item, f (snd item))) items.

Record state (V : Type) := State {
  topology : list nat;
  contents : bindings V;
  failure : option nat
}.
Arguments State {V}.
Arguments topology {V}.
Arguments contents {V}.
Arguments failure {V}.

Inductive action (V : Type) :=
| Install (key : nat) (value : V)
| Remove (key : nat)
| Refuse (code : nat).
Arguments Install {V}.
Arguments Remove {V}.
Arguments Refuse {V}.

Definition step {V} (current : state V) (op : action V) : state V :=
  match failure current with
  | Some _ => current
  | None => match op with
    | Install key value => State (topology current) (put key value (contents current)) None
    | Remove key => State (topology current) (erase key (contents current)) None
    | Refuse code => State (topology current) [] (Some code)
    end
  end.

Fixpoint run {V} (ops : list (action V)) (current : state V) :=
  match ops with [] => current | op :: tail => run tail (step current op) end.

Definition map_action {V W} (f : V -> W) (op : action V) : action W :=
  match op with
  | Install key value => Install key (f value)
  | Remove key => Remove key
  | Refuse code => Refuse code
  end.
Definition map_state {V W} (f : V -> W) (current : state V) : state W :=
  State (topology current) (denote f (contents current)) (failure current).
Definition publish {V} (current : state V) : option (bindings V) :=
  match failure current with None => Some (contents current) | Some _ => None end.

Theorem erase_commutes_with_denotation {V W} (f : V -> W) key items :
  denote f (erase key items) = erase key (denote f items).
Proof.
  unfold denote, erase.
  induction items as [|[k v] tail IH]; cbn; [reflexivity|].
  destruct (negb (k =? key)); cbn; now rewrite IH.
Qed.

Theorem install_commutes_with_denotation {V W} (f : V -> W) key value items :
  denote f (put key value items) = put key (f value) (denote f items).
Proof.
  change ((key, f value) :: denote f (erase key items) =
    (key, f value) :: erase key (denote f items)).
  now rewrite erase_commutes_with_denotation.
Qed.

Local Opaque denote erase put.

Theorem each_writer_step_commutes {V W} (f : V -> W) current op :
  map_state f (step current op) = step (map_state f current) (map_action f op).
Proof.
  destruct current as [shape items error]; destruct error; [reflexivity|].
  destruct op; cbn; unfold map_state; cbn;
    try rewrite install_commutes_with_denotation;
    try rewrite erase_commutes_with_denotation; reflexivity.
Qed.

Theorem writer_trace_commutes {V W} (f : V -> W) ops current :
  map_state f (run ops current) = run (map (map_action f) ops) (map_state f current).
Proof.
  revert current. induction ops as [|op tail IH]; intros current; cbn; [reflexivity|].
  rewrite IH, each_writer_step_commutes. reflexivity.
Qed.

Theorem publication_commutes {V W} (f : V -> W) current :
  publish (map_state f current) = option_map (denote f) (publish current).
Proof. destruct current as [shape items [error|]]; reflexivity. Qed.

Theorem operations_preserve_topology {V} ops (current : state V) :
  topology (run ops current) = topology current.
Proof.
  revert current. induction ops as [|op tail IH]; intros current; cbn; [reflexivity|].
  rewrite IH. destruct current as [shape items [error|]]; [reflexivity|].
  destruct op; reflexivity.
Qed.

Theorem refusal_cannot_publish {V} ops shape (items : bindings V) code :
  publish (run ops (State shape items (Some code))) = None.
Proof.
  induction ops as [|op tail IH]; cbn; [reflexivity|]. exact IH.
Qed.

Local Transparent denote erase put.

Lemma erase_absent {V} key (items : bindings V) :
  Forall (fun item => fst item <> key) items -> erase key items = items.
Proof.
  intro absent. induction absent as [|[k v] tail different absent IH]; [reflexivity|].
  cbn in different. apply Nat.eqb_neq in different.
  unfold erase in *; cbn. now rewrite different; cbn; rewrite IH.
Qed.

Theorem scaffold_erasure_preserves_real_bindings {V} key (scaffold : V) before descendants :
  Forall (fun item => fst item <> key) before ->
  Forall (fun item => fst item <> key) descendants ->
  erase key (descendants ++ put key scaffold before) = descendants ++ before.
Proof.
  intros oldAbsent childAbsent. unfold put.
  rewrite (erase_absent oldAbsent).
  unfold erase at 1. rewrite filter_app.
  change (erase key descendants ++
    (if negb (key =? key) then (key, scaffold) :: erase key before
     else erase key before) = descendants ++ before).
  rewrite Nat.eqb_refl; cbn.
  now rewrite (erase_absent childAbsent), (erase_absent oldAbsent).
Qed.

Theorem equal_payloads_preserve_distinct_keys {V} (value : V) first second :
  first <> second ->
  put first value (put second value []) = [(first, value); (second, value)].
Proof.
  intro different. unfold put, erase; cbn.
  assert (second <> first) by congruence.
  apply Nat.eqb_neq in H. now rewrite H.
Qed.

Theorem scaffold_leak_mutant_is_observable {V} key (scaffold : V) :
  publish (State [] [(key, scaffold)] None) <> Some [].
Proof. discriminate. Qed.

Print Assumptions erase_commutes_with_denotation.
Print Assumptions install_commutes_with_denotation.
Print Assumptions each_writer_step_commutes.
Print Assumptions writer_trace_commutes.
Print Assumptions publication_commutes.
Print Assumptions operations_preserve_topology.
Print Assumptions refusal_cannot_publish.
Print Assumptions scaffold_erasure_preserves_real_bindings.
Print Assumptions equal_payloads_preserve_distinct_keys.
Print Assumptions scaffold_leak_mutant_is_observable.

End EpmPayloadRepresentation.
