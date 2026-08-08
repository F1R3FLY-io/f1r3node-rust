(* ===========================================================================
   ChannelNetting.v - THE GAP-1 proof: the per-channel `ChannelChange.combine`
   algebra. The shipped max-multiset-union combine is COMMUTATIVE but NOT
   ASSOCIATIVE (disclosed Finding A, EXHIBITED here as a theorem); the sum-union
   FIX is a commutative MONOID whose fold is permutation-invariant (deterministic
   netting regardless of branch fold order).

   `ChannelChange<A>::combine` (rspace++/src/rspace/merger/channel_change.rs:17)
   folds two per-channel diffs:
       added   = vec_union(self.added,   other.added)
       removed = vec_union(self.removed, other.removed)
       cancel_common(&mut added, &mut removed)
   where `vec_union` (channel_change.rs:25) keeps `left` and appends the elements
   of `right` NOT already present -- i.e. per element the multiplicity is
   max(mult_left, mult_right): an IDEMPOTENT MAX-MULTISET UNION. `cancel_common`
   then removes matched added/removed pairs (min-multiplicity cancellation).

   We model a change for a SINGLE datum as its (added, removed) multiplicity pair
   (that suffices to exhibit non-associativity, which is per-element).

   ---------------------------------------------------------------------------
   FINDING A (disclosed; NOT a code change requested here)
   ---------------------------------------------------------------------------
   Max-union is non-associative. With a = add(x), b = add(x), c = remove(x):
       (a . b) . c  =  (max-union add then cancel)  =  {}    (x cancels)
       a . (b . c)  =  add(x) combined with {}       =  {x}   (x survives)
   so the fold RESULT depends on the association (hence, in a multi-branch fold,
   on the order) -- a determinism hazard, mirrored on the arithmetic side by
   IntegerAdd.v `launder_exhibit`. Commutativity alone does NOT rescue a fold;
   associativity is required for order-independence.

   ---------------------------------------------------------------------------
   THE FIX
   ---------------------------------------------------------------------------
   Replacing max-union with SUM-union (componentwise addition of multiplicities,
   cancel deferred to a single final normalization) restores associativity: it is
   the free commutative monoid on datum multiplicities. Its fold over any branch
   PERMUTATION is identical (`netting_fold_perm`), so the netted diff is
   deterministic. `cancel` is a net-preserving normalization (`net_cancel`), so
   applying it does not disturb determinism.

   ---------------------------------------------------------------------------
   Spec-to-Code Traceability
   ---------------------------------------------------------------------------
   Rocq                          | Rust (channel_change.rs)
   ------------------------------+----------------------------------------------
   cc = (added, removed) : nat*nat | per-datum multiplicity of a ChannelChange<A>
   combine_max                   | combine = vec_union(max) x2 then cancel_common
   combine_max_comm              | combine commutativity (HOLDS)
   combine_not_assoc_exhibit     | Finding A (max-union NON-associative)
   combine_sum                   | the sum-union FIX (associative)
   netting_fold_perm             | order-independent netted diff (determinism)
   net / net_cancel              | cancel_common preserves the net multiplicity

   Stdlib-only, axiom-free. `Print Assumptions channel_netting_fixed_deterministic`
   => "Closed under the global context".
   =========================================================================== *)

From Stdlib Require Import Arith.Arith.
From Stdlib Require Import ZArith.
From Stdlib Require Import Lists.List.
From Stdlib Require Import Sorting.Permutation.
From Stdlib Require Import Lia.
Import ListNotations.

(* Per-datum ChannelChange: (added multiplicity, removed multiplicity). *)
Definition cc := (nat * nat)%type.
Definition mk_add   : cc := (1, 0).   (* add(x)    *)
Definition mk_rem   : cc := (0, 1).   (* remove(x) *)
Definition empty_cc : cc := (0, 0).   (* {}        *)

(* cancel_common: remove min(added,removed) matched pairs from both sides. *)
Definition cancel (c : cc) : cc :=
  (fst c - Nat.min (fst c) (snd c), snd c - Nat.min (fst c) (snd c)).

(* ===========================================================================
   Section 1 - the SHIPPED max-union combine: commutative but NON-associative.
   =========================================================================== *)

(* vec_union = idempotent max-multiset union; combine caps each side at the max
   then cancels the common part. *)
Definition combine_max (x y : cc) : cc :=
  cancel (Nat.max (fst x) (fst y), Nat.max (snd x) (snd y)).

Theorem combine_max_comm : forall x y, combine_max x y = combine_max y x.
Proof.
  intros [xa xr] [ya yr]. unfold combine_max, cancel; simpl.
  rewrite (Nat.max_comm xa ya), (Nat.max_comm xr yr). reflexivity.
Qed.

(* FINDING A, EXHIBITED as a theorem (mirror of IntegerAdd.launder_exhibit). *)
Theorem combine_not_assoc_exhibit :
  combine_max (combine_max mk_add mk_add) mk_rem = empty_cc
  /\ combine_max mk_add (combine_max mk_add mk_rem) = mk_add
  /\ empty_cc <> mk_add.
Proof.
  split; [vm_compute; reflexivity |].
  split; [vm_compute; reflexivity |].
  unfold empty_cc, mk_add. discriminate.
Qed.

(* ===========================================================================
   Section 2 - the SUM-union FIX: a commutative monoid, hence order-independent.
   =========================================================================== *)

Definition combine_sum (x y : cc) : cc := (fst x + fst y, snd x + snd y).

Theorem combine_sum_comm : forall x y, combine_sum x y = combine_sum y x.
Proof.
  intros [xa xr] [ya yr]. unfold combine_sum; simpl.
  rewrite (Nat.add_comm xa ya), (Nat.add_comm xr yr). reflexivity.
Qed.

(* orientation a (b c) = (a b) c, matching Merge.v's op_assoc, so the swap-case
   fold tactic below is the exact Merge.v pattern. *)
Theorem combine_sum_assoc :
  forall x y z, combine_sum x (combine_sum y z) = combine_sum (combine_sum x y) z.
Proof.
  intros [xa xr] [ya yr] [za zr]. unfold combine_sum; simpl.
  rewrite (Nat.add_assoc xa ya za), (Nat.add_assoc xr yr zr). reflexivity.
Qed.

Theorem combine_sum_id_l : forall x, combine_sum empty_cc x = x.
Proof. intros [xa xr]. unfold combine_sum, empty_cc; simpl. reflexivity. Qed.

Definition netting_fold (l : list cc) : cc := fold_right combine_sum empty_cc l.

(* THE determinism headline lemma: folding the fixed combine over ANY branch
   permutation yields the IDENTICAL netted diff (uses comm + assoc only -- the
   Merge.v `merge_perm` pattern). *)
Theorem netting_fold_perm :
  forall l1 l2, Permutation l1 l2 -> netting_fold l1 = netting_fold l2.
Proof.
  intros l1 l2 H.
  induction H as [| x l1 l2 Hp IH | x y l | l1 l2 l3 Hp1 IH1 Hp2 IH2].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
  - simpl. rewrite combine_sum_assoc, combine_sum_assoc, (combine_sum_comm y x). reflexivity.
  - rewrite IH1, IH2. reflexivity.
Qed.

(* ===========================================================================
   Section 3 - net multiplicity: cancel_common is net-preserving, and sum-union
   is a net-homomorphism, so the observable (net) is deterministic under the fix.
   =========================================================================== *)

Definition net (c : cc) : Z := (Z.of_nat (fst c) - Z.of_nat (snd c))%Z.

Theorem net_combine_sum : forall x y, net (combine_sum x y) = (net x + net y)%Z.
Proof.
  intros [xa xr] [ya yr]. unfold net, combine_sum; simpl.
  rewrite !Nat2Z.inj_add. lia.
Qed.

Theorem net_cancel : forall c, net (cancel c) = net c.
Proof.
  intros [a r]. unfold net, cancel; simpl.
  rewrite (Nat2Z.inj_sub a (Nat.min a r)) by apply Nat.le_min_l.
  rewrite (Nat2Z.inj_sub r (Nat.min a r)) by apply Nat.le_min_r.
  lia.
Qed.

(* ===========================================================================
   Section 4 - the headline: the FIXED operator is a commutative monoid whose
   fold is deterministic (permutation-invariant), and net is preserved by cancel.
   =========================================================================== *)

Theorem channel_netting_fixed_deterministic :
  (forall x y, combine_sum x y = combine_sum y x)
  /\ (forall x y z, combine_sum x (combine_sum y z) = combine_sum (combine_sum x y) z)
  /\ (forall x, combine_sum empty_cc x = x)
  /\ (forall l1 l2, Permutation l1 l2 -> netting_fold l1 = netting_fold l2)
  /\ (forall c, net (cancel c) = net c).
Proof.
  exact (conj combine_sum_comm
          (conj combine_sum_assoc
            (conj combine_sum_id_l
              (conj netting_fold_perm net_cancel)))).
Qed.

(* ===========================================================================
   Section 5 - THE DEPLOYED operator's determinism: a SEMILATTICE JOIN with a
   DEFERRED single cancel, folded in a CANONICAL order.

   The sum-union `combine_sum` (Section 2) is the associative FIX, but it is NOT
   what the node runs. This section models the DEPLOYED determinism faithfully.

   WHAT THE NODE ACTUALLY FOLDS. The shipped `ChannelChange::combine`
   (channel_change.rs:17) is `combine_max` (Section 1): a per-side MAX-multiset
   union (`vec_union`) followed by an INLINE `cancel_common`. As a fold operator
   it is COMMUTATIVE but NON-ASSOCIATIVE (`combine_not_assoc_exhibit`, Finding A).
   So its multi-branch fold is order-DEPENDENT in general. Determinism therefore
   does NOT come from associativity.

   WHERE DETERMINISM COMES FROM (the no-fork guarantee). The deployed merge
   (conflict_set_merger.rs:384-426) folds `combine_max` over a CANONICAL order:
   the branches are sorted by `compare_branches` and the items within each branch
   by `DeployChainIndex::cmp` — the injective 5-key STRICT TOTAL order proven
   node-identical in KeepOneOrder (`output_indep_of_input_perm`) — and only THEN
   are they folded left-to-right (`combined = combined.combine(item_changes)`).
   Because every node canonicalizes with the SAME permutation-invariant sort, the
   folded result is a pure function of the input SET — byte-identical regardless
   of HashSet reseeding. A non-associative operator folded in a canonical order is
   still deterministic; Finding A is rendered BENIGN by the canonical order, not
   by a semantics change (mirrors the dossier's T-FOLD).

   THE STRUCTURE ("semilattice join with deferred single cancel"). `combine_max`
   decomposes as `cancel ∘ vunion` (`combine_max_eq_cancel_join`), where the
   MAX-union `vunion` (no cancel) is an idempotent, commutative, ASSOCIATIVE
   semilattice JOIN whose fold IS order-independent (`vunion_fold_perm`). The ONLY
   non-associative part is interleaving the `cancel` between joins; deferring it
   to a SINGLE final normalization (as `cancel (vunion_fold l)`) restores genuine
   order-independence, and `cancel` is net-preserving (`net_cancel`, Section 3).
   Finding A is exactly the gap between an INLINE cancel and a DEFERRED one.

   Spec-to-Code (this section):
   Rocq                              | Rust
   ----------------------------------+----------------------------------------
   vunion (max-union, no cancel)     | vec_union (channel_change.rs:25)
   combine_max_eq_cancel_join        | combine = vec_union x2 then cancel_common
   vunion_fold_perm                  | max-union part is order-free
   deployed_fold / canon             | conflict_set_merger.rs:385,391,426 fold
   deployed_fold_canonical_...       | node-identical fold winner (no fork)
   canon perm-invariance premise     | DeployChainIndex::cmp / compare_branches
                                     |   sort = KeepOneOrder.output_indep_of_...
   =========================================================================== *)

(* The MAX-union JOIN (`vec_union`): per-datum multiplicity = max, NO cancel.
   This is the semilattice the deployed combine is built on. *)
Definition vunion (x y : cc) : cc := (Nat.max (fst x) (fst y), Nat.max (snd x) (snd y)).

Theorem vunion_comm : forall x y, vunion x y = vunion y x.
Proof.
  intros [xa xr] [ya yr]. unfold vunion; simpl.
  rewrite (Nat.max_comm xa ya), (Nat.max_comm xr yr). reflexivity.
Qed.

Theorem vunion_assoc :
  forall x y z, vunion x (vunion y z) = vunion (vunion x y) z.
Proof.
  intros [xa xr] [ya yr] [za zr]. unfold vunion; simpl.
  rewrite (Nat.max_assoc xa ya za), (Nat.max_assoc xr yr zr). reflexivity.
Qed.

Theorem vunion_idem : forall x, vunion x x = x.
Proof.
  intros [xa xr]. unfold vunion; simpl.
  rewrite (Nat.max_id xa), (Nat.max_id xr). reflexivity.
Qed.

Theorem vunion_id_l : forall x, vunion empty_cc x = x.
Proof. intros [xa xr]. unfold vunion, empty_cc; simpl. reflexivity. Qed.

(* THE structural decomposition: the shipped `combine_max` IS the semilattice
   JOIN followed by a SINGLE (deferred) `cancel`. Definitionally true — both are
   `cancel (Nat.max (fst x)(fst y), Nat.max (snd x)(snd y))`. *)
Theorem combine_max_eq_cancel_join :
  forall x y, combine_max x y = cancel (vunion x y).
Proof. reflexivity. Qed.

(* The semilattice JOIN fold is order-independent (permutation-invariant): the max
   multiplicity per datum does not depend on branch order (uses comm + assoc, the
   `netting_fold_perm` pattern). This is the sense in which the max-union part of
   the deployed combine is genuinely "order-free". *)
Definition vunion_fold (l : list cc) : cc := fold_right vunion empty_cc l.

Theorem vunion_fold_perm :
  forall l1 l2, Permutation l1 l2 -> vunion_fold l1 = vunion_fold l2.
Proof.
  intros l1 l2 H.
  induction H as [| x l1 l2 Hp IH | x y l | l1 l2 l3 Hp1 IH1 Hp2 IH2].
  - reflexivity.
  - simpl. rewrite IH. reflexivity.
  - simpl. rewrite vunion_assoc, vunion_assoc, (vunion_comm y x). reflexivity.
  - rewrite IH1, IH2. reflexivity.
Qed.

(* THE deployed-fold determinism (the shipped NO-FORK guarantee): fold the shipped
   (non-associative) `combine_max` over a CANONICAL order produced by `canon`.
   Every node canonicalizes with the SAME permutation-invariant sort, so the folded
   result is a function of the input SET — node-identical regardless of HashSet
   reseeding. The `canon` permutation-invariance is an explicit THEOREM PREMISE (a
   PROVEN property of the real sort: KeepOneOrder.output_indep_of_input_perm on
   DeployChainIndex::cmp / compare_branches), NOT an axiom — so this is exactly why
   a non-associative fold does not fork under the canonical order. *)
Definition deployed_fold (canon : list cc -> list cc) (l : list cc) : cc :=
  fold_right combine_max empty_cc (canon l).

Theorem deployed_fold_canonical_deterministic :
  forall (canon : list cc -> list cc),
    (forall l l', Permutation l l' -> canon l = canon l') ->
    forall l l', Permutation l l' -> deployed_fold canon l = deployed_fold canon l'.
Proof.
  intros canon Hcanon l l' Hperm. unfold deployed_fold.
  rewrite (Hcanon l l' Hperm). reflexivity.
Qed.

(* The headline for the DEPLOYED operator (companion to
   `channel_netting_fixed_deterministic`, which is the DEFERRED-cancel FIX): the
   shipped combine is a semilattice JOIN with a deferred single cancel, whose
   canonical-order fold is node-identical, and whose cancel preserves the net. *)
Theorem channel_netting_deployed_deterministic :
  (forall x y, combine_max x y = cancel (vunion x y))
  /\ (forall x y, vunion x y = vunion y x)
  /\ (forall x y z, vunion x (vunion y z) = vunion (vunion x y) z)
  /\ (forall x, vunion x x = x)
  /\ (forall l1 l2, Permutation l1 l2 -> vunion_fold l1 = vunion_fold l2)
  /\ (forall canon, (forall l l', Permutation l l' -> canon l = canon l') ->
        forall l l', Permutation l l' -> deployed_fold canon l = deployed_fold canon l')
  /\ (forall c, net (cancel c) = net c).
Proof.
  exact (conj combine_max_eq_cancel_join
          (conj vunion_comm
            (conj vunion_assoc
              (conj vunion_idem
                (conj vunion_fold_perm
                  (conj deployed_fold_canonical_deterministic net_cancel)))))).
Qed.

(* ===========================================================================
   Section 6 - GENUINE order-independence under the OrderDependenceGuard.

   Section 5 shows the deployed `combine_max` fold is order-independent by
   CANONICAL ORDER: every node folds the SAME sorted list, so a non-associative
   operator still yields a node-identical result. Section 6 proves a STRONGER,
   guard-gated fact: on a channel where the Rust `OrderDependenceGuard`
   (casper/src/rust/merging/conflict_set_merger.rs) does NOT trip, the SAME
   `combine_max` fold is order-independent over EVERY permutation of the per-datum
   contribution list -- not merely the canonical one.

   THE GUARD. For a NON-mergeable channel the guard trips iff some datum is
   contributed (on the added OR the removed side) by >= 2 distinct survivors.
   When it does NOT trip, the per-datum contribution list `l : list cc` satisfies
   `adds l <= 1` and `rems l <= 1`, where `adds`/`rems` total the per-side
   multiplicities. Under those bounds the sum-union net `netting_fold l` lands in
   `{(0,0),(1,0),(0,1),(1,1)}`, and the interleaved INLINE cancels of a
   `combine_max` fold coincide with the SINGLE DEFERRED `cancel` of the sum-net
   (Finding A's inline-vs-deferred gap vanishes precisely when no side exceeds 1:
   e.g. one (1,0) and one (0,1) both fold to (0,0) = cancel (1,1)). Hence the fold
   equals `cancel (netting_fold l)` REGARDLESS of order, so any two permutations
   agree -- GENUINE order-independence, not merely canonical-order determinism.

   Spec-to-Code (this section):
   Rocq                                   | Rust
   ---------------------------------------+-------------------------------------
   adds / rems (per-side multiplicity sum)| survivor added/removed contributions
   adds l <= 1 /\ rems l <= 1             | OrderDependenceGuard does NOT trip
   combine_max_cancel_step                | inline cancel = deferred cancel, per step
   deployed_fold_id_eq_cancel_netting     | (Lemma 1) whole inline fold = deferred cancel
   combine_max_order_independent_...      | (Lemma 2) fold is order-free (any permutation)
   =========================================================================== *)

(* Total per-side multiplicities of a contribution list. *)
Definition adds (l : list cc) : nat := fold_right Nat.add 0 (map fst l).
Definition rems (l : list cc) : nat := fold_right Nat.add 0 (map snd l).

Lemma adds_cons : forall x l, adds (x :: l) = fst x + adds l.
Proof. reflexivity. Qed.

Lemma rems_cons : forall x l, rems (x :: l) = snd x + rems l.
Proof. reflexivity. Qed.

(* adds / rems are permutation-invariant: each is `fold_right Nat.add 0` over a
   projected multiplicity list, folded with a commutative monoid (the
   `netting_fold_perm` pattern, on the swap case discharged by `lia`). *)
Lemma adds_perm : forall l l', Permutation l l' -> adds l = adds l'.
Proof.
  intros l l' H.
  induction H as [| x l1 l2 Hp IH | x y l0 | l1 l2 l3 H1 IH1 H2 IH2].
  - reflexivity.
  - rewrite !adds_cons, IH. reflexivity.
  - rewrite !adds_cons. lia.
  - rewrite IH1, IH2. reflexivity.
Qed.

Lemma rems_perm : forall l l', Permutation l l' -> rems l = rems l'.
Proof.
  intros l l' H.
  induction H as [| x l1 l2 Hp IH | x y l0 | l1 l2 l3 H1 IH1 H2 IH2].
  - reflexivity.
  - rewrite !rems_cons, IH. reflexivity.
  - rewrite !rems_cons. lia.
  - rewrite IH1, IH2. reflexivity.
Qed.

(* The sum-union net is exactly the pair of per-side totals -- an unconditional
   identity (holds for ALL `l`; the guard bounds are only needed downstream). *)
Lemma netting_fold_eq_adds_rems : forall l, netting_fold l = (adds l, rems l).
Proof.
  induction l as [| x l' IH].
  - reflexivity.
  - change (netting_fold (x :: l')) with (combine_sum x (netting_fold l')).
    rewrite IH, adds_cons, rems_cons. unfold combine_sum. simpl. reflexivity.
Qed.

(* THE per-step algebra (where Finding A becomes benign): with neither side
   exceeding 1 across the head `(a,r)` and the already-netted tail `(A,R)`, one
   INLINE `combine_max` step over the deferred cancel of `(A,R)` equals the
   DEFERRED cancel of the summed pair. Proven by exhausting the {0,1} values --
   every larger value contradicts a bound (`exfalso; lia`), and the surviving
   nine concrete cases compute by `reflexivity`. *)
Lemma combine_max_cancel_step :
  forall a r A R, a + A <= 1 -> r + R <= 1 ->
    combine_max (a, r) (cancel (A, R)) = cancel (a + A, r + R).
Proof.
  intros a r A R Ha Hr.
  destruct a as [|[|a]]; try (exfalso; lia);
  destruct A as [|[|A]]; try (exfalso; lia);
  destruct r as [|[|r]]; try (exfalso; lia);
  destruct R as [|[|R]]; try (exfalso; lia);
  reflexivity.
Qed.

(* Core induction for LEMMA 1 (on the bare fold): under the guard bounds, the
   identity-order `combine_max` fold (interleaved INLINE cancels) equals the
   SINGLE DEFERRED cancel of the sum-union net. *)
Lemma combine_max_fold_eq_cancel_netting :
  forall l, adds l <= 1 -> rems l <= 1 ->
    fold_right combine_max empty_cc l = cancel (netting_fold l).
Proof.
  intros l. induction l as [| x l' IH]; intros Ha Hr.
  - reflexivity.
  - destruct x as [a r].
    rewrite adds_cons in Ha. rewrite rems_cons in Hr. simpl in Ha, Hr.
    assert (Ha' : adds l' <= 1) by lia.
    assert (Hr' : rems l' <= 1) by lia.
    specialize (IH Ha' Hr').
    change (fold_right combine_max empty_cc ((a, r) :: l'))
      with (combine_max (a, r) (fold_right combine_max empty_cc l')).
    change (netting_fold ((a, r) :: l'))
      with (combine_sum (a, r) (netting_fold l')).
    rewrite IH.
    rewrite (netting_fold_eq_adds_rems l').
    change (combine_sum (a, r) (adds l', rems l')) with (a + adds l', r + rems l').
    apply combine_max_cancel_step; assumption.
Qed.

(* ---------------------------------------------------------------------------
   LEMMA 1 (structural): the deployed fold under the IDENTITY canon equals the
   single deferred cancel of the sum-union net, whenever the guard does not trip.
   --------------------------------------------------------------------------- *)
Lemma deployed_fold_id_eq_cancel_netting :
  forall l, adds l <= 1 -> rems l <= 1 ->
    deployed_fold (fun x => x) l = cancel (netting_fold l).
Proof.
  intros l Ha Hr. unfold deployed_fold. cbv beta.
  apply combine_max_fold_eq_cancel_netting; assumption.
Qed.

(* ---------------------------------------------------------------------------
   LEMMA 2 (headline): when the OrderDependenceGuard does not trip
   (`adds l <= 1 /\ rems l <= 1`), the shipped `combine_max` fold is GENUINELY
   order-independent -- equal on ANY permutation, not merely the canonical order.
   Both sides reduce (Lemma 1) to the deferred cancel of the sum-net, which is
   permutation-invariant (`netting_fold_perm`); the bounds transfer to the
   permuted list because `adds`/`rems` are permutation-invariant.
   --------------------------------------------------------------------------- *)
Theorem combine_max_order_independent_under_no_dup :
  forall l l', Permutation l l' -> adds l <= 1 -> rems l <= 1 ->
    deployed_fold (fun x => x) l = deployed_fold (fun x => x) l'.
Proof.
  intros l l' Hperm Ha Hr.
  assert (Ha' : adds l' <= 1) by (rewrite <- (adds_perm l l' Hperm); exact Ha).
  assert (Hr' : rems l' <= 1) by (rewrite <- (rems_perm l l' Hperm); exact Hr).
  rewrite (deployed_fold_id_eq_cancel_netting l Ha Hr).
  rewrite (deployed_fold_id_eq_cancel_netting l' Ha' Hr').
  rewrite (netting_fold_perm l l' Hperm).
  reflexivity.
Qed.
