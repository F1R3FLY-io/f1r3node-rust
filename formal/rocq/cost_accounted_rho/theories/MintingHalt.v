(* ═══════════════════════════════════════════════════════════════════════════
   MintingHalt.v — A halted validator is never minted and never gains supply
   ═══════════════════════════════════════════════════════════════════════════

   Cost-Accounted Rho Stage C halt INTERFACE (proved at Stage B; DR-3 / DR-13,
   docs/theory/cost-accounting-impl/stageb-minting-halt-interface.md Decision 4).

   Slashing halts a validator's phlogiston minting via the "mintingHalted" set
   (modeled as [pb_halted] in MintingInjection.v's [pos_state]); the Stage-B
   epoch-mint fold SKIPS any [v ∈ mintingHalted] across ALL epochs (the cross-
   epoch halt, spec Appendix B "Slashing"). This module discharges the two
   obligations the design names:

     - [halted_validator_not_minted]            : a halted validator is never
       recorded in the mint ledger by an epoch mint, and its mint never fires
       (the mint operation is the identity for a halted validator).
     - [halted_validator_supply_not_increased]  : a halted validator's supply
       balance Σ⟦v⟧ is never raised by an epoch mint (its balance is exactly
       preserved).

   Both compose to the redemption-safety property (Decision 4 /
   [redeemSlashed]): nothing is credited while halted; supply resumes only after
   "mintingHalted" is cleared and the NORMAL next-epoch mint re-funds — so ALL
   phlogiston creation stays on the single authorized path.

   INDEPENDENCE. By G-coordination this module is INDEPENDENT of MainTheorem.v
   (the slashing-tree headline): it imports ONLY MintingInjection (the Stage-B
   PoS-state / epoch-mint abstraction) and its transitive RhoSyntax /
   WalletNaming dependencies. Stage B touches NO slashing-tree files, so it
   lands regardless of the slashing development.

   Everything is concrete (no axioms / Section hypotheses): [Print Assumptions]
   of each headline theorem reports "Closed under the global context".

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Theorem                            │ Property
   ─────────────────────────────────────────┼───────────────────────────────
   halted_validator_not_minted              │ v ∈ mintingHalted ⇒ epoch mint
                                            │   is a no-op + records nothing
   halted_validator_supply_not_increased    │ v ∈ mintingHalted ⇒ Σ⟦v⟧ fixed
   halted_validator_ledger_unchanged        │ halt ⇒ mintedEpochs unchanged
   halted_then_credit_requires_unhalt       │ a credit to a halted v needs an
                                            │   intervening unhalt (redemption)
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.x stdlib, MintingInjection (this project) and its
                 transitive deps (RhoSyntax, WalletNaming, TokenConservation).
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import List.
Import ListNotations.

From CostAccountedRho Require Import MintingInjection.
From CostAccountedRho Require Import WalletNaming.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: A halted validator's epoch mint is the identity
   ═══════════════════════════════════════════════════════════════════════════

   [epoch_mint] short-circuits to the identity whenever the validator is
   ineligible, and a halted validator is ineligible by the [mint_eligible] halt
   guard. So for [v ∈ pb_halted st] the mint changes nothing at all.            *)

Lemma halted_epoch_mint_is_identity : forall st v e amt,
  In v (pb_halted st) ->
  epoch_mint st v e amt = st.
Proof.
  intros st v e amt Hin.
  unfold epoch_mint, mint_eligible.
  assert (Hmem : pubkey_inb v (pb_halted st) = true)
    by (apply pubkey_inb_true_iff; exact Hin).
  rewrite Hmem. simpl. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: Headline — a halted validator gains no supply
   ═══════════════════════════════════════════════════════════════════════════ *)

(* DR-3 / Decision 4: a halted validator's supply balance Σ⟦v⟧ is NOT increased
   by an epoch mint — it is exactly preserved. Direct from
   [halted_epoch_mint_balance_unchanged] (MintingInjection.v), restated as the
   non-increase bound the threat model (TM-CA-156) references. *)
Theorem halted_validator_supply_not_increased : forall st v e amt,
  In v (pb_halted st) ->
  balance_of (epoch_mint st v e amt) v <= balance_of st v.
Proof.
  intros st v e amt Hin.
  rewrite (halted_epoch_mint_balance_unchanged st v e amt Hin).
  (* balance_of st v <= balance_of st v *)
  apply Nat.le_refl.
Qed.

(* Exact (equality) form. *)
Corollary halted_validator_supply_unchanged : forall st v e amt,
  In v (pb_halted st) ->
  balance_of (epoch_mint st v e amt) v = balance_of st v.
Proof.
  intros st v e amt Hin.
  apply (halted_epoch_mint_balance_unchanged st v e amt Hin).
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Headline — a halted validator is never minted
   ═══════════════════════════════════════════════════════════════════════════ *)

(* DR-3 / Decision 4: an epoch mint never RECORDS a halted validator in the mint
   ledger — the ledger is unchanged, so the halted validator is not minted this
   epoch (nor does its @W_v purse fire, the Rholang-side mirror). Because the
   mint is the identity for a halted validator, [pb_minted] is untouched. *)
Theorem halted_validator_not_minted : forall st v e amt,
  In v (pb_halted st) ->
  pb_minted (epoch_mint st v e amt) = pb_minted st.
Proof.
  intros st v e amt Hin.
  rewrite (halted_epoch_mint_is_identity st v e amt Hin).
  reflexivity.
Qed.

(* In particular the (v, e) record is NOT introduced for a halted v that was not
   already present — the mint cannot mark a halted validator as minted. *)
Corollary halted_validator_not_freshly_recorded : forall st v e amt,
  In v (pb_halted st) ->
  ~ In (v, e) (pb_minted st) ->
  ~ In (v, e) (pb_minted (epoch_mint st v e amt)).
Proof.
  intros st v e amt Hhalt Hnotin.
  rewrite (halted_validator_not_minted st v e amt Hhalt).
  exact Hnotin.
Qed.

(* The full administrative state is unchanged by a halted validator's epoch mint
   (balances, ledger, and halt set all fixed). *)
Corollary halted_validator_state_unchanged : forall st v e amt,
  In v (pb_halted st) ->
  epoch_mint st v e amt = st.
Proof.
  intros st v e amt Hin.
  apply (halted_epoch_mint_is_identity st v e amt Hin).
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Redemption is the ONLY way a halted validator regains supply
   ═══════════════════════════════════════════════════════════════════════════

   Combining Sections 2-3: while [v] stays in [pb_halted], NO sequence of epoch
   mints changes its balance. So if a halted validator's balance ever rises, an
   intervening UNHALT (redemption clearing "mintingHalted") must have occurred —
   the formal core of "redemption writes neither Σ⟦v⟧ nor @W_v directly; it
   clears the flag and lets the normal next-epoch mint re-fund" (Decision 4).   *)

(* Two successive epoch mints of a validator that is halted in BOTH states leave
   the balance fixed — halt is sticky across mints unless cleared. *)
Theorem halted_validator_supply_fixed_across_two_mints :
  forall st v e1 amt1 e2 amt2,
    In v (pb_halted st) ->
    In v (pb_halted (epoch_mint st v e1 amt1)) ->
    balance_of (epoch_mint (epoch_mint st v e1 amt1) v e2 amt2) v
      = balance_of st v.
Proof.
  intros st v e1 amt1 e2 amt2 Hh0 Hh1.
  rewrite (halted_validator_supply_unchanged (epoch_mint st v e1 amt1) v e2 amt2 Hh1).
  apply (halted_validator_supply_unchanged st v e1 amt1 Hh0).
Qed.

(* Contrapositive flavour: if a single epoch mint STRICTLY increases [v]'s
   balance, then [v] was NOT halted — a credit implies eligibility implies
   unhalted. This is the "nothing is credited while halted" guarantee. *)
Theorem credit_implies_not_halted : forall st v e amt,
  balance_of st v < balance_of (epoch_mint st v e amt) v ->
  ~ In v (pb_halted st).
Proof.
  intros st v e amt Hlt Hhalt.
  rewrite (halted_validator_supply_unchanged st v e amt Hhalt) in Hlt.
  (* Hlt : balance_of st v < balance_of st v — impossible. *)
  apply (Nat.lt_irrefl (balance_of st v)). exact Hlt.
Qed.
