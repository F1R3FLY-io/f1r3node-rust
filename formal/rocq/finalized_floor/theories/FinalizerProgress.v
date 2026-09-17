From Stdlib Require Import Lists.List.
From Stdlib Require Import Bool.Bool.
Import ListNotations.

Inductive scan_result (A : Type) : Type :=
| Selected : A -> scan_result A
| Exhausted : scan_result A
| Inconclusive : A -> scan_result A.

Arguments Selected {A} _.
Arguments Exhausted {A}.
Arguments Inconclusive {A} _.

Fixpoint scan {A : Type} (decides : A -> option bool) (candidates : list A) : scan_result A :=
  match candidates with
  | [] => Exhausted
  | candidate :: remaining =>
      match decides candidate with
      | None => Inconclusive candidate
      | Some true => Selected candidate
      | Some false => scan decides remaining
      end
  end.

Lemma scan_selected_sound :
  forall (A : Type) (decides : A -> option bool) candidates selected,
    scan decides candidates = Selected selected ->
    In selected candidates /\ decides selected = Some true.
Proof.
  intros A decides candidates.
  induction candidates as [|candidate remaining IH]; intros selected Hscan.
  - discriminate.
  - simpl in Hscan.
    destruct (decides candidate) as [[|]|] eqn:Hdecision.
    + inversion Hscan; subst. split; [left; reflexivity | exact Hdecision].
    + apply IH in Hscan. destruct Hscan as [Hin Htrue].
      split; [right; exact Hin | exact Htrue].
    + discriminate.
Qed.

Lemma scan_exhausted_complete :
  forall (A : Type) (decides : A -> option bool) candidates,
    scan decides candidates = Exhausted ->
    forall candidate, In candidate candidates -> decides candidate = Some false.
Proof.
  intros A decides candidates.
  induction candidates as [|head tail IH]; intros Hscan candidate Hin.
  - contradiction.
  - simpl in Hscan.
    destruct (decides head) as [[|]|] eqn:Hhead; try discriminate.
    destruct Hin as [Heq | Hin].
    + subst. exact Hhead.
    + apply IH; assumption.
Qed.

Lemma complete_scan_selects_when_ready_candidate_exists :
  forall (A : Type) (decides : A -> option bool) candidates,
    Forall (fun candidate => exists decision, decides candidate = Some decision) candidates ->
    (exists candidate, In candidate candidates /\ decides candidate = Some true) ->
    exists selected, scan decides candidates = Selected selected.
Proof.
  intros A decides candidates Hready.
  induction Hready as [|head tail Hhead Htail IH]; intros Hexists.
  - destruct Hexists as [candidate [Hin _]]. contradiction.
  - destruct Hhead as [decision Hdecision].
    destruct decision.
    + exists head. simpl. rewrite Hdecision. reflexivity.
    + destruct Hexists as [candidate [[Heq | Hin] Htrue]].
      * subst. rewrite Hdecision in Htrue. discriminate.
      * destruct (IH (ex_intro _ candidate (conj Hin Htrue))) as [selected Hselected].
        exists selected. simpl. rewrite Hdecision. exact Hselected.
Qed.

Lemma inconclusive_is_not_exhaustion :
  forall (A : Type) (decides : A -> option bool) candidates candidate,
    scan decides candidates = Inconclusive candidate ->
    scan decides candidates <> Exhausted.
Proof.
  intros A decides candidates candidate Hinconclusive Hexhausted.
  rewrite Hinconclusive in Hexhausted.
  discriminate.
Qed.

Example fixed_prefix_can_starve_a_finalizable_candidate :
  scan (fun candidate => Some (Nat.eqb candidate 3)) (firstn 2 [1; 2; 3]) = Exhausted /\
  scan (fun candidate => Some (Nat.eqb candidate 3)) [1; 2; 3] = Selected 3.
Proof.
  split; reflexivity.
Qed.

Section CandidateDiscovery.

Variable A : Type.
Variable eq_dec : forall left right : A, {left = right} + {left <> right}.

Definition schedule_once (scheduled proposed : list A) : list A :=
  nodup eq_dec (scheduled ++ proposed).

Theorem schedule_once_has_no_duplicates :
  forall scheduled proposed,
    NoDup (schedule_once scheduled proposed).
Proof.
  intros scheduled proposed.
  unfold schedule_once.
  apply NoDup_nodup.
Qed.

Theorem schedule_once_preserves_exact_membership :
  forall scheduled proposed candidate,
    In candidate (schedule_once scheduled proposed) <->
    In candidate scheduled \/ In candidate proposed.
Proof.
  intros scheduled proposed candidate.
  unfold schedule_once.
  rewrite nodup_In.
  apply in_app_iff.
Qed.

End CandidateDiscovery.
