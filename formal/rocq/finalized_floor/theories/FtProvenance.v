(* ===========================================================================
   FtProvenance.v - G2: provenance determinism of the θ fault-tolerance threshold
   (ppm), and the widened i128 overflow envelope over the runtime's FULL validated
   ppm range [-den, den].

   A9 (FtExact.v) proves the finalization DECISION is integer-exact GIVEN the
   threshold numerator θ_ppm. It does NOT model where θ_ppm comes from. If two
   honest nodes could finalize with DIFFERENT θ_ppm for the same DAG, the exactness
   of the per-node test would not, by itself, rule out a fork. This module closes
   that seam: it models the node's θ_ppm SOURCING and proves it is a pure function
   of the on-chain value, independent of local config.

   ---------------------------------------------------------------------------
   The runtime sourcing path (casper/src/rust)
   ---------------------------------------------------------------------------
   Symbol         | Meaning                          | Rust site
   ---------------+----------------------------------+--------------------------
   local          | node's configured ppm            | casper_shard_conf
                  |   (native-token config)          |   .fault_tolerance_threshold_ppm
   onchain        | ppm baked into genesis PoS state | read_on_chain_fault_
                  |   (range-checked to [-1e6,1e6])   |   tolerance_threshold_ppm
                  |                                  |   (token_metadata_check.rs:101,105)
   reconcile      | value the node FINALIZES with    | casper.rs:266
                  |   (= onchain, UNCONDITIONALLY)    |   casper_shard_conf.ftt_ppm
                  |                                  |     = on_chain_ppm
   ---------------+----------------------------------+--------------------------

   At casper.rs:236-270 the node reads the on-chain ppm (:253), logs at :259 if the
   local config disagrees, and then at :266 OVERWRITES the in-memory config's ppm
   with the on-chain value, UNCONDITIONALLY (the `if` at :259 only gates the log,
   not the assignment). Every finalization thereafter
   uses `casper_shard_conf.fault_tolerance_threshold_ppm`, which now equals the
   on-chain value. Hence `reconcile local onchain = onchain` — local config is NOT
   a fork input.

   ---------------------------------------------------------------------------
   What this rules out (with A9 / FtExact)
   ---------------------------------------------------------------------------
   - Two nodes with the SAME genesis (same on-chain ppm) but DIFFERENT local
     `native-token` config finalize with the SAME θ_ppm  (reconcile_is_onchain),
     and the exact decision they run over i128 never overflows across the FULL
     validated range num ∈ [-den, den]  (ppm_range_decision_no_overflow), so the
     decision is bit-for-bit identical on every node -> no config-driven fork.

   Zero `Admitted`. No custom `Axiom` / `Parameter`. Stdlib (`ZArith`, `Lia`,
   `Psatz`) only, plus FtExact. Every lemma is `reflexivity`- or (via FtExact)
   `lia`/`nia`-closed; verify with `Print Assumptions` (all "Closed under the
   global context").
   =========================================================================== *)

From Stdlib Require Import ZArith.
From Stdlib Require Import Lia.
From Stdlib Require Import Psatz.
From Stdlib Require Import List Bool.

From FinalizedFloor Require Import FtExact.

Open Scope Z_scope.

(* The reconcile/override the node performs at casper.rs:266: the
   in-memory `local` ppm is UNCONDITIONALLY replaced by the on-chain `onchain` ppm.
   Modelled faithfully as the second projection — the result ignores `local`. *)
Definition reconcile (local onchain : Z) : Z := onchain.

(* PROVENANCE DETERMINISM. Whatever a node has in local config, the ppm it
   finalizes with is the on-chain value. Two nodes on the same genesis therefore
   agree on θ_ppm regardless of their local configuration, so local config is
   removed as a possible fork input. *)
Lemma reconcile_is_onchain :
  forall local onchain, reconcile local onchain = onchain.
Proof. intros local onchain. unfold reconcile. reflexivity. Qed.

(* The reconciled ppm depends ONLY on the on-chain value: agreeing on-chain ppm
   forces agreeing reconciled ppm, for any (even differing) local configs. This is
   the "no config-driven divergence" statement used with A9's exact decision. *)
Lemma reconcile_agrees_on_onchain :
  forall local local' onchain,
    reconcile local onchain = reconcile local' onchain.
Proof. intros local local' onchain. unfold reconcile. reflexivity. Qed.

(* Re-export of the widened i128 overflow envelope, specialized to the runtime's
   FULL validated ppm range: num ∈ [-den, den] = [-1e6, 1e6] (the range-check at
   token_metadata_check.rs:105 and the negative-θ sentinels of ft_decides_exact),
   NOT merely [0, den]. Both sides of the exact decision `2q·den ⋛ S·(den+num)`
   stay within i128, so a node that sourced its ppm from on-chain evaluates the
   decision without overflow on ANY validated on-chain threshold. *)
Lemma ppm_range_decision_no_overflow :
  forall q S num den,
    0 <= q <= S -> 0 <= S <= 2^63 -> -den <= num <= den -> den = 1000000 ->
    Z.abs (2*q*den) < 2^127 /\ Z.abs (S*(den+num)) < 2^127.
Proof. exact ft_exact_no_overflow. Qed.

Close Scope Z_scope.

Import ListNotations.

Record chain_parameters := {
  chain_parent_depth : Z;
  chain_deploy_lifespan : Z;
  chain_minimum_price : Z
}.

Definition chain_parameter_values (p : chain_parameters) : list Z :=
  [chain_parent_depth p; chain_deploy_lifespan p; chain_minimum_price p].

Theorem chain_parameter_values_injective : forall p q,
  chain_parameter_values p = chain_parameter_values q -> p = q.
Proof. intros [pd pl pm] [qd ql qm] equal. inversion equal. reflexivity. Qed.

Theorem chain_parameter_disagreement_changes_values : forall p q,
  p <> q -> chain_parameter_values p <> chain_parameter_values q.
Proof. intros p q different equal. apply different. now apply chain_parameter_values_injective. Qed.

Definition valid_chain_parameters (p : chain_parameters) : bool :=
  (1 <=? chain_parent_depth p)%Z && (chain_parent_depth p <=? 2147483647)%Z &&
  (1 <=? chain_deploy_lifespan p)%Z && (chain_deploy_lifespan p <=? 2147483647)%Z &&
  (0 <=? chain_minimum_price p)%Z && (chain_minimum_price p <=? 9223372036854775807)%Z.

Definition adopt_chain_parameters (_local : chain_parameters)
  (authenticated : option (list Z)) : option chain_parameters :=
  match authenticated with
  | Some [depth; lifespan; minimum] =>
      let p := {| chain_parent_depth := depth; chain_deploy_lifespan := lifespan;
                  chain_minimum_price := minimum |} in
      if valid_chain_parameters p then Some p else None
  | _ => None
  end.

Theorem chain_parameter_ranges_exact : forall p,
  valid_chain_parameters p = true <->
  (1 <= chain_parent_depth p <= 2147483647)%Z /\
  (1 <= chain_deploy_lifespan p <= 2147483647)%Z /\
  (0 <= chain_minimum_price p <= 9223372036854775807)%Z.
Proof. intros. unfold valid_chain_parameters. rewrite !andb_true_iff, !Z.leb_le. tauto. Qed.

Theorem chain_adoption_ignores_local_configuration : forall a b authenticated,
  adopt_chain_parameters a authenticated = adopt_chain_parameters b authenticated.
Proof. reflexivity. Qed.

Theorem missing_chain_parameters_reject : forall local,
  adopt_chain_parameters local None = None.
Proof. reflexivity. Qed.

Theorem invalid_chain_parameters_reject : forall local p,
  valid_chain_parameters p = false ->
  adopt_chain_parameters local
    (Some [chain_parent_depth p; chain_deploy_lifespan p; chain_minimum_price p]) = None.
Proof. intros local [depth lifespan minimum] invalid. cbn in *. rewrite invalid. reflexivity. Qed.

Theorem valid_chain_parameters_override_local : forall local p,
  valid_chain_parameters p = true ->
  adopt_chain_parameters local
    (Some [chain_parent_depth p; chain_deploy_lifespan p; chain_minimum_price p]) = Some p.
Proof. intros local [depth lifespan minimum] valid. cbn in *. rewrite valid. reflexivity. Qed.

Theorem accepted_chain_parameters_are_valid : forall local authenticated p,
  adopt_chain_parameters local authenticated = Some p -> valid_chain_parameters p = true.
Proof.
  intros local [values|] p accepted; [|discriminate].
  destruct values as [|depth [|lifespan [|minimum [|extra rest]]]]; try discriminate.
  cbn [adopt_chain_parameters] in accepted.
  destruct (valid_chain_parameters _) eqn:valid; inversion accepted; subst; assumption.
Qed.

Theorem adopted_consumer_decisions_agree : forall (Input Output : Type)
  (consumer : chain_parameters -> Input -> Output) local other authenticated p q input,
  adopt_chain_parameters local authenticated = Some p ->
  adopt_chain_parameters other authenticated = Some q ->
  consumer p input = consumer q input.
Proof.
  intros Input Output consumer local other authenticated p q input first second.
  rewrite (chain_adoption_ignores_local_configuration local other authenticated) in first.
  rewrite first in second. inversion second. reflexivity.
Qed.

Example local_minimum_can_disagree : (2 <=? 3)%Z = true /\ (4 <=? 3)%Z = false.
Proof. split; reflexivity. Qed.

Example oversized_lifespan_is_rejected : forall local,
  adopt_chain_parameters local (Some [10%Z; 2147483648%Z; 0%Z]) = None.
Proof. reflexivity. Qed.

Theorem malformed_chain_tuple_rejects : forall local values,
  length values <> 3 -> adopt_chain_parameters local (Some values) = None.
Proof.
  intros local [|a [|b [|c [|d rest]]]] malformed; try reflexivity.
  exfalso. apply malformed. reflexivity.
Qed.

Definition advertised_minimum (bootstrap : Z) (running : option chain_parameters) : Z :=
  match running with
  | Some adopted => chain_minimum_price adopted
  | None => bootstrap
  end.

Theorem running_status_uses_adopted_minimum : forall bootstrap adopted,
  advertised_minimum bootstrap (Some adopted) = chain_minimum_price adopted.
Proof. reflexivity. Qed.

Theorem running_status_ignores_bootstrap : forall first second adopted,
  advertised_minimum first (Some adopted) = advertised_minimum second (Some adopted).
Proof. reflexivity. Qed.

Theorem startup_status_uses_bootstrap : forall bootstrap,
  advertised_minimum bootstrap None = bootstrap.
Proof. reflexivity. Qed.

Theorem running_status_history_agrees : forall first second history,
  map (fun p => advertised_minimum first (Some p)) history =
  map (fun p => advertised_minimum second (Some p)) history.
Proof. reflexivity. Qed.

Print Assumptions running_status_uses_adopted_minimum.
Print Assumptions running_status_ignores_bootstrap.
Print Assumptions startup_status_uses_bootstrap.
Print Assumptions running_status_history_agrees.

Definition next_block_deploy_open (valid_after tip lifespan : Z) : bool :=
  (tip + 1 - lifespan <? valid_after)%Z.

Theorem next_block_deploy_window_exact : forall valid_after tip lifespan,
  next_block_deploy_open valid_after tip lifespan = true <->
  (tip + 1 < valid_after + lifespan)%Z.
Proof. intros. unfold next_block_deploy_open. rewrite Z.ltb_lt. lia. Qed.

Theorem next_block_deploy_boundary_rejects : forall tip lifespan,
  next_block_deploy_open (tip + 1 - lifespan) tip lifespan = false.
Proof. intros. unfold next_block_deploy_open. apply Z.ltb_irrefl. Qed.

Theorem next_block_deploy_admission_refines_tip : forall valid_after tip lifespan,
  next_block_deploy_open valid_after tip lifespan = true ->
  (tip - lifespan <? valid_after)%Z = true.
Proof. intros. apply Z.ltb_lt. apply next_block_deploy_window_exact in H. lia. Qed.

Print Assumptions next_block_deploy_window_exact.
Print Assumptions next_block_deploy_boundary_rejects.
Print Assumptions next_block_deploy_admission_refines_tip.

Definition checked_next_height (tip : Z) : option Z :=
  if (tip <? 9223372036854775807)%Z then Some (tip + 1)%Z else None.

Theorem checked_next_height_exact : forall tip next,
  checked_next_height tip = Some next <->
  (tip < 9223372036854775807)%Z /\ next = (tip + 1)%Z.
Proof.
  intros tip next. unfold checked_next_height.
  destruct (tip <? 9223372036854775807)%Z eqn:bound.
  - apply Z.ltb_lt in bound. split.
    + intros equal. inversion equal; subst. split; auto.
    + intros [_ ->]. reflexivity.
  - apply Z.ltb_ge in bound. split; [discriminate|intros [H _]; lia].
Qed.

Theorem checked_next_height_preserves_machine_range : forall tip next,
  (0 <= tip)%Z -> checked_next_height tip = Some next ->
  (1 <= next <= 9223372036854775807)%Z.
Proof. intros. apply checked_next_height_exact in H0. lia. Qed.

Theorem exhausted_height_rejects : checked_next_height 9223372036854775807 = None.
Proof. reflexivity. Qed.

Print Assumptions checked_next_height_exact.
Print Assumptions checked_next_height_preserves_machine_range.
Print Assumptions exhausted_height_rejects.
Print Assumptions chain_parameter_ranges_exact.
Print Assumptions chain_parameter_values_injective.
Print Assumptions chain_parameter_disagreement_changes_values.
Print Assumptions chain_adoption_ignores_local_configuration.
Print Assumptions missing_chain_parameters_reject.
Print Assumptions invalid_chain_parameters_reject.
Print Assumptions valid_chain_parameters_override_local.
Print Assumptions accepted_chain_parameters_are_valid.
Print Assumptions adopted_consumer_decisions_agree.
Print Assumptions local_minimum_can_disagree.
Print Assumptions oversized_lifespan_is_rejected.
Print Assumptions malformed_chain_tuple_rejects.
