(* DR-101 (bug 10056): the seal of residue that a system body leaves behind.

   A continuation sealed by system authority alone (genesis, system deploys
   and earlier residue) runs its body for the deployment that fires it. That
   deployment pays the body's work through its payer region. Before DR-101
   the body also stored that payer region as the permanent seal of the data
   and continuations it left in shared state, so a later deployment that
   touched the state (a registry lookup, for example) was charged to the
   lane of whichever deployment wrote the state last.

   DR-101 stores a residue region instead: a system (Unit) region bound to
   the paying deployment and to the payer region it stands for. Charging a
   stored seal resolves it: inside the deployment that created it, the
   residue region becomes the payer region again; in every other deployment
   it stays a Unit region, which has no demand. A process held in a variable
   keeps its sender's provenance (P1 rem:signed-subst), so the reducer runs
   it outside system mode and its residue keeps the payer region.

   Hashes are modelled as free constructors of [region_id]: constructor
   injectivity is collision resistance, and distinct constructors are
   domain separation. No axiom, parameter or admission is used.

   Results:
   - unit_payer_keeps_its_region: genesis and system deploys store the same
     seal as before.
   - in_deploy_resolution_returns_the_payer_region and
     in_deploy_charge_refines_inheritance: inside the creating deployment
     the charge equals the charge of the old rule.
   - cross_deploy_residue_has_no_demand, another_payer_never_resolves and
     other_entropy_never_resolves: residue never resolves for another
     deployment, another payer, or a merged (re-derived) entropy.
   - signed_regions_never_resolve and user_term_still_charged: a user seal
     keeps its writer's lane (the DR-68/D5 boundary).
   - no_foreign_lane, system_residue_charges_only_the_current_payer and
     comm_charges_only_payer_or_user_signers: a charge goes to the current
     payer or to a region's own non-Unit signer, never to an earlier payer
     of system residue.
   - demand_is_writer_independent: the demand of system residue does not
     depend on who wrote it.
   - held_process_residue_keeps_its_senders_lane: a process held in a
     variable runs outside system mode, and its residue keeps the payer's
     lane in every later deployment.
   - Negative controls: last_writer_seal_charges_a_foreign_payer (the old
     rule) and never_resolving_loses_in_deploy_charges (no resolution).

   Rust correspondence: rholang/src/rust/interpreter/accounting/authority/
   residue.rs (system_residue_region, resolve_system_residue), reduce.rs
   (eval_send, eval_receive, produce_peeks, without_residue_seal),
   dispatch.rs (system_body), observation_construction.rs
   (comm_with_identity). *)

From Stdlib Require Import Lists.List Arith.PeanoNat.
Import ListNotations.

Inductive sig := Unit | Ground (key : nat).

Inductive region_id :=
| PayerId (s : sig) (entropy : nat)
| ResidueId (deploy : nat) (payer : region_id).

Record region := { rid : region_id; rsig : sig }.

Definition sig_eq_dec (a b : sig) : {a = b} + {a <> b}.
Proof. decide equality; apply Nat.eq_dec. Qed.

Definition region_id_eq_dec (a b : region_id) : {a = b} + {a <> b}.
Proof. decide equality; try apply Nat.eq_dec; apply sig_eq_dec. Qed.

(* cost_region: the region a payer opens for an item of entropy [e]. *)
Definition payer_region (p : sig) (e : nat) : region :=
  {| rid := PayerId p e; rsig := p |}.

(* system_residue_region: the stored seal of residue that [r] pays for in
   deployment [d]. *)
Definition residue_region (d : nat) (r : region) : region :=
  match rsig r with
  | Unit => r
  | Ground _ => {| rid := ResidueId d (rid r); rsig := Unit |}
  end.

(* resolve_system_residue: the region that a stored region charges in the
   deployment of payer [p] and identity [d], for an item of entropy [e]. *)
Definition resolve (p : sig) (d e : nat) (r : region) : region :=
  match p, rsig r with
  | Ground _, Unit =>
      if region_id_eq_dec (rid r) (ResidueId d (PayerId p e))
      then payer_region p e
      else r
  | _, _ => r
  end.

(* The lane that a region charges. A Unit region has no demand. *)
Definition lane (r : region) : option sig :=
  match rsig r with
  | Unit => None
  | Ground _ => Some (rsig r)
  end.

Theorem unit_payer_keeps_its_region :
  forall d e, residue_region d (payer_region Unit e) = payer_region Unit e.
Proof. reflexivity. Qed.

Theorem in_deploy_resolution_returns_the_payer_region :
  forall k d e,
    resolve (Ground k) d e (residue_region d (payer_region (Ground k) e))
    = payer_region (Ground k) e.
Proof.
  intros k d e. unfold resolve, residue_region, payer_region; simpl.
  destruct (region_id_eq_dec (ResidueId d (PayerId (Ground k) e))
                             (ResidueId d (PayerId (Ground k) e))) as [_ | Hne].
  - reflexivity.
  - exfalso; apply Hne; reflexivity.
Qed.

Theorem in_deploy_charge_refines_inheritance :
  forall k d e,
    lane (resolve (Ground k) d e (residue_region d (payer_region (Ground k) e)))
    = lane (payer_region (Ground k) e).
Proof.
  intros k d e. rewrite in_deploy_resolution_returns_the_payer_region. reflexivity.
Qed.

Lemma unresolved_residue_has_no_demand :
  forall p d e k d0 e0,
    resolve p d e (residue_region d0 (payer_region (Ground k) e0))
    = residue_region d0 (payer_region (Ground k) e0) ->
    lane (resolve p d e (residue_region d0 (payer_region (Ground k) e0))) = None.
Proof. intros p d e k d0 e0 Hsame. rewrite Hsame. reflexivity. Qed.

(* Residue resolves only for the payer, deployment and entropy that derived
   it; otherwise it stays as stored. *)
Lemma resolve_residue_cases :
  forall p d e k d0 e0,
    resolve p d e (residue_region d0 (payer_region (Ground k) e0))
    = residue_region d0 (payer_region (Ground k) e0)
    \/ (p = Ground k /\ d = d0 /\ e = e0).
Proof.
  intros p d e k d0 e0.
  destruct p as [| k'].
  - left. reflexivity.
  - unfold resolve, residue_region, payer_region; simpl.
    destruct (region_id_eq_dec (ResidueId d0 (PayerId (Ground k) e0))
                               (ResidueId d (PayerId (Ground k') e))) as [Heq | Hne].
    + right. injection Heq as Hd Hk He. subst. auto.
    + left. reflexivity.
Qed.

Theorem cross_deploy_residue_has_no_demand :
  forall p d e k d0 e0, d <> d0 ->
    lane (resolve p d e (residue_region d0 (payer_region (Ground k) e0))) = None.
Proof.
  intros p d e k d0 e0 Hd.
  destruct (resolve_residue_cases p d e k d0 e0) as [Hsame | [_ [Hdd _]]].
  - apply unresolved_residue_has_no_demand; exact Hsame.
  - contradiction.
Qed.

Theorem another_payer_never_resolves :
  forall p d e k d0 e0, p <> Ground k ->
    lane (resolve p d e (residue_region d0 (payer_region (Ground k) e0))) = None.
Proof.
  intros p d e k d0 e0 Hp.
  destruct (resolve_residue_cases p d e k d0 e0) as [Hsame | [Hpk _]].
  - apply unresolved_residue_has_no_demand; exact Hsame.
  - contradiction.
Qed.

(* A merge re-derives the entropy of a merged item, so merged residue never
   resolves and stays system residue. *)
Theorem other_entropy_never_resolves :
  forall p d e k d0 e0, e <> e0 ->
    lane (resolve p d e (residue_region d0 (payer_region (Ground k) e0))) = None.
Proof.
  intros p d e k d0 e0 He.
  destruct (resolve_residue_cases p d e k d0 e0) as [Hsame | [_ [_ Hee]]].
  - apply unresolved_residue_has_no_demand; exact Hsame.
  - contradiction.
Qed.

Theorem signed_regions_never_resolve :
  forall p d e r, rsig r <> Unit -> resolve p d e r = r.
Proof.
  intros p d e [rid0 [| k]] Hs; simpl in *.
  - contradiction.
  - destruct p; reflexivity.
Qed.

Theorem user_term_still_charged :
  forall p d e k e0, lane (resolve p d e (payer_region (Ground k) e0)) = Some (Ground k).
Proof.
  intros p d e k e0. rewrite signed_regions_never_resolve; [reflexivity | discriminate].
Qed.

Theorem no_foreign_lane :
  forall p d e r s,
    lane (resolve p d e r) = Some s -> s = p \/ (rsig r = s /\ rsig r <> Unit).
Proof.
  intros p d e [rid0 rsig0] s Hlane.
  unfold resolve, lane, payer_region in *; simpl in *.
  destruct p as [| k]; destruct rsig0 as [| k0]; simpl in *.
  - discriminate.
  - injection Hlane as Hs. right. split; [exact Hs | discriminate].
  - destruct (region_id_eq_dec rid0 (ResidueId d (PayerId (Ground k) e))); simpl in *.
    + injection Hlane as Hs. left. symmetry. exact Hs.
    + discriminate.
  - injection Hlane as Hs. right. split; [exact Hs | discriminate].
Qed.

Theorem system_residue_charges_only_the_current_payer :
  forall p d e r s, rsig r = Unit -> lane (resolve p d e r) = Some s -> s = p.
Proof.
  intros p d e r s Hunit Hlane.
  destruct (no_foreign_lane p d e r s Hlane) as [Hp | [Hs Hne]].
  - exact Hp.
  - contradiction.
Qed.

(* comm_with_identity: the lanes of a COMM are the lanes of its participants'
   resolved seals, each with the participant's own entropy. *)
Definition comm_lanes (p : sig) (d : nat) (participants : list (nat * region)) :
    list (option sig) :=
  map (fun participant => lane (resolve p d (fst participant) (snd participant)))
      participants.

Theorem comm_charges_only_payer_or_user_signers :
  forall p d participants s,
    In (Some s) (comm_lanes p d participants) ->
    s = p \/ exists participant, In participant participants
                                 /\ rsig (snd participant) = s
                                 /\ rsig (snd participant) <> Unit.
Proof.
  intros p d participants s Hin.
  unfold comm_lanes in Hin. apply in_map_iff in Hin.
  destruct Hin as [participant [Hlane Hmember]].
  destruct (no_foreign_lane p d (fst participant) (snd participant) s Hlane)
    as [Hp | [Hs Hne]].
  - left; exact Hp.
  - right. exists participant. auto.
Qed.

Theorem demand_is_writer_independent :
  forall p d e k1 d1 e1 k2 d2 e2, d1 <> d -> d2 <> d ->
    lane (resolve p d e (residue_region d1 (payer_region (Ground k1) e1)))
    = lane (resolve p d e (residue_region d2 (payer_region (Ground k2) e2))).
Proof.
  intros p d e k1 d1 e1 k2 d2 e2 H1 H2.
  rewrite (cross_deploy_residue_has_no_demand p d e k1 d1 e1) by congruence.
  rewrite (cross_deploy_residue_has_no_demand p d e k2 d2 e2) by congruence.
  reflexivity.
Qed.

(* eval_send and eval_receive: the seal that a body stores in a mode. A
   system body stores residue; any other body, including a process held in a
   variable that a system body runs (without_residue_seal), stores the payer
   region. *)
Inductive mode := SystemBody | UserCode.

Definition stored_seal (m : mode) (p : sig) (d e : nat) : region :=
  match m with
  | SystemBody => residue_region d (payer_region p e)
  | UserCode => payer_region p e
  end.

Theorem held_process_residue_keeps_its_senders_lane :
  forall k d e p' d' e',
    lane (resolve p' d' e' (stored_seal UserCode (Ground k) d e)) = Some (Ground k).
Proof. intros. apply user_term_still_charged. Qed.

(* Negative control: the old rule stored the payer region itself, so a later
   deployment of another payer is charged the writer's lane. *)
Definition last_writer_seal (d : nat) (r : region) : region := r.

Theorem last_writer_seal_charges_a_foreign_payer :
  exists k k' d d' e e',
    k <> k' /\
    lane (resolve (Ground k') d' e' (last_writer_seal d (payer_region (Ground k) e)))
    = Some (Ground k).
Proof.
  exists 1, 2, 0, 1, 0, 0. split; [discriminate | reflexivity].
Qed.

(* Negative control: a seal that never resolves loses the charges inside the
   creating deployment. *)
Theorem never_resolving_loses_in_deploy_charges :
  exists k d e,
    lane (residue_region d (payer_region (Ground k) e))
    <> lane (payer_region (Ground k) e).
Proof.
  exists 1, 0, 0. simpl. discriminate.
Qed.
