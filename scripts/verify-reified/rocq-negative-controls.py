#!/usr/bin/env python3
"""Rocq negative controls for the Reified RSpaces P1, P2, P4, P5 and P6 models.

Each control mutates one rule in a scratch copy of a module
(formal/rocq/reified_rspaces/theories). The mutation changes one definition,
one statement, or every site of a rule that the model states in more than
one definition. The control passes when the mutated copy fails to compile
and the first error is inside one of the expected declarations. So each
control shows that a proof depends on the rule that it models: the proof
breaks when the model or the claim reverts to the old rule.

The controls cover the round-2 rules and the round-3 rule
(prompts/p1-fix-rules.md):
  FR-1'  a canonical group is non-decreasing and can repeat a channel
         (H17, H22, the uniqueness of the canonical form)
  FR-5'  the parent tracker resets per evaluation scope, on a store swap
         and on a session change (H20)
  FR-9'  the sweep through the handle honors the nested-transaction guard
         (H13)
  FR-10  a produce-side peek fire never stores its incoming datum, and the
         agent re-offers that datum as the next operation (H11)
and two FR-1 controls that the round-2 rules keep (the sweep validates the
stored join groups, and a rejected read does not touch the cache).

Decision 45 (Rule E) has three controls. Each reverts one part of the rule:
  ec-peek-retains            a consume-side peek fire keeps its stored
                             records (the retention of FR-10):
                             fire_view_removes, and so H11, breaks
  ec-restore-skipped         the agent stages no re-store after a
                             consume-side peek fire:
                             restores_are_produce_peeks, and so H11, breaks
  ec-restore-wrong-identity  the re-store sends the matched value, so it
                             takes another produce identity:
                             restore_same_identity, and so H11, breaks
Rule E removes the kept records, so the FR-10 controls
ec-kept-includes-incoming and ec-h11-claims-p1-deviation have no site. They
are disabled.

Decision 36 (replay reproduces play) has three controls. Each reverts one
part of the rule:
  ec-replay-fire-fails-at-staging  an unpaired replay fire fails at staging
                                   (the rule of decision 19): H23 breaks
  ec-unpaired-skips-observe-comm   an unpaired replay fire fails before its
                                   observe_comm call: H23 breaks
  ec-unpaired-fire-commits         an unpaired replay fire commits: H4 (and
                                   H5) break

Decision 44 (the merge of feature/cost-accounted-rho) has one control:
  ec-split-zero-child              a split appends (s, i) instead of the
                                   DR-84 segment (s, i + 1), so a child
                                   segment can equal an operation segment:
                                   sched_part_nonzero (and so H8) breaks

The P4 controls (batch 4E-F) revert one rule of the hashmap:queue slice:
  Namespace           the ephemeral KIND bit, distinct op ids, the sid
                      length check of the decoder, the name tail lengths
  RecordEnvelope      the strict tag (kind and flag), the strict list flags,
                      the kind check of the datum envelope, natural orders
  CounterConflict     the counter COMM has no peeks (M-1), the first
                      operation consumes S(sid) (A25), reified channels are
                      never mergeable, pre-state produces are made by Base,
                      every operation bumps its counter, a removal and a
                      peek included (M-1, decision 31), and a pop takes the
                      top of a stack
  HeadOnlyDisciplines the least (ord, rank) head, the tail join of a
                      datum, the divergent re-issue (A10), the counter bump
  SpaceRefs           the write flag of a handle, R3 and R5 routing, the
                      USE wrapper, the reply name check (A23)
  SpaceFactory        the qualifier matrix, the family indices, the call
                      ops of A21, SpaceAlreadyExists, the canonical order,
                      the bit check of a qualifier element
  Encoding (I-20)     the refresh, the derived tuple flag, the strict flag
                      check

Decision 47 (EventCorrelation.v, H24 to H27) has four controls. Each reverts
one rule of the native replay backend of spaces or of its view charges:
  ec-native-searches-again     the backend fires against any recorded
                               candidate, as a backend that searches the
                               candidates again (the DR-75 search) would:
                               native_replay_fails_closed (H26) breaks
  ec-native-ignores-denial     the backend applies an operation whose row
                               records a denial: H24 breaks
  ec-native-effect-before-prepare  the backend changes the store before it
                               checks the candidate: H26 breaks
  ec-view-charges-on-miss-only a view load charges its value on a miss
                               only: view_charge_cache_independent (H27)
                               breaks

Decision 47 (SpaceRefs.v) has two controls: a host runtime that rejects space
jobs in native play, as decision 46 did, and a native replay runtime that runs
space jobs before its environment binds it. The control of decision 46 is
retired with its headline.

Decision 42 (AgentReadCoverage.v) has five controls. Each reverts one part of
the metering rule: a step that reads before it reserves, a fire that does not
prepay the decode of its continuation, an operation that does not prepay the
buffer of its outcome, an outcome that reserves after the commit, and a scan
that copies every entry instead of the head only.

Decision 43 (HeadOnlyDisciplines.v) has three controls. Each reverts one part
of the rule that a re-issued persistent operation keeps its first-round ord:
  ho-reentry-fresh-ord        the re-issue takes the ord of its bump:
                              queue_reissue_keeps_ord breaks
  ho-reentry-ignores-head     a re-entered element is at the head whatever
                              the ord of the head: reenter_offer_shadow, and
                              so the SHADOW headline, breaks
  ho-nonpersistent-keeps-ord  a re-issued non-persistent element keeps its
                              ord (the scope rule): queue_quiescent_inv breaks

Decision 45 (HeadOnlyDisciplines.v) has three controls. Each reverts one part
of Rule E, under which the substrate removes a peeked record and the agent
stores it again:
  ho-restore-skipped     the code stages no re-store after a peek fire of a
                         non-persistent record, so the record is lost:
                         restore_fire_step, and so queue_restore_realizes_plan,
                         breaks
  ho-peek-retains        a peek fire keeps the records that it binds (the
                         retention before decision 45), so the re-store
                         duplicates the record: restore_fire_step breaks
  ho-restore-persistent  the code also re-stores a peeked persistent record,
                         which the fire keeps, so the record is duplicated:
                         restore_fire_step breaks

Decision 45 (Option P) has five controls in LegacyCharges.v. Each moves or
drops one legacy charge of a space transaction, or drops a premise that Q80
takes from H23 of EventCorrelation.v:
  lc-tail-dropped            the job never runs the tail
                             (LegacyChain::finish charges nothing), so the
                             post-charges of the last operation are lost:
                             commit_finish_rebracket, and so Q79, breaks
  lc-carry-dropped           a callback does not charge the carry, so the
                             post-charges of each operation before the last
                             are lost: commit_finish_rebracket breaks
  lc-pre-after-body          the pre-charge comes after the body of its
                             operation, so a pre-charge that runs out of
                             phlo stops after the body:
                             commit_finish_rebracket breaks
  lc-replay-unpaired         Q80 without the premise that replay pairs each
                             operation whose body play applied:
                             legacy_replay_stops_with_play breaks
  lc-unpaired-skips-observe  an unpaired replay body diverges before its
                             observer calls (decision 36 reverted), so a
                             body that the observer rejects in play
                             diverges in replay: host_op_pairing, and so
                             Q80, breaks

The P5 controls (wave W2) revert one rule of a discipline, an outer store or
the engine:
  CellDiscipline         the capacity check, the plain re-store of a peek,
                         the re-entry as a fresh element
  DataCollection,        the DR-75 visit order, the first spatial match, the
  ContinuationCollection multiplicity, the second send of a peek, the order
                         of the waiters
  Match, GenericRSpace   the guard checked once, the wildcard matcher, the
                         engine primitives, the restore of a peek
  OuterStorage,          the allocation rules of fixed, cyclic and growing
  VectorStore,           stores, the key check of an index name, the keys of
  HashMapStore           the HashMap store
  PathMapPrefix          the segment codec of a path, the re-rooting laws, and
                         (wave W3) the marker record, the vehicle, the path
                         rules, the delivery and retire plans, the second
                         send of a peek, the token order and the metering
  SpaceFactory,          the registration that follows the dispatch arms,
  SpaceRefs              the constructor params, the route of a receive query
  StackDiscipline        the top, the LIFO re-entry, the settle loop and its
                         one fire count for each transaction (decision 49)
  CounterConflict        the bumps of removals and peeks (above)
  SetDiscipline          the length framing and the sort of the set key, the
                         class view, the class fire and RETIRE-CLASS, the peek
                         of the representative, the DR-90 membership, the key
                         metering
  PriorityQueue,         the level order, the lanes with the older head first,
  RecordEnvelope         the settle order, the re-entry at the FIFO place of a
                         level, the priority domain, the slots of a record and
                         of a continuation

The P5 controls (wave W3) revert one rule of the join engine:
  JoinCascade,           the incoming datum bound once, the claims, the greedy
  RecordEnvelope,        search and its one guard check, the bind order of the
  CounterConflict        join slots, the head rule, the set retirement and the
                         set cascade (decision 50), the cascade and its
                         measure, one fire count, lift and re-offer, the
                         touched counters, the re-issued orders, the second
                         sends of Rule E, the closure of the scope, the commit
                         rules (A1, DV-5), the version 2 record, the join
                         race on every counter, and the set merges (a
                         retirement as a peek, a set view in list order)

P6 step 2 (Similarity.v) has five controls. Each reverts one rule of the
exact decisions of the VectorDB family, or claims more than the rule gives:
  sim-squares-without-signs        cosine compares the squares only:
                                   cosine_squared_form_iff (S1) breaks
  sim-order-swapped-norms          the cosine order uses the wrong norms:
                                   cosine_order_squared_form (S2) breaks
  sim-radius-unsquared             euclidean compares d2 with r, not r²:
                                   euclidean_radius_squared_form (S3) breaks
  sim-total-without-distinct-keys  the selection order claims totality
                                   without distinct tie keys: precedes_total,
                                   and so S13, breaks
  sim-unit-threshold-claimed       the cosine bound claims that no record
                                   passes the threshold 1: cosine_bounds (S9)
                                   breaks
Headline S16 (kappa_dropped_not_unique) is the proved negative result of the
kappa tie-break: without kappa, two sorted arrangements differ.

P6 step 8 (SpaceFactory.v, headline 14 and the P6 rows of headlines 9 and 10)
has controls that revert one rule of the registration or of the VectorDB
params:
  sf-p6-register-unwired         a P6 row is registered without its arm:
                                 registered_rows breaks
  sf-p6-define-unregistered      the factory of every P6 row is defined:
                                 definitions_incl breaks
  sf-p6-row-wrong-op             vector:vectordb takes the op 24:
                                 rows_facts breaks
  sf-p5-pathmap-alias-other-op   PathMapSpace names the op 0: alias_rows_facts
                                 breaks
  sf-p6-vectordb-on-pathmap      pathmap:vectordb gets a schema:
                                 vectordb_schema breaks
  sf-p6-metric-required          the VectorDB families require metric:
                                 vectordb_schema breaks
  sf-p6-capacity-zero            capacity accepts 0: canonical_capacity_rule
                                 breaks
  sf-p6-capacity-bound           the capacity bound is 1000001:
                                 vectordb_params_validated breaks
  sf-p6-dimensions-bound         the dimensions bound is 4097:
                                 vectordb_params_validated breaks
  sf-p6-metric-any-string        metric accepts every string:
                                 canonical_metric_rule breaks
  sf-p6-mirror-any-string        mirror accepts every string:
                                 canonical_mirror_rule breaks
  sf-p6-gate9-any-metric         gate 9 accepts every metric name:
                                 check_mirror_spec breaks
  sf-p6-gate9-any-qualifier      gate 9 ignores the qualifier:
                                 check_mirror_spec breaks
  sf-p6-gate9-no-default-metric  without the metric param, gate 9 has no
                                 metric: mirror_metric_default breaks
  sf-p6-indexed-manhattan        ChromaDB indexes manhattan too:
                                 vectordb_params_validated breaks

Decision 71 (CounterConflict.v, headline 16) has two controls:
  cc-capped-store-at-cap         a store passes on a collection that holds
                                 cap records: capped_pass_iff breaks
  cc-capacity-hard-claimed       the claim that a merge never exceeds the
                                 capacity: capped_writes_soft_bound breaks

P6 step 8b (VectorDbDiscipline.v) has seven controls. Each reverts one rule
of the driver of a VectorDB collection:
  vdb-no-cascade          the tail runs no LIST-CASCADE:
                          first_cascade_none, and so headline 2, breaks
  vdb-peek-not-owed       a peek fire of RECEIVE-VDB owes no second sends
                          (Rule E): receive_nl, and so headline 5, breaks
  vdb-lift-not-owed       a cascade fire of a peek owes no second send for
                          its lifted member: tail_nl, and so headline 5,
                          breaks
  vdb-retire-unlogged     an offer fire does not log the members that it
                          retires: offer_nl, and so headline 5, breaks
  vdb-window-top1         the similarity window of a join bind holds only
                          its best record: jfill_mono, and so headline 1,
                          breaks
  vdb-cont-on-persistent  a fire whose persistent partner stays continues
                          the loop: after_fire_cont, and so the fuel bound
                          of headline 4, breaks
  vdb-offer-never-fires   OFFER-VDB fires no parked waiter:
                          first_offer_none, and so headline 2, breaks

P6 step 8d (VectorOps.v) has three controls. Each reverts one rule of the
replies of rho:lang:vector:
  vo-kjoin-narrower       the join of two kinds takes the narrower kind:
                          kjoin_rank breaks
  vo-dot-rounds-each-term dot rounds each product before the sum (no
                          single rounding, decision 54):
                          vector_replies_exact breaks
  vo-faithful-root-bound  the check bounds the square of the root by the
                          square of the next float, not of the midpoint (a
                          faithful rounding): root_midpoint_nearest breaks

Rules of the harness:
  * The pristine modules must compile first. Otherwise every control fails.
  * A control has one mutation, or a tuple of mutations that the harness
    applies in order. Each mutation text must occur exactly once in the
    module when the harness applies it. Otherwise the control fails as
    stale.
  * The scratch tree is under target/ and never in the source tree. TMPDIR
    points into it. Unless RRS_SANDBOX=1, each coqc run is bounded by a
    systemd user scope (MemoryMax, MemorySwapMax=0, CPUQuota).

Environment: RRS_SANDBOX, RRS_SCRATCH, RRS_MEMORY_MAX (default 6G),
RRS_CPU_QUOTA (default 400%), RRS_ROCQ_SRC (default formal/rocq/
reified_rspaces).

Exit codes: 0 every control passes, 1 a control failed, 2 usage or setup
error. Python 3 standard library only.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]

# (id, module, description, old text, new text, expected first-failing
# declarations). A control that mutates more than one site gives a tuple of
# old texts and a tuple of new texts of the same length.
CONTROLS = (
    # ── FR-1' ────────────────────────────────────────────────────────────
    ("vs-strict-order", "VolatileSweep",
     "FR-1 strictness instead of FR-1': adjacent channels must differ",
     "y :: _ => chan_leb x y && nondecreasing t end",
     "y :: _ => chan_leb x y && negb (chan_leb y x) && nondecreasing t end",
     ("nondecreasing_sorted",)),
    ("vs-example-claims-fr1", "VolatileSweep",
     "the H22 example claims the FR-1 rule: [x; x] is not canonical",
     "    canonical [x; x] = true /\\ canonical [x; x; y] = true /\\\n"
     "    canonical [x; y; x] = false /\\ canonical [y; x; x] = false.\nProof.",
     "    canonical [x; x] = false /\\ canonical [x; x; y] = true /\\\n"
     "    canonical [x; y; x] = false /\\ canonical [y; x; x] = false.\nProof.",
     ("repeated_group_canonical_when_sorted",)),
    ("vs-unique-without-antisymmetry", "VolatileSweep",
     "the uniqueness lemma without the antisymmetry premise",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> leb_antisymmetric ->",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> True ->",
     ("sorted_unique_canonical",)),
    ("vs-weak-antisymmetry", "VolatileSweep",
     "an antisymmetry premise that concludes nothing",
     "Definition leb_antisymmetric : Prop :=\n"
     "  forall x y, chan_leb x y = true -> chan_leb y x = true -> x = y.",
     "Definition leb_antisymmetric : Prop :=\n"
     "  forall x y, chan_leb x y = true -> chan_leb y x = true -> True.",
     ("sorted_unique_canonical",)),
    ("vs-no-totality", "VolatileSweep",
     "a totality premise that holds for every comparison",
     "Definition leb_total : Prop :=\n"
     "  forall x y, chan_leb x y = true \\/ chan_leb y x = true.",
     "Definition leb_total : Prop :=\n"
     "  forall x y, chan_leb x y = true \\/ True.",
     ("isort_canonical",)),
    ("vs-keys-ignore-order", "VolatileSweep",
     "collision freedom that says nothing about two continuation keys",
     "    | CCont g1, CCont g2 => Permutation g1 g2",
     "    | CCont g1, CCont g2 => True",
     ("sorted_keys_injective",)),
    ("vs-sweep-skips-canonical-check", "VolatileSweep",
     "the sweep does not validate the order of the stored join groups (FR-1)",
     "  && forallb (fun g => is_none (tlookup (key_cont g) T)) G\n  && forallb canonical G.",
     "  && forallb (fun g => is_none (tlookup (key_cont g) T)) G.",
     ("validate_spec",)),
    # ── FR-9' ────────────────────────────────────────────────────────────
    ("vs-sweep-ignores-open-txn", "VolatileSweep",
     "the handle sweeps under an open transaction of the same task",
     "  if in_dec ctx_eq_dec cur open then (S, Some NestedTransaction)",
     "  if in_dec ctx_eq_dec cur open then\n"
     "    match sweep_volatile T installed S chans with\n"
     "    | Some S' => (S', None)\n"
     "    | None => (S, Some SweepRejected)\n"
     "    end",
     ("sweep_exact",)),
    # ── FR-1' in EventCorrelation.v ──────────────────────────────────────
    ("ec-check-rejects-repeats", "EventCorrelation",
     "the check rejects a repeated channel again (FR-1 DuplicateChannelInGroup)",
     "  if nondecreasing g then None else Some NonCanonicalChannelOrder.",
     "  if nondecreasing g && match g with [x; y] => negb (chan_leb y x) | _ => true end\n"
     "  then None else Some NonCanonicalChannelOrder.",
     ("canonical_nondecreasing",)),
    ("ec-h17-claims-fr1", "EventCorrelation",
     "H17 claims the FR-1 rule: a repeated channel fails",
     "  (forall c, chan_leb c c = true -> group_check [c; c] = None) /\\",
     "  (forall c, chan_leb c c = true -> group_check [c; c] = Some NonCanonicalChannelOrder) /\\",
     ("non_canonical_fails_closed",)),
    ("ec-unique-without-antisymmetry", "EventCorrelation",
     "the uniqueness lemma without the antisymmetry premise",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> leb_antisymmetric ->",
     "Lemma sorted_unique_canonical :\n  leb_transitive -> True ->",
     ("sorted_unique_canonical",)),
    ("ec-read-caches-rejected-key", "EventCorrelation",
     "a rejected continuations read caches its key (FR-1 fails open)",
     "  | ARead (RConts g) =>\n      match group_check g with\n      | Some e => (cache, Fail (EGroup e))",
     "  | ARead (RConts g) =>\n      match group_check g with\n      | Some e => (touch (CCont g) cache, Fail (EGroup e))",
     ("access_grows",)),
    # ── FR-5' ────────────────────────────────────────────────────────────
    ("ec-scope-does-not-reset", "EventCorrelation",
     "begin_evaluation_scope keeps the tracker",
     "  | CallScope => Some (tracker_reset, [])",
     "  | CallScope => Some (tr, [])",
     ("two_evaluations_commit",)),
    ("ec-store-swap-ignored", "EventCorrelation",
     "the tracker ignores the store identity",
     "  | Some s, Some st => Nat.eqb s sess && Nat.eqb st store",
     "  | Some s, Some st => Nat.eqb s sess",
     ("scope_parents_other",)),
    ("ec-session-change-ignored", "EventCorrelation",
     "the tracker ignores the session",
     "  | Some s, Some st => Nat.eqb s sess && Nat.eqb st store",
     "  | Some s, Some st => Nat.eqb st store",
     ("scope_parents_other",)),
    ("ec-parent-reuse-admitted", "EventCorrelation",
     "a second commit under one parent in one scope is admitted",
     "      if in_dec path_eq_dec parent base then None\n      else Some",
     "      Some",
     ("run_calls_scope",)),
    ("ec-two-evaluations-claim-fr5", "EventCorrelation",
     "the two-evaluation lemma claims the FR-5 rule: the second evaluation fails after a scope reset",
     "                       CallScope; CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1 ++ sub_orders s parent n2 /\\\n"
     "    (forall st', st' <> st ->",
     "                       CallScope; CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1 /\\\n"
     "    (forall st', st' <> st ->",
     ("two_evaluations_commit",)),
    ("ec-no-reset-claims-commit", "EventCorrelation",
     "the two-evaluation lemma claims that a commit without a reset reuses the parent",
     "    snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);\n"
     "                       CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1.\nProof.",
     "    snd (run_calls tr [CallScope; CallCommit (ReqParent s st parent n1);\n"
     "                       CallCommit (ReqParent s st parent n2)]) =\n"
     "      sub_orders s parent n1 ++ sub_orders s parent n2.\nProof.",
     ("two_evaluations_commit",)),
    # ── FR-10 ────────────────────────────────────────────────────────────
    ("ec-fire-stores-incoming", "EventCorrelation",
     "a peek fire stores its non-persistent incoming datum again (the P1 rule before FR-10) "
     "in the view and both commits",
     ("  vpf_loop V1 chs (sort_desc vkey cands).",
      "                  mkRes (fst p2) rd (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk",
      "                  mkRes (fst p2) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2) StOk"),
     ("  let V2 := vpf_loop V1 chs (sort_desc vkey cands) in\n"
      "  if negb (nullb (w_peeks wk)) && negb persist\n"
      "  then vput_datum V2 ch (mkDatum v persist (produce_id ch v persist)) else V2.",
      "                  let p3 := if negb (nullb (w_peeks wk)) && negb persist\n"
      "                            then put_datum (fst p2) ch (mkDatum v persist src) else (fst p2, []) in\n"
      "                  mkRes (fst p3) rd (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2 ++ snd p3) StOk",
      "                  let p3 := if negb (nullb (w_peeks wk)) && negb persist\n"
      "                            then put_datum (fst p2) ch (mkDatum v persist src) else (fst p2, []) in\n"
      "                  mkRes (fst p3) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])\n"
      "                        [EvProduce src; EvComm ProduceSide cm] (snd p1 ++ snd p2 ++ snd p3) StOk"),
     ("fire_stores_no_datum",)),
    # Disabled by decision 45 (Rule E): the kept records (retained) are gone, so this rule has no site.
    # ("ec-kept-includes-incoming", "EventCorrelation",
    #  "the kept records of a produce-side peek fire include its incoming datum (the P1 retained)",
    #  "  | OProduce _ _ _ (PFire _ wk _ cands) =>\n"
    #  "      if negb (nullb (w_peeks wk)) then fun c => np_recs c cands else fun _ => []",
    #  "  | OProduce ch v persist (PFire _ wk _ cands) =>\n"
    #  "      if negb (nullb (w_peeks wk))\n"
    #  "      then fun c => np_recs c cands\n"
    #  "                    ++ (if negb persist && chan_eqb ch c\n"
    #  "                        then [mkDatum v persist (produce_id ch v persist)] else [])\n"
    #  "      else fun _ => []",
    #  ("retained_stored",)),
    ("ec-h11-claims-incoming-stored", "EventCorrelation",
     "H11 claims the P1 rule: a committed non-persistent produce-side peek fire stores its incoming datum",
     "  (forall m H S rd sg, is_fire (sg_op sg) = true ->\n"
     "     forall c x, count_occ datum_eq_dec (s_data (r_st (commit_op m H S rd sg)) c) x <=\n"
     "                 count_occ datum_eq_dec (s_data S c) x) /\\",
     "  (forall m H S rd ch v chs wk kidx cands j0 scomm sp,\n"
     "     w_peeks wk <> [] ->\n"
     "     let r := commit_op m H S rd (mkStaged (OProduce ch v false (PFire chs wk kidx cands)) j0 scomm sp) in\n"
     "     r_status r = StOk ->\n"
     "     s_data (r_st r) ch = mkDatum v false (produce_id ch v false) :: s_data S ch) /\\",
     ("peek_reproduces_trace_eq_host",)),
    # Disabled by decision 45 (Rule E): no kept record replaces a re-produce of HEAD, so this claim has no site.
    # ("ec-h11-claims-p1-deviation", "EventCorrelation",
    #  "H11 claims the P1 accounting: the kept records replace every re-produce of HEAD, "
    #  "the incoming datum included",
    #  "  (forall op c, retaining op = true ->\n"
    #  "     values_on c (kept_reproduces (fire_cands op)) = map d_val (retained op c)) /\\",
    #  "  (forall op c, retaining op = true ->\n"
    #  "     values_on c (head_reproduces (fire_cands op)) = map d_val (retained op c)) /\\",
    #  ("peek_retention_trace_eq_host",)),
    # ── Decision 45, Rule E: no retention, and the re-store (H11, H2) ─────
    ("ec-peek-retains", "EventCorrelation",
     "a consume-side peek fire keeps its stored records (the retention of FR-10) in the view and "
     "both commits, so the records that the receipt reports non-persistent stay in the store",
     ("          stage_fire T (vcf_effects V cands) (KConsume src) cm (mkStaged op [] (Some cm))",
      "                  let p := space_cf_loop S (sort_desc vkey cands) in\n"
      "                  mkRes (fst p) rd (obs ++ [oc; ObFinish s Matched])",
      "                  let p := space_cf_loop S (sort_desc vkey cands) in\n"
      "                  mkRes (fst p) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])"),
     ("          stage_fire T (if nullb peeks then vcf_effects V cands else V) (KConsume src) cm\n"
      "                     (mkStaged op [] (Some cm))",
      "                  let p := if nullb peeks then space_cf_loop S (sort_desc vkey cands) else (S, []) in\n"
      "                  mkRes (fst p) rd (obs ++ [oc; ObFinish s Matched])",
      "                  let p := if nullb peeks then space_cf_loop S (sort_desc vkey cands) else (S, []) in\n"
      "                  mkRes (fst p) (unbind rd cm) (obs ++ [oc; ObFinish s Matched])"),
     ("fire_view_removes",)),
    ("ec-restore-skipped", "EventCorrelation",
     "the agent stages no re-store after a consume-side peek fire, so its operations are not "
     "HEAD's produce_peeks",
     "  | OConsume _ _ _ _ peeks (CFire cands) =>\n"
     "      if nullb peeks then [] else map restore_of (filter removable cands)\n",
     "  | OConsume _ _ _ _ peeks (CFire cands) => []\n",
     ("restores_are_produce_peeks",)),
    ("ec-restore-wrong-identity", "EventCorrelation",
     "the re-store sends the matched value instead of the stored record (the payload without its "
     "envelope), so it takes another produce identity",
     "  OProduce (cd_chan cd) (d_val (cd_rec cd)) false PStore.",
     "  OProduce (cd_chan cd) (cd_matched cd) false PStore.",
     ("restore_same_identity",)),
    ("ec-plan-stores-incoming", "EventCorrelation",
     "the head-only plan stores the datum after a peek fire (the P1 rule) instead of the re-offer",
     "            else if w_persist w then (ws, [])\n"
     "            else offer wmatch ws' [] d",
     "            else if negb (d_persist d) then ((if w_persist w then ws else ws'), [d])\n"
     "            else if w_persist w then (ws, [])\n"
     "            else offer wmatch ws' [] d",
     ("offer_keeps_no_pending_match",)),
    ("ec-reoffer-due-without-a10", "EventCorrelation",
     "the re-offer is due also after a persistent waiter (FR-10 without A10), "
     "which the plan cannot stage in the same transaction",
     "      negb persist && negb (nullb (w_peeks wk)) && negb (w_persist wk)",
     "      negb persist && negb (nullb (w_peeks wk))",
     ("offer_ops_reoffers",)),
    # ── Decision 36: replay reproduces play (H4, H5, H23) ─────────────────
    ("ec-replay-fire-fails-at-staging", "EventCorrelation",
     "an unpaired replay fire fails at staging (the rule of decision 19), so replay stages "
     "fewer operations than play",
     "  let ps := pair_step T k cm in\n"
     "  Some (txn_push T V (fst ps) (mk (snd ps))).",
     "  let ps := pair_step T k cm in\n"
     "  match backend (t_mode T), snd ps with\n"
     "  | Play, _ | _, true => Some (txn_push T V (fst ps) (mk (snd ps)))\n"
     "  | _, false => None\n"
     "  end.",
     ("replay_stage_sim", "replay_stage_all_sim", "replay_sim_gen", "replay_reproduces_play_run")),
    ("ec-unpaired-skips-observe-comm", "EventCorrelation",
     "an unpaired replay fire fails before the observe_comm call, so replay makes fewer calls "
     "than play",
     ("                let oc := ObComm cm (w_kont wk) (w_persist wk) (comm_data cands) in\n"
      "                if negb (ob_accept (H ++ obs) oc)\n"
      "                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n"
      "                else if negb paired\n"
      "                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StDiverged\n",
      "                let oc := ObComm cm k persist (comm_data cands) in\n"
      "                if negb (ob_accept (H ++ obs) oc)\n"
      "                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n"
      "                else if negb paired\n"
      "                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StDiverged\n"),
     ("                let oc := ObComm cm (w_kont wk) (w_persist wk) (comm_data cands) in\n"
      "                if negb paired\n"
      "                then mkRes (unbump S1 src persist) rd (obs ++ [ObFinish s Rejected]) [] [] StDiverged\n"
      "                else if negb (ob_accept (H ++ obs) oc)\n"
      "                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n",
      "                let oc := ObComm cm k persist (comm_data cands) in\n"
      "                if negb paired\n"
      "                then mkRes S rd (obs ++ [ObFinish s Rejected]) [] [] StDiverged\n"
      "                else if negb (ob_accept (H ++ obs) oc)\n"
      "                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n"),
     ("replay_produce_sim", "replay_consume_sim", "replay_commit_op_sim", "replay_sim_gen",
      "replay_reproduces_play_run")),
    ("ec-unpaired-fire-commits", "EventCorrelation",
     "an unpaired replay fire commits: it logs, applies its effects and unbinds a COMM that "
     "staging did not pair",
     ("                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n"
      "                else if negb paired\n"
      "                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StDiverged\n"
      "                else\n",
      "                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n"
      "                else if negb paired\n"
      "                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StDiverged\n"
      "                else\n"),
     ("                then mkRes (unbump S1 src persist) rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n"
      "                else\n",
      "                then mkRes S rd (obs ++ [oc; ObFinish s Rejected]) [] [] StRejected\n"
      "                else\n"),
     ("unpaired_fire_inert", "verify_sound_complete", "commit_run_rd", "replay_pairs_each_comm_once")),
    # ── Decision 44: the compacted split DR-84 (H8) ──────────────────────
    ("ec-split-zero-child", "EventCorrelation",
     "a split appends the segment (s, i), so the first child segment equals an operation segment",
     "| sp_split : forall p s i, sched_part p -> sched_part (p ++ [(s, Datatypes.S i)]).",
     "| sp_split : forall p s i, sched_part p -> sched_part (p ++ [(s, i)]).",
     ("sched_part_nonzero",)),
    # ── Decision 47: the native replay backend of spaces (H24-H27) ────────
    ("ec-native-searches-again", "EventCorrelation",
     "the native backend fires against any recorded candidate (a search of the candidates again)",
     "if comm_eq_dec c rc then\n                          if grant",
     "if true then\n                          if grant",
     ("native_replay_fails_closed",)),
    ("ec-native-ignores-denial", "EventCorrelation",
     "the native backend applies an operation whose row records a denial of its introduction",
     "| NDeniedIntro =>\n                  (h ++ [NIntro o], s, if grant h (NIntro o) then NMismatch else NOutOfPhlo)",
     "| NDeniedIntro => native_replay (h ++ [NIntro o]) (apply s o) rest rows'",
     ("native_replay_reproduces_play", "native_replay_denial_keeps_prefix")),
    ("ec-native-effect-before-prepare", "EventCorrelation",
     "the native backend changes the store before it checks the candidate of a fire",
     "else (h ++ [NIntro o], s, NMismatch)\n                    | Some c, NDeniedComm rc =>",
     "else (h ++ [NIntro o], apply s o, NMismatch)\n                    | Some c, NDeniedComm rc =>",
     ("native_replay_fails_closed",)),
    ("ec-view-charges-on-miss-only", "EventCorrelation",
     "a view load charges its value on a miss only, so a warm load charges less than a cold load",
     "  | Some v => (v, key_charge k + value_charge v)",
     "  | Some v => (v, key_charge k)",
     ("view_charge_cache_independent",)),
    # ── P4: Namespace.v ──────────────────────────────────────────────────
    ("ns-ephemeral-bit-dropped", "Namespace",
     "an ephemeral id has the durable KIND byte (no bit 0x20)",
     "  | Ephemeral => Nat.lor (kind_letter k) ephemeral_bit",
     "  | Ephemeral => kind_letter k",
     ("scoped_kind_byte_spec", "kind_of_byte_scoped")),
    ("ns-op-ids-collide", "Namespace",
     "getSpaceAgent takes the op of the engine factory",
     "Definition get_space_agent : nat := 33.",
     "Definition get_space_agent : nat := 32.",
     ("op_ids_pairwise_distinct",)),
    ("ns-short-sid-accepted", "Namespace",
     "the decoder accepts a payload shorter than a sid",
     "                  if length payload <? sid_len then None else decode_payload k d payload",
     "                  if length payload <? 1 then None else decode_payload k d payload",
     ("decode_tag_cons",)),
    # ── P4: RecordEnvelope.v ─────────────────────────────────────────────
    ("re-tag-flag-ignored", "RecordEnvelope",
     "is_tag accepts a tag with the flag true",
     "  | PBytes b false => bytes_eqb b (tag_bytes k)",
     "  | PBytes b _ => bytes_eqb b (tag_bytes k)",
     ("is_tag_iff",)),
    ("re-list-flags-unchecked", "RecordEnvelope",
     "list_elements does not check the derived flag of the holder Par",
     "      if Bool.eqb lf (existsb flag ps) && Bool.eqb pf (existsb flag ps) then Some ps else None",
     "      if Bool.eqb lf (existsb flag ps) then Some ps else None",
     ("list_elements_sound",)),
    ("re-datum-kind-unchecked", "RecordEnvelope",
     "datum_ord does not check the kind of the tag",
     "          if is_tag t RDatum && is_nil priority && is_nil embedding then natural_of o else None",
     "          if is_nil priority && is_nil embedding then natural_of o else None",
     ("datum_unwrap_sound", "envelope_kinds_disjoint")),
    ("re-negative-order", "RecordEnvelope",
     "natural_of accepts a negative order",
     "  | PInt z false => if (0 <=? z)%Z then Some z else None",
     "  | PInt z false => if (0 <=? z)%Z || true then Some z else None",
     ("natural_of_sound",)),
    # ── P4: CounterConflict.v (M-1, A2, A25) ─────────────────────────────
    ("cc-counter-comm-peeks", "CounterConflict",
     "the counter COMM of an operation has peeks, so check #1 does not see it",
     "          ([EvConsume kont; EvComm kont [counter_produce (ChS k) v cr] false;",
     "          ([EvConsume kont; EvComm kont [counter_produce (ChS k) v cr] true;",
     ("counter_consumed",)),
    ("cc-first-op-skips-space-counter", "CounterConflict",
     "the first operation opens S(sid,k) without the COMM on S(sid) (A25)",
     "          ([EvConsume kont; EvComm kont [counter_produce (ChSP s) m crs] false;",
     "          ([EvConsume kont;",
     ("space_counter_consumed",)),
    ("cc-reified-mergeable", "CounterConflict",
     "reified channels are mergeable, so check #1 drops their races",
     "  Definition reified_mergeable (c : chan) : bool := false.",
     "  Definition reified_mergeable (c : chan) : bool := true.",
     ("no_mergeable_produces",)),
    ("cc-pre-state-any-creator", "CounterConflict",
     "every counter produce counts as a pre-state produce, so depends drops the chain",
     "    | PidCounter _ _ Base | PidData _ Base => true",
     "    | PidCounter _ _ _ | PidData _ Base => true",
     ("same_block_ops_chain",)),
    # ── P5 wave W2 (MERGE): CounterConflict.v (the M-1 arbitration, decision 31) ──
    ("cc-removal-skips-counter", "CounterConflict",
     "a removal (a pop or a take) makes no counter events (the M-1 arbitration)",
     "          ([EvConsume kont; EvComm kont [counter_produce (ChS k) v cr] false;\n"
     "            EvProduce (counter_produce (ChS k) (S v) (Made tag))] ++ plan_events tag k q v (sd st k))",
     "          ((match q with\n"
     "            | QDequeue => []\n"
     "            | _ => [EvConsume kont; EvComm kont [counter_produce (ChS k) v cr] false;\n"
     "                    EvProduce (counter_produce (ChS k) (S v) (Made tag))]\n"
     "            end) ++ plan_events tag k q v (sd st k))",
     ("counter_consumed",)),
    ("cc-peek-skips-counter", "CounterConflict",
     "a peek makes no counter events (the rule before decision 31)",
     "          ([EvConsume kont; EvComm kont [counter_produce (ChS k) v cr] false;\n"
     "            EvProduce (counter_produce (ChS k) (S v) (Made tag))] ++ plan_events tag k q v (sd st k))",
     "          ((match q with\n"
     "            | QPeek => []\n"
     "            | _ => [EvConsume kont; EvComm kont [counter_produce (ChS k) v cr] false;\n"
     "                    EvProduce (counter_produce (ChS k) (S v) (Made tag))]\n"
     "            end) ++ plan_events tag k q v (sd st k))",
     ("counter_consumed",)),
    ("cc-crossed-lifo-pops-bottom", "CounterConflict",
     "a pop takes the bottom record (FIFO), so a serial order gives the merged pops",
     "    | QDequeue => match rev l with h :: rest => (rev rest, Some h) | [] => (l, None) end",
     "    | QDequeue => match l with h :: rest => (rest, Some h) | [] => (l, None) end",
     ("crossed_stacks_without_removal_counters_fabricate",)),
    ("cc-rf-bumps-removal", "CounterConflict",
     "the removal-counter-free model bumps the counter of a removal, so the crossed stacks race",
     "  Definition exec_rf (tag : nat) (st : store) (k : Coll) (q : qop) : step_result :=\n"
     "    match q with\n"
     "    | QDequeue =>",
     "  Definition exec_rf (tag : nat) (st : store) (k : Coll) (q : qop) : step_result :=\n"
     "    match q with\n"
     "    | QPeek =>",
     ("crossed_stacks_without_removal_counters_fabricate",)),
    # ── P5 wave W2 (SET): SetDiscipline.v (F5 to F10, Rule E, DR-90, decision 42) ──
    ('sd-key-without-length-framing', 'SetDiscipline', 'the set key concatenates the element encodings without their u64 length fields, so two payloads with one concatenation collide (F5, F6)', 'Definition frame (b : list nat) : list nat := u64_be (length b) ++ b.', 'Definition frame (b : list nat) : list nat := b.', ('frames_injective',)),
    ('sd-key-without-sort', 'SetDiscipline', 'the set key encodes each element as it is, not its sorted Par, so two equal Pars in two orders get two keys (F5)', '  Definition canonical_bytes (p : T) : list nat := enc (srt p).', '  Definition canonical_bytes (p : T) : list nat := enc p.', ('canonical_map',)),
    ('sd-view-keeps-every-member', 'SetDiscipline', 'the view of a set keeps every member of a class, not one representative (the legacy write-time dedup removed, and nothing replaces it)', '  Definition view (rs : list nat) : list nat := dedup [] (canon rs).', '  Definition view (rs : list nat) : list nat := canon rs.', ('set_view_dedups_equal_pars',)),
    ('sd-view-in-list-order', 'SetDiscipline', 'the representative of a class is its first member in the list order of the store, not in the canonical order of DR-75 (decision 41)', '  Definition view (rs : list nat) : list nat := dedup [] (canon rs).', '  Definition view (rs : list nat) : list nat := dedup [] rs.', ('set_view_dedups_equal_pars',)),
    ('sd-consume-retires-representative-only', 'SetDiscipline', 'a receive that does not peek binds and removes the representative only, so the other members of its class stay (the legacy consume; F3 reverted)', '  Definition class_of (k : nat) (rs : list nat) : list nat := filter (same k) (canon rs).', '  Definition class_of (k : nat) (rs : list nat) : list nat := [k].', ('class_of_count',)),
    ('sd-class-fire-removes-persistent-members', 'SetDiscipline', 'a class fire removes its persistent members too, where a COMM never removes a persistent record', "    | c :: C' => remove_all C' (if persistent c then rs else remove_one c rs)", "    | c :: C' => remove_all C' (remove_one c rs)", ('count_remove_all',)),
    ('sd-class-fire-repeats-representative', 'SetDiscipline', 'the class fire lists its representative twice (the representative and then the whole class), so two candidates claim one record (claim_positions)', '    let C := class_of k rs in', '    let C := k :: class_of k rs in', ('set_retire_comm_lines_up',)),
    ('sd-retire-dispatched', 'SetDiscipline', 'a K_retire fire dispatches its result, where a retire is never dispatched (Staged::Retire)', '    {| chans := [d]; pats := [rp]; cands := [(d, m)]; dispatched := 0 |}.', '    {| chans := [d]; pats := [rp]; cands := [(d, m)]; dispatched := 1 |}.', ('set_retire_comm_lines_up',)),
    ('sd-class-fire-dispatches-every-member', 'SetDiscipline', 'the class fire dispatches the result of every member, not of the representative only (F3, F20: binds = 1)', '       cands := map (fun s => (d, s)) C; dispatched := 1 |}.', '       cands := map (fun s => (d, s)) C; dispatched := length C |}.', ('set_retire_comm_lines_up',)),
    ('sd-retire-class-stages-nothing', 'SetDiscipline', 'RETIRE-CLASS stages no K_retire, so a produce-side fire leaves the stored members of the class (decision 37 reverted: the R5 S3 trace)', '    map Retire (filter (fun s => same x s && negb (persistent s)) (canon rs)).', '    map Retire [].', ('retire_class_clears',)),
    ('sd-peek-fire-retires-class', 'SetDiscipline', 'a fire of a peek waiter retires the class of the datum, where a peek waiter takes no datum', '          let retires := if wpeek w then [] else retire_class x rs in', '          let retires := retire_class x rs in', ('offer_retire_after_fire',)),
    ('sd-peek-binds-whole-class', 'SetDiscipline', 'a peek binds the whole class of its choice, not the representative alone', '  Definition peek_group (k : nat) (rs : list nat) : list nat := [k].', '  Definition peek_group (k : nat) (rs : list nat) : list nat := class_of k rs.', ('set_peek_restores_representative_only',)),
    ('sd-peek-without-restore', 'SetDiscipline', 'a peek fire removes the representative and the operation never sends it again (decision 45, Rule E, reverted)', '          let (ops, e) := offer_pass (canon ws) k (remove_all (peek_group k rs) rs) rounds in\n          (FireGroup c (peek_group k rs) :: ops, e)', '          ([FireGroup c (peek_group k rs)], Settled)', ('set_peek_restores_representative_only',)),
    ('sd-membership-compares-hashes-only', 'SetDiscipline', 'the membership test answers with the comparison of the hashes alone, so a hash collision makes two classes one', '    then ([CompareHashes; CompareKeys], keq key other)', '    then ([CompareHashes], true)', ('set_membership_full_compare_on_equal_hash',)),
    ('sd-membership-always-compares-keys', 'SetDiscipline', 'the membership test compares the full keys also on unequal hashes, so it reserves and reads a full comparison for every candidate', '    else ([CompareHashes], false).', '    else ([CompareHashes; CompareKeys], keq key other).', ('set_membership_full_compare_on_equal_hash',)),
    ('sd-key-buffer-filled-before-reserve', 'SetDiscipline', 'the key buffer is filled before it is reserved (decision 42)', '  ++ step (key_buffer kw)', '  ++ [Read (key_buffer kw); Reserve (key_buffer kw)]', ('set_key_trace_covered',)),
    # ── P5 wave W2 (PQ): RecordEnvelope.v (the PQ slots) and PriorityQueue.v (decision 38) ──
    ('re-pq-negative-level', 'RecordEnvelope', 'the priority slot decoder accepts a negative GInt as a level', '  else match natural_of p with Some z => Some (Some z) | None => None end.', '  else match p with PInt z false => Some (Some z) | _ => None end.', ('level_of_slot_sound',)),
    ('re-pq-unprioritized-zero', 'RecordEnvelope', 'the unprioritized level is encoded as GInt(0), the encoding of the level 0', '  match priority with Some p => int_par p | None => PNil end.', '  match priority with Some p => int_par p | None => int_par 0 end.', ('level_of_slot_priority_slot',)),
    ('re-pq-ignores-embedding', 'RecordEnvelope', 'the datum decoder of a priority queue does not check the embedding slot', '          if is_tag t RDatum && is_nil embedding then', '          if is_tag t RDatum then', ('unwrap_pq_datum_sound',)),
    ('re-pq-policy-any-key', 'RecordEnvelope', 'the policy decoder accepts a map with any string key', '      if bytes_eqb k key_priority then', '      if true then', ('level_of_policy_sound',)),
    ('re-pq-into-strict', 'RecordEnvelope', 'the outcome decoder decodes with the strict P4 decoder, which rejects a policy', 'Definition into_continuation (k : tagged) : option (Z * tagged) :=\n  match unwrap_pq_continuation k with\n  | Some (z, _, original) => Some (z, original)\n  | None => None\n  end.', 'Definition into_continuation (k : tagged) : option (Z * tagged) := unwrap_continuation k.', ('continuation_policy_slot_roundtrip',)),
    ('pq-head-greatest', 'PriorityQueue', 'the lane-head scan keeps the greater order instead of the least', '  Definition below (a b : E) : bool := (k1 a <? k1 b) || ((k1 a =? k1 b) && (k2 a <? k2 b)).', '  Definition below (a b : E) : bool := (k1 b <? k1 a) || ((k1 a =? k1 b) && (k2 a <? k2 b)).', ('below_false_le',)),
    ('pq-head-by-position', 'PriorityQueue', 'the head of a lane is its first entry in list order', '  Definition least (l : list E) : option E := least_from None l.', '  Definition least (l : list E) : option E := hd_error l.', ('least_spec',)),
    ('pq-unprioritized-first', 'PriorityQueue', 'the unprioritized level comes before every priority level (F6 reverted)', '  Definition lvl_class (lvl : option nat) : nat := match lvl with Some _ => 0 | None => 1 end.', '  Definition lvl_class (lvl : option nat) : nat := match lvl with Some _ => 1 | None => 0 end.', ('lvl_key_le',)),
    ('pq-first-match-by-position', 'PriorityQueue', 'a receive binds the first matching level head in list order, not in level order', '    least (fun r => lvl_class (r_lvl r)) (fun r => lvl_index (r_lvl r))', '    hd_error', ('first_match_spec',)),
    ('pq-level-lane-sees-all', 'PriorityQueue', 'the waiter queue of a level sees every level (decision 38 reverted)', '    match lane with Some p => lvl_eqb lvl (Some p) | None => true end.', '    match lane with Some p => true | None => true end.', ('sees_level',)),
    ('pq-fire-behind-head', 'PriorityQueue', 'a receive binds the first visible matching record in list order, also behind a level head', '        match first_match (s_records st) (w_lane k) (w_pat k) with\n        | None => mkPlanned [SPark k] PDone', '        match find (fun r => sees (w_lane k) (r_lvl r) && spatial (w_pat k) (r_val r)) (s_records st) with\n        | None => mkPlanned [SPark k] PDone', ('pq_unrestricted_first_matching_level',)),
    ('pq-guard-every-head', 'PriorityQueue', 'a guard refusal moves on to the next level head instead of parking (decision 38 reverted)', '                  mkPlanned (fire ++ p_ops rest) (p_end rest)\n              end\n            else mkPlanned [SPark k] PDone', "                  mkPlanned (fire ++ p_ops rest) (p_end rest)\n              end\n            else receive_loop fuel' fired (mkS (drop_record h (s_records st)) (s_waiters st) (s_counter st)) k", ('pq_unrestricted_first_matching_level',)),
    ('pq-settle-any-order', 'PriorityQueue', 'the settle fires the first fireable head in list order, not the oldest (S4 reverted)', '  Definition oldest (l : list waiter) : option waiter := least w_ord w_rank l.', '  Definition oldest (l : list waiter) : option waiter := hd_error l.', ('oldest_spec',)),
    ('pq-offer-any-order', 'PriorityQueue', 'a produce plan tests the head of the level queue first, whatever the orders (decision 38 reverted)', '        if w_ord w2 <? w_ord w1 then', '        if false then', ('offer_pick_older_first',)),
    ('pq-offer-refusal-stores', 'PriorityQueue', 'a refusal of the older candidate stores the datum although the other candidate fits (decision 37 reverted)', '          else if fits_datum w1 v then (Some w1, (ref1, true))', '          else if false then (Some w1, (ref1, true))', ('offer_pick_older_first',)),
    ('pq-no-settle-after', 'PriorityQueue', 'an operation whose plan completes runs no settle after the plan', '          | PDone =>\n              let after := settle_at (f + fires (p_ops p)) (run st (z_ops before ++ p_ops p)) in\n              mkOutcome (z_ops before ++ p_ops p ++ z_ops after) (settle_next (z_end after))', '          | PDone => mkOutcome (z_ops before ++ p_ops p) None', ('with_settles_inv',)),
    ('pq-no-lift', 'PriorityQueue', 'the settle re-offers a stored record without its lift', '    match x with Some _ => [] | None => if r_pers r then [] else [SLift r] end.', '    match x with Some _ => [] | None => if r_pers r then [] else [] end.', ('settle_step_records',)),
    ('pq-land-drops', 'PriorityQueue', 'the record in flight does not land: the settle loses it', '    match x with None => [] | Some r => [SRestore r] end.', '    match x with None => [] | Some _ => [] end.', ('run_land_records',)),
    ('pq-view-without-flight', 'PriorityQueue', 'the record in flight is not a level head of the view of a settle', '    match x with Some r => r :: s_records st | None => s_records st end.', '    match x with Some _ => s_records st | None => s_records st end.', ('run_land_records',)),
    ('pq-no-land-first', 'PriorityQueue', 'a fire of a stored record while a record is in flight does not land the record in flight first', '  Definition lands_first (x : option record) (in_flight : bool) : bool := has x && negb in_flight.', '  Definition lands_first (x : option record) (in_flight : bool) : bool := false.', ('next_fire_bound',)),
    ('pq-peek-spends-lifted', 'PriorityQueue', 'a fire of a waiter that peeks spends the lifted record (decision 30 reverted)', '    if flying x r && negb spent then Some r else None.', '    None.', ('settle_loop_keeps',)),
    ('pq-no-cap-check', 'PriorityQueue', 'the settle loop has no cap check of the transaction at the top of a round (I-21)', '  Definition at_cap (fired : nat) : bool := cap <=? fired.', '  Definition at_cap (fired : nat) : bool := false.', ('settle_loop_bounds', 'settle_loop_cap')),
    ('pq-no-divergent-end', 'PriorityQueue', 'a divergent pair goes on instead of ending the round (A10)', '    else if stays then FDivergent', '    else if stays then FContinue', ('settle_loop_bounds',)),
    ('pq-peek-no-restore', 'PriorityQueue', 'a consume-side peek of a record that is not persistent stages no re-store (Rule E)', '    if w_peek k && negb (r_pers h) then [SFireConsume k h; SRestore h] else [SFireConsume k h].', '    if w_peek k && negb (r_pers h) then [SFireConsume k h] else [SFireConsume k h].', ('pq_peek_restores_head',)),
    ('pq-fresh-jumps-queue', 'PriorityQueue', 'a fresh datum plans at the head although its level has an older head', '    match record_head (s_records s) (r_lvl x) with\n    | Some _ => mkPlanned [SStore x] PDone', '    match record_head (s_records s) (r_lvl x) with\n    | Some _ => offer_loop (S cap) f s x (false, false)', ('pq_fifo_within_level',)),
    ('pq-reentry-at-head', 'PriorityQueue', 'a re-entered datum plans at the head whatever the order of the head (decision 43)', '    | Some h => if r_ord x <? r_ord h then offer_loop (S cap) f s x (false, false)\n                else mkPlanned [SStore x] PDone', '    | Some _ => offer_loop (S cap) f s x (false, false)', ('pq_fifo_within_level',)),
    ('pq-reentry-empty-level-stores', 'PriorityQueue', 'a re-entered datum on an empty level is stored with no plan, so it never fires', '                else mkPlanned [SStore x] PDone\n    | None => offer_loop (S cap) f s x (false, false)', '                else mkPlanned [SStore x] PDone\n    | None => mkPlanned [SStore x] PDone', ('reentry_at_fifo_place', 'pq_reissue_takes_level_fifo_place')),
    ('pq-reentry-waiter-at-head', 'PriorityQueue', 'a re-entered continuation plans at the head whatever the order of the head (decision 43)', '    | Some w => if w_ord k <? w_ord w then receive_loop (S cap) f s k\n                else mkPlanned [SPark k] PDone', '    | Some _ => receive_loop (S cap) f s k', ('pq_fifo_within_level',)),
    ('pq-reentry-empty-lane-parks', 'PriorityQueue', 'a re-entered continuation on an empty lane parks with no plan, so it never fires', '                else mkPlanned [SPark k] PDone\n    | None => receive_loop (S cap) f s k', '                else mkPlanned [SPark k] PDone\n    | None => mkPlanned [SPark k] PDone', ('reentry_at_fifo_place', 'pq_reentered_waiter_keeps_seniority')),
    ('pq-reissue-fresh-ord', 'PriorityQueue', 'a re-issued persistent operation carries no first-round order (decision 43)', '  Definition carried (pers : bool) (o : nat) : option nat := if pers then Some o else None.', '  Definition carried (pers : bool) (o : nat) : option nat := None.', ('pq_reissue_takes_level_fifo_place',)),
    ('pq-no-bump', 'PriorityQueue', 'an operation does not advance the counter', '  Definition bump_next (c : option nat) : option nat := Some (S (bump_ord c)).', '  Definition bump_next (c : option nat) : option nat := Some (bump_ord c).', ('pq_reissue_takes_level_fifo_place',)),
    ('pq-perm-without-distinct-ords', 'PriorityQueue', 'the permutation invariance without the wf premise of distinct orders', '  Definition dist (st : sstate) : Prop :=\n    ords_distinct r_ord (s_records st) /\\ ords_distinct w_ord (s_waiters st).', '  Definition dist (st : sstate) : Prop := True /\\ True.', ('apply_equiv',)),
    ('pq-priority-negative', 'PriorityQueue', 'check_values admits a negative priority', '    (0 <=? p)%Z && match bound with Some n => (p <? Z.of_nat n)%Z | None => true end.', '    true && match bound with Some n => (p <? Z.of_nat n)%Z | None => true end.', ('pq_priority_domain',)),
    ('pq-priority-bound-ignored', 'PriorityQueue', 'check_values ignores the bound priorities of the space (I-10)', '    (0 <=? p)%Z && match bound with Some n => (p <? Z.of_nat n)%Z | None => true end.', '    (0 <=? p)%Z && match bound with Some n => true | None => true end.', ('pq_priority_domain',)),
    ('pq-level-storage', 'PriorityQueue', 'a store also keeps an entry for the level of its datum (storage for each level, I-10 reverted)', '    | SStore r => mkS (r :: s_records st) (s_waiters st) (s_counter st)', '    | SStore r => mkS (r :: r :: s_records st) (s_waiters st) (s_counter st)', ('store_after_bump',)),
    ('pq-read-before-reserve', 'PriorityQueue', 'a metered step reads before it reserves (DR-88)', 'Definition step (units : nat) : list event := [Reserve units; Read units].', 'Definition step (units : nat) : list event := [Read units; Reserve units].', ('step_covered',)),
    ('pq-unprepaid-unwrap', 'PriorityQueue', 'a settle round does not prepay the decode of the fired continuation (decision 42)', '    ++ flat_map step (lift r)\n    ++ [Reserve (unwrap r)]\n    ++ flat_map step (reoffer r).', '    ++ flat_map step (lift r)\n    ++ flat_map step (reoffer r).', ('round_staged_shape',)),
    ('pq-plan-opens-at-cap', 'PriorityQueue', 'open_plan has no cap check: the plan opens after a settle that ended at the cap (I-21)', '  Definition plan_at_cap (fired : nat) : bool := cap <=? fired.', '  Definition plan_at_cap (fired : nat) : bool := false.', ('pq_settle_terminates',)),
    ('pq-plan-restarts-count', 'PriorityQueue', 'the plan restarts the fire count at 0 instead of the count of the settle before it (I-21)', '          let p := plan f (run st (z_ops before)) in', '          let p := plan 0 (run st (z_ops before)) in', ('pq_settle_terminates',)),
    ('pq-after-settle-restarts-count', 'PriorityQueue', 'the settle after the plan restarts the fire count at 0 (each settle call has its own count)', 'settle_at (f + fires (p_ops p))', 'settle_at 0', ('pq_settle_terminates',)),
    # ── P4: HeadOnlyDisciplines.v (decision 11, A10, decision 31) ────────
    ("ho-head-greatest-ord", "HeadOnlyDisciplines",
     "the head is the entry with the greatest ord",
     "    (o1 <? o2) || ((o1 =? o2) && (r1 <? r2)).",
     "    (o2 <? o1) || ((o1 =? o2) && (r1 <? r2)).",
     ("precedes_false_le", "queue_head_is_min_ord")),
    ("ho-datum-skips-tail", "HeadOnlyDisciplines",
     "a datum skips the queued records and meets the head waiter",
     "  Definition offer (st : qstate) (r : record) : outcome :=\n"
     "    match q_records st with\n"
     "    | _ :: _ => mkOutcome [SStore r] (store_record st r) None",
     "  Definition offer (st : qstate) (r : record) : outcome :=\n"
     "    match q_records st with\n"
     "    | _ :: _ => offer_loop fire_round_cap 0 st r",
     ("offer_props", "queue_produce_preserves_inv", "queue_fifo")),
    ("ho-no-divergent-reissue", "HeadOnlyDisciplines",
     "a fire whose partner stays continues instead of the divergent re-issue (A10)",
     "  else if stays then FDivergent",
     "  else if stays then FContinue",
     ("after_fire_continue", "queue_loop_bounded_or_reissues")),
    ("ho-counter-not-bumped", "HeadOnlyDisciplines",
     "an operation does not bump the counter, so two operations share an ord",
     "  Definition bump_next (c : option nat) : option nat := Some (S (bump_ord c)).",
     "  Definition bump_next (c : option nat) : option nat := Some (bump_ord c).",
     ("bump_ord_next", "queue_ord_fresh")),
    # ── Decision 43: HeadOnlyDisciplines.v (re-issued persistent operations) ─
    ("ho-reentry-fresh-ord", "HeadOnlyDisciplines",
     "a re-issued persistent operation takes the ord of its bump, not its first-round ord",
     "    | Some o => if o <? bump then Some o else None",
     "    | Some o => if o <? bump then Some bump else None",
     ("queue_reissue_keeps_ord",)),
    ("ho-reentry-ignores-head", "HeadOnlyDisciplines",
     "a re-entered element is at the head whatever the ord of the head (QD-9)",
     "    | Some h => if o =? h then None else Some (ord_precedes o h)",
     "    | Some h => if o =? h then None else Some true",
     ("reenter_offer_shadow", "queue_reentry_preserves_shadow_inv")),
    ("ho-nonpersistent-keeps-ord", "HeadOnlyDisciplines",
     "a re-issued non-persistent element keeps its first-round ord (the scope rule)",
     "    if flight_pers x then Some (flight_ord x) else None.",
     "    Some (flight_ord x).",
     ("queue_quiescent_inv",)),
    # ── Decision 45: HeadOnlyDisciplines.v (Rule E: the re-store of a peek) ─
    ("ho-restore-skipped", "HeadOnlyDisciplines",
     "the code stages no re-store after a peek fire of a non-persistent record",
     "then [CStaged s; CRestore h]",
     "then [CStaged s]",
     ("restore_fire_step", "queue_restore_realizes_plan")),
    ("ho-peek-retains", "HeadOnlyDisciplines",
     "a peek fire keeps the records that it binds (the retention before decision 45)",
     "    | CStaged (SFireConsume _ h) => if r_pers h then l else remove_key (r_ord h) (r_rank h) l",
     "    | CStaged (SFireConsume k h) => if r_pers h || w_peek k then l else remove_key (r_ord h) (r_rank h) l",
     ("restore_fire_step", "queue_restore_realizes_plan")),
    ("ho-restore-persistent", "HeadOnlyDisciplines",
     "the code also re-stores a peeked persistent record, which the fire keeps",
     "if w_peek k && negb (r_pers h) then",
     "if w_peek k then",
     ("restore_fire_step", "queue_restore_realizes_plan")),
    # ── P4: SpaceRefs.v (C.1, C.2, A23) ──────────────────────────────────
    ("sr-handle-ignores-write-flag", "SpaceRefs",
     "a handle under a bundle without the write flag is a native space",
     "  | Some (RHandle _), false => SRErr HandleNotWritable",
     "  | Some (RHandle n), false => SROk (SNative n)",
     ("classify_space_ref_total_deterministic", "handle_requires_write_flag")),
    ("sr-forged-ignores-context", "SpaceRefs",
     "a forged channel routes to the default space inside a use block (R5)",
     "    | CForged, Some t => space_route (t_native t) c",
     "    | CForged, Some t => RouteOk RDefault",
     ("route_total_deterministic", "route_rules")),
    ("sr-space-name-takes-context", "SpaceRefs",
     "a space name routes to the space of the use-context (R3)",
     "    | CSpaceName n, _ => space_route n c",
     "    | CSpaceName n, Some t => space_route (t_native t) c\n"
     "    | CSpaceName n, None => space_route n c",
     ("route_total_deterministic", "route_rules", "space_name_route_ignores_ctx")),
    ("sr-wrapper-drops-context", "SpaceRefs",
     "a stored body is not wrapped, so it runs without its context",
     "  match cx with Some t => use_tuple (t_space t) body | None => body end.",
     "  body.",
     ("use_wrapper_restores_ctx",)),
    ("sr-any-reply-name", "SpaceRefs",
     "the handle replies on a ret that is not a name (A23)",
     "            if is_reply_name ret then",
     "            if true then",
     ("parse_message_ret",)),
    # ── P4: SpaceFactory.v (A13, A21) ────────────────────────────────────
    ("sf-hashset-takes-default", "SpaceFactory",
     "the HashSet store takes every qualifier",
     "  | HashSet, _ => false",
     "  | HashSet, _ => true",
     ("qualifier_matrix",)),
    ("sf-family-index-collision", "SpaceFactory",
     "the pathmap families start at index 6, so two families share an index",
     "  match o with HashMap => 0 | PathMap => 7 | Array => 13 | Vector => 19 | HashSet => 26 end.",
     "  match o with HashMap => 0 | PathMap => 6 | Array => 13 | Vector => 19 | HashSet => 26 end.",
     ("supported_in_all", "from_family_index_of", "family_index_bijective")),
    # Replaced by sf-ann-not-a-call-op (P6 step 7): is_call_op now ends at ann_op (0x25), so a
    # change of vector_op no longer changes the range of the call ops, and this mutant would
    # survive.
    # ("sf-vector-not-a-call-op", "SpaceFactory",
    #  "vector takes no calls, so `!!` on it does not abort",
    #  "Definition vector_op : nat := 35.",
    #  "Definition vector_op : nat := 34.",
    #  ("persistent_op_call_aborts",)),
    ("sf-ann-not-a-call-op", "SpaceFactory",
     "ann takes no calls, so `!!` on it does not abort",
     "Definition ann_op : nat := 37.",
     "Definition ann_op : nat := 36.",
     ("persistent_op_call_aborts",)),
    ("sf-recreate-allowed", "SpaceFactory",
     "a creation over an existing config record stages a second space",
     "    | _ :: _ => CreateErr SpaceAlreadyExists",
     "    | _ :: _ => Staged [ParkHandle]",
     ("space_already_exists_is_deterministic_abort",)),
    ("sf-theory-before-qualifier", "SpaceFactory",
     "to_elements puts the FREE tuple before the qualifier",
     "    match c_qualifier c with Some q => [qual_elem q] | None => [] end ++\n"
     "    match c_theory c with Some t => [free_elem t] | None => [] end ++",
     "    match c_theory c with Some t => [free_elem t] | None => [] end ++\n"
     "    match c_qualifier c with Some q => [qual_elem q] | None => [] end ++",
     ("constructor_parse_canonical",)),
    ("sf-qualifier-bits-unchecked", "SpaceFactory",
     "qualifier_of does not check the locally_free bits, so a qualifier element with bits "
     "parses and to_elements does not give it back",
     "  Definition qual_of (e : elem) : qual_result := if clean e then qual_name_of e else QNonCanonical.",
     "  Definition qual_of (e : elem) : qual_result := qual_name_of e.",
     ("constructor_parse_sound",)),
    # ── I-20: Encoding.v ─────────────────────────────────────────────────
    ("enc-refresh-keeps-stale-flags", "Encoding",
     "the hooks decode without the refresh (the P2 behavior before I-20)",
     "Definition refresh (x : ftuple) : ftuple := mkFTuple (ft_construct x) (derived_flags (ft_construct x)).",
     "Definition refresh (x : ftuple) : ftuple := x.",
     ("refresh_strict_decode_erase",)),
    ("enc-use-flag-ignores-body", "Encoding",
     "the derived flag of a USE tuple ignores the flag of the body",
     "      let t := vflag s || vflag b in",
     "      let t := vflag s in",
     ("connective_marker_tuple_recognized",)),
    ("enc-strict-check-dropped", "Encoding",
     "the strict decoder ignores the flags (a weaker decoder contract than decision 21)",
     "  if tuple_flags_eqb (ft_flags x) (derived_flags (ft_construct x)) then decode (ferase x) else None.",
     "  decode (ferase x).",
     ("refresh_strict_decode_erase",)),
    # ── Decision 46: SpaceRefs.v ─────────────────────────────────────────
    # Retired by decision 47 with the headline native_execution_fails_closed:
    # native play now runs space jobs, so this mutant states the new rule.
    # ("sr-native-play-runs-spaces", "SpaceRefs",
    #  "a host runtime runs space jobs in native play, as before decision 46",
    #  "  | RtHost => if native then Some SpacesUnsupportedInNativeExecution else None",
    #  "  | RtHost => None",
    #  ("native_execution_fails_closed",)),
    # ── Decision 47: SpaceRefs.v ─────────────────────────────────────────
    ("sr-native-play-rejects-spaces", "SpaceRefs",
     "a host runtime rejects space jobs in native play, as decision 46 did",
     "  | RtHost => None",
     "  | RtHost => if native then Some RuntimeUnavailable else None",
     ("native_execution_runs_spaces",)),
    ("sr-unbound-native-replay-runs", "SpaceRefs",
     "a native replay runtime runs space jobs before its environment binds it",
     "  | RtNativeReplay bound => if bound then None else Some RuntimeUnavailable",
     "  | RtNativeReplay bound => None",
     ("native_execution_runs_spaces",)),
    # ── Decision 42: AgentReadCoverage.v ─────────────────────────────────
    ("arc-read-before-reserve", "AgentReadCoverage",
     "a step reads before it reserves (the P4 order: copy, then charge)",
     "Definition step (units : nat) : list event := [Reserve units; Read units].",
     "Definition step (units : nat) : list event := [Read units; Reserve units].",
     ("step_covered",)),
    ("arc-unwrap-not-prepaid", "AgentReadCoverage",
     "a fired round reserves nothing for the decode of its continuation",
     "  ++ (if fires r then [Reserve (unwrap r)] else [])",
     "  ++ (if fires r then [Reserve 0] else [])",
     ("round_staged_totals",)),
    ("arc-outcome-not-prepaid", "AgentReadCoverage",
     "the operation does not prepay the buffer of the outcome",
     "  ++ flat_map round_staged (rounds x) ++ [Reserve (outcome x)].",
     "  ++ flat_map round_staged (rounds x).",
     ("staged_totals",)),
    ("arc-outcome-reserves-after-commit", "AgentReadCoverage",
     "the outcome reserves its buffer after the commit",
     "  Read (outcome x) :: flat_map round_after_commit (rounds x).",
     "  Reserve (outcome x) :: Read (outcome x) :: flat_map round_after_commit (rounds x).",
     ("operation_trace_covered", "reservations_precede_commit")),
    ("arc-scan-copies-every-entry", "AgentReadCoverage",
     "a scan copies every entry instead of the head only",
     "  ++ flat_map step (map (fun _ => compare) (tl side))\n  ++ step (payload head).",
     "  ++ flat_map step (map (fun _ => compare) (tl side))\n"
     "  ++ flat_map step (map payload side) ++ step (payload head).",
     ("scan_reserved", "scan_charge_independent_of_unread_payloads")),
    # ── Decision 45, Option P: LegacyCharges.v (Q79, Q80) ────────────────
    ("lc-tail-dropped", "LegacyCharges",
     "the job never runs the tail (LegacyChain::finish charges nothing), so the post-charges "
     "of the last operation are lost",
     "Definition finish (ch : chain) (s : state) : result := charge_all (chain_carry ch) s.",
     "Definition finish (ch : chain) (s : state) : result := (s, Done).",
     ("commit_finish_rebracket",)),
    ("lc-carry-dropped", "LegacyCharges",
     "a callback does not charge the carry, so the post-charges of each operation before the "
     "last are lost",
     "  (mkCallback (chain_next ch) o (chain_carry ch),",
     "  (mkCallback (chain_next ch) o [],",
     ("commit_finish_rebracket",)),
    ("lc-pre-after-body", "LegacyCharges",
     "the pre-charge comes after the observer calls and the events of its operation, so a "
     "pre-charge that runs out of phlo stops after the body",
     ("  charge (PPre (cb_index cb)) (pre (cb_op cb)) s2)).",
      "      bind (body paired (cb_index cb) (cb_op cb) s1) (fun s2 =>\n"
      "      commit paired rest s2))"),
     ("  (s2, Done))).",
      "      bind (body paired (cb_index cb) (cb_op cb) s1) (fun s2 =>\n"
      "      bind (charge (PPre (cb_index cb)) (pre (cb_op cb)) s2) (fun s3 =>\n"
      "      commit paired rest s3)))"),
     ("commit_finish_rebracket",)),
    ("lc-replay-unpaired", "LegacyCharges",
     "Q80 without the premise of H23 that replay pairs each operation whose body play applied",
     "    (forall i, i < length (bodies (fst pr)) -> paired i = true) ->",
     "    True ->",
     ("legacy_replay_stops_with_play",)),
    ("lc-unpaired-skips-observe", "LegacyCharges",
     "an unpaired replay body diverges before its observer calls (decision 36 reverted), so a "
     "body that the observer rejects in play diverges in replay",
     "  bind (call_step (PObserve i) (CObserve o) s) (fun s1 =>\n"
     "    if paired i then (run_body o s1, Done) else (s1, Diverged (PObserve i))).",
     "  if paired i\n"
     "  then bind (call_step (PObserve i) (CObserve o) s) (fun s1 => (run_body o s1, Done))\n"
     "  else (s, Diverged (PObserve i)).",
     ("host_op_pairing",)),
    # ── P5 W2: CellDiscipline.v (batch CELL, F15) ───────────────────────────────────────
    ("cd-no-capacity-check", "CellDiscipline",
     "a store or a park never checks the capacity of its side (F15)",
     "  | Some bound => held <? bound\n  end.",
     "  | Some bound => true\n  end.",
     ("store_checked_spec", "park_checked_spec", "offer_loop_capacity", "receive_loop_capacity", "cell_capacity_one")),
    ("cd-capacity-admits-a-full-slot", "CellDiscipline",
     "the capacity check admits a store into a full slot (held <= bound)",
     "  | Some bound => held <? bound\n  end.",
     "  | Some bound => held <=? bound\n  end.",
     ("store_checked_spec", "park_checked_spec", "offer_loop_capacity", "receive_loop_capacity", "cell_capacity_one")),
    ("cd-capacity-rejects-an-empty-slot", "CellDiscipline",
     "the capacity check rejects every store and park, so a plan fails without an overflow",
     "  | Some bound => held <? bound\n  end.",
     "  | Some bound => false\n  end.",
     ("store_checked_spec", "park_checked_spec", "offer_loop_capacity", "receive_loop_capacity", "cell_capacity_one")),
    ("cd-restore-counts-the-removed-record", "CellDiscipline",
     "the capacity view of the re-store counts the record that the peek fire removed",
     "    | SRestore h =>\n        if has_room cell_capacity (length (c_records v))",
     "    | SRestore h =>\n        if has_room cell_capacity (S (length (c_records v)))",
     ("receive_fire_replays", "receive_loop_replays", "cell_restore_never_occupied")),
    ("cd-peek-retains-the-record", "CellDiscipline",
     "a consume-side peek fire keeps its record (the retention before decision 45)",
     "    | SFireConsume _ h =>\n        Some (if r_pers h then v else",
     "    | SFireConsume k h =>\n        Some (if r_pers h || w_peek k then v else",
     ("receive_fire_replays", "receive_loop_replays", "cell_restore_never_occupied")),
    ("cd-reentry-queue-rule", "CellDiscipline",
     "a re-entered element waits behind an older head (the precedes rule of the queue)",
     "Definition cell_precedes (ord head : nat) : bool := true.",
     "Definition cell_precedes (ord head : nat) : bool := ord <? head.",
     ("cell_reentry_as_fresh",)),
    ("cd-fresh-joins-the-tail", "CellDiscipline",
     "a fresh element reaches the head only of an empty side (the reaches_head rule of the queue)",
     "Definition cell_reaches_head (queued : nat) : bool := true.",
     "Definition cell_reaches_head (queued : nat) : bool := queued =? 0.",
     ("cell_reentry_as_fresh",)),
    # ── P5 W2: OuterStorage.v, VectorStore.v, HashMapStore.v (batch ARRAY, P5d) ─────────
    ("os-fixed-unbounded", "OuterStorage",
     "a fixed array allocates past its size (no OutOfNames)",
     "  else if n + k <=? s then Allocated (seq n k) (n + k) else OutOfNames.",
     "  else Allocated (seq n k) (n + k).",
     ("fixed_plan_full",)),
    ("os-fixed-strict-bound", "OuterStorage",
     "a fixed array refuses an allocation that fills it exactly",
     "  else if n + k <=? s then Allocated (seq n k) (n + k) else OutOfNames.",
     "  else if n + k <? s then Allocated (seq n k) (n + k) else OutOfNames.",
     ("fixed_plan_fits",)),
    ("os-fixed-indices-shifted", "OuterStorage",
     "a fixed array allocates from n + 1 instead of n",
     "  else if n + k <=? s then Allocated (seq n k) (n + k) else OutOfNames.",
     "  else if n + k <=? s then Allocated (seq (S n) k) (n + k) else OutOfNames.",
     ("fixed_plan_fits",)),
    ("os-cyclic-no-wrap", "OuterStorage",
     "the cyclic loop does not wrap to 0 at the size",
     "  | S k' => idx :: wrap_from s (if idx + 1 =? s then 0 else idx + 1) k'",
     "  | S k' => idx :: wrap_from s (idx + 1) k'",
     ("wrap_from_mod",)),
    ("os-cyclic-next-unreduced", "OuterStorage",
     "the next value of a cyclic array is n + k, not reduced modulo the size",
     "  if n <? s then Allocated (wrap_from s n k) ((n + k mod s) mod s) else Inconsistent.",
     "  if n <? s then Allocated (wrap_from s n k) (n + k) else Inconsistent.",
     ("cyclic_array_index_mod_size",)),
    ("os-alloc-stages-on-failure", "OuterStorage",
     "a failed plan still stages the take of the allocator (a partial allocation)",
     "    | o => (o, [])",
     "    | o => (o, [Take n job])",
     ("fixed_array_out_of_names_exact", "allocation_batch_atomic")),
    ("os-allocator-not-advanced", "OuterStorage",
     "a vector allocation puts the allocator back at its old value",
     "    if n + k <=? vmax then Allocated (seq n k) (n + k) else OutOfNames.",
     "    if n + k <=? vmax then Allocated (seq n k) n else OutOfNames.",
     ("vector_run_seq",)),
    ("os-put-random-shared", "OuterStorage",
     "the put of every job takes one random state, so a repeated cyclic value repeats its identity",
     "    | Put v job => ProduceId v (job_random job 1)",
     "    | Put v job => ProduceId v (job_random 0 1)",
     ("alloc_identities",)),
    ("os-decode-unreserved", "OuterStorage",
     "the decode of the allocator value reads before it reserves",
     "    step (u_shape u) ++ step (u_copy u) ++ step (u_decode u) ++",
     "    step (u_shape u) ++ step (u_copy u) ++ [Read (u_decode u)] ++",
     ("alloc_trace_covered",)),
    ("os-after-commit-read", "OuterStorage",
     "the outcome reads after the commit without a prepaid reservation",
     "  Definition after_commit_trace : list event := [].",
     "  Definition after_commit_trace : list event := [Read 1].",
     ("alloc_trace_covered",)),
    ("os-key-check-inclusive", "OuterStorage",
     "an Array space accepts the index equal to its size",
     "        | Fixed s | Cyclic s => i <? s",
     "        | Fixed s | Cyclic s => i <=? s",
     ("check_key_bounded",)),
    ("os-key-check-any-space", "OuterStorage",
     "a store accepts the index names of another space",
     "        Nat.eqb sp space &&",
     "        true &&",
     ("check_key_bounded",)),
    ("vs-not-advanced", "VectorStore",
     "a vector allocation puts the allocator back at its old value",
     "    if n + k <=? vmax then Some (seq n k, n + k) else None.",
     "    if n + k <=? vmax then Some (seq n k, n) else None.",
     ("vplan_fits",)),
    ("vs-index-not-counter", "VectorStore",
     "a vector allocation returns indices from n + 1 instead of the counter value n",
     "    if n + k <=? vmax then Some (seq n k, n + k) else None.",
     "    if n + k <=? vmax then Some (seq (S n) k, n + k) else None.",
     ("vplan_fits",)),
    ("vs-unbounded", "VectorStore",
     "a vector allocation passes the bound of the allocator record",
     "    if n + k <=? vmax then Some (seq n k, n + k) else None.",
     "    Some (seq n k, n + k).",
     ("vplan_full",)),
    ("vs-after-stays", "VectorStore",
     "the commit of an allocation leaves the allocator at its old value",
     "    | Some (_, next) => next",
     "    | Some (_, next) => n",
     ("vrun_seq",)),
    ("vs-key-check-any-space", "VectorStore",
     "a Vector space accepts the index names of another space",
     "    | IndexName sp _ => Nat.eqb sp space",
     "    | IndexName sp _ => true",
     ("vcheck_key_spec",)),
    ("hm-key-ignored", "HashMapStore",
     "the store files every record under every key (one collection for every channel)",
     "    if key_eq_dec (fst entry) k then true else false.",
     "    true.",
     ("is_key_other",)),
    ("hm-get-head-by-position", "HashMapStore",
     "a reader sees only the first record in the list order of the store",
     "  Definition get (k : K) (s : store) : list R := map snd (filter (is_key k) s).",
     "  Definition get (k : K) (s : store) : list R := firstn 1 (map snd (filter (is_key k) s)).",
     ("get_permutation",)),
    ("hm-take-any-key", "HashMapStore",
     "a removal takes the first matching record of any collection",
     "        then (if p r then rest else (k', r) :: take k p rest)\n        else (k', r) :: take k p rest",
     "        then (if p r then rest else (k', r) :: take k p rest)\n        else (if p r then rest else (k', r) :: take k p rest)",
     ("filter_take_other",)),
    ("hm-iteration-list-order", "HashMapStore",
     "the iteration reads each collection in the list order of the store",
     "    map (fun k => (k, view (get k s))) keys.",
     "    map (fun k => (k, get k s)) keys.",
     ("hashmap_iteration_permutation_invariant",)),
    # ── P5 W2: SpaceRefs.v, receive queries (batch HOOKS) ───────────────────────────────
    ("sr-query-fast-path-admits-markers", "SpaceRefs",
     "the fast path admits marker tuples (P4), so a QUERY tuple can become a channel of the host",
     "  routes_to_default_without_context c && negb (is_marker_tuple c).",
     "  routes_to_default_without_context c.",
     ("query_source_routes_inner_channel",)),
    ("sr-query-routes-the-tuple", "SpaceRefs",
     "the receive routes the QUERY tuple instead of its inner channel",
     "        | Readable ch => route_bind cx ch (Some q)",
     "        | Readable ch => route_bind cx c (Some q)",
     ("query_source_routes_inner_channel",)),
    ("sr-query-default-parks", "SpaceRefs",
     "a query on the default space becomes the consume of the host and drops the query",
     "    | BindOk _ (Some _) RDefault => TFail RQuerySpecUnsupported",
     "    | BindOk ch (Some _) RDefault => THost ch",
     ("query_source_routes_inner_channel",)),
    ("sr-query-skips-read-rule", "SpaceRefs",
     "the inner channel of a query skips the read rule of unbundle_receive",
     "        match read_channel inner with",
     "        match Readable inner with",
     ("query_source_routes_inner_channel",)),
    ("sr-h4-rejects-queries", "SpaceRefs",
     "H4 still rejects every QUERY tuple, as in P4",
     "    match decode_source c with SrcMalformed => false | SrcPlain | SrcQuery _ _ => true end.",
     "    match decode_source c with SrcMalformed | SrcQuery _ _ => false | SrcPlain => true end.",
     ("query_source_routes_inner_channel",)),
    # ── P5 W2: SpaceFactory.v, the P5 families (batch FACTORY) ──────────────────────────
    ("sf-p5-register-unwired", "SpaceFactory",
     "a P5 row is registered without its dispatch arm",
     "    p4_rows ++ filter (fun r => wired (fst r)) p5_rows ++",
     "    p4_rows ++ p5_rows ++",
     ("registered_rows",)),
    ("sf-p5-row-wrong-op", "SpaceFactory",
     "the hashmap:set row takes the op 2 of hashmap:stack instead of its index 3",
     "row HashMap SetC 3;",
     "row HashMap SetC 2;",
     ("rows_facts",)),
    ("sf-p5-define-unregistered", "SpaceFactory",
     "definitions installs the factory of every P5 row, wired or not",
     "    map (fun r => (snd r, Some (fst r))) (filter (fun r => wired (fst r)) p5_rows) ++",
     "    map (fun r => (snd r, Some (fst r))) p5_rows ++",
     ("definitions_incl", "registered_families_complete")),
    ("sf-p5-alias-other-op", "SpaceFactory",
     "the alias HashMapBagSpace names the op 1 of hashmap:queue",
     "(mkChan Str.hashmap_bag_alias 0 2, hashmap_bag)",
     "(mkChan Str.hashmap_bag_alias 1 2, hashmap_bag)",
     ("alias_rows_facts",)),
    ("sf-p5-alias-other-urn", "SpaceFactory",
     "the alias row carries the URN of hashmap:queue, so it does not parse to hashmap:bag",
     "(mkChan Str.hashmap_bag_alias 0 2, hashmap_bag)",
     "(mkChan (urn_of queue_family) 0 2, hashmap_bag)",
     ("alias_rows_facts",)),
    ("sf-p5-pathmap-alias-other-op", "SpaceFactory",
     "the alias PathMapSpace names the op 0 of hashmap:bag",
     "(mkChan Str.pathmap_bag_alias 7 2, pathmap_bag)",
     "(mkChan Str.pathmap_bag_alias 0 2, pathmap_bag)",
     ("alias_rows_facts",)),
    ("sf-p5-alias-without-family", "SpaceFactory",
     "the alias is registered before its family is registered",
     "    filter (fun a => is_registered (snd a)) alias_rows.",
     "    alias_rows.",
     ("definitions_incl", "registered_families_complete", "aliases_bind_family_capability")),
    ("sf-p5-urn-map-not-functional", "SpaceFactory",
     "assertEqual takes the URN of getSpaceAgent, so one URN binds two ops",
     "mkChan StrP5.assert_equal 34 3",
     "mkChan StrP5.get_space_agent 34 3",
     ("all_definitions_functional",)),
    ("sf-p5-positional-accepted", "SpaceFactory",
     "positional params do not fail with PositionalParamsUnsupported",
     "        | _ :: _ => PErr EPositional",
     "        | _ :: _ => PErr EUnavailable",
     ("constructor_params_validated",)),
    ("sf-p5-unknown-key-accepted", "SpaceFactory",
     "the keys are not checked against the schema",
     "            match find (fun kv => negb (mem (fst kv) schema)) named with",
     "            match find (fun kv => false) named with",
     ("constructor_params_validated",)),
    ("sf-p5-suggestion-first", "SpaceFactory",
     "the suggestion is the first candidate within the bound, not the least (distance, key)",
     "        | true, Some b => if lex_ltb b (dist w c, c) then Some b else Some (dist w c, c)",
     "        | true, Some b => Some (dist w c, c)",
     ("least_spec",)),
    ("sf-p5-required-unchecked", "SpaceFactory",
     "a missing required key does not fail with MissingParam",
     "                | Some k => PErr (EMissing k)",
     "                | Some k => canonical_all named",
     ("constructor_params_validated",)),
    ("sf-p5-value-kept", "SpaceFactory",
     "a value that its rule refuses is stored as it is",
     "          | None => PErr (EInvalid k)",
     "          | None => match canonical_all rest with POk ps => POk ((k, v) :: ps) | PErr e => PErr e end",
     ("canonical_all_ok",)),
    ("sf-p5-cyclic-unruled", "SpaceFactory",
     "the schema key cyclic has no value rule",
     "  else if bytes_eqb k StrP5.cyclic then Some RCyclic",
     "  else if bytes_eqb k StrP5.cyclic then None",
     ("schema_keys_ruled",)),
    ("sf-p5-priorities-zero", "SpaceFactory",
     "priorities accepts 0",
     "  | RPriorities, VInt z => if (1 <=? z)%Z && (z <=? i64_max)%Z then Some (VInt z) else None",
     "  | RPriorities, VInt z => if (0 <=? z)%Z && (z <=? i64_max)%Z then Some (VInt z) else None",
     ("canonical_int_rule",)),
    ("sf-p5-priorities-on-queue", "SpaceFactory",
     "the HashMap, PathMap and Vector families that are no priority queue take priorities",
     "  | (HashMap | PathMap | Vector), (Bag | Queue | Stack | SetC | Cell) => Some ([], [])",
     "  | (HashMap | PathMap | Vector), (Bag | Queue | Stack | SetC | Cell) => Some ([StrP5.priorities], [])",
     ("schema_cases",)),
    ("sf-p5-priorities-required", "SpaceFactory",
     "the HashMap, PathMap and Vector priority queues require priorities",
     "  | (HashMap | PathMap | Vector), PriorityQueue => Some ([StrP5.priorities], [])",
     "  | (HashMap | PathMap | Vector), PriorityQueue => Some ([StrP5.priorities], [StrP5.priorities])",
     ("schema_cases",)),
    ("sf-p5-size-optional", "SpaceFactory",
     "an Array family does not require size",
     "  | Array, (Bag | Queue | Stack | SetC | Cell) => Some ([StrP5.cyclic; StrP5.size], [StrP5.size])",
     "  | Array, (Bag | Queue | Stack | SetC | Cell) => Some ([StrP5.cyclic; StrP5.size], [])",
     ("schema_cases",)),
    ("sf-p5-size-zero", "SpaceFactory",
     "size accepts 0",
     "  | RSize, VInt z => if (1 <=? z)%Z && (z <=? i64_max)%Z then Some (VInt z) else None",
     "  | RSize, VInt z => if (0 <=? z)%Z && (z <=? i64_max)%Z then Some (VInt z) else None",
     ("canonical_int_rule",)),
    ("sf-p5-cyclic-any-value", "SpaceFactory",
     "cyclic accepts a GInt",
     "  | RCyclic, VBool b => Some (VBool b)",
     "  | RCyclic, VBool b => Some (VBool b)\n  | RCyclic, VInt z => Some (VInt z)",
     ("canonical_cyclic_rule",)),
    ("sf-p5-vector-takes-size", "SpaceFactory",
     "a Vector family takes size",
     "  | (HashMap | PathMap | Vector), (Bag | Queue | Stack | SetC | Cell) => Some ([], [])",
     "  | (HashMap | PathMap), (Bag | Queue | Stack | SetC | Cell) => Some ([], [])\n  | Vector, (Bag | Queue | Stack | SetC | Cell) => Some ([StrP5.size], [])",
     ("schema_cases",)),
    # ── P6 step 8: CounterConflict.v, the soft bound of capacity (decision 71) ─────────
    ("cc-capped-store-at-cap", "CounterConflict",
     "a capped store passes on a collection that already holds cap records",
     "        (length (sd st k) <? cap) &&",
     "        (length (sd st k) <=? cap) &&",
     ("capped_pass_iff",)),
    ("cc-capacity-hard-claimed", "CounterConflict",
     "the claim that a merge of capped stores never exceeds the capacity",
     "          length l <= cap + Nat.min (length (writes_to k wsA)) (length (writes_to k wsB)))) /\\",
     "          length l <= cap)) /\\",
     ("capped_writes_soft_bound",)),
    # ── P6 step 8: SpaceFactory.v, the P6 rows and the VectorDB params ─────────────────
    ("sf-p6-register-unwired", "SpaceFactory",
     "a P6 row is registered without its dispatch arm",
     " ++ filter (fun r => wired (fst r)) p6_rows.",
     " ++ p6_rows.",
     ("registered_rows",)),
    ("sf-p6-define-unregistered", "SpaceFactory",
     "definitions installs the factory of every P6 row, wired or not",
     "    map (fun r => (snd r, Some (fst r))) (filter (fun r => wired (fst r)) p6_rows) ++",
     "    map (fun r => (snd r, Some (fst r))) p6_rows ++",
     ("definitions_incl", "registered_families_complete")),
    ("sf-p6-row-wrong-op", "SpaceFactory",
     "the vector:vectordb row takes the op 24 of vector:priorityqueue instead of its index 25",
     "row Vector VectorDb 25]",
     "row Vector VectorDb 24]",
     ("rows_facts",)),
    ("sf-p6-vectordb-on-pathmap", "SpaceFactory",
     "the pathmap:vectordb pair gets the VectorDB schema",
     "  | (HashMap | Vector), VectorDb => Some (vectordb_params, vectordb_required)",
     "  | (HashMap | PathMap | Vector), VectorDb => Some (vectordb_params, vectordb_required)",
     ("vectordb_schema",)),
    ("sf-p6-metric-required", "SpaceFactory",
     "the VectorDB families require metric",
     "Definition vectordb_required : list bytes := [StrP6.capacity; StrP6.dimensions].",
     "Definition vectordb_required : list bytes := [StrP6.capacity; StrP6.dimensions; StrP6.metric].",
     ("vectordb_schema",)),
    ("sf-p6-capacity-zero", "SpaceFactory",
     "capacity accepts 0",
     "  | RCapacity, VInt z => if (1 <=? z)%Z && (z <=? max_capacity)%Z then Some (VInt z) else None",
     "  | RCapacity, VInt z => if (0 <=? z)%Z && (z <=? max_capacity)%Z then Some (VInt z) else None",
     ("canonical_capacity_rule",)),
    ("sf-p6-capacity-bound", "SpaceFactory",
     "the capacity bound is one more than MAX_CAPACITY",
     "Definition max_capacity : Z := 1000000%Z.",
     "Definition max_capacity : Z := 1000001%Z.",
     ("vectordb_params_validated",)),
    ("sf-p6-dimensions-bound", "SpaceFactory",
     "the dimensions bound is one more than MAX_DIMENSIONS",
     "Definition max_dimensions : Z := 4096%Z.",
     "Definition max_dimensions : Z := 4097%Z.",
     ("vectordb_params_validated",)),
    ("sf-p6-metric-any-string", "SpaceFactory",
     "metric accepts every string",
     "  | RMetric, VStr s => if mem s metric_names then Some (VStr s) else None",
     "  | RMetric, VStr s => Some (VStr s)",
     ("canonical_metric_rule",)),
    ("sf-p6-mirror-any-string", "SpaceFactory",
     "mirror accepts every string",
     "  | RMirror, VStr s => if bytes_eqb s StrP6.chroma then Some (VStr s) else None",
     "  | RMirror, VStr s => Some (VStr s)",
     ("canonical_mirror_rule",)),
    ("sf-p6-gate9-any-metric", "SpaceFactory",
     "gate 9 accepts a mirror with every metric name",
     "      | QDefault, Some m => mem m indexed_metrics",
     "      | QDefault, Some m => mem m metric_names",
     ("check_mirror_spec",)),
    ("sf-p6-gate9-any-qualifier", "SpaceFactory",
     "gate 9 accepts a mirror with every qualifier",
     "      | QDefault, Some m => mem m indexed_metrics\n      | _, _ => false",
     "      | _, Some m => mem m indexed_metrics\n      | _, None => false",
     ("check_mirror_spec",)),
    ("sf-p6-gate9-no-default-metric", "SpaceFactory",
     "without the metric param gate 9 has no metric, so it refuses every mirror",
     "  | None => Some StrP6.cosine",
     "  | None => None",
     ("mirror_metric_default",)),
    ("sf-p6-indexed-manhattan", "SpaceFactory",
     "ChromaDB indexes manhattan too",
     "Definition indexed_metrics : list bytes := [StrP6.cosine; StrP6.dot; StrP6.euclidean].",
     "Definition indexed_metrics : list bytes := [StrP6.cosine; StrP6.dot; StrP6.euclidean; StrP6.manhattan].",
     ("vectordb_params_validated",)),
    # ── P5 W2: PathMapPrefix.v (batch PATHMAP, P5e) ─────────────────────────────────────
    ("pm-term-is-tag", "PathMapPrefix",
     "GBool(false) takes the byte 0x00 of the terminator as its tag (codec and parser)",
     ("  | TBool false => [1]", "          if t =? 1 then Some (TBool false, r)"),
     ("  | TBool false => [0]", "          if t =? 0 then Some (TBool false, r)"),
     ("ser_tag",)),
    ("pm-string-unprefixed", "PathMapPrefix",
     "a GString segment without the length of its bytes",
     "  | TStr s => 4 :: uv (length s) ++ s",
     "  | TStr s => 4 :: s",
     ("parse_ser_ser",)),
    ("pm-list-uncounted", "PathMapPrefix",
     "a list segment without its element count",
     "  | TList xs => 11 :: uv (length xs) ++ flat_map ser xs",
     "  | TList xs => 11 :: flat_map ser xs",
     ("parse_ser_ser",)),
    ("pm-tuple-takes-list-tag", "PathMapPrefix",
     "a tuple segment with the tag of a list",
     "  | TTuple xs => 12 :: uv (length xs) ++ flat_map ser xs",
     "  | TTuple xs => 11 :: uv (length xs) ++ flat_map ser xs",
     ("parse_ser_ser",)),
    ("pm-zigzag-not-injective", "PathMapPrefix",
     "zigzag maps -p and p to one number",
     "  | Zneg p => 2 * Pos.to_nat p - 1",
     "  | Zneg p => 2 * Pos.to_nat p",
     ("unzigzag_zigzag",)),
    ("pm-escape-unprefixed", "PathMapPrefix",
     "the escape arm without the length of its payload",
     "  if structural t then ser t else 15 :: uv (length (ser t)) ++ ser t.",
     "  if structural t then ser t else 15 :: ser t.",
     ("parse_seg_seg",)),
    ("pm-escape-takes-list-tag", "PathMapPrefix",
     "the escape arm takes the tag of a list",
     "  if structural t then ser t else 15 :: uv (length (ser t)) ++ ser t.",
     "  if structural t then ser t else 11 :: uv (length (ser t)) ++ ser t.",
     ("parse_seg_seg",)),
    ("pm-terminator-above-tags", "PathMapPrefix",
     "the terminator 0xFF sorts after every tag, so an extension precedes its path",
     "Definition path_key (p : path) : bytes := prefix_key p ++ [0].",
     "Definition path_key (p : path) : bytes := prefix_key p ++ [255].",
     ("exact_path_first",)),
    ("pm-reroot-normalizes-root", "PathMapPrefix",
     "R(eps, m) rewrites a PathMap in canonical form instead of delivering it as it is",
     "  | [] => v\n  | _ :: _ =>\n      match v with",
     "  | [] => match v with PMap es => PMap (head_encode (head_decode es)) | POther x => POther x end\n  | _ :: _ =>\n      match v with",
     ("reroot_ident",)),
    ("pm-canon-loses-list-entry", "PathMapPrefix",
     "a path of one plain list is written bare, so its entry splits into the list elements",
     "  | [TList xs] => TList [TList xs]\n",
     "",
     ("elems_canon",)),
    ("pm-rl-appends", "PathMapPrefix",
     "re-rooting appends the suffix after each path instead of prepending it",
     "  | _ => map (fun p => s ++ p) ps",
     "  | _ => map (fun p => p ++ s) ps",
     ("rl_sorted",)),
    ("pm-decode-unsorted", "PathMapPrefix",
     "the decode keeps the order of the entries of the representation",
     "Definition head_decode (es : list term) : list path := sort_dedup (map elems es).",
     "Definition head_decode (es : list term) : list path := map elems es.",
     ("head_decode_sorted",)),
    ("pm-other-payload-first", "PathMapPrefix",
     "a value that is not a PathMap comes before the suffix in its delivery",
     "      | POther x => POther (TList (s ++ [x]))",
     "      | POther x => POther (TList (x :: s))",
     ("reroot_compose",)),
    ("pm-decode-keeps-repeats", "PathMapPrefix",
     "the decode keeps every entry of a repeated key",
     "      | Eq => q :: t\n      | Gt => q :: insert_path p t",
     "      | Eq => p :: q :: t\n      | Gt => q :: insert_path p t",
     ("insert_sorted",)),
    ("pm-unit-rule-dropped", "PathMapPrefix",
     "the empty map stays empty under a suffix (no FIPS unit rule)",
     "  | [] => [s]\n  | _ => map",
     "  | [] => []\n  | _ => map",
     ("rl_unit_class",)),
    ("pm-other-payload-dropped", "PathMapPrefix",
     "a value that is not a PathMap is dropped from its delivery",
     "      | POther x => POther (TList (s ++ [x]))",
     "      | POther x => POther (TList s)",
     ("reroot_injective",)),
    ("pm-candidate-ignores-rank", "PathMapPrefix",
     "two candidates of one path are not ordered by their discipline rank",
     "  | Eq => rlt (snd a) (snd b)",
     "  | Eq => False",
     ("candidate_order_total",)),
    # ── P5 W3: PathMapPrefix.v, the PathMap engine (batch PATHMAP-ENGINE, Phase 1) ──────────
    ('pme-marker-tag-unchecked', 'PathMapPrefix', 'the marker decoder accepts any tag, so it decodes a record that is not a marker', '      if bytes_eqb t marker_tag && (length src =? source_len) then', '      if (length src =? source_len) then', ('marker_part_inv', 'marker_record_roundtrip')),
    ('pme-marker-source-unchecked', 'PathMapPrefix', 'the marker decoder accepts a source of any length', '      if bytes_eqb t marker_tag && (length src =? source_len) then', '      if bytes_eqb t marker_tag then', ('marker_part_inv', 'marker_record_roundtrip')),
    ('pme-marker-tag-datum-kind', 'PathMapPrefix', 'the marker tag takes the kind of a datum record', 'Definition marker_tag : bytes := record_tag kind_index.', 'Definition marker_tag : bytes := record_tag kind_datum.', ('marker_record_roundtrip',)),
    ('pme-envelope-drops-source', 'PathMapPrefix', 'the marker envelope drops the source hash of its datum', 'Definition envelope (src : bytes) : rpar := RList [RBytes marker_tag; RBytes src].', 'Definition envelope (src : bytes) : rpar := RList [RBytes marker_tag; RBytes []].', ('marker_part_inv', 'marker_record_roundtrip')),
    ('pme-vehicle-drops-payload', 'PathMapPrefix', 'the vehicle is the short marker (decision 40 option a), so it does not carry the payload', '  marker_pars src s ++ payload.', '  marker_pars src s.', ('vehicle_carries_payload',)),
    ('pme-vehicle-payload-first', 'PathMapPrefix', 'the vehicle puts the payload before the marker pars', '  marker_pars src s ++ payload.', '  payload ++ marker_pars src s.', ('vehicle_carries_payload',)),
    ('pme-depth-cap-off-by-one', 'PathMapPrefix', 'a path of 65 elements passes the depth check', '  if depth_cap <? length p then Some PathTooDeep', '  if S depth_cap <? length p then Some PathTooDeep', ('path_element_rules',)),
    ('pme-map-element-accepted', 'PathMapPrefix', 'a PathMap is a path element', '  | PMapElem => true', '  | PMapElem => false', ('path_element_rules',)),
    ('pme-nested-zipper-accepted', 'PathMapPrefix', 'the element scan looks only at the first element inside a composite element', '  | PNest xs => existsb holds_map xs', '  | PNest xs => match xs with x :: _ => holds_map x | [] => false end', ('path_element_rules',)),
    ('pme-vehicle-stored-on-data', 'PathMapPrefix', 'a produce-side delivery also stores the vehicle on a data channel', '    [EvProduce (index_chan q); EvComm [index_chan q] [index_chan q]].', '    [EvProduce (index_chan q); EvComm [index_chan q] [index_chan q]; EvProduce (data_chan q)].', ('delivery_vehicle_single_channel',)),
    ('pme-retire-joins-two-channels', 'PathMapPrefix', 'a K_retire consumes on a group of two channels (a synthesized join, I-16)', '  Definition retire_events (c : chan) : list ev := [EvConsume [c]; EvComm [c] [c]].', '  Definition retire_events (c : chan) : list ev := [EvConsume [c; c]; EvComm [c; c] [c]].', ('retire_events_single', 'delivery_vehicle_single_channel')),
    ('pme-delivery-takes-data-record', 'PathMapPrefix', 'a consume-side delivery COMM on I(q) takes the datum record of D(p) instead of the marker', '    [EvConsume [index_chan q]; EvComm [index_chan q] [index_chan q]].', '    [EvConsume [index_chan q]; EvComm [index_chan q] [data_chan q]].', ('delivery_vehicle_single_channel',)),
    ('pme-space-wide-markers', 'PathMapPrefix', 'every marker lives on the space-wide index I(eps), as a space-wide index stores it', '    mkM (firstn k (d_path d)) (d_id d) (skipn k (d_path d)).', '    mkM [] (d_id d) (d_path d).', ('prefix_contact_anchored',)),
    ('pme-store-marks-exact-path-only', 'PathMapPrefix', 'a store produces a marker on the index channel of its own path only', '    EvProduce (data_chan p) :: map (fun q => EvProduce (index_chan q)) (prefixes p).', '    EvProduce (data_chan p) :: map (fun q => EvProduce (index_chan q)) [p].', ('prefix_contact_anchored',)),
    ('pme-one-index-channel', 'PathMapPrefix', 'every prefix shares one index channel', '  Definition index_chan (q : path) : chan := ChI (prefix_key q).', '  Definition index_chan (q : path) : chan := ChI [].', ('prefix_contact_anchored',)),
    ('pme-peek-restore-skipped', 'PathMapPrefix', 'the second send that meets no waiter does not store the marker of the COMM again', '    mkS (data st) (marker_at d k :: marks st) (wtrs st) (token st).', '    mkS (data st) (marks st) (wtrs st) (token st).', ('marker_effects_keep_mi', 'peek_restores_delivered_marker')),
    ('pme-peek-restores-root-marker', 'PathMapPrefix', 'the second send restores the marker of the root prefix instead of the marker of its COMM', '    mkS (data st) (marker_at d k :: marks st) (wtrs st) (token st).', '    mkS (data st) (marker_at d 0 :: marks st) (wtrs st) (token st).', ('marker_effects_keep_mi', 'peek_restores_delivered_marker')),
    ('pme-second-send-least-specific-first', 'PathMapPrefix', 'the second send meets the prefixes from the root, the least specific first', '  Definition send_prefixes (p : path) : list path := rev (prefixes p).', '  Definition send_prefixes (p : path) : list path := prefixes p.', ('send_prefixes_depths', 'peek_restores_delivered_marker')),
    ('pme-offer-exact-path-only', 'PathMapPrefix', 'a send meets the waiters of its exact path only, not of its prefixes', '    flat_map (fun q => worder (waiters_on ws q)) (send_prefixes p).', '    worder (waiters_on ws p).', ('offer_list_complete', 'no_match_remains')),
    ('pme-candidates-exact-path-only', 'PathMapPrefix', 'a receive on q sees the datums of the exact path q only', '    dorder (filter (fun d => prefixb q (d_path d)) ds).', '    dorder (filter (fun d => path_eqb q (d_path d)) ds).', ('candidates_complete', 'no_match_remains')),
    ('pme-receive-head-blocks', 'PathMapPrefix', 'a receive of a scan shape stops at the first candidate that does not match', '    | d :: rest => if spatial_ok w d then Some (d, rest) else first_spatial w rest', '    | d :: rest => if spatial_ok w d then Some (d, rest) else match first_spatial w rest with Some _ => None | None => None end', ('first_spatial_none', 'receive_loop_no_match', 'no_match_remains')),
    ('pme-offer-stops-at-refusal', 'PathMapPrefix', 'a send stops at the first waiter that refuses it', '    | w :: rest => if fires w d then Some (w, rest) else first_fire d rest', '    | w :: rest => if fires w d then Some (w, rest) else match first_fire d rest with Some _ => None | None => None end', ('first_fire_none', 'offer_loop_no_match', 'no_match_remains')),
    ('pme-persistent-pair-continues', 'PathMapPrefix', 'a datum that survives a persistent waiter is offered on and can be stored next to it', '            else if w_pers w then (st1, Reissue)', '            else if false then (st1, Reissue)', ('offer_loop_no_match', 'no_match_remains')),
    ('pme-retire-skips-root', 'PathMapPrefix', 'the retire plan leaves the marker of the root prefix', '    filter (fun j => match delivered with Some k => negb (Nat.eqb j k) | None => true end)', '    filter (fun j => match delivered with Some k => negb (Nat.eqb j k) && negb (Nat.eqb j 0) | None => true end)', ('retire_plan_partition', 'marker_effects_keep_mi')),
    ('pme-store-skips-root-marker', 'PathMapPrefix', 'a store adds no marker on the root prefix', '    map (marker_at d) (seq 0 (S (length (d_path d)))).', '    map (marker_at d) (seq 1 (length (d_path d))).', ('markers_of_split', 'marker_effects_keep_mi', 'prefix_contact_anchored')),
    ('pme-delivery-keeps-marker', 'PathMapPrefix', 'a consume-side delivery leaves the marker that its COMM delivered', '             (remove1 (marker_at d k) (marks st)))', '             (marks st))', ('marker_effects_keep_mi',)),
    ('pme-spend-keeps-datum', 'PathMapPrefix', 'a fire of a second send retires the markers and keeps the datum record', '        mkS (remove_nth n (data st))\n          (remove_all (map (marker_at d) (retired_depths (length (d_path d)) (Some k))) (marks st))', '        mkS (data st)\n          (remove_all (map (marker_at d) (retired_depths (length (d_path d)) (Some k))) (marks st))', ('marker_effects_keep_mi',)),
    ('pme-dirty-exact-path-only', 'PathMapPrefix', 'a change on D(p) dirties the prefix p only, not its shorter prefixes', '    | AddDatum d => prefixes (d_path d)', '    | AddDatum d => [d_path d]', ('clean_change', 'heads_no_match')),
    ('pme-park-not-dirty', 'PathMapPrefix', 'a park dirties no prefix', '    | AddWaiter w => [w_pre w]', '    | AddWaiter w => []', ('clean_change', 'heads_no_match')),
    ('pme-next-fire-stops-at-refusal', 'PathMapPrefix', 'NEXT-FIRE stops at the first dirty prefix whose head refuses', '            | Some (d, _) => if guard_ok w d then Some (w, d) else next_fire st rest', '            | Some (d, _) => if guard_ok w d then Some (w, d) else None', ('next_fire_none', 'cascade_quiet', 'heads_no_match')),
    ('pme-peek-fire-keeps-waiter', 'PathMapPrefix', 'a cascade fire of a peek waiter keeps the waiter, so the cascade does not lower its measure', '    (if w_pers w then [] else [DelWaiter (w_id w)])', '    (if w_pers w || w_peek w then [] else [DelWaiter (w_id w)])', ('fire_lowers_measure', 'cascade_terminates', 'heads_no_match')),
    ('pme-bump-repeats', 'PathMapPrefix', 'a bump does not advance the token, so two operations share an order', 'Definition bump (t : nat) : nat * nat := (t, S t).', 'Definition bump (t : nat) : nat * nat := (t, t).', ('token_ord_reentry',)),
    ('pme-reissue-fresh-order', 'PathMapPrefix', 'a re-issued persistent operation takes a fresh order (no decision 43)', '  | Some o => if o <? bumped then Some o else None', '  | Some o => Some bumped', ('token_ord_reentry',)),
    ('pme-reentry-at-tail', 'PathMapPrefix', 'a re-entered element of a queue is at the head only when it is younger than the head', '  match least os with None => true | Some m => o <? m end.', '  match least os with None => true | Some m => m <? o end.', ('token_ord_reentry',)),
    ('pme-scan-reissue-nonzero', 'PathMapPrefix', 'a bag or a set refuses its own re-issued order 0', '  | Some 0 => Some 0', '  | Some 0 => None', ('token_ord_reentry',)),
    ('pme-trace-reads-first', 'PathMapPrefix', 'a step of the prefix scan reads before it reserves', 'Definition mstep (units : nat) : list tev := [TReserve units; TRead units].', 'Definition mstep (units : nat) : list tev := [TRead units; TReserve units].', ('mstep_covered', 'prefix_scan_trace_covered')),
    ('pme-outcome-not-prepaid', 'PathMapPrefix', 'the outcome after the commit reads what the scan did not reserve', '  ++ [TReserve (ps_outcome s)] ++ [TRead (ps_outcome s)].', '  ++ [TRead (ps_outcome s)].', ('prefix_scan_trace_covered',)),
    # ── P5 W3: JoinCascade.v, RecordEnvelope.v and CounterConflict.v, the joins of one space (batch JOINS-FORMAL) ──
    ('jc-incoming-every-position', 'JoinCascade', 'the produce search forces the incoming record at every position of its collection where it matches (decision 28 reverted)', '    if k =? p then spatial (b_pat b) (r_val x) else None.', '    if b_coll b =? r_coll x then spatial (b_pat b) (r_val x) else None.', ('join_incoming_binds_once',)),
    ('jc-claims-ignored', 'JoinCascade', 'the search ignores its claim set, so one record (or one class) can bind two binds', '        if claimed C e then try_cands g p C k more else', '        if false then try_cands g p C k more else', ('join_repeated_channel_distinct_records',)),
    ('jc-backtracking', 'JoinCascade', 'the search backtracks at a dead end instead of parking (decision 37 reverted before P12)', '  Definition on_dead_end {X : Type} (retry : option X) : option X := None.', '  Definition on_dead_end {X : Type} (retry : option X) : option X := retry.', ('greedy_join_can_miss_assignment',)),
    ('jc-guard-per-bind', 'JoinCascade', 'the search checks the guard on each bind instead of once over all binds (decision 37)', '  Definition pick_match (g : Gd) (p : P) (e : rec) : option M := spatial p (r_val e).', '  Definition pick_match (g : Gd) (p : P) (e : rec) : option M :=\n    match spatial p (r_val e) with Some m => if admits g [m] then Some m else None | None => None end.', ('join_search_is_head_greedy',)),
    ('jc-incoming-skipped', 'JoinCascade', 'the produce search tries no position for the incoming record, so it binds stored records only', '  Definition incoming_base (W : wtr) : list (nat * (nat * bnd)) := enum 0 (gpairs W).', '  Definition incoming_base (W : wtr) : list (nat * (nat * bnd)) := [].', ('join_produce_search_agrees_with_head',)),
    ('jc-inverse-permutation', 'JoinCascade', 'the outcome reads results[slot_i.bind] into position i (the inverse permutation)', '    map (fun b => assoc b (combine bs rs)) (seq 0 (length bs)).', '    map (fun i => assoc (nth i bs 0) (combine (seq 0 (length bs)) rs)) (seq 0 (length bs)).', ('bind_permutation_restores_order',)),
    ('jc-set-no-cascade', 'JoinCascade', 'a joined set collection does not cascade (decision 50 reverted), so a class that moves leaves a fireable waiter', '  Definition cascades : bool := match sh with Head | Lanes | Classes => true | Scan => false end.', '  Definition cascades : bool := match sh with Head | Lanes => true | Scan | Classes => false end.', ('cascade_restores_inv',)),
    ('jc-inflight-np-only', 'JoinCascade', 'a peek fire sends the element in flight again only when it is not persistent (decision 30 and DV-3 reverted)', '    if is_inc inc (fst krm) then inflight else negb (r_pers (fst (snd krm))).', '    if is_inc inc (fst krm) then inflight && negb (r_pers (fst (snd krm))) else negb (r_pers (fst (snd krm))).', ('peek_join_restores_stored_candidates',)),
    ('jc-commit-own-pending', 'JoinCascade', 'COMMIT keeps the element of the operation in the pending second sends of the settle job (A1 reverted)', '  Definition pending_work (j : jst) : list pend := filter (fun q => negb (p_own q)) (j_work j).', '  Definition pending_work (j : jst) : list pend := j_work j.', ('peek_join_restores_stored_candidates',)),
    ('jc-commit-empty-settle', 'JoinCascade', 'a stop that owes nothing and resumes nothing commits an empty settle job (DV-5 reverted)', '        if owes j\n', '        if owes j || negb (is_some (resume_of o v0 j))\n', ('peek_join_restores_stored_candidates',)),
    ('jc-two-lane-heads', 'JoinCascade', 'a single-head discipline keeps a lane for each bind, so a collection has two lane heads', '  Definition lane_of (b : bnd) : option nat := match sh with Lanes => b_lane b | _ => None end.', '  Definition lane_of (b : bnd) : option nat := b_lane b.', ('cascade_order_independent',)),
    ('jc-lift-persistent', 'JoinCascade', 'the cascade lifts a persistent record too, so the re-offer loses it', '  Definition lift_ops (r : rec) : list staged := if r_pers r then [] else [SLift r].', '  Definition lift_ops (r : rec) : list staged := [SLift r].', ('lift_reoffer_equals_direct_comm',)),
    ('jc-reoffer-first-position', 'JoinCascade', 'the cascade re-offers the record of position 0 although another bound record is not persistent', '  Definition reoffer_pos (sg : list (rec * M)) : nat := match first_np 0 sg with Some k => k | None => 0 end.', '  Definition reoffer_pos (sg : list (rec * M)) : nat := 0.', ('lift_reoffer_equals_direct_comm',)),
    ('jc-retire-incoming-class-only', 'JoinCascade', 'RETIRE-SET-BINDS retires only the class of the incoming record', '  Definition retire_positions (inc : option nat) (n : nat) : list nat := seq 0 n.', '  Definition retire_positions (inc : option nat) (n : nat) : list nat :=\n    match inc with Some p => [p] | None => [] end.', ('join_set_bind_retires_class',)),
    ('jc-head-on-incoming-only', 'JoinCascade', 'the produce side checks the head condition on the collection of the incoming record only', '  Definition heads_required (W : wtr) (x : rec) : list nat := colls W.', '  Definition heads_required (W : wtr) (x : rec) : list nat := [r_coll x].', ('join_fires_heads_only',)),
    ('jc-bump-own-only', 'JoinCascade', "TOUCH bumps only the operation's own collections, so the cascade stages on unbumped counters", '  Definition touch_scope (own : list nat) (c : nat) : bool := true.', '  Definition touch_scope (own : list nat) (c : nat) : bool := mem c own.', ('cascade_bumps_touched_counters',)),
    ('jc-reissue-fresh-ords', 'JoinCascade', 'a re-issued persistent JoinOp carries no first-round ords (decision 43 reverted)', '  Definition carried_ords (pers : bool) (os : list nat) : option (list nat) := if pers then Some os else None.', '  Definition carried_ords (pers : bool) (os : list nat) : option (list nat) := None.', ('join_reissue_keeps_bind_ords',)),
    ('jc-peek-persistent-waiter', 'JoinCascade', 'a receive may peek and persist, so a peek fire keeps its waiter', '  Definition wf_waiter (W : wtr) : bool := negb (w_pers W && w_peek W).', '  Definition wf_waiter (W : wtr) : bool := true.', ('cascade_terminates',)),
    ('jc-per-phase-count', 'JoinCascade', 'each phase of a transaction starts its own fire count at 0 (decision 49 reverted)', '  Definition phase_count (f : nat) : nat := f.', '  Definition phase_count (f : nat) : nat := 0.', ('join_txn_fires_bounded',)),
    ('jc-second-send-plain-store', 'JoinCascade', 'a popped second send is stored plainly instead of offered (Rule E in joins reverted)', '    offer (S (length (v_wtrs (j_view j)))) own j (resend_elem opauth q).', '    store own j (resend_elem opauth q).', ('peek_join_restores_stored_candidates',)),
    ('jc-second-sends-reversed', 'JoinCascade', 'the second sends are pushed from the first result up, so they pop in reversed receipt order', '    with_work (fold_left (fun w q => q :: w) (rev l) (j_work j)) j.', '    with_work (fold_left (fun w q => q :: w) l (j_work j)) j.', ('peek_join_restores_stored_candidates',)),
    ('jc-closure-one-level', 'JoinCascade', 'the join closure stops one level deep, so a collection reached through a second join is not locked', '  Definition closure_depth (v : view) (seeds : list nat) : nat := length (universe v seeds).', '  Definition closure_depth (v : view) (seeds : list nat) : nat := 1.', ('plan_then_lock_stable',)),
    ('jc-cascade-incoming-only', 'JoinCascade', 'a fire makes only its incoming collection dirty, so the cascade misses a waiter that the fire exposes on another collection of the group', '  Definition fire_dirty (W : wtr) (c_in : nat) : list nat := colls W.', '  Definition fire_dirty (W : wtr) (c_in : nat) : list nat := [c_in].', ('cascade_restores_inv',)),
    ('re-v2-slot-without-bind', 'RecordEnvelope', 'a slot of the version 2 join record has no bind index', '  list_par [int_par (s_ord s); int_par (Z.of_nat (s_bind s)); pq_policy (s_priority s)].', '  list_par [int_par (s_ord s); pq_policy (s_priority s)].', ('continuation_v2_roundtrip',)),
    ('re-v2-tag-version-1', 'RecordEnvelope', 'the tag of the version 2 join record carries the version 1', 'Definition join_continuation_version : nat := 2.', 'Definition join_continuation_version : nat := 1.', ('continuation_v2_roundtrip',)),
    ('cc-join-bumps-first-only', 'CounterConflict', 'a join step bumps only the first collection of its group', '  Definition join_bumps (ks : list Coll) : list Coll := nodup coll_eq_dec ks.', '  Definition join_bumps (ks : list Coll) : list Coll := match ks with [] => [] | k :: _ => [k] end.', ('join_ops_race_on_every_counter',)),
    # ── P5 W3: CounterConflict.v, the merges of set collections (F24, F25; main session) ──
    ('cc-set-retire-peeks', 'CounterConflict', 'a retirement of a set member is a peek COMM, so its produce is filed as peeked and check #1 does not see the race of two retirements (F24)', '    exists kont ps, In (EvComm kont ps false) log /\\ In p ps.\n', '    exists kont ps, In (EvComm kont ps true) log /\\ In p ps.\n', ('retires_consumed',)),
    ('cc-set-view-list-order', 'CounterConflict', 'the view of a set depends on the list order of its records, not on the canonical order of DR-75 (decision 41 reverted), so the merged view need not be a serial view (F25)', "      (forall l l', Permutation l l' -> set_view l = set_view l') ->\n", "      (forall l l', l = l' -> set_view l = set_view l') ->\n", ('unordered_set_writes_commute',)),
    # ── P5 W3 Phase 2: CounterConflict.v, the PathMap token race (batch PATHMAP-ENGINE, headline 15) ──
    ("cc-pathmap-write-skips-token", "CounterConflict",
     "a bag or a set step that changed the state stages no token bump, so a PathMap write does "
     "not consume the token record and two writing branches need not race on S(sid)",
     "    | false, true =>\n"
     "        let b := token_bump tag st s in (fst b, pmap_events tag st s j ++ snd b)\n",
     "    | false, true => (st, pmap_events tag st s j)\n",
     ("exec_pmap_consumes_token",)),
    ("cc-pathmap-store-not-a-change", "CounterConflict",
     "a store that is not a restore does not count as a change of the state (decision 51 "
     "reverted for stores), so a bag or a set branch that only stores bumps no token",
     "    | PStore | PPark | PSpendDatum | PSpendWaiter | PRetireClass => true\n"
     "    | PRestore | PRead => false\n",
     "    | PPark | PSpendDatum | PSpendWaiter | PRetireClass => true\n"
     "    | PStore | PRestore | PRead => false\n",
     ("store_changes",)),
    # ── P5 W2: DataCollection.v, ContinuationCollection.v (batch BAG, P5a) ──────────────
    ("dc-order-by-list-position", "DataCollection",
     "the driver visits the records in list order, not in the canonical order of DR-75 (decision 41)",
     "Definition visit (l : list nat) : list nat := canon l.",
     "Definition visit (l : list nat) : list nat := l.",
     ("bag_visit_order_canonical",)),
    ("dc-guard-on-every-candidate", "DataCollection",
     "a receive checks the guard on every spatial match and takes the first that passes, not once on the first spatial match (decision 37)",
     "    match first_match p l with\n    | Some k => if admits g (val k) then Some k else None\n    | None => None\n    end.",
     "    find (fun k => spatial p (val k) && admits g (val k)) (visit l).",
     ("bag_consume_first_spatial_match",)),
    ("dc-fire-removes-persistent", "DataCollection",
     "a fire removes the record that it binds also when the record is persistent",
     "    | FireSt _ k _ => if persistent k then rs else remove_one k rs",
     "    | FireSt _ k _ => remove_one k rs",
     ("bag_persistent_match_not_removed",)),
    ("dc-fire-removes-every-equal-record", "DataCollection",
     "a fire removes every record equal to the record that it binds (the legacy dedup), not one record",
     "    | FireSt _ k _ => if persistent k then rs else remove_one k rs",
     "    | FireSt _ k _ => if persistent k then rs else filter (fun h => negb (h =? k)) rs",
     ("bag_multiplicity_preserved",)),
    ("dc-store-adds-nothing", "DataCollection",
     "a store does not add its record",
     "    | Store k _ => k :: rs",
     "    | Store k _ => rs",
     ("bag_multiplicity_preserved",)),
    ("dc-peek-without-restore", "DataCollection",
     "a peek fire removes the record and the operation never sends it again (decision 45, Rule E, reverted)",
     "        else let (ops, e) := offer ws k (rauth k) rounds in (FireSt c k a_op :: ops, e)",
     "        else ([FireSt c k a_op], Settled)",
     ("bag_peek_restore_is_host_second_send",)),
    ("dc-restore-with-operation-authority", "DataCollection",
     "the second send of a peeked record registers the authority of the peek, not the authority of the record (produce_peeks)",
     "offer ws k (rauth k) rounds",
     "offer ws k a_op rounds",
     ("bag_peek_restore_is_host_second_send",)),
    ("dc-restore-without-reoffer", "DataCollection",
     "the peeked record is stored again without an offer to the waiters, so a guarded waiter that admits it does not fire",
     "offer ws k (rauth k) rounds",
     "([Store k (rauth k)], Settled)",
     ("bag_peek_restore_is_host_second_send",)),
    ("dc-decode-reads-before-reserve", "DataCollection",
     "the payload decode of a tested record reads before it reserves (decision 42)",
     "Definition decode (units : nat) : list event := step units.",
     "Definition decode (units : nat) : list event := [Read units; Reserve units].",
     ("scan_trace_covered",)),
    ("dc-outcome-reserved-after-commit", "DataCollection",
     "the outcome is read after the commit before it is reserved (no prepaid outcome)",
     "  ++ [Reserve (outcome s)] ++ [Read (outcome s)].",
     "  ++ [Read (outcome s)] ++ [Reserve (outcome s)].",
     ("scan_trace_covered",)),
    ("cc-removal-by-pattern-equality", "ContinuationCollection",
     "a fire removes the first waiter whose pattern equals the pattern of the fired waiter (the legacy rule), not the fired waiter itself (D10)",
     "  Definition fire_remove (w : nat) (ws : list nat) : list nat := remove_one w ws.",
     "  Definition fire_remove (w : nat) (ws : list nat) : list nat := remove_by_pattern w ws.",
     ("waiter_removal_by_record_identity",)),
    ("cc-legacy-removes-by-identity", "ContinuationCollection",
     "the legacy rule removes by identity, so the counterexample of F13 disappears",
     "    | h :: t => if same_pattern (pattern h) (pattern w) then t else h :: remove_by_pattern w t",
     "    | h :: t => if h =? w then t else h :: remove_by_pattern w t",
     ("pattern_equality_removal_can_remove_another_waiter",)),
    ("cc-park-always-adds", "ContinuationCollection",
     "a park adds its waiter also when a byte-identical waiter is parked (F14)",
     "    if existsb (Nat.eqb w) ws then ws else w :: ws.",
     "    w :: ws.",
     ("park_byte_identity_idempotent",)),
    ("cc-order-by-list-position", "ContinuationCollection",
     "the driver visits the waiters in list order, not in the canonical order of DR-75 (decision 41)",
     "Definition visit (l : list nat) : list nat := canon l.",
     "Definition visit (l : list nat) : list nat := l.",
     ("waiter_visit_order_canonical",)),
    ("cc-produce-guard-stops-at-first-match", "ContinuationCollection",
     "a send checks the guard once on the first waiter whose pattern binds it and stops on a refusal, where a refusal must move to the next waiter (decision 37, produce side)",
     "  Definition offer_choice (x : V) (ws : list nat) : option nat := find (fits x) (visit ws).",
     "  Definition offer_choice (x : V) (ws : list nat) : option nat :=\n    match find (fun w => spatial (pattern w) x) (visit ws) with\n    | Some w => if admits (guard w) x then Some w else None\n    | None => None\n    end.",
     ("waiter_visit_order_canonical",)),
    ("cc-head-offers-two", "ContinuationCollection",
     "a head-only side offers its two least waiters (decision 11)",
     "  | Head => firstn 1 (visit ws)",
     "  | Head => firstn 2 (visit ws)",
     ("head_waiter_unique",)),
    # ── P5 W2: Match.v, GenericRSpace.v, AgentReadCoverage.v (batch ENGINE-FORMAL) ──────
    ("mt-receive-skips-guard", "Match",
     "F17: the receive plan fires its first spatial match without the injected guard",
     "    | Some (d, m) => if admits mt g m then RFire d m else RPark\n    | None => RPark\n    end.\n\n  (* A record matches spatially. *)",
     "    | Some (d, m) => RFire d m\n    | None => RPark\n    end.\n\n  (* A record matches spatially. *)",
     ("scan_receive_host",)),
    ("mt-hardwired-park", "Match",
     "F17: a receive plan with a hard-wired matcher that never fires",
     "  Definition scan_receive (mt : matcher) (p : P) (g : G) (l : list D) : receive_choice :=\n    match first_spatial mt p l with\n    | Some (d, m) => if admits mt g m then RFire d m else RPark\n    | None => RPark\n    end.",
     "  Definition scan_receive (mt : matcher) (p : P) (g : G) (l : list D) : receive_choice :=\n    RPark.",
     ("scan_receive_host",)),
    ("mt-guard-every-candidate", "Match",
     "F18: the guard is checked on every candidate (the F1r3lang rule of P12) instead of once on the first spatial match",
     "  Definition scan_receive (mt : matcher) (p : P) (g : G) (l : list D) : receive_choice :=\n    match first_spatial mt p l with\n    | Some (d, m) => if admits mt g m then RFire d m else RPark\n    | None => RPark\n    end.",
     "  Definition scan_receive (mt : matcher) (p : P) (g : G) (l : list D) : receive_choice :=\n    guard_filter_receive mt p g l.",
     ("scan_receive_host",)),
    ("mt-offer-hardwired-store", "Match",
     "F17: a produce plan with a hard-wired matcher that never fires",
     "  Definition scan_offer (mt : matcher) (v : V) (ws : list W) : offer_choice :=\n    scan_offer_loop (S (length ws)) mt v [] ws.",
     "  Definition scan_offer (mt : matcher) (v : V) (ws : list W) : offer_choice :=\n    OStore.",
     ("scan_offer_host",)),
    ("mt-refusal-stores", "Match",
     "F18: a guard refusal on the produce side stops the scan, so the datum is stored instead of moving to the next waiter",
     "            else scan_offer_loop fuel' mt v (w_id w :: refused) ws",
     "            else scan_offer_loop fuel' mt v (w_id w :: refused) []",
     ("scan_offer_loop_host",)),
    ("mt-head-ignores-guard", "Match",
     "F18: the head driver fires its head on a spatial match without the guard",
     "    match matches mt (w_pat head) (w_guard head) v with",
     "    match spatial mt (w_pat head) v with",
     ("scan_offer_one",)),
    ("mt-wildcard-is-real", "Match",
     "F19: the legacy WildcardMatch replaced by the real matcher, so a pattern for 42 never receives 43",
     "  mkMatcher (fun (_ : int_pattern) (v : nat) => Some v) no_guard.",
     "  mkMatcher int_spatial no_guard.",
     ("wildcard_first_spatial", "wildcard_matcher_degenerate")),
    ("gr-receive-skips-guard", "GenericRSpace",
     "F20: RECEIVE-SCAN fires its first spatial match without checking the guard",
     "            if negb (admits (w_guard k) m) then ([SPark k], None)",
     "            if false then ([SPark k], None)",
     ("receive_loop_trace",)),
    ("gr-offer-ignores-guard", "GenericRSpace",
     "F21: OFFER-SCAN fires the first waiter that matches spatially, whatever its guard says",
     "        | Some m => if admits (w_guard w) m then Some (w, m) else first_admitting v rest",
     "        | Some m => Some (w, m)",
     ("first_admitting_some",)),
    ("gr-cell-offer-ignores-guard", "GenericRSpace",
     "F21: the head driver of the cell fires its waiter without checking the guard",
     "                if negb (admits (w_guard w) m) then store",
     "                if false then store",
     ("cell_peek_resends", "cell_offer_loop_ok")),
    ("gr-offer-stops-at-refusal", "GenericRSpace",
     "F22 (decision 37, produce side): a guard refusal stops the scan, so the datum is stored while a later waiter without a guard matches it",
     "        | Some m => if admits (w_guard w) m then Some (w, m) else first_admitting v rest",
     "        | Some m => if admits (w_guard w) m then Some (w, m) else None",
     ("first_admitting_none",)),
    ("gr-receive-parks-on-first-miss", "GenericRSpace",
     "F22: RECEIVE-SCAN tests only the first record, as a head, so it parks while a later record matches a receive without a guard",
     "        | None => first_spatial p rest",
     "        | None => match first_spatial p rest with Some _ => None | None => None end",
     ("first_spatial_none",)),
    ("gr-cell-no-capacity", "GenericRSpace",
     "F22 (F15): the cell stores a second record, with no capacity check",
     "        let store := if side_full (recs s) then None else Some ([SStore e], None) in",
     "        let store := Some ([SStore e], @None reissue) in",
     ("cell_offer_loop_ok",)),
    ("gr-peek-without-restore", "GenericRSpace",
     "Rule E: a peek fire removes the record and stages no re-store",
     "                if w_peek k && negb (r_pers r) then restore_scan k r s1 (S fired) else ([], None) in",
     "                if false then restore_scan k r s1 (S fired) else ([], None) in",
     ("scan_peek_resends",)),
    ("gr-restore-op-authority", "GenericRSpace",
     "Rule E: the re-store registers the authority of the peek operation, not the record's own",
     "    mkElem (r_bytes r) false (own_auth (r_bytes r)).",
     "    mkElem (r_bytes r) false (w_auth k).",
     ("scan_peek_resends",)),
    ("gr-restore-without-reoffer", "GenericRSpace",
     "Rule E: the re-store is a plain store, so a parked guarded waiter that admits the record is not fired as by the host's second send",
     "    offer_loop (S (length (wtrs s))) (restore_elem k r) s fired.",
     "    ([SStore (restore_elem k r)], @None reissue).",
     ("scan_peek_resends",)),
    ("gr-ord-not-zero", "GenericRSpace",
     "decision 43: a collection without a counter keeps a re-issued order other than 0",
     "    | Some o, None => if o =? 0 then Some 0 else None",
     "    | Some o, None => Some o",
     ("scan_reissue_identity_stable",)),
    ("gr-reissue-carries-order", "GenericRSpace",
     "decision 43 (A.7): a re-issued scan produce carries an order, so its next round fails or stages another record",
     "    mkProduceOp (op_data op) (op_pers op) (op_auth op) None.",
     "    mkProduceOp (op_data op) (op_pers op) (op_auth op) (Some 1).",
     ("scan_reissue_iter",)),
    ("gr-restore-reissue-persistent", "GenericRSpace",
     "decision 43 (A.7): the cap re-issue of a re-send is persistent, so its record has another identity",
     "    | Some (_, d) => Some (mkProduceOp d false (own_auth (r_bytes r)) None)",
     "    | Some (_, d) => Some (mkProduceOp d true (own_auth (r_bytes r)) None)",
     ("scan_reissue_identity_stable",)),
    ("gr-reissue-other-authority", "GenericRSpace",
     "decision 43 (B.7): a re-issued produce re-offers its datum with the authority of the fired waiter",
     "            | FReissue => (fire :: retires, Some (ReProduce e))",
     "            | FReissue => (fire :: retires, Some (ReProduce (mkElem (e_bytes e) (e_pers e) (w_auth w))))",
     ("offer_loop_reissue_shape",)),
    ("arc-growth-before-reserve", "AgentReadCoverage",
     "S1: a staged list grows before it reserves its doubled buffer",
     "Definition grow_step (units : nat) : list event := [Reserve units; Read units].",
     "Definition grow_step (units : nat) : list event := [Read units; Reserve units].",
     ("grow_step_balanced",)),
    ("arc-group-copies-unreserved", "AgentReadCoverage",
     "S1: a group fire copies the candidates of the other members without a reservation",
     "  ++ flat_map step (er_group r)\n  ++ growth (er_pushes r) (er_staged_cap r) (er_staged_len r)",
     "  ++ map Read (er_group r)\n  ++ growth (er_pushes r) (er_staged_cap r) (er_staged_len r)",
     ("engine_round_covered",)),
    ("arc-capacity-unreserved", "AgentReadCoverage",
     "S1: the capacity check compares without a reservation",
     "  ++ (if er_capacity r then step compare else [])",
     "  ++ (if er_capacity r then [Read compare] else [])",
     ("engine_round_covered",)),
    ("arc-prefix-outcome-copies", "AgentReadCoverage",
     "S1: the outcome of a prefix fire copies its kept results after the commit instead of truncating them",
     "  if fires (er_round r) then [Read (unwrap (er_round r))] else [].",
     "  if fires (er_round r) then [Read (unwrap (er_round r)); Read (if er_prefix r then er_results r else 0)] else [].",
     ("engine_after_rounds_read",)),
    # ── P5 W2: StackDiscipline.v (batches STACK and STACK-I21, decision 49) ─────────────
    ("sd-top-least-ord", "StackDiscipline",
     "the head scan keeps the least order instead of the greatest",
     "      (ord b <? ord a) || ((ord a =? ord b) && (rank b <? rank a)).",
     "      (ord a <? ord b) || ((ord a =? ord b) && (rank b <? rank a)).",
     ("above_false_ge",)),
    ("sd-top-by-position", "StackDiscipline",
     "the top of a side is its first entry in list order",
     "    Definition top (l : list E) : option E := option_map snd (top_index l).",
     "    Definition top (l : list E) : option E := hd_error l.",
     ("top_spec",)),
    ("sd-no-settle-after", "StackDiscipline",
     "an operation whose plan completes runs no settle after the plan",
     "          | PDone =>\n              let after := settle_at (f + fires (p_ops p)) (run st (z_ops before ++ p_ops p)) in\n              mkOutcome (z_ops before ++ p_ops p ++ z_ops after) (settle_next (z_end after))",
     "          | PDone => mkOutcome (z_ops before ++ p_ops p) None",
     ("with_settles_inv",)),
    ("sd-no-lift", "StackDiscipline",
     "the settle re-offers a stored record without its lift",
     "    match x with Some _ => [] | None => if r_pers r then [] else [SLift r] end.",
     "    match x with Some _ => [] | None => if r_pers r then [] else [] end.",
     ("settle_step_records",)),
    ("sd-land-drops", "StackDiscipline",
     "the record in flight does not land: the settle loses it",
     "    match x with None => [] | Some r => [SRestore r] end.",
     "    match x with None => [] | Some _ => [] end.",
     ("next_fire_none_land_inv",)),
    ("sd-peek-spends-lifted", "StackDiscipline",
     "a fire of a waiter that peeks spends the lifted record (decision 30 reverted)",
     "    if flying x r && negb spent then Some r else None.",
     "    None.",
     ("settle_loop_keeps",)),
    ("sd-no-cap-check", "StackDiscipline",
     "the settle loop has no cap check of the transaction at the top of a round (I-21)",
     "  Definition at_cap (fired : nat) : bool := cap <=? fired.",
     "  Definition at_cap (fired : nat) : bool := false.",
     ("settle_loop_bounds",)),
    ("sd-no-divergent-end", "StackDiscipline",
     "a divergent pair goes on instead of ending the round (A10)",
     "    else if stays then FDivergent",
     "    else if stays then FContinue",
     ("settle_loop_bounds",)),
    ("sd-peek-no-restore", "StackDiscipline",
     "a consume-side peek of a record that is not persistent stages no re-store (Rule E)",
     "    if w_peek k && negb (r_pers h) then [SFireConsume k h; SRestore h] else [SFireConsume k h].",
     "    if w_peek k && negb (r_pers h) then [SFireConsume k h] else [SFireConsume k h].",
     ("stack_peek_restore_needs_no_settle",)),
    ("sd-reentry-at-top", "StackDiscipline",
     "a re-entered datum plans at the top whatever the order of the top (decision 43)",
     "    | Some h => if r_ord h <? r_ord x then offer_loop (S cap) f s x else mkPlanned [SStore x] PDone",
     "    | Some _ => offer_loop (S cap) f s x",
     ("stack_lifo", "stack_reentry_at_lifo_place")),
    ("sd-reentry-empty-side-stores", "StackDiscipline",
     "a re-entered datum on an empty side is stored with no plan, so it never fires",
     "    | None => offer_loop (S cap) f s x",
     "    | None => mkPlanned [SStore x] PDone",
     ("stack_reentry_at_lifo_place",)),
    ("sd-reissue-fresh-ord", "StackDiscipline",
     "a re-issued persistent operation carries no first-round order (decision 43)",
     "  Definition carried (pers : bool) (o : nat) : option nat := if pers then Some o else None.",
     "  Definition carried (pers : bool) (o : nat) : option nat := None.",
     ("stack_reissue_keeps_ord",)),
    ("sd-no-bump", "StackDiscipline",
     "an operation does not advance the counter",
     "  Definition bump_next (c : option nat) : option nat := Some (S (bump_ord c)).",
     "  Definition bump_next (c : option nat) : option nat := Some (bump_ord c).",
     ("stack_reissue_keeps_ord",)),
    ("sd-fire-blocked-top", "StackDiscipline",
     "a produce fires the top waiter although the waiter does not fit the datum",
     "        | Some w =>\n            if fits (w_pat w) (r_val r) then\n              let spent := negb (r_pers r) && negb (w_peek w) in",
     "        | Some w =>\n            if true then\n              let spent := negb (r_pers r) && negb (w_peek w) in",
     ("stack_head_blocks",)),
    ("sd-perm-without-distinct-ords", "StackDiscipline",
     "the permutation invariance without the wf premise of distinct orders",
     "  Definition dist (st : sstate) : Prop :=\n    ords_distinct r_ord (s_records st) /\\ ords_distinct w_ord (s_waiters st).",
     "  Definition dist (st : sstate) : Prop := True /\\ True.",
     ("apply_equiv",)),
    ("sd-read-before-reserve", "StackDiscipline",
     "a metered step reads before it reserves (DR-88)",
     "Definition step (units : nat) : list event := [Reserve units; Read units].",
     "Definition step (units : nat) : list event := [Read units; Reserve units].",
     ("step_covered",)),
    ("sd-unprepaid-unwrap", "StackDiscipline",
     "a settle round does not prepay the decode of the fired continuation (decision 42)",
     "    ++ flat_map step (lift r)\n    ++ [Reserve (unwrap r)]\n    ++ flat_map step (reoffer r).",
     "    ++ flat_map step (lift r)\n    ++ flat_map step (reoffer r).",
     ("round_staged_shape",)),
    ("sd-plan-opens-at-cap", "StackDiscipline",
     "open_plan has no cap check: the plan opens after a settle that ended at the cap (I-21)",
     "  Definition plan_at_cap (fired : nat) : bool := cap <=? fired.",
     "  Definition plan_at_cap (fired : nat) : bool := false.",
     ("stack_settle_terminates",)),
    ("sd-plan-restarts-count", "StackDiscipline",
     "the plan restarts the fire count at 0 instead of the count of the settle before it (I-21)",
     "          let p := plan f (run st (z_ops before)) in",
     "          let p := plan 0 (run st (z_ops before)) in",
     ("stack_settle_terminates",)),
    ("sd-after-settle-restarts-count", "StackDiscipline",
     "the settle after the plan restarts the fire count at 0 (each settle call has its own count)",
     "settle_at (f + fires (p_ops p))",
     "settle_at 0",
     ("stack_settle_terminates",)),
    # P6 step 2 (Similarity.v): each control reverts one rule of the exact
    # decisions of the VectorDB family, or claims more than the rule gives.
    ("sim-squares-without-signs", "Similarity",
     "cosine compares the squares only and ignores the signs of q.e and t",
     "  Definition cosine_within (u t N : R) : Prop :=\n"
     "    (rzero <= u /\\ t < rzero) \\/\n"
     "    (rzero <= u /\\ rzero <= t /\\ t * t * N <= u * u) \\/\n"
     "    (u < rzero /\\ t < rzero /\\ u * u <= t * t * N).",
     "  Definition cosine_within (u t N : R) : Prop := t * t * N <= u * u.",
     ("cosine_squared_form_iff",)),
    ("sim-order-swapped-norms", "Similarity",
     "the cosine order multiplies each squared dot product by its own norm",
     "    (rzero <= u1 /\\ rzero <= u2 /\\ u1 * u1 * n2 <= u2 * u2 * n1) \\/",
     "    (rzero <= u1 /\\ rzero <= u2 /\\ u1 * u1 * n1 <= u2 * u2 * n2) \\/",
     ("cosine_order_squared_form",)),
    ("sim-radius-unsquared", "Similarity",
     "euclidean compares the squared distance with the radius, not with its square",
     "  Definition euclidean_within (d2 r : R) : Prop := rzero <= r /\\ d2 <= r * r.",
     "  Definition euclidean_within (d2 r : R) : Prop := rzero <= r /\\ d2 <= r.",
     ("euclidean_radius_squared_form",)),
    ("sim-total-without-distinct-keys", "Similarity",
     "the selection order claims totality without distinct tie keys",
     "  Lemma precedes_total :\n"
     "    forall r1 r2, tie_key r1 <> tie_key r2 -> precedes r1 r2 \\/ precedes r2 r1.",
     "  Lemma precedes_total :\n"
     "    forall r1 r2, precedes r1 r2 \\/ precedes r2 r1.",
     ("precedes_total",)),
    ("sim-unit-threshold-claimed", "Similarity",
     "the cosine bound claims that no record passes the threshold 1 (parallel vectors do)",
     "      (forall t, rone < t -> ~ cosine_within (dot q e) t (dot q q * dot e e)).\n  Proof.",
     "      (forall t, rle rone t -> ~ cosine_within (dot q e) t (dot q q * dot e e)).\n  Proof.",
     ("cosine_bounds",)),
    # P6 step 8b (VectorDbDiscipline.v): each control reverts one rule of the
    # driver of a VectorDB collection.
    ("vdb-no-cascade", "VectorDbDiscipline",
     "the tail runs no LIST-CASCADE: no parked list waiter fires after a removal (decision 66, LM-b)",
     "    | W :: rest => if is_list W && fires W R then Some W else first_cascade R rest",
     "    | W :: rest => first_cascade R rest",
     ("first_cascade_none",)),
    ("vdb-peek-not-owed", "VectorDbDiscipline",
     "a peek fire of RECEIVE-VDB removes its non-persistent members and owes no second sends (Rule E)",
     "          let t' := mkTx (mkSt (drop gone (recs (t_st t))) (wtrs (t_st t))) (S (t_fired t))\n"
     "                         (if w_peek W then gone ++ t_owed t else t_owed t) true",
     "          let t' := mkTx (mkSt (drop gone (recs (t_st t))) (wtrs (t_st t))) (S (t_fired t))\n"
     "                         (t_owed t) true",
     ("receive_nl",)),
    ("vdb-lift-not-owed", "VectorDbDiscipline",
     "a cascade fire of a peek lifts its first member and owes no second send for it (Rule E, decision 30)",
     "(if w_peek W && lifted then [lead] else []) true",
     "[] true",
     ("tail_nl",)),
    ("vdb-retire-unlogged", "VectorDbDiscipline",
     "an offer fire does not log the members that it retires as spent",
     "(map r_id out ++ (if spent then [r_id x] else []) ++ t_spent t) in",
     "((if spent then [r_id x] else []) ++ t_spent t) in",
     ("offer_nl",)),
    ("vdb-window-top1", "VectorDbDiscipline",
     "the similarity window of a join bind holds only its best eligible record, so a removal can expose one",
     "        match first_fit C p (esort q R) with",
     "        match first_fit C p (firstn 1 (esort q R)) with",
     ("jfill_mono",)),
    ("vdb-cont-on-persistent", "VectorDbDiscipline",
     "a fire whose persistent partner stays continues the loop: no divergent-pair stop (FireRounds::after_fire)",
     "    if spent then Done else if stays then Stop else if cap <=? fired then Stop else Cont.",
     "    if spent then Done else if cap <=? fired then Stop else Cont.",
     ("after_fire_cont",)),
    ("vdb-offer-never-fires", "VectorDbDiscipline",
     "OFFER-VDB fires no parked waiter: a send is stored next to a waiter that it fires",
     "    | W :: rest => if ofires W R x then Some W else first_offer R x rest",
     "    | W :: rest => first_offer R x rest",
     ("first_offer_none",)),
    # P6 step 8d (VectorOps.v): each control reverts one rule of the replies
    # of rho:lang:vector.
    ("vo-kjoin-narrower", "VectorOps",
     "the join of two kinds takes the narrower kind, so a reply can lose the kind of a wider input",
     "Definition kjoin (a b : kind) : kind := if Nat.ltb (krank a) (krank b) then b else a.",
     "Definition kjoin (a b : kind) : kind := if Nat.ltb (krank a) (krank b) then a else b.",
     ("kjoin_rank",)),
    ("vo-dot-rounds-each-term", "VectorOps",
     "dot rounds each product before the sum, so a float reply is not one rounding of the exact value (decision 54)",
     "    encode k (qsum (map (fun p => val (fst p) * val (snd p)) (combine a b))).",
     "    encode k (qsum (map (fun p => val (encode k (val (fst p) * val (snd p)))) (combine a b))).",
     ("vector_replies_exact",)),
    ("vo-faithful-root-bound", "VectorOps",
     "the check bounds the square of the root by the square of the next float, so a nearer float can exist",
     "    x <= ((r + hi) * (1 # 2)) * ((r + hi) * (1 # 2)) ->",
     "    x <= hi * hi ->",
     ("root_midpoint_nearest",)),
)

DECLARATION = re.compile(
    r"^\s*(?:Lemma|Theorem|Corollary|Fact|Remark|Proposition|Definition|Fixpoint|Inductive|Record)\s+"
    r"([A-Za-z_][A-Za-z0-9_']*)\b")
ERROR_LOCATION = re.compile(r'^File "[^"]*", line (\d+), characters')


def bounded(command: list[str]) -> list[str]:
    if os.environ.get("RRS_SANDBOX") == "1":
        return command
    memory = os.environ.get("RRS_MEMORY_MAX", "6G")
    quota = os.environ.get("RRS_CPU_QUOTA", "400%")
    return ["systemd-run", "--user", "--scope", "--quiet", "-p", f"MemoryMax={memory}",
            "-p", "MemorySwapMax=0", "-p", f"CPUQuota={quota}", "--"] + command


def compile_module(directory: Path, module: str, env: dict[str, str]) -> tuple[int, str]:
    result = subprocess.run(bounded(["coqc", "-q", "-noglob", f"{module}.v"]), cwd=directory,
                            env=env, capture_output=True, text=True, check=False)
    return result.returncode, result.stdout + result.stderr


def enclosing_declaration(source: str, line: int) -> str | None:
    name = None
    for number, text in enumerate(source.split("\n"), start=1):
        if number > line:
            break
        match = DECLARATION.match(text)
        if match:
            name = match.group(1)
    return name


def main() -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n\n")[0])
    parser.add_argument("--only", action="append", default=[], metavar="ID",
                        help="run only this control (repeatable)")
    parser.add_argument("--list", action="store_true", help="list the controls and exit")
    args = parser.parse_args()
    if args.list:
        for cid, module, description, *_ in CONTROLS:
            print(f"{cid:34s} {module:17s} {description}")
        return 0

    source_root = Path(os.environ.get("RRS_ROCQ_SRC", REPO_ROOT / "formal/rocq/reified_rspaces"))
    theories = source_root / "theories"
    scratch_root = Path(os.environ.get("RRS_SCRATCH",
                                       REPO_ROOT / "target/verification/reified-rspaces/scratch"))
    if os.environ.get("RRS_SANDBOX") == "1":
        log_dir = scratch_root / "logs" / "negative-controls"
    else:
        log_dir = REPO_ROOT / "target/verification/reified-rspaces/logs/negative-controls"
    scratch_root.mkdir(parents=True, exist_ok=True)
    log_dir.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="negctl.", dir=scratch_root))
    env = dict(os.environ)
    env["TMPDIR"] = str(work / "tmp")
    (work / "tmp").mkdir()

    selected = [c for c in CONTROLS if not args.only or c[0] in args.only]
    unknown = sorted(set(args.only) - {c[0] for c in CONTROLS})
    if unknown:
        print(f"error: unknown control(s): {', '.join(unknown)}", file=sys.stderr)
        return 2
    passed = failed = 0
    try:
        print(f"== Reified RSpaces Rocq negative controls ({len(selected)} controls)")
        print(f"   scratch: {work}")
        modules = sorted({c[1] for c in selected})
        pristine: dict[str, str] = {}
        for module in modules:
            path = theories / f"{module}.v"
            if not path.is_file():
                print(f"  FAIL baseline {module}: {path} is missing")
                return 1
            pristine[module] = path.read_text(encoding="utf-8")
            directory = work / "baseline" / module
            directory.mkdir(parents=True)
            (directory / f"{module}.v").write_text(pristine[module], encoding="utf-8")
            code, output = compile_module(directory, module, env)
            (log_dir / f"baseline-{module}.log").write_text(output, encoding="utf-8")
            if code != 0:
                print(f"  FAIL baseline {module}: the pristine module does not compile")
                print("\n".join("        " + line for line in output.strip().split("\n")[-10:]))
                return 1
            print(f"  PASS baseline {module}: the pristine module compiles")

        for cid, module, description, old, new, expected in selected:
            olds = old if isinstance(old, tuple) else (old,)
            news = new if isinstance(new, tuple) else (new,)
            if len(olds) != len(news):
                failed += 1
                print(f"  FAIL {cid}: malformed control, {len(olds)} old texts and {len(news)} new texts")
                continue
            mutated = pristine[module]
            stale = None
            for site, (old_text, new_text) in enumerate(zip(olds, news), start=1):
                count = mutated.count(old_text)
                if count != 1:
                    stale = (site, count)
                    break
                mutated = mutated.replace(old_text, new_text, 1)
            if stale is not None:
                failed += 1
                print(f"  FAIL {cid}: stale control, the mutation text of site {stale[0]} occurs "
                      f"{stale[1]} times in {module}.v")
                continue
            directory = work / cid
            directory.mkdir(parents=True)
            (directory / f"{module}.v").write_text(mutated, encoding="utf-8")
            code, output = compile_module(directory, module, env)
            (log_dir / f"{cid}.log").write_text(output, encoding="utf-8")
            if code == 0:
                failed += 1
                print(f"  FAIL {cid}: the mutant compiles ({description})")
                continue
            match = None
            for line in output.split("\n"):
                match = ERROR_LOCATION.match(line)
                if match:
                    break
            if match is None:
                failed += 1
                print(f"  FAIL {cid}: coqc failed with no error location; see {log_dir / (cid + '.log')}")
                continue
            line_number = int(match.group(1))
            where = enclosing_declaration(mutated, line_number)
            if where in expected:
                passed += 1
                print(f"  PASS {cid}: rejected in {where} (line {line_number}): {description}")
            else:
                failed += 1
                print(f"  FAIL {cid}: rejected in {where} (line {line_number}), expected one of "
                      f"{', '.join(expected)}")
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print(f"reified-rspaces negative controls: PASS={passed} FAIL={failed}")
    print(f"   logs: {log_dir}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
