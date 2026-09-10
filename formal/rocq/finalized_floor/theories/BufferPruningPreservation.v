From Stdlib Require Import Lists.List Arith Lia.
Import ListNotations.

Section Pruning.
Context {Key : Type}.
Context (key_eq : forall x y : Key, {x = y} + {x <> y}).
Context (required : Key -> Key -> Prop).
Context (terminal : Key -> Prop).

Record buffer_state := BufferState {
  seen : Key -> Prop;
  rows : Key -> Prop;
  edges : Key -> Key -> Prop;
  proven : Key -> Prop;
  cache : list Key
}.

Definition initial : buffer_state :=
  BufferState (fun _ => False) (fun _ => False)
    (fun _ _ => False) (fun _ => False) [].

Definition durable_owner (s : buffer_state) : Prop :=
  forall b, seen s b -> ~ proven s b -> rows s b.

Definition unresolved_edges (s : buffer_state) : Prop :=
  forall b, rows s b -> forall p,
    required b p -> ~ proven s p -> edges s b p.

Definition cache_owned (s : buffer_state) : Prop :=
  forall b, In b (cache s) -> rows s b.

Definition terminal_sound (s : buffer_state) : Prop :=
  forall b, proven s b -> terminal b.

Definition invariant (cap : nat) (s : buffer_state) : Prop :=
  durable_owner s /\ unresolved_edges s /\ cache_owned s /\
  length (cache s) <= cap /\ terminal_sound s.

Definition publish (b : Key) (s : buffer_state) : buffer_state :=
  BufferState
    (fun k => k = b \/ seen s k)
    (fun k => k = b \/ rows s k)
    (fun k p => if key_eq k b
                then required k p /\ ~ proven s p
                else edges s k p)
    (proven s) (cache s).

Definition resolve (b : Key) (s : buffer_state) : buffer_state :=
  BufferState
    (seen s)
    (fun k => rows s k /\ k <> b)
    (fun k p => edges s k p /\ p <> b)
    (fun k => proven s k \/ k = b)
    (filter (fun k => if key_eq k b then false else true) (cache s)).

Definition replace_cache (keys : list Key) (s : buffer_state) : buffer_state :=
  BufferState (seen s) (rows s) (edges s) (proven s) keys.

Definition forget_row (b : Key) (s : buffer_state) : buffer_state :=
  BufferState (seen s) (fun k => rows s k /\ k <> b)
    (edges s) (proven s) [].

Definition erase_edge (b : Key) (s : buffer_state) : buffer_state :=
  BufferState (seen s) (rows s)
    (fun k p => edges s k p /\ p <> b) (proven s) (cache s).

Lemma filtered_length_le : forall (predicate : Key -> bool) keys,
  length (filter predicate keys) <= length keys.
Proof.
  intros predicate keys. induction keys as [|key keys IH]; simpl; [lia|].
  destruct (predicate key); simpl; lia.
Qed.

Theorem initial_invariant : forall cap, invariant cap initial.
Proof.
  intros cap. unfold invariant, durable_owner, unresolved_edges, cache_owned,
    terminal_sound, initial. simpl. intuition lia.
Qed.

Theorem publish_preserves : forall cap b s,
  invariant cap s -> invariant cap (publish b s).
Proof.
  intros cap b s [Howner [Hedges [Hcache [Hbound Hterminal]]]].
  unfold invariant, durable_owner, unresolved_edges, cache_owned, terminal_sound in *.
  simpl. refine (conj _ (conj _ (conj _ (conj _ _)))).
  - intros k [E|H] HP; [left; exact E|right; eauto].
  - intros k H p HQ HP. destruct (key_eq k b) as [E|NE].
    + auto.
    + destruct H as [E|H]; [contradiction|]. eauto.
  - intros k H. right. apply Hcache. exact H.
  - exact Hbound.
  - exact Hterminal.
Qed.

Theorem resolve_preserves : forall cap b s,
  invariant cap s -> terminal b -> invariant cap (resolve b s).
Proof.
  intros cap b s [Howner [Hedges [Hcache [Hbound Hterminal]]]] HT.
  unfold invariant, durable_owner, unresolved_edges, cache_owned, terminal_sound in *.
  simpl. refine (conj _ (conj _ (conj _ (conj _ _)))).
  - intros k HS HP. split.
    + apply Howner; [exact HS|]. intro H. apply HP. left. exact H.
    + intro E. apply HP. right. exact E.
  - intros k [HR NE] p HQ HP. split.
    + apply Hedges; [exact HR|exact HQ|].
      intro H. apply HP. left. exact H.
    + intro E. apply HP. right. exact E.
  - intros k H. apply filter_In in H. destruct H as [HI HF].
    split; [apply Hcache; exact HI|].
    destruct (key_eq k b); [discriminate|assumption].
  - eapply Nat.le_trans; [apply filtered_length_le|exact Hbound].
  - intros k [H|E]; [apply Hterminal; exact H|subst; exact HT].
Qed.

Theorem cache_replacement_preserves : forall cap keys s,
  invariant cap s ->
  (forall k, In k keys -> rows s k) ->
  length keys <= cap ->
  invariant cap (replace_cache keys s).
Proof.
  intros cap keys s [HO [HE [HC [HB HT]]]] HK HL.
  unfold invariant, durable_owner, unresolved_edges, cache_owned, terminal_sound in *.
  simpl. auto.
Qed.

Theorem restart_preserves : forall cap s,
  invariant cap s -> invariant cap (replace_cache [] s).
Proof.
  intros. apply cache_replacement_preserves; auto.
  - simpl. contradiction.
  - simpl. lia.
Qed.

Inductive transition (cap : nat) : buffer_state -> buffer_state -> Prop :=
| PublishRow : forall b s, transition cap s (publish b s)
| ResolveProven : forall b s, terminal b -> transition cap s (resolve b s)
| PageOrEvict : forall keys s,
    (forall k, In k keys -> rows s k) ->
    length keys <= cap ->
    transition cap s (replace_cache keys s)
| AcknowledgeOrFailedWrite : forall s, transition cap s s.

Inductive history (cap : nat) : buffer_state -> buffer_state -> Prop :=
| NoSteps : forall s, history cap s s
| MoreSteps : forall before middle after,
    history cap before middle ->
    transition cap middle after ->
    history cap before after.

Theorem transition_preserves : forall cap before after,
  transition cap before after ->
  invariant cap before -> invariant cap after.
Proof.
  intros cap before after H. destruct H; intros;
    eauto using publish_preserves, resolve_preserves, cache_replacement_preserves.
Qed.

Theorem history_preserves : forall cap before after,
  history cap before after ->
  invariant cap before -> invariant cap after.
Proof.
  intros cap before after H. induction H; intros; eauto using transition_preserves.
Qed.

Theorem every_history_preserves_retry : forall cap s b,
  history cap initial s -> seen s b -> ~ proven s b -> rows s b.
Proof.
  intros cap s b HH HS HP.
  pose proof (history_preserves _ _ _ HH (initial_invariant cap)) as H.
  destruct H as [HO _]. exact (HO b HS HP).
Qed.

Theorem every_history_preserves_unresolved_edges : forall cap s b p,
  history cap initial s -> rows s b ->
  required b p -> ~ proven s p -> edges s b p.
Proof.
  intros cap s b p HH HR HQ HP.
  pose proof (history_preserves _ _ _ HH (initial_invariant cap)) as H.
  destruct H as [_ [HE _]]. exact (HE b HR p HQ HP).
Qed.

Theorem every_history_bounds_cache : forall cap s,
  history cap initial s -> length (cache s) <= cap.
Proof.
  intros cap s HH.
  pose proof (history_preserves _ _ _ HH (initial_invariant cap)) as H.
  destruct H as [_ [_ [_ [HB _]]]]. exact HB.
Qed.

Theorem eviction_cannot_change_durable_rows : forall keys s b,
  rows (replace_cache keys s) b <-> rows s b.
Proof. reflexivity. Qed.

Theorem eviction_cannot_change_dependency_edges : forall keys s b p,
  edges (replace_cache keys s) b p <-> edges s b p.
Proof. reflexivity. Qed.

Theorem deleting_unresolved_owner_is_unsafe : forall s b,
  seen s b -> ~ proven s b -> ~ durable_owner (forget_row b s).
Proof.
  intros s b HS HP HO.
  specialize (HO b HS HP). simpl in HO.
  destruct HO as [_ H]. apply H. reflexivity.
Qed.

Theorem deleting_unresolved_edge_is_unsafe : forall s b p,
  rows s b -> required b p -> ~ proven s p ->
  ~ unresolved_edges (erase_edge p s).
Proof.
  intros s b p HR HQ HP HE.
  specialize (HE b HR p HQ HP). simpl in HE.
  destruct HE as [_ H]. apply H. reflexivity.
Qed.

End Pruning.

Print Assumptions initial_invariant.
Print Assumptions publish_preserves.
Print Assumptions resolve_preserves.
Print Assumptions cache_replacement_preserves.
Print Assumptions restart_preserves.
Print Assumptions transition_preserves.
Print Assumptions history_preserves.
Print Assumptions every_history_preserves_retry.
Print Assumptions every_history_preserves_unresolved_edges.
Print Assumptions every_history_bounds_cache.
Print Assumptions eviction_cannot_change_durable_rows.
Print Assumptions eviction_cannot_change_dependency_edges.
Print Assumptions deleting_unresolved_owner_is_unsafe.
Print Assumptions deleting_unresolved_edge_is_unsafe.
