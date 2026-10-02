From Stdlib Require Import Lists.List Bool.Bool Arith.PeanoNat.
Import ListNotations.

Inductive channel_shape :=
| Absent
| Receivers (persistent : bool) (joins_and_arities : list (nat * nat)).

Record registry_shape := {
  lookup_shape : channel_shape;
  random_insert_shape : channel_shape;
  signed_insert_shape : channel_shape;
  internal_v1_shape : channel_shape;
  public_v1_shape : channel_shape
}.

Definition forwarder := Receivers false [(1, 1)].
Definition internal_api := Receivers true [(1, 4); (1, 5); (1, 5); (1, 6)].
Definition public_api := Receivers true [(1, 2)].

Definition fresh (s : registry_shape) : bool :=
  match s with
  | {| lookup_shape := Absent;
       random_insert_shape := Absent;
       signed_insert_shape := Absent;
       internal_v1_shape := Absent;
       public_v1_shape := Absent |} => true
  | _ => false
  end.

Definition bootstrapped (s : registry_shape) : bool :=
  match s with
  | {| lookup_shape := Receivers false [(1, 1)];
       random_insert_shape := Receivers false [(1, 1)];
       signed_insert_shape := Receivers false [(1, 1)];
       internal_v1_shape := Receivers false [(1, 1)];
       public_v1_shape := Receivers false [(1, 1)] |} => true
  | _ => false
  end.

Definition installed (s : registry_shape) : bool :=
  match s with
  | {| lookup_shape := Receivers false [(1, 1)];
       random_insert_shape := Receivers false [(1, 1)];
       signed_insert_shape := Receivers false [(1, 1)];
       internal_v1_shape := Receivers true [(1, 4); (1, 5); (1, 5); (1, 6)];
       public_v1_shape := Receivers true [(1, 2)] |} => true
  | _ => false
  end.

Inductive startup_action := BootstrapAndInstall | Install | Reuse | Refuse.

Definition decide (s : registry_shape) : startup_action :=
  if fresh s then BootstrapAndInstall
  else if bootstrapped s then Install
  else if installed s then Reuse
  else Refuse.

Definition fresh_shape : registry_shape :=
  {| lookup_shape := Absent;
     random_insert_shape := Absent;
     signed_insert_shape := Absent;
     internal_v1_shape := Absent;
     public_v1_shape := Absent |}.

Definition bootstrap_shape : registry_shape :=
  {| lookup_shape := forwarder;
     random_insert_shape := forwarder;
     signed_insert_shape := forwarder;
     internal_v1_shape := forwarder;
     public_v1_shape := forwarder |}.

Definition installed_shape : registry_shape :=
  {| lookup_shape := forwarder;
     random_insert_shape := forwarder;
     signed_insert_shape := forwarder;
     internal_v1_shape := internal_api;
     public_v1_shape := public_api |}.

Theorem fresh_requires_bootstrap : decide fresh_shape = BootstrapAndInstall.
Proof. reflexivity. Qed.

Theorem partial_bootstrap_refused :
  decide {| lookup_shape := forwarder;
            random_insert_shape := forwarder;
            signed_insert_shape := forwarder;
            internal_v1_shape := forwarder;
            public_v1_shape := Absent |} = Refuse.
Proof. reflexivity. Qed.

Theorem complete_bootstrap_installs_without_duplicate_bootstrap :
  decide bootstrap_shape = Install.
Proof. reflexivity. Qed.

Theorem installed_registry_is_reused : decide installed_shape = Reuse.
Proof. reflexivity. Qed.

Theorem bootstrap_requires_all_channels_absent : forall s,
  decide s = BootstrapAndInstall ->
  lookup_shape s = Absent /\ random_insert_shape s = Absent /\
  signed_insert_shape s = Absent /\ internal_v1_shape s = Absent /\
  public_v1_shape s = Absent.
Proof.
  intros [lookup random signed internal public] H.
  unfold decide in H.
  destruct (fresh
    {| lookup_shape := lookup; random_insert_shape := random;
       signed_insert_shape := signed; internal_v1_shape := internal;
       public_v1_shape := public |}) eqn:Hfresh.
  - unfold fresh in Hfresh.
    destruct lookup, random, signed, internal, public;
      try discriminate; repeat split; reflexivity.
  - destruct (bootstrapped
      {| lookup_shape := lookup; random_insert_shape := random;
         signed_insert_shape := signed; internal_v1_shape := internal;
         public_v1_shape := public |}); try discriminate.
    destruct (installed
      {| lookup_shape := lookup; random_insert_shape := random;
         signed_insert_shape := signed; internal_v1_shape := internal;
         public_v1_shape := public |}); discriminate.
Qed.

Theorem installed_never_rebootstraps :
  decide installed_shape <> BootstrapAndInstall.
Proof. discriminate. Qed.

Theorem retry_after_success_is_reuse :
  decide installed_shape = Reuse /\ decide installed_shape <> Install.
Proof. split; [reflexivity | discriminate]. Qed.
