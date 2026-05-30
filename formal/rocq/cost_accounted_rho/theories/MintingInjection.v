(* ═══════════════════════════════════════════════════════════════════════════
   MintingInjection.v — Minting is exogenous injection, not a metered step
   ═══════════════════════════════════════════════════════════════════════════

   Stage-0 "layering theorem" of the Cost-Accounted Rho realization.

   The cost-accounted calculus conserves fuel: every [ca_step] consumes a
   strictly positive quantum of token-fuel and never creates any
   (TokenConservation.v: [token_monotone_step], [token_consumed_per_step],
   [token_strictly_decreases]). For that invariant to survive in a running
   system, the act of *minting* — bringing new token-fuel into existence —
   must sit OUTSIDE the reduction relation. Minting is exogenous
   administration (spec §2.4 / §4.6): an authorized party constructs a token
   stack and deposits it as a free token in the ambient parallel
   composition. It is never a cost-accounted reduction step.

   This module makes that separation precise:

   1. [mint_inject S t] is the administrative injection of token stack [t]
      into system [S]; its fuel is exactly the prior fuel plus [token_size t].
   2. No [ca_step] increases the total fuel ([user_ca_step_does_not_mint]),
      so injecting a non-empty stack can never be realized by a [ca_step]
      ([mint_inject_not_ca_step]).
   3. An interleaved administration model ([admin_trans]) evolves a system by
      user [ca_step]s and authorized mint injections. Along any such trace
      the net fuel increase is bounded above by the total minted size:
      reduction only consumes, minting is the SOLE producer
      ([admin_reachable_net_increase_bounded_by_minted]).

   Because minting lives strictly outside [ca_step], every existing
   token-conservation / strong-normalization / confluence result over
   [ca_step] / [ca_reachable] survives verbatim: the producer of fuel is a
   different relation entirely.

   ─────────────────────────────────────────────────────────────────────────
   Spec-to-Code Traceability
   ─────────────────────────────────────────────────────────────────────────
   Rocq Definition / Theorem                       │ Paper Property
   ────────────────────────────────────────────────┼──────────────────────────
   mint_inject S t                                  │ §2.4/§4.6 token injection
   mint_inject_token_count                          │ "‖mint(S,t)‖ = ‖S‖ + |t|"
   user_ca_step_does_not_mint                       │ "S ⤳ S' ⇒ ‖S'‖ ≤ ‖S‖"
   mint_inject_not_ca_step                          │ "minting ≠ a ⤳ step"
   admin_op / admin_trans                           │ interleaved administration
   admin_trans_step_no_mint                         │ "AStep never creates fuel"
   admin_trans_mint_adds_exactly                    │ "AMint t adds exactly |t|"
   admin_reachable                                  │ ⤳/mint reflexive-trans closure
   admin_reachable_net_increase_bounded_by_minted   │ "reduction consumes;
                                                    │   minting is sole producer"
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.x stdlib, RhoSyntax, CostAccountedSyntax,
                 CostAccountedReduction, TokenConservation (this project)
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import Lia.
From Stdlib Require Import Arith.PeanoNat.
From Stdlib Require Import Bool.Bool.
From Stdlib Require Import List.
Import ListNotations.

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import TokenConservation.
From CostAccountedRho Require Import WalletNaming.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: Minting as Exogenous Injection
   ═══════════════════════════════════════════════════════════════════════════

   [mint_inject S t] deposits the token stack [t] as a free token alongside
   [S]. Compare with the five COMM rules of [ca_step], every one of which
   *strips* gates from an existing token to authorise a redex; minting does
   the opposite, and crucially it is a plain function on systems rather than
   a constructor of the reduction relation.                                   *)

Definition mint_inject (S : system) (t : token) : system :=
  SPar S (SToken t).

(* The fuel of a minted system is exactly the prior fuel plus the size of
   the injected stack. Immediate from the additive shape of
   [system_token_count] on [SPar] and [system_token_count (SToken t) =
   token_size t]. *)
Lemma mint_inject_token_count :
  forall S t,
    system_token_count (mint_inject S t)
    = system_token_count S + token_size t.
Proof.
  intros S t. unfold mint_inject. simpl. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: No Cost-Accounted Step Ever Mints
   ═══════════════════════════════════════════════════════════════════════════

   The conservation invariant restated at the layering boundary: a single
   cost-accounted reduction step never increases the total fuel. This is the
   [<=] form, obtained directly from [token_monotone_step] in
   TokenConservation.v (which is already stated as the non-increase bound;
   were it stated as the strict-decrease [token_strictly_decreases] we would
   weaken it here with [lia]).                                                 *)

Theorem user_ca_step_does_not_mint :
  forall S S',
    ca_step S S' ->
    system_token_count S' <= system_token_count S.
Proof.
  intros S S' Hstep.
  exact (token_monotone_step S S' Hstep).
Qed.

(* Hence minting a NON-EMPTY stack cannot be realized by any [ca_step]:
   such a step would have to raise the fuel count strictly above the source,
   contradicting [user_ca_step_does_not_mint]. *)
Theorem mint_inject_not_ca_step :
  forall S t,
    token_size t > 0 ->
    ~ ca_step S (mint_inject S t).
Proof.
  intros S t Hpos Hstep.
  apply user_ca_step_does_not_mint in Hstep.
  rewrite mint_inject_token_count in Hstep.
  lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: Interleaved Administration Model
   ═══════════════════════════════════════════════════════════════════════════

   A running cost-accounted system evolves by two kinds of operation:

     - [AStep]   : a user-driven cost-accounted reduction step ([ca_step]).
     - [AMint t] : an authorized exogenous injection of the token stack [t].

   [admin_trans] is the labelled one-step transition relation that unites
   them. Tagging transitions with the operation lets us state precisely how
   the fuel count is allowed to move: AStep transitions never create fuel,
   AMint transitions create exactly the injected stack size.                  *)

Inductive admin_op : Type :=
  | AStep : admin_op
  | AMint : token -> admin_op.

Inductive admin_trans : admin_op -> system -> system -> Prop :=
  | at_step : forall S S',
      ca_step S S' ->
      admin_trans AStep S S'
  | at_mint : forall S t,
      admin_trans (AMint t) S (mint_inject S t).

(* A user step, viewed as an administrative transition, never creates fuel. *)
Theorem admin_trans_step_no_mint :
  forall S S',
    admin_trans AStep S S' ->
    system_token_count S' <= system_token_count S.
Proof.
  intros S S' Htr.
  inversion Htr; subst.
  apply user_ca_step_does_not_mint. assumption.
Qed.

(* A mint transition adds exactly the size of the injected stack — no more,
   no less. Minting is the only fuel-creating operation, and it creates a
   precisely accountable amount. *)
Theorem admin_trans_mint_adds_exactly :
  forall S S' t,
    admin_trans (AMint t) S S' ->
    system_token_count S' = system_token_count S + token_size t.
Proof.
  intros S S' t Htr.
  inversion Htr; subst.
  apply mint_inject_token_count.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: Reachability under Interleaved Administration
   ═══════════════════════════════════════════════════════════════════════════

   [admin_reachable] is the reflexive-transitive closure of [admin_trans],
   forgetting the operation labels. Each step also records the operation in
   [admin_trace], so that we can sum the total fuel minted along a trace.    *)

Inductive admin_reachable : system -> system -> Prop :=
  | ar_refl : forall S,
      admin_reachable S S
  | ar_step : forall op S1 S2 S3,
      admin_trans op S1 S2 ->
      admin_reachable S2 S3 ->
      admin_reachable S1 S3.

(* The total fuel contributed by an administrative operation: a user step
   contributes nothing (it only consumes), a mint contributes exactly the
   size of its injected stack. *)
Definition admin_op_minted (op : admin_op) : nat :=
  match op with
  | AStep    => 0
  | AMint t  => token_size t
  end.

(* A labelled reflexive-transitive closure that also accumulates the total
   minted fuel along the trace, so the "minting is the sole producer"
   accounting can be stated quantitatively. *)
Inductive admin_trace : system -> nat -> system -> Prop :=
  | tr_refl : forall S,
      admin_trace S 0 S
  | tr_step : forall op S1 S2 m S3,
      admin_trans op S1 S2 ->
      admin_trace S2 m S3 ->
      admin_trace S1 (admin_op_minted op + m) S3.

(* An [admin_trace] is in particular an [admin_reachable] (forget the
   accumulated minted total). *)
Lemma admin_trace_reachable :
  forall S m S',
    admin_trace S m S' ->
    admin_reachable S S'.
Proof.
  intros S m S' Htr.
  induction Htr as [S | op S1 S2 m S3 Hstep Htr' IH].
  - apply ar_refl.
  - eapply ar_step.
    + exact Hstep.
    + exact IH.
Qed.

(* Conversely every [admin_reachable] trace can be annotated with the total
   fuel minted along it, witnessed by an [admin_trace]. *)
Lemma admin_reachable_trace :
  forall S S',
    admin_reachable S S' ->
    exists m, admin_trace S m S'.
Proof.
  intros S S' Hreach.
  induction Hreach as [S | op S1 S2 S3 Hstep Hreach' IH].
  - exists 0. apply tr_refl.
  - destruct IH as [m Htr].
    exists (admin_op_minted op + m).
    eapply tr_step.
    + exact Hstep.
    + exact Htr.
Qed.

(* One-step bound: an [admin_trans] raises the fuel count by at most the
   amount it mints. For [AStep] the minted amount is 0 and the count does
   not increase; for [AMint t] the count rises by exactly [token_size t],
   which is the minted amount. In both cases the post-state count is bounded
   above by the pre-state count plus the minted amount. *)
Lemma admin_trans_increase_bounded_by_minted :
  forall op S S',
    admin_trans op S S' ->
    system_token_count S' <= system_token_count S + admin_op_minted op.
Proof.
  intros op S S' Htr.
  destruct Htr as [S S' Hstep | S t].
  - (* AStep: minted = 0, and the step does not create fuel. *)
    apply user_ca_step_does_not_mint in Hstep. simpl. lia.
  - (* AMint t: minted = token_size t, and the count rises by exactly that. *)
    rewrite mint_inject_token_count. simpl. lia.
Qed.

(* Headline accounting theorem: along any administrative trace, the net
   increase in total fuel is bounded above by the total minted along the
   trace. Equivalently, [‖S'‖ ≤ ‖S‖ + (total minted)]: reduction can only
   consume fuel, so MINTING IS THE SOLE PRODUCER of fuel, and it can produce
   at most what it injects.

   By induction on the trace. The reflexive case is immediate. The step case
   chains the one-step bound [admin_trans_increase_bounded_by_minted] with
   the inductive hypothesis on the remainder of the trace; [lia] discharges
   the resulting linear arithmetic over the per-operation minted amounts. *)
Theorem admin_trace_net_increase_bounded_by_minted :
  forall S m S',
    admin_trace S m S' ->
    system_token_count S' <= system_token_count S + m.
Proof.
  intros S m S' Htr.
  induction Htr as [S | op S1 S2 m S3 Hstep Htr' IH].
  - (* tr_refl: ‖S‖ <= ‖S‖ + 0. *)
    lia.
  - (* tr_step: S1 --op--> S2, then S2 ⇝ S3 minting m.
       IH    : ‖S3‖ <= ‖S2‖ + m
       Hstep : ‖S2‖ <= ‖S1‖ + admin_op_minted op
       Goal  : ‖S3‖ <= ‖S1‖ + (admin_op_minted op + m). *)
    apply admin_trans_increase_bounded_by_minted in Hstep.
    lia.
Qed.

(* The same accounting stated directly over [admin_reachable]: there exists a
   total minted amount [m] (the sum of injected stack sizes along the trace)
   that bounds the net fuel increase from above. Fuel created across the
   trace is therefore attributable entirely to minting. *)
Theorem admin_reachable_net_increase_bounded_by_minted :
  forall S S',
    admin_reachable S S' ->
    exists m, system_token_count S' <= system_token_count S + m.
Proof.
  intros S S' Hreach.
  apply admin_reachable_trace in Hreach.
  destruct Hreach as [m Htr].
  exists m.
  apply admin_trace_net_increase_bounded_by_minted.
  exact Htr.
Qed.

(* Corollary: a mint-free administrative trace (total minted = 0) never
   increases the fuel count — it behaves exactly like a pure [ca_reachable]
   reduction sequence. This is the precise sense in which the existing
   token-conservation results survive verbatim once minting is excluded. *)
Corollary admin_trace_no_mint_conserves :
  forall S S',
    admin_trace S 0 S' ->
    system_token_count S' <= system_token_count S.
Proof.
  intros S S' Htr.
  apply admin_trace_net_increase_bounded_by_minted in Htr.
  lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: The supply-pool balance layer (Stage B; DR-13)
   ═══════════════════════════════════════════════════════════════════════════

   Stage A proved the validator DRAW wallet @W_v is injective + domain-
   separated (WalletNaming.v). Stage B adds the distinct SUPPLY POOL
   Σ⟦v⟧ = from_sig(Ground(pk)) — the channel the WD-D2 acceptance gate reads
   and the Rust [supply::produce_balance] writes — and the per-validator-per-
   epoch mint ledger. We model:

     - [supply_name pk]  : the content-addressed supply channel Σ⟦v⟧, a FOURTH
       content-addressed family beside Wallet / Quarantine / FundingSlot. Built
       with the SAME injective bit-encoder [encode_bits] (WalletNaming.v) under
       a structurally-distinct [Supply] marker, so injectivity is inherited and
       proved unconditionally.
     - [pos_state]       : the administrative state the mint reads/writes — the
       per-validator balances [pb_balance], the mint ledger [pb_minted] (the
       Rholang "mintedEpochs": Set[(Pk,Int)]), and the halt set [pb_halted]
       (the Rholang "mintingHalted": Set[Pk]).
     - [epoch_mint]      : the Stage-B mint operation on [pos_state]; it credits
       [amt] to [v] and records [(v,e)] ONLY when [v] is eligible (not halted,
       not already minted for epoch [e]) — mirroring the Rholang fold predicate
       and the Rust post_eval recompute. Otherwise it is the identity.

   Everything is concrete (no axioms / Section hypotheses), so the headline
   lemmas are Closed under the global context.                                 *)

(* The supply-pool domain marker: a FOURTH structurally-distinct closed marker,
   never equal to the three WalletNaming markers (a 4-deep [PReplicate] nest is
   distinct from the 1/2/3-deep wallet/quarantine/funding-slot nests). *)
Definition supply_marker : proc :=
  PReplicate (PReplicate (PReplicate (PReplicate PNil))).

(* Σ⟦v⟧ for validator [pk]: the quotation pairing the supply marker with the
   injective bit-encoding of [pk]. The same shape as [domain_name], so the
   supply channel inherits [encode_bits]'s injectivity. *)
Definition supply_name (pk : pubkey) : name :=
  Quote (PPar supply_marker (encode_bits pk)).

(* DR-13 / Decision 7: the supply pool is INJECTIVE in the validator public
   key — distinct validators' Σ⟦v⟧ channels are distinct, so a mint credited
   for [pk1] can never land in [pk2]'s pool and the multi-parent merge engine
   never conflates two validators' supply Produces. Proved by descending the
   [Quote]/[PPar] (identical [supply_marker] on both sides) to the bit-encoding
   equality and applying [encode_bits_injective] (WalletNaming.v's blake2b-
   analogue injective encoder). *)
Theorem supply_write_injective_in_pk : forall pk1 pk2,
  supply_name pk1 = supply_name pk2 -> pk1 = pk2.
Proof.
  intros pk1 pk2 Heq.
  unfold supply_name in Heq.
  injection Heq as Hbits.
  apply encode_bits_injective. exact Hbits.
Qed.

(* Inequality form: distinct keys ⇒ distinct supply pools. *)
Corollary supply_name_distinct : forall pk1 pk2,
  pk1 <> pk2 -> supply_name pk1 <> supply_name pk2.
Proof.
  intros pk1 pk2 Hne Heq. apply Hne.
  apply supply_write_injective_in_pk. exact Heq.
Qed.

(* The supply pool is a DISTINCT channel family from the draw wallet @W_v: for
   ANY keys, Σ⟦v⟧ ≠ @W_v' (the markers differ — supply is a 4-deep nest, wallet
   a 1-deep nest). This is the DR-13 "@W_v is DISTINCT from Σ⟦v⟧" property at
   the name layer: the gate's read channel can never collide with a draw. *)
Theorem supply_wallet_disjoint : forall pk1 pk2,
  supply_name pk1 <> wallet_name pk2.
Proof.
  intros pk1 pk2 Heq.
  unfold supply_name, wallet_name, domain_name in Heq.
  injection Heq as Hm _.
  (* Hm : supply_marker = domain_marker Wallet, i.e. a 4-deep nest = 1-deep. *)
  unfold supply_marker, domain_marker in Hm. discriminate.
Qed.

(* Public keys decide equality (bit-lists over [bool] have decidable equality),
   used to evaluate the eligibility predicate concretely. *)
Definition pubkey_eqb (a b : pubkey) : bool :=
  if list_eq_dec Bool.bool_dec a b then true else false.

Lemma pubkey_eqb_true_iff : forall a b, pubkey_eqb a b = true <-> a = b.
Proof.
  intros a b. unfold pubkey_eqb.
  destruct (list_eq_dec Bool.bool_dec a b) as [Heq | Hne]; split; intro H;
    try reflexivity; try assumption; try discriminate.
  - exfalso. apply Hne. exact H.
Qed.

(* A (pubkey, epoch) pair decides equality (epoch is a [nat]). *)
Definition mint_key_eqb (a b : pubkey * nat) : bool :=
  pubkey_eqb (fst a) (fst b) && Nat.eqb (snd a) (snd b).

Lemma mint_key_eqb_true_iff : forall a b, mint_key_eqb a b = true <-> a = b.
Proof.
  intros [a1 a2] [b1 b2]. unfold mint_key_eqb. simpl.
  rewrite Bool.andb_true_iff, pubkey_eqb_true_iff, Nat.eqb_eq.
  split.
  - intros [-> ->]. reflexivity.
  - intros Heq. injection Heq as -> ->. split; reflexivity.
Qed.

(* Membership of a (pk, epoch) in the mint ledger, as a boolean. *)
Definition mint_key_inb (k : pubkey * nat) (l : list (pubkey * nat)) : bool :=
  existsb (fun x => mint_key_eqb k x) l.

Lemma mint_key_inb_true_iff : forall k l,
  mint_key_inb k l = true <-> In k l.
Proof.
  intros k l. unfold mint_key_inb. rewrite existsb_exists. split.
  - intros [x [Hin Heq]]. apply mint_key_eqb_true_iff in Heq. subst x. exact Hin.
  - intros Hin. exists k. split; [exact Hin |]. apply mint_key_eqb_true_iff. reflexivity.
Qed.

(* Membership of a pk in the halt set, as a boolean. *)
Definition pubkey_inb (v : pubkey) (l : list pubkey) : bool :=
  existsb (fun x => pubkey_eqb v x) l.

Lemma pubkey_inb_true_iff : forall v l,
  pubkey_inb v l = true <-> In v l.
Proof.
  intros v l. unfold pubkey_inb. rewrite existsb_exists. split.
  - intros [x [Hin Heq]]. apply pubkey_eqb_true_iff in Heq. subst x. exact Hin.
  - intros Hin. exists v. split; [exact Hin |]. apply pubkey_eqb_true_iff. reflexivity.
Qed.

(* The administrative PoS economic state read/written by the Stage-B mint. *)
Record pos_state : Type := {
  pb_balance : pubkey -> nat;             (* Σ⟦v⟧ supply balances *)
  pb_minted  : list (pubkey * nat);        (* "mintedEpochs": Set[(Pk,Int)] *)
  pb_halted  : list pubkey                 (* "mintingHalted": Set[Pk] *)
}.

Definition balance_of (st : pos_state) (v : pubkey) : nat := pb_balance st v.

(* Eligibility for an epoch mint — the EXACT predicate of the Rholang fold and
   the Rust post_eval recompute: NOT halted AND NOT already minted for [e].
   (Activeness is an orthogonal membership the caller folds over; halt +
   not-already-minted are the idempotency/halt guards modeled here.) *)
Definition mint_eligible (st : pos_state) (v : pubkey) (e : nat) : bool :=
  negb (pubkey_inb v (pb_halted st)) && negb (mint_key_inb (v, e) (pb_minted st)).

(* Credit [amt] to [v] (a point update of the balance function). *)
Definition credit (st : pos_state) (v : pubkey) (amt : nat) : pubkey -> nat :=
  fun w => if pubkey_eqb w v then pb_balance st w + amt else pb_balance st w.

(* The Stage-B epoch mint on the administrative state. Eligible ⇒ credit [amt]
   and record [(v,e)]; ineligible (halted or already minted this epoch) ⇒ the
   IDENTITY (no balance change, no ledger change) — mirroring both the Rholang
   guard (no second @W_v purse) and the Rust post_eval guard (no
   produce_balance). [produce_balance]'s read-modify-REPLACE means an accidental
   re-exec rewrites the SAME value, exactly captured by this idempotent shape. *)
Definition epoch_mint (st : pos_state) (v : pubkey) (e : nat) (amt : nat) : pos_state :=
  if mint_eligible st v e
  then {| pb_balance := credit st v amt;
          pb_minted  := (v, e) :: pb_minted st;
          pb_halted  := pb_halted st |}
  else st.

(* Decision 7 / Decision 3: the epoch mint is IDEMPOTENT on the balance — once
   [(v,e)] is in the mint ledger, re-running the epoch mint for [(v,e)] does not
   change [v]'s balance. This is the formal core of multi-parent-merge / replay
   mint-idempotency: a duplicated epoch mint is a no-op on Σ⟦v⟧. Immediate from
   the eligibility guard short-circuiting [epoch_mint] to the identity. *)
Theorem epoch_mint_idempotent_on_balance : forall st v e amt,
  In (v, e) (pb_minted st) ->
  balance_of (epoch_mint st v e amt) v = balance_of st v.
Proof.
  intros st v e amt Hin.
  unfold epoch_mint, mint_eligible.
  (* (v,e) ∈ minted ⇒ mint_key_inb (v,e) ... = true ⇒ negb ... = false ⇒
     the eligibility conjunction is false ⇒ epoch_mint = st. *)
  assert (Hmem : mint_key_inb (v, e) (pb_minted st) = true)
    by (apply mint_key_inb_true_iff; exact Hin).
  rewrite Hmem.
  rewrite Bool.andb_false_r.
  reflexivity.
Qed.

(* A halted validator is also never credited by an epoch mint (the halt guard),
   regardless of the ledger — used by MintingHalt.v. *)
Theorem halted_epoch_mint_balance_unchanged : forall st v e amt,
  In v (pb_halted st) ->
  balance_of (epoch_mint st v e amt) v = balance_of st v.
Proof.
  intros st v e amt Hin.
  unfold epoch_mint, mint_eligible.
  assert (Hmem : pubkey_inb v (pb_halted st) = true)
    by (apply pubkey_inb_true_iff; exact Hin).
  rewrite Hmem. simpl. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: User reduction never moves a supply balance (Stage B)
   ═══════════════════════════════════════════════════════════════════════════

   The balance-layer companion of [user_ca_step_does_not_mint]. The cost-
   accounted reduction relation [ca_step] acts on the token-fuel [system]; the
   supply balances live in the ADMINISTRATIVE [pos_state], written ONLY by the
   authorized mint/settlement path (DR-13: Σ⟦v⟧ is unwritable from the reducer).
   We model the joint runtime configuration as a pair [(system, pos_state)] and
   the user-reduction transition as stepping ONLY the [system] component, leaving
   [pos_state] (hence every balance) fixed. Therefore no user step can increase
   any supply balance — the balance-layer form of "user steps consume, never
   mint."                                                                       *)

Definition config : Type := (system * pos_state)%type.

Inductive user_step : config -> config -> Prop :=
  | us_reduce : forall S S' P,
      ca_step S S' ->
      user_step (S, P) (S', P).

(* The administrative state is invariant under a user step. *)
Lemma user_step_preserves_pos_state : forall S P S' P',
  user_step (S, P) (S', P') -> P' = P.
Proof.
  intros S P S' P' Hstep. inversion Hstep; subst. reflexivity.
Qed.

(* Decision 7: a user [ca_step] does not INCREASE any validator's supply
   balance — in fact it leaves every balance UNCHANGED, since balances live in
   the administrative state the reducer cannot touch. The [<=] form mirrors
   [user_ca_step_does_not_mint] at the balance layer. *)
Theorem user_ca_step_does_not_increase_balance : forall S P S' P' v,
  user_step (S, P) (S', P') ->
  balance_of P' v <= balance_of P v.
Proof.
  intros S P S' P' v Hstep.
  rewrite (user_step_preserves_pos_state S P S' P' Hstep).
  lia.
Qed.

(* Exact (equality) form: a user step leaves every supply balance fixed. *)
Corollary user_ca_step_balance_unchanged : forall S P S' P' v,
  user_step (S, P) (S', P') ->
  balance_of P' v = balance_of P v.
Proof.
  intros S P S' P' v Hstep.
  rewrite (user_step_preserves_pos_state S P S' P' Hstep). reflexivity.
Qed.
