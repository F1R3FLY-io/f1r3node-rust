(* G1-4a (DR-122): a complete linear estimate bounds the realized charge.

   Section 11 of docs/casper/theory/cost-accounting-impl/linear-cost-estimate.md
   argues that a complete estimate bounds every charge that the native meter
   can realize, for every execution and every order of matching. This module
   mechanizes that argument.

   The model.
   - A lane is a natural number. A participant is a send or a receive. A send
     carries its region multiset (one entry for each region of its seal, with
     multiplicity), its channel, the length of its datum message and its
     introduction bytes. A receive carries its introduction regions, its COMM
     regions, the channels of its binds (a multiset) and its introduction
     bytes.
   - A term is a tree of parallel parts, participants, receives with bodies
     and binary branch points.
   - The estimate follows design section 5.5. The channel tables M, T and TR
     range over every participant of the term, every branch included. Parallel
     parts add, a receive adds its contribution to the vector of its body, and
     a branch point takes the maximum.
   - A run introduces the participants of one selection of the term: each
     branch point runs one side, and a receive that fires runs its body. Each
     COMM consumes one receive and one send for each bind, on the bind's
     channel. The participants of all COMMs form a sub-multiset of the
     introduced ones, so no participant joins two COMMs (rules X1 and X7). The
     authority of a COMM is a sub-multiset of the concatenated seals of its
     participants, because the runtime merges regions with equal identities.
     Every participant of a run belongs to the term (rules X2 and X3, Lemma 2
     of the design), and its sizes are the sizes that the analysis computed
     (rules X4 to X6, Lemma 4).
   - The realized charge of a lane and a class sums the introduction charges
     and the COMM charges, one for each region of each authority, as the
     native meter charges.

   The theorems.
   1. [branch_max_dominates]: the contributions of every selection are at
      most the estimate (Lemma 6).
   2. [closed_intro_bound]: the introduction charge of a run is at most the
      introduction row of the estimate.
   3. [closed_comm_bound], [closed_transfer_bound], [closed_trace_bound]: the
      charge of one COMM is at most the contributions of its participants
      (Lemma 5).
   4. [complete_estimate_sound]: every realized charge of every run is at
      most the estimate, for every lane and every class.
   5. [fresh_unescaped_no_foreign_match]: a name that the deploy allocates
      differs from every name of another deploy and from every name that the
      deploy's data carry (Lemma 2). Collision freedom and unique deploys are
      section hypotheses, and [witness_allocation_is_collision_free] gives a
      witness that satisfies them.
   6. [units_applied_once]: the counted witness of the raw counts has the
      estimate as its usage.

   Negative controls. Each control breaks one premise or one rule of the
   estimate and gives a concrete run whose charge exceeds the estimate. *)

From Stdlib Require Import Arith.PeanoNat Lists.List Sorting.Permutation Lia.
Import ListNotations.

Definition lane := nat.
Definition channel := nat.

Inductive participant : Type :=
| PSend (regions : list lane) (chan : channel) (msg introduction : nat)
| PRecv (intro comm : list lane) (chans : list channel) (introduction : nat).

Inductive proc : Type :=
| TNil : proc
| TPar : proc -> proc -> proc
| TAct : participant -> proc
| TRecv : participant -> proc -> proc
| TIf : proc -> proc -> proc.

Fixpoint participants (t : proc) : list participant :=
  match t with
  | TNil => []
  | TPar p q => participants p ++ participants q
  | TAct a => [a]
  | TRecv r body => r :: participants body
  | TIf p q => participants p ++ participants q
  end.

Definition sum (xs : list nat) : nat := fold_right Nat.add 0 xs.

Definition max_list (xs : list nat) : nat := fold_right Nat.max 0 xs.

(** The number of regions of lane [s] in a multiset. *)
Definition mu (s : lane) (regions : list lane) : nat := count_occ Nat.eq_dec regions s.

(** * The channel tables *)

Definition messages_on (all : list participant) (x : channel) : list nat :=
  flat_map (fun p => match p with
                     | PSend _ c m _ => if Nat.eqb c x then [m] else []
                     | PRecv _ _ _ _ => []
                     end) all.

(** [M x]: the largest datum message on channel [x]. *)
Definition table_m (all : list participant) (x : channel) : nat := max_list (messages_on all x).

(** The sum of [M] over the binds of a receive, as a multiset. *)
Definition bind_sum (all : list participant) (chans : list channel) : nat :=
  sum (map (table_m all) chans).

Definition receives_on (all : list participant) (x : channel) : list (list channel) :=
  flat_map (fun p => match p with
                     | PRecv _ _ cs _ => if existsb (Nat.eqb x) cs then [cs] else []
                     | PSend _ _ _ _ => []
                     end) all.

(** [T x]: the largest transfer of a receive with a bind on [x]. *)
Definition table_t (all : list participant) (x : channel) : nat :=
  max_list (map (bind_sum all) (receives_on all x)).

Definition trace_of (chans : list channel) : nat := 32 + 96 * length chans.

(** [TR x]: the largest trace of a receive with a bind on [x]. *)
Definition table_tr (all : list participant) (x : channel) : nat :=
  max_list (map trace_of (receives_on all x)).

(** * The estimate *)

(** The classes: 0 compute, 1 introduction, 2 transfer and 3 trace. *)
Definition contrib (all : list participant) (p : participant) (s : lane) (k : nat) : nat :=
  match p with
  | PSend regions c _ ib =>
      match k with
      | 0 => mu s regions
      | 1 => mu s regions * ib
      | 2 => mu s regions * table_t all c
      | 3 => mu s regions * table_tr all c
      | _ => 0
      end
  | PRecv intro comm chans ib =>
      match k with
      | 0 => mu s comm
      | 1 => mu s intro * ib
      | 2 => mu s comm * bind_sum all chans
      | 3 => mu s comm * trace_of chans
      | _ => 0
      end
  end.

Fixpoint estimate (all : list participant) (t : proc) (s : lane) (k : nat) : nat :=
  match t with
  | TNil => 0
  | TPar p q => estimate all p s k + estimate all q s k
  | TAct a => contrib all a s k
  | TRecv r body => contrib all r s k + estimate all body s k
  | TIf p q => Nat.max (estimate all p s k) (estimate all q s k)
  end.

Definition estimate_of (t : proc) : lane -> nat -> nat := estimate (participants t) t.

(** * Runs *)

Inductive selects : proc -> list participant -> Prop :=
| selects_nil : selects TNil []
| selects_par : forall p q xs ys, selects p xs -> selects q ys -> selects (TPar p q) (xs ++ ys)
| selects_act : forall a, selects (TAct a) [a]
| selects_wait : forall r body, selects (TRecv r body) [r]
| selects_fire : forall r body ys, selects body ys -> selects (TRecv r body) (r :: ys)
| selects_left : forall p q xs, selects p xs -> selects (TIf p q) xs
| selects_right : forall p q xs, selects q xs -> selects (TIf p q) xs.

Record comm : Type := {
  comm_receive : participant;
  comm_sends : list participant;
  comm_authority : list lane
}.

Definition is_send (p : participant) : Prop :=
  match p with
  | PSend _ _ _ _ => True
  | PRecv _ _ _ _ => False
  end.

Definition chan_of (p : participant) : channel :=
  match p with
  | PSend _ c _ _ => c
  | PRecv _ _ _ _ => 0
  end.

Definition msg_of (p : participant) : nat :=
  match p with
  | PSend _ _ m _ => m
  | PRecv _ _ _ _ => 0
  end.

(** The seal regions of a participant in a COMM: the regions of a send, or the
    COMM regions of a receive. *)
Definition seal_of (p : participant) : list lane :=
  match p with
  | PSend regions _ _ _ => regions
  | PRecv _ comm _ _ => comm
  end.

(** A COMM consumes one send for each bind of its receive, on the bind's
    channel, and its authority is a sub-multiset of the concatenated seals. *)
Definition comm_valid (c : comm) : Prop :=
  match comm_receive c with
  | PRecv _ rc chans _ =>
      Forall is_send (comm_sends c) /\
      chans = map chan_of (comm_sends c) /\
      exists extra, Permutation (rc ++ concat (map seal_of (comm_sends c)))
                                (comm_authority c ++ extra)
  | PSend _ _ _ _ => False
  end.

Definition comm_participants (comms : list comm) : list participant :=
  flat_map (fun c => comm_receive c :: comm_sends c) comms.

(** The participants of all COMMs form a sub-multiset of the introduced
    participants. *)
Definition run_valid (xs : list participant) (comms : list comm) : Prop :=
  Forall comm_valid comms /\ exists rest, Permutation xs (comm_participants comms ++ rest).

Definition intro_charge (p : participant) (s : lane) : nat :=
  match p with
  | PSend regions _ _ ib => mu s regions * ib
  | PRecv intro _ _ ib => mu s intro * ib
  end.

Definition comm_quantity (c : comm) (k : nat) : nat :=
  match k with
  | 0 => 1
  | 2 => sum (map msg_of (comm_sends c))
  | 3 => 32 + 96 * length (comm_sends c)
  | _ => 0
  end.

(** The native meter charges the quantity of a COMM once for each region of
    its authority. *)
Definition comm_charge (c : comm) (s : lane) (k : nat) : nat :=
  mu s (comm_authority c) * comm_quantity c k.

Definition realized (xs : list participant) (comms : list comm) (s : lane) (k : nat) : nat :=
  match k with
  | 1 => sum (map (fun p => intro_charge p s) xs)
  | _ => sum (map (fun c => comm_charge c s k) comms)
  end.

(** * Arithmetic of sums *)

Lemma sum_cons : forall x xs, sum (x :: xs) = x + sum xs.
Proof. reflexivity. Qed.

Lemma sum_map_cons : forall (A : Type) (f : A -> nat) x xs,
  sum (map f (x :: xs)) = f x + sum (map f xs).
Proof. reflexivity. Qed.

Lemma sum_app : forall xs ys, sum (xs ++ ys) = sum xs + sum ys.
Proof. induction xs as [| x xs IH]; intros ys; simpl; [reflexivity | rewrite IH; lia]. Qed.

Lemma sum_permutation : forall xs ys, Permutation xs ys -> sum xs = sum ys.
Proof.
  intros xs ys permutation.
  induction permutation; simpl; lia.
Qed.

Lemma sum_map_le : forall (A : Type) (f g : A -> nat) xs,
  (forall x, In x xs -> f x <= g x) -> sum (map f xs) <= sum (map g xs).
Proof.
  intros A f g xs.
  induction xs as [| x xs IH]; intros bounded; simpl; [lia |].
  pose proof (bounded x (or_introl eq_refl)).
  assert (sum (map f xs) <= sum (map g xs)) by (apply IH; intros y inside; apply bounded; right; exact inside).
  lia.
Qed.

Lemma sum_map_app : forall (A : Type) (f : A -> nat) xs ys,
  sum (map f (xs ++ ys)) = sum (map f xs) + sum (map f ys).
Proof. intros A f xs ys. rewrite map_app. apply sum_app. Qed.

Lemma sum_map_scale : forall (A : Type) (f : A -> nat) scale xs,
  sum (map f xs) * scale = sum (map (fun x => f x * scale) xs).
Proof.
  intros A f scale xs.
  induction xs as [| x xs IH]; simpl; [reflexivity |].
  rewrite <- IH. lia.
Qed.

Lemma mu_app : forall s left right, mu s (left ++ right) = mu s left + mu s right.
Proof. intros s left right. unfold mu. apply count_occ_app. Qed.

Lemma mu_concat : forall s (pieces : list (list lane)),
  mu s (concat pieces) = sum (map (mu s) pieces).
Proof.
  intros s pieces.
  induction pieces as [| piece pieces IH]; simpl; [reflexivity |].
  rewrite mu_app, IH. reflexivity.
Qed.

Lemma mu_permutation : forall s left right, Permutation left right -> mu s left = mu s right.
Proof.
  intros s left right permutation. unfold mu.
  exact (proj1 (Permutation_count_occ Nat.eq_dec left right) permutation s).
Qed.

(** A merged authority charges at most the concatenated seals: merging only
    lowers a charge. *)
Theorem merged_authority_charges_less : forall s seals authority extra,
  Permutation seals (authority ++ extra) -> mu s authority <= mu s seals.
Proof.
  intros s seals authority extra permutation.
  rewrite (mu_permutation s _ _ permutation), mu_app. lia.
Qed.

(** * Tables bound every participant of the term *)

Lemma max_list_upper : forall x xs, In x xs -> x <= max_list xs.
Proof.
  intros x xs. induction xs as [| y ys IH]; simpl; [contradiction |].
  intros [same | inside]; [subst; lia |]. specialize (IH inside). lia.
Qed.

Lemma message_upper : forall all regions c m ib,
  In (PSend regions c m ib) all -> m <= table_m all c.
Proof.
  intros all regions c m ib inside. unfold table_m. apply max_list_upper.
  unfold messages_on. apply in_flat_map. exists (PSend regions c m ib).
  split; [exact inside |]. simpl. rewrite Nat.eqb_refl. left. reflexivity.
Qed.

Lemma receive_listed : forall all intro rc chans ib x,
  In (PRecv intro rc chans ib) all -> In x chans -> In chans (receives_on all x).
Proof.
  intros all intro rc chans ib x inside bound. unfold receives_on. apply in_flat_map.
  exists (PRecv intro rc chans ib). split; [exact inside |].
  assert (listed : existsb (Nat.eqb x) chans = true).
  { apply existsb_exists. exists x. split; [exact bound | apply Nat.eqb_refl]. }
  simpl. rewrite listed. left. reflexivity.
Qed.

Lemma transfer_upper : forall all intro rc chans ib x,
  In (PRecv intro rc chans ib) all -> In x chans -> bind_sum all chans <= table_t all x.
Proof.
  intros all intro rc chans ib x inside bound. unfold table_t. apply max_list_upper.
  apply in_map. eapply receive_listed; eassumption.
Qed.

Lemma trace_upper : forall all intro rc chans ib x,
  In (PRecv intro rc chans ib) all -> In x chans -> trace_of chans <= table_tr all x.
Proof.
  intros all intro rc chans ib x inside bound. unfold table_tr. apply max_list_upper.
  apply in_map. eapply receive_listed; eassumption.
Qed.

Lemma sends_within_bind_sum : forall all sends,
  incl sends all -> Forall is_send sends ->
  sum (map msg_of sends) <= bind_sum all (map chan_of sends).
Proof.
  intros all sends. unfold bind_sum.
  induction sends as [| o os IH]; intros within shapes; [simpl; lia |].
  inversion shapes as [| o' os' shape rest]; subst.
  rewrite map_cons, map_cons, sum_map_cons, sum_cons.
  destruct o as [regions c m ib | intro rc chans ib]; [| simpl in shape; destruct shape].
  pose proof (message_upper all regions c m ib (within _ (or_introl eq_refl))) as head.
  assert (tail : sum (map msg_of os) <= sum (map (table_m all) (map chan_of os))).
  { apply IH; [intros p inside; apply within; right; exact inside | exact rest]. }
  simpl msg_of. simpl chan_of. lia.
Qed.

(** * Lemma 6: the maximum at branches dominates every selection *)

Lemma selects_incl : forall t xs, selects t xs -> incl xs (participants t).
Proof.
  intros t xs selection.
  induction selection; intros x inside; simpl.
  - contradiction.
  - apply in_app_or in inside. apply in_or_app.
    destruct inside as [inside | inside];
      [left; apply IHselection1; exact inside | right; apply IHselection2; exact inside].
  - exact inside.
  - destruct inside as [same | []]. left. exact same.
  - destruct inside as [same | inside]; [left; exact same | right; apply IHselection; exact inside].
  - apply in_or_app. left. apply IHselection. exact inside.
  - apply in_or_app. right. apply IHselection. exact inside.
Qed.

Theorem branch_max_dominates : forall all t xs,
  selects t xs -> forall s k, sum (map (fun p => contrib all p s k) xs) <= estimate all t s k.
Proof.
  intros all t xs selection s k.
  induction selection; simpl estimate.
  - simpl. lia.
  - rewrite sum_map_app. lia.
  - simpl. lia.
  - simpl. lia.
  - rewrite sum_map_cons. lia.
  - lia.
  - lia.
Qed.

Theorem closed_intro_bound : forall t xs s,
  selects t xs -> sum (map (fun p => intro_charge p s) xs) <= estimate_of t s 1.
Proof.
  intros t xs s selection. unfold estimate_of.
  replace (map (fun p => intro_charge p s) xs) with (map (fun p => contrib (participants t) p s 1) xs).
  - apply branch_max_dominates. exact selection.
  - apply map_ext. intros [regions c m ib | intro rc chans ib]; reflexivity.
Qed.

(** * Lemma 5: the charge of one COMM *)

Lemma authority_bound : forall c s rc chans intro ib,
  comm_receive c = PRecv intro rc chans ib -> comm_valid c ->
  mu s (comm_authority c) <= mu s rc + sum (map (fun o => mu s (seal_of o)) (comm_sends c)).
Proof.
  intros c s rc chans intro ib shape valid.
  unfold comm_valid in valid. rewrite shape in valid.
  destruct valid as [_ [_ [extra permutation]]].
  pose proof (merged_authority_charges_less s _ _ _ permutation) as merged.
  rewrite mu_app, mu_concat, map_map in merged. exact merged.
Qed.

Lemma seal_sum_is_compute : forall all s sends,
  sum (map (fun o => mu s (seal_of o)) sends) = sum (map (fun p => contrib all p s 0) sends).
Proof.
  intros all s sends. f_equal. apply map_ext.
  intros [regions c m ib | intro rc chans ib]; reflexivity.
Qed.

Theorem closed_comm_bound : forall all c s,
  comm_valid c ->
  comm_charge c s 0 <= sum (map (fun p => contrib all p s 0) (comm_receive c :: comm_sends c)).
Proof.
  intros all c s valid.
  destruct (comm_receive c) as [regions ch m ib | intro rc chans ib] eqn:shape.
  - unfold comm_valid in valid. rewrite shape in valid. destruct valid.
  - pose proof (authority_bound c s rc chans intro ib shape valid) as bound.
    rewrite sum_map_cons, <- seal_sum_is_compute.
    change (contrib all (PRecv intro rc chans ib) s 0) with (mu s rc).
    unfold comm_charge. change (comm_quantity c 0) with 1. lia.
Qed.

Theorem closed_transfer_bound : forall all c s,
  comm_valid c -> incl (comm_receive c :: comm_sends c) all ->
  comm_charge c s 2 <= sum (map (fun p => contrib all p s 2) (comm_receive c :: comm_sends c)).
Proof.
  intros all c s valid within.
  destruct (comm_receive c) as [regions ch m ib | intro rc chans ib] eqn:shape.
  - unfold comm_valid in valid. rewrite shape in valid. destruct valid.
  - pose proof (authority_bound c s rc chans intro ib shape valid) as bound.
    pose proof valid as valid'. unfold comm_valid in valid'. rewrite shape in valid'.
    destruct valid' as [shapes [channels _]].
    assert (receive_in : In (PRecv intro rc chans ib) all).
    { apply within. left. reflexivity. }
    assert (sends_in : incl (comm_sends c) all).
    { intros p inside. apply within. right. exact inside. }
    assert (to_receive : sum (map msg_of (comm_sends c)) <= bind_sum all chans).
    { rewrite channels. apply sends_within_bind_sum; assumption. }
    assert (to_sends : forall o, In o (comm_sends c) ->
                       mu s (seal_of o) * sum (map msg_of (comm_sends c)) <= contrib all o s 2).
    { intros o inside.
      rewrite Forall_forall in shapes. pose proof (shapes o inside) as send.
      destruct o as [regions c' m' ib' | i' rc' cs' ib']; [| simpl in send; destruct send].
      change (seal_of (PSend regions c' m' ib')) with regions.
      change (contrib all (PSend regions c' m' ib') s 2) with (mu s regions * table_t all c').
      apply Nat.mul_le_mono_l.
      assert (bound_by : bind_sum all chans <= table_t all c').
      { apply (transfer_upper all intro rc chans ib c' receive_in).
        rewrite channels. apply in_map_iff. exists (PSend regions c' m' ib').
        split; [reflexivity | exact inside]. }
      lia. }
    rewrite sum_map_cons.
    change (contrib all (PRecv intro rc chans ib) s 2) with (mu s rc * bind_sum all chans).
    unfold comm_charge. change (comm_quantity c 2) with (sum (map msg_of (comm_sends c))).
    apply Nat.le_trans with ((mu s rc + sum (map (fun o => mu s (seal_of o)) (comm_sends c))) *
                             sum (map msg_of (comm_sends c))).
    { apply Nat.mul_le_mono_r. exact bound. }
    rewrite Nat.mul_add_distr_r, sum_map_scale.
    apply Nat.add_le_mono.
    + apply Nat.mul_le_mono_l. exact to_receive.
    + apply sum_map_le. exact to_sends.
Qed.

Theorem closed_trace_bound : forall all c s,
  comm_valid c -> incl (comm_receive c :: comm_sends c) all ->
  comm_charge c s 3 <= sum (map (fun p => contrib all p s 3) (comm_receive c :: comm_sends c)).
Proof.
  intros all c s valid within.
  destruct (comm_receive c) as [regions ch m ib | intro rc chans ib] eqn:shape.
  - unfold comm_valid in valid. rewrite shape in valid. destruct valid.
  - pose proof (authority_bound c s rc chans intro ib shape valid) as bound.
    pose proof valid as valid'. unfold comm_valid in valid'. rewrite shape in valid'.
    destruct valid' as [shapes [channels _]].
    assert (receive_in : In (PRecv intro rc chans ib) all).
    { apply within. left. reflexivity. }
    assert (same_trace : 32 + 96 * length (comm_sends c) = trace_of chans).
    { unfold trace_of. rewrite channels, length_map. reflexivity. }
    assert (to_sends : forall o, In o (comm_sends c) ->
                       mu s (seal_of o) * trace_of chans <= contrib all o s 3).
    { intros o inside.
      rewrite Forall_forall in shapes. pose proof (shapes o inside) as send.
      destruct o as [regions c' m' ib' | i' rc' cs' ib']; [| simpl in send; destruct send].
      change (seal_of (PSend regions c' m' ib')) with regions.
      change (contrib all (PSend regions c' m' ib') s 3) with (mu s regions * table_tr all c').
      apply Nat.mul_le_mono_l.
      apply (trace_upper all intro rc chans ib c' receive_in).
      rewrite channels. apply in_map_iff. exists (PSend regions c' m' ib').
      split; [reflexivity | exact inside]. }
    rewrite sum_map_cons.
    change (contrib all (PRecv intro rc chans ib) s 3) with (mu s rc * trace_of chans).
    unfold comm_charge. change (comm_quantity c 3) with (32 + 96 * length (comm_sends c)).
    rewrite same_trace.
    apply Nat.le_trans with ((mu s rc + sum (map (fun o => mu s (seal_of o)) (comm_sends c))) *
                             trace_of chans).
    { apply Nat.mul_le_mono_r. exact bound. }
    rewrite Nat.mul_add_distr_r, sum_map_scale.
    apply Nat.add_le_mono; [lia |].
    apply sum_map_le. exact to_sends.
Qed.

(** * The theorem *)

Lemma comm_participants_cons : forall c cs,
  comm_participants (c :: cs) = (comm_receive c :: comm_sends c) ++ comm_participants cs.
Proof. reflexivity. Qed.

Lemma comm_charges_within_participants : forall all comms s k,
  Forall comm_valid comms -> incl (comm_participants comms) all -> k <> 1 ->
  sum (map (fun c => comm_charge c s k) comms) <=
  sum (map (fun p => contrib all p s k) (comm_participants comms)).
Proof.
  intros all comms s k valid within not_intro.
  induction comms as [| c cs IH]; [simpl; lia |].
  inversion valid as [| c' cs' valid_c valid_cs]; subst.
  rewrite comm_participants_cons in within |- *.
  rewrite sum_map_cons, sum_map_app.
  assert (inner : incl (comm_receive c :: comm_sends c) all).
  { intros p inside. apply within. apply in_or_app. left. exact inside. }
  assert (rest : sum (map (fun c0 => comm_charge c0 s k) cs) <=
                 sum (map (fun p => contrib all p s k) (comm_participants cs))).
  { apply IH; [exact valid_cs |]. intros p inside. apply within. apply in_or_app. right. exact inside. }
  assert (head : comm_charge c s k <=
                 sum (map (fun p => contrib all p s k) (comm_receive c :: comm_sends c))).
  { destruct k as [| [| [| [| k]]]].
    - exact (closed_comm_bound all c s valid_c).
    - contradiction.
    - exact (closed_transfer_bound all c s valid_c inner).
    - exact (closed_trace_bound all c s valid_c inner).
    - unfold comm_charge. change (comm_quantity c (S (S (S (S k))))) with 0. lia. }
  lia.
Qed.

Theorem complete_estimate_sound : forall t xs comms,
  selects t xs -> run_valid xs comms ->
  forall s k, realized xs comms s k <= estimate_of t s k.
Proof.
  intros t xs comms selection [valid [rest permutation]] s k.
  pose proof (selects_incl t xs selection) as within_term.
  destruct (Nat.eq_dec k 1) as [intro | not_intro].
  - subst k. apply closed_intro_bound. exact selection.
  - assert (realized_comms : realized xs comms s k = sum (map (fun c => comm_charge c s k) comms)).
    { destruct k as [| [| k]]; [reflexivity | contradiction | reflexivity]. }
    rewrite realized_comms.
    assert (within : incl (comm_participants comms) (participants t)).
    { intros p inside. apply within_term.
      apply (Permutation_in p (Permutation_sym permutation)).
      apply in_or_app. left. exact inside. }
    apply Nat.le_trans with (sum (map (fun p => contrib (participants t) p s k) (comm_participants comms))).
    { apply comm_charges_within_participants; assumption. }
    apply Nat.le_trans with (sum (map (fun p => contrib (participants t) p s k) xs)).
    + rewrite (sum_permutation _ _ (Permutation_map (fun p => contrib (participants t) p s k) permutation)).
      rewrite sum_map_app. lia.
    + unfold estimate_of. apply branch_max_dominates. exact selection.
Qed.

(** * Lemma 2: a fresh name meets no foreign participant *)

Section Names.

Variable Seed Name : Type.
(** The allocation of the [index]-th name of the deploy with seed [seed]. *)
Variable alloc : Seed -> nat -> Name.

(** Collision freedom and unique deploys: two allocations give the same name
    only for the same seed and the same index. *)
Hypothesis allocation_is_collision_free : forall seed index seed' index',
  alloc seed index = alloc seed' index' -> seed = seed' /\ index = index'.

(** A name that a foreign process holds comes from another deploy, or from
    the data of this deploy. Rule X3 keeps the fresh names of the deploy out of
    its data, so neither source carries one. *)
Theorem fresh_unescaped_no_foreign_match : forall deploy other index (data_names : list Name) foreign,
  deploy <> other ->
  (forall name, In name data_names -> forall i, name <> alloc deploy i) ->
  (exists i, foreign = alloc other i) \/ In foreign data_names ->
  foreign <> alloc deploy index.
Proof.
  intros deploy other index data_names foreign distinct unescaped [[i allocated] | carried] same.
  - subst foreign. apply allocation_is_collision_free in same. destruct same as [seeds _].
    apply distinct. symmetry. exact seeds.
  - exact (unescaped foreign carried index same).
Qed.

End Names.

Definition witness_alloc (seed index : nat) : nat * nat := (seed, index).

Lemma witness_allocation_is_collision_free : forall seed index seed' index',
  witness_alloc seed index = witness_alloc seed' index' -> seed = seed' /\ index = index'.
Proof. intros seed index seed' index' same. inversion same. split; reflexivity. Qed.

Corollary witness_fresh_name_meets_no_foreign_name :
  witness_alloc 2 0 <> witness_alloc 1 0.
Proof.
  apply (fresh_unescaped_no_foreign_match nat (nat * nat) witness_alloc
           witness_allocation_is_collision_free 1 2 0 [] (witness_alloc 2 0)).
  - discriminate.
  - intros name [].
  - left. exists 0. reflexivity.
Qed.

(** * Units are applied once *)

Definition classes : list nat := [0; 1; 2; 3].

(** The estimate in phlo: the weight and the units applied once to each raw
    count (design section 5.7). *)
Definition estimate_phlo (lanes : list lane) (units : lane -> nat) (weights : nat -> nat)
    (counts : lane -> nat -> nat) : nat :=
  sum (map (fun s => sum (map (fun k => weights k * units s * counts s k) classes)) lanes).

(** The counted witness: one entry for each lane and class with its raw count.
    Its usage applies the weight and the units of the entry's lane. *)
Definition witness_entries (lanes : list lane) (counts : lane -> nat -> nat)
  : list (lane * nat * nat) :=
  flat_map (fun s => map (fun k => (s, k, counts s k)) classes) lanes.

Definition witness_usage (units : lane -> nat) (weights : nat -> nat)
    (entries : list (lane * nat * nat)) : nat :=
  sum (map (fun entry => match entry with (s, k, count) => weights k * units s * count end) entries).

Lemma sum_map_flat_map : forall (A B : Type) (g : B -> nat) (f : A -> list B) l,
  sum (map g (flat_map f l)) = sum (map (fun x => sum (map g (f x))) l).
Proof.
  intros A B g f l. induction l as [| x l IH]; [reflexivity |].
  cbn [flat_map]. rewrite map_app, sum_app, IH, sum_map_cons. reflexivity.
Qed.

Theorem units_applied_once : forall lanes units weights counts,
  witness_usage units weights (witness_entries lanes counts) = estimate_phlo lanes units weights counts.
Proof.
  intros lanes units weights counts. unfold witness_usage, witness_entries, estimate_phlo.
  rewrite sum_map_flat_map. f_equal; apply map_ext; intro s; rewrite map_map; reflexivity.
Qed.

(** * Negative controls *)

Ltac sends_shape := repeat (apply Forall_cons; [exact I |]); apply Forall_nil.

(** A deploy lane 0, a signature lane 1, and the channels 5 and 6. *)

(** A public channel or an escaping name (rules X2 and X3): a foreign send on
    the deploy's channel delivers a datum that no table bounds. *)
Definition foreign_receive : participant := PRecv [0] [0] [5] 10.
Definition foreign_term : proc := TRecv foreign_receive TNil.
Definition foreign_comm : comm := {|
  comm_receive := foreign_receive;
  comm_sends := [PSend [] 5 1000 0];
  comm_authority := [0]
|}.

Theorem nc_public_channel_breaks_the_bound :
  selects foreign_term [foreign_receive] /\ comm_valid foreign_comm /\
  realized [foreign_receive] [foreign_comm] 0 2 > estimate_of foreign_term 0 2.
Proof.
  split; [constructor |]. split.
  - unfold comm_valid. simpl. split; [sends_shape |]. split; [reflexivity |].
    exists []. simpl. apply Permutation_refl.
  - vm_compute. lia.
Qed.

(** A persistent receive (rule X1): the receive fires twice, so two COMMs
    consume one participant. *)
Definition persistent_receive : participant := PRecv [0] [0] [5] 10.
Definition persistent_send : participant := PSend [0] 5 7 20.
Definition persistent_term : proc := TPar (TAct persistent_send) (TRecv persistent_receive TNil).
Definition persistent_comm : comm := {|
  comm_receive := persistent_receive;
  comm_sends := [persistent_send];
  comm_authority := [0; 0]
|}.

Theorem nc_persistent_receive_breaks_the_bound :
  selects persistent_term [persistent_send; persistent_receive] /\
  realized [persistent_send; persistent_receive] [persistent_comm; persistent_comm] 0 0 >
    estimate_of persistent_term 0 0.
Proof.
  split.
  - change [persistent_send; persistent_receive] with ([persistent_send] ++ [persistent_receive]).
    repeat constructor.
  - vm_compute. lia.
Qed.

(** A peek (rule X7): one datum is delivered to two receives. *)
Definition peek_send : participant := PSend [0] 5 7 20.
Definition peek_first : participant := PRecv [0] [0] [5] 10.
Definition peek_second : participant := PRecv [0] [0] [5] 11.
Definition peek_term : proc :=
  TPar (TAct peek_send) (TPar (TRecv peek_first TNil) (TRecv peek_second TNil)).
Definition peek_comm (receive : participant) : comm := {|
  comm_receive := receive;
  comm_sends := [peek_send];
  comm_authority := [0; 0]
|}.

Theorem nc_peek_breaks_the_bound :
  realized [peek_send; peek_first; peek_second] [peek_comm peek_first; peek_comm peek_second] 0 2 >
  estimate_of peek_term 0 2.
Proof. vm_compute. lia. Qed.

(** A received value in data (rule X4), or a datum sized before its
    evaluation: the delivered datum is longer than the sized one. *)
Definition sized_send : participant := PSend [0] 5 16 20.
Definition delivered_send : participant := PSend [0] 5 40 20.
Definition sized_receive : participant := PRecv [0] [0] [5] 10.
Definition sized_term : proc := TPar (TAct sized_send) (TRecv sized_receive TNil).
Definition delivered_comm : comm := {|
  comm_receive := sized_receive;
  comm_sends := [delivered_send];
  comm_authority := [0; 0]
|}.

Theorem nc_received_value_in_data_breaks_the_bound :
  comm_valid delivered_comm /\
  realized [delivered_send; sized_receive] [delivered_comm] 0 2 > estimate_of sized_term 0 2.
Proof.
  split.
  - unfold comm_valid. simpl. split; [sends_shape |]. split; [reflexivity |].
    exists []. simpl. apply Permutation_refl.
  - vm_compute. lia.
Qed.

Theorem nc_datum_sized_before_evaluation_breaks_the_bound :
  realized [delivered_send; sized_receive] [delivered_comm] 0 2 > estimate_of sized_term 0 2.
Proof. vm_compute. lia. Qed.

(** A received value inside a nested introduction (rule X6): the stored
    continuation is longer than the sized one. *)
Definition sized_inner : participant := PRecv [0] [0] [6] 50.
Definition grown_inner : participant := PRecv [0] [0] [6] 90.
Definition nested_term : proc := TRecv sized_receive (TRecv sized_inner TNil).

Theorem nc_received_value_in_nested_introduction_breaks_the_bound :
  realized [sized_receive; grown_inner] [] 0 1 > estimate_of nested_term 0 1.
Proof. vm_compute. lia. Qed.

(** A method call in process position (rule X8): the method's result runs as
    a participant that the term does not hold. *)
Definition method_term : proc := TAct (PSend [0] 5 7 20).

Theorem nc_method_call_breaks_the_bound :
  realized [PSend [0] 5 7 20; PSend [0] 6 7 20] [] 0 1 > estimate_of method_term 0 1.
Proof. vm_compute. lia. Qed.

(** The minimum instead of the maximum over branches. *)
Fixpoint estimate_min (all : list participant) (t : proc) (s : lane) (k : nat) : nat :=
  match t with
  | TNil => 0
  | TPar p q => estimate_min all p s k + estimate_min all q s k
  | TAct a => contrib all a s k
  | TRecv r body => contrib all r s k + estimate_min all body s k
  | TIf p q => Nat.min (estimate_min all p s k) (estimate_min all q s k)
  end.

Definition branch_term : proc := TIf (TAct (PSend [0] 5 7 20)) TNil.

Theorem nc_minimum_over_branches_breaks_the_bound :
  selects branch_term [PSend [0] 5 7 20] /\
  realized [PSend [0] 5 7 20] [] 0 1 > estimate_min (participants branch_term) branch_term 0 1.
Proof.
  split; [repeat constructor |].
  vm_compute. lia.
Qed.

(** The introduction charged to the regions of the signed binds instead of
    the introduction regions. *)
Definition contrib_bind_intro (all : list participant) (p : participant) (s : lane) (k : nat) : nat :=
  match p, k with
  | PRecv _ comm _ ib, 1 => mu s comm * ib
  | _, _ => contrib all p s k
  end.

Definition signed_join : participant := PRecv [0] [1; 1] [5; 6] 30.

Theorem nc_introduction_on_bind_regions_breaks_the_bound :
  realized [signed_join] [] 0 1 > contrib_bind_intro [signed_join] signed_join 0 1.
Proof. vm_compute. lia. Qed.

(** Units applied twice: the usage exceeds the estimate for a lane of two
    units. *)
Definition two_units (_ : lane) : nat := 2.
Definition unit_weights (_ : nat) : nat := 1.
Definition one_count (s : lane) (k : nat) : nat := if Nat.eqb k 0 then 1 else 0.

Theorem nc_units_applied_twice_breaks_the_bound :
  sum (map (fun entry => match entry with (s, k, count) => unit_weights k * two_units s * two_units s * count end)
         (witness_entries [0] one_count)) >
  estimate_phlo [0] two_units unit_weights one_count.
Proof. vm_compute. lia. Qed.

(** A join with a repeated channel counted as a set: the transfer of two data
    on one channel exceeds one datum. *)
Definition bind_sum_set (all : list participant) (chans : list channel) : nat :=
  sum (map (table_m all) (nodup Nat.eq_dec chans)).

Definition repeated_receive : participant := PRecv [0] [0] [5; 5] 10.
Definition repeated_first : participant := PSend [] 5 7 20.
Definition repeated_second : participant := PSend [] 5 7 21.
Definition repeated_all : list participant := [repeated_first; repeated_second; repeated_receive].
Definition repeated_comm : comm := {|
  comm_receive := repeated_receive;
  comm_sends := [repeated_first; repeated_second];
  comm_authority := [0]
|}.

Theorem nc_repeated_channel_as_set_breaks_the_bound :
  comm_valid repeated_comm /\
  comm_charge repeated_comm 0 2 > mu 0 [0] * bind_sum_set repeated_all [5; 5] /\
  comm_charge repeated_comm 0 2 <= mu 0 [0] * bind_sum repeated_all [5; 5].
Proof.
  split.
  - unfold comm_valid. simpl. split; [sends_shape |]. split; [reflexivity |].
    exists []. simpl. apply Permutation_refl.
  - split; vm_compute; lia.
Qed.

(** Lane membership instead of region multiplicity: the two signed binds of
    one signature carry two regions of one lane. *)
Definition contrib_membership (all : list participant) (p : participant) (s : lane) (k : nat) : nat :=
  match p, k with
  | PRecv _ comm _ _, 0 => Nat.min 1 (mu s comm)
  | _, _ => contrib all p s k
  end.

Definition member_first : participant := PSend [] 5 7 20.
Definition member_second : participant := PSend [] 6 7 21.
Definition member_comm : comm := {|
  comm_receive := signed_join;
  comm_sends := [member_first; member_second];
  comm_authority := [1; 1]
|}.

Theorem nc_lane_membership_breaks_the_bound :
  comm_valid member_comm /\
  comm_charge member_comm 1 0 >
    contrib_membership [] signed_join 1 0 + contrib_membership [] member_first 1 0 +
    contrib_membership [] member_second 1 0.
Proof.
  split.
  - unfold comm_valid. simpl. split; [sends_shape |]. split; [reflexivity |].
    exists []. simpl. apply Permutation_refl.
  - vm_compute. lia.
Qed.

Print Assumptions merged_authority_charges_less.
Print Assumptions branch_max_dominates.
Print Assumptions closed_intro_bound.
Print Assumptions closed_comm_bound.
Print Assumptions closed_transfer_bound.
Print Assumptions closed_trace_bound.
Print Assumptions complete_estimate_sound.
Print Assumptions fresh_unescaped_no_foreign_match.
Print Assumptions witness_fresh_name_meets_no_foreign_name.
Print Assumptions units_applied_once.
Print Assumptions nc_public_channel_breaks_the_bound.
Print Assumptions nc_persistent_receive_breaks_the_bound.
Print Assumptions nc_peek_breaks_the_bound.
Print Assumptions nc_received_value_in_data_breaks_the_bound.
Print Assumptions nc_datum_sized_before_evaluation_breaks_the_bound.
Print Assumptions nc_received_value_in_nested_introduction_breaks_the_bound.
Print Assumptions nc_method_call_breaks_the_bound.
Print Assumptions nc_minimum_over_branches_breaks_the_bound.
Print Assumptions nc_introduction_on_bind_regions_breaks_the_bound.
Print Assumptions nc_units_applied_twice_breaks_the_bound.
Print Assumptions nc_repeated_channel_as_set_breaks_the_bound.
Print Assumptions nc_lane_membership_breaks_the_bound.
