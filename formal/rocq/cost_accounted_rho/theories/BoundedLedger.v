(* ═══════════════════════════════════════════════════════════════════════════
   BoundedLedger.v — the i64-bounded phlogiston ledger: over/underflow MODELED
   ═══════════════════════════════════════════════════════════════════════════

   [item 2505 re-modeling + item 2494 formal mirror]

   The nat conservation theorems (TokenConservation.v, MintingInjection.v) are
   sound but domain-EXCLUDE the adversarial branch the runtime actually guards:
   [nat] is unbounded, so a CREDIT [+] can never overflow, and the DEBIT guard is
   only reachable via the [draw <= pre] premise those theorems ASSUME. The Rust
   runtime (casper/.../costacc/close_block_deploy.rs::dual_write_supply) holds each
   pool balance as an [i64] in [[0, i64::MAX]] and updates it with [checked_add] /
   [checked_sub] — both of which return [None] at the machine boundary, which the
   caller turns into a DETERMINISTIC block rejection (never a panic; item 2494).

   This module re-models the ledger quantity in [Z], bounded to the i64
   non-negative range, so BOTH branches are REPRESENTABLE and PROVEN:

     - CREDIT (item 2494): [checked_add_i64 old amt] = [Some (old+amt)] in range
       (conservation, never a wrap) or [None] on overflow (deterministic reject).
     - DEBIT: [checked_sub_nonneg pre draw] = [Some (pre-draw)] when [draw <= pre]
       (conservation, non-negative) or [None] when the gate is violated (reject).

   The [draw <= pre] premise the nat settlement theorems ASSUME is here DISCHARGED
   by the checked op returning [None]. The nat model is recovered as the in-range
   (happy-path) restriction via [Z.of_nat] ([checked_add_i64_matches_nat]), so NO
   existing guarantee is weakened — the bounded layer strictly ADDS the
   adversarial-branch modeling. Concrete (ZArith + the nat bridges); axiom-free so
   every headline is Closed under the global context.

   The i64 bound IS the supply cap: there is deliberately NO economic [SUPPLY_MAX]
   constant (that would be a business parameter — item 2494 prefers the technical
   machine bound), and the invariant is "Sigma operations use checked arithmetic
   that deterministically ERRORS on overflow (never wraps, never panics)".

   ─────────────────────────────────────────────────────────────────────────
   Rocq Theorem                                     | Code / Paper property
   ──────────────────────────────────────────────────┼────────────────────────────
   checked_add_i64_conserved_or_rejected             | credit: Sigma conserved OR reject
   checked_add_i64_never_wraps                        | checked_add never wraps/loses
   checked_add_i64_none_iff_overflow                  | reject iff overflow
   checked_sub_nonneg_conserved_or_rejected           | debit: Sigma conserved OR reject
   checked_add_i64_matches_nat                         | nat model = in-range restriction
   supply_credit_conserved_or_rejected                | dual_write_supply mint/convert/
                                                      |   collect loop (item 2494)
   bounded_settlement_conserved_or_rejected            | settle_balance / settlement_conserves
   bounded_fee_convert_conserved_or_rejected           | fb_convert / fee_convert_*_backed
   ─────────────────────────────────────────────────────────────────────────

   Dependencies: Rocq 9.1.x ZArith, TokenConservation, MintingInjection.
   ═══════════════════════════════════════════════════════════════════════════ *)

From Stdlib Require Import ZArith Lia Bool.Bool Lists.List.
From Stdlib Require Import Arith.PeanoNat.
Import ListNotations.

From CostAccountedRho Require Import TokenConservation.
From CostAccountedRho Require Import MintingInjection.

Open Scope Z_scope.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 1: The i64 range and the checked operations
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The i64 ceiling — the phlogiston supply cap is the MACHINE bound, not an
   economic constant. *)
Definition i64_max : Z := 2 ^ 63 - 1.

(* A ledger quantity is in the i64 non-negative range the runtime maintains. *)
Definition in_i64 (z : Z) : Prop := 0 <= z <= i64_max.

(* Faithful model of Rust [i64::checked_add] on the NON-NEGATIVE balance domain:
   the sum is exact while it stays [<= i64_max], else [None] (overflow). Both
   operands are [>= 0], so the lower i64 bound cannot be crossed — only the ceiling
   matters. This is EXACTLY [close_block_deploy::checked_supply_credit]. *)
Definition checked_add_i64 (x y : Z) : option Z :=
  if x + y <=? i64_max then Some (x + y) else None.

(* Faithful model of the supply DEBIT the runtime performs (read-modify-replace
   under the gate guarantee [draw <= pre]): the difference is exact and
   non-negative when [y <= x], else [None] (the gate-invariant violation the
   runtime rejects deterministically). *)
Definition checked_sub_nonneg (x y : Z) : option Z :=
  if y <=? x then Some (x - y) else None.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 2: The CREDIT dichotomy (item 2494 — overflow is deterministic reject)
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The DEFINING dichotomy of the checked credit: for any non-negative old balance
   and non-negative addend, EITHER the credit succeeds with the EXACT sum (which is
   in range — conservation, never a wrap) OR it fails with [None] (overflow — the
   block is deterministically rejected). There is no third outcome. *)
Theorem checked_add_i64_conserved_or_rejected : forall x y,
  0 <= x -> 0 <= y ->
  (checked_add_i64 x y = Some (x + y) /\ in_i64 (x + y))
  \/ (checked_add_i64 x y = None /\ x + y > i64_max).
Proof.
  intros x y Hx Hy. unfold checked_add_i64, in_i64.
  destruct (x + y <=? i64_max) eqn:E.
  - apply Z.leb_le in E. left. split; [reflexivity | lia].
  - apply Z.leb_gt in E. right. split; [reflexivity | lia].
Qed.

(* Never wraps: whenever the checked credit RETURNS a balance, that balance is the
   EXACT mathematical sum (the anti-silent-corruption property — the result is
   old+amt, not a 2^64-modular wrap). *)
Theorem checked_add_i64_never_wraps : forall x y s,
  checked_add_i64 x y = Some s -> s = x + y.
Proof.
  intros x y s H. unfold checked_add_i64 in H.
  destruct (x + y <=? i64_max); [injection H; auto | discriminate].
Qed.

(* Rejection is EXACTLY the overflow case. *)
Theorem checked_add_i64_none_iff_overflow : forall x y,
  checked_add_i64 x y = None <-> x + y > i64_max.
Proof.
  intros x y. unfold checked_add_i64.
  destruct (x + y <=? i64_max) eqn:E.
  - apply Z.leb_le in E. split; intro H; [discriminate H | exfalso; lia].
  - apply Z.leb_gt in E. split; intro H; [lia | reflexivity].
Qed.

(* A successful credit stays within i64 (given non-negative in-range operands). *)
Theorem checked_add_i64_some_in_range : forall x y s,
  0 <= x -> 0 <= y ->
  checked_add_i64 x y = Some s -> in_i64 s.
Proof.
  intros x y s Hx Hy H.
  pose proof (checked_add_i64_never_wraps _ _ _ H) as Hs.
  unfold checked_add_i64 in H. unfold in_i64.
  destruct (x + y <=? i64_max) eqn:E; [| discriminate].
  apply Z.leb_le in E. subst s. lia.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 3: The DEBIT dichotomy (underflow is deterministic reject)
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The debit dichotomy: EITHER the draw fits ([draw <= pre], the gate guarantee)
   and the debit succeeds with the EXACT non-negative remainder that RECOVERS the
   pre-balance ([post + draw = pre] — conservation), OR the draw exceeds the pool
   and the debit is rejected ([None]). The [draw <= pre] premise the nat settlement
   theorems ASSUME is here DISCHARGED by the op returning [None]. *)
Theorem checked_sub_nonneg_conserved_or_rejected : forall pre draw,
  in_i64 pre -> 0 <= draw ->
  (exists post, checked_sub_nonneg pre draw = Some post
                /\ post + draw = pre /\ in_i64 post /\ draw <= pre)
  \/ (checked_sub_nonneg pre draw = None /\ draw > pre).
Proof.
  intros pre draw Hpre Hdraw. unfold checked_sub_nonneg, in_i64 in *.
  destruct (draw <=? pre) eqn:E.
  - apply Z.leb_le in E. left. exists (pre - draw).
    split; [reflexivity | repeat split; lia].
  - apply Z.leb_gt in E. right. split; [reflexivity | lia].
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 4: The nat model is the in-range restriction (no guarantee weakened)
   ═══════════════════════════════════════════════════════════════════════════ *)

(* The bounded model AGREES with the nat model on the in-range (happy-path)
   subdomain: a nat credit whose sum fits i64 is the checked credit's [Some], and
   its value is the nat sum embedded by [Z.of_nat]. So the existing nat conservation
   (TokenConservation / MintingInjection) is EXACTLY the non-overflowing restriction
   of the bounded ledger — the bounded layer only ADDS the overflow branch. *)
Theorem checked_add_i64_matches_nat : forall a b : nat,
  (Z.of_nat a + Z.of_nat b <= i64_max) ->
  checked_add_i64 (Z.of_nat a) (Z.of_nat b) = Some (Z.of_nat (a + b)%nat).
Proof.
  intros a b Hle. unfold checked_add_i64.
  apply Z.leb_le in Hle. rewrite Hle.
  rewrite Nat2Z.inj_add. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 5: The applied headline — the dual_write_supply credit loop (item 2494)
   ═══════════════════════════════════════════════════════════════════════════

   [dual_write_supply] folds a list of credits (the mint list, the fee carve, the
   collection, the convert, the client seed) onto pool balances via the checked
   credit, [?]-propagating the first overflow as the deterministic rejection.     *)

(* Apply a list of non-negative credits to a pool, short-circuiting to [None] on the
   first overflow — the exact shape of the mint/convert/collect loop. *)
Fixpoint apply_credits (old : Z) (amts : list Z) : option Z :=
  match amts with
  | [] => Some old
  | a :: rest =>
      match checked_add_i64 old a with
      | Some mid => apply_credits mid rest
      | None => None
      end
  end.

Fixpoint zsum (l : list Z) : Z :=
  match l with
  | [] => 0
  | a :: r => a + zsum r
  end.

(* Item 2494 headline (the Rocq mirror of the Rust [checked_supply_credit] guard):
   folding the block's credits onto a pool EITHER conserves — the final balance is
   EXACTLY the pre-balance plus the total credited (no wrap, no loss) — OR it is
   deterministically REJECTED ([None], exactly as [dual_write_supply] returns the
   [ReplaySupplyOverflow] error and every node rejects the same block the same way).
   "The ledger sum is conserved OR the block is deterministically rejected on
   overflow." *)
Theorem supply_credit_conserved_or_rejected : forall amts old,
  0 <= old -> Forall (fun a => 0 <= a) amts ->
  apply_credits old amts = Some (old + zsum amts)
  \/ apply_credits old amts = None.
Proof.
  induction amts as [| a rest IH]; intros old Hold Hall.
  - simpl. left. f_equal. lia.
  - pose proof (Forall_inv Hall) as Ha. cbv beta in Ha.
    pose proof (Forall_inv_tail Hall) as Hrest.
    simpl.
    destruct (checked_add_i64 old a) as [mid |] eqn:E.
    + pose proof (checked_add_i64_never_wraps _ _ _ E) as Hmid.
      assert (Hmidnn : 0 <= mid) by lia.
      destruct (IH mid Hmidnn Hrest) as [Hok | Hno].
      * left. rewrite Hok. f_equal. simpl. lia.
      * right. exact Hno.
    + right. reflexivity.
Qed.

(* ═══════════════════════════════════════════════════════════════════════════
   Section 6: Bounded restatements of the named balance-layer conservation laws
   ═══════════════════════════════════════════════════════════════════════════ *)

(* Bounded settlement debit (TokenConservation.settle_balance / settlement_conserves
   at the i64 layer): debiting the admitted demand [Delta] from the pool EITHER
   conserves ([post + Delta = pre] — the nat [settlement_conserves] is this
   Some-branch, with its [Delta <= pre] premise now DISCHARGED by the checked op)
   OR is deterministically rejected. *)
Theorem bounded_settlement_conserved_or_rejected : forall pre demand,
  in_i64 pre -> 0 <= demand ->
  (exists post, checked_sub_nonneg pre demand = Some post /\ post + demand = pre)
  \/ checked_sub_nonneg pre demand = None.
Proof.
  intros pre demand Hpre Hd.
  destruct (checked_sub_nonneg_conserved_or_rejected pre demand Hpre Hd)
    as [[post [Hsome [Hconserve _]]] | [Hnone _]].
  - left. exists post. split; assumption.
  - right. exact Hnone.
Qed.

(* Bounded fee->v convert (MintingInjection.fb_convert / fee_convert_credit_is_backed
   at the i64 layer): crediting [Sigma(v)] by the collected fee [f] and zeroing
   [F_v] EITHER conserves the [F_v + Sigma(v)] holding with the exact 1:1 peg (the
   fees that leave F_v EXACTLY enter Sigma(v); nat [fee_convert_conserves_holding]
   is this Some-branch) OR overflows and is deterministically rejected. The credit
   is BACKED (bounded by the drained fee), never an unbacked mint. *)
Theorem bounded_fee_convert_conserved_or_rejected : forall fees supply,
  0 <= fees -> 0 <= supply ->
  match checked_add_i64 supply fees with
  | Some supply' => supply' + 0 = supply + fees /\ in_i64 supply'
  | None => supply + fees > i64_max
  end.
Proof.
  intros fees supply Hf Hs.
  destruct (checked_add_i64_conserved_or_rejected supply fees Hs Hf)
    as [[Hsome Hin] | [Hnone Hover]].
  - rewrite Hsome. split; [lia | exact Hin].
  - rewrite Hnone. exact Hover.
Qed.
