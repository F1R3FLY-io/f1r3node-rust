From Stdlib Require Import Arith.PeanoNat.

Definition specification (fallback : nat) (file cli : option nat) : nat :=
  match cli, file with
  | Some value, _ => value
  | None, Some value => value
  | None, None => fallback
  end.

Definition load_file (fallback : nat) (file : option nat) : nat :=
  match file with Some value => value | None => fallback end.

Definition apply_cli (configured : nat) (cli : option nat) : nat :=
  match cli with Some value => value | None => configured end.

Definition resolve (fallback : nat) (file cli : option nat) : nat :=
  apply_cli (load_file fallback file) cli.

Theorem layered_resolution_matches_specification : forall fallback file cli,
  resolve fallback file cli = specification fallback file cli.
Proof. intros; destruct file, cli; reflexivity. Qed.

Theorem explicit_cli_wins : forall fallback file value,
  resolve fallback file (Some value) = value.
Proof. reflexivity. Qed.

Theorem absent_cli_preserves_file : forall fallback value,
  resolve fallback (Some value) None = value.
Proof. reflexivity. Qed.

Theorem absent_overrides_preserve_default : forall fallback,
  resolve fallback None None = fallback.
Proof. reflexivity. Qed.

Definition resolve_fields
  (fallback : nat -> nat) (file cli : nat -> option nat) (field : nat) : nat :=
  resolve (fallback field) (file field) (cli field).

Theorem unrelated_fields_do_not_interfere : forall fallback file cli file' cli' field,
  file field = file' field -> cli field = cli' field ->
  resolve_fields fallback file cli field = resolve_fields fallback file' cli' field.
Proof. intros; unfold resolve_fields; now rewrite H, H0. Qed.

Definition legacy_cli_default (fallback : nat) (cli : option nat) : option nat :=
  match cli with Some value => Some value | None => Some fallback end.

Theorem legacy_default_discards_configured_value : forall fallback value,
  value <> fallback ->
  resolve fallback (Some value) (legacy_cli_default fallback None) <>
  specification fallback (Some value) None.
Proof. intros; cbn; congruence. Qed.
