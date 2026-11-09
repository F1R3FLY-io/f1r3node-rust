(* G2 (DR-65 items 1 to 6, design sections 8.3 and 8.4, option B): several
   offered-funded deploys in one block.

   A batch is a list of members in canonical order. The decision at the
   block pre-state R0 first rejects every candidate that fails alone. The
   other candidates that share a key (a canonical custody, or a grant that
   two members draw) form one funding group. A group whose holds on some
   key exceed the supply of that key at R0 is rejected, all or none.

   Member i of the accepted batch funds from R0 and executes from R_{i-1}.
   Under option B its selection capacity on each own purse is the balance
   at R_{i-1} minus the holds of the later members, on the success path and
   on the rollback path. The held-capacity guard of HeldCapacityGuard.v at
   the settlement root decides between a settlement and a charged rollback.

   The module proves these results, with no axiom:
   - batch_held_capacity_guard_preserves_later_holds: the batch form of
     held_capacity_guard_preserves_later_holds. Along the canonical list on
     one purse, the balance before each member covers its own hold and the
     later holds, and the balance after it covers the later holds.
   - group_holds_prevent_overdraft: when the supply covers the sum of the
     holds, every charge fits its balance, and the cap never falls below the
     member's own hold.
   - group_decision_permutation_invariant: the decision does not depend on
     the order of the candidates.
   - subset_of_accepted_group_is_accepted and guard_monotone_in_later_holds:
     the two facts of the truncation argument. A shipped prefix of an
     accepted batch is accepted again, and a guard that holds with the later
     holds of the full batch holds with the fewer later holds of a prefix.
   - accepted_members_fit_every_purse: the decision gives each purse the
     premise of the two batch theorems.
   Four proved negative controls refute four wrong rules: prefix admission,
   independent full capacity, a rollback without the cap, and a kept guard
   rollback after a truncation.

   Formulations that were rejected before any proof attempt (reasoned):
   - A rollback premise of the form charge <= hold for each purse is too
     strong. The selection can move a charge from one purse to another, so
     the charge on one purse can exceed the hold on that purse. The cap of
     option B is the premise that every placement satisfies.
   - A charges-fit statement without the hold-below-cap clause is trivial:
     each clause follows from the rule of the same member. The clause that
     needs the induction is that the cap is never below the own hold.
   - A fuel that depends on the order of the pool is not needed. The fuel is
     the length of the pool, and a permutation keeps the length.
   - A sum over a sublist in the sense of incl is not bounded by the sum of
     the larger list when the sublist repeats an element. The proofs use an
     ordered subsequence relation instead, which keeps multiplicities.

   Proof steps that failed in this development (attempted):
   - simpl in a hypothesis of the form guard [entry] = true unfolds it to
     guard_at entry && true = true. The lemmas of HeldCapacityGuard.v then
     do not apply. A change back to guard [entry] = true restores them.
   - Nat.eqb_refl and Nat.eqb_eq are not in scope with NArith alone. The
     module imports Arith.PeanoNat for them.
   - In the negative controls, simpl followed by an intro pattern gave
     hypotheses on closed amounts that lia rejected with the message
     Cannot find witness. A cbv with an explicit list of the definitions of
     this module keeps the amounts in a form that lia accepts. *)

From Stdlib Require Import NArith Bool.Bool Lists.List Lia Sorting.Permutation Arith.PeanoNat.
From CostAccountedRho Require Import HeldCapacityGuard.
Import ListNotations.
Open Scope N_scope.

(** * List facts *)

Lemma permutation_filter : forall (A : Type) (f : A -> bool) (l l' : list A),
  Permutation l l' -> Permutation (filter f l) (filter f l').
Proof.
  intros A f l l' perm.
  induction perm as [|x l1 l2 _ IH|x y l1|l1 l2 l3 _ IH1 _ IH2]; simpl.
  - constructor.
  - destruct (f x); [apply perm_skip|]; exact IH.
  - destruct (f x), (f y); first [apply perm_swap | apply Permutation_refl].
  - eapply Permutation_trans; eassumption.
Qed.

Lemma existsb_permutation : forall (A : Type) (f : A -> bool) (l l' : list A),
  Permutation l l' -> existsb f l = existsb f l'.
Proof.
  intros A f l l' perm.
  induction perm as [|x l1 l2 _ IH|x y l1|l1 l2 l3 _ IH1 _ IH2]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
  - destruct (f x), (f y); reflexivity.
  - rewrite IH1. exact IH2.
Qed.

Lemma forallb_permutation : forall (A : Type) (f : A -> bool) (l l' : list A),
  Permutation l l' -> forallb f l = forallb f l'.
Proof.
  intros A f l l' perm.
  induction perm as [|x l1 l2 _ IH|x y l1|l1 l2 l3 _ IH1 _ IH2]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
  - destruct (f x), (f y); reflexivity.
  - rewrite IH1. exact IH2.
Qed.

Lemma existsb_pointwise : forall (A : Type) (f g : A -> bool) (l : list A),
  (forall x, f x = g x) -> existsb f l = existsb g l.
Proof.
  intros A f g l same. induction l as [|x l IH]; simpl; [reflexivity|].
  rewrite same, IH. reflexivity.
Qed.

Lemma forallb_pointwise : forall (A : Type) (f g : A -> bool) (l : list A),
  (forall x, f x = g x) -> forallb f l = forallb g l.
Proof.
  intros A f g l same. induction l as [|x l IH]; simpl; [reflexivity|].
  rewrite same, IH. reflexivity.
Qed.

Lemma filter_keeps_all : forall (A : Type) (f : A -> bool) (l : list A),
  (forall x, In x l -> f x = true) -> filter f l = l.
Proof.
  intros A f l all. induction l as [|x l IH]; simpl; [reflexivity|].
  rewrite (all x (or_introl eq_refl)). f_equal.
  apply IH. intros y member. apply all. right. exact member.
Qed.

(** The sum of a list of amounts. *)
Fixpoint nsum (amounts : list N) : N :=
  match amounts with
  | [] => 0
  | amount :: rest => amount + nsum rest
  end.

Lemma nsum_permutation : forall amounts amounts',
  Permutation amounts amounts' -> nsum amounts = nsum amounts'.
Proof.
  intros amounts amounts' perm.
  induction perm as [|x l1 l2 _ IH|x y l1|l1 l2 l3 _ IH1 _ IH2]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
  - lia.
  - rewrite IH1. exact IH2.
Qed.

(** An ordered subsequence: [subsequence s l] keeps some elements of [l] in
    their order and with their multiplicities. *)
Inductive subsequence {A : Type} : list A -> list A -> Prop :=
| subsequence_nil : subsequence [] []
| subsequence_keep : forall x s l, subsequence s l -> subsequence (x :: s) (x :: l)
| subsequence_skip : forall x s l, subsequence s l -> subsequence s (x :: l).

Lemma subsequence_nil_l : forall (A : Type) (l : list A), subsequence [] l.
Proof.
  intros A l. induction l as [|x l IH].
  - apply subsequence_nil.
  - apply subsequence_skip. exact IH.
Qed.

Lemma subsequence_refl : forall (A : Type) (l : list A), subsequence l l.
Proof.
  intros A l. induction l as [|x l IH].
  - apply subsequence_nil.
  - apply subsequence_keep. exact IH.
Qed.

Lemma subsequence_in : forall (A : Type) (s l : list A) (x : A),
  subsequence s l -> In x s -> In x l.
Proof.
  intros A s l x sub. induction sub as [|y s0 l0 _ IH|y s0 l0 _ IH]; simpl; intros member.
  - exact member.
  - destruct member as [same|later]; [left; exact same | right; exact (IH later)].
  - right. exact (IH member).
Qed.

Lemma subsequence_length : forall (A : Type) (s l : list A),
  subsequence s l -> (length s <= length l)%nat.
Proof.
  intros A s l sub. induction sub; simpl; lia.
Qed.

Lemma subsequence_trans : forall (A : Type) (a b c : list A),
  subsequence a b -> subsequence b c -> subsequence a c.
Proof.
  intros A a b c ab bc. revert a ab.
  induction bc as [|x s0 l0 _ IH|x s0 l0 _ IH]; intros a ab.
  - exact ab.
  - inversion ab as [|y s' l' sub'|y s' l' sub']; subst.
    + apply subsequence_keep. apply IH. exact sub'.
    + apply subsequence_skip. apply IH. exact sub'.
  - apply subsequence_skip. apply IH. exact ab.
Qed.

Lemma subsequence_filter_mono : forall (A : Type) (f g : A -> bool) (s l : list A),
  subsequence s l ->
  (forall x, In x s -> f x = true -> g x = true) ->
  subsequence (filter f s) (filter g l).
Proof.
  intros A f g s l sub. induction sub as [|x s0 l0 _ IH|x s0 l0 _ IH]; intros implies; simpl.
  - apply subsequence_nil.
  - assert (tail : forall y, In y s0 -> f y = true -> g y = true).
    { intros y member. apply implies. right. exact member. }
    destruct (f x) eqn:fx.
    + rewrite (implies x (or_introl eq_refl) fx).
      apply subsequence_keep. exact (IH tail).
    + destruct (g x); [apply subsequence_skip|]; exact (IH tail).
  - destruct (g x); [apply subsequence_skip|]; exact (IH implies).
Qed.

Lemma subsequence_firstn : forall (A : Type) (n : nat) (l : list A),
  subsequence (firstn n l) l.
Proof.
  intros A n. induction n as [|n IH]; intros l; simpl.
  - apply subsequence_nil_l.
  - destruct l as [|x l].
    + apply subsequence_nil.
    + apply subsequence_keep. apply IH.
Qed.

Lemma subsequence_map : forall (A B : Type) (f : A -> B) (s l : list A),
  subsequence s l -> subsequence (map f s) (map f l).
Proof.
  intros A B f s l sub. induction sub as [|x s0 l0 _ IH|x s0 l0 _ IH]; simpl.
  - apply subsequence_nil.
  - apply subsequence_keep. exact IH.
  - apply subsequence_skip. exact IH.
Qed.

Lemma nsum_subsequence : forall s l, subsequence s l -> nsum s <= nsum l.
Proof.
  intros s l sub. induction sub; simpl; lia.
Qed.

(** * The batch on one purse *)

(** How one member settles, as one purse [p] sees it.
    - [SettledOnPurse after_run success_debit]: the guard of the member held.
      The run left [after_run] on [p] at the settlement root, and the
      settlement debits [success_debit] on [p].
    - [RolledBackOnPurse rollback_charge]: the run failed, or its guard failed
      on some purse. The member rolls back to its execution root R_{i-1} and
      pays [rollback_charge] on [p].
    The run can drain or fill [p], also when [p] is outside the custody of
    the member, so [after_run] is free. *)
Inductive purse_settlement :=
| SettledOnPurse (after_run success_debit : N)
| RolledBackOnPurse (rollback_charge : N).

(** A member of the batch on [p]: its hold on [p] at R0 and its settlement. *)
Record batch_member := {
  member_hold : N;
  member_settlement : purse_settlement;
}.

(** The sum of the holds of [rest], the members after member i, on [p]. *)
Fixpoint later_sum (rest : list batch_member) : N :=
  match rest with
  | [] => 0
  | m :: tail => member_hold m + later_sum tail
  end.

(** The balance of [p] after member [m], from the balance [restored] at its
    execution root. *)
Definition balance_after_member (restored : N) (m : batch_member) : N :=
  match member_settlement m with
  | SettledOnPurse after_run success_debit => after_run - success_debit
  | RolledBackOnPurse rollback_charge => restored - rollback_charge
  end.

(** The entry of [p] in the guard of member i (HeldCapacityGuard.v). *)
Definition guard_entry (after_run success_debit later : N) : purse :=
  {| balance_after := after_run; debit := success_debit; later_holds := later |}.

(** The selection capacity of option B on [p]: the balance at R_{i-1} minus
    the later holds. *)
Definition option_b_cap (restored later : N) : N := restored - later.

(** The rule of option B along the canonical list. A settled member passed
    its guard on [p], and both paths place their charge within the cap. *)
Fixpoint follows_option_b (restored : N) (members : list batch_member) : Prop :=
  match members with
  | [] => True
  | m :: rest =>
      match member_settlement m with
      | SettledOnPurse after_run success_debit =>
          guard [guard_entry after_run success_debit (later_sum rest)] = true /\
          success_debit <= option_b_cap restored (later_sum rest)
      | RolledBackOnPurse rollback_charge =>
          rollback_charge <= option_b_cap restored (later_sum rest)
      end /\
      follows_option_b (balance_after_member restored m) rest
  end.

(** The balance before each member covers its own hold and the later holds,
    and the balance after it covers the later holds. *)
Fixpoint covers_later_holds (restored : N) (members : list batch_member) : Prop :=
  match members with
  | [] => True
  | m :: rest =>
      member_hold m + later_sum rest <= restored /\
      later_sum rest <= balance_after_member restored m /\
      covers_later_holds (balance_after_member restored m) rest
  end.

(** The batch form of [held_capacity_guard_preserves_later_holds]. If the
    balance at the execution root of the first member covers every hold, then
    along the canonical list the balance after each member covers the holds
    of the later members. A settled member keeps them by its guard, and the
    lemma of HeldCapacityGuard.v gives the step. A rolled-back member keeps
    them by the cap. *)
Theorem batch_held_capacity_guard_preserves_later_holds : forall members restored,
  later_sum members <= restored ->
  follows_option_b restored members ->
  covers_later_holds restored members.
Proof.
  induction members as [|m rest IH]; intros restored covered rule; [exact I|].
  simpl in covered, rule |- *. destruct rule as [step tail].
  assert (kept : later_sum rest <= balance_after_member restored m).
  { revert step. unfold balance_after_member.
    destruct (member_settlement m) as [after_run success_debit|rollback_charge];
      intros step.
    - destruct step as [held _].
      change (guard [guard_entry after_run success_debit (later_sum rest)] = true) in held.
      pose proof (Forall_inv (held_capacity_guard_preserves_later_holds _ held)) as fits.
      unfold remaining in fits. simpl in fits. exact fits.
    - unfold option_b_cap in step. lia. }
  split; [exact covered|]. split; [exact kept|].
  apply IH; [exact kept | exact tail].
Qed.

(** Every charge of the batch fits its balance, and the cap of each member
    is at least its own hold. *)
Fixpoint charges_fit (restored : N) (members : list batch_member) : Prop :=
  match members with
  | [] => True
  | m :: rest =>
      member_hold m <= option_b_cap restored (later_sum rest) /\
      match member_settlement m with
      | SettledOnPurse after_run success_debit => success_debit <= after_run
      | RolledBackOnPurse rollback_charge => rollback_charge <= restored
      end /\
      charges_fit (balance_after_member restored m) rest
  end.

(** The group holds prevent an overdraft. The group check gives
    supply >= the sum of the holds on [p]. Then, by induction over the
    canonical list, a guarded success debit fits the balance at its
    settlement root (the guard implies the condition of SystemVault), and a
    rollback charge within the cap fits the restored balance. The cap is
    never below the member's own hold, so a rollback charge of at most
    min(hold, cap) is a charge of at most the hold (see
    [rollback_charge_at_most_the_hold_is_within_the_cap]). *)
Theorem group_holds_prevent_overdraft : forall members supply,
  later_sum members <= supply ->
  follows_option_b supply members ->
  charges_fit supply members.
Proof.
  intros members supply fits rule.
  pose proof (batch_held_capacity_guard_preserves_later_holds members supply fits rule)
    as covered.
  clear fits. revert supply rule covered.
  induction members as [|m rest IH]; intros restored rule covered; [exact I|].
  simpl in rule, covered |- *.
  destruct rule as [step tail]. destruct covered as [before [_ rest_covered]].
  split; [unfold option_b_cap; lia|].
  split.
  - revert step.
    destruct (member_settlement m) as [after_run success_debit|rollback_charge];
      intros step.
    + destruct step as [held _].
      change (guard [guard_entry after_run success_debit (later_sum rest)] = true) in held.
      pose proof (guard_implies_vault_reservation _ held) as vault.
      unfold vault_condition, vault_at in vault. simpl in vault.
      apply andb_prop in vault. destruct vault as [vault _].
      apply N.leb_le in vault. exact vault.
    + unfold option_b_cap in step. lia.
  - apply IH; assumption.
Qed.

(** In a covered state, a rollback charge of at most the hold is at most
    min(hold, cap), and min(hold, cap) is the hold. *)
Corollary rollback_charge_at_most_the_hold_is_within_the_cap :
  forall hold later restored rollback_charge,
  hold + later <= restored ->
  rollback_charge <= hold ->
  rollback_charge <= N.min hold (option_b_cap restored later) /\
  N.min hold (option_b_cap restored later) = hold.
Proof.
  intros hold later restored rollback_charge covered within.
  unfold option_b_cap.
  assert (cap : hold <= restored - later) by lia.
  rewrite (N.min_l _ _ cap). split; [exact within | reflexivity].
Qed.

(** * The decision at R0 *)

(** A candidate of the batch. [cid] is its canonical identity. [ckeys] lists
    its canonical custodies and the grants that it draws (O-8). [chold] is
    its hold on each key. [cvalid] stands for the checks of DR-64 that do not
    depend on the holds. *)
Record candidate := {
  cid : nat;
  ckeys : list nat;
  chold : nat -> N;
  cvalid : bool;
}.

(** The check of one candidate on its own keys (DR-64). *)
Definition alone_ok (supply : nat -> N) (c : candidate) : bool :=
  cvalid c && forallb (fun k => chold c k <=? supply k) (ckeys c).

(** The candidates that pass alone. They are the pool of the grouping. *)
Definition alone_survivors (supply : nat -> N) (l : list candidate) : list candidate :=
  filter (alone_ok supply) l.

Definition has_key (k : nat) (c : candidate) : bool := existsb (Nat.eqb k) (ckeys c).

(** Two candidates share a key. *)
Definition linked (a b : candidate) : bool := existsb (fun k => has_key k b) (ckeys a).

(** The reach of [a] within [pool] by paths of at most [fuel] links. *)
Fixpoint connected_within (fuel : nat) (pool : list candidate) (a b : candidate) : bool :=
  match fuel with
  | O => Nat.eqb (cid a) (cid b)
  | S fuel' =>
      Nat.eqb (cid a) (cid b) ||
      existsb (fun c => linked a c && connected_within fuel' pool c b) pool
  end.

(** The group of [a]: the members of the pool that [a] reaches. The fuel is
    the length of the pool. A path through distinct members of the pool has
    fewer links than the pool has members. The proofs below need only that
    two orders of one pool give the same fuel. *)
Definition group_of (pool : list candidate) (a : candidate) : list candidate :=
  filter (connected_within (length pool) pool a) pool.

(** The sum of the holds of a group on key [k]. *)
Definition sum_on (k : nat) (g : list candidate) : N := nsum (map (fun c => chold c k) g).

(** A group fits when, on every key of every member, the sum of its holds is
    at most the supply (DR-65 item 4). *)
Definition group_fits (supply : nat -> N) (g : list candidate) : bool :=
  forallb (fun c => forallb (fun k => sum_on k g <=? supply k) (ckeys c)) g.

(** The decision of DR-65 items 2 to 4 for candidate [c] of the batch [l]. *)
Definition group_decision (supply : nat -> N) (l : list candidate) (c : candidate) : bool :=
  alone_ok supply c && group_fits supply (group_of (alone_survivors supply l) c).

(** The accepted members, in the order of [l]. *)
Definition accepted_list (supply : nat -> N) (l : list candidate) : list candidate :=
  filter (group_decision supply l) l.

Lemma connected_within_permutation : forall fuel pool pool',
  Permutation pool pool' ->
  forall a b, connected_within fuel pool a b = connected_within fuel pool' a b.
Proof.
  induction fuel as [|fuel IH]; intros pool pool' perm a b; simpl; [reflexivity|].
  f_equal.
  rewrite (existsb_permutation _ _ _ _ perm).
  apply existsb_pointwise. intros c. cbv beta.
  rewrite (IH pool pool' perm c b). reflexivity.
Qed.

Lemma group_of_permutation : forall pool pool' a,
  Permutation pool pool' -> Permutation (group_of pool a) (group_of pool' a).
Proof.
  intros pool pool' a perm. unfold group_of.
  rewrite (Permutation_length perm).
  rewrite (filter_ext _ _ (connected_within_permutation (length pool') pool pool' perm a)).
  apply permutation_filter. exact perm.
Qed.

Lemma sum_on_permutation : forall k g g',
  Permutation g g' -> sum_on k g = sum_on k g'.
Proof.
  intros k g g' perm. unfold sum_on. apply nsum_permutation.
  apply Permutation_map. exact perm.
Qed.

Lemma group_fits_permutation : forall supply g g',
  Permutation g g' -> group_fits supply g = group_fits supply g'.
Proof.
  intros supply g g' perm. unfold group_fits.
  rewrite (forallb_permutation _ _ _ _ perm).
  apply forallb_pointwise. intros c. apply forallb_pointwise. intros k.
  rewrite (sum_on_permutation k g g' perm). reflexivity.
Qed.

(** The decision does not depend on the order of the candidates. Two orders
    of one batch give the same decision for every candidate, accepted lists
    that are permutations of each other, and the same accepted members. *)
Theorem group_decision_permutation_invariant : forall supply l l',
  Permutation l l' ->
  (forall c, group_decision supply l c = group_decision supply l' c) /\
  Permutation (accepted_list supply l) (accepted_list supply l') /\
  (forall c, In c (accepted_list supply l) <-> In c (accepted_list supply l')).
Proof.
  intros supply l l' perm.
  assert (same : forall c, group_decision supply l c = group_decision supply l' c).
  { intros c. unfold group_decision. f_equal.
    apply group_fits_permutation. apply group_of_permutation.
    unfold alone_survivors. apply permutation_filter. exact perm. }
  assert (accepted : Permutation (accepted_list supply l) (accepted_list supply l')).
  { unfold accepted_list. rewrite (filter_ext _ _ same).
    apply permutation_filter. exact perm. }
  split; [exact same|]. split; [exact accepted|].
  intros c. split; intros member.
  - eapply Permutation_in; [exact accepted | exact member].
  - eapply Permutation_in; [apply Permutation_sym; exact accepted | exact member].
Qed.

(** * The truncation argument *)

Lemma connected_within_mono : forall fuel fuel' pool pool' a b,
  (fuel <= fuel')%nat ->
  incl pool pool' ->
  connected_within fuel pool a b = true ->
  connected_within fuel' pool' a b = true.
Proof.
  induction fuel as [|fuel IH]; intros fuel' pool pool' a b more wider reach.
  - destruct fuel' as [|fuel']; simpl in reach |- *; [exact reach|].
    rewrite reach. reflexivity.
  - destruct fuel' as [|fuel']; [lia|]. simpl in reach |- *.
    apply orb_true_iff in reach. apply orb_true_iff.
    destruct reach as [same|step]; [left; exact same|right].
    apply existsb_exists in step. destruct step as [c [member link]].
    apply andb_true_iff in link. destruct link as [linked_ac rest].
    apply existsb_exists. exists c. split; [apply wider; exact member|].
    apply andb_true_iff. split; [exact linked_ac|].
    apply (IH fuel' pool pool'); [lia | exact wider | exact rest].
Qed.

Lemma group_decision_alone : forall supply l c,
  group_decision supply l c = true -> alone_ok supply c = true.
Proof.
  intros supply l c accepted. unfold group_decision in accepted.
  apply andb_true_iff in accepted. exact (proj1 accepted).
Qed.

Lemma accepted_subsequence_of_survivors : forall supply l,
  subsequence (accepted_list supply l) (alone_survivors supply l).
Proof.
  intros supply l. unfold accepted_list, alone_survivors.
  apply subsequence_filter_mono; [apply subsequence_refl|].
  intros x _ accepted. exact (group_decision_alone supply l x accepted).
Qed.

Lemma sum_on_subsequence : forall k s g, subsequence s g -> sum_on k s <= sum_on k g.
Proof.
  intros k s g sub. unfold sum_on. apply nsum_subsequence.
  apply subsequence_map. exact sub.
Qed.

Lemma group_fits_subsequence : forall supply s g,
  subsequence s g -> group_fits supply g = true -> group_fits supply s = true.
Proof.
  intros supply s g sub fits. unfold group_fits in *.
  rewrite forallb_forall in fits |- *. intros c member.
  specialize (fits c (subsequence_in _ _ _ c sub member)). cbv beta in fits |- *.
  rewrite forallb_forall in fits |- *. intros k key.
  specialize (fits k key). cbv beta in fits |- *.
  apply N.leb_le in fits. apply N.leb_le.
  pose proof (sum_on_subsequence k s g sub). lia.
Qed.

(** A subset of an accepted group is accepted. When the play budget stops a
    batch, every shipped member is accepted again by the decision over the
    shipped members alone: each one still passes alone, its group within
    the shipped members is a subsequence of its group within the batch, and
    the holds are not negative. So the decision of the re-run over the
    shipped prefix accepts all of it. *)
Theorem subset_of_accepted_group_is_accepted : forall supply l shipped,
  subsequence shipped (accepted_list supply l) ->
  forall c, In c shipped -> group_decision supply shipped c = true.
Proof.
  intros supply l shipped sub c member.
  assert (accepted_all : forall x, In x shipped -> group_decision supply l x = true).
  { intros x inx. pose proof (subsequence_in _ _ _ x sub inx) as acc.
    unfold accepted_list in acc. apply filter_In in acc. exact (proj2 acc). }
  assert (survivors : alone_survivors supply shipped = shipped).
  { unfold alone_survivors. apply filter_keeps_all. intros x inx.
    exact (group_decision_alone supply l x (accepted_all x inx)). }
  assert (into : subsequence shipped (alone_survivors supply l)).
  { eapply subsequence_trans; [exact sub | apply accepted_subsequence_of_survivors]. }
  pose proof (accepted_all c member) as accepted_c.
  unfold group_decision in accepted_c |- *. apply andb_true_iff in accepted_c.
  destruct accepted_c as [alone fits]. rewrite alone, andb_true_l.
  rewrite survivors.
  apply (group_fits_subsequence supply _ (group_of (alone_survivors supply l) c));
    [|exact fits].
  unfold group_of. apply subsequence_filter_mono; [exact into|].
  intros x _ reach.
  apply (connected_within_mono (length shipped) _ shipped); [| |exact reach].
  - apply subsequence_length. exact into.
  - intros y iny. exact (subsequence_in _ _ _ y into iny).
Qed.

(** The shipped prefix of an accepted batch is accepted again. *)
Corollary shipped_prefix_is_accepted : forall supply l n c,
  In c (firstn n (accepted_list supply l)) ->
  group_decision supply (firstn n (accepted_list supply l)) c = true.
Proof.
  intros supply l n c member.
  apply (subset_of_accepted_group_is_accepted supply l); [|exact member].
  apply subsequence_firstn.
Qed.

(** The guard is monotone in the later holds. Take a guard that holds over
    the scope of member i in the full batch. Each entry of a scope with the
    same balances and debits and no larger later holds also passes. The scope
    of the shipped prefix is such a scope: a purse leaves it when its later
    holds fall to zero and it is not debited. So a guard that held in the
    full batch holds in the replay of the shipped prefix. *)
Theorem guard_monotone_in_later_holds : forall purses fewer,
  (forall q, In q fewer -> exists p, In p purses /\
     balance_after q = balance_after p /\ debit q = debit p /\
     later_holds q <= later_holds p) ->
  guard purses = true -> guard fewer = true.
Proof.
  intros purses fewer covered held.
  apply guard_true_iff in held. apply guard_true_iff.
  apply Forall_forall. intros q member. cbv beta.
  destruct (covered q member) as [p [inp [same_balance [same_debit lower]]]].
  rewrite Forall_forall in held. specialize (held p inp). cbv beta in held.
  rewrite same_balance, same_debit. lia.
Qed.

(** * The decision supplies the premise of the batch theorems *)

(** A candidate holds nothing on a key that it does not list. *)
Definition holds_only_on_keys (l : list candidate) : Prop :=
  forall c k, In c l -> has_key k c = false -> chold c k = 0.

Lemma sum_on_keyed : forall k l,
  (forall c, In c l -> has_key k c = false -> chold c k = 0) ->
  sum_on k l = sum_on k (filter (has_key k) l).
Proof.
  intros k l zero. induction l as [|c l IH]; [reflexivity|].
  assert (tail : forall d, In d l -> has_key k d = false -> chold d k = 0).
  { intros d ind. apply zero. right. exact ind. }
  unfold sum_on in *. simpl.
  destruct (has_key k c) eqn:keyed; simpl.
  - rewrite (IH tail). reflexivity.
  - rewrite (zero c (or_introl eq_refl) keyed). rewrite (IH tail). lia.
Qed.

Lemma self_connected : forall fuel pool a, connected_within fuel pool a a = true.
Proof.
  intros fuel pool a. destruct fuel; simpl; rewrite Nat.eqb_refl; reflexivity.
Qed.

Lemma has_key_in : forall k c, has_key k c = true -> In k (ckeys c).
Proof.
  intros k c keyed. unfold has_key in keyed. apply existsb_exists in keyed.
  destruct keyed as [k' [member same]]. apply Nat.eqb_eq in same. subst k'. exact member.
Qed.

Lemma shared_key_connects : forall fuel pool a b k,
  In b pool -> In k (ckeys a) -> has_key k b = true ->
  connected_within (S fuel) pool a b = true.
Proof.
  intros fuel pool a b k inb ka kb. simpl. apply orb_true_iff. right.
  apply existsb_exists. exists b. split; [exact inb|].
  apply andb_true_iff. split.
  - unfold linked. apply existsb_exists. exists k. split; [exact ka | exact kb].
  - apply self_connected.
Qed.

(** The accepted members fit every purse. The accepted members that list a
    key [k] all share [k], so they lie in one group, and that group fits [k].
    This is the premise [later_sum members <= supply] of the batch theorems
    on the purse [k]. *)
Theorem accepted_members_fit_every_purse : forall supply l k,
  holds_only_on_keys l ->
  sum_on k (accepted_list supply l) <= supply k.
Proof.
  intros supply l k zero.
  assert (in_l : forall c, In c (accepted_list supply l) -> In c l).
  { intros c inc. unfold accepted_list in inc. apply filter_In in inc. exact (proj1 inc). }
  rewrite (sum_on_keyed k (accepted_list supply l));
    [|intros c inc; apply zero; apply in_l; exact inc].
  destruct (filter (has_key k) (accepted_list supply l)) as [|c0 keyed_rest] eqn:keyed.
  - unfold sum_on. simpl. lia.
  - assert (inc0 : In c0 (filter (has_key k) (accepted_list supply l))).
    { rewrite keyed. left. reflexivity. }
    apply filter_In in inc0. destruct inc0 as [acc0 key0].
    pose proof acc0 as dec0. unfold accepted_list in dec0. apply filter_In in dec0.
    destruct dec0 as [_ dec0].
    unfold group_decision in dec0. apply andb_true_iff in dec0. destruct dec0 as [_ fits].
    assert (survivor : forall x, In x (accepted_list supply l) ->
              In x (alone_survivors supply l)).
    { intros x inx.
      exact (subsequence_in _ _ _ x (accepted_subsequence_of_survivors supply l) inx). }
    assert (sub : subsequence (filter (has_key k) (accepted_list supply l))
                    (group_of (alone_survivors supply l) c0)).
    { unfold group_of. apply subsequence_filter_mono.
      - apply accepted_subsequence_of_survivors.
      - intros x inx keyx.
        pose proof (survivor x inx) as inpool.
        destruct (length (alone_survivors supply l)) as [|fuel] eqn:len.
        + apply length_zero_iff_nil in len. rewrite len in inpool. destruct inpool.
        + apply (shared_key_connects fuel _ c0 x k inpool); [|exact keyx].
          apply has_key_in. exact key0. }
    rewrite <- keyed.
    assert (c0_in_group : In c0 (group_of (alone_survivors supply l) c0)).
    { unfold group_of. apply filter_In. split; [exact (survivor c0 acc0)|].
      apply self_connected. }
    unfold group_fits in fits. rewrite forallb_forall in fits.
    specialize (fits c0 c0_in_group). cbv beta in fits. rewrite forallb_forall in fits.
    specialize (fits k (has_key_in k c0 key0)). cbv beta in fits.
    apply N.leb_le in fits.
    pose proof (sum_on_subsequence k _ _ sub). lia.
Qed.

(** The view of the batch from one purse [k]: each member contributes its
    hold on [k] and its settlement on [k]. *)
Definition purse_batch (k : nat) (settle : candidate -> purse_settlement)
    (members : list candidate) : list batch_member :=
  map (fun c => {| member_hold := chold c k; member_settlement := settle c |}) members.

Lemma later_sum_purse_batch : forall k settle members,
  later_sum (purse_batch k settle members) = sum_on k members.
Proof.
  intros k settle members. unfold purse_batch, sum_on.
  induction members as [|c rest IH]; simpl; [reflexivity|].
  rewrite IH. reflexivity.
Qed.

(** End to end on one purse: the accepted batch, executed in canonical order
    under option B, keeps every later hold covered, and every charge fits. *)
Corollary accepted_batch_keeps_every_later_hold : forall supply l k settle,
  holds_only_on_keys l ->
  follows_option_b (supply k) (purse_batch k settle (accepted_list supply l)) ->
  covers_later_holds (supply k) (purse_batch k settle (accepted_list supply l)) /\
  charges_fit (supply k) (purse_batch k settle (accepted_list supply l)).
Proof.
  intros supply l k settle zero rule.
  assert (fits : later_sum (purse_batch k settle (accepted_list supply l)) <= supply k).
  { rewrite later_sum_purse_batch. apply accepted_members_fit_every_purse. exact zero. }
  split.
  - apply batch_held_capacity_guard_preserves_later_holds; assumption.
  - apply group_holds_prevent_overdraft; assumption.
Qed.

(** The same holds for the shipped prefix that the re-run executes after a
    truncation. *)
Corollary shipped_prefix_keeps_every_later_hold : forall supply l k settle n,
  holds_only_on_keys l ->
  follows_option_b (supply k) (purse_batch k settle (firstn n (accepted_list supply l))) ->
  covers_later_holds (supply k) (purse_batch k settle (firstn n (accepted_list supply l))).
Proof.
  intros supply l k settle n zero rule.
  apply batch_held_capacity_guard_preserves_later_holds; [|exact rule].
  rewrite later_sum_purse_batch.
  pose proof (sum_on_subsequence k _ _ (subsequence_firstn _ n (accepted_list supply l))).
  pose proof (accepted_members_fit_every_purse supply l k zero). lia.
Qed.

(** * Negative controls *)

Definition key_p : nat := 0%nat.

(** A candidate with one key and one hold on it. *)
Definition single_key_candidate (identity key : nat) (amount : N) : candidate :=
  {| cid := identity;
     ckeys := [key];
     chold := fun k => if Nat.eqb k key then amount else 0;
     cvalid := true |}.

(** The supply of 10 on [key_p]. *)
Definition ten_on_p (k : nat) : N := if Nat.eqb k key_p then 10 else 0.

Definition six_first : candidate := single_key_candidate 1 key_p 6.
Definition six_second : candidate := single_key_candidate 2 key_p 6.

(** Prefix admission accepts the candidates in order while their cumulative
    holds fit. DR-65 rejects it, because it contradicts the simultaneous
    arrival rule of P1. *)
Fixpoint prefix_admission_from (supply : nat -> N) (admitted pending : list candidate)
    : list candidate :=
  match pending with
  | [] => admitted
  | c :: rest =>
      if alone_ok supply c && group_fits supply (admitted ++ [c])
      then prefix_admission_from supply (admitted ++ [c]) rest
      else prefix_admission_from supply admitted rest
  end.

Definition prefix_admission (supply : nat -> N) (l : list candidate) : list candidate :=
  prefix_admission_from supply [] l.

(** N1: prefix admission splits a group. The supply is 10, and two
    candidates hold 6 each on the same purse. Prefix admission accepts the
    first and rejects the second, although they form one group. The group
    rule rejects both. *)
Theorem nc_prefix_admission_splits_a_group :
  map cid (prefix_admission ten_on_p [six_first; six_second]) = [1%nat] /\
  linked six_first six_second = true /\
  group_decision ten_on_p [six_first; six_second] six_first = false /\
  group_decision ten_on_p [six_first; six_second] six_second = false.
Proof.
  repeat split; reflexivity.
Qed.

(** The independent rule checks each candidate alone and gives each one the
    full supply at R0 as its capacity. *)
Definition independent_admission (supply : nat -> N) (l : list candidate) : list candidate :=
  filter (alone_ok supply) l.

(** The run of the independent rule on the purse: each member debits 6, the
    first from 10 and the second from the 4 that the first leaves. *)
Definition independent_run : list batch_member :=
  [ {| member_hold := 6; member_settlement := SettledOnPurse 10 6 |};
    {| member_hold := 6; member_settlement := SettledOnPurse 4 6 |} ].

(** N2: independent full capacity overdraws. Both candidates pass alone and
    are accepted. Each one sees the full supply of 10 as its capacity, so its
    debit of 6 fits the capacity. The two debits sum to 12, which is more
    than the supply. After the first debit, 4 remain, and SystemVault cannot
    reserve the second debit of 6. The conclusion of
    [group_holds_prevent_overdraft] fails on this run. The group rule
    accepts neither candidate. *)
Theorem nc_independent_capacity_overdraws :
  map cid (independent_admission ten_on_p [six_first; six_second]) = [1%nat; 2%nat] /\
  6 <= ten_on_p key_p /\
  sum_on key_p [six_first; six_second] = 12 /\
  ten_on_p key_p < sum_on key_p [six_first; six_second] /\
  vault_condition [{| balance_after := 10 - 6; debit := 6; later_holds := 0 |}] = false /\
  ~ charges_fit 10 independent_run /\
  accepted_list ten_on_p [six_first; six_second] = [].
Proof.
  split; [reflexivity|].
  split; [unfold ten_on_p, key_p; simpl; lia|].
  split; [reflexivity|].
  split; [unfold ten_on_p, sum_on, key_p; simpl; lia|].
  split; [reflexivity|].
  split; [|reflexivity].
  simpl. intros [too_much _]. unfold option_b_cap in too_much. lia.
Qed.

(** The rollback rule without the cap: a rollback charge needs only to fit
    the restored balance. *)
Fixpoint follows_without_rollback_cap (restored : N) (members : list batch_member) : Prop :=
  match members with
  | [] => True
  | m :: rest =>
      match member_settlement m with
      | SettledOnPurse after_run success_debit =>
          guard [guard_entry after_run success_debit (later_sum rest)] = true
      | RolledBackOnPurse rollback_charge => rollback_charge <= restored
      end /\
      follows_without_rollback_cap (balance_after_member restored m) rest
  end.

(** The view from purse q: member 1 holds 2 on q and rolls back with
    [charge_on_q] on q, and member 2 holds 3 on q. *)
Definition rollback_on_q (charge_on_q : N) (second : purse_settlement) : list batch_member :=
  [ {| member_hold := 2; member_settlement := RolledBackOnPurse charge_on_q |};
    {| member_hold := 3; member_settlement := second |} ].

(** N3: a rollback without the cap loses a later hold. Member 1 holds 3 on p
    and 2 on q. Member 2 holds 3 on q. The supply is 3 on p and 5 on q. The
    code of member 1 drains p, so its guard fails on p, and it rolls back
    with a charge of 5. Without the cap, the capacity on q is the restored
    balance 5, so a selection can place all 5 on q. The later hold of 3 on q
    is then lost. Option B caps q at 5 - 3 = 2, so the charge splits into 3
    on p and 2 on q, and q keeps the 3 that member 2 holds. *)
Theorem nc_rollback_without_cap_loses_a_later_hold :
  guard [ {| balance_after := 0; debit := 3; later_holds := 0 |};
          {| balance_after := 5; debit := 2; later_holds := 3 |} ] = false /\
  follows_without_rollback_cap 5 (rollback_on_q 5 (RolledBackOnPurse 0)) /\
  ~ covers_later_holds 5 (rollback_on_q 5 (RolledBackOnPurse 0)) /\
  ~ follows_option_b 5 (rollback_on_q 5 (RolledBackOnPurse 0)) /\
  3 <= option_b_cap 3 0 /\
  2 <= option_b_cap 5 3 /\
  follows_option_b 5 (rollback_on_q 2 (SettledOnPurse 3 3)) /\
  covers_later_holds 5 (rollback_on_q 2 (SettledOnPurse 3 3)).
Proof.
  split; [reflexivity|].
  split.
  { cbv [follows_without_rollback_cap rollback_on_q later_sum balance_after_member
         member_hold member_settlement].
    repeat split; lia. }
  split.
  { cbv [covers_later_holds rollback_on_q later_sum balance_after_member
         member_hold member_settlement].
    intros [_ [lost _]]. lia. }
  split.
  { cbv [follows_option_b rollback_on_q later_sum balance_after_member
         member_hold member_settlement option_b_cap].
    intros [over _]. lia. }
  split; [unfold option_b_cap; lia|].
  split; [unfold option_b_cap; lia|].
  split.
  - cbv [follows_option_b rollback_on_q later_sum balance_after_member
         member_hold member_settlement option_b_cap guard_entry].
    repeat split; first [reflexivity | lia].
  - cbv [covers_later_holds rollback_on_q later_sum balance_after_member
         member_hold member_settlement].
    repeat split; lia.
Qed.

(** N4: a kept guard rollback disagrees with the replay of the shipped
    prefix. Member 1 is funded from q, where its balance is 1 and its debit
    is 1. Its code drains p, which only the dropped member 2 holds, with a
    later hold of 1. Over the full batch the guard fails on p, and member 1
    rolls back. Validators replay the shipped prefix alone. There p carries
    no later hold and leaves the scope, so the guard holds and member 1
    settles. A failing guard is therefore not monotone in the later holds,
    and the proposer must run the prefix again (truncation condition b). *)
Theorem nc_kept_guard_rollback_disagrees_with_prefix_replay :
  guard [ {| balance_after := 1; debit := 1; later_holds := 0 |};
          {| balance_after := 0; debit := 0; later_holds := 1 |} ] = false /\
  guard [ {| balance_after := 1; debit := 1; later_holds := 0 |} ] = true.
Proof.
  split; reflexivity.
Qed.

Print Assumptions batch_held_capacity_guard_preserves_later_holds.
Print Assumptions group_holds_prevent_overdraft.
Print Assumptions rollback_charge_at_most_the_hold_is_within_the_cap.
Print Assumptions group_decision_permutation_invariant.
Print Assumptions subset_of_accepted_group_is_accepted.
Print Assumptions shipped_prefix_is_accepted.
Print Assumptions guard_monotone_in_later_holds.
Print Assumptions accepted_members_fit_every_purse.
Print Assumptions accepted_batch_keeps_every_later_hold.
Print Assumptions shipped_prefix_keeps_every_later_hold.
Print Assumptions nc_prefix_admission_splits_a_group.
Print Assumptions nc_independent_capacity_overdraws.
Print Assumptions nc_rollback_without_cap_loses_a_later_hold.
Print Assumptions nc_kept_guard_rollback_disagrees_with_prefix_replay.
