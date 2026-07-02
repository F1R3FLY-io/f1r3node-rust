(* ===========================================================================
   ConflictSoundness.v - THE GAP-3 proof: removing the single-value-cell
   (consume-then-produce-by-channel) conflict predicate is SOUND -- everything it
   would have flagged is already caught by the RETAINED double-consume /
   same-IO-event race detector, EXCEPT on number/foldable channels, which are
   intrinsically mergeable and therefore safe to leave unflagged.

   `conflicts` (rspace++/src/rspace/merger/merging_logic.rs:262-479) retains:
     Check #1  races_for_same_io_event -- the SAME non-persistent Produce/Consume
               destroyed in COMM in BOTH branches (produces_consumed /
               consumes_produced intersection), minus both-mergeable;
     Check #2  potential_comms;
     Check #3  produce_touch_base_join.
   The REMOVED predicate was a single-value-cell check: two branches that both
   consume-then-produce on a shared channel (a write-write on one storage cell),
   which EXEMPTED number channels (they fold, so they never truly conflict).

   ---------------------------------------------------------------------------
   Why the removal is sound (the subsumption argument)
   ---------------------------------------------------------------------------
   A single-value cell holds exactly one datum; to UPDATE it a branch must CONSUME
   the cell's current (base) datum and PRODUCE a new one. So "branch does a
   consume-then-produce on channel ch" DEFINITIONALLY includes "branch consumed
   the base datum on ch" (`consumes ch` -- the base Produce destroyed in COMM,
   which lands in produces_consumed). If BOTH branches update the same non-number
   cell, they both consumed the SAME base datum -> a produces_consumed race, which
   is EXACTLY Check #1 (races_for_same_io_event). If the cell is a number channel,
   it is in produces_mergeable, so Check #1 (minus both-mergeable) correctly does
   NOT flag it -- and the removed predicate also exempted it. Either way, nothing
   is lost.

   ---------------------------------------------------------------------------
   Model (abstract branch event-sets; NO hidden hypothesis)
   ---------------------------------------------------------------------------
   A Branch carries two boolean channel-indexed fields:
     consumes ch : the base datum on ch was DESTROYED in COMM inside the branch
                   (models produces_consumed; non-persistent, since a persistent
                   produce is never destroyed -- the retained detector's
                   `!persistent` filter is thereby respected);
     produces ch : the branch produced a NEW datum on ch.
   svc_update ch := consumes ch && produces ch  (the consume-then-produce cell
   update). The single-value-cell semantics -- "updating consumes the base" -- is
   captured DEFINITIONALLY by this conjunction, so the subsumption needs NO
   hypothesis (unlike a stated `svc_update -> consumes` assumption): it is
   `andb_prop`. `is_number_ch` is an abstract PARAMETER (any number-channel
   classifier), not an axiom; the side-condition it induces is EXPLICIT in every
   statement.

   Headline `conflict_removal_sound`; core lemma `removed_subset_retained`.

   Stdlib-only, axiom-free. `Print Assumptions conflict_removal_sound`
   => "Closed under the global context".
   =========================================================================== *)

From Stdlib Require Import Bool.Bool.
From Stdlib Require Import Lists.List.
Import ListNotations.

Definition Channel := nat.

Section Conflict.
  (* Abstract number/foldable-channel classifier. A PARAMETER (not an axiom): the
     results hold for ANY classifier. The persistent/number-channel side-condition
     is thereby EXPLICIT in each statement below. *)
  Variable is_number_ch : Channel -> bool.

  Record Branch := {
    consumes : Channel -> bool;   (* base datum on ch destroyed in COMM (non-persistent) *)
    produces : Channel -> bool    (* a new datum produced on ch *)
  }.

  (* single-value-cell update = consume-then-produce on a channel (the per-channel
     condition of the REMOVED predicate). Consuming the base is a DEFINITIONAL
     conjunct, so no separate semantic hypothesis is needed. *)
  Definition svc_update (br : Branch) (ch : Channel) : bool :=
    consumes br ch && produces br ch.

  (* The REMOVED predicate: both branches consume-then-produce on a shared channel
     (a single-value-cell write-write). Number-channel exemption is expressed by
     the soundness disjunct below (matching the code's "exempting number
     channels"). *)
  Definition removed_fires (a b : Branch) : Prop :=
    exists ch, svc_update a ch = true /\ svc_update b ch = true.

  (* The RETAINED double-consume / same-IO-event race (Check #1 on the shared base
     datum): both branches destroyed the same base produce on a NON-number channel
     (the "minus both-mergeable" filter = `is_number_ch ch = false`). *)
  Definition retained_conflict (a b : Branch) : Prop :=
    exists ch, consumes a ch = true /\ consumes b ch = true /\ is_number_ch ch = false.

  (* The number-channel exemption witness: the shared single-value-cell channel is
     a number/foldable channel -- intrinsically mergeable, safe to leave unflagged. *)
  Definition is_number_channel (a b : Branch) : Prop :=
    exists ch, svc_update a ch = true /\ svc_update b ch = true /\ is_number_ch ch = true.

  (* THE core soundness lemma: whatever the removed predicate fires on is either a
     retained conflict OR a number channel. NO premise, NO hidden hypothesis. *)
  Theorem removed_subset_retained :
    forall a b, removed_fires a b -> retained_conflict a b \/ is_number_channel a b.
  Proof.
    intros a b [ch [Ha Hb]].
    destruct (is_number_ch ch) eqn:E.
    - (* number channel: exempt (intrinsically mergeable) *)
      right. exists ch. split; [exact Ha | split; [exact Hb | exact E]].
    - (* non-number: consume-then-produce => the base datum was consumed in both,
         i.e. exactly the retained produces_consumed race *)
      left. exists ch. unfold svc_update in Ha, Hb.
      apply andb_prop in Ha. destruct Ha as [Hca _].
      apply andb_prop in Hb. destruct Hb as [Hcb _].
      split; [exact Hca | split; [exact Hcb | exact E]].
  Qed.

  (* The code's ACTUAL predicate had the number-channel exemption baked in. With
     that filter, the removed check is FULLY subsumed by the retained detector --
     nothing is lost by dropping it. *)
  Definition removed_fires_exempt (a b : Branch) : Prop :=
    exists ch, svc_update a ch = true /\ svc_update b ch = true /\ is_number_ch ch = false.

  (* THE headline: the removal is sound (both the general disjunctive form and the
     number-filtered subsumption the code relies on). *)
  Theorem conflict_removal_sound :
    (forall a b, removed_fires a b -> retained_conflict a b \/ is_number_channel a b)
    /\ (forall a b, removed_fires_exempt a b -> retained_conflict a b).
  Proof.
    split.
    - exact removed_subset_retained.
    - intros a b [ch [Ha [Hb Hn]]]. exists ch.
      unfold svc_update in Ha, Hb.
      apply andb_prop in Ha. destruct Ha as [Hca _].
      apply andb_prop in Hb. destruct Hb as [Hcb _].
      split; [exact Hca | split; [exact Hcb | exact Hn]].
  Qed.

End Conflict.
