From Stdlib Require Import Lists.List Arith Lia.
Import ListNotations.

Section PublicationOwnership.
Context {Key : Type}.
Context (key_eq : forall x y : Key, {x = y} + {x <> y}).

Record ownership_state := OwnershipState {
  durable : Key -> Prop;
  terminal : Key -> Prop;
  requested : Key -> Prop;
  seen : Key -> Prop;
  acknowledged : Key -> Prop;
  cached : list Key
}.

Definition initial : ownership_state :=
  OwnershipState (fun _ => False) (fun _ => False) (fun _ => False)
    (fun _ => False) (fun _ => False) [].

Definition inv_cache_ownership (s : ownership_state) : Prop :=
  forall k, In k (cached s) -> durable s k.

Definition inv_acknowledged_durability (s : ownership_state) : Prop :=
  forall k, acknowledged s k -> durable s k \/ terminal s k.

Definition inv_live_retry (s : ownership_state) : Prop :=
  forall k, seen s k -> durable s k \/ terminal s k \/ requested s k.

Definition invariant (cap : nat) (s : ownership_state) : Prop :=
  inv_cache_ownership s /\ inv_acknowledged_durability s /\
  inv_live_retry s /\ length (cached s) <= cap.

Definition begin_request (b : Key) (s : ownership_state) : ownership_state :=
  OwnershipState (durable s) (terminal s)
    (fun k => k = b \/ requested s k)
    (fun k => k = b \/ seen s k) (acknowledged s) (cached s).

Definition commit (b : Key) (s : ownership_state) : ownership_state :=
  OwnershipState (fun k => k = b \/ durable s k) (terminal s)
    (requested s) (seen s) (acknowledged s) (cached s).

Definition acknowledge (b : Key) (s : ownership_state) : ownership_state :=
  OwnershipState (durable s) (terminal s)
    (fun k => requested s k /\ k <> b) (seen s)
    (fun k => k = b \/ acknowledged s k) (cached s).

Definition replace_cache (keys : list Key) (s : ownership_state) : ownership_state :=
  OwnershipState (durable s) (terminal s) (requested s) (seen s)
    (acknowledged s) keys.

Definition resolve (b : Key) (s : ownership_state) : ownership_state :=
  OwnershipState (fun k => durable s k /\ k <> b)
    (fun k => k = b \/ terminal s k)
    (fun k => requested s k /\ k <> b) (seen s) (acknowledged s)
    (filter (fun k => if key_eq k b then false else true) (cached s)).

Definition restart (s : ownership_state) : ownership_state :=
  OwnershipState (durable s) (terminal s) (fun _ => False)
    (fun k => durable s k \/ terminal s k \/ acknowledged s k)
    (acknowledged s) [].

Theorem initial_invariant : forall cap, invariant cap initial.
Proof.
  intros. unfold invariant, inv_cache_ownership, inv_acknowledged_durability,
    inv_live_retry, initial. simpl. intuition lia.
Qed.

Theorem begin_preserves : forall cap b s,
  invariant cap s -> invariant cap (begin_request b s).
Proof.
  intros cap b s [HC [HA [HL HB]]].
  unfold invariant, inv_cache_ownership, inv_acknowledged_durability,
    inv_live_retry in *. simpl.
  refine (conj HC (conj HA (conj _ HB))).
  intros k [E|HS].
  - right. right. left. exact E.
  - destruct (HL k HS) as [HD|[HT|HR]]; auto.
Qed.

Theorem commit_preserves : forall cap b s,
  invariant cap s -> invariant cap (commit b s).
Proof.
  intros cap b s [HC [HA [HL HB]]].
  unfold invariant, inv_cache_ownership, inv_acknowledged_durability,
    inv_live_retry in *. simpl.
  refine (conj _ (conj _ (conj _ HB))).
  - intros k H. right. apply HC. exact H.
  - intros k H. destruct (HA k H); auto.
  - intros k H. destruct (HL k H) as [HD|[HT|HR]]; auto.
Qed.

Theorem acknowledge_preserves : forall cap b s,
  invariant cap s -> durable s b \/ terminal s b ->
  invariant cap (acknowledge b s).
Proof.
  intros cap b s [HC [HA [HL HB]]] HO.
  unfold invariant, inv_cache_ownership, inv_acknowledged_durability,
    inv_live_retry in *. simpl.
  refine (conj HC (conj _ (conj _ HB))).
  - intros k [E|H]; [subst; exact HO|apply HA; exact H].
  - intros k HS. destruct (key_eq k b) as [E|NE].
    + subst. destruct HO; auto.
    + destruct (HL k HS) as [HD|[HT|HR]]; auto.
Qed.

Theorem cache_replacement_preserves : forall cap keys s,
  invariant cap s -> (forall k, In k keys -> durable s k) ->
  length keys <= cap -> invariant cap (replace_cache keys s).
Proof.
  intros cap keys s [HC [HA [HL HB]]] HK HN.
  unfold invariant, inv_cache_ownership, inv_acknowledged_durability,
    inv_live_retry in *. simpl. auto.
Qed.

Lemma filtered_cache_length : forall (predicate : Key -> bool) keys,
  length (filter predicate keys) <= length keys.
Proof.
  intros predicate keys. induction keys as [|key keys IH]; simpl; [lia|].
  destruct (predicate key); simpl; lia.
Qed.

Theorem resolve_preserves : forall cap b s,
  invariant cap s -> invariant cap (resolve b s).
Proof.
  intros cap b s [HC [HA [HL HB]]].
  unfold invariant, inv_cache_ownership, inv_acknowledged_durability,
    inv_live_retry in *. simpl.
  refine (conj _ (conj _ (conj _ _))).
  - intros k H. apply filter_In in H. destruct H as [HI HF].
    split; [apply HC; exact HI|].
    destruct (key_eq k b); [discriminate|assumption].
  - intros k H. destruct (key_eq k b) as [E|NE].
    + auto.
    + destruct (HA k H); auto.
  - intros k H. destruct (key_eq k b) as [E|NE].
    + auto.
    + destruct (HL k H) as [HD|[HT|HR]]; auto.
  - eapply Nat.le_trans; [apply filtered_cache_length|exact HB].
Qed.

Theorem restart_preserves : forall cap s,
  invariant cap s -> invariant cap (restart s).
Proof.
  intros cap s [HC [HA [HL HB]]].
  unfold invariant, inv_cache_ownership, inv_acknowledged_durability,
    inv_live_retry in *. simpl.
  refine (conj _ (conj HA (conj _ _))).
  - intros k H. contradiction.
  - intros k [HD|[HT|HK]]; [auto|auto|]. destruct (HA k HK); auto.
  - lia.
Qed.

Inductive transition (cap : nat) : ownership_state -> ownership_state -> Prop :=
| Begin : forall b s, transition cap s (begin_request b s)
| Commit : forall b s, transition cap s (commit b s)
| Acknowledge : forall b s, durable s b \/ terminal s b ->
    transition cap s (acknowledge b s)
| CacheOrEvict : forall keys s,
    (forall k, In k keys -> durable s k) -> length keys <= cap ->
    transition cap s (replace_cache keys s)
| Resolve : forall b s, transition cap s (resolve b s)
| Restart : forall s, transition cap s (restart s)
| FailOrRelease : forall s, transition cap s s.

Inductive history (cap : nat) : ownership_state -> ownership_state -> Prop :=
| NoSteps : forall s, history cap s s
| MoreSteps : forall before middle after,
    history cap before middle -> transition cap middle after ->
    history cap before after.

Theorem transition_preserves : forall cap before after,
  transition cap before after -> invariant cap before -> invariant cap after.
Proof.
  intros cap before after H. destruct H; intros;
    eauto using begin_preserves, commit_preserves, acknowledge_preserves,
      cache_replacement_preserves, resolve_preserves, restart_preserves.
Qed.

Theorem history_preserves : forall cap before after,
  history cap before after -> invariant cap before -> invariant cap after.
Proof.
  intros cap before after H. induction H; intros; eauto using transition_preserves.
Qed.

Theorem every_history_retains_acknowledged_ownership : forall cap s b,
  history cap initial s -> acknowledged s b -> durable s b \/ terminal s b.
Proof.
  intros cap s b HH HA.
  pose proof (history_preserves _ _ _ HH (initial_invariant cap)) as H.
  destruct H as [_ [HO _]]. exact (HO b HA).
Qed.

Theorem every_history_retains_live_retry : forall cap s b,
  history cap initial s -> seen s b -> durable s b \/ terminal s b \/ requested s b.
Proof.
  intros cap s b HH HS.
  pose proof (history_preserves _ _ _ HH (initial_invariant cap)) as H.
  destruct H as [_ [_ [HL _]]]. exact (HL b HS).
Qed.

Theorem every_history_bounds_cache : forall cap s,
  history cap initial s -> length (cached s) <= cap.
Proof.
  intros cap s HH.
  pose proof (history_preserves _ _ _ HH (initial_invariant cap)) as H.
  destruct H as [_ [_ [_ HB]]]. exact HB.
Qed.

Theorem unowned_acknowledgement_is_unsafe : forall s b,
  ~ durable s b -> ~ terminal s b ->
  ~ inv_acknowledged_durability (acknowledge b s).
Proof.
  intros s b HD HT HA.
  specialize (HA b (or_introl eq_refl)). simpl in HA. tauto.
Qed.

Theorem eviction_preserves_retry_ownership : forall keys s b,
  durable (replace_cache keys s) b <-> durable s b.
Proof. reflexivity. Qed.

End PublicationOwnership.

Print Assumptions initial_invariant.
Print Assumptions begin_preserves.
Print Assumptions commit_preserves.
Print Assumptions acknowledge_preserves.
Print Assumptions cache_replacement_preserves.
Print Assumptions resolve_preserves.
Print Assumptions restart_preserves.
Print Assumptions transition_preserves.
Print Assumptions history_preserves.
Print Assumptions every_history_retains_acknowledged_ownership.
Print Assumptions every_history_retains_live_retry.
Print Assumptions every_history_bounds_cache.
Print Assumptions unowned_acknowledgement_is_unsafe.
Print Assumptions eviction_preserves_retry_ownership.
