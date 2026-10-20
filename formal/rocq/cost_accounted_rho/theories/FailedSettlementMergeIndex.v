(** * FailedSettlementMergeIndex

    DR-115 (bug 10986): the merge index of a failed offered deploy.

    A processed deploy's log is its user events followed by its settlement
    events (the wallet settlement and the receipts). A user failure rolls the
    user events back and commits the settlement events. The operation
    journal records one completion per user operation: a rejected operation
    logs no event, a stored one logs one event, and a matched one logs two
    (its introduction and its COMM). Replay binds the log prefix of the
    journal's width to the user trace, so the journal fixes the number of user
    events of a validated record.

    The merge index takes the committed part of the log:
    - a successful deploy: the whole log;
    - a failed legacy deploy: nothing (the dev rule, unchanged);
    - a failed offered deploy: the suffix after the user events.

    A block on the merge base holds its commitment in the base state. A block
    in a merged branch contributes its indexed chain, which the merge applies
    or rejects with a record. Events and map entries are modeled as natural
    numbers. *)

From Stdlib Require Import Lists.List PeanoNat.
Import ListNotations.

Inductive deploy_kind : Type := LegacyDeploy | OfferedDeploy.
Inductive deploy_outcome : Type := Succeeded | Failed.
Inductive completion : Type := Rejected | Stored | Matched.

Definition completion_width (operation : completion) : nat :=
  match operation with
  | Rejected => 0
  | Stored => 1
  | Matched => 2
  end.

Fixpoint user_event_count (journal : list completion) : nat :=
  match journal with
  | [] => 0
  | operation :: rest => completion_width operation + user_event_count rest
  end.

Record processed : Type := {
  kind : deploy_kind;
  outcome : deploy_outcome;
  journal : list completion;
  user_events : list nat;
  settlement_events : list nat;
  user_map : list nat;
  wallet_map : list nat
}.

(** The replay binding: a validated record has exactly as many user events as
    its journal's width. *)
Definition journal_bound (d : processed) : Prop :=
  user_event_count (journal d) = length (user_events d).

Definition deploy_log (d : processed) : list nat :=
  user_events d ++ settlement_events d.

(** What the block's post-state holds. *)
Definition committed_events (d : processed) : list nat :=
  match outcome d with
  | Succeeded => deploy_log d
  | Failed => settlement_events d
  end.

(** The stored mergeable map: the user map joins the wallet map on success. *)
Definition stored_map (d : processed) : list nat :=
  match outcome d with
  | Succeeded => user_map d ++ wallet_map d
  | Failed => wallet_map d
  end.

(** The DR-115 index rule, [ProcessedIndexDeploy::committed_log]. *)
Definition committed_log (d : processed) : option (list nat) :=
  match outcome d, kind d with
  | Succeeded, _ => Some (deploy_log d)
  | Failed, LegacyDeploy => None
  | Failed, OfferedDeploy =>
      Some (skipn (user_event_count (journal d)) (deploy_log d))
  end.

(** The contribution of a block in a merged branch: its indexed chain when
    the merge keeps it, nothing when the merge rejects it. *)
Definition branch_events (d : processed) (rejected : bool) : list nat :=
  match committed_log d with
  | Some events => if rejected then [] else events
  | None => []
  end.

Definition branch_map (d : processed) (rejected : bool) : list nat :=
  match committed_log d with
  | Some _ => if rejected then [] else stored_map d
  | None => []
  end.

(** A rejection leaves a record only for a chain that exists. *)
Definition rejection_record (d : processed) (rejected : bool) : bool :=
  match committed_log d with
  | Some _ => rejected
  | None => false
  end.

(** M2' (user decision 2026-10-08): the applied set holds the deploy of every
    kept branch chain. *)
Definition applied_from_scope (d : processed) (rejected : bool) : bool :=
  match committed_log d with
  | Some _ => negb rejected
  | None => false
  end.

Lemma skipn_length_app :
  forall (A : Type) (prefix suffix : list A),
    skipn (length prefix) (prefix ++ suffix) = suffix.
Proof.
  intros A prefix suffix.
  induction prefix as [| head prefix IH]; simpl; auto.
Qed.

Lemma firstn_length_app :
  forall (A : Type) (prefix suffix : list A),
    firstn (length prefix) (prefix ++ suffix) = prefix.
Proof.
  intros A prefix suffix.
  induction prefix as [| head prefix IH]; simpl.
  - reflexivity.
  - rewrite IH. reflexivity.
Qed.

Lemma nodup_app_disjoint :
  forall (A : Type) (left right : list A) (value : A),
    NoDup (left ++ right) -> In value left -> ~ In value right.
Proof.
  intros A left right value Hnodup.
  induction left as [| head left IH]; simpl; intros Hin.
  - contradiction.
  - inversion Hnodup as [| element rest Hfresh Hrest]; subst.
    destruct Hin as [Hhead | Htail].
    + subst. intros Hright. apply Hfresh. apply in_or_app. right. exact Hright.
    + apply IH; assumption.
Qed.

(** ** The index rule *)

Theorem failed_offered_index_is_settlement_suffix :
  forall d,
    journal_bound d ->
    outcome d = Failed ->
    kind d = OfferedDeploy ->
    committed_log d = Some (settlement_events d).
Proof.
  intros d Hbound Hfailed Hkind.
  unfold committed_log, deploy_log.
  rewrite Hfailed, Hkind, Hbound.
  rewrite skipn_length_app. reflexivity.
Qed.

Theorem failed_offered_index_is_the_committed_part :
  forall d,
    journal_bound d ->
    outcome d = Failed ->
    kind d = OfferedDeploy ->
    committed_log d = Some (committed_events d).
Proof.
  intros d Hbound Hfailed Hkind.
  unfold committed_events. rewrite Hfailed.
  apply failed_offered_index_is_settlement_suffix; assumption.
Qed.

Theorem failed_offered_index_has_no_rolled_back_event :
  forall d events value,
    journal_bound d ->
    NoDup (deploy_log d) ->
    outcome d = Failed ->
    kind d = OfferedDeploy ->
    committed_log d = Some events ->
    In value (user_events d) ->
    ~ In value events.
Proof.
  intros d events value Hbound Hnodup Hfailed Hkind Hlog Hin.
  rewrite (failed_offered_index_is_settlement_suffix d Hbound Hfailed Hkind) in Hlog.
  injection Hlog as Hevents. subst events.
  apply (nodup_app_disjoint nat (user_events d)); assumption.
Qed.

Theorem successful_index_is_whole_log :
  forall d, outcome d = Succeeded -> committed_log d = Some (deploy_log d).
Proof.
  intros d Hsucceeded. unfold committed_log. rewrite Hsucceeded. reflexivity.
Qed.

Theorem failed_legacy_index_is_empty :
  forall d,
    outcome d = Failed -> kind d = LegacyDeploy -> committed_log d = None.
Proof.
  intros d Hfailed Hkind. unfold committed_log. rewrite Hfailed, Hkind. reflexivity.
Qed.

(** Replay binds the prefix of the journal's width, and the merge index takes
    the rest: the two parts are the user events and the whole log. *)
Theorem replay_prefix_and_index_partition_the_log :
  forall d,
    journal_bound d ->
    firstn (user_event_count (journal d)) (deploy_log d) = user_events d /\
    firstn (user_event_count (journal d)) (deploy_log d) ++
      skipn (user_event_count (journal d)) (deploy_log d) = deploy_log d.
Proof.
  intros d Hbound. split.
  - unfold deploy_log. rewrite Hbound. apply firstn_length_app.
  - apply firstn_skipn.
Qed.

(** ** The merge *)

(** A kept branch chain of a failed offered deploy brings exactly what a
    block on the merge base holds: its committed events and its stored map. *)
Theorem branch_merge_equals_base_commitment :
  forall d,
    journal_bound d ->
    outcome d = Failed ->
    kind d = OfferedDeploy ->
    branch_events d false = committed_events d /\
    branch_map d false = stored_map d.
Proof.
  intros d Hbound Hfailed Hkind.
  unfold branch_events, branch_map.
  rewrite (failed_offered_index_is_settlement_suffix d Hbound Hfailed Hkind).
  unfold committed_events. rewrite Hfailed. split; reflexivity.
Qed.

Theorem failed_wallet_delta_survives_branch_merge :
  forall d,
    journal_bound d ->
    outcome d = Failed ->
    kind d = OfferedDeploy ->
    branch_map d false = wallet_map d.
Proof.
  intros d Hbound Hfailed Hkind.
  destruct (branch_merge_equals_base_commitment d Hbound Hfailed Hkind) as [_ Hmap].
  rewrite Hmap. unfold stored_map. rewrite Hfailed. reflexivity.
Qed.

(** An offered deploy in a merged branch brings its whole commitment, or the
    merge records its rejection. It is never dropped silently. *)
Theorem offered_branch_never_drops_silently :
  forall d rejected,
    journal_bound d ->
    kind d = OfferedDeploy ->
    (branch_events d rejected = committed_events d /\
     branch_map d rejected = stored_map d) \/
    rejection_record d rejected = true.
Proof.
  intros d rejected Hbound Hkind.
  destruct rejected.
  - right. unfold rejection_record, committed_log.
    destruct (outcome d); [reflexivity | rewrite Hkind; reflexivity].
  - left. destruct (outcome d) eqn:Houtcome.
    + unfold branch_events, branch_map, committed_events, stored_map, committed_log.
      rewrite Houtcome. split; reflexivity.
    + apply branch_merge_equals_base_commitment; assumption.
Qed.

(** M2': every kept branch chain with an effect is in the applied set, so no
    node executes the deploy again on top of its own settlement. *)
Theorem applied_set_covers_every_kept_settlement :
  forall d rejected,
    branch_events d rejected <> [] -> applied_from_scope d rejected = true.
Proof.
  intros d rejected Hnonempty.
  unfold branch_events, applied_from_scope in *.
  destruct (committed_log d) as [events |]; destruct rejected; simpl in *;
    try reflexivity; exfalso; apply Hnonempty; reflexivity.
Qed.

(** ** Negative controls *)

(** A validated failed offered record: one matched operation (two user
    events), then two settlement events. *)
Definition failed_offered_witness : processed :=
  {| kind := OfferedDeploy;
     outcome := Failed;
     journal := [Matched];
     user_events := [1; 2];
     settlement_events := [3; 4];
     user_map := [10];
     wallet_map := [20] |}.

Definition failed_legacy_witness : processed :=
  {| kind := LegacyDeploy;
     outcome := Failed;
     journal := [Matched];
     user_events := [1; 2];
     settlement_events := [3; 4];
     user_map := [10];
     wallet_map := [20] |}.

Lemma failed_offered_witness_is_bound : journal_bound failed_offered_witness.
Proof. reflexivity. Qed.

Lemma failed_offered_witness_index :
  committed_log failed_offered_witness = Some [3; 4].
Proof. reflexivity. Qed.

Definition events_of (rule : processed -> option (list nat)) (d : processed) : list nat :=
  match rule d with
  | Some events => events
  | None => []
  end.

(** The HEAD rule: skip every failed deploy. *)
Definition skip_rule (d : processed) : option (list nat) :=
  match outcome d with
  | Succeeded => Some (deploy_log d)
  | Failed => None
  end.

Definition full_log_rule (d : processed) : option (list nat) := Some (deploy_log d).

Definition user_prefix_rule (d : processed) : option (list nat) :=
  match outcome d with
  | Succeeded => Some (deploy_log d)
  | Failed => Some (firstn (user_event_count (journal d)) (deploy_log d))
  end.

Definition shifted_boundary_rule (low : bool) (d : processed) : option (list nat) :=
  match outcome d with
  | Succeeded => Some (deploy_log d)
  | Failed =>
      Some (skipn (if low then user_event_count (journal d) - 1
                   else user_event_count (journal d) + 1) (deploy_log d))
  end.

Definition dropped_wallet_map (d : processed) : list nat :=
  match outcome d with
  | Succeeded => stored_map d
  | Failed => []
  end.

Definition legacy_extension_rule (d : processed) : option (list nat) :=
  match outcome d with
  | Succeeded => Some (deploy_log d)
  | Failed => Some (skipn (user_event_count (journal d)) (deploy_log d))
  end.

(** M1: keep a failed deploy out of the applied set. *)
Definition applied_without_failed (d : processed) (rejected : bool) : bool :=
  match outcome d with
  | Succeeded => applied_from_scope d rejected
  | Failed => false
  end.

Theorem skip_rule_drops_failed_settlement :
  events_of skip_rule failed_offered_witness <> committed_events failed_offered_witness.
Proof. simpl. discriminate. Qed.

Theorem full_log_rule_indexes_rolled_back_event :
  In 1 (user_events failed_offered_witness) /\
  In 1 (events_of full_log_rule failed_offered_witness).
Proof. simpl. split; left; reflexivity. Qed.

Theorem user_prefix_rule_drops_settlement :
  events_of user_prefix_rule failed_offered_witness <> committed_events failed_offered_witness.
Proof. simpl. discriminate. Qed.

Theorem low_boundary_indexes_rolled_back_event :
  In 2 (user_events failed_offered_witness) /\
  In 2 (events_of (shifted_boundary_rule true) failed_offered_witness).
Proof. simpl. split; [right; left; reflexivity | left; reflexivity]. Qed.

Theorem high_boundary_drops_settlement_event :
  In 3 (committed_events failed_offered_witness) /\
  ~ In 3 (events_of (shifted_boundary_rule false) failed_offered_witness).
Proof.
  simpl. split.
  - left. reflexivity.
  - intros [Hfour | []]. discriminate.
Qed.

Theorem dropped_wallet_map_loses_charge :
  dropped_wallet_map failed_offered_witness <> wallet_map failed_offered_witness.
Proof. simpl. discriminate. Qed.

Theorem legacy_extension_changes_failed_legacy_index :
  legacy_extension_rule failed_legacy_witness <> committed_log failed_legacy_witness.
Proof. simpl. discriminate. Qed.

Theorem excluding_failed_deploys_misses_a_kept_settlement :
  branch_events failed_offered_witness false <> [] /\
  applied_without_failed failed_offered_witness false = false.
Proof. simpl. split; [discriminate | reflexivity]. Qed.
