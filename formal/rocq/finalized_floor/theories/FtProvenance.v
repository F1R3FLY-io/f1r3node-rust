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
   reconcile      | value the node FINALIZES with    | casper.rs:242
                  |   (= onchain, UNCONDITIONALLY)    |   casper_shard_conf.ftt_ppm
                  |                                  |     = on_chain_ppm
   ---------------+----------------------------------+--------------------------

   At casper.rs:230-242 the node reads the on-chain ppm (:230), logs at :236 if the
   local config disagrees, and then at :242 OVERWRITES the in-memory config's ppm
   with the on-chain value, UNCONDITIONALLY (the `if` at :235 only gates the log,
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

From FinalizedFloor Require Import FtExact.

Open Scope Z_scope.

(* The reconcile/override the node performs at casper.rs:242: the
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
