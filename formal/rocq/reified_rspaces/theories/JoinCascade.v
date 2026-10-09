(* ═══════════════════════════════════════════════════════════════════════════
   JoinCascade.v — joins in one space: the search, the join engine and the
   cascade (P5c)

   This module models the joins of one space (plan decision 29 and the
   joins plan). A join is a receive with two or more binds. Its binds can
   name one collection more than once (a repeated channel) or several
   collections of the space. The model covers the four windows of
   Discipline::JOIN_WINDOW: Head (queue, stack, cell), Lanes (priority
   queue), Scan (bag) and Classes (set). The module proves only statements
   about more than one bind or more than one collection. The single
   collection facts stay in StackDiscipline.v, GenericRSpace.v,
   CounterConflict.v and HeadOnlyDisciplines.v (see "Not restated").

   The search (plan section 3). GREEDY takes the binds in the canonical
   group order (FR-1': the stable hash of the channel, then the bind
   index). Each bind picks the first unclaimed record of its lane-visible
   window that matches its pattern spatially. A pick claims the tag of its
   record (the record, or the class of a set member). The guard runs once,
   over the values in bind order (decision 37). The search does not
   backtrack. On the produce side, SEARCH-PRODUCE forces the incoming record
   at one position of its collection. It fills the other positions from
   the windows with the incoming record placed, and it tries the positions
   in group order (the position retry). The incoming record binds exactly
   once (decision 28). A bag and a set put the element in flight at the end
   of a window that holds every stored record, so it always lies in its
   window.

   The engine (plan section 4, amended 2026-10-07). RUN-JOIN is one
   transaction of a join operation. TOUCH bumps the counter of a collection
   before the first staging on it (decision 31). A settle job has no plan:
   it loads its pending second sends, makes its scope dirty, drains, and
   commits (DV-1). Another operation runs the settle before the plan when
   its discipline cascades, clears the dirty collections, and opens the
   plan only below the fire cap (open_plan, decision 49, DV-2). The plan
   runs OFFER or RECEIVE, and the drain after the plan runs the second
   sends and the cascade again. The head shapes and the set cascade
   (decisions 11 and 50, CASCADES), and the bag does not. A fire makes
   every collection of its group dirty. The cascade fires each fireable
   waiter of the dirty collections through LIFT-AND-REOFFER: it lifts the
   first non-persistent bound record and offers it again with its own
   authority. A peek fire pushes the element in flight, also when it is
   persistent, and its other non-persistent results as second sends (Rule
   E, decisions 30 and 45, DV-3). The drain offers them again in receipt
   order. A set fire that does not peek retires the other non-persistent
   members of each bound class (RETIRE-SET-BINDS), also in the cascade
   (decision 50). One fire count runs through the transaction (decision
   49). A divergent pair of the operation's own element does not stop the
   transaction (plan 4.10). A stop that owes work commits a settle job with
   its seeds, its pending second sends and its resume operation
   (CascadeState). The element of the operation in the work returns to its
   operation. A stop that owes nothing re-issues its resume only (DV-5). A
   re-issued persistent join keeps the ords of its first round (decision
   43). PREPARE closes the collections of the operation under the join
   groups of the state. It escalates the lock once when the own keys of
   the operation do not cover the closure ("scope, then lock, then plan").

   Sources
   - Plan: .reified-rspaces-work/p5/joins-plan.md with the amendments of
     2026-10-07 (decision 50 and the alignment points DV-1 to DV-5),
     sections 2 to 4 (the types, the definitions and the algorithms), 3.6
     (the invariants), 7.1 (the cascade witness) and 8.1 (the headlines
     and the controls of this module). Decisions 11, 28, 29, 30, 31, 37,
     41, 42, 43, 45, 49 and 50 of the realignment plan.
   - Code: rholang/src/rust/interpreter/spaces/join.rs of the P5c engine
     step, in the working tree of the model alignment pass of 2026-10-07
     (4329 lines, not yet committed). The line numbers below are the line
     numbers of that tree. The v2 slots are in spaces/record.rs of the same
     tree.

   ─────────────────────────────────────────────────────────────────────────
   Traceability: Rocq definition → Rust code (rholang/src/rust/interpreter)
   ─────────────────────────────────────────────────────────────────────────
   shape, window, win,        spaces/join.rs:1689-1784 (KeyedSource::window:
   win_in, wsel               the first m entries, the first m of each
                              level, every record, with the element in
                              flight placed, and the claims of a set) and
                              720-847 (the trait JoinSource)
   lane_of, sees, visible     spaces/join.rs:4928-4936 (visible) and
                              spaces/disciplines/priority_queue.rs
                              (receive_lane, send_level, Lane::sees)
   lanes_on, lane_queue,      spaces/join.rs:1887-1924 and 1926-1989
   at_head, heads_ok,         (KeyedSource::at_head, at_head_incoming) and
   park_view                  2284-2303 (Engine::all_at_head)
   gle, gpairs, gorder        spaces/join.rs:476-500 (group_order, FR-1')
   restore, in_bind_order     spaces/record.rs:1936 (restore_bind_order) and
                              spaces/join.rs:3212-3275 (admits: the values
                              in the bind order of the slots)
   guard_ok                   spaces/join.rs:3212-3275 (admits: the guard
                              once, admits_all)
   tag, claimed, try_cands,   spaces/join.rs:3091-3204 (Engine::search: the
   fill, slots_cons, greedy,  first unclaimed entry of the window that the
   receive_decision           lane sees and the pattern binds, with no
                              backtracking), 1392-1411 (KeyedSource::claim)
                              and 3516-3535 (is_claimed)
   forced, slots_prod,        spaces/join.rs:3280-3392 (search_produce: the
   incoming_positions,        heads, x in its window, the positions of the
   in_win, x_in_win,          collection of x in group order) and the
   search_at, search_produce  Scan and Classes arm of 1298-1390 (x at the
                              end of the window)
   staged, apply, jst, stage  spaces/join.rs:2514-2544 (Engine: bumps,
                              dirty, work, own and stop) and the stagings
                              of Cx in spaces/generic.rs
   cascades                   spaces/join.rs:2607-2612 (Engine::cascades)
   touch, touch_all,          spaces/join.rs:2621-2650 and 2653-2665 (TOUCH)
   touch_scope                and 1181-1198 (KeyedSource::touch)
   store                      spaces/join.rs:3687-3707 (store)
   is_inc, sends_at,          spaces/join.rs:3594-3635 (second_sends: the
   np_results,                element in flight whether or not it is
   push_second_sends          persistent, and the removed records that are
                              not persistent) and 2606-2611 (push_work)
   retire_positions,          spaces/join.rs:2254-2288 and 2293-2306
   class_members,             (retire_classes, class_claims) and
   retire_class,              spaces/disciplines/scan.rs:819 (retire_class_on)
   retire_set_binds
   fire_tail                  the tails of offer, receive and
                              lift_and_reoffer (second sends or retires)
   after_fire, cap            reified/costs.rs:44, 322-333 (FIRE_ROUND_CAP,
                              FireRounds::after_fire), called by offer,
                              receive and lift_and_reoffer
   fire_dirty                 spaces/join.rs:2668 and 873-879 (mark_dirty
                              and mark, on the collections of the group)
   offer_cands, fires_with,   spaces/join.rs:3713-3871 (offer), 1413-1445
   offer_pick, offer1, offer  (KeyedSource::offer_order) and 1618-1697
                              (union_order)
   class_pers, rcv_stays      spaces/join.rs:3944-4006 (class_candidates:
                              the class stays when one member is
                              persistent) and 2977 (the step of a
                              single-bind fire)
   receive_sel, receive1,     spaces/join.rs:4013-4206, 4328-4344 and
   receive, park              3088-3115 (receive, after_receive, park)
   first_np, reoffer_pos,     spaces/join.rs:4603-4703 and 4712-4764
   lift_ops, lift_reoffer     (lift_and_reoffer, lift)
   fireable, heads_of,        spaces/join.rs:4382-4424 (next_fire) and
   find_fire, next_fire_in,   1447-1487 (KeyedSource::cascade_order, with
   next_fire                  union_order for a set)
   resend_elem, resend,       spaces/join.rs:4467-4499 (drain_and_cascade)
   dstep, drain               and 2674-2703 (incoming_of: the own
                              introduction of the record)
   op, own_of, ords_of,       spaces/join.rs:2772-2810 (carried_ords) and
   prod_ord, carried_ord,     3445-3499 (resume)
   carried_ords, reissue_op
   pending_work, resume_of,   spaces/join.rs:4769-4831 (commit: the own
   owes, commit               entries of the work, owed, the seeds),
                              3411-3440 (settle_fields), 3445-3499 (resume)
                              and spaces/agent.rs:244-287 (PendingProduce,
                              CascadeState, SettleOp)
   phase_count, plan_at_cap   the one count of Cx::rounds for the
                              transaction, and spaces/join.rs:2734 (the
                              plan opens below the cap)
   init, plan, pending_of,    spaces/join.rs:2681-2768 (Engine::run: the
   run_join                   settle job at 1968, the settle before the
                              plan at 1988) and 598-627 (run_join)
   partners, close_step,      spaces/join.rs:371-404 (classify), 462-584
   close_iter, universe,      (join_scope: the closure) and 183-226
   closure, join_scope,       (JoinScope, covers)
   covers, locked_ok, prepare

   ─────────────────────────────────────────────────────────────────────────
   Abstraction
   ─────────────────────────────────────────────────────────────────────────
   - spatial is the metered spatial match (SpaceMatcher::bind), and admits
     is admits_all over the values in bind order. They are separate section
     variables (decision 37). no_guard is the guard of a receive without a
     where clause, and it admits every list.
   - rorder is the visit order of the records of a collection: the head
     order of a queue or a stack, level then ord for a priority queue, and
     DR-75 for a bag and a set. Its hypotheses ask for a permutation, a
     function of the entry multiset, and an insertion that keeps the order
     of the other entries. rorder_filter derives from them that the order
     commutes with each filter. worder is the head order of the waiters of
     a collection. wit_parameters_valid shows parameters that satisfy every
     hypothesis of the section.
   - chash is the stable hash of D(sid, k), cls_of is join_class_key, cap
     is FIRE_ROUND_CAP, and sh is the window of the space. They are section
     variables, so each theorem holds for each value.
   - The view of a space is a record list, a waiter list and the counters.
     A removal takes the first entry with the identity of the removed entry
     (del_rec, del_wtr). The theorems that need it assume distinct
     identities (WF, start_ok, or a NoDup premise).
   - The window of a set holds one representative per class, the first
     member in the visit order. The code holds every record and skips a
     claimed class. The two agree when the members of a class bind the same
     patterns, which class_candidates requires of the code.
   - The window of a bag or a set places the element in flight by the
     visit order, and the produce search treats it as always in its window
     (x_in_win), as the code does with x at the end of the window. The
     other binds see the same records: not_x removes x, and the claim of x
     skips its class.
   - A single-bind set fire ([D; n], plan 4.7) is a fire and then K_retire
     stagings, so the view effect is the same. Its partner stays when a
     member of the class is persistent (rcv_stays, class_candidates).
   - A K_lift event carries no key. The code keys it by the counter and the
     step, or by the source hash of the record on a set (decision 50).
   - Headline 4 is proved at the three selection sites of the engine
     (offer_pick, receive_sel and next_fire). The TLA model checks the same
     property on each fire of a trace.
   - Headline 17 is a theorem over abstract parameters.
     greedy_join_can_miss_assignment_witness instantiates it on the valid
     parameters.
   - The model omits the Inconsistent errors of the engine and the
     CellOccupied refusal of a cell.
   - The fuel arguments make the definitions total. A run out of fuel
     stops with RoundCap, which is a conservative stop that re-issues the
     operation. Headline 7 shows that each step lowers μ or stops.
   - Premises of the headlines: wf_op (a receive has a bind) for headline
     14, wf_waiter (no receive both peeks and persists) and nonempty binds
     for headline 7, distinct identities for headlines 6 and 10, and
     start_ok for headline 9.

   Headline theorems (each one has a Print Assumptions line at the end)
    1. join_search_is_head_greedy         the consume-side search is GREEDY
                                          over the windows in group order,
                                          with the guard once in bind order
                                          (sound: an assignment)
    2. join_produce_search_agrees_with_head
                                          when GREEDY with x stored binds x
                                          at p*, SEARCH-PRODUCE completes at
                                          a p <= p* with that assignment at
                                          p*, and it agrees with HEAD's
                                          greedy that binds x
    3. join_incoming_binds_once           a produce-side fire binds its
                                          incoming record at exactly one
                                          position, after at most m_W(c_x)
                                          attempts
    4. join_fires_heads_only              every fire is at the head of each
                                          lane of its group and binds
                                          records of its windows
    5. join_repeated_channel_distinct_records
                                          the binds of one fire bind
                                          distinct records (distinct
                                          classes for a set)
    6. join_set_bind_retires_class        after a set fire that does not
                                          peek, no non-persistent member of
                                          a bound class stays stored, and a
                                          peek retires nothing
    7. cascade_terminates                 each drain step lowers μ or
                                          stops, also when the persistent
                                          element of the operation is a
                                          pending second send, and a drain
                                          that starts at f fires stages at
                                          most cap - f fires
    8. join_txn_fires_bounded             one fire count runs through the
                                          transaction (at most cap fires),
                                          and a settle job starts at 0
    9. cascade_restores_inv               head shapes and set: CI at each
                                          end, and INV_head at an end with
                                          no stop (and no work). Bag:
                                          INV_scan at each end
   10. cascade_order_independent          single-head disciplines: two
                                          fireable waiters that do not peek
                                          have disjoint groups, and their
                                          fires commute
   11. lift_reoffer_equals_direct_comm    lift and re-offer leave the view
                                          of one direct COMM and add only
                                          the K_lift events, and the
                                          retirements of a set fire that
                                          does not peek
   12. cascade_bumps_touched_counters     each staging on a collection
                                          follows the one bump of its
                                          counter in the transaction
   13. bind_permutation_restores_order    the group order is a function of
                                          the bind multiset, and the slots
                                          put the results back in bind
                                          order for each permutation
   14. plan_then_lock_stable              every staging is on a lock key of
                                          the closure, and with the
                                          footprint premise PREPARE runs the
                                          plan of the state and its lock
                                          check passes
   15. join_reissue_keeps_bind_ords       a re-issued persistent join
                                          carries its first-round ords, and
                                          each round bumps its own counters
   16. peek_join_restores_stored_candidates
                                          a peek fire sends its removed
                                          non-persistent records again
                                          through OFFER, in receipt order,
                                          with their own authority, and the
                                          element in flight also when it is
                                          persistent. No record is lost,
                                          also across a stop, the commit
                                          returns the own element to its
                                          operation, and a stop that owes
                                          nothing re-issues its resume only
   17. greedy_join_can_miss_assignment    a computed negative result: GREEDY
                                          parks while an assignment exists
                                          (decision 37 until P12)

   Not restated (the single-collection headlines that this module cites)
   - StackDiscipline.v: stack_settle_terminates,
     stack_plan_permutation_invariant, stack_without_settle_breaks_inv,
     stack_lifo, stack_head_blocks, stack_reentry_at_lifo_place,
     stack_reissue_keeps_ord, stack_peek_restore_needs_no_settle,
     settle_trace_covered, stack_settle_keeps_unspent_records.
   - GenericRSpace.v: consume_fires_only_matching,
     produce_fires_only_matching, unordered_no_pending_match,
     peek_restore_equals_host_reproduce, scan_reissue_identity_stable.
   - CounterConflict.v: shared_collection_race, crossed_collections_race,
     ordered_writes_race_on_counter, ordered_reads_race_on_counter,
     first_ops_race_on_space_counter, same_block_ops_chain.
   - HeadOnlyDisciplines.v (the queue, one collection).

   No Axiom, Parameter, Admitted, admit, Abort or Conjecture. Section
   variables and the section hypotheses CascadeRule, BumpRule and
   PlanThenLock are parameters of the closed theorems, and the headlines
   discharge the rule hypotheses. The file imports only the Rocq standard
   library.

   ─────────────────────────────────────────────────────────────────────────
   Findings and deviations
   ─────────────────────────────────────────────────────────────────────────
   - F-JF-1 (answered by decision 50). Plan 3.6 "Monotonicity" is false
     for the set (Classes). A class sits at the place of its
     representative, the first stored member in DR-75 order. When a fire
     consumes a non-persistent representative and a persistent member of
     the class stays, the class moves to the place of that member.
     set_join_class_shift_breaks_inv_scan computes the negative result of
     the set without a cascade: the plan of a receive starts with INV_scan
     and ends without it. Decision 50 makes joined set collections cascade,
     and the same lemma computes that RUN-JOIN with the cascade ends with
     INV_head. Headline 9 now holds for the set with the head shapes.
   - DV-1 and DV-2 (fixed in join.rs). A settle job loads its pending
     second sends before any fire and has no plan, and the plan opens only
     below the cap. The model follows the code (the earlier deviations D-1
     and D-2 are now the code).
   - DV-3 (code kept, model aligned). second_sends pushes the element in
     flight whether or not it is persistent (decision 30, as
     generic::offer_head continues with a persistent datum after a peek
     fire). A stored persistent record that the cascade re-offers is not in
     flight. The model pushes the incoming position when it is in flight
     (sends_at).
   - DV-4 (model, fixed in the first pass). A divergent own element does
     not stop the transaction (plan 4.10).
   - DV-5 (code kept, model aligned). A stop that owes nothing (no pending
     second send, and no dirty collection of a discipline that cascades)
     re-issues its resume only, so nothing when the element settled.
   - A1 (model aligned). COMMIT moves the element of the operation out of
     the pending second sends and re-issues the operation (resume_of). The
     first model kept it in both.
   - A2 (model aligned). A single-bind fire on a set stays when a member of
     the class is persistent (rcv_stays).
   - A3 (model aligned). A settle job has no plan. The first model ran a
     second drain over the scope, which fired nothing.
   - A4 (model aligned). The element in flight always lies in the window of
     a bag or a set (x_in_win).
   - A5 (model aligned). The pending second sends of a settle job load as
     records that are not the element of an operation.

   ─────────────────────────────────────────────────────────────────────────
   Negative controls
   ─────────────────────────────────────────────────────────────────────────
   Each control changes one rule of the model in a scratch copy. The copy
   must fail to compile, and its first error must be in the lemma that the
   change makes false. The rows use the CONTROLS format of
   scripts/verify-reified/rocq-negative-controls.py.
   jc-guard-per-bind              the guard runs on each bind
                                  → join_search_is_head_greedy
   jc-incoming-skipped            the produce search tries no position
                                  for x → join_produce_search_agrees_with_head
   jc-incoming-every-position     x is forced at every matching position
                                  → join_incoming_binds_once
   jc-head-on-incoming-only       the produce side checks the head of the
                                  collection of x only
                                  → join_fires_heads_only
   jc-claims-ignored              the search ignores its claims
                                  → join_repeated_channel_distinct_records
   jc-retire-incoming-class-only  a set fire retires the class of x only
                                  → join_set_bind_retires_class
   jc-peek-persistent-waiter      a receive can peek and persist
                                  → cascade_terminates
   jc-per-phase-count             each phase starts its count at 0
                                  → join_txn_fires_bounded
   jc-cascade-incoming-only       a fire makes only its incoming
                                  collection dirty → cascade_restores_inv
   jc-set-no-cascade              the set does not cascade (decision 50
                                  reverted) → cascade_restores_inv
   jc-two-lane-heads              a single-head discipline keeps a lane
                                  for each bind → cascade_order_independent
   jc-lift-persistent             the cascade lifts a persistent record
                                  → lift_reoffer_equals_direct_comm
   jc-reoffer-first-position      the cascade re-offers position 0
                                  → lift_reoffer_equals_direct_comm
   jc-bump-own-only               TOUCH bumps the own collections only
                                  → cascade_bumps_touched_counters
   jc-inverse-permutation         the outcome reads the inverse
                                  permutation → bind_permutation_restores_order
   jc-closure-one-level           the closure stops one level deep
                                  → plan_then_lock_stable
   jc-reissue-fresh-ords          a re-issue carries no ords
                                  → join_reissue_keeps_bind_ords
   jc-second-send-plain-store     a second send is stored plainly
                                  → peek_join_restores_stored_candidates
   jc-second-sends-reversed       the second sends pop in reversed order
                                  → peek_join_restores_stored_candidates
   jc-inflight-np-only            the element in flight is sent again only
                                  when it is not persistent (DV-3
                                  reverted) → peek_join_restores_stored_candidates
   jc-commit-own-pending          COMMIT keeps the element of the operation
                                  in the pending second sends (A1
                                  reverted) → peek_join_restores_stored_candidates
   jc-commit-empty-settle         a stop that owes nothing and resumes
                                  nothing commits an empty settle job (DV-5
                                  reverted) → peek_join_restores_stored_candidates
   jc-backtracking                the search backtracks at a dead end
                                  → greedy_join_can_miss_assignment

   ─────────────────────────────────────────────────────────────────────────
   Failed strategies (do not try them again)
   ─────────────────────────────────────────────────────────────────────────
   - repeat split goes into a goal "forall ... -> A /\ B" and introduces
     its variables. Use explicit refine (conj ...) chains.
   - destruct t eqn:E also rewrites the hypotheses that mention t. Use try
     rewrite, or prove the fact without a case split
     (Bool.not_true_iff_false).
   - destruct (keep e0) does not rewrite keep e inside a match branch that
     is not reduced. Use the eqn hypothesis (first_fit_mono).
   - injection H as <- <- substitutes away a variable that the proof still
     needs. Use -> ->.
   - cbn [is_x] does not unfold a definition that is an argument of filter.
     Use change.
   - cbn without a list unfolds <? into leb, so rewrite Nat.ltb_irrefl
     finds no subterm. Use cbn [lex length] (lex_total).
   - apply in_map infers the wrong function under a lambda, and apply
     in_rev takes the wrong direction of the iff. Pass the function
     explicitly and use proj1 or proj2 of in_rev (pend_ids_perm).
   - eapply ext_trans with an evar intermediate fails. Give the
     intermediate state.
   - A case split on w_peek W before the after_fire term that mentions it
     leaves a match that does not reduce. Split on after_fire first.
   - induction on a Permutation clashes with the bound name l. Rename it.
   - apply and exact cannot infer an argument that occurs only in a
     premise or under a projection (the claims of ref_fill_mono, the
     element of CI_offer). Pass it explicitly.
   - Hypothesis names V, P, A, W and Gd clash with section variables. Use
     Vj, Pw, Ka, Hw and Hgd.
   - cbn [plan] does not reduce own_of, and lia then sees two atoms. Add
     own_of to the list.
   - A full cbn folds fires [SStore ...] differently on the two sides.
     Replace it with 0 by reflexivity.
   - The intro patterns of fire_tail_spec (rs and ten components),
     retire_set_binds_spec (rs and eleven components) and store_spec (bs
     and eleven components) were miscounted more than once. Count the
     components.
   - classic_closed needs an excluded middle. closed_dec decides
     closedness constructively (forallb_false_exists).
   - set (j3 := if ... then drain ... (with_dirty scope j1) else j1) does
     not fold after set (j2 := with_dirty scope j1) folded the inner term.
     Set the drain term, then replace the branch (CI_run_join).
   - rewrite E1, E2 with E1 : L = l1 ++ l2 destroys the left side of E2.
     Rewrite E2 first (ins_split).
   - A proof of the cascade invariant that unfolds fire_dirty breaks under
     the control jc-cascade-incoming-only before headline 9. The rule
     enters as the hypothesis of Section CascadeRule, and headline 9
     discharges it. The same holds for cascades: headline 9 derives
     cascades = true from sh <> Scan, so jc-set-no-cascade fails there.
   - rorder_filter does not follow from the permutation hypothesis alone,
     and an induction on the list fails at an element that the filter
     keeps. The induction runs on the count of the removed elements and
     uses rorder_insert at a removed element.
   - f_equal closes a goal whose two sides are convertible, so a tactic
     after it in a new sentence fails with "No such goal". Chain the
     tactics with ";" after f_equal.
   - A lemma that states the trace of lift_reoffer cannot come before
     fire_tail_spec. Headline 11 therefore sits in section 27a.
   - A proof of #15 that destructs owes breaks under a mutant of the commit
     condition. commit_cases states the two outcomes of COMMIT without the
     condition.
   - A keeps relation over every pending second send fails when the
     persistent element of the operation goes from the work to flight.
     keeps tracks the stored records and the second sends that are not the
     element of the operation (tracked, pend_ids).
   ═══════════════════════════════════════════════════════════════════════════ *)
From Stdlib Require Import List Arith Bool Lia PeanoNat.
From Stdlib Require Import Sorting.Permutation.
Import ListNotations.

(* ── 0. Utilities ───────────────────────────────────────────────────────── *)

Definition mem (n : nat) (l : list nat) : bool := existsb (Nat.eqb n) l.

Lemma mem_spec : forall n l, mem n l = true <-> In n l.
Proof.
  intros n l. unfold mem. rewrite existsb_exists. split.
  - intros [y [Hy E]]. apply Nat.eqb_eq in E. subst. exact Hy.
  - intros H. exists n. split; [exact H | apply Nat.eqb_refl].
Qed.

Lemma mem_false : forall n l, mem n l = false <-> ~ In n l.
Proof.
  intros n l. split.
  - intros H Hin. apply mem_spec in Hin. rewrite Hin in H. discriminate.
  - intros H. destruct (mem n l) eqn:E; [apply mem_spec in E; contradiction | reflexivity].
Qed.

(* A claim tag: the collection and the record identity (or the class of a
   set member). *)
Definition tag_eqb (a b : nat * nat) : bool := (fst a =? fst b) && (snd a =? snd b).

Lemma tag_eqb_spec : forall a b, tag_eqb a b = true <-> a = b.
Proof.
  intros [a1 a2] [b1 b2]. unfold tag_eqb. simpl. rewrite andb_true_iff, !Nat.eqb_eq.
  split; [intros [-> ->]; reflexivity | intros H; injection H as -> ->; split; reflexivity].
Qed.

Definition tmem (t : nat * nat) (l : list (nat * nat)) : bool := existsb (tag_eqb t) l.

Lemma tmem_spec : forall t l, tmem t l = true <-> In t l.
Proof.
  intros t l. unfold tmem. rewrite existsb_exists. split.
  - intros [y [Hy E]]. apply tag_eqb_spec in E. subst. exact Hy.
  - intros H. exists t. split; [exact H | apply tag_eqb_spec; reflexivity].
Qed.

Lemma tmem_false : forall t l, tmem t l = false <-> ~ In t l.
Proof.
  intros t l. split.
  - intros H Hin. apply tmem_spec in Hin. rewrite Hin in H. discriminate.
  - intros H. destruct (tmem t l) eqn:E; [apply tmem_spec in E; contradiction | reflexivity].
Qed.

(* The positions of a list: enum k [x0; x1; ...] = [(k, x0); (k+1, x1); ...]. *)
Fixpoint enum {X : Type} (k : nat) (l : list X) : list (nat * X) :=
  match l with [] => [] | x :: r => (k, x) :: enum (S k) r end.

Lemma enum_length : forall (X : Type) (l : list X) k, length (enum k l) = length l.
Proof. intros X l. induction l as [| x r IH]; intros k; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

Lemma enum_map_snd : forall (X : Type) (l : list X) k, map snd (enum k l) = l.
Proof. intros X l. induction l as [| x r IH]; intros k; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

Lemma enum_fst : forall (X : Type) (l : list X) k, map fst (enum k l) = seq k (length l).
Proof. intros X l. induction l as [| x r IH]; intros k; simpl; [reflexivity | rewrite IH; reflexivity]. Qed.

Lemma enum_nth : forall (X : Type) (l : list X) k i y,
  In (i, y) (enum k l) -> k <= i /\ nth_error l (i - k) = Some y.
Proof.
  intros X l. induction l as [| x r IH]; intros k i y H; [destruct H |].
  destruct H as [H | H].
  - injection H as <- <-. split; [lia |]. rewrite Nat.sub_diag. reflexivity.
  - apply IH in H as [Hle Hn]. split; [lia |].
    replace (i - k) with (S (i - S k)) by lia. exact Hn.
Qed.

Lemma enum_nth_error : forall (X : Type) (l : list X) k i,
  nth_error (enum k l) i = option_map (fun y => (k + i, y)) (nth_error l i).
Proof.
  intros X l. induction l as [| x r IH]; intros k i; destruct i as [| i]; simpl; try reflexivity.
  - rewrite Nat.add_0_r. reflexivity.
  - rewrite IH. destruct (nth_error r i); simpl; [| reflexivity]. rewrite Nat.add_succ_r. reflexivity.
Qed.

Lemma enum_NoDup : forall (X : Type) (l : list X) k, NoDup (map fst (enum k l)).
Proof. intros X l k. rewrite enum_fst. apply seq_NoDup. Qed.

(* Insertion sort by a boolean order. *)
Section Sort.
  Variable X : Type.
  Variable le : X -> X -> bool.

  Fixpoint ins (x : X) (l : list X) : list X :=
    match l with [] => [x] | y :: r => if le x y then x :: y :: r else y :: ins x r end.

  Definition isort (l : list X) : list X := fold_right ins [] l.

  Lemma ins_perm : forall x l, Permutation (ins x l) (x :: l).
  Proof.
    intros x l. induction l as [| y r IH]; simpl; [apply Permutation_refl |].
    destruct (le x y); [apply Permutation_refl |].
    apply Permutation_trans with (y :: x :: r); [apply perm_skip; exact IH | apply perm_swap].
  Qed.

  Lemma isort_perm : forall l, Permutation (isort l) l.
  Proof.
    induction l as [| x r IH]; simpl; [apply Permutation_refl |].
    apply Permutation_trans with (x :: isort r); [apply ins_perm | apply perm_skip; exact IH].
  Qed.
End Sort.

Arguments ins {X} le x l.
Arguments isort {X} le l.

(* Options. *)
Definition is_some {X : Type} (o : option X) : bool := match o with Some _ => true | None => false end.

Fixpoint somes {X : Type} (l : list (option X)) : list X :=
  match l with [] => [] | Some x :: r => x :: somes r | None :: r => somes r end.

(* The value paired with the key b. *)
Fixpoint assoc {X : Type} (b : nat) (l : list (nat * X)) : option X :=
  match l with [] => None | (k, x) :: r => if k =? b then Some x else assoc b r end.

(* ── 1. The model ────────────────────────────────────────────────────────── *)

(* Discipline::JOIN_WINDOW: queue, stack and cell (Head), priority queue
   (Lanes), bag (Scan) and set (Classes). *)
Inductive shape : Type := Head | Lanes | Scan | Classes.

Section Join.
  Variable V : Type.     (* a payload *)
  Variable P : Type.     (* the pattern of a bind *)
  Variable M : Type.     (* the bound values of a spatial match *)
  Variable Gd : Type.    (* a where guard *)
  Variable A : Type.     (* a cost authority *)

  Variable spatial : P -> V -> option M.   (* SpaceMatcher::bind, metered *)
  Variable admits : Gd -> list M -> bool.  (* admits_all: the guard over the values in bind order *)
  Variable no_guard : Gd.                  (* the guard of a receive without a where clause *)
  Hypothesis no_guard_admits : forall ms, admits no_guard ms = true.

  Variable sh : shape.         (* Discipline::JOIN_WINDOW of the space *)
  Variable cap : nat.          (* FIRE_ROUND_CAP *)
  Variable chash : nat -> nat. (* stable_hash_provider::hash of D(sid, k) *)
  Variable cls_of : V -> nat.  (* join_class_key: the F5 key of a payload *)

  (* A record of a collection: its store identity, its collection key, the
     order of its envelope, its level (priority queue), its payload and its
     persistence. *)
  Record rec : Type := mkRec {
    r_id : nat; r_coll : nat; r_ord : nat; r_lvl : nat; r_val : V; r_pers : bool }.

  (* A bind of a receive: the collection, the pattern and the lane (a PQ
     query priority; None is the unrestricted lane). *)
  Record bnd : Type := mkBnd { b_coll : nat; b_pat : P; b_lane : option nat }.

  (* A waiter: its identity, its binds in bind order, the ord of the
     collection of each bind (the slots of the v2 envelope), its guard and
     its flags. *)
  Record wtr : Type := mkWtr {
    w_id : nat; w_binds : list bnd; w_ords : list nat; w_guard : Gd; w_pers : bool; w_peek : bool }.

  (* The staged view of a space: the records, the waiters and the counters
     (collection key ↦ the next counter value). *)
  Record view : Type := mkView { v_recs : list rec; v_wtrs : list wtr; v_cnt : list (nat * nat) }.

  (* The visit order of the records of a collection (DR-75 for bag and set,
     the head order for queue and stack, level then ord for a priority
     queue). Any order: a permutation, a function of the entry multiset, and
     an insertion that keeps the order of the other entries. *)
  Variable rorder : list rec -> list rec.
  Hypothesis rorder_perm : forall l, Permutation (rorder l) l.
  Hypothesis rorder_set : forall l l', Permutation l l' -> rorder l = rorder l'.
  Hypothesis rorder_insert :
    forall e l, exists l1 l2, rorder l = l1 ++ l2 /\ rorder (e :: l) = l1 ++ e :: l2.

  (* The head order of the waiters of a collection (by the ord of that
     collection, or DR-75 of the union for bag and set). *)
  Variable worder : nat -> list wtr -> list wtr.
  Hypothesis worder_perm : forall c l, Permutation (worder c l) l.
  Hypothesis worder_insert :
    forall c W l, exists l1 l2, worder c l = l1 ++ l2 /\ worder c (W :: l) = l1 ++ W :: l2.

  (* Introduction::for_record: the cost authority of a stored record. *)
  Variable own_auth : rec -> A.

  (* ── 2. Collections, windows and lanes (plan section 3) ──────────────── *)

  Definition recs_of (c : nat) (v : view) : list rec := filter (fun r => r_coll r =? c) (v_recs v).

  (* The distinct collections of a waiter. *)
  Definition colls (W : wtr) : list nat := nodup Nat.eq_dec (map b_coll (w_binds W)).

  (* m_W(c): the binds of W on c. *)
  Definition mult (W : wtr) (c : nat) : nat := count_occ Nat.eq_dec (map b_coll (w_binds W)) c.

  (* The window selection: an entry is in the window when fewer than lim
     earlier entries share its key. *)
  Fixpoint wsel (lim : nat) (key : rec -> nat) (seen : list nat) (l : list rec) : list rec :=
    match l with
    | [] => []
    | e :: rest =>
        if count_occ Nat.eq_dec seen (key e) <? lim
        then e :: wsel lim key (key e :: seen) rest
        else wsel lim key (key e :: seen) rest
    end.

  Definition class_key (e : rec) : nat := cls_of (r_val e).

  (* WIN(c, m): the first m entries (Head), the first m of each level
     (Lanes), every record (Scan), one representative per class (Classes),
     over the visit order. *)
  Definition window (m : nat) (l : list rec) : list rec :=
    match sh with
    | Head => firstn m l
    | Lanes => wsel m r_lvl [] l
    | Scan => l
    | Classes => wsel 1 class_key [] l
    end.

  (* The window of the collection c for the binds of W, and the window with
     the incoming record x placed by the visit order. *)
  Definition win (W : wtr) (c : nat) (v : view) : list rec := window (mult W c) (rorder (recs_of c v)).

  Definition win_in (W : wtr) (x : rec) (c : nat) (v : view) : list rec :=
    window (mult W c) (rorder (if c =? r_coll x then x :: recs_of c v else recs_of c v)).

  (* Lanes: only a priority queue has lanes, so a single-head discipline has
     one lane (None) on each collection. *)
  Definition lane_of (b : bnd) : option nat := match sh with Lanes => b_lane b | _ => None end.

  (* Lane::sees: the unrestricted lane sees every level, and Level(p) sees p. *)
  Definition sees (L : option nat) (lvl : nat) : bool :=
    match L with None => true | Some p => p =? lvl end.

  Definition visible (b : bnd) (l : list rec) : list rec := filter (fun e => sees (lane_of b) (r_lvl e)) l.

  Definition opt_eqb (a b : option nat) : bool :=
    match a, b with None, None => true | Some x, Some y => x =? y | _, _ => false end.

  Definition waiters_of (c : nat) (v : view) : list wtr := filter (fun W => mem c (colls W)) (v_wtrs v).

  Definition lanes_on (W : wtr) (c : nat) : list (option nat) :=
    map lane_of (filter (fun b => b_coll b =? c) (w_binds W)).

  Definition in_lane (W : wtr) (c : nat) (L : option nat) : bool := existsb (opt_eqb L) (lanes_on W c).

  (* The waiters of the lane L of c in head order, and its head. *)
  Definition lane_queue (c : nat) (L : option nat) (ws : list wtr) : list wtr :=
    worder c (filter (fun W => in_lane W c L) ws).

  (* AT-HEAD(W, c): W is the head of every lane of W on c (decision 11 per
     collection). Bag and set have no head condition. *)
  Definition at_head (W : wtr) (c : nat) (v : view) : bool :=
    match sh with
    | Scan | Classes => true
    | Head | Lanes =>
        forallb (fun L => match lane_queue c L (v_wtrs v) with
                          | W0 :: _ => w_id W0 =? w_id W
                          | [] => false
                          end) (lanes_on W c)
    end.

  Definition heads_ok (cs : list nat) (W : wtr) (v : view) : bool := forallb (fun c => at_head W c v) cs.

  (* The view with W parked: AT-HEAD-INCOMING is at_head on it. *)
  Definition park_view (W : wtr) (v : view) : view := mkView (v_recs v) (W :: v_wtrs v) (v_cnt v).

  (* The view with x stored. *)
  Definition store_view (x : rec) (v : view) : view := mkView (x :: v_recs v) (v_wtrs v) (v_cnt v).

  (* ── 3. The group order and the bind order (FR-1', plan 2.2) ─────────── *)

  (* The canonical group order: the binds sorted by the stable hash of their
     channel, ties by the bind index. *)
  Definition gle (a b : nat * bnd) : bool :=
    (chash (b_coll (snd a)) <? chash (b_coll (snd b))) ||
    ((chash (b_coll (snd a)) =? chash (b_coll (snd b))) && (fst a <=? fst b)).

  Definition gpairs (W : wtr) : list (nat * bnd) := isort gle (enum 0 (w_binds W)).

  (* The bind index of each group position: the bind_i of the v2 slots. *)
  Definition gorder (W : wtr) : list nat := map fst (gpairs W).

  (* The outcome of a join fire (Staged::JoinFire): ordered[slot.bind] =
     results[i], read as the result paired with each bind index. *)
  Definition restore {X : Type} (bs : list nat) (rs : list X) : list (option X) :=
    map (fun b => assoc b (combine bs rs)) (seq 0 (length bs)).

  Definition in_bind_order (W : wtr) (ms : list M) : list M := somes (restore (gorder W) ms).

  (* admits_all: the guard once, over the values in bind order (decision 37). *)
  Definition guard_ok (W : wtr) (sg : list (rec * M)) : bool :=
    admits (w_guard W) (in_bind_order W (map snd sg)).

  (* ── 4. The search (plan 3.4 and 3.5) ───────────────────────────────── *)

  (* A slot of the search, in group order: a pick from a candidate list with
     the pattern of its bind, or the incoming record forced at its position. *)
  Inductive slot : Type := Pick (cands : list rec) (p : P) | Forced (e : rec) (m : M).

  (* The claim tag: the record (its collection and identity), or the class
     of a set member in its collection. *)
  Definition tag (e : rec) : nat * nat :=
    (r_coll e, match sh with Classes => class_key e | _ => r_id e end).

  Definition claimed (C : list (nat * nat)) (e : rec) : bool := tmem (tag e) C.

  (* The spatial test of one candidate of one bind. *)
  Definition pick_match (g : Gd) (p : P) (e : rec) : option M := spatial p (r_val e).

  (* What the search does when the binds after a pick cannot be filled. *)
  Definition on_dead_end {X : Type} (retry : option X) : option X := None.

  (* The candidates of one slot: the first unclaimed candidate that matches.
     k fills the slots after this one with the claims extended. *)
  Fixpoint try_cands (g : Gd) (p : P) (C : list (nat * nat))
      (k : list (nat * nat) -> option (list (rec * M))) (cs : list rec)
      : option (list (rec * M)) :=
    match cs with
    | [] => None
    | e :: more =>
        if claimed C e then try_cands g p C k more else
        match pick_match g p e with
        | None => try_cands g p C k more
        | Some m =>
            match k (tag e :: C) with
            | Some sg => Some ((e, m) :: sg)
            | None => on_dead_end (try_cands g p C k more)
            end
        end
    end.

  Fixpoint fill (g : Gd) (C : list (nat * nat)) (ss : list slot) : option (list (rec * M)) :=
    match ss with
    | [] => Some []
    | Forced e m :: rest =>
        match fill g C rest with Some sg => Some ((e, m) :: sg) | None => None end
    | Pick cs p :: rest => try_cands g p C (fun C' => fill g C' rest) cs
    end.

  (* GREEDY(W): one pick for each group position, over the lane-visible
     window of the collection of the bind. *)
  Definition slots_cons (W : wtr) (v : view) : list slot :=
    map (fun ib => Pick (visible (snd ib) (win W (b_coll (snd ib)) v)) (b_pat (snd ib))) (gpairs W).

  Definition greedy (W : wtr) (v : view) : option (list (rec * M)) := fill (w_guard W) [] (slots_cons W v).

  (* The consume decision of RECEIVE: GREEDY, then the guard once. *)
  Definition receive_decision (W : wtr) (v : view) : option (list (rec * M)) :=
    match greedy W v with
    | Some sg => if guard_ok W sg then Some sg else None
    | None => None
    end.

  (* SEARCH-PRODUCE(W, x): the incoming record x is forced at the position p,
     and the other positions pick stored records (x excluded) from their
     windows with x placed. x is claimed from the start. *)
  Definition forced (W : wtr) (x : rec) (p : nat) (k : nat) (b : bnd) : option M :=
    if k =? p then spatial (b_pat b) (r_val x) else None.

  Definition not_x (x : rec) (l : list rec) : list rec := filter (fun e => negb (r_id e =? r_id x)) l.

  Definition slots_prod (W : wtr) (x : rec) (p : nat) (v : view) : list slot :=
    map (fun kib : nat * (nat * bnd) =>
           let b := snd (snd kib) in
           match forced W x p (fst kib) b with
           | Some m => Forced x m
           | None => Pick (not_x x (visible b (win_in W x (b_coll b) v))) (b_pat b)
           end) (enum 0 (gpairs W)).

  (* The positions that the search tries for the incoming record. *)
  Definition incoming_base (W : wtr) : list (nat * (nat * bnd)) := enum 0 (gpairs W).

  Definition incoming_positions (W : wtr) (x : rec) : list (nat * (nat * bnd)) :=
    filter (fun kib : nat * (nat * bnd) => b_coll (snd (snd kib)) =? r_coll x) (incoming_base W).

  Definition in_win (x : rec) (l : list rec) : bool := existsb (fun e => r_id e =? r_id x) l.

  (* x lies in the window of its collection. A bag and a set put the
     element in flight at the end of a window that holds every stored
     record (KeyedSource::window), so x always lies in it. A head shape and
     a priority queue place x at its position, and x must lie among the
     first m entries (of its level). *)
  Definition x_in_win (W : wtr) (x : rec) (c : nat) (v : view) : bool :=
    match sh with
    | Scan | Classes => true
    | Head | Lanes => in_win x (win_in W x c v)
    end.

  Fixpoint search_at (W : wtr) (x : rec) (v : view) (ks : list (nat * (nat * bnd)))
      : option (nat * list (rec * M)) :=
    match ks with
    | [] => None
    | kib :: rest =>
        let b := snd (snd kib) in
        if sees (lane_of b) (r_lvl x) && x_in_win W x (b_coll b) v then
          match spatial (b_pat b) (r_val x) with
          | None => search_at W x v rest
          | Some _ =>
              match fill (w_guard W) [tag x] (slots_prod W x (fst kib) v) with
              | Some sg => Some (fst kib, sg)
              | None => search_at W x v rest
              end
          end
        else search_at W x v rest
    end.

  Definition search_produce (W : wtr) (x : rec) (v : view) : option (nat * list (rec * M)) :=
    search_at W x v (incoming_positions W x).

  (* ── 5. Generic facts of the search ─────────────────────────────────── *)

  Lemma on_dead_end_cases : forall (X : Type) (r : option X), on_dead_end r = None \/ on_dead_end r = r.
  Proof. intros X r. unfold on_dead_end. auto. Qed.

  (* A pick of try_cands binds a candidate of its list, and the slots after
     it are filled with the claims extended. *)
  Lemma try_cands_some :
    forall g p C k cs sg,
      try_cands g p C k cs = Some sg ->
      exists e m sg', sg = (e, m) :: sg' /\ In e cs /\ pick_match g p e = Some m /\ k (tag e :: C) = Some sg'.
  Proof.
    intros g p C k cs. induction cs as [| e more IH]; intros sg H; cbn [try_cands] in H; [discriminate |].
    repeat match type of H with
           | context [if ?b then _ else _] => let E := fresh "E" in destruct b eqn:E
           | context [match ?o with Some _ => _ | None => _ end] => let E := fresh "E" in destruct o eqn:E
           end.
    all: first
      [ discriminate
      | match type of H with
        | Some (_ :: _) = Some _ =>
            injection H as <-; eexists _, _, _; split; [reflexivity |];
            split; [left; reflexivity | split; eassumption]
        | try_cands _ _ _ _ _ = Some _ =>
            destruct (IH _ H) as (e' & m' & s' & -> & Hin & Hp & Hk);
            exists e', m', s'; split; [reflexivity | split; [right; exact Hin | split; assumption]]
        | on_dead_end (try_cands ?a ?b ?c ?d ?l) = Some _ =>
            destruct (on_dead_end_cases _ (try_cands a b c d l)) as [Ed | Ed]; rewrite Ed in H;
            [discriminate |
             destruct (IH _ H) as (e' & m' & s' & -> & Hin & Hp & Hk);
             exists e', m', s'; split; [reflexivity | split; [right; exact Hin | split; assumption]]]
        end ].
  Qed.

  (* The shape of a filled search: a forced slot binds its record, and a pick
     binds a candidate of its list. *)
  Fixpoint slots_rel (ss : list slot) (sg : list (rec * M)) : Prop :=
    match ss, sg with
    | [], [] => True
    | Forced e m :: ss', rm :: sg' => rm = (e, m) /\ slots_rel ss' sg'
    | Pick L _ :: ss', rm :: sg' => In (fst rm) L /\ slots_rel ss' sg'
    | _, _ => False
    end.

  Lemma fill_shape : forall g ss C sg, fill g C ss = Some sg -> slots_rel ss sg.
  Proof.
    intros g ss. induction ss as [| s rest IH]; intros C sg H; cbn [fill] in H.
    - injection H as <-. exact I.
    - destruct s as [L p | e m].
      + apply try_cands_some in H as (e & m & sg' & -> & Hin & _ & Hk).
        split; [exact Hin | exact (IH _ _ Hk)].
      + destruct (fill g C rest) eqn:Hf; [| discriminate].
        injection H as <-. split; [reflexivity | exact (IH _ _ Hf)].
  Qed.

  Lemma slots_rel_length : forall ss sg, slots_rel ss sg -> length sg = length ss.
  Proof.
    induction ss as [| s rest IH]; intros [| rm sg] H.
    - reflexivity.
    - contradiction.
    - destruct s; contradiction.
    - destruct s; destruct H as [_ H]; simpl; rewrite (IH _ H); reflexivity.
  Qed.

  Lemma slots_rel_nth : forall ss sg i s rm,
    slots_rel ss sg -> nth_error ss i = Some s -> nth_error sg i = Some rm ->
    match s with Forced e m => rm = (e, m) | Pick L _ => In (fst rm) L end.
  Proof.
    induction ss as [| s0 rest IH]; intros sg i s rm H Hs Hr; [destruct i; discriminate |].
    destruct sg as [| rm0 sg]; [destruct s0; contradiction |].
    destruct i as [| i]; simpl in Hs, Hr.
    - injection Hs as <-. injection Hr as <-. destruct s0; destruct H as [H _]; exact H.
    - destruct s0; destruct H as [_ H]; exact (IH _ _ _ _ H Hs Hr).
  Qed.

  Lemma slots_rel_nth_some : forall ss sg i s,
    slots_rel ss sg -> nth_error ss i = Some s -> exists rm, nth_error sg i = Some rm.
  Proof.
    intros ss sg i s H Hs. apply slots_rel_length in H.
    destruct (nth_error sg i) as [rm |] eqn:E; [exists rm; reflexivity |].
    apply nth_error_None in E. assert (i < length ss) by (apply nth_error_Some; rewrite Hs; discriminate).
    lia.
  Qed.

  (* The search loop returns a position of its list whose checks pass and
     whose fill succeeds. *)
  Lemma search_at_some :
    forall W x v ks p sg,
      search_at W x v ks = Some (p, sg) ->
      exists i b m,
        In (p, (i, b)) ks /\ sees (lane_of b) (r_lvl x) = true /\ x_in_win W x (b_coll b) v = true /\
        spatial (b_pat b) (r_val x) = Some m /\ fill (w_guard W) [tag x] (slots_prod W x p v) = Some sg.
  Proof.
    intros W x v ks. induction ks as [| kib rest IH]; intros p sg H; cbn [search_at] in H; [discriminate |].
    destruct kib as [k [i b]]. cbn [fst snd] in H.
    destruct (sees (lane_of b) (r_lvl x) && x_in_win W x (b_coll b) v) eqn:Ec.
    - apply andb_true_iff in Ec as [Es Ew].
      destruct (spatial (b_pat b) (r_val x)) as [m |] eqn:Em.
      + destruct (fill (w_guard W) [tag x] (slots_prod W x k v)) as [sg' |] eqn:Ef.
        * injection H as <- <-. exists i, b, m. split; [left; reflexivity | repeat split; assumption].
        * destruct (IH _ _ H) as (i' & b' & m' & Hin & Hrest). exists i', b', m'. split; [right; exact Hin | exact Hrest].
      + destruct (IH _ _ H) as (i' & b' & m' & Hin & Hrest). exists i', b', m'. split; [right; exact Hin | exact Hrest].
    - destruct (IH _ _ H) as (i' & b' & m' & Hin & Hrest). exists i', b', m'. split; [right; exact Hin | exact Hrest].
  Qed.

  (* The tried positions are positions of the group order on the collection
     of x. *)
  Lemma incoming_positions_on :
    forall W x k i b, In (k, (i, b)) (incoming_positions W x) -> b_coll b = r_coll x.
  Proof.
    intros W x k i b H. unfold incoming_positions in H. apply filter_In in H as [_ H].
    apply Nat.eqb_eq in H. exact H.
  Qed.

  Lemma incoming_base_sub :
    forall W, incl (incoming_base W) (enum 0 (gpairs W)) /\ NoDup (incoming_base W).
  Proof.
    intros W. unfold incoming_base. split.
    - first [apply incl_refl | intros a Ha; destruct Ha].
    - first [apply (NoDup_map_inv fst); apply enum_NoDup | constructor].
  Qed.

  Lemma not_x_excludes : forall x l e, In e (not_x x l) -> r_id e <> r_id x.
  Proof.
    intros x l e H. unfold not_x in H. apply filter_In in H as [_ H].
    apply negb_true_iff, Nat.eqb_neq in H. exact H.
  Qed.

  Lemma slots_prod_nth :
    forall W x p v k i b,
      nth_error (enum 0 (gpairs W)) k = Some (k, (i, b)) ->
      nth_error (slots_prod W x p v) k =
        Some (match forced W x p k b with
              | Some m => Forced x m
              | None => Pick (not_x x (visible b (win_in W x (b_coll b) v))) (b_pat b)
              end).
  Proof.
    intros W x p v k i b H. unfold slots_prod. rewrite nth_error_map, H. reflexivity.
  Qed.

  Lemma enum_gpairs_nth :
    forall W k i b, In (k, (i, b)) (enum 0 (gpairs W)) -> nth_error (enum 0 (gpairs W)) k = Some (k, (i, b)).
  Proof.
    intros W k i b H. apply enum_nth in H as [_ Hn]. rewrite Nat.sub_0_r in Hn.
    rewrite enum_nth_error, Hn. reflexivity.
  Qed.

  (* The count of the bound records with the identity of x, over a filled
     search whose picks exclude x. *)
  Definition is_x (x : rec) (rm : rec * M) : bool := r_id (fst rm) =? r_id x.

  Definition forced_x (x : rec) (s : slot) : bool :=
    match s with Forced e _ => r_id e =? r_id x | Pick _ _ => false end.

  Lemma count_x_slots :
    forall x ss sg,
      slots_rel ss sg ->
      (forall L p e, In (Pick L p) ss -> In e L -> r_id e <> r_id x) ->
      length (filter (is_x x) sg) = length (filter (forced_x x) ss).
  Proof.
    intros x ss. induction ss as [| s rest IH]; intros sg Hr Hex; destruct sg as [| rm sg].
    1: reflexivity.
    1: contradiction.
    1: destruct s; contradiction.
    destruct s as [L p | e m]; destruct Hr as [Hh Hr].
    - change (filter (is_x x) (rm :: sg)) with
        (if is_x x rm then rm :: filter (is_x x) sg else filter (is_x x) sg).
      change (filter (forced_x x) (Pick L p :: rest)) with (filter (forced_x x) rest).
      change (is_x x rm) with (r_id (fst rm) =? r_id x).
      destruct (r_id (fst rm) =? r_id x) eqn:E.
      + apply Nat.eqb_eq in E. exfalso. exact (Hex L p (fst rm) (or_introl eq_refl) Hh E).
      + apply IH; [exact Hr | intros L' p' e' Hin He; exact (Hex L' p' e' (or_intror Hin) He)].
    - subst rm.
      change (filter (is_x x) ((e, m) :: sg)) with
        (if is_x x (e, m) then (e, m) :: filter (is_x x) sg else filter (is_x x) sg).
      change (filter (forced_x x) (Forced e m :: rest)) with
        (if forced_x x (Forced e m) then Forced e m :: filter (forced_x x) rest else filter (forced_x x) rest).
      change (is_x x (e, m)) with (r_id e =? r_id x).
      change (forced_x x (Forced e m)) with (r_id e =? r_id x).
      destruct (r_id e =? r_id x); cbn [length]; [f_equal |];
        (apply IH; [exact Hr | intros L' p' e' Hin He; exact (Hex L' p' e' (or_intror Hin) He)]).
  Qed.

  Lemma count_occ_filter_length :
    forall (l : list bnd) c, count_occ Nat.eq_dec (map b_coll l) c = length (filter (fun b => b_coll b =? c) l).
  Proof.
    induction l as [| b r IH]; intros c; [reflexivity |]. simpl.
    destruct (Nat.eq_dec (b_coll b) c) as [E | N].
    - rewrite (proj2 (Nat.eqb_eq _ _) E). simpl. rewrite IH. reflexivity.
    - rewrite (proj2 (Nat.eqb_neq _ _) N). exact (IH c).
  Qed.

  Lemma filter_length_perm :
    forall (X : Type) (f : X -> bool) l l', Permutation l l' -> length (filter f l) = length (filter f l').
  Proof.
    intros X f l l' H. induction H; simpl; try reflexivity.
    - destruct (f x); simpl; rewrite IHPermutation; reflexivity.
    - destruct (f x), (f y); reflexivity.
    - rewrite IHPermutation1, IHPermutation2. reflexivity.
  Qed.

  Lemma filter_length_incl :
    forall (X : Type) (f : X -> bool) (l l' : list X),
      NoDup l -> incl l l' -> length (filter f l) <= length (filter f l').
  Proof.
    intros X f l l' Hnd Hincl. apply NoDup_incl_length.
    - apply NoDup_filter. exact Hnd.
    - intros a Ha. apply filter_In in Ha as [Ha Hf]. apply filter_In. split; [exact (Hincl a Ha) | exact Hf].
  Qed.

  Lemma enum_filter_snd :
    forall (X : Type) (f : X -> bool) (l : list X) k,
      length (filter (fun kx : nat * X => f (snd kx)) (enum k l)) = length (filter f l).
  Proof.
    intros X f l. induction l as [| x r IH]; intros k; [reflexivity |]. simpl.
    destruct (f x); simpl; rewrite IH; reflexivity.
  Qed.

  Lemma gpairs_perm : forall W, Permutation (gpairs W) (enum 0 (w_binds W)).
  Proof. intros W. unfold gpairs. apply isort_perm. Qed.

  (* The positions of the collection c in the group order number m_W(c). *)
  Lemma positions_on_count :
    forall W c,
      length (filter (fun kib : nat * (nat * bnd) => b_coll (snd (snd kib)) =? c) (enum 0 (gpairs W))) = mult W c.
  Proof.
    intros W c. rewrite (enum_filter_snd (nat * bnd) (fun ib => b_coll (snd ib) =? c)).
    rewrite (filter_length_perm _ _ _ _ (gpairs_perm W)).
    rewrite (enum_filter_snd bnd (fun b => b_coll b =? c)). unfold mult.
    rewrite count_occ_filter_length. reflexivity.
  Qed.

  Lemma filter_map_length :
    forall (X Y : Type) (f : Y -> bool) (g : X -> Y) l,
      length (filter f (map g l)) = length (filter (fun a => f (g a)) l).
  Proof.
    intros X Y f g l. induction l as [| a r IH]; simpl; [reflexivity |].
    destruct (f (g a)); simpl; rewrite IH; reflexivity.
  Qed.

  Lemma enum_fst_count :
    forall (X : Type) (l : list X) k p,
      length (filter (fun kx : nat * X => fst kx =? p) (enum k l)) =
        (if (k <=? p) && (p <? k + length l) then 1 else 0).
  Proof.
    intros X l. induction l as [| x r IH]; intros k p.
    - cbn [enum filter length]. rewrite Nat.add_0_r.
      destruct (Nat.leb_spec k p); destruct (Nat.ltb_spec p k); simpl; try reflexivity; lia.
    - cbn [enum filter fst]. specialize (IH (S k) p).
      destruct (Nat.eqb_spec k p) as [E | E].
      + subst p. cbn [length]. rewrite IH.
        destruct (Nat.leb_spec (S k) k); [lia |].
        destruct (Nat.leb_spec k k); [| lia].
        destruct (Nat.ltb_spec k (k + S (length r))); [reflexivity | lia].
      + rewrite IH. cbn [length].
        destruct (Nat.leb_spec (S k) p); destruct (Nat.ltb_spec p (S k + length r));
          destruct (Nat.leb_spec k p); destruct (Nat.ltb_spec p (k + S (length r)));
          simpl; try reflexivity; lia.
  Qed.

  (* ── 6. Headline 3: the incoming record binds exactly once ──────────── *)

  (* Headline 3 (decision 28). Every produce-side fire binds its incoming
     record at exactly one position: the position p that the search
     returns. The search tries at most m_W(c_x) positions, one for each bind
     of W on the collection of x. *)
  Theorem join_incoming_binds_once :
    forall W x v p sg,
      search_produce W x v = Some (p, sg) ->
      length (filter (is_x x) sg) = 1 /\
      (exists m, nth_error sg p = Some (x, m)) /\
      length (incoming_positions W x) <= mult W (r_coll x).
  Proof.
    intros W x v p sg H. unfold search_produce in H.
    apply search_at_some in H as (i & b & m & Hin & _ & _ & Hm & Hf).
    pose proof (fill_shape _ _ _ _ Hf) as Hr.
    pose proof (incoming_base_sub W) as [Hsub Hnd].
    assert (Hin' : In (p, (i, b)) (enum 0 (gpairs W))).
    { unfold incoming_positions in Hin. apply filter_In in Hin as [Hin _]. exact (Hsub _ Hin). }
    pose proof (enum_gpairs_nth W p i b Hin') as Hnth.
    split; [| split].
    - rewrite (count_x_slots x _ _ Hr).
      + set (F := fun kib : nat * (nat * bnd) =>
                    let b0 := snd (snd kib) in
                    match forced W x p (fst kib) b0 with
                    | Some m0 => Forced x m0
                    | None => Pick (not_x x (visible b0 (win_in W x (b_coll b0) v))) (b_pat b0)
                    end).
        (* x is forced at the position p and at no other position *)
        assert (Hfx : forall kib, In kib (enum 0 (gpairs W)) -> forced_x x (F kib) = (fst kib =? p)).
        { intros [k [i0 b0]] Hk. unfold F. cbn [fst snd]. unfold forced.
          destruct (k =? p) eqn:E.
          - apply Nat.eqb_eq in E. subst k. pose proof (enum_gpairs_nth W p i0 b0 Hk) as Hp0.
            rewrite Hnth in Hp0. injection Hp0 as <- <-. rewrite Hm. cbn [forced_x]. apply Nat.eqb_refl.
          - reflexivity. }
        change (slots_prod W x p v) with (map F (enum 0 (gpairs W))).
        rewrite filter_map_length, (filter_ext_in _ _ _ Hfx), enum_fst_count.
        apply enum_nth in Hin' as [_ Hn]. rewrite Nat.sub_0_r in Hn.
        assert (p < length (gpairs W)) by (apply nth_error_Some; rewrite Hn; discriminate).
        destruct (Nat.leb_spec 0 p); [| lia].
        destruct (Nat.ltb_spec p (0 + length (gpairs W))); [reflexivity | lia].
      + intros L q e HinL HeL. unfold slots_prod in HinL. apply in_map_iff in HinL as ([k [i0 b0]] & Hs & _).
        cbn [fst snd] in Hs. destruct (forced W x p k b0); [discriminate |].
        injection Hs as <- _. exact (not_x_excludes _ _ _ HeL).
    - pose proof (slots_prod_nth W x p v p i b Hnth) as Hs.
      unfold forced in Hs. rewrite Nat.eqb_refl, Hm in Hs.
      destruct (slots_rel_nth_some _ _ _ _ Hr Hs) as [rm Hrm].
      pose proof (slots_rel_nth _ _ _ _ _ Hr Hs Hrm) as Hx. cbn in Hx. subst rm.
      exists m. exact Hrm.
    - unfold incoming_positions. rewrite <- positions_on_count.
      apply filter_length_incl; [exact Hnd | exact Hsub].
  Qed.

  (* The produce search forces x at one position at most. *)
  Definition forced_tags (ss : list slot) : list (nat * nat) :=
    flat_map (fun s => match s with Forced e _ => [tag e] | Pick _ _ => [] end) ss.

  Lemma slots_prod_forced_tags :
    forall W x p v, incl (forced_tags (slots_prod W x p v)) [tag x] /\ NoDup (forced_tags (slots_prod W x p v)).
  Proof.
    intros W x p v. unfold slots_prod.
    assert (G : forall l, NoDup (map fst l) ->
              incl (forced_tags (map (fun kib : nat * (nat * bnd) =>
                       let b := snd (snd kib) in
                       match forced W x p (fst kib) b with
                       | Some m => Forced x m
                       | None => Pick (not_x x (visible b (win_in W x (b_coll b) v))) (b_pat b)
                       end) l)) [tag x] /\
              NoDup (forced_tags (map (fun kib : nat * (nat * bnd) =>
                       let b := snd (snd kib) in
                       match forced W x p (fst kib) b with
                       | Some m => Forced x m
                       | None => Pick (not_x x (visible b (win_in W x (b_coll b) v))) (b_pat b)
                       end) l)) /\
              (forall t, In t (forced_tags (map (fun kib : nat * (nat * bnd) =>
                       let b := snd (snd kib) in
                       match forced W x p (fst kib) b with
                       | Some m => Forced x m
                       | None => Pick (not_x x (visible b (win_in W x (b_coll b) v))) (b_pat b)
                       end) l)) -> In p (map fst l))).
    { induction l as [| [k [i b]] r IH]; intros Hnd; [split; [intros t [] | split; [constructor | intros t []]] |].
      simpl in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd]. destruct (IH Hnd) as (Hi & Hn & Hp).
      cbn [map forced_tags flat_map fst snd] in *.
      destruct (forced W x p k b) as [m |] eqn:Ef.
      - assert (k = p) by (unfold forced in Ef; destruct (k =? p) eqn:E; [apply Nat.eqb_eq in E; exact E | discriminate]).
        subst k. cbn [app]. split; [| split].
        + intros t [<- | Ht]; [left; reflexivity | exact (Hi t Ht)].
        + constructor; [| exact Hn]. intros Hin. exact (Hnot (Hp _ Hin)).
        + intros t _. left. reflexivity.
      - cbn [app]. split; [exact Hi | split; [exact Hn | intros t Ht; right; exact (Hp t Ht)]]. }
    destruct (G (enum 0 (gpairs W)) (enum_NoDup _ _ _)) as (Hi & Hn & _). split; [exact Hi | exact Hn].
  Qed.

  (* The tags that a filled search binds. *)
  Definition bound_tags (sg : list (rec * M)) : list (nat * nat) := map (fun rm => tag (fst rm)) sg.

  (* ── 7. Headline 5: the binds of one fire bind distinct records ─────── *)

  (* Headline 5 (decision 28). The binds of one fire bind pairwise distinct
     records, persistent ones included, and for sets pairwise distinct
     classes. A search never binds a claimed tag: a forced record carries a
     tag that the search claimed at its start, and each pick claims its own.
     So GREEDY binds distinct tags, and SEARCH-PRODUCE binds the incoming
     record and stored records with tags other than its tag. *)
  Theorem join_repeated_channel_distinct_records :
    (forall g ss C sg,
       fill g C ss = Some sg -> incl (forced_tags ss) C -> NoDup (forced_tags ss) ->
       NoDup (bound_tags sg) /\ (forall rm, In rm sg -> In (tag (fst rm)) C -> In (tag (fst rm)) (forced_tags ss))) /\
    (forall W v sg, greedy W v = Some sg -> NoDup (bound_tags sg) /\ NoDup (map fst sg)) /\
    (forall W x v p sg, search_produce W x v = Some (p, sg) -> NoDup (bound_tags sg) /\ NoDup (map fst sg)).
  Proof.
    (* A pick never binds a claimed record or class. *)
    assert (Htc : forall g p C k cs sg,
              try_cands g p C k cs = Some sg ->
              exists e m sg', sg = (e, m) :: sg' /\ claimed C e = false /\ k (tag e :: C) = Some sg').
    { intros g p C k cs. induction cs as [| e more IH]; intros sg H; cbn [try_cands] in H; [discriminate |].
      destruct (claimed C e) eqn:Ec; [exact (IH _ H) |].
      destruct (pick_match g p e) as [m |]; [| exact (IH _ H)].
      destruct (k (tag e :: C)) as [s |] eqn:Ek.
      - injection H as <-. exists e, m, s. split; [reflexivity | split; [exact Ec | exact Ek]].
      - destruct (on_dead_end_cases _ (try_cands g p C k more)) as [Ed | Ed]; rewrite Ed in H;
          [discriminate | exact (IH _ H)]. }
    assert (Hfill : forall g ss C sg,
              fill g C ss = Some sg -> incl (forced_tags ss) C -> NoDup (forced_tags ss) ->
              NoDup (bound_tags sg) /\
              (forall rm, In rm sg -> In (tag (fst rm)) C -> In (tag (fst rm)) (forced_tags ss))).
    { intros g ss. induction ss as [| s rest IH]; intros C sg H Hi Hn; cbn [fill] in H.
      - injection H as <-. split; [constructor | intros rm []].
      - destruct s as [L p | e m].
        + apply Htc in H as (e & m & sg' & -> & Ec & Hk).
          cbn [forced_tags flat_map app] in Hi, Hn |- *.
          assert (Hi' : incl (forced_tags rest) (tag e :: C)) by (intros t Ht; right; exact (Hi t Ht)).
          destruct (IH _ _ Hk Hi' Hn) as [Hnd Hc].
          apply tmem_false in Ec.
          split.
          * cbn [bound_tags map fst]. constructor; [| exact Hnd].
            intros Hin. apply in_map_iff in Hin as (rm & Ht & Hrm).
            pose proof (Hc rm Hrm (or_introl (eq_sym Ht))) as Hf. rewrite Ht in Hf. exact (Ec (Hi _ Hf)).
          * intros rm [<- | Hrm] HC; [cbn [fst] in HC; contradiction |].
            exact (Hc rm Hrm (or_intror HC)).
        + destruct (fill g C rest) as [sg' |] eqn:Hf; [| discriminate]. injection H as <-.
          cbn [forced_tags flat_map app] in Hi, Hn |- *.
          apply NoDup_cons_iff in Hn as [Hnot Hn].
          assert (Hi' : incl (forced_tags rest) C) by (intros t Ht; exact (Hi t (or_intror Ht))).
          destruct (IH _ _ Hf Hi' Hn) as [Hnd Hc].
          split.
          * cbn [bound_tags map fst]. constructor; [| exact Hnd].
            intros Hin. apply in_map_iff in Hin as (rm & Ht & Hrm).
            assert (HC : In (tag (fst rm)) C) by (rewrite Ht; exact (Hi _ (or_introl eq_refl))).
            apply Hnot. rewrite <- Ht. exact (Hc rm Hrm HC).
          * intros rm [<- | Hrm] HC; [left; reflexivity | right; exact (Hc rm Hrm HC)]. }
    split; [exact Hfill |]. split.
    - intros W v sg H. unfold greedy in H.
      assert (Hnf : forced_tags (slots_cons W v) = []).
      { unfold slots_cons. induction (gpairs W) as [| ib r IHr]; [reflexivity | exact IHr]. }
      destruct (Hfill _ _ _ _ H) as [Hnd _]; [rewrite Hnf; intros t [] | rewrite Hnf; constructor |].
      split; [exact Hnd | apply (NoDup_map_inv (fun e => tag e)); unfold bound_tags in Hnd; rewrite map_map; exact Hnd].
    - intros W x v p sg H. unfold search_produce in H.
      apply search_at_some in H as (i & b & m & _ & _ & _ & _ & Hf).
      destruct (slots_prod_forced_tags W x p v) as [Hi Hn].
      destruct (Hfill _ _ _ _ Hf Hi Hn) as [Hnd _].
      split; [exact Hnd | apply (NoDup_map_inv (fun e => tag e)); unfold bound_tags in Hnd; rewrite map_map; exact Hnd].
  Qed.

  (* ── 8. Headline 17: greedy can miss an assignment ─────────────────── *)

  (* An assignment of the binds of W: each group position binds a record of
     its lane-visible window that matches its pattern spatially, and the
     bound tags are pairwise distinct (decision 28). *)
  Fixpoint slots_match (ss : list slot) (sg : list (rec * M)) : Prop :=
    match ss with
    | [] => sg = []
    | s :: ss' =>
        match sg with
        | [] => False
        | (e, m) :: sg' =>
            match s with
            | Pick L p => In e L /\ spatial p (r_val e) = Some m
            | Forced e' m' => e = e' /\ m = m'
            end /\ slots_match ss' sg'
        end
    end.

  Definition assignment (W : wtr) (v : view) (sg : list (rec * M)) : Prop :=
    slots_match (slots_cons W v) sg /\ NoDup (bound_tags sg).

  Lemma pick_match_no_guard : forall p e, pick_match no_guard p e = spatial p (r_val e).
  Proof.
    intros p e. unfold pick_match. destruct (spatial p (r_val e)) eqn:E; try rewrite no_guard_admits; reflexivity.
  Qed.

  Lemma claimed_self : forall e C, claimed (tag e :: C) e = true.
  Proof. intros e C. unfold claimed. apply tmem_spec. left. reflexivity. Qed.

  Lemma claimed_other : forall e e', tag e <> tag e' -> claimed [tag e'] e = false.
  Proof.
    intros e e' H. unfold claimed. apply tmem_false. intros [Ht | []]. apply H. symmetry. exact Ht.
  Qed.

  (* Headline 17 (decision 37 until P12, a computed negative result). On a
     repeated channel, the greedy search parks a waiter although an
     assignment exists: the binds [c, c] with the patterns P0 = {r1, r2} and
     P1 = {r1} over the window [r1; r2]. Greedy binds r1 to P0 and then
     finds no record for P1, but r2 → P0 and r1 → P1 is an assignment. So
     the rule of no pending match is relative to the search until P12. *)
  Theorem greedy_join_can_miss_assignment :
    forall W v c b0 b1 r1 r2 m10 m20 m11,
      w_binds W = [b0; b1] -> b_coll b0 = c -> b_coll b1 = c -> w_guard W = no_guard ->
      visible b0 (win W c v) = [r1; r2] -> visible b1 (win W c v) = [r1; r2] ->
      tag r1 <> tag r2 ->
      spatial (b_pat b0) (r_val r1) = Some m10 -> spatial (b_pat b0) (r_val r2) = Some m20 ->
      spatial (b_pat b1) (r_val r1) = Some m11 -> spatial (b_pat b1) (r_val r2) = None ->
      greedy W v = None /\ receive_decision W v = None /\ assignment W v [(r2, m20); (r1, m11)].
  Proof.
    intros W v c b0 b1 r1 r2 m10 m20 m11 Hb Hc0 Hc1 Hg Hw0 Hw1 Ht H10 H20 H11 H21.
    assert (Hgp : gpairs W = [(0, b0); (1, b1)]).
    { unfold gpairs. rewrite Hb. cbn [enum isort fold_right ins]. unfold gle. cbn [snd fst].
      rewrite Hc0, Hc1, Nat.ltb_irrefl, Nat.eqb_refl. reflexivity. }
    assert (Hs : slots_cons W v = [Pick [r1; r2] (b_pat b0); Pick [r1; r2] (b_pat b1)]).
    { unfold slots_cons. rewrite Hgp. cbn [map snd]. rewrite Hc0, Hc1, Hw0, Hw1. reflexivity. }
    assert (Hgr : greedy W v = None).
    { unfold greedy. rewrite Hs, Hg. cbn [fill try_cands].
      replace (claimed [] r1) with false by reflexivity.
      rewrite pick_match_no_guard, H10. cbn [fill try_cands].
      rewrite claimed_self, (claimed_other r2 r1 (fun E => Ht (eq_sym E))), pick_match_no_guard, H21.
      cbn [try_cands]. unfold on_dead_end. reflexivity. }
    split; [exact Hgr |]. split; [unfold receive_decision; rewrite Hgr; reflexivity |].
    split.
    - rewrite Hs. cbn [slots_match]. split; [split; [right; left; reflexivity | exact H20] |].
      split; [split; [left; reflexivity | exact H11] | reflexivity].
    - cbn [bound_tags map fst]. constructor; [intros [E | []]; exact (Ht E) |].
      constructor; [intros [] | constructor].
  Qed.

  (* ── 9. The search without its continuations ──────────────────────── *)

  (* The pick of one slot: the first unclaimed candidate that matches. *)
  Fixpoint first_pick (g : Gd) (C : list (nat * nat)) (p : P) (L : list rec) : option (rec * M) :=
    match L with
    | [] => None
    | e :: more =>
        if claimed C e then first_pick g C p more else
        match pick_match g p e with Some m => Some (e, m) | None => first_pick g C p more end
    end.

  Fixpoint gfill (g : Gd) (C : list (nat * nat)) (ss : list slot) : option (list (rec * M)) :=
    match ss with
    | [] => Some []
    | Forced e m :: rest => option_map (cons (e, m)) (gfill g C rest)
    | Pick L p :: rest =>
        match first_pick g C p L with
        | Some (e, m) => option_map (cons (e, m)) (gfill g (tag e :: C) rest)
        | None => None
        end
    end.

  (* With no backtracking, the search with continuations is the pick of each
     slot in group order. *)
  Lemma fill_gfill : forall g ss C, fill g C ss = gfill g C ss.
  Proof.
    intros g ss. induction ss as [| s rest IH]; intros C; [reflexivity |].
    destruct s as [L p | e m]; cbn [fill gfill].
    - induction L as [| e more IHL]; [reflexivity |]. cbn [try_cands first_pick].
      destruct (claimed C e); [exact IHL |].
      destruct (pick_match g p e) as [m |]; [| exact IHL].
      rewrite IH. destruct (gfill g (tag e :: C) rest); reflexivity.
    - rewrite IH. destruct (gfill g C rest); reflexivity.
  Qed.

  (* The reference search of decision 37 (HEAD's algorithm until P12): the
     first spatial match per bind in group order, no backtracking, and no
     guard during the search. *)
  Fixpoint first_fit (C : list (nat * nat)) (p : P) (L : list rec) : option (rec * M) :=
    match L with
    | [] => None
    | e :: more =>
        if claimed C e then first_fit C p more else
        match spatial p (r_val e) with Some m => Some (e, m) | None => first_fit C p more end
    end.

  Fixpoint ref_fill (C : list (nat * nat)) (ss : list slot) : option (list (rec * M)) :=
    match ss with
    | [] => Some []
    | Forced e m :: rest => option_map (cons (e, m)) (ref_fill C rest)
    | Pick L p :: rest =>
        match first_fit C p L with
        | Some (e, m) => option_map (cons (e, m)) (ref_fill (tag e :: C) rest)
        | None => None
        end
    end.

  Lemma first_fit_some :
    forall C p L e m,
      first_fit C p L = Some (e, m) ->
      In e L /\ claimed C e = false /\ spatial p (r_val e) = Some m.
  Proof.
    intros C p L. induction L as [| e0 more IH]; intros e m H; [discriminate |].
    cbn [first_fit] in H. destruct (claimed C e0) eqn:Ec.
    - destruct (IH _ _ H) as (Hi & Hc & Hs). split; [right; exact Hi | split; assumption].
    - destruct (spatial p (r_val e0)) as [m0 |] eqn:Es.
      + injection H as <- <-. split; [left; reflexivity | split; assumption].
      + destruct (IH _ _ H) as (Hi & Hc & Hs). split; [right; exact Hi | split; assumption].
  Qed.

  Lemma ref_fill_match : forall ss C sg, ref_fill C ss = Some sg -> slots_match ss sg.
  Proof.
    induction ss as [| s rest IH]; intros C sg H; cbn [ref_fill] in H.
    - injection H as <-. reflexivity.
    - destruct s as [L p | e m].
      + destruct (first_fit C p L) as [[e m] |] eqn:Ef; [| discriminate].
        destruct (ref_fill (tag e :: C) rest) as [sg' |] eqn:Hr; [| discriminate].
        injection H as <-. destruct (first_fit_some _ _ _ _ _ Ef) as (Hi & _ & Hs).
        cbn [slots_match]. split; [split; assumption | exact (IH _ _ Hr)].
      + destruct (ref_fill C rest) as [sg' |] eqn:Hr; [| discriminate].
        injection H as <-. cbn [slots_match]. split; [split; reflexivity | exact (IH _ _ Hr)].
  Qed.

  (* ── 10. Headline 1: the consume-side search is HEAD's greedy ───────── *)

  (* Headline 1 (decision 37). The search of a join is the reference greedy
     search: for each group position in turn, the first unclaimed record of
     the lane-visible window of its collection that matches its pattern
     spatially, with no backtracking and no guard. GREEDY is this search
     over the windows of W. Soundness: every bound record lies in its window
     and matches its bind spatially, and the bound tags are distinct. The
     receive fires exactly when GREEDY succeeds and the guard admits the
     values in bind order: one guard check, over all binds, and a refusal
     parks. *)
  Theorem join_search_is_head_greedy :
    (forall g C ss, fill g C ss = ref_fill C ss) /\
    (forall W v, greedy W v = ref_fill [] (slots_cons W v)) /\
    (forall W v sg, greedy W v = Some sg -> assignment W v sg) /\
    (forall W v sg,
       receive_decision W v = Some sg <->
       greedy W v = Some sg /\ admits (w_guard W) (in_bind_order W (map snd sg)) = true).
  Proof.
    assert (Hpick : forall g C p L, first_pick g C p L = first_fit C p L).
    { intros g C p L. induction L as [| e more IH]; [reflexivity |]. cbn [first_pick first_fit].
      destruct (claimed C e); [exact IH |]. unfold pick_match.
      destruct (spatial p (r_val e)); [reflexivity | exact IH]. }
    assert (Hgr : forall g ss C, gfill g C ss = ref_fill C ss).
    { intros g ss. induction ss as [| s rest IH]; intros C; [reflexivity |].
      destruct s as [L p | e m]; cbn [gfill ref_fill].
      - rewrite Hpick. destruct (first_fit C p L) as [[e m] |]; [rewrite IH |]; reflexivity.
      - rewrite IH. reflexivity. }
    assert (Hfr : forall g C ss, fill g C ss = ref_fill C ss) by (intros g C ss; rewrite fill_gfill; apply Hgr).
    split; [exact Hfr |]. split; [intros W v; unfold greedy; apply Hfr |]. split.
    - intros W v sg H. split.
      + unfold greedy in H. rewrite Hfr in H. exact (ref_fill_match _ _ _ H).
      + exact (proj1 (proj1 (proj2 join_repeated_channel_distinct_records) W v sg H)).
    - intros W v sg. unfold receive_decision, guard_ok. destruct (greedy W v) as [sg' |].
      + destruct (admits (w_guard W) (in_bind_order W (map snd sg'))) eqn:Ea.
        * split; [intros H; injection H as <-; split; [reflexivity | exact Ea] |].
          intros [H _]. exact H.
        * split; [discriminate |]. intros [H Hb]. injection H as <-. rewrite Ea in Hb. discriminate.
      + split; [discriminate | intros [H _]; discriminate].
  Qed.

  (* ── 11. Windows and the incoming record ──────────────────────────── *)

  Lemma wsel_incl : forall lim key seen l, incl (wsel lim key seen l) l.
  Proof.
    intros lim key seen l. revert seen. induction l as [| e r IH]; intros seen; [intros a [] |].
    cbn [wsel]. destruct (count_occ Nat.eq_dec seen (key e) <? lim).
    - intros a [<- | Ha]; [left; reflexivity | right; exact (IH _ a Ha)].
    - intros a Ha. right. exact (IH _ a Ha).
  Qed.

  Lemma window_incl : forall m l, incl (window m l) l.
  Proof.
    intros m l. unfold window. destruct sh.
    - intros a Ha. rewrite <- (firstn_skipn m l). apply in_or_app. left. exact Ha.
    - apply wsel_incl.
    - apply incl_refl.
    - apply wsel_incl.
  Qed.

  (* A window of one representative per class holds each class once. *)
  Lemma wsel1_keys :
    forall key l seen,
      NoDup (map key (wsel 1 key seen l)) /\ (forall e, In e (wsel 1 key seen l) -> ~ In (key e) seen).
  Proof.
    intros key l. induction l as [| e r IH]; intros seen; [split; [constructor | intros a []] |].
    cbn [wsel]. destruct (count_occ Nat.eq_dec seen (key e) <? 1) eqn:Ec.
    - apply Nat.ltb_lt in Ec. assert (Hnot : ~ In (key e) seen).
      { intros Hin. apply (count_occ_In Nat.eq_dec) in Hin. lia. }
      destruct (IH (key e :: seen)) as [Hnd Hout]. split.
      + cbn [map]. constructor; [| exact Hnd].
        intros Hin. apply in_map_iff in Hin as (a & Ha & Hin). apply (Hout a Hin). left. exact (eq_sym Ha).
      + intros a [<- | Ha]; [exact Hnot |]. intros Hs. apply (Hout a Ha). right. exact Hs.
    - destruct (IH (key e :: seen)) as [Hnd Hout]. split; [exact Hnd |].
      intros a Ha Hs. apply (Hout a Ha). right. exact Hs.
  Qed.

  Lemma nodup_map_inj :
    forall (X Y : Type) (f : X -> Y) l a b, NoDup (map f l) -> In a l -> In b l -> f a = f b -> a = b.
  Proof.
    intros X Y f l. induction l as [| c r IH]; intros a b Hnd Ha Hb E; [destruct Ha |].
    cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd].
    destruct Ha as [<- | Ha]; destruct Hb as [<- | Hb]; try reflexivity.
    - exfalso. apply Hnot. rewrite E. apply in_map. exact Hb.
    - exfalso. apply Hnot. rewrite <- E. apply in_map. exact Ha.
    - exact (IH a b Hnd Ha Hb E).
  Qed.

  Lemma recs_of_store :
    forall x v c, recs_of c (store_view x v) = if c =? r_coll x then x :: recs_of c v else recs_of c v.
  Proof.
    intros x v c. unfold recs_of, store_view. cbn [v_recs filter].
    destruct (Nat.eqb_spec (r_coll x) c) as [E | E]; destruct (Nat.eqb_spec c (r_coll x)) as [E' | E'];
      try reflexivity; congruence.
  Qed.

  Lemma win_store : forall W x v c, win W c (store_view x v) = win_in W x c v.
  Proof. intros W x v c. unfold win, win_in. rewrite recs_of_store. reflexivity. Qed.

  Lemma win_in_mem :
    forall W x c v e,
      In e (win_in W x c v) -> (e = x /\ c = r_coll x) \/ (In e (v_recs v) /\ r_coll e = c).
  Proof.
    intros W x c v e H. unfold win_in in H. apply window_incl in H.
    apply (Permutation_in _ (rorder_perm _)) in H.
    destruct (c =? r_coll x) eqn:E.
    - destruct H as [<- | H]; [left; split; [reflexivity | apply Nat.eqb_eq in E; exact E] |].
      right. unfold recs_of in H. apply filter_In in H as [Hi Hc]. apply Nat.eqb_eq in Hc. split; assumption.
    - right. unfold recs_of in H. apply filter_In in H as [Hi Hc]. apply Nat.eqb_eq in Hc. split; assumption.
  Qed.

  (* In the windows with x placed, only x carries the tag of x: another
     record of the collection of x has another identity, and in a set the
     window holds one representative per class. *)
  Lemma window_tag :
    forall W x v c e',
      (forall e, In e (v_recs v) -> r_id e <> r_id x) ->
      In e' (win_in W x c v) -> r_id e' <> r_id x -> In x (win_in W x (r_coll x) v) -> tag e' <> tag x.
  Proof.
    intros W x v c e' Hfresh He Hne Hx Ht.
    destruct (win_in_mem W x c v e' He) as [[-> _] | [Hin Hc]]; [exact (Hne eq_refl) |].
    unfold tag in Ht. destruct sh eqn:Esh; injection Ht as Ecoll Ekey; try exact (Hne Ekey).
    (* a set: x and e' are two representatives of one class *)
    subst c. rewrite Ecoll in He.
    unfold win_in, window in He, Hx. rewrite Esh in He, Hx.
    destruct (wsel1_keys class_key
                (rorder (if r_coll x =? r_coll x then x :: recs_of (r_coll x) v else recs_of (r_coll x) v)) [])
      as [Hnd _].
    pose proof (nodup_map_inj _ _ _ _ _ _ Hnd He Hx Ekey) as E. subst e'. exact (Hne eq_refl).
  Qed.

  Lemma in_win_spec :
    forall W x v,
      (forall e, In e (v_recs v) -> r_id e <> r_id x) ->
      (in_win x (win_in W x (r_coll x) v) = true <-> In x (win_in W x (r_coll x) v)).
  Proof.
    intros W x v Hfresh. unfold in_win. rewrite existsb_exists. split.
    - intros (e & He & E). apply Nat.eqb_eq in E.
      destruct (win_in_mem W x (r_coll x) v e He) as [[-> _] | [Hin _]]; [exact He |].
      exfalso. exact (Hfresh e Hin E).
    - intros H. exists x. split; [exact H | apply Nat.eqb_refl].
  Qed.

  (* A record of a window with x placed lies in the window that the produce
     search checks. *)
  Lemma in_win_x_in_win : forall W x c v, in_win x (win_in W x c v) = true -> x_in_win W x c v = true.
  Proof. intros W x c v H. unfold x_in_win. destruct sh; [exact H | exact H | reflexivity | reflexivity]. Qed.

  (* ── 12. The simulation of the produce search ────────────────────────── *)

  Lemma not_x_cons :
    forall x e l, not_x x (e :: l) = if r_id e =? r_id x then not_x x l else e :: not_x x l.
  Proof. intros x e l. unfold not_x. cbn [filter]. destruct (r_id e =? r_id x); reflexivity. Qed.

  Lemma not_x_idem : forall x l, not_x x (not_x x l) = not_x x l.
  Proof.
    intros x l. induction l as [| e r IH]; [reflexivity |]. rewrite not_x_cons.
    destruct (r_id e =? r_id x) eqn:E; [exact IH |]. rewrite not_x_cons, E, IH. reflexivity.
  Qed.

  Lemma not_x_head : forall x l, not_x x (x :: l) = not_x x l.
  Proof. intros x l. rewrite not_x_cons, Nat.eqb_refl. reflexivity. Qed.

  (* A pick that does not bind x binds the same record from the list without
     x, under claims that agree away from x. *)
  Lemma first_fit_drop_x :
    forall x C1 C2 p L e m,
      first_fit C1 p L = Some (e, m) -> r_id e <> r_id x ->
      (forall e', In e' L -> r_id e' <> r_id x -> claimed C1 e' = claimed C2 e') ->
      first_fit C2 p (not_x x L) = Some (e, m).
  Proof.
    intros x C1 C2 p L. induction L as [| e0 more IH]; intros e m H Hne Hc; [discriminate |].
    assert (Hc' : forall e', In e' more -> r_id e' <> r_id x -> claimed C1 e' = claimed C2 e')
      by (intros e' Hi Hn; exact (Hc e' (or_intror Hi) Hn)).
    cbn [first_fit] in H. rewrite not_x_cons.
    destruct (r_id e0 =? r_id x) eqn:Ex.
    - apply Nat.eqb_eq in Ex.
      destruct (claimed C1 e0); [exact (IH _ _ H Hne Hc') |].
      destruct (spatial p (r_val e0)) as [m0 |].
      + injection H as <- <-. contradiction.
      + exact (IH _ _ H Hne Hc').
    - apply Nat.eqb_neq in Ex. cbn [first_fit]. rewrite <- (Hc e0 (or_introl eq_refl) Ex).
      destruct (claimed C1 e0); [exact (IH _ _ H Hne Hc') |].
      destruct (spatial p (r_val e0)) as [m0 |]; [exact H | exact (IH _ _ H Hne Hc')].
  Qed.

  (* The relation of one slot of a search that may bind x with the slot of
     the produce search at the same group position. *)
  Definition sim1 (x : rec) (s1 s2 : slot) (rm : rec * M) : Prop :=
    match s1 with
    | Pick L p =>
        match s2 with
        | Pick L' p' => L' = not_x x L /\ p' = p /\ r_id (fst rm) <> r_id x
        | Forced e m => e = x /\ rm = (x, m)
        end
    | Forced _ _ => False
    end.

  Fixpoint sim_slots (x : rec) (ss1 ss2 : list slot) (sg : list (rec * M)) : Prop :=
    match ss1, ss2, sg with
    | [], [], [] => True
    | s1 :: r1, s2 :: r2, rm :: rs => sim1 x s1 s2 rm /\ sim_slots x r1 r2 rs
    | _, _, _ => False
    end.

  Lemma sim_slots_nth :
    forall x ss1 ss2 sg,
      length ss1 = length ss2 -> length ss2 = length sg ->
      (forall i s1 s2 rm, nth_error ss1 i = Some s1 -> nth_error ss2 i = Some s2 -> nth_error sg i = Some rm ->
                          sim1 x s1 s2 rm) ->
      sim_slots x ss1 ss2 sg.
  Proof.
    intros x ss1. induction ss1 as [| s1 r1 IH]; intros ss2 sg L1 L2 H.
    - destruct ss2; [destruct sg; [exact I | discriminate] | discriminate].
    - destruct ss2 as [| s2 r2]; [discriminate |]. destruct sg as [| rm rs]; [discriminate |].
      cbn [length] in L1, L2. split.
      + exact (H 0 s1 s2 rm eq_refl eq_refl eq_refl).
      + apply IH; [lia | lia |]. intros i a b c Ha Hb Hc. exact (H (S i) a b c Ha Hb Hc).
  Qed.

  Definition pick_lists (ss : list slot) : list rec :=
    flat_map (fun s => match s with Pick L _ => L | Forced _ _ => [] end) ss.

  Lemma ref_fill_sim :
    forall x ss1 ss2 sg C1 C2,
      sim_slots x ss1 ss2 sg ->
      (forall t, t <> tag x -> (In t C1 <-> In t C2)) ->
      (forall e', In e' (pick_lists ss1) -> r_id e' <> r_id x -> tag e' <> tag x) ->
      ref_fill C1 ss1 = Some sg -> ref_fill C2 ss2 = Some sg.
  Proof.
    intros x ss1. induction ss1 as [| s1 r1 IH]; intros ss2 sg C1 C2 Hs Hc Ht H.
    - destruct ss2; destruct sg; try contradiction. reflexivity.
    - destruct ss2 as [| s2 r2]; [destruct sg; contradiction |].
      destruct sg as [| rm rs]; [contradiction |]. destruct Hs as [H1 Hs].
      destruct s1 as [L p | e0 m0]; [| contradiction].
      cbn [ref_fill] in H. destruct (first_fit C1 p L) as [[e m] |] eqn:Ef; [| discriminate].
      destruct (ref_fill (tag e :: C1) r1) as [rs' |] eqn:Hr; [| discriminate].
      cbn [option_map] in H. injection H as Erm Ers. subst rm rs'.
      assert (Ht' : forall e', In e' (pick_lists r1) -> r_id e' <> r_id x -> tag e' <> tag x).
      { intros e' Hi Hn. apply Ht; [| exact Hn]. cbn [pick_lists flat_map]. apply in_or_app. right. exact Hi. }
      destruct s2 as [L' p' | e2 m2].
      + destruct H1 as (-> & -> & Hne). cbn [fst] in Hne. cbn [ref_fill].
        assert (Hcl : forall e', In e' L -> r_id e' <> r_id x -> claimed C1 e' = claimed C2 e').
        { intros e' Hi Hn. unfold claimed.
          assert (Hte : tag e' <> tag x).
          { apply Ht; [cbn [pick_lists flat_map]; apply in_or_app; left; exact Hi | exact Hn]. }
          destruct (tmem (tag e') C1) eqn:E1, (tmem (tag e') C2) eqn:E2; try reflexivity.
          - apply tmem_spec in E1. apply (proj1 (Hc _ Hte)) in E1. apply tmem_spec in E1.
            rewrite E1 in E2. discriminate.
          - apply tmem_spec in E2. apply (proj2 (Hc _ Hte)) in E2. apply tmem_spec in E2.
            rewrite E2 in E1. discriminate. }
        rewrite (first_fit_drop_x x C1 C2 p L e m Ef Hne Hcl). cbn [option_map].
        rewrite (IH r2 rs (tag e :: C1) (tag e :: C2) Hs); [reflexivity | | exact Ht' | exact Hr].
        intros t Htx. specialize (Hc t Htx). cbn [In]. tauto.
      + destruct H1 as (-> & Erm). injection Erm as -> ->. cbn [ref_fill option_map].
        rewrite (IH r2 rs (tag x :: C1) C2 Hs); [reflexivity | | exact Ht' | exact Hr].
        intros t Htx. specialize (Hc t Htx). cbn [In]. split.
        * intros [E | Hin]; [exfalso; exact (Htx (eq_sym E)) | exact (proj1 Hc Hin)].
        * intros Hin. right. exact (proj2 Hc Hin).
  Qed.

  (* A search binds x first at a position whose list starts with x and whose
     pattern matches x: no earlier such position exists. *)
  Lemma ref_fill_head_first :
    forall x ss C sg k m,
      ref_fill C ss = Some sg -> nth_error sg k = Some (x, m) -> NoDup (map fst sg) ->
      ~ In (tag x) C ->
      (forall e', In e' (pick_lists ss) -> r_id e' <> r_id x -> tag e' <> tag x) ->
      (forall e', In e' (pick_lists ss) -> r_id e' = r_id x -> e' = x) ->
      forall i rest p, i < k -> nth_error ss i = Some (Pick (x :: rest) p) -> spatial p (r_val x) = None.
  Proof.
    intros x ss. induction ss as [| s0 r IH]; intros C sg k m H Hk Hnd Hc Ht Hu i rest p Hi Hs;
      [destruct i; discriminate |].
    assert (Ht' : forall e', In e' (pick_lists r) -> r_id e' <> r_id x -> tag e' <> tag x).
    { intros e' Hin Hn. apply Ht; [| exact Hn]. cbn [pick_lists flat_map]. apply in_or_app. right. exact Hin. }
    assert (Hu' : forall e', In e' (pick_lists r) -> r_id e' = r_id x -> e' = x).
    { intros e' Hin Hn. apply Hu; [| exact Hn]. cbn [pick_lists flat_map]. apply in_or_app. right. exact Hin. }
    cbn [ref_fill] in H. destruct s0 as [L0 p0 | e0 m0].
    - destruct (first_fit C p0 L0) as [[e0 m0] |] eqn:Ef; [| discriminate].
      destruct (ref_fill (tag e0 :: C) r) as [sg' |] eqn:Hr; [| discriminate].
      cbn [option_map] in H. injection H as <-.
      destruct (first_fit_some _ _ _ _ _ Ef) as (Hin0 & _ & _).
      (* the first pick is not x, since x is bound at k *)
      assert (Hne : e0 <> x).
      { intros ->. destruct k as [| k]; [lia |]. cbn [nth_error] in Hk.
        cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hnot _]. apply Hnot.
        apply in_map_iff. exists (x, m). split; [reflexivity | exact (nth_error_In _ _ Hk)]. }
      destruct i as [| i].
      + cbn [nth_error] in Hs. injection Hs as -> ->. cbn [first_fit] in Ef.
        destruct (claimed C x) eqn:Ecl.
        * unfold claimed in Ecl. apply tmem_spec in Ecl. contradiction.
        * destruct (spatial p (r_val x)) as [mx |]; [injection Ef as E _; exfalso; exact (Hne (eq_sym E)) | reflexivity].
      + destruct k as [| k]; [lia |]. cbn [nth_error] in Hs, Hk. cbn [map] in Hnd.
        apply NoDup_cons_iff in Hnd as [_ Hnd].
        assert (Hid : r_id e0 <> r_id x).
        { intros E. apply Hne. apply Hu; [cbn [pick_lists flat_map]; apply in_or_app; left; exact Hin0 | exact E]. }
        assert (Hte : tag e0 <> tag x).
        { apply Ht; [cbn [pick_lists flat_map]; apply in_or_app; left; exact Hin0 | exact Hid]. }
        assert (Hc' : ~ In (tag x) (tag e0 :: C)) by (intros [E | Hin]; [exact (Hte E) | exact (Hc Hin)]).
        exact (IH (tag e0 :: C) sg' k m Hr Hk Hnd Hc' Ht' Hu' i rest p ltac:(lia) Hs).
    - destruct (ref_fill C r) as [sg' |] eqn:Hr; [| discriminate]. cbn [option_map] in H. injection H as <-.
      destruct i as [| i]; [discriminate |]. destruct k as [| k]; [lia |].
      cbn [nth_error] in Hs, Hk. cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [_ Hnd].
      exact (IH C sg' k m Hr Hk Hnd Hc Ht' Hu' i rest p ltac:(lia) Hs).
  Qed.

  (* The positions of the produce search ascend. *)
  Fixpoint ascending (l : list (nat * (nat * bnd))) : Prop :=
    match l with [] => True | y :: r => (forall z, In z r -> fst y < fst z) /\ ascending r end.

  Lemma ascending_enum : forall (l : list (nat * bnd)) j, ascending (enum j l).
  Proof.
    induction l as [| ib r IH]; intros j; [exact I |]. cbn [enum ascending]. split; [| apply IH].
    intros z Hz. destruct z as [k y]. apply enum_nth in Hz as [Hle _]. cbn [fst]. lia.
  Qed.

  Lemma ascending_filter : forall f l, ascending l -> ascending (filter f l).
  Proof.
    intros f l. induction l as [| y r IH]; intros H; [exact I |]. destruct H as [Hlt H].
    cbn [filter]. destruct (f y); [| exact (IH H)]. split; [| exact (IH H)].
    intros z Hz. apply filter_In in Hz as [Hz _]. exact (Hlt z Hz).
  Qed.

  Lemma search_at_le :
    forall W x v ks k j b sg,
      ascending ks -> In (k, (j, b)) ks ->
      sees (lane_of b) (r_lvl x) && x_in_win W x (b_coll b) v = true ->
      is_some (spatial (b_pat b) (r_val x)) = true ->
      fill (w_guard W) [tag x] (slots_prod W x k v) = Some sg ->
      exists k' sg', search_at W x v ks = Some (k', sg') /\ k' <= k.
  Proof.
    intros W x v ks. induction ks as [| [k0 [j0 b0]] r IH]; intros k j b sg Hasc Hin Hc Hs Hf; [destruct Hin |].
    destruct Hasc as [Hlt Hasc]. cbn [search_at fst snd].
    destruct Hin as [E | Hin].
    - injection E as -> -> ->. rewrite Hc.
      destruct (spatial (b_pat b) (r_val x)); [| discriminate]. rewrite Hf.
      exists k, sg. split; [reflexivity | lia].
    - assert (Hk : k0 < k) by exact (Hlt _ Hin).
      destruct (sees (lane_of b0) (r_lvl x) && x_in_win W x (b_coll b0) v).
      + destruct (spatial (b_pat b0) (r_val x)).
        * destruct (fill (w_guard W) [tag x] (slots_prod W x k0 v)) as [sg0 |].
          -- exists k0, sg0. split; [reflexivity | lia].
          -- exact (IH _ _ _ _ Hasc Hin Hc Hs Hf).
        * exact (IH _ _ _ _ Hasc Hin Hc Hs Hf).
      + exact (IH _ _ _ _ Hasc Hin Hc Hs Hf).
  Qed.

  Lemma search_at_app_skip :
    forall W x v l1 l2,
      (forall k j b, In (k, (j, b)) l1 -> sees (lane_of b) (r_lvl x) = false \/ spatial (b_pat b) (r_val x) = None) ->
      search_at W x v (l1 ++ l2) = search_at W x v l2.
  Proof.
    intros W x v l1 l2 H. induction l1 as [| [k [j b]] r IH]; [reflexivity |].
    assert (H' : forall k j b, In (k, (j, b)) r -> sees (lane_of b) (r_lvl x) = false \/ spatial (b_pat b) (r_val x) = None)
      by (intros k' j' b' Hin; exact (H k' j' b' (or_intror Hin))).
    cbn [app search_at fst snd]. destruct (H k j b (or_introl eq_refl)) as [E | E].
    - rewrite E. cbn [andb]. exact (IH H').
    - destruct (sees (lane_of b) (r_lvl x) && x_in_win W x (b_coll b) v); [rewrite E |]; exact (IH H').
  Qed.

  (* HEAD's search (ops_produce.rs): the incoming record at index 0 of the
     candidates of its channel, over the windows of the space. *)
  Definition slots_head (W : wtr) (x : rec) (v : view) : list slot :=
    map (fun ib : nat * bnd =>
           let b := snd ib in
           let L := not_x x (visible b (win_in W x (b_coll b) v)) in
           Pick (if (b_coll b =? r_coll x) && sees (lane_of b) (r_lvl x) then x :: L else L) (b_pat b))
        (gpairs W).

  Lemma gpairs_enum_nth :
    forall W i ib, nth_error (gpairs W) i = Some ib -> In (i, ib) (enum 0 (gpairs W)).
  Proof.
    intros W i ib H. apply nth_error_In with i. rewrite enum_nth_error, H. reflexivity.
  Qed.

  Lemma visible_incl : forall b l, incl (visible b l) l.
  Proof. intros b l a Ha. unfold visible in Ha. apply filter_In in Ha as [Ha _]. exact Ha. Qed.

  Lemma not_x_incl : forall x l, incl (not_x x l) l.
  Proof. intros x l a Ha. unfold not_x in Ha. apply filter_In in Ha as [Ha _]. exact Ha. Qed.

  Lemma slots_match_length : forall ss sg, slots_match ss sg -> length sg = length ss.
  Proof.
    induction ss as [| s r IH]; intros sg Hm; cbn [slots_match] in Hm.
    - subst. reflexivity.
    - destruct sg as [| [e m0] sg]; [contradiction |]. destruct Hm as [_ Hm]. cbn [length]. rewrite (IH _ Hm). reflexivity.
  Qed.

  Lemma slots_match_nth :
    forall ss sg i s rm,
      slots_match ss sg -> nth_error ss i = Some s -> nth_error sg i = Some rm ->
      match s with
      | Pick L p => In (fst rm) L /\ spatial p (r_val (fst rm)) = Some (snd rm)
      | Forced e m => rm = (e, m)
      end.
  Proof.
    induction ss as [| s0 r IH]; intros sg i s rm Hm Hs Hr; [destruct i; discriminate |].
    destruct sg as [| [e m0] sg]; [cbn in Hm; contradiction |]. cbn [slots_match] in Hm.
    destruct i as [| i]; cbn [nth_error] in Hs, Hr.
    - injection Hs as <-. injection Hr as <-. destruct Hm as [Hm _].
      destruct s0 as [L p | e' m']; [exact Hm | destruct Hm as [-> ->]; reflexivity].
    - destruct Hm as [_ Hm]. exact (IH _ _ _ _ Hm Hs Hr).
  Qed.

  (* ── 13. Headline 2: the produce search agrees with greedy and HEAD ──── *)

  (* Headline 2 (decision 28, decision 37). Let GREEDY on the state with x
     stored bind x at the group position k. Then the produce search at k
     binds the same records (x at k), so the search tries positions in group
     order and completes at a position k' <= k: the position retry finds
     every assignment that GREEDY finds with x. And when HEAD's search, with
     x at index 0 of the candidates of its collection, binds x, the produce
     search fires the same assignment at the same position. x is a fresh
     record: no stored record carries its identity. *)
  Theorem join_produce_search_agrees_with_head :
    (forall W x v sg k m,
       (forall e, In e (v_recs v) -> r_id e <> r_id x) ->
       greedy W (store_view x v) = Some sg -> nth_error sg k = Some (x, m) ->
       fill (w_guard W) [tag x] (slots_prod W x k v) = Some sg /\
       exists k' sg', search_produce W x v = Some (k', sg') /\ k' <= k) /\
    (forall W x v sg k m,
       (forall e, In e (v_recs v) -> r_id e <> r_id x) ->
       in_win x (win_in W x (r_coll x) v) = true ->
       fill (w_guard W) [] (slots_head W x v) = Some sg -> nth_error sg k = Some (x, m) ->
       search_produce W x v = Some (k, sg)).
  Proof.
    destruct join_search_is_head_greedy as (Hfr & Hgr & Hsound & _).
    split.
    - intros W x v sg k m Hfresh H Hk.
      pose proof H as Hg. rewrite Hgr in Hg.
      destruct (Hsound _ _ _ H) as [Hm Hnd]. unfold bound_tags in Hnd.
      assert (Hndr : NoDup (map fst sg)) by (apply (NoDup_map_inv (fun e => tag e)); rewrite map_map; exact Hnd).
      set (ss1 := slots_cons W (store_view x v)) in *. set (ss2 := slots_prod W x k v).
      assert (Hlen1 : length ss1 = length (gpairs W)) by (unfold ss1, slots_cons; apply length_map).
      assert (Hlen2 : length ss2 = length (gpairs W)) by (unfold ss2, slots_prod; rewrite length_map; apply enum_length).
      assert (Hlens : length sg = length ss1) by exact (slots_match_length _ _ Hm).
      (* the slot of k binds x from its window *)
      assert (Hk1 : k < length (gpairs W)) by (rewrite <- Hlen1, <- Hlens; apply nth_error_Some; rewrite Hk; discriminate).
      destruct (nth_error (gpairs W) k) as [[j b] |] eqn:Hgk; [| apply nth_error_None in Hgk; lia].
      assert (Hs1 : forall i j' b', nth_error (gpairs W) i = Some (j', b') ->
                     nth_error ss1 i = Some (Pick (visible b' (win W (b_coll b') (store_view x v))) (b_pat b'))).
      { intros i j' b' Hi. unfold ss1, slots_cons. rewrite nth_error_map, Hi. reflexivity. }
      assert (Hmk : In x (visible b (win W (b_coll b) (store_view x v))) /\ spatial (b_pat b) (r_val x) = Some m)
        by exact (slots_match_nth _ _ _ _ _ Hm (Hs1 k j b Hgk) Hk).
      destruct Hmk as [Hxin Hxm].
      rewrite win_store in Hxin. pose proof (visible_incl _ _ _ Hxin) as Hxw.
      assert (Hcb : b_coll b = r_coll x).
      { destruct (win_in_mem W x (b_coll b) v x Hxw) as [[_ E] | [Hin _]]; [exact E |].
        exfalso. exact (Hfresh x Hin eq_refl). }
      rewrite Hcb in Hxw.
      (* the tags of the windows *)
      assert (Ht : forall e', In e' (pick_lists ss1) -> r_id e' <> r_id x -> tag e' <> tag x).
      { intros e' Hin Hne. unfold pick_lists, ss1, slots_cons in Hin.
        apply in_flat_map in Hin as (s & Hs & He). apply in_map_iff in Hs as ([j' b'] & <- & _).
        cbn [snd] in He. rewrite win_store in He. apply visible_incl in He.
        exact (window_tag W x v _ e' Hfresh He Hne Hxw). }
      (* the two searches are related slot by slot *)
      assert (Hsim : sim_slots x ss1 ss2 sg).
      { apply sim_slots_nth; [lia | lia |]. intros i s1 s2 rm H1 H2 H3.
        assert (Hi : i < length (gpairs W)) by (rewrite <- Hlen1; apply nth_error_Some; rewrite H1; discriminate).
        destruct (nth_error (gpairs W) i) as [[j' b'] |] eqn:Hgi; [| apply nth_error_None in Hgi; lia].
        rewrite (Hs1 i j' b' Hgi) in H1. injection H1 as <-.
        unfold ss2 in H2. rewrite (slots_prod_nth W x k v i j' b' (enum_gpairs_nth W i j' b' (gpairs_enum_nth W i (j', b') Hgi))) in H2.
        injection H2 as <-. unfold forced. cbn [sim1].
        destruct (Nat.eqb_spec i k) as [-> | Hne].
        - rewrite Hgi in Hgk. injection Hgk as -> ->. rewrite Hxm. split; [reflexivity |].
          rewrite Hk in H3. injection H3 as <-. reflexivity.
        - rewrite win_store. split; [reflexivity | split; [reflexivity |]].
          intros E. apply Hne.
          (* rm is x, so it occurs twice in sg *)
          assert (Hrm : In (fst rm) (visible b' (win_in W x (b_coll b') v))).
          { pose proof (slots_match_nth _ _ _ _ _ Hm (Hs1 i j' b' Hgi) H3) as [Hrm _].
            rewrite win_store in Hrm. exact Hrm. }
          apply visible_incl in Hrm.
          destruct (win_in_mem W x _ v (fst rm) Hrm) as [[Ex _] | [Hin _]]; [| exfalso; exact (Hfresh _ Hin E)].
          destruct rm as [e0 m0]. cbn [fst] in Ex. subst e0.
          apply (NoDup_nth_error (map fst sg)); [exact Hndr | rewrite length_map; apply nth_error_Some; rewrite H3; discriminate |].
          rewrite !nth_error_map, H3, Hk. reflexivity. }
      assert (Hf2 : ref_fill [tag x] ss2 = Some sg).
      { apply (ref_fill_sim x ss1 ss2 sg [] [tag x] Hsim); [| exact Ht | exact Hg].
        intros t Htx. cbn [In]. split; [intros [] | intros [E | []]; exfalso; exact (Htx (eq_sym E))]. }
      assert (Hf : fill (w_guard W) [tag x] ss2 = Some sg) by (rewrite Hfr; exact Hf2).
      split; [exact Hf |].
      unfold search_produce.
      apply (search_at_le W x v (incoming_positions W x) k j b sg).
      + unfold incoming_positions, incoming_base. apply ascending_filter, ascending_enum.
      + unfold incoming_positions, incoming_base. apply filter_In. split.
        * exact (gpairs_enum_nth W k (j, b) Hgk).
        * cbn [snd]. rewrite Hcb. apply Nat.eqb_refl.
      + apply andb_true_iff. split.
        * unfold visible in Hxin. apply filter_In in Hxin as [_ E]. exact E.
        * rewrite Hcb. apply in_win_x_in_win. apply in_win_spec; [exact Hfresh | exact Hxw].
      + rewrite Hxm. reflexivity.
      + exact Hf.
    - intros W x v sg k m Hfresh Hwin H Hk.
      pose proof H as Hg. rewrite Hfr in Hg.
      pose proof (ref_fill_match _ _ _ Hg) as Hm.
      assert (Hnf : forced_tags (slots_head W x v) = []).
      { unfold slots_head. induction (gpairs W) as [| ib r IHr]; [reflexivity | exact IHr]. }
      destruct (proj1 join_repeated_channel_distinct_records _ _ _ _ H) as [Hnd _];
        [rewrite Hnf; intros t [] | rewrite Hnf; constructor |].
      unfold bound_tags in Hnd.
      assert (Hndr : NoDup (map fst sg)) by (apply (NoDup_map_inv (fun e => tag e)); rewrite map_map; exact Hnd).
      apply in_win_spec in Hwin; [| exact Hfresh].
      set (ss1 := slots_head W x v) in *. set (ss2 := slots_prod W x k v).
      assert (Hlen1 : length ss1 = length (gpairs W)) by (unfold ss1, slots_head; apply length_map).
      assert (Hlen2 : length ss2 = length (gpairs W)) by (unfold ss2, slots_prod; rewrite length_map; apply enum_length).
      assert (Hlens : length sg = length ss1) by exact (slots_match_length _ _ Hm).
      assert (Hs1 : forall i j' b', nth_error (gpairs W) i = Some (j', b') ->
                     nth_error ss1 i =
                       Some (Pick (if (b_coll b' =? r_coll x) && sees (lane_of b') (r_lvl x)
                                   then x :: not_x x (visible b' (win_in W x (b_coll b') v))
                                   else not_x x (visible b' (win_in W x (b_coll b') v))) (b_pat b'))).
      { intros i j' b' Hi. unfold ss1, slots_head. rewrite nth_error_map, Hi. reflexivity. }
      (* the slot list of each position, and its bound record *)
      assert (Hpos : forall i rm j' b', nth_error sg i = Some rm -> nth_error (gpairs W) i = Some (j', b') ->
                       In (fst rm) (if (b_coll b' =? r_coll x) && sees (lane_of b') (r_lvl x)
                                    then x :: not_x x (visible b' (win_in W x (b_coll b') v))
                                    else not_x x (visible b' (win_in W x (b_coll b') v))) /\
                       spatial (b_pat b') (r_val (fst rm)) = Some (snd rm)).
      { intros i rm j' b' H3 Hgi. exact (slots_match_nth _ _ _ _ _ Hm (Hs1 i j' b' Hgi) H3). }
      (* the only record of the lists with the identity of x is x *)
      assert (Hu : forall e', In e' (pick_lists ss1) -> r_id e' = r_id x -> e' = x).
      { intros e' Hin E. unfold pick_lists, ss1, slots_head in Hin.
        apply in_flat_map in Hin as (s & Hs & He). apply in_map_iff in Hs as ([j' b'] & <- & _). cbn [snd] in He.
        destruct ((b_coll b' =? r_coll x) && sees (lane_of b') (r_lvl x)).
        - destruct He as [<- | He]; [reflexivity |]. apply not_x_excludes in He. contradiction.
        - apply not_x_excludes in He. contradiction. }
      assert (Ht : forall e', In e' (pick_lists ss1) -> r_id e' <> r_id x -> tag e' <> tag x).
      { intros e' Hin Hne. unfold pick_lists, ss1, slots_head in Hin.
        apply in_flat_map in Hin as (s & Hs & He). apply in_map_iff in Hs as ([j' b'] & <- & _). cbn [snd] in He.
        assert (He' : In e' (not_x x (visible b' (win_in W x (b_coll b') v)))).
        { destruct ((b_coll b' =? r_coll x) && sees (lane_of b') (r_lvl x)); [| exact He].
          destruct He as [<- | He]; [exfalso; exact (Hne eq_refl) | exact He]. }
        apply not_x_incl, visible_incl in He'. exact (window_tag W x v _ e' Hfresh He' Hne Hwin). }
      assert (Hk1 : k < length (gpairs W)) by (rewrite <- Hlen1, <- Hlens; apply nth_error_Some; rewrite Hk; discriminate).
      destruct (nth_error (gpairs W) k) as [[j b] |] eqn:Hgk; [| apply nth_error_None in Hgk; lia].
      destruct (Hpos k (x, m) j b Hk Hgk) as [Hxl Hxm]. cbn [fst snd] in Hxl, Hxm.
      assert (Hcond : (b_coll b =? r_coll x) && sees (lane_of b) (r_lvl x) = true).
      { destruct ((b_coll b =? r_coll x) && sees (lane_of b) (r_lvl x)); [reflexivity |].
        apply not_x_excludes in Hxl. exfalso. exact (Hxl eq_refl). }
      apply andb_true_iff in Hcond as [Hcb Hsee]. apply Nat.eqb_eq in Hcb.
      assert (Hsim : sim_slots x ss1 ss2 sg).
      { apply sim_slots_nth; [lia | lia |]. intros i s1 s2 rm H1 H2 H3.
        assert (Hi : i < length (gpairs W)) by (rewrite <- Hlen1; apply nth_error_Some; rewrite H1; discriminate).
        destruct (nth_error (gpairs W) i) as [[j' b'] |] eqn:Hgi; [| apply nth_error_None in Hgi; lia].
        rewrite (Hs1 i j' b' Hgi) in H1. injection H1 as <-.
        unfold ss2 in H2. rewrite (slots_prod_nth W x k v i j' b' (enum_gpairs_nth W i j' b' (gpairs_enum_nth W i (j', b') Hgi))) in H2.
        injection H2 as <-. unfold forced. cbn [sim1].
        destruct (Nat.eqb_spec i k) as [-> | Hne].
        - rewrite Hgi in Hgk. injection Hgk as -> ->. rewrite Hxm. split; [reflexivity |].
          rewrite Hk in H3. injection H3 as <-. reflexivity.
        - split; [destruct ((b_coll b' =? r_coll x) && sees (lane_of b') (r_lvl x)); [rewrite not_x_head |]; rewrite not_x_idem; reflexivity |].
          split; [reflexivity |]. intros E. apply Hne.
          destruct (Hpos i rm j' b' H3 Hgi) as [Hrm _].
          assert (Ex : fst rm = x) by (apply Hu; [unfold pick_lists, ss1, slots_head; apply in_flat_map;
            exists (Pick (if (b_coll b' =? r_coll x) && sees (lane_of b') (r_lvl x)
                          then x :: not_x x (visible b' (win_in W x (b_coll b') v))
                          else not_x x (visible b' (win_in W x (b_coll b') v))) (b_pat b'));
            split; [apply in_map_iff; exists (j', b'); split; [reflexivity | exact (nth_error_In _ _ Hgi)] | exact Hrm] | exact E]).
          destruct rm as [e0 m0]. cbn [fst] in Ex. subst e0.
          apply (NoDup_nth_error (map fst sg)); [exact Hndr | rewrite length_map; apply nth_error_Some; rewrite H3; discriminate |].
          rewrite !nth_error_map, H3, Hk. reflexivity. }
      assert (Hf2 : ref_fill [tag x] ss2 = Some sg).
      { apply (ref_fill_sim x ss1 ss2 sg [] [tag x] Hsim); [| exact Ht | exact Hg].
        intros t Htx. cbn [In]. split; [intros [] | intros [E | []]; exfalso; exact (Htx (eq_sym E))]. }
      assert (Hf : fill (w_guard W) [tag x] ss2 = Some sg) by (rewrite Hfr; exact Hf2).
      (* the positions of x before k are not pickable: HEAD would bind x there *)
      assert (Hearly : forall i j' b', i < k -> nth_error (gpairs W) i = Some (j', b') -> b_coll b' = r_coll x ->
                         sees (lane_of b') (r_lvl x) = false \/ spatial (b_pat b') (r_val x) = None).
      { intros i j' b' Hik Hgi Hc'. destruct (sees (lane_of b') (r_lvl x)) eqn:Es; [right | left; reflexivity].
        apply (ref_fill_head_first x ss1 [] sg k m Hg Hk Hndr (fun H0 => H0) Ht Hu i
                 (not_x x (visible b' (win_in W x (b_coll b') v))) (b_pat b') Hik).
        rewrite (Hs1 i j' b' Hgi), Hc', Nat.eqb_refl, Es. reflexivity. }
      unfold search_produce, incoming_positions, incoming_base.
      assert (Hin : In (k, (j, b)) (filter (fun kib : nat * (nat * bnd) => b_coll (snd (snd kib)) =? r_coll x)
                                         (enum 0 (gpairs W)))).
      { apply filter_In. split; [exact (gpairs_enum_nth W k (j, b) Hgk) | cbn [snd]; rewrite Hcb; apply Nat.eqb_refl]. }
      pose proof (ascending_filter (fun kib : nat * (nat * bnd) => b_coll (snd (snd kib)) =? r_coll x) _
                    (ascending_enum (gpairs W) 0)) as Hasc.
      apply in_split in Hin as (l1 & l2 & Hsplit). rewrite Hsplit in Hasc |- *.
      rewrite search_at_app_skip.
      + cbn [search_at fst snd]. rewrite Hsee, Hcb.
        replace (x_in_win W x (r_coll x) v) with true
          by (symmetry; apply in_win_x_in_win; apply in_win_spec; [exact Hfresh | exact Hwin]).
        cbn [andb]. rewrite Hxm. unfold ss2 in Hf. rewrite Hf. reflexivity.
      + intros k' j' b' Hin'.
        assert (Hlt : k' < k).
        { clear -Hasc Hin'. induction l1 as [| y r IH]; [destruct Hin' |].
          cbn [app ascending] in Hasc. destruct Hasc as [Hlt Hasc]. destruct Hin' as [-> | Hin'].
          - exact (Hlt (k, (j, b)) (in_or_app _ _ _ (or_intror (in_eq _ _)))).
          - exact (IH Hasc Hin'). }
        assert (Hin2 : In (k', (j', b')) (filter (fun kib : nat * (nat * bnd) => b_coll (snd (snd kib)) =? r_coll x)
                                             (enum 0 (gpairs W)))).
        { rewrite Hsplit. apply in_or_app. left. exact Hin'. }
        apply filter_In in Hin2 as [Hin2 Hc2]. cbn [snd] in Hc2. apply Nat.eqb_eq in Hc2.
        apply enum_nth in Hin2 as [_ Hn2]. rewrite Nat.sub_0_r in Hn2.
        exact (Hearly k' j' b' Hlt Hn2 Hc2).
  Qed.

  (* ── 14. Headline 13: the group order and the bind order ───────────── *)

  Lemma gle_total : forall a b, gle a b = false -> gle b a = true.
  Proof.
    intros a b H. unfold gle in *. apply orb_false_iff in H as [H1 H2].
    apply Nat.ltb_ge in H1. apply orb_true_iff.
    destruct (Nat.eq_dec (chash (b_coll (snd a))) (chash (b_coll (snd b)))) as [E | E].
    - right. rewrite E, Nat.eqb_refl in *. cbn [andb] in H2 |- *. apply Nat.leb_gt in H2. apply Nat.leb_le. lia.
    - left. apply Nat.ltb_lt. lia.
  Qed.

  Lemma gle_trans : forall a b c, gle a b = true -> gle b c = true -> gle a c = true.
  Proof.
    intros a b c H1 H2. unfold gle in *. apply orb_true_iff in H1, H2. apply orb_true_iff.
    destruct H1 as [H1 | H1]; destruct H2 as [H2 | H2].
    - left. apply Nat.ltb_lt in H1, H2. apply Nat.ltb_lt. lia.
    - left. apply andb_true_iff in H2 as [H2 _]. apply Nat.ltb_lt in H1. apply Nat.eqb_eq in H2.
      apply Nat.ltb_lt. lia.
    - left. apply andb_true_iff in H1 as [H1 _]. apply Nat.ltb_lt in H2. apply Nat.eqb_eq in H1.
      apply Nat.ltb_lt. lia.
    - right. apply andb_true_iff in H1 as [E1 L1], H2 as [E2 L2]. apply Nat.eqb_eq in E1, E2.
      apply Nat.leb_le in L1, L2. apply andb_true_iff. split; [apply Nat.eqb_eq; lia | apply Nat.leb_le; lia].
  Qed.

  Fixpoint gsorted (l : list (nat * bnd)) : Prop :=
    match l with [] => True | a :: r => (forall z, In z r -> gle a z = true) /\ gsorted r end.

  Lemma ins_gsorted : forall x l, gsorted l -> gsorted (ins gle x l).
  Proof.
    intros x l. induction l as [| y r IH]; intros H; [split; [intros z [] | exact I] |].
    destruct H as [Hy H]. cbn [ins]. destruct (gle x y) eqn:E.
    - split; [| split; [exact Hy | exact H]].
      intros z [<- | Hz]; [exact E | exact (gle_trans _ _ _ E (Hy z Hz))].
    - split; [| exact (IH H)].
      intros z Hz. apply (Permutation_in _ (ins_perm _ gle x r)) in Hz.
      destruct Hz as [<- | Hz]; [exact (gle_total _ _ E) | exact (Hy z Hz)].
  Qed.

  Lemma isort_gsorted : forall l, gsorted (isort gle l).
  Proof. induction l as [| x r IH]; [exact I | apply ins_gsorted; exact IH]. Qed.

  Fixpoint csorted (l : list nat) : Prop :=
    match l with [] => True | a :: r => (forall z, In z r -> chash a <= chash z) /\ csorted r end.

  Lemma gsorted_csorted : forall l, gsorted l -> csorted (map (fun ib => b_coll (snd ib)) l).
  Proof.
    induction l as [| a r IH]; intros H; [exact I |]. destruct H as [Ha H]. cbn [map csorted]. split; [| exact (IH H)].
    intros z Hz. apply in_map_iff in Hz as (ib & <- & Hib). specialize (Ha ib Hib). unfold gle in Ha.
    apply orb_true_iff in Ha as [Ha | Ha]; [apply Nat.ltb_lt in Ha; lia |].
    apply andb_true_iff in Ha as [Ha _]. apply Nat.eqb_eq in Ha. lia.
  Qed.

  Lemma csorted_unique :
    (forall a b, chash a = chash b -> a = b) ->
    forall l1 l2, csorted l1 -> csorted l2 -> Permutation l1 l2 -> l1 = l2.
  Proof.
    intros Hinj l1. induction l1 as [| a r1 IH]; intros l2 H1 H2 Hp.
    - symmetry. exact (Permutation_nil Hp).
    - destruct l2 as [| b r2]; [apply Permutation_sym, Permutation_nil in Hp; discriminate |].
      destruct H1 as [Ha H1]. destruct H2 as [Hb H2].
      assert (E : a = b).
      { assert (Hab : In a (b :: r2)) by exact (Permutation_in _ Hp (in_eq _ _)).
        assert (Hba : In b (a :: r1)) by exact (Permutation_in _ (Permutation_sym Hp) (in_eq _ _)).
        destruct Hab as [-> | Hab]; [reflexivity |].
        destruct Hba as [-> | Hba]; [reflexivity |].
        apply Hinj. specialize (Hb a Hab). specialize (Ha b Hba). lia. }
      subst b. f_equal. exact (IH r2 H1 H2 (Permutation_cons_inv Hp)).
  Qed.

  Lemma assoc_unique :
    forall (X : Type) (l : list (nat * X)) b y, NoDup (map fst l) -> In (b, y) l -> assoc b l = Some y.
  Proof.
    intros X l. induction l as [| [k z] r IH]; intros b y Hnd Hin; [destruct Hin |].
    cbn [map fst] in Hnd. apply NoDup_cons_iff in Hnd as [Hnot Hnd]. cbn [assoc].
    destruct Hin as [E | Hin].
    - injection E as -> ->. rewrite Nat.eqb_refl. reflexivity.
    - destruct (Nat.eqb_spec k b) as [-> | Ne]; [| exact (IH _ _ Hnd Hin)].
      exfalso. apply Hnot. apply in_map_iff. exists (b, y). split; [reflexivity | exact Hin].
  Qed.

  Lemma combine_map_fst :
    forall (X Y : Type) (f : nat * X -> Y) (l : list (nat * X)),
      combine (map fst l) (map f l) = map (fun ib => (fst ib, f ib)) l.
  Proof. intros X Y f l. induction l as [| a r IH]; [reflexivity | cbn; rewrite IH; reflexivity]. Qed.

  (* Headline 13 (FR-1', plan 2.2 and 2.10). The channels of a group in its
     canonical order depend only on the multiset of the bind channels (for a
     collision-free stable hash). Putting the receipt results back by the
     bind indices of the slots gives the results in bind order, for every
     permutation of the binds; the outcome of Staged::JoinFire and the guard
     values are the case of the canonical group order. *)
  Theorem bind_permutation_restores_order :
    ((forall a b, chash a = chash b -> a = b) ->
     forall W1 W2, Permutation (map b_coll (w_binds W1)) (map b_coll (w_binds W2)) ->
       map (fun ib => b_coll (snd ib)) (gpairs W1) = map (fun ib => b_coll (snd ib)) (gpairs W2)) /\
    (forall (X : Type) (f : nat * bnd -> X) (W : wtr) (ps : list (nat * bnd)),
       Permutation ps (enum 0 (w_binds W)) ->
       restore (map fst ps) (map f ps) = map (fun ib => Some (f ib)) (enum 0 (w_binds W))) /\
    (forall (X : Type) (f : nat * bnd -> X) (W : wtr),
       restore (gorder W) (map f (gpairs W)) = map (fun ib => Some (f ib)) (enum 0 (w_binds W))).
  Proof.
    assert (Hperm : forall (X : Type) (f : nat * bnd -> X) (W : wtr) (ps : list (nat * bnd)),
               Permutation ps (enum 0 (w_binds W)) ->
               restore (map fst ps) (map f ps) = map (fun ib => Some (f ib)) (enum 0 (w_binds W))).
    { intros X f W ps Hp. unfold restore. rewrite combine_map_fst, length_map.
      rewrite (Permutation_length Hp), enum_length.
      assert (Hnd : NoDup (map fst (map (fun ib => (fst ib, f ib)) ps))).
      { rewrite map_map. cbn [fst]. apply (Permutation_NoDup (Permutation_map fst (Permutation_sym Hp))).
        apply enum_NoDup. }
      apply nth_error_ext. intros n. rewrite !nth_error_map, nth_error_seq, enum_nth_error.
      destruct (Nat.ltb_spec n (length (w_binds W))) as [Hlt | Hge].
      - destruct (nth_error (w_binds W) n) as [b |] eqn:Eb; [| apply nth_error_None in Eb; lia].
        cbn [option_map]. f_equal. rewrite Nat.add_0_l.
        apply assoc_unique; [exact Hnd |].
        apply in_map_iff. exists (n, b). split; [reflexivity |].
        apply (Permutation_in _ (Permutation_sym Hp)). apply nth_error_In with n.
        rewrite enum_nth_error, Eb. reflexivity.
      - destruct (nth_error (w_binds W) n) eqn:Eb; [| reflexivity].
        assert (n < length (w_binds W)) by (apply nth_error_Some; rewrite Eb; discriminate). lia. }
    split; [| split; [exact Hperm | intros X f W; unfold gorder; apply Hperm, gpairs_perm]].
    intros Hinj W1 W2 Hp. apply (csorted_unique Hinj).
    - apply gsorted_csorted, isort_gsorted.
    - apply gsorted_csorted, isort_gsorted.
    - assert (Hch : forall W, Permutation (map (fun ib => b_coll (snd ib)) (gpairs W)) (map b_coll (w_binds W))).
      { intros W. replace (map b_coll (w_binds W)) with (map (fun ib => b_coll (snd ib)) (enum 0 (w_binds W))).
        - apply Permutation_map, gpairs_perm.
        - rewrite <- (enum_map_snd _ (w_binds W) 0) at 2. rewrite map_map. reflexivity. }
      apply Permutation_trans with (map b_coll (w_binds W1)); [apply Hch |].
      apply Permutation_trans with (map b_coll (w_binds W2)); [exact Hp | apply Permutation_sym, Hch].
  Qed.

  (* ── 15. The join engine: staged operations and the transaction state ── *)

  (* A staged operation of a transaction (the staging primitives of Cx). *)
  Inductive staged : Type :=
  | SBump (c : nat)                    (* counter::bump of S(sid, c): the take and the put *)
  | SStore (r : rec) (a : A)           (* store_on D(c), with the authority that it registers *)
  | SPark (W : wtr)                    (* park_on the group of W: one join entry *)
  | SFire (W : wtr) (sg : list (rec * M)) (inc : option (nat * A))
                                       (* a fire of W over the bound records in group order; inc is the
                                          incoming position and its introduction (a produce-side fire) *)
  | SLift (r : rec)                    (* K_lift: removes a stored record *)
  | SRetire (r : rec).                 (* K_retire: removes a set member *)

  Fixpoint cnt_of (c : nat) (cnt : list (nat * nat)) : nat :=
    match cnt with [] => 0 | (k, n) :: r => if k =? c then n else cnt_of c r end.

  Fixpoint del_rec (id : nat) (l : list rec) : list rec :=
    match l with [] => [] | r :: rest => if r_id r =? id then rest else r :: del_rec id rest end.

  Fixpoint del_wtr (id : nat) (l : list wtr) : list wtr :=
    match l with [] => [] | W :: rest => if w_id W =? id then rest else W :: del_wtr id rest end.

  Definition remove_all (rs : list rec) (l : list rec) : list rec :=
    fold_left (fun acc r => del_rec (r_id r) acc) rs l.

  Definition inc_pos (inc : option (nat * A)) (k : nat) : bool :=
    match inc with Some (p, _) => k =? p | None => false end.

  (* The stored records that a fire removes: the non-persistent records of
     the positions other than the incoming one (also for a peek: Rule E). *)
  Definition removed (sg : list (rec * M)) (inc : option (nat * A)) : list rec :=
    map (fun krm : nat * (rec * M) => fst (snd krm))
        (filter (fun krm : nat * (rec * M) => negb (inc_pos inc (fst krm)) && negb (r_pers (fst (snd krm))))
                (enum 0 sg)).

  Definition apply (v : view) (op : staged) : view :=
    match op with
    | SBump c => mkView (v_recs v) (v_wtrs v) ((c, S (cnt_of c (v_cnt v))) :: v_cnt v)
    | SStore r _ => mkView (r :: v_recs v) (v_wtrs v) (v_cnt v)
    | SPark W => mkView (v_recs v) (W :: v_wtrs v) (v_cnt v)
    | SFire W sg inc =>
        mkView (remove_all (removed sg inc) (v_recs v))
               (if w_pers W then v_wtrs v else del_wtr (w_id W) (v_wtrs v)) (v_cnt v)
    | SLift r | SRetire r => mkView (del_rec (r_id r) (v_recs v)) (v_wtrs v) (v_cnt v)
    end.

  Definition apply_all (v : view) (ops : list staged) : view := fold_left apply ops v.

  (* The collections that a staged operation touches. *)
  Definition staged_colls (op : staged) : list nat :=
    match op with
    | SBump c => [c]
    | SStore r _ | SLift r | SRetire r => [r_coll r]
    | SPark W | SFire W _ _ => colls W
    end.

  Inductive own_st : Type := ONot | OSpent | OStored | OParked | OFlight.
  Inductive cause : Type := CDivergent | CRoundCap.

  (* A second send (Rule E): the record and whether it is the operation's
     own element. *)
  Record pend : Type := mkPend { p_rec : rec; p_own : bool }.

  (* The state J of a transaction of the join engine (plan 4.2). *)
  Record jst : Type := mkJ {
    j_view : view; j_bumps : list nat; j_dirty : list nat; j_work : list pend;
    j_own : own_st; j_stop : option cause; j_fired : nat; j_trace : list staged }.

  Definition stage (j : jst) (op : staged) : jst :=
    mkJ (apply (j_view j) op) (j_bumps j) (j_dirty j) (j_work j) (j_own j) (j_stop j) (j_fired j)
        (j_trace j ++ [op]).
  Definition stage_all (j : jst) (ops : list staged) : jst := fold_left stage ops j.
  Definition with_bumps (b : list nat) (j : jst) : jst :=
    mkJ (j_view j) b (j_dirty j) (j_work j) (j_own j) (j_stop j) (j_fired j) (j_trace j).
  Definition with_dirty (d : list nat) (j : jst) : jst :=
    mkJ (j_view j) (j_bumps j) d (j_work j) (j_own j) (j_stop j) (j_fired j) (j_trace j).
  Definition add_dirty (d : list nat) (j : jst) : jst := with_dirty (d ++ j_dirty j) j.
  Definition with_work (w : list pend) (j : jst) : jst :=
    mkJ (j_view j) (j_bumps j) (j_dirty j) w (j_own j) (j_stop j) (j_fired j) (j_trace j).
  Definition set_own (o : own_st) (j : jst) : jst :=
    mkJ (j_view j) (j_bumps j) (j_dirty j) (j_work j) o (j_stop j) (j_fired j) (j_trace j).
  Definition set_stop (c : cause) (j : jst) : jst :=
    mkJ (j_view j) (j_bumps j) (j_dirty j) (j_work j) (j_own j) (Some c) (j_fired j) (j_trace j).
  Definition with_fired (n : nat) (j : jst) : jst :=
    mkJ (j_view j) (j_bumps j) (j_dirty j) (j_work j) (j_own j) (j_stop j) n (j_trace j).

  (* ── 16. TOUCH, STORE, the second sends and the set retirements ─────── *)

  Definition uses_counter : bool := match sh with Head | Lanes => true | Scan | Classes => false end.

  (* CASCADES (plan 4.2, decisions 11 and 50, Engine::cascades): the head
     shapes and the set cascade. A removal can expose a fire there: a new
     head, or a set class that moves when its first member leaves and a
     persistent member stays (finding F-JF-1). A removal on a bag exposes
     no fire (ref_fill_mono), so the bag has no cascade. *)
  Definition cascades : bool := match sh with Head | Lanes | Classes => true | Scan => false end.

  (* TOUCH (plan 4.3): the bump of the counter of a collection before its
     first staging, once in a transaction (decision 31). *)
  Definition touch_scope (own : list nat) (c : nat) : bool := true.

  Definition touch (own : list nat) (j : jst) (c : nat) : jst :=
    if uses_counter && touch_scope own c && negb (mem c (j_bumps j))
    then with_bumps (c :: j_bumps j) (stage j (SBump c))
    else j.

  Definition touch_all (own : list nat) (j : jst) (cs : list nat) : jst := fold_left (touch own) cs j.

  (* An element of OFFER: the record, the authority that its stagings
     register, and whether it is the operation's own element. *)
  Record elem : Type := mkElem { e_rec : rec; e_auth : A; e_own : bool }.

  Definition store (own : list nat) (j : jst) (x : elem) : jst :=
    let j1 := touch own j (r_coll (e_rec x)) in
    let j2 := add_dirty [r_coll (e_rec x)] (stage j1 (SStore (e_rec x) (e_auth x))) in
    if e_own x then set_own OStored j2 else j2.

  (* PUSH-SECOND-SENDS (plan 4.8, decision 30, DV-3): in receipt (group)
     order, the element in flight at the incoming position, whether or not
     it is persistent, and each other result that is not persistent. The
     incoming position is in flight in OFFER. In LIFT-AND-REOFFER it is in
     flight only when the cascade lifted its record (inflight). The
     element in flight keeps its own flag. The work is LIFO: the pushes go
     from the last result down to the first, so the pop order is the
     receipt order. *)
  Definition is_inc (inc : option nat) (k : nat) : bool := match inc with Some p => k =? p | None => false end.

  Definition sends_at (inc : option nat) (inflight : bool) (krm : nat * (rec * M)) : bool :=
    if is_inc inc (fst krm) then inflight else negb (r_pers (fst (snd krm))).

  Definition np_results (sg : list (rec * M)) (inc : option nat) (own_inc inflight : bool) : list pend :=
    map (fun krm : nat * (rec * M) => mkPend (fst (snd krm)) (own_inc && is_inc inc (fst krm)))
        (filter (sends_at inc inflight) (enum 0 sg)).

  Definition push_second_sends (j : jst) (l : list pend) : jst :=
    with_work (fold_left (fun w q => q :: w) (rev l) (j_work j)) j.

  (* RETIRE-SET-BINDS (plan 4.7): after a fire of a waiter that does not
     peek, the K_retire of every other stored non-persistent member of each
     bound class. *)
  Definition retire_positions (inc : option nat) (n : nat) : list nat := seq 0 n.

  Definition class_members (c k : nat) (v : view) : list rec :=
    filter (fun e => negb (r_pers e) && (class_key e =? k)) (rorder (recs_of c v)).

  Definition retire_class (j : jst) (c k : nat) : jst :=
    fold_left (fun j0 e => stage j0 (SRetire e)) (class_members c k (j_view j)) j.

  Definition retire_set_binds (j : jst) (sg : list (rec * M)) (inc : option nat) : jst :=
    match sh with
    | Classes =>
        fold_left (fun j0 k => match nth_error sg k with
                               | Some (e, _) => retire_class j0 (r_coll e) (class_key e)
                               | None => j0
                               end) (retire_positions inc (length sg)) j
    | Head | Lanes | Scan => j
    end.

  (* After a fire: a peek pushes the second sends, and a fire of a waiter
     that does not peek retires the bound set classes. *)
  Definition fire_tail (W : wtr) (sg : list (rec * M)) (inc : option nat) (own_inc inflight : bool) (j : jst) : jst :=
    if w_peek W then push_second_sends j (np_results sg inc own_inc inflight) else retire_set_binds j sg inc.

  (* ── 17. OFFER, RECEIVE, LIFT-AND-REOFFER (plan 4.4 to 4.6) ──────────── *)

  Inductive fstep : Type := FDone | FContinue | FDivergent | FRoundCap.

  (* FireRounds::after_fire with the fire count after this fire. *)
  Definition after_fire (fired : nat) (spent stays : bool) : fstep :=
    if spent then FDone else if stays then FDivergent else if cap <=? fired then FRoundCap else FContinue.

  (* The collections that a fire makes dirty (the cascade runs over them). *)
  Definition fire_dirty (W : wtr) (c_in : nat) : list nat := colls W.

  (* The candidates of OFFER: the heads of the lanes of the collection of x
     (Head, Lanes), or every waiter of that collection (Scan, Classes), in
     the head order. *)
  Definition offer_cands (x : rec) (v : view) : list wtr :=
    match sh with
    | Head | Lanes => filter (fun W => at_head W (r_coll x) v) (worder (r_coll x) (waiters_of (r_coll x) v))
    | Scan | Classes => worder (r_coll x) (waiters_of (r_coll x) v)
    end.

  (* SEARCH-PRODUCE requires W at the head of every collection of its group. *)
  Definition heads_required (W : wtr) (x : rec) : list nat := colls W.

  Definition fires_with (W : wtr) (x : rec) (v : view) : option (nat * list (rec * M)) :=
    if heads_ok (heads_required W x) W v then
      match search_produce W x v with
      | Some (p, sg) => if guard_ok W sg then Some (p, sg) else None
      | None => None
      end
    else None.

  (* The first candidate that fires: a refusal moves to the next candidate
     (decisions 37, 38); for Head the head is the only candidate, so a head
     that does not fire blocks. *)
  Fixpoint offer_pick (x : rec) (v : view) (ws : list wtr) : option (wtr * nat * list (rec * M)) :=
    match ws with
    | [] => None
    | W :: rest =>
        match fires_with W x v with
        | Some (p, sg) => Some (W, p, sg)
        | None => offer_pick x v rest
        end
    end.

  Definition own_flight (x : elem) (j : jst) : jst := if e_own x then set_own OFlight j else j.

  (* One round of OFFER(x). The boolean asks for another round: x is
     persistent and the fired waiter left. *)
  Definition offer1 (own : list nat) (j : jst) (x : elem) : jst * bool :=
    match offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j)) with
    | None => (store own j x, false)
    | Some (W, p, sg) =>
        let j1 := touch_all own j (colls W) in
        let j2 := stage j1 (SFire W sg (Some (p, e_auth x))) in
        let j3 := add_dirty (fire_dirty W (r_coll (e_rec x))) (with_fired (S (j_fired j)) j2) in
        let st := after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb (w_peek W)) (w_pers W) in
        let j4 := fire_tail W sg (Some p) (e_own x) true j3 in
        if w_peek W then (j4, false) else
        match st with
        | FDone => ((if e_own x then set_own OSpent j4 else j4), false)
        | FContinue => (j4, true)
        | FDivergent => (own_flight x j4, false)
        | FRoundCap => (set_stop CRoundCap (own_flight x j4), false)
        end
    end.

  Fixpoint offer (fuel : nat) (own : list nat) (j : jst) (x : elem) : jst :=
    match fuel with
    | 0 => set_stop CRoundCap (own_flight x j)
    | S f => match offer1 own j x with (j', true) => offer f own j' x | (j', false) => j' end
    end.

  Definition park (j : jst) (W : wtr) : jst := set_own OParked (stage j (SPark W)).

  (* The selection of RECEIVE: AT-HEAD-INCOMING on every collection of the
     group, GREEDY, and the guard once. *)
  Definition receive_sel (v : view) (W : wtr) : option (list (rec * M)) :=
    if heads_ok (colls W) W (park_view W v) then receive_decision W v else None.

  (* The partner of a consume-side fire stays (the argument partner_stays
     of FireRounds::after_fire): the waiter peeks, or every bound record is
     persistent. A single-bind fire on a set binds the whole class of its
     representative ([D; n], F3, F20, class_candidates), so its partner
     stays when a member of the class is persistent. *)
  Definition class_pers (c k : nat) (v : view) : bool :=
    existsb (fun e => r_pers e && (class_key e =? k)) (recs_of c v).

  Definition rcv_stays (W : wtr) (sg : list (rec * M)) (v : view) : bool :=
    w_peek W ||
    match sh with
    | Classes => match sg with
                 | [(r, _)] => class_pers (r_coll r) (class_key r) v
                 | _ => forallb (fun rm => r_pers (fst rm)) sg
                 end
    | Head | Lanes | Scan => forallb (fun rm => r_pers (fst rm)) sg
    end.

  (* One round of RECEIVE(W) for the operation's own waiter W. *)
  Definition receive1 (j : jst) (W : wtr) : jst * bool :=
    match receive_sel (j_view j) W with
    | None => (park j W, false)
    | Some sg =>
        let j2 := stage j (SFire W sg None) in
        let j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) j2) in
        let st := after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)) in
        let j4 := fire_tail W sg None false false j3 in
        match st with
        | FDone => (set_own OSpent j4, false)
        | FContinue => (j4, true)
        | FDivergent => (set_own OFlight j4, false)
        | FRoundCap => (set_stop CRoundCap (set_own OFlight j4), false)
        end
    end.

  Fixpoint receive (fuel : nat) (j : jst) (W : wtr) : jst :=
    match fuel with
    | 0 => set_stop CRoundCap (set_own OFlight j)
    | S f => match receive1 j W with (j', true) => receive f j' W | (j', false) => j' end
    end.

  (* LIFT-AND-REOFFER (plan 4.6): the least position with a non-persistent
     record is lifted and offered again, and a persistent record is never
     lifted. The re-offered record is in flight only when it was lifted. A
     peek pushes the second sends. A fire that does not peek retires the
     bound set classes, with the re-offered record as the element (decision
     50). *)
  Fixpoint first_np (k : nat) (sg : list (rec * M)) : option nat :=
    match sg with [] => None | rm :: r => if r_pers (fst rm) then first_np (S k) r else Some k end.

  Definition reoffer_pos (sg : list (rec * M)) : nat := match first_np 0 sg with Some k => k | None => 0 end.

  Definition lift_ops (r : rec) : list staged := if r_pers r then [] else [SLift r].

  Definition lift_reoffer (own : list nat) (j : jst) (W : wtr) (sg : list (rec * M)) : jst :=
    let vp := reoffer_pos sg in
    match nth_error sg vp with
    | None => j
    | Some (rv, _) =>
        let j1 := touch_all own j (colls W) in
        let j2 := stage_all j1 (lift_ops rv) in
        let j3 := stage j2 (SFire W sg (Some (vp, own_auth rv))) in
        let j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j)) j3) in
        let st := after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W) in
        let j5 := fire_tail W sg (Some vp) false (negb (r_pers rv)) j4 in
        match st with
        | FDivergent => set_stop CDivergent j5
        | FRoundCap => set_stop CRoundCap j5
        | FDone | FContinue => j5
        end
    end.

  (* ── 18. DRAIN-AND-CASCADE (plan 4.9) ──────────────────────────────── *)

  (* FIREABLE(W): the head of every collection of its group, GREEDY, and the
     guard once. *)
  Definition fireable (W : wtr) (v : view) : option (list (rec * M)) :=
    if heads_ok (colls W) W v then receive_decision W v else None.

  Definition heads_of (c : nat) (v : view) : list wtr :=
    filter (fun W => at_head W c v) (worder c (waiters_of c v)).

  Fixpoint find_fire (ws : list wtr) (v : view) : option (wtr * list (rec * M)) :=
    match ws with
    | [] => None
    | W :: r => match fireable W v with Some sg => Some (W, sg) | None => find_fire r v end
    end.

  Fixpoint next_fire_in (cs : list nat) (v : view) : option (wtr * list (rec * M)) :=
    match cs with
    | [] => None
    | c :: r => match find_fire (heads_of c v) v with Some f => Some f | None => next_fire_in r v end
    end.

  (* NEXT-FIRE: the dirty collections in ascending key order. *)
  Definition next_fire (dirty : list nat) (v : view) : option (wtr * list (rec * M)) :=
    next_fire_in (isort Nat.leb (nodup Nat.eq_dec dirty)) v.

  (* A second send: OFFER of the record with its own authority
     (Introduction::for_record), or the operation's authority for its own
     element (d45 C.4). *)
  Definition resend_elem (opauth : A) (q : pend) : elem :=
    mkElem (p_rec q) (if p_own q then opauth else own_auth (p_rec q)) (p_own q).

  Definition resend (own : list nat) (opauth : A) (j : jst) (q : pend) : jst :=
    offer (S (length (v_wtrs (j_view j)))) own j (resend_elem opauth q).

  Definition dstep (own : list nat) (opauth : A) (j : jst) : jst * bool :=
    match j_stop j with
    | Some _ => (j, false)
    | None =>
        match j_work j with
        | q :: rest =>
            if cap <=? j_fired j then (set_stop CRoundCap j, false)
            else (resend own opauth (with_work rest j) q, true)
        | [] =>
            if cascades then
              match next_fire (j_dirty j) (j_view j) with
              | None => (j, false)
              | Some (W, sg) =>
                  if cap <=? j_fired j then (set_stop CRoundCap j, false)
                  else (lift_reoffer own j W sg, true)
              end
            else (j, false)
        end
    end.

  Fixpoint drain (fuel : nat) (own : list nat) (opauth : A) (j : jst) : jst :=
    match fuel with
    | 0 => set_stop CRoundCap j
    | S f => match dstep own opauth j with (j', true) => drain f own opauth j' | (j', false) => j' end
    end.

  (* ── 19. The operations, RUN-JOIN and COMMIT (plan 4.2, 4.10, 4.11) ─── *)

  Inductive op : Type :=
  | OProduce (x : rec) (carried : option nat) (auth : A)
  | OReceive (W : wtr) (carried : option (list nat)) (auth : A)
  | OSettle (key : nat) (seeds : list nat) (pending : list pend) (resume : option op) (auth : A).

  Definition own_of (o : op) : list nat :=
    match o with
    | OProduce x _ _ => [r_coll x]
    | OReceive W _ _ => colls W
    | OSettle key _ _ _ _ => [key]
    end.

  Definition auth_of (o : op) : A :=
    match o with OProduce _ _ a | OReceive _ _ a | OSettle _ _ _ _ a => a end.

  Definition key_of (o : op) : nat :=
    match o with OProduce x _ _ => r_coll x | OReceive W _ _ => hd 0 (colls W) | OSettle k _ _ _ _ => k end.

  (* Decision 43: a round takes its carried first-round ord, or the counter
     value of its bump. *)
  Definition bump_value (v0 : view) (c : nat) : nat := if uses_counter then cnt_of c (v_cnt v0) else 0.

  Definition prod_ord (v0 : view) (x : rec) (carried : option nat) : nat :=
    match carried with Some o => o | None => bump_value v0 (r_coll x) end.

  Definition ords_of (v0 : view) (W : wtr) (carried : option (list nat)) : list nat :=
    match carried with Some os => os | None => map (fun b => bump_value v0 (b_coll b)) (w_binds W) end.

  Definition with_ord (x : rec) (o : nat) : rec := mkRec (r_id x) (r_coll x) o (r_lvl x) (r_val x) (r_pers x).

  Definition with_ords (W : wtr) (os : list nat) : wtr :=
    mkWtr (w_id W) (w_binds W) os (w_guard W) (w_pers W) (w_peek W).

  (* REISSUE-OP: a persistent element carries its first-round ords. *)
  Definition carried_ord (pers : bool) (o : nat) : option nat := if pers then Some o else None.
  Definition carried_ords (pers : bool) (os : list nat) : option (list nat) := if pers then Some os else None.

  Definition reissue_op (o : op) (v0 : view) : option op :=
    match o with
    | OProduce x c a => Some (OProduce x (carried_ord (r_pers x) (prod_ord v0 x c)) a)
    | OReceive W c a => Some (OReceive W (carried_ords (w_pers W) (ords_of v0 W c)) a)
    | OSettle _ _ _ resume _ => resume
    end.

  Definition is_nil {X : Type} (l : list X) : bool := match l with [] => true | _ :: _ => false end.

  (* The pending second sends of COMMIT (plan 4.10): the entries of the work
     that are not the element of the operation. The element of the
     operation in the work returns to its operation, which re-issues. *)
  Definition pending_work (j : jst) : list pend := filter (fun q => negb (p_own q)) (j_work j).

  (* The resume of the transaction (Engine::resume): a settle job resumes
     the operation that it carries. Another operation re-issues when its
     element is still in flight, in the work or not started (decision 43). *)
  Definition resume_of (o : op) (v0 : view) (j : jst) : option op :=
    match o with
    | OSettle _ _ _ rs _ => rs
    | OProduce _ _ _ | OReceive _ _ _ =>
        if existsb p_own (j_work j) then reissue_op o v0
        else match j_own j with ONot | OFlight => reissue_op o v0 | OSpent | OStored | OParked => None end
    end.

  (* The transaction owes work: a pending second send, or a dirty collection
     of a discipline that cascades. *)
  Definition owes (j : jst) : bool := negb (is_nil (pending_work j)) || (cascades && negb (is_nil (j_dirty j))).

  (* COMMIT: the re-issue of the transaction. A stop that owes work commits
     a settle job with its seeds, its pending second sends and its resume.
     Another end re-issues the resume only, so a stop that owes nothing and
     resumes nothing re-issues nothing (DV-5). *)
  Definition commit (o : op) (v0 : view) (j : jst) : option op :=
    match j_stop j with
    | Some _ =>
        if owes j
        then Some (OSettle (key_of o) (if cascades then remove Nat.eq_dec (key_of o) (j_dirty j) else [])
                           (pending_work j) (resume_of o v0 j) (auth_of o))
        else resume_of o v0 j
    | None => resume_of o v0 j
    end.

  (* One fire count for each transaction (decision 49): each phase continues
     the count of the phase before it. *)
  Definition phase_count (f : nat) : nat := f.

  (* open_plan: no plan opens at a full transaction. *)
  Definition plan_at_cap (f : nat) : bool := cap <=? f.

  Definition init (v0 : view) : jst := mkJ v0 [] [] [] ONot None 0 [].

  (* The plan: OFFER of the produced element, or RECEIVE of the waiter. A
     settle job has no plan, and run_join does not call plan for it. *)
  Definition plan (fuel : nat) (o : op) (v0 : view) (j : jst) : jst :=
    match o with
    | OProduce x c a => offer fuel (own_of o) j (mkElem (with_ord x (prod_ord v0 x c)) a true)
    | OReceive W c a => receive fuel j (with_ords W (ords_of v0 W c))
    | OSettle _ _ _ _ _ => j
    end.

  Definition is_settle (o : op) : bool := match o with OSettle _ _ _ _ _ => true | _ => false end.

  (* The pending second sends of a settle job load as records that are not
     the element of an operation (PendingProduce has no own flag). *)
  Definition pending_of (o : op) : list pend :=
    match o with OSettle _ _ pd _ _ => map (fun q => mkPend (p_rec q) false) pd | _ => [] end.

  (* RUN-JOIN (plan 4.2, Engine::run). TOUCH bumps the own collections. A
     settle job has no plan: it loads its pending second sends (DV-1),
     makes its scope dirty, drains, and commits. Another operation runs the
     settle before the plan when its discipline cascades, clears the dirty
     collections, opens the plan only below the cap (open_plan, DV-2), runs
     the plan and the drain after it, and commits. *)
  Definition run_join (fuel : nat) (o : op) (scope : list nat) (v0 : view) : jst * option op :=
    let own := own_of o in
    let opauth := auth_of o in
    let j1 := with_work (pending_of o) (touch_all own (init v0) own) in
    if is_settle o then
      let j3 := drain fuel own opauth (with_dirty scope j1) in (j3, commit o v0 j3)
    else
      let j3 := if cascades then drain fuel own opauth (with_dirty scope j1) else j1 in
      match j_stop j3 with
      | Some _ => (j3, commit o v0 j3)
      | None =>
          let j4 := with_dirty [] j3 in
          if plan_at_cap (j_fired j4)
          then (set_stop CRoundCap j4, commit o v0 (set_stop CRoundCap j4))
          else
            let j5 := plan fuel o v0 (with_fired (phase_count (j_fired j4)) j4) in
            let j6 := drain fuel own opauth (with_fired (phase_count (j_fired j5)) j5) in
            (j6, commit o v0 j6)
      end.

  (* ── 20. CLASSIFY and the closure (plan 4.1) ───────────────────────── *)

  Definition partners (c : nat) (v : view) : list nat := flat_map colls (waiters_of c v).

  Definition close_step (v : view) (comp : list nat) : list nat :=
    nodup Nat.eq_dec (comp ++ flat_map (fun c => partners c v) comp).

  Fixpoint close_iter (n : nat) (v : view) (comp : list nat) : list nat :=
    match n with 0 => comp | S n' => close_iter n' v (close_step v comp) end.

  Definition universe (v : view) (seeds : list nat) : list nat :=
    nodup Nat.eq_dec (seeds ++ flat_map colls (v_wtrs v)).

  Definition closure_depth (v : view) (seeds : list nat) : nat := length (universe v seeds).

  Definition closure (v : view) (seeds : list nat) : list nat :=
    close_iter (closure_depth v seeds) v (nodup Nat.eq_dec seeds).

  Definition seeds_of (o : op) : list nat :=
    match o with
    | OSettle key sd pd _ _ => key :: sd ++ map (fun q => r_coll (p_rec q)) pd
    | OProduce _ _ _ | OReceive _ _ _ => own_of o
    end.

  Definition join_scope (o : op) (v : view) : list nat := closure v (seeds_of o ++ own_of o).

  Definition covers (locked scope : list nat) : bool := forallb (fun c => mem c locked) scope.

  (* The lock check of the substrate: every staging is on a locked channel,
     else UnlockedChannel (fail closed, plan L3). *)
  Definition locked_ok (locked : list nat) (tr : list staged) : bool :=
    forallb (fun op0 => forallb (fun c => mem c locked) (staged_colls op0)) tr.

  (* PREPARE (plan 4.1): transaction 1 reads the closure on its view v1; when
     its own keys do not cover the scope, it escalates once and transaction
     2 runs on its view v2 under the lock keys of the scope. None is the
     UnlockedChannel failure. *)
  Definition prepare (fuel : nat) (o : op) (v1 v2 : view) : option (jst * option op) :=
    let scope := join_scope o v1 in
    let locked := if covers (own_of o) scope then own_of o else scope in
    let v := if covers (own_of o) scope then v1 else v2 in
    let r := run_join fuel o scope v in
    if locked_ok locked (j_trace (fst r)) then Some r else None.

  (* ── 21. Generic facts of the engine ──────────────────────────────── *)

  Lemma apply_all_app : forall v a b, apply_all v (a ++ b) = apply_all (apply_all v a) b.
  Proof. intros v a b. unfold apply_all. apply fold_left_app. Qed.

  Lemma stage_all_fields :
    forall ops j,
      j_trace (stage_all j ops) = j_trace j ++ ops /\ j_view (stage_all j ops) = apply_all (j_view j) ops /\
      j_bumps (stage_all j ops) = j_bumps j /\ j_dirty (stage_all j ops) = j_dirty j /\
      j_work (stage_all j ops) = j_work j /\ j_own (stage_all j ops) = j_own j /\
      j_stop (stage_all j ops) = j_stop j /\ j_fired (stage_all j ops) = j_fired j.
  Proof.
    induction ops as [| op r IH]; intros j.
    - cbn. rewrite app_nil_r. repeat split; reflexivity.
    - cbn [stage_all fold_left]. fold (stage_all (stage j op) r).
      destruct (IH (stage j op)) as (T & Vw & Bm & D & Wk & O & St & F).
      rewrite T, Vw, Bm, D, Wk, O, St, F. cbn [stage j_trace j_view j_bumps j_dirty j_work j_own j_stop j_fired].
      rewrite <- app_assoc. repeat split; reflexivity.
  Qed.

  Lemma del_rec_comm : forall i k l, del_rec i (del_rec k l) = del_rec k (del_rec i l).
  Proof.
    intros i k l. induction l as [| r rest IH]; [reflexivity |]. cbn [del_rec].
    destruct (Nat.eqb_spec (r_id r) k) as [Ek | Ek]; destruct (Nat.eqb_spec (r_id r) i) as [Ei | Ei].
    - subst. reflexivity.
    - cbn [del_rec]. rewrite (proj2 (Nat.eqb_eq _ _) Ek). reflexivity.
    - cbn [del_rec]. rewrite (proj2 (Nat.eqb_eq _ _) Ei). reflexivity.
    - cbn [del_rec]. rewrite (proj2 (Nat.eqb_neq _ _) Ek), (proj2 (Nat.eqb_neq _ _) Ei), IH. reflexivity.
  Qed.

  Lemma remove_all_cons : forall r rs l, remove_all (r :: rs) l = remove_all rs (del_rec (r_id r) l).
  Proof. reflexivity. Qed.

  Lemma remove_all_del : forall rs i l, remove_all rs (del_rec i l) = del_rec i (remove_all rs l).
  Proof.
    induction rs as [| r rest IH]; intros i l; [reflexivity |].
    rewrite !remove_all_cons, del_rec_comm. apply IH.
  Qed.

  Lemma remove_all_perm : forall rs rs' l, Permutation rs rs' -> remove_all rs l = remove_all rs' l.
  Proof.
    intros rs rs' l0 H. revert l0. induction H; intros l0.
    - reflexivity.
    - rewrite !remove_all_cons. apply IHPermutation.
    - rewrite !remove_all_cons, del_rec_comm. reflexivity.
    - rewrite IHPermutation1. apply IHPermutation2.
  Qed.

  Lemma remove_all_app : forall a b l, remove_all (a ++ b) l = remove_all b (remove_all a l).
  Proof. intros a b l. unfold remove_all. apply fold_left_app. Qed.

  Lemma del_wtr_comm : forall i k l, del_wtr i (del_wtr k l) = del_wtr k (del_wtr i l).
  Proof.
    intros i k l. induction l as [| W rest IH]; [reflexivity |]. cbn [del_wtr].
    destruct (Nat.eqb_spec (w_id W) k) as [Ek | Ek]; destruct (Nat.eqb_spec (w_id W) i) as [Ei | Ei].
    - subst. reflexivity.
    - cbn [del_wtr]. rewrite (proj2 (Nat.eqb_eq _ _) Ek). reflexivity.
    - cbn [del_wtr]. rewrite (proj2 (Nat.eqb_eq _ _) Ei). reflexivity.
    - cbn [del_wtr]. rewrite (proj2 (Nat.eqb_neq _ _) Ek), (proj2 (Nat.eqb_neq _ _) Ei), IH. reflexivity.
  Qed.

  Lemma del_wtr_incl : forall i l, incl (del_wtr i l) l.
  Proof.
    intros i l. induction l as [| W rest IH]; [intros a [] |]. cbn [del_wtr].
    destruct (w_id W =? i); [intros a Ha; right; exact Ha |].
    intros a [<- | Ha]; [left; reflexivity | right; exact (IH a Ha)].
  Qed.

  Lemma nodup_wtr_ids_del : forall i l, NoDup (map w_id l) -> NoDup (map w_id (del_wtr i l)).
  Proof.
    intros i l. induction l as [| W r IH]; intros H; [constructor |]. cbn [del_wtr map] in *. apply NoDup_cons_iff in H as [Hn H].
    destruct (w_id W =? i); [exact H |]. cbn [map]. constructor; [| exact (IH H)].
    intros Hin. apply Hn. apply in_map_iff in Hin as (y & E & Hy). rewrite <- E. apply in_map. exact (del_wtr_incl _ _ y Hy).
  Qed.

  (* The positions of a fire around the incoming position. *)
  Lemma enum_split_at :
    forall (l : list (rec * M)) k vp y,
      nth_error l vp = Some y ->
      exists l1 l2, enum k l = l1 ++ (k + vp, y) :: l2 /\
                    (forall z, In z l1 -> fst z < k + vp) /\ (forall z, In z l2 -> k + vp < fst z).
  Proof.
    intros l. induction l as [| a r IH]; intros k vp y H; [destruct vp; discriminate |].
    destruct vp as [| vp]; cbn [nth_error] in H.
    - injection H as ->. exists [], (enum (S k) r). rewrite Nat.add_0_r. split; [reflexivity |].
      split; [intros z [] |]. intros [i z] Hz. apply enum_nth in Hz as [Hle _]. cbn [fst]. lia.
    - destruct (IH (S k) vp y H) as (l1 & l2 & E & H1 & H2). exists ((k, a) :: l1), l2.
      cbn [enum]. rewrite E. replace (S k + vp) with (k + S vp) by lia. split; [reflexivity |].
      split; [| intros z Hz; specialize (H2 z Hz); lia].
      intros z [<- | Hz]; [cbn [fst]; lia | specialize (H1 z Hz); lia].
  Qed.

  Lemma removed_split :
    forall sg vp rv mv a,
      nth_error sg vp = Some (rv, mv) ->
      exists A1 A2, removed sg None = A1 ++ (if r_pers rv then [] else [rv]) ++ A2 /\
                    removed sg (Some (vp, a)) = A1 ++ A2.
  Proof.
    intros sg vp rv mv a H. destruct (enum_split_at sg 0 vp (rv, mv) H) as (l1 & l2 & E & H1 & H2).
    cbn [Nat.add] in E, H1, H2. unfold removed.
    replace (enum 0 sg) with (l1 ++ [(vp, (rv, mv))] ++ l2) by (rewrite E; reflexivity).
    rewrite !filter_app, !map_app.
    assert (F1 : filter (fun krm : nat * (rec * M) => negb (inc_pos (Some (vp, a)) (fst krm)) && negb (r_pers (fst (snd krm)))) l1 =
                 filter (fun krm : nat * (rec * M) => negb (inc_pos None (fst krm)) && negb (r_pers (fst (snd krm)))) l1).
    { apply filter_ext_in. intros [k z] Hz. specialize (H1 _ Hz). cbn [fst inc_pos] in *.
      rewrite (proj2 (Nat.eqb_neq k vp)) by lia. reflexivity. }
    assert (F2 : filter (fun krm : nat * (rec * M) => negb (inc_pos (Some (vp, a)) (fst krm)) && negb (r_pers (fst (snd krm)))) l2 =
                 filter (fun krm : nat * (rec * M) => negb (inc_pos None (fst krm)) && negb (r_pers (fst (snd krm)))) l2).
    { apply filter_ext_in. intros [k z] Hz. specialize (H2 _ Hz). cbn [fst inc_pos] in *.
      rewrite (proj2 (Nat.eqb_neq k vp)) by lia. reflexivity. }
    rewrite F1, F2. cbn [filter fst snd inc_pos]. rewrite Nat.eqb_refl. cbn [negb andb map app].
    exists (map (fun krm : nat * (rec * M) => fst (snd krm))
              (filter (fun krm : nat * (rec * M) => negb (inc_pos None (fst krm)) && negb (r_pers (fst (snd krm)))) l1)).
    exists (map (fun krm : nat * (rec * M) => fst (snd krm))
              (filter (fun krm : nat * (rec * M) => negb (inc_pos None (fst krm)) && negb (r_pers (fst (snd krm)))) l2)).
    destruct (r_pers rv); split; reflexivity.
  Qed.

  (* ── 22. (Headline 11 is in section 27a: its trace statement uses
     fire_tail_spec of section 27.) ──────────────────────────────────── *)

  (* ── 23. Headline 6: the binds of a set retire their classes ─────────── *)

  Lemma del_rec_incl : forall i l, incl (del_rec i l) l.
  Proof.
    intros i l. induction l as [| r rest IH]; [intros a [] |]. cbn [del_rec].
    destruct (r_id r =? i); [intros a Ha; right; exact Ha |].
    intros a [<- | Ha]; [left; reflexivity | right; exact (IH a Ha)].
  Qed.

  Lemma del_rec_nodup : forall i l, NoDup (map r_id l) -> NoDup (map r_id (del_rec i l)).
  Proof.
    intros i l. induction l as [| r rest IH]; intros H; [constructor |]. cbn [del_rec map] in *.
    apply NoDup_cons_iff in H as [Hn H]. destruct (r_id r =? i); [exact H |].
    cbn [map]. constructor; [| exact (IH H)]. intros Hin. apply Hn.
    apply in_map_iff in Hin as (e & E & He). rewrite <- E. apply in_map. exact (del_rec_incl i rest e He).
  Qed.

  Lemma in_del_rec :
    forall i l e, NoDup (map r_id l) -> (In e (del_rec i l) <-> In e l /\ r_id e <> i).
  Proof.
    intros i l. induction l as [| r rest IH]; intros e H; [split; [intros [] | intros [[] _]] |].
    cbn [map] in H. apply NoDup_cons_iff in H as [Hn H]. cbn [del_rec].
    destruct (Nat.eqb_spec (r_id r) i) as [E | E].
    - split.
      + intros He. split; [right; exact He |]. intros Ei. apply Hn. rewrite E, <- Ei. apply in_map. exact He.
      + intros [[<- | He] Ne]; [contradiction | exact He].
    - split.
      + intros [<- | He]; [split; [left; reflexivity | exact E] |].
        apply (IH e H) in He as [He Ne]. split; [right; exact He | exact Ne].
      + intros [[<- | He] Ne]; [left; reflexivity | right; apply (IH e H); split; assumption].
  Qed.

  Lemma retire_fold :
    forall L j,
      j_view (fold_left (fun j0 e => stage j0 (SRetire e)) L j) =
        mkView (remove_all L (v_recs (j_view j))) (v_wtrs (j_view j)) (v_cnt (j_view j)) /\
      j_trace (fold_left (fun j0 e => stage j0 (SRetire e)) L j) = j_trace j ++ map SRetire L.
  Proof.
    induction L as [| e r IH]; intros j.
    - cbn. rewrite app_nil_r. destruct (j_view j); split; reflexivity.
    - cbn [fold_left map]. destruct (IH (stage j (SRetire e))) as [Hv Ht]. rewrite Hv, Ht.
      cbn [stage j_view j_trace apply v_recs v_wtrs v_cnt]. rewrite <- app_assoc. split; reflexivity.
  Qed.

  Lemma in_remove_all :
    forall L l e, NoDup (map r_id l) -> (In e (remove_all L l) <-> In e l /\ ~ In (r_id e) (map r_id L)).
  Proof.
    induction L as [| r rest IH]; intros l e H; [cbn; split; [intros He; split; [exact He | intros []] | intros [He _]; exact He] |].
    rewrite remove_all_cons, (IH _ e (del_rec_nodup _ _ H)), (in_del_rec _ _ e H). cbn [map In].
    split.
    - intros [[He Ne] Nr]. split; [exact He |]. intros [E | Hin]; [exact (Ne (eq_sym E)) | exact (Nr Hin)].
    - intros [He Nr]. split; [split; [exact He | intros E; apply Nr; left; exact (eq_sym E)] |].
      intros Hin. apply Nr. right. exact Hin.
  Qed.

  Lemma remove_all_nodup : forall L l, NoDup (map r_id l) -> NoDup (map r_id (remove_all L l)).
  Proof.
    induction L as [| r rest IH]; intros l H; [exact H |].
    rewrite remove_all_cons. apply IH, del_rec_nodup, H.
  Qed.

  Lemma remove_all_incl : forall L l, incl (remove_all L l) l.
  Proof.
    induction L as [| r rest IH]; intros l; [apply incl_refl |].
    rewrite remove_all_cons. intros a Ha. apply (del_rec_incl (r_id r) l). exact (IH _ a Ha).
  Qed.

  (* A clean class: every stored member of the class k in c is persistent. *)
  Definition clean_class (c k : nat) (v : view) : Prop :=
    forall e', In e' (recs_of c v) -> class_key e' = k -> r_pers e' = true.

  Lemma retire_class_clean :
    forall j c k, NoDup (map r_id (v_recs (j_view j))) -> clean_class c k (j_view (retire_class j c k)).
  Proof.
    intros j c k Hnd e' He Hk. unfold retire_class in He. rewrite (proj1 (retire_fold _ _)) in He.
    unfold recs_of in He. cbn [v_recs] in He. apply filter_In in He as [He Hc].
    apply (in_remove_all _ _ e' Hnd) in He as [He Nr].
    destruct (r_pers e') eqn:Ep; [reflexivity |]. exfalso. apply Nr. apply in_map.
    unfold class_members. apply filter_In. split.
    - apply (Permutation_in _ (Permutation_sym (rorder_perm _))). unfold recs_of. apply filter_In. split; assumption.
    - rewrite Ep, Hk, Nat.eqb_refl. reflexivity.
  Qed.

  Lemma retire_class_keeps :
    forall j c k, NoDup (map r_id (v_recs (j_view j))) ->
      NoDup (map r_id (v_recs (j_view (retire_class j c k)))) /\
      incl (v_recs (j_view (retire_class j c k))) (v_recs (j_view j)) /\
      (forall e', In e' (v_recs (j_view j)) -> r_pers e' = true -> In e' (v_recs (j_view (retire_class j c k)))) /\
      v_wtrs (j_view (retire_class j c k)) = v_wtrs (j_view j).
  Proof.
    intros j c k Hnd. unfold retire_class. rewrite (proj1 (retire_fold _ _)). cbn [v_recs v_wtrs].
    split; [apply remove_all_nodup, Hnd |]. split; [apply remove_all_incl |]. split; [| reflexivity].
    intros e' He Hp. apply (in_remove_all _ _ e' Hnd). split; [exact He |].
    intros Hin. apply in_map_iff in Hin as (e & E & Hm). unfold class_members in Hm. apply filter_In in Hm as [Hm Hf].
    apply (Permutation_in _ (rorder_perm _)) in Hm. unfold recs_of in Hm. apply filter_In in Hm as [Hm _].
    assert (e = e').
    { apply (nodup_map_inj _ _ r_id (v_recs (j_view j))); assumption. }
    subst e. rewrite Hp in Hf. discriminate.
  Qed.

  Lemma clean_class_keep :
    forall c k v v', incl (v_recs v') (v_recs v) -> clean_class c k v -> clean_class c k v'.
  Proof.
    intros c k v v' Hi Hc e' He Hk. unfold recs_of in He. apply filter_In in He as [He Hcol].
    apply Hc; [unfold recs_of; apply filter_In; split; [exact (Hi e' He) | exact Hcol] | exact Hk].
  Qed.

  (* Headline 6 (plan 4.7, F3). After a fire of a waiter that does not peek,
     RETIRE-SET-BINDS leaves no non-persistent stored member of a bound class:
     for each bound record, its class in its collection keeps only persistent
     members. A persistent member keeps its class. A fire of a waiter that
     peeks retires nothing: it only pushes the second sends. *)
  Theorem join_set_bind_retires_class :
    (sh = Classes ->
     forall j sg inc,
       NoDup (map r_id (v_recs (j_view j))) ->
       forall k e m, nth_error sg k = Some (e, m) ->
         clean_class (r_coll e) (class_key e) (j_view (retire_set_binds j sg inc))) /\
    (forall j sg inc e', NoDup (map r_id (v_recs (j_view j))) ->
       In e' (v_recs (j_view j)) -> r_pers e' = true -> In e' (v_recs (j_view (retire_set_binds j sg inc)))) /\
    (forall W sg inc oi fl j, w_peek W = true ->
       j_trace (fire_tail W sg inc oi fl j) = j_trace j /\ j_view (fire_tail W sg inc oi fl j) = j_view j).
  Proof.
    (* the fold over positions keeps the ids distinct and only removes records *)
    assert (Hfold : forall (sg : list (rec * M)) (ps : list nat) (j : jst), NoDup (map r_id (v_recs (j_view j))) ->
              let j' := fold_left (fun j0 k => match nth_error sg k with
                                               | Some (e, _) => retire_class j0 (r_coll e) (class_key e)
                                               | None => j0
                                               end) ps j in
              NoDup (map r_id (v_recs (j_view j'))) /\ incl (v_recs (j_view j')) (v_recs (j_view j)) /\
              (forall e', In e' (v_recs (j_view j)) -> r_pers e' = true -> In e' (v_recs (j_view j'))) /\
              (forall k e m, In k ps -> nth_error sg k = Some (e, m) -> clean_class (r_coll e) (class_key e) (j_view j'))).
    { intros sg ps. induction ps as [| k0 r IH]; intros j Hnd; cbn [fold_left].
      - split; [exact Hnd | split; [apply incl_refl | split; [intros e' He _; exact He | intros k e m []]]].
      - set (j1 := match nth_error sg k0 with
                   | Some (e, _) => retire_class j (r_coll e) (class_key e)
                   | None => j
                   end).
        assert (H1 : NoDup (map r_id (v_recs (j_view j1))) /\ incl (v_recs (j_view j1)) (v_recs (j_view j)) /\
                     (forall e', In e' (v_recs (j_view j)) -> r_pers e' = true -> In e' (v_recs (j_view j1)))).
        { unfold j1. destruct (nth_error sg k0) as [[e0 m0] |].
          - destruct (retire_class_keeps j (r_coll e0) (class_key e0) Hnd) as (Ka & Kb & Kc & _). split; [exact Ka | split; assumption].
          - split; [exact Hnd | split; [apply incl_refl | intros e' He _; exact He]]. }
        destruct H1 as (N1 & I1 & K1).
        destruct (IH j1 N1) as (N2 & I2 & K2 & C2).
        split; [exact N2 |]. split; [exact (incl_tran I2 I1) |]. split; [intros e' He Hp; exact (K2 e' (K1 e' He Hp) Hp) |].
        intros k e m [<- | Hk] Hn.
        + apply (clean_class_keep _ _ (j_view j1)); [exact I2 |].
          unfold j1. rewrite Hn. apply retire_class_clean, Hnd.
        + exact (C2 k e m Hk Hn). }
    split; [| split].
    - intros Hsh j sg inc Hnd k e m Hk. unfold retire_set_binds. rewrite Hsh.
      apply (proj2 (proj2 (proj2 (Hfold sg (retire_positions inc (length sg)) j Hnd))) k e m); [| exact Hk].
      unfold retire_positions. apply in_seq. split; [lia |]. cbn [Nat.add].
      apply nth_error_Some. rewrite Hk. discriminate.
    - intros j sg inc e' Hnd He Hp. unfold retire_set_binds. destruct sh; try exact He.
      exact (proj1 (proj2 (proj2 (Hfold sg (retire_positions inc (length sg)) j Hnd))) e' He Hp).
    - intros W sg inc oi fl j Hpk. unfold fire_tail. rewrite Hpk. unfold push_second_sends, with_work.
      split; reflexivity.
  Qed.

  (* ── 24. The frame of fireability ───────────────────────────────────── *)

  Lemma forallb_ext_in :
    forall (X : Type) (f g : X -> bool) l, (forall x, In x l -> f x = g x) -> forallb f l = forallb g l.
  Proof.
    intros X f g l H. induction l as [| x r IH]; [reflexivity |]. cbn [forallb].
    rewrite (H x (or_introl eq_refl)), IH; [reflexivity |]. intros y Hy. exact (H y (or_intror Hy)).
  Qed.

  Lemma in_lane_coll : forall W c L, in_lane W c L = true -> mem c (colls W) = true.
  Proof.
    intros W c L H. unfold in_lane in H. apply existsb_exists in H as (L0 & HL & _).
    unfold lanes_on in HL. apply in_map_iff in HL as (b & _ & Hb). apply filter_In in Hb as [Hb Hc].
    apply Nat.eqb_eq in Hc. apply mem_spec. unfold colls. apply nodup_In. rewrite <- Hc. apply in_map. exact Hb.
  Qed.

  Lemma filter_in_lane_waiters :
    forall c L v, filter (fun W => in_lane W c L) (v_wtrs v) = filter (fun W => in_lane W c L) (waiters_of c v).
  Proof.
    intros c L v. unfold waiters_of. induction (v_wtrs v) as [| W r IH]; [reflexivity |]. cbn [filter].
    destruct (in_lane W c L) eqn:E.
    - rewrite (in_lane_coll _ _ _ E). cbn [filter]. rewrite E, IH. reflexivity.
    - destruct (mem c (colls W)); [cbn [filter]; rewrite E |]; exact IH.
  Qed.

  Lemma at_head_frame : forall W c v v', waiters_of c v' = waiters_of c v -> at_head W c v' = at_head W c v.
  Proof.
    intros W c v v' H. unfold at_head. destruct sh; try reflexivity.
    all: apply forallb_ext_in; intros L _; unfold lane_queue; rewrite !filter_in_lane_waiters, H; reflexivity.
  Qed.

  Lemma gpairs_binds : forall W ib, In ib (gpairs W) -> In (snd ib) (w_binds W).
  Proof.
    intros W [i b] H. apply (Permutation_in _ (gpairs_perm W)) in H. apply enum_nth in H as [_ H].
    exact (nth_error_In _ _ H).
  Qed.

  Lemma bind_coll : forall W b, In b (w_binds W) -> In (b_coll b) (colls W).
  Proof. intros W b H. unfold colls. apply nodup_In. apply in_map. exact H. Qed.

  Lemma greedy_frame :
    forall W v v', (forall c, In c (colls W) -> recs_of c v' = recs_of c v) -> greedy W v' = greedy W v.
  Proof.
    intros W v v' H. unfold greedy, slots_cons. f_equal. apply map_ext_in. intros ib Hib.
    unfold win. rewrite (H _ (bind_coll _ _ (gpairs_binds _ _ Hib))). reflexivity.
  Qed.

  (* Fireability depends only on the records and the waiters of the
     collections of the group. *)
  Lemma fireable_frame :
    forall W v v',
      (forall c, In c (colls W) -> recs_of c v' = recs_of c v /\ waiters_of c v' = waiters_of c v) ->
      fireable W v' = fireable W v.
  Proof.
    intros W v v' H. unfold fireable, heads_ok, receive_decision.
    rewrite (forallb_ext_in _ (fun c => at_head W c v') (fun c => at_head W c v)); [| intros c Hc; exact (at_head_frame _ _ _ _ (proj2 (H c Hc)))].
    rewrite (greedy_frame W v v'); [reflexivity | intros c Hc; exact (proj1 (H c Hc))].
  Qed.

  Lemma filter_coll_del_rec :
    forall c id l, (forall e, In e l -> r_id e = id -> r_coll e <> c) ->
      filter (fun r => r_coll r =? c) (del_rec id l) = filter (fun r => r_coll r =? c) l.
  Proof.
    intros c id l. induction l as [| r rest IH]; intros H; [reflexivity |]. cbn [del_rec].
    destruct (Nat.eqb_spec (r_id r) id) as [E | E].
    - cbn [filter]. rewrite (proj2 (Nat.eqb_neq _ _) (H r (or_introl eq_refl) E)). reflexivity.
    - cbn [filter]. rewrite IH; [reflexivity |]. intros e He. exact (H e (or_intror He)).
  Qed.

  Lemma filter_coll_remove_all :
    forall c rs l, (forall r e, In r rs -> In e l -> r_id e = r_id r -> r_coll e <> c) ->
      filter (fun r => r_coll r =? c) (remove_all rs l) = filter (fun r => r_coll r =? c) l.
  Proof.
    intros c rs. induction rs as [| r rest IH]; intros l H; [reflexivity |].
    rewrite remove_all_cons, IH.
    - apply filter_coll_del_rec. intros e He E. exact (H r e (or_introl eq_refl) He E).
    - intros r' e Hr He E. exact (H r' e (or_intror Hr) (del_rec_incl _ _ e He) E).
  Qed.

  Lemma filter_mem_del_wtr :
    forall c id l, (forall W, In W l -> w_id W = id -> mem c (colls W) = false) ->
      filter (fun W => mem c (colls W)) (del_wtr id l) = filter (fun W => mem c (colls W)) l.
  Proof.
    intros c id l. induction l as [| W rest IH]; intros H; [reflexivity |]. cbn [del_wtr].
    destruct (Nat.eqb_spec (w_id W) id) as [E | E].
    - cbn [filter]. rewrite (H W (or_introl eq_refl) E). reflexivity.
    - cbn [filter]. rewrite IH; [reflexivity |]. intros W' Hw. exact (H W' (or_intror Hw)).
  Qed.

  (* The records that a fire removes are bound records. *)
  Lemma removed_incl : forall sg inc r, In r (removed sg inc) -> In r (map fst sg).
  Proof.
    intros sg inc r H. unfold removed in H. apply in_map_iff in H as ([k [e m]] & <- & H).
    apply filter_In in H as [H _]. apply (enum_nth _ sg 0) in H as [_ H]. cbn [fst snd].
    apply in_map_iff. exists (e, m). split; [reflexivity | exact (nth_error_In _ _ H)].
  Qed.

  (* A greedy fire binds records of the collections of its group. *)
  Lemma greedy_colls :
    forall W v sg, greedy W v = Some sg -> forall e, In e (map fst sg) -> In e (v_recs v) /\ In (r_coll e) (colls W).
  Proof.
    intros W v sg H e He. unfold greedy in H. pose proof (fill_shape _ _ _ _ H) as Hr.
    apply in_map_iff in He as ([e0 m] & <- & Hin). apply In_nth_error in Hin as [i Hi]. cbn [fst].
    unfold slots_cons in Hr.
    assert (Hlen : i < length (gpairs W)).
    { rewrite <- (length_map (fun ib : nat * bnd => Pick (visible (snd ib) (win W (b_coll (snd ib)) v)) (b_pat (snd ib))) (gpairs W)).
      rewrite <- (slots_rel_length _ _ Hr). apply nth_error_Some. rewrite Hi. discriminate. }
    destruct (nth_error (gpairs W) i) as [ib |] eqn:Hg; [| apply nth_error_None in Hg; lia].
    pose proof (slots_rel_nth _ _ i _ _ Hr (eq_trans (nth_error_map _ _ _) (f_equal _ Hg)) Hi) as Hm. cbn [fst] in Hm.
    apply visible_incl in Hm. unfold win in Hm. apply window_incl in Hm.
    apply (Permutation_in _ (rorder_perm _)) in Hm. unfold recs_of in Hm. apply filter_In in Hm as [Hm Hc].
    apply Nat.eqb_eq in Hc. split; [exact Hm |]. rewrite Hc. exact (bind_coll _ _ (gpairs_binds _ _ (nth_error_In _ _ Hg))).
  Qed.

  (* ── 25. Headline 10: two fireable heads commute (single head) ───────── *)

  (* Headline 10 (plan 4.9, confluence). For a single-head discipline
     (queue, stack, cell), two distinct fireable waiters are each the head of
     every collection of their groups, so their groups are disjoint. Each
     stays fireable with the same records after the fire of the other, and
     the two fires (direct COMMs; headline 11 equates the cascade fire with
     one) commute. *)
  Theorem cascade_order_independent :
    sh = Head ->
    forall v W1 W2 sg1 sg2,
      NoDup (map r_id (v_recs v)) -> NoDup (map w_id (v_wtrs v)) ->
      In W1 (v_wtrs v) -> In W2 (v_wtrs v) -> w_id W1 <> w_id W2 ->
      w_peek W1 = false -> w_peek W2 = false ->
      fireable W1 v = Some sg1 -> fireable W2 v = Some sg2 ->
      (forall c, In c (colls W1) -> ~ In c (colls W2)) /\
      fireable W2 (apply v (SFire W1 sg1 None)) = Some sg2 /\
      fireable W1 (apply v (SFire W2 sg2 None)) = Some sg1 /\
      apply (apply v (SFire W1 sg1 None)) (SFire W2 sg2 None) =
        apply (apply v (SFire W2 sg2 None)) (SFire W1 sg1 None).
  Proof.
    intros Hsh v W1 W2 sg1 sg2 Hndr Hndw Hw1 Hw2 Hne _ _ H1 H2.
    assert (Hheads : forall W sg c, fireable W v = Some sg -> In c (colls W) -> at_head W c v = true).
    { intros W sg c H Hc. unfold fireable in H. destruct (heads_ok (colls W) W v) eqn:Eh; [| discriminate].
      unfold heads_ok in Eh. rewrite forallb_forall in Eh. exact (Eh c Hc). }
    assert (Hgr : forall W sg, fireable W v = Some sg -> greedy W v = Some sg).
    { intros W sg H. unfold fireable in H. destruct (heads_ok (colls W) W v); [| discriminate].
      unfold receive_decision in H. destruct (greedy W v) as [sg' |]; [| discriminate].
      destruct (guard_ok W sg'); [exact H | discriminate]. }
    (* a single-head discipline has the one lane None on each collection *)
    assert (Hlane : forall W c, In c (colls W) -> at_head W c v = true ->
              match lane_queue c None (v_wtrs v) with W0 :: _ => w_id W0 = w_id W | [] => False end).
    { intros W c Hc Ha. unfold at_head in Ha. rewrite Hsh in Ha. rewrite forallb_forall in Ha.
      unfold colls in Hc. apply nodup_In in Hc. apply in_map_iff in Hc as (b & Eb & Hb).
      assert (HL : In None (lanes_on W c)).
      { unfold lanes_on. apply in_map_iff. exists b. split; [unfold lane_of; rewrite Hsh; reflexivity |].
        apply filter_In. split; [exact Hb | rewrite Eb; apply Nat.eqb_refl]. }
      specialize (Ha None HL). destruct (lane_queue c None (v_wtrs v)) as [| W0 r]; [discriminate |].
      apply Nat.eqb_eq in Ha. exact Ha. }
    assert (Hdisj : forall c, In c (colls W1) -> ~ In c (colls W2)).
    { intros c Hc1 Hc2. pose proof (Hlane W1 c Hc1 (Hheads _ _ _ H1 Hc1)) as L1.
      pose proof (Hlane W2 c Hc2 (Hheads _ _ _ H2 Hc2)) as L2.
      destruct (lane_queue c None (v_wtrs v)) as [| W0 r]; [contradiction |]. apply Hne. rewrite <- L1, <- L2. reflexivity. }
    (* the frame of one fire on the other waiter *)
    assert (Hframe : forall Wa sga Wb sgb, fireable Wa v = Some sga -> fireable Wb v = Some sgb -> In Wa (v_wtrs v) ->
               (forall c, In c (colls Wa) -> ~ In c (colls Wb)) ->
               fireable Wb (apply v (SFire Wa sga None)) = Some sgb).
    { intros Wa sga Wb sgb Ha Hb HwA Hd. rewrite <- Hb. apply fireable_frame. intros c Hc. split.
      - unfold recs_of. cbn [apply v_recs]. apply filter_coll_remove_all.
        intros r e Hr He E. apply removed_incl in Hr.
        destruct (greedy_colls Wa v sga (Hgr _ _ Ha) r Hr) as [Hrv Hrc].
        assert (e = r) by exact (nodup_map_inj _ _ r_id _ e r Hndr He Hrv E). subst e.
        intros Ec. rewrite Ec in Hrc. exact (Hd c Hrc Hc).
      - unfold waiters_of. cbn [apply v_wtrs]. destruct (w_pers Wa); [reflexivity |].
        apply filter_mem_del_wtr. intros W' HW' E.
        assert (W' = Wa) by exact (nodup_map_inj _ _ w_id _ W' Wa Hndw HW' HwA E). subst W'.
        apply mem_false. intros Hc'. exact (Hd c Hc' Hc). }
    split; [exact Hdisj |]. split; [exact (Hframe W1 sg1 W2 sg2 H1 H2 Hw1 Hdisj) |].
    split; [apply (Hframe W2 sg2 W1 sg1 H2 H1 Hw2); intros c Hc Hc'; exact (Hdisj c Hc' Hc) |].
    cbn [apply v_recs v_wtrs v_cnt]. f_equal.
    - rewrite <- !remove_all_app. apply remove_all_perm. apply Permutation_app_comm.
    - destruct (w_pers W1), (w_pers W2); try reflexivity. apply del_wtr_comm.
  Qed.

  (* ── 26. Headline 4: every fire is at the heads ──────────────────────── *)

  (* Headline 4 (decision 11 for each collection). Each of the three fire
     sites of the engine selects its waiter at the head of each lane of
     every collection of its group, and binds stored records of its windows:
     OFFER (a candidate of offer_pick, with the stored records in the
     windows with x placed and x at its position), RECEIVE (AT-HEAD-INCOMING,
     then GREEDY over the windows) and the cascade (NEXT-FIRE, then
     FIREABLE). Bag and set have no head condition, so the condition holds
     for them at once. *)
  Theorem join_fires_heads_only :
    (forall x v W p sg, offer_pick x v (offer_cands x v) = Some (W, p, sg) ->
       heads_ok (colls W) W v = true /\ In W (v_wtrs v) /\ slots_rel (slots_prod W x p v) sg /\
       (exists m, nth_error sg p = Some (x, m))) /\
    (forall v W sg, receive_sel v W = Some sg ->
       heads_ok (colls W) W (park_view W v) = true /\ slots_rel (slots_cons W v) sg) /\
    (forall d v W sg, next_fire d v = Some (W, sg) ->
       heads_ok (colls W) W v = true /\ In W (v_wtrs v) /\ slots_rel (slots_cons W v) sg).
  Proof.
    split; [| split].
    - intros x v W p sg H.
      assert (G : forall ws, incl ws (v_wtrs v) -> offer_pick x v ws = Some (W, p, sg) ->
                    In W (v_wtrs v) /\ fires_with W x v = Some (p, sg)).
      { induction ws as [| W0 r IH]; intros Hi Hp; [discriminate |]. cbn [offer_pick] in Hp.
        destruct (fires_with W0 x v) as [[p0 sg0] |] eqn:Ef.
        - injection Hp as <- <- <-. split; [exact (Hi W0 (or_introl eq_refl)) | exact Ef].
        - apply IH; [intros a Ha; exact (Hi a (or_intror Ha)) | exact Hp]. }
      destruct (G (offer_cands x v)) as [Hw Hf]; [| exact H |].
      { intros a Ha. unfold offer_cands in Ha.
        assert (Ha' : In a (worder (r_coll x) (waiters_of (r_coll x) v))) by (destruct sh; try exact Ha; apply filter_In in Ha as [Ha _]; exact Ha).
        apply (Permutation_in _ (worder_perm _ _)) in Ha'. unfold waiters_of in Ha'. apply filter_In in Ha' as [Ha' _]. exact Ha'. }
      unfold fires_with in Hf. destruct (heads_ok (heads_required W x) W v) eqn:Eh; [| discriminate].
      destruct (search_produce W x v) as [[p0 sg0] |] eqn:Es; [| discriminate].
      destruct (guard_ok W sg0); [| discriminate]. injection Hf as -> ->.
      unfold heads_required in Eh. split; [exact Eh |]. split; [exact Hw |].
      unfold search_produce in Es. pose proof Es as Es'. apply search_at_some in Es' as (i & b & m & _ & _ & _ & _ & Hfill).
      split; [exact (fill_shape _ _ _ _ Hfill) |].
      exact (proj1 (proj2 (join_incoming_binds_once W x v p sg Es))).
    - intros v W sg H. unfold receive_sel in H. destruct (heads_ok (colls W) W (park_view W v)) eqn:Eh; [| discriminate].
      split; [reflexivity |]. unfold receive_decision in H. destruct (greedy W v) as [sg' |] eqn:Eg; [| discriminate].
      destruct (guard_ok W sg'); [| discriminate]. injection H as <-. unfold greedy in Eg. exact (fill_shape _ _ _ _ Eg).
    - intros d v W sg H. unfold next_fire in H.
      assert (G : forall cs, next_fire_in cs v = Some (W, sg) ->
                    exists c, In W (heads_of c v) /\ fireable W v = Some sg).
      { induction cs as [| c r IH]; intros Hn; [discriminate |]. cbn [next_fire_in] in Hn.
        destruct (find_fire (heads_of c v) v) as [[W0 sg0] |] eqn:Ef.
        - injection Hn as <- <-. exists c.
          assert (G2 : forall ws, find_fire ws v = Some (W0, sg0) -> In W0 ws /\ fireable W0 v = Some sg0).
          { induction ws as [| W1 r1 IH1]; intros Hf; [discriminate |]. cbn [find_fire] in Hf.
            destruct (fireable W1 v) as [sg1 |] eqn:E1.
            - injection Hf as <- <-. split; [left; reflexivity | exact E1].
            - destruct (IH1 Hf) as [Hi Hfr]. split; [right; exact Hi | exact Hfr]. }
          exact (G2 _ Ef).
        - exact (IH Hn). }
      destruct (G _ H) as (c & Hh & Hf).
      unfold heads_of in Hh. apply filter_In in Hh as [Hh _]. apply (Permutation_in _ (worder_perm _ _)) in Hh.
      unfold waiters_of in Hh. apply filter_In in Hh as [Hw _].
      unfold fireable in Hf. destruct (heads_ok (colls W) W v) eqn:Eh; [| discriminate].
      split; [reflexivity |]. split; [exact Hw |].
      unfold receive_decision in Hf. destruct (greedy W v) as [sg' |] eqn:Eg; [| discriminate].
      destruct (guard_ok W sg'); [| discriminate]. injection Hf as <-. unfold greedy in Eg. exact (fill_shape _ _ _ _ Eg).
  Qed.

  (* ── 27. The fields of the engine's primitives ────────────────────── *)

  Definition is_fire (op0 : staged) : bool := match op0 with SFire _ _ _ => true | _ => false end.
  Definition fires (tr : list staged) : nat := length (filter is_fire tr).

  Lemma fires_app : forall a b, fires (a ++ b) = fires a + fires b.
  Proof. intros a b. unfold fires. rewrite filter_app, length_app. reflexivity. Qed.

  Lemma fires_bumps : forall bs, fires (map SBump bs) = 0.
  Proof. induction bs as [| c r IH]; [reflexivity | exact IH]. Qed.

  Lemma fires_retires : forall rs, fires (map SRetire rs) = 0.
  Proof. induction rs as [| c r IH]; [reflexivity | exact IH]. Qed.

  Lemma bumps_view :
    forall bs v, v_recs (apply_all v (map SBump bs)) = v_recs v /\ v_wtrs (apply_all v (map SBump bs)) = v_wtrs v.
  Proof.
    induction bs as [| c r IH]; intros v; [split; reflexivity |].
    cbn [map apply_all fold_left]. fold (apply_all (apply v (SBump c)) (map SBump r)).
    destruct (IH (apply v (SBump c))) as [H1 H2]. rewrite H1, H2. split; reflexivity.
  Qed.

  Lemma touch_spec :
    forall own j c,
      (touch own j c = j /\ (uses_counter = true -> touch_scope own c = true -> In c (j_bumps j))) \/
      (touch own j c = with_bumps (c :: j_bumps j) (stage j (SBump c)) /\ ~ In c (j_bumps j) /\ uses_counter = true).
  Proof.
    intros own j c. unfold touch.
    destruct (uses_counter && touch_scope own c && negb (mem c (j_bumps j))) eqn:E.
    - right. apply andb_true_iff in E as [E1 E2]. apply andb_true_iff in E1 as [Eu _].
      apply negb_true_iff, mem_false in E2. split; [reflexivity | split; assumption].
    - left. split; [reflexivity |]. intros Hu Ht. rewrite Hu, Ht in E. cbn [andb negb] in E.
      apply negb_false_iff, mem_spec in E. exact E.
  Qed.

  Lemma touch_all_spec :
    forall own cs j,
      exists bs,
        j_trace (touch_all own j cs) = j_trace j ++ map SBump bs /\
        j_view (touch_all own j cs) = apply_all (j_view j) (map SBump bs) /\
        j_bumps (touch_all own j cs) = rev bs ++ j_bumps j /\
        j_dirty (touch_all own j cs) = j_dirty j /\ j_work (touch_all own j cs) = j_work j /\
        j_own (touch_all own j cs) = j_own j /\ j_stop (touch_all own j cs) = j_stop j /\
        j_fired (touch_all own j cs) = j_fired j /\
        NoDup bs /\ (forall c, In c bs -> In c cs /\ ~ In c (j_bumps j) /\ uses_counter = true) /\
        (forall c, In c cs -> uses_counter = true -> touch_scope own c = true -> In c (j_bumps (touch_all own j cs))).
  Proof.
    intros own cs. induction cs as [| c r IH]; intros j.
    - exists []. cbn [touch_all fold_left map rev app apply_all]. rewrite app_nil_r.
      split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
      split; [reflexivity |]. split; [reflexivity |]. split; [reflexivity |].
      split; [reflexivity |]. split; [reflexivity |]. split; [constructor |].
      split; [intros c' [] | intros c' []].
    - cbn [touch_all fold_left]. fold (touch_all own (touch own j c) r).
      destruct (IH (touch own j c)) as (bs & T & Vw & B & D & Wk & O & St & F & Nd & Hb & Hcov).
      destruct (touch_spec own j c) as [[Et Hin] | (Et & Hn & Hu)]; rewrite Et in *.
      + exists bs. refine (conj T (conj Vw (conj B (conj D (conj Wk (conj O (conj St (conj F (conj Nd (conj _ _)))))))))).
        * intros c' Hc'. destruct (Hb c' Hc') as (H1 & H2 & H3). split; [right; exact H1 | split; assumption].
        * intros c' [<- | Hc'] Hu Ht; [rewrite B; apply in_or_app; right; exact (Hin Hu Ht) | exact (Hcov c' Hc' Hu Ht)].
      + exists (c :: bs). cbn [stage with_bumps j_trace j_view j_bumps j_dirty j_work j_own j_stop j_fired map] in *.
        rewrite T, Vw, B.
        refine (conj _ (conj _ (conj _ (conj D (conj Wk (conj O (conj St (conj F (conj _ (conj _ _)))))))))).
        * rewrite <- app_assoc. reflexivity.
        * reflexivity.
        * cbn [rev]. rewrite <- app_assoc. reflexivity.
        * constructor; [| exact Nd]. intros Hin. destruct (Hb c Hin) as (_ & Hn' & _). apply Hn'. left. reflexivity.
        * intros c' [<- | Hc']; [split; [left; reflexivity | split; [exact Hn | exact Hu]] |].
          destruct (Hb c' Hc') as (H1 & H2 & H3). split; [right; exact H1 |]. split; [| exact H3].
          intros Hin. apply H2. right. exact Hin.
        * intros c' [<- | Hc'] Hu' Ht.
          -- apply in_or_app. right. left. reflexivity.
          -- pose proof (Hcov c' Hc' Hu' Ht) as Hx. rewrite B in Hx. exact Hx.
  Qed.

  Lemma store_spec :
    forall own j x,
      exists bs,
        j_trace (store own j x) = j_trace j ++ map SBump bs ++ [SStore (e_rec x) (e_auth x)] /\
        j_view (store own j x) = apply_all (j_view j) (map SBump bs ++ [SStore (e_rec x) (e_auth x)]) /\
        j_bumps (store own j x) = rev bs ++ j_bumps j /\
        j_dirty (store own j x) = r_coll (e_rec x) :: j_dirty j /\ j_work (store own j x) = j_work j /\
        j_own (store own j x) = (if e_own x then OStored else j_own j) /\ j_stop (store own j x) = j_stop j /\
        j_fired (store own j x) = j_fired j /\
        NoDup bs /\ (forall c, In c bs -> c = r_coll (e_rec x) /\ ~ In c (j_bumps j) /\ uses_counter = true) /\
        (uses_counter = true -> touch_scope own (r_coll (e_rec x)) = true -> In (r_coll (e_rec x)) (j_bumps (store own j x))).
  Proof.
    intros own j x. unfold store.
    destruct (touch_spec own j (r_coll (e_rec x))) as [[Et Hin] | (Et & Hn & Hu)]; rewrite Et.
    - exists [].
      refine (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ _)))))))))).
      1-8: destruct (e_own x); reflexivity.
      + constructor.
      + intros c [].
      + intros Hu Ht. destruct (e_own x); exact (Hin Hu Ht).
    - exists [r_coll (e_rec x)].
      refine (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ (conj _ _)))))))))).
      1-8: destruct (e_own x); cbn; rewrite <- ?app_assoc; reflexivity.
      + constructor; [intros [] | constructor].
      + intros c [<- | []]. split; [reflexivity | split; assumption].
      + intros _ _. destruct (e_own x); left; reflexivity.
  Qed.

  Lemma fold_push : forall (L w : list pend), fold_left (fun w0 q => q :: w0) L w = rev L ++ w.
  Proof.
    induction L as [| q r IH]; intros w; [reflexivity |]. cbn [fold_left rev]. rewrite IH, <- app_assoc. reflexivity.
  Qed.

  Lemma push_spec :
    forall j l,
      Permutation (j_work (push_second_sends j l)) (l ++ j_work j) /\
      j_trace (push_second_sends j l) = j_trace j /\ j_view (push_second_sends j l) = j_view j /\
      j_bumps (push_second_sends j l) = j_bumps j /\ j_dirty (push_second_sends j l) = j_dirty j /\
      j_own (push_second_sends j l) = j_own j /\ j_stop (push_second_sends j l) = j_stop j /\
      j_fired (push_second_sends j l) = j_fired j.
  Proof.
    intros j l. unfold push_second_sends, with_work. cbn [j_work j_trace j_view j_bumps j_dirty j_own j_stop j_fired].
    rewrite fold_push. repeat split; try reflexivity.
    first [rewrite rev_involutive; apply Permutation_refl
          | apply Permutation_app_tail; apply Permutation_sym; apply Permutation_rev].
  Qed.

  Lemma retire_class_spec :
    forall j c k,
      exists rs,
        j_trace (retire_class j c k) = j_trace j ++ map SRetire rs /\
        j_view (retire_class j c k) = mkView (remove_all rs (v_recs (j_view j))) (v_wtrs (j_view j)) (v_cnt (j_view j)) /\
        j_bumps (retire_class j c k) = j_bumps j /\ j_dirty (retire_class j c k) = j_dirty j /\
        j_work (retire_class j c k) = j_work j /\ j_own (retire_class j c k) = j_own j /\
        j_stop (retire_class j c k) = j_stop j /\ j_fired (retire_class j c k) = j_fired j /\
        (forall e, In e rs -> In e (v_recs (j_view j)) /\ r_pers e = false /\ r_coll e = c).
  Proof.
    intros j c k. exists (class_members c k (j_view j)). unfold retire_class.
    destruct (retire_fold (class_members c k (j_view j)) j) as [Hv Ht]. rewrite Hv, Ht.
    assert (Hctl : forall L j0,
              let j1 := fold_left (fun j2 e => stage j2 (SRetire e)) L j0 in
              j_bumps j1 = j_bumps j0 /\ j_dirty j1 = j_dirty j0 /\ j_work j1 = j_work j0 /\
              j_own j1 = j_own j0 /\ j_stop j1 = j_stop j0 /\ j_fired j1 = j_fired j0).
    { induction L as [| e r IH]; intros j0; [repeat split; reflexivity |]. cbn [fold_left].
      destruct (IH (stage j0 (SRetire e))) as (B & D & W & O & St & F). rewrite B, D, W, O, St, F.
      repeat split; reflexivity. }
    destruct (Hctl (class_members c k (j_view j)) j) as (B & D & W & O & St & F).
    refine (conj eq_refl (conj eq_refl (conj B (conj D (conj W (conj O (conj St (conj F _)))))))).
    intros e He. unfold class_members in He. apply filter_In in He as [He Hf].
    apply andb_true_iff in Hf as [Hf _]. apply negb_true_iff in Hf.
    apply (Permutation_in _ (rorder_perm _)) in He. unfold recs_of in He. apply filter_In in He as [He Hc].
    apply Nat.eqb_eq in Hc. split; [exact He | split; [exact Hf | exact Hc]].
  Qed.

  Lemma retire_set_binds_spec :
    forall j sg inc,
      exists rs,
        j_trace (retire_set_binds j sg inc) = j_trace j ++ map SRetire rs /\
        j_view (retire_set_binds j sg inc) = mkView (remove_all rs (v_recs (j_view j))) (v_wtrs (j_view j)) (v_cnt (j_view j)) /\
        j_bumps (retire_set_binds j sg inc) = j_bumps j /\ j_dirty (retire_set_binds j sg inc) = j_dirty j /\
        j_work (retire_set_binds j sg inc) = j_work j /\ j_own (retire_set_binds j sg inc) = j_own j /\
        j_stop (retire_set_binds j sg inc) = j_stop j /\ j_fired (retire_set_binds j sg inc) = j_fired j /\
        (forall e, In e rs -> In e (v_recs (j_view j)) /\ r_pers e = false /\
                              exists k e0 m0, nth_error sg k = Some (e0, m0) /\ r_coll e = r_coll e0) /\
        (sh <> Classes -> rs = []).
  Proof.
    intros j sg inc. unfold retire_set_binds.
    assert (Hid : forall j0 : jst, j0 = j ->
              exists rs : list rec,
                j_trace j0 = j_trace j ++ map SRetire rs /\
                j_view j0 = mkView (remove_all rs (v_recs (j_view j))) (v_wtrs (j_view j)) (v_cnt (j_view j)) /\
                j_bumps j0 = j_bumps j /\ j_dirty j0 = j_dirty j /\ j_work j0 = j_work j /\ j_own j0 = j_own j /\
                j_stop j0 = j_stop j /\ j_fired j0 = j_fired j /\
                (forall e, In e rs -> In e (v_recs (j_view j)) /\ r_pers e = false /\
                                      exists k e0 m0, nth_error sg k = Some (e0, m0) /\ r_coll e = r_coll e0) /\
                (sh <> Classes -> rs = [])).
    { intros j0 ->. exists []. cbn [map remove_all fold_left]. rewrite app_nil_r.
      refine (conj eq_refl (conj _ (conj eq_refl (conj eq_refl (conj eq_refl (conj eq_refl (conj eq_refl (conj eq_refl (conj _ _))))))))).
      - destruct (j_view j). reflexivity.
      - intros e [].
      - intros _. reflexivity. }
    destruct sh eqn:Esh; try (apply Hid; reflexivity).
    assert (G : forall ps j0,
              exists rs,
                j_trace (fold_left (fun j1 k => match nth_error sg k with
                                                | Some (e, _) => retire_class j1 (r_coll e) (class_key e)
                                                | None => j1 end) ps j0) = j_trace j0 ++ map SRetire rs /\
                j_view (fold_left (fun j1 k => match nth_error sg k with
                                                | Some (e, _) => retire_class j1 (r_coll e) (class_key e)
                                                | None => j1 end) ps j0) =
                  mkView (remove_all rs (v_recs (j_view j0))) (v_wtrs (j_view j0)) (v_cnt (j_view j0)) /\
                (let j1 := fold_left (fun j1 k => match nth_error sg k with
                                                  | Some (e, _) => retire_class j1 (r_coll e) (class_key e)
                                                  | None => j1 end) ps j0 in
                 j_bumps j1 = j_bumps j0 /\ j_dirty j1 = j_dirty j0 /\ j_work j1 = j_work j0 /\
                 j_own j1 = j_own j0 /\ j_stop j1 = j_stop j0 /\ j_fired j1 = j_fired j0) /\
                (forall e, In e rs -> In e (v_recs (j_view j0)) /\ r_pers e = false /\
                                      exists k e0 m0, nth_error sg k = Some (e0, m0) /\ r_coll e = r_coll e0)).
    { induction ps as [| k0 r IH]; intros j0.
      - exists []. cbn [fold_left map remove_all]. rewrite app_nil_r.
        refine (conj eq_refl (conj _ (conj _ _))).
        + destruct (j_view j0). reflexivity.
        + repeat split; reflexivity.
        + intros e [].
      - cbn [fold_left]. destruct (nth_error sg k0) as [[e0 m0] |] eqn:Ek.
        + destruct (retire_class_spec j0 (r_coll e0) (class_key e0)) as (rs1 & T1 & V1 & B1 & D1 & W1 & O1 & S1 & F1 & H1).
          destruct (IH (retire_class j0 (r_coll e0) (class_key e0))) as (rs2 & T2 & V2 & (B2 & D2 & W2 & O2 & S2 & F2) & H2).
          exists (rs1 ++ rs2). rewrite T2, V2, T1, V1. cbn [v_recs v_wtrs v_cnt].
          rewrite map_app, app_assoc, remove_all_app.
          refine (conj eq_refl (conj eq_refl (conj _ _))).
          * rewrite B2, D2, W2, O2, S2, F2, B1, D1, W1, O1, S1, F1. repeat split; reflexivity.
          * intros e He. apply in_app_or in He as [He | He].
            -- destruct (H1 e He) as (Hin & Hp & Hc). split; [exact Hin | split; [exact Hp |]].
               exists k0, e0, m0. split; [exact Ek | exact Hc].
            -- destruct (H2 e He) as (Hin & Hp & Hc). rewrite V1 in Hin. cbn [v_recs] in Hin.
               split; [exact (remove_all_incl _ _ e Hin) | split; [exact Hp | exact Hc]].
        + exact (IH j0). }
    destruct (G (retire_positions inc (length sg)) j) as (rs & T & Vw & (B & D & W & O & St & F) & H).
    exists rs. refine (conj T (conj Vw (conj B (conj D (conj W (conj O (conj St (conj F (conj H _))))))))).
    intros Hn. exfalso. apply Hn. reflexivity.
  Qed.

  Lemma fire_tail_spec :
    forall W sg inc oi fl j,
      exists rs,
        j_trace (fire_tail W sg inc oi fl j) = j_trace j ++ map SRetire rs /\
        j_view (fire_tail W sg inc oi fl j) = mkView (remove_all rs (v_recs (j_view j))) (v_wtrs (j_view j)) (v_cnt (j_view j)) /\
        j_bumps (fire_tail W sg inc oi fl j) = j_bumps j /\ j_dirty (fire_tail W sg inc oi fl j) = j_dirty j /\
        Permutation (j_work (fire_tail W sg inc oi fl j)) ((if w_peek W then np_results sg inc oi fl else []) ++ j_work j) /\
        j_own (fire_tail W sg inc oi fl j) = j_own j /\
        j_stop (fire_tail W sg inc oi fl j) = j_stop j /\ j_fired (fire_tail W sg inc oi fl j) = j_fired j /\
        (forall e, In e rs -> In e (v_recs (j_view j)) /\ r_pers e = false /\
                              exists k e0 m0, nth_error sg k = Some (e0, m0) /\ r_coll e = r_coll e0) /\
        (w_peek W = true \/ sh <> Classes -> rs = []).
  Proof.
    intros W sg inc oi fl j. unfold fire_tail. destruct (w_peek W) eqn:Ep.
    - exists []. destruct (push_spec j (np_results sg inc oi fl)) as (Pw & T & Vw & B & D & O & St & F).
      rewrite T, Vw, B, D, O, St, F, app_nil_r.
      refine (conj eq_refl (conj _ (conj eq_refl (conj eq_refl (conj Pw (conj eq_refl (conj eq_refl (conj eq_refl (conj _ _))))))))).
      + destruct (j_view j). reflexivity.
      + intros e [].
      + intros _. reflexivity.
    - destruct (retire_set_binds_spec j sg inc) as (rs & T & Vw & B & D & Wk & O & St & F & H & Hc).
      exists rs. rewrite Wk. cbn [app].
      refine (conj T (conj Vw (conj B (conj D (conj (Permutation_refl _) (conj O (conj St (conj F (conj H _))))))))).
      intros [E | E]; [discriminate | exact (Hc E)].
  Qed.

  (* The second sends of a fire: each one is a bound record. It is a stored
     result that is not persistent and not the own element, or the element
     in flight at the incoming position with the own flag of the element. *)
  Lemma np_results_spec :
    forall sg inc oi fl q, In q (np_results sg inc oi fl) ->
      In (p_rec q) (map fst sg) /\
      ((r_pers (p_rec q) = false /\ p_own q = false) \/
       (fl = true /\ p_own q = oi /\ exists p m, inc = Some p /\ nth_error sg p = Some (p_rec q, m))).
  Proof.
    intros sg inc oi fl q H. unfold np_results in H. apply in_map_iff in H as ([k [e m]] & <- & H).
    apply filter_In in H as [H Hs]. apply (enum_nth _ sg 0) in H as [_ Hk]. rewrite Nat.sub_0_r in Hk.
    cbn [p_rec p_own fst snd]. split; [apply in_map_iff; exists (e, m); split; [reflexivity | exact (nth_error_In _ _ Hk)] |].
    unfold sends_at in Hs. cbn [fst snd] in Hs. destruct (is_inc inc k) eqn:Ei.
    - right. split; [destruct fl; [reflexivity | discriminate Hs] |]. split; [rewrite andb_true_r; reflexivity |].
      destruct inc as [p |]; [| discriminate]. cbn [is_inc] in Ei. apply Nat.eqb_eq in Ei. subst k.
      exists p, m. split; [reflexivity | exact Hk].
    - left. split; [apply negb_true_iff; exact Hs | rewrite andb_false_r; reflexivity].
  Qed.

  Lemma map_enum_filter :
    forall (f : rec -> bool) (sg : list (rec * M)) k,
      map (fun krm : nat * (rec * M) => fst (snd krm)) (filter (fun krm : nat * (rec * M) => f (fst (snd krm))) (enum k sg)) =
      filter f (map fst sg).
  Proof.
    intros f sg. induction sg as [| [e m] r IH]; intros k; [reflexivity |].
    cbn [enum filter map fst snd]. destruct (f e); cbn [map]; rewrite IH; reflexivity.
  Qed.

  (* The second sends of a consume-side peek fire: the bound records that
     are not persistent. *)
  Lemma np_results_none_recs :
    forall sg oi fl, map p_rec (np_results sg None oi fl) = filter (fun r => negb (r_pers r)) (map fst sg).
  Proof.
    intros sg oi fl. unfold np_results. rewrite map_map. cbn [p_rec].
    rewrite <- (map_enum_filter (fun r => negb (r_pers r)) sg 0).
    f_equal; apply filter_ext; intros [k [e m]]; unfold sends_at; reflexivity.
  Qed.

  (* The second sends of a peek fire of the cascade: the bound records that
     are not persistent, because the re-offered record is in flight only when
     the cascade lifted it. *)
  Lemma np_results_lift_recs :
    forall sg vp rv mv oi, nth_error sg vp = Some (rv, mv) ->
      map p_rec (np_results sg (Some vp) oi (negb (r_pers rv))) = filter (fun r => negb (r_pers r)) (map fst sg).
  Proof.
    intros sg vp rv mv oi Hv. unfold np_results. rewrite map_map. cbn [p_rec].
    rewrite <- (map_enum_filter (fun r => negb (r_pers r)) sg 0). f_equal.
    apply filter_ext_in. intros [k [e m]] Hin. apply (enum_nth _ sg 0) in Hin as [_ Hk]. rewrite Nat.sub_0_r in Hk.
    unfold sends_at. cbn [fst snd is_inc].
    destruct (Nat.eqb_spec k vp) as [-> | _]; [rewrite Hv in Hk; injection Hk as -> _ |]; destruct (r_pers e); reflexivity.
  Qed.

  Lemma resend_cases :
    forall own a j q,
      resend own a j q = offer (S (length (v_wtrs (j_view j)))) own j (resend_elem a q) \/
      resend own a j q = store own j (resend_elem a q).
  Proof. intros own a j q. unfold resend. first [left; reflexivity | right; reflexivity]. Qed.

  (* ── 27a. Headline 11: lift and re-offer equal one direct COMM ──────── *)

  (* Headline 11 (plan 4.6). Let the cascade fire W with the stored records
     sg. Lift and re-offer (the K_lift of the re-offered record, then the
     produce-side fire with that record incoming) leave the view of one
     direct COMM of the stored pair: the same records removed, the same
     waiter spent, and the same results, so the same values in bind order.
     The re-offer spends its element exactly when the COMM removes a record
     and the waiter does not peek, so it reports a divergent pair only when
     the COMM removes nothing (A10). The cascade stages the K_lift events,
     then the fire, after the bumps of TOUCH. A set fire that does not peek
     then retires the bound classes (decision 50), as a produce-side fire
     does, and no other fire adds a K_retire event. *)
  Theorem lift_reoffer_equals_direct_comm :
    forall v W sg rv mv a,
      nth_error sg (reoffer_pos sg) = Some (rv, mv) ->
      apply_all v (lift_ops rv ++ [SFire W sg (Some (reoffer_pos sg, a))]) = apply v (SFire W sg None) /\
      negb (r_pers rv) = existsb (fun rm => negb (r_pers (fst rm))) sg /\
      (forall own j, exists rs,
         j_trace (lift_reoffer own j W sg) =
           j_trace (touch_all own j (colls W)) ++ lift_ops rv ++ [SFire W sg (Some (reoffer_pos sg, own_auth rv))] ++
           map SRetire rs /\
         (w_peek W = true \/ sh <> Classes -> rs = [])).
  Proof.
    intros v W sg rv mv a H. split; [| split].
    - destruct (removed_split sg (reoffer_pos sg) rv mv a H) as (A1 & A2 & E1 & E2).
      rewrite apply_all_app. unfold lift_ops. destruct (r_pers rv) eqn:Ep.
      + cbn [apply_all fold_left apply]. rewrite E1, E2. try rewrite Ep. reflexivity.
      + cbn [apply_all fold_left apply v_recs v_wtrs v_cnt]. rewrite E1, E2. try rewrite Ep. cbn [app]. f_equal.
        rewrite <- remove_all_cons. apply remove_all_perm. apply Permutation_middle.
    - unfold reoffer_pos in H. revert H.
      assert (G : forall sg k, match first_np k sg with
                               | Some i => exists rm, nth_error sg (i - k) = Some rm /\ r_pers (fst rm) = false /\ k <= i
                               | None => forallb (fun rm => r_pers (fst rm)) sg = true
                               end).
      { induction sg0 as [| rm r IH]; intros k; [reflexivity |]. cbn [first_np].
        destruct (r_pers (fst rm)) eqn:Ep.
        + specialize (IH (S k)). destruct (first_np (S k) r) as [i |].
          * destruct IH as (rm' & Hn & Hp & Hle). exists rm'. split; [| split; [exact Hp | lia]].
            replace (i - k) with (S (i - S k)) by lia. exact Hn.
          * cbn [forallb]. rewrite Ep, IH. reflexivity.
        + exists rm. rewrite Nat.sub_diag. split; [reflexivity | split; [exact Ep | lia]]. }
      specialize (G sg 0). destruct (first_np 0 sg) as [i |]; intros Hn.
      + destruct G as (rm & Hn' & Hp & _). rewrite Nat.sub_0_r, Hn in Hn'. injection Hn' as <-.
        cbn [fst] in Hp. rewrite Hp. symmetry. apply existsb_exists.
        exists (rv, mv). split; [exact (nth_error_In _ _ Hn) | cbn [fst]; rewrite Hp; reflexivity].
      + assert (Hp : r_pers rv = true).
        { apply forallb_forall with (x := (rv, mv)) in G; [exact G | exact (nth_error_In _ _ Hn)]. }
        rewrite Hp. symmetry. apply Bool.not_true_iff_false. intros Hex.
        apply existsb_exists in Hex as (rm & Hin & Hnp). apply forallb_forall with (x := rm) in G; [| exact Hin].
        rewrite G in Hnp. discriminate.
    - intros own j. unfold lift_reoffer. rewrite H.
      set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                   (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
      destruct (fire_tail_spec W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4)
        as (rs & T5 & _ & _ & _ & _ & _ & _ & _ & _ & Hrs).
      exists rs. split; [| exact Hrs].
      assert (T4 : j_trace j4 = j_trace (touch_all own j (colls W)) ++ lift_ops rv ++ [SFire W sg (Some (reoffer_pos sg, own_auth rv))]).
      { unfold j4. cbn [add_dirty with_dirty with_fired stage j_trace].
        destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (T & _). rewrite T, <- app_assoc. reflexivity. }
      destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W));
        cbn [set_stop j_trace]; rewrite T5, T4, <- !app_assoc; reflexivity.
  Qed.

  (* ── 28. Trace extension and the selections of the cascade ─────────── *)

  (* Every engine function only appends to the trace. *)
  Definition ext (j j' : jst) : Prop := exists ops, j_trace j' = j_trace j ++ ops.

  Lemma ext_refl : forall j, ext j j.
  Proof. intros j. exists []. rewrite app_nil_r. reflexivity. Qed.

  Lemma ext_trans : forall j1 j2 j3, ext j1 j2 -> ext j2 j3 -> ext j1 j3.
  Proof. intros j1 j2 j3 [o1 H1] [o2 H2]. exists (o1 ++ o2). rewrite H2, H1, app_assoc. reflexivity. Qed.

  Lemma ext_stage : forall j op0, ext j (stage j op0).
  Proof. intros j op0. exists [op0]. reflexivity. Qed.

  Lemma ext_touch_all : forall own cs j, ext j (touch_all own j cs).
  Proof. intros own cs j. destruct (touch_all_spec own cs j) as (bs & T & _). exists (map SBump bs). exact T. Qed.

  Lemma ext_store : forall own j x, ext j (store own j x).
  Proof. intros own j x. destruct (store_spec own j x) as (bs & T & _). exists (map SBump bs ++ [SStore (e_rec x) (e_auth x)]). exact T. Qed.

  Lemma ext_fire_tail : forall W sg inc oi fl j, ext j (fire_tail W sg inc oi fl j).
  Proof. intros W sg inc oi fl j. destruct (fire_tail_spec W sg inc oi fl j) as (rs & T & _). exists (map SRetire rs). exact T. Qed.

  Lemma ext_same : forall j j', j_trace j' = j_trace j -> ext j j'.
  Proof. intros j j' H. exists []. rewrite H, app_nil_r. reflexivity. Qed.

  Lemma ext_offer1 : forall own j x, ext j (fst (offer1 own j x)).
  Proof.
    intros own j x. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |]; [| apply ext_store].
    set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                 (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
    assert (E3 : ext j j3).
    { apply (ext_trans _ (touch_all own j (colls W))); [apply ext_touch_all |].
      exists [SFire W sg (Some (p, e_auth x))]. reflexivity. }
    assert (E4 : ext j (fire_tail W sg (Some p) (e_own x) true j3)) by (eapply ext_trans; [exact E3 | apply ext_fire_tail]).
    destruct (w_peek W); [exact E4 |].
    destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
      cbn [fst]; unfold own_flight; try destruct (e_own x);
      (eapply ext_trans; [exact E4 | apply ext_same; reflexivity]).
  Qed.

  Lemma ext_offer : forall fuel own j x, ext j (offer fuel own j x).
  Proof.
    induction fuel as [| f IH]; intros own j x; cbn [offer].
    - unfold own_flight. destruct (e_own x); apply ext_same; reflexivity.
    - pose proof (ext_offer1 own j x) as E. destruct (offer1 own j x) as [j' []]; cbn [fst] in E.
      + exact (ext_trans _ _ _ E (IH own j' x)).
      + exact E.
  Qed.

  Lemma ext_receive1 : forall j W, ext j (fst (receive1 j W)).
  Proof.
    intros j W. unfold receive1. destruct (receive_sel (j_view j) W) as [sg |]; [| unfold park; exists [SPark W]; reflexivity].
    set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
    assert (E4 : ext j (fire_tail W sg None false false j3)).
    { apply (ext_trans _ j3); [exists [SFire W sg None]; reflexivity | apply ext_fire_tail]. }
    destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)));
      cbn [fst]; (eapply ext_trans; [exact E4 | apply ext_same; reflexivity]).
  Qed.

  Lemma ext_receive : forall fuel j W, ext j (receive fuel j W).
  Proof.
    induction fuel as [| f IH]; intros j W; cbn [receive]; [apply ext_same; reflexivity |].
    pose proof (ext_receive1 j W) as E. destruct (receive1 j W) as [j' []]; cbn [fst] in E.
    - exact (ext_trans _ _ _ E (IH j' W)).
    - exact E.
  Qed.

  Lemma ext_stage_all : forall ops j, ext j (stage_all j ops).
  Proof. intros ops j. exists ops. exact (proj1 (stage_all_fields ops j)). Qed.

  Lemma ext_lift_reoffer : forall own j W sg, ext j (lift_reoffer own j W sg).
  Proof.
    intros own j W sg. unfold lift_reoffer. destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |]; [| apply ext_refl].
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                 (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    assert (E : ext j j4).
    { apply (ext_trans _ (touch_all own j (colls W))); [apply ext_touch_all |].
      apply (ext_trans _ (stage_all (touch_all own j (colls W)) (lift_ops rv))); [apply ext_stage_all |].
      exists [SFire W sg (Some (reoffer_pos sg, own_auth rv))]. reflexivity. }
    assert (E5 : ext j (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4))
      by (eapply ext_trans; [exact E | apply ext_fire_tail]).
    destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W));
      (eapply ext_trans; [exact E5 | apply ext_same; reflexivity]).
  Qed.

  Lemma ext_dstep : forall own a j, ext j (fst (dstep own a j)).
  Proof.
    intros own a j. unfold dstep. destruct (j_stop j); [apply ext_same; reflexivity |].
    destruct (j_work j) as [| q rest].
    - destruct cascades; [| apply ext_same; reflexivity].
      destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |]; [| apply ext_same; reflexivity].
      destruct (cap <=? j_fired j); [apply ext_same; reflexivity | apply ext_lift_reoffer].
    - destruct (cap <=? j_fired j); [apply ext_same; reflexivity |]. cbn [fst].
      apply (ext_trans _ (with_work rest j)); [apply ext_same; reflexivity |].
      destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E; [apply ext_offer | apply ext_store].
  Qed.

  Lemma ext_drain : forall fuel own a j, ext j (drain fuel own a j).
  Proof.
    induction fuel as [| f IH]; intros own a j; cbn [drain]; [apply ext_same; reflexivity |].
    pose proof (ext_dstep own a j) as E. destruct (dstep own a j) as [j' []]; cbn [fst] in E.
    - exact (ext_trans _ _ _ E (IH own a j')).
    - exact E.
  Qed.

  (* NEXT-FIRE selects a stored waiter and stored records of its collections. *)
  Lemma next_fire_sel :
    forall d v W sg, next_fire d v = Some (W, sg) ->
      In W (v_wtrs v) /\ fireable W v = Some sg /\ (forall e, In e (map fst sg) -> In e (v_recs v) /\ In (r_coll e) (colls W)).
  Proof.
    intros d v W sg H. unfold next_fire in H.
    assert (G : forall cs, next_fire_in cs v = Some (W, sg) -> In W (v_wtrs v) /\ fireable W v = Some sg).
    { induction cs as [| c r IH]; intros Hn; [discriminate |]. cbn [next_fire_in] in Hn.
      destruct (find_fire (heads_of c v) v) as [[W0 sg0] |] eqn:Ef; [| exact (IH Hn)].
      injection Hn as <- <-.
      assert (G2 : forall ws, incl ws (v_wtrs v) -> find_fire ws v = Some (W0, sg0) -> In W0 (v_wtrs v) /\ fireable W0 v = Some sg0).
      { induction ws as [| W1 r1 IH1]; intros Hi Hf; [discriminate |]. cbn [find_fire] in Hf.
        destruct (fireable W1 v) as [sg1 |] eqn:E1.
        - injection Hf as <- <-. split; [exact (Hi W1 (or_introl eq_refl)) | exact E1].
        - apply IH1; [intros x Hx; exact (Hi x (or_intror Hx)) | exact Hf]. }
      apply (G2 (heads_of c v)); [| exact Ef]. intros x Hx. unfold heads_of in Hx. apply filter_In in Hx as [Hx _].
      apply (Permutation_in _ (worder_perm _ _)) in Hx. unfold waiters_of in Hx. apply filter_In in Hx as [Hx _]. exact Hx. }
    destruct (G _ H) as [Hw Hf]. split; [exact Hw | split; [exact Hf |]].
    intros e He. apply (greedy_colls W v sg); [| exact He].
    unfold fireable in Hf. destruct (heads_ok (colls W) W v); [| discriminate].
    unfold receive_decision in Hf. destruct (greedy W v) as [sg' |] eqn:Eg; [| discriminate].
    destruct (guard_ok W sg'); [injection Hf as <-; reflexivity | discriminate].
  Qed.

  (* ── 29. Headline 12: TOUCH bumps every touched counter once ──────── *)

  Definition bump_events (tr : list staged) : list nat :=
    flat_map (fun op0 => match op0 with SBump c => [c] | _ => [] end) tr.

  Definition op_colls (op0 : staged) : list nat := match op0 with SBump _ => [] | _ => staged_colls op0 end.

  (* Every staging on a collection follows a bump of its counter in the same
     transaction. *)
  Definition bumps_before (tr : list staged) : Prop :=
    forall i op0 c, nth_error tr i = Some op0 -> In c (op_colls op0) ->
      exists i', i' < i /\ nth_error tr i' = Some (SBump c).

  Definition JB (j : jst) : Prop :=
    (forall c, In c (j_bumps j) <-> In c (bump_events (j_trace j))) /\
    NoDup (bump_events (j_trace j)) /\ bumps_before (j_trace j).

  Lemma bump_events_app : forall a b, bump_events (a ++ b) = bump_events a ++ bump_events b.
  Proof. intros a b. unfold bump_events. apply flat_map_app. Qed.

  Lemma bump_events_bumps : forall bs, bump_events (map SBump bs) = bs.
  Proof. induction bs as [| c r IH]; [reflexivity | cbn; f_equal; exact IH]. Qed.

  Lemma in_bump_events : forall tr c, In c (bump_events tr) -> exists i, nth_error tr i = Some (SBump c).
  Proof.
    induction tr as [| op0 r IH]; intros c H; [destruct H |]. cbn [bump_events flat_map] in H.
    apply in_app_or in H as [H | H].
    - destruct op0 as [c0 | r0 a0 | W0 | W0 sg0 inc0 | r0 | r0]; cbn in H; try contradiction.
      destruct H as [<- | []]. exists 0. reflexivity.
    - destruct (IH c H) as [i Hi]. exists (S i). exact Hi.
  Qed.

  Lemma bumps_before_app :
    forall tr ops,
      bumps_before tr ->
      (forall i op0 c, nth_error ops i = Some op0 -> In c (op_colls op0) ->
         In c (bump_events tr) \/ exists i', i' < i /\ nth_error ops i' = Some (SBump c)) ->
      bumps_before (tr ++ ops).
  Proof.
    intros tr ops Hb Hops i op0 c Hi Hc.
    destruct (Nat.lt_ge_cases i (length tr)) as [Hlt | Hge].
    - rewrite nth_error_app1 in Hi by exact Hlt. destruct (Hb i op0 c Hi Hc) as (i' & Hlt' & Hi').
      exists i'. split; [exact Hlt' |]. rewrite nth_error_app1; [exact Hi' | lia].
    - rewrite nth_error_app2 in Hi by exact Hge. destruct (Hops _ _ _ Hi Hc) as [Hin | (i' & Hlt' & Hi')].
      + destruct (in_bump_events _ _ Hin) as [i' Hi']. exists i'.
        assert (i' < length tr) by (apply nth_error_Some; rewrite Hi'; discriminate).
        split; [lia |]. rewrite nth_error_app1; [exact Hi' | lia].
      + exists (length tr + i'). split; [lia |]. rewrite nth_error_app2 by lia.
        replace (length tr + i' - length tr) with i' by lia. exact Hi'.
  Qed.

  Lemma JB_append :
    forall j j' ops,
      JB j -> j_trace j' = j_trace j ++ ops ->
      (forall c, In c (j_bumps j') <-> In c (j_bumps j) \/ In c (bump_events ops)) ->
      NoDup (bump_events ops) -> (forall c, In c (bump_events ops) -> ~ In c (j_bumps j)) ->
      (forall i op0 c, nth_error ops i = Some op0 -> In c (op_colls op0) ->
         In c (j_bumps j) \/ exists i', i' < i /\ nth_error ops i' = Some (SBump c)) ->
      JB j'.
  Proof.
    intros j j' ops (Hb & Hnd & Hbb) T Hbm Hnd' Hfresh Hops. unfold JB. rewrite T. split; [| split].
    - intros c. rewrite Hbm, bump_events_app, (Hb c). split; [intros [H | H]; apply in_or_app; [left | right]; exact H |].
      intros H. apply in_app_or in H. exact H.
    - rewrite bump_events_app. apply NoDup_app; [exact Hnd | exact Hnd' |].
      intros c H1 H2. apply (Hfresh c H2). apply (Hb c). exact H1.
    - apply bumps_before_app; [exact Hbb |]. intros i op0 c Hi Hc.
      destruct (Hops i op0 c Hi Hc) as [H | H]; [left; apply (Hb c); exact H | right; exact H].
  Qed.

  Lemma JB_ctl :
    forall j j', JB j -> j_trace j' = j_trace j -> j_bumps j' = j_bumps j -> JB j'.
  Proof. intros j j' (Hb & Hnd & Hbb) T B. unfold JB. rewrite T. split; [intros c; rewrite B; exact (Hb c) | split; assumption]. Qed.

  Lemma JB_touch_all :
    forall own cs j, JB j -> JB (touch_all own j cs).
  Proof.
    intros own cs j H. destruct (touch_all_spec own cs j) as (bs & T & _ & B & _ & _ & _ & _ & _ & Nd & Hbs & _).
    apply (JB_append j _ (map SBump bs) H T); rewrite ?bump_events_bumps.
    - intros c. rewrite B. split.
      + intros Hc. apply in_app_or in Hc as [Hc | Hc]; [right; exact (proj2 (in_rev bs c) Hc) | left; exact Hc].
      + intros [Hc | Hc]; apply in_or_app; [right; exact Hc | left; exact (proj1 (in_rev bs c) Hc)].
    - exact Nd.
    - intros c Hc. exact (proj1 (proj2 (Hbs c Hc))).
    - intros i op0 c Hi Hc. apply nth_error_In in Hi. apply in_map_iff in Hi as (c' & <- & _). destruct Hc.
  Qed.

  Lemma JB_stage :
    forall j op0, JB j -> (forall c, In c (op_colls op0) -> In c (j_bumps j)) -> (forall c, op0 <> SBump c) ->
      JB (stage j op0).
  Proof.
    intros j op0 H Hc Hnb. apply (JB_append j _ [op0] H); [reflexivity | | | |].
    - intros c. cbn [stage j_bumps]. assert (E : bump_events [op0] = []) by (destruct op0; try reflexivity; exfalso; exact (Hnb c0 eq_refl)).
      rewrite E. split; [intros Hin; left; exact Hin | intros [Hin | []]; exact Hin].
    - assert (E : bump_events [op0] = []) by (destruct op0; try reflexivity; exfalso; exact (Hnb c eq_refl)). rewrite E. constructor.
    - intros c Hin. assert (E : bump_events [op0] = []) by (destruct op0; try reflexivity; exfalso; exact (Hnb c0 eq_refl)).
      rewrite E in Hin. destruct Hin.
    - intros i op1 c Hi Hin. destruct i as [| i]; [| destruct i; discriminate]. injection Hi as <-. left. exact (Hc c Hin).
  Qed.

  Lemma JB_store :
    (forall own c, touch_scope own c = true) -> uses_counter = true ->
    forall own j x, JB j -> JB (store own j x).
  Proof.
    intros Hts Hu own j x H. destruct (store_spec own j x) as (bs & T & _ & B & _ & _ & _ & _ & _ & Nd & Hbs & Hcov).
    apply (JB_append j _ (map SBump bs ++ [SStore (e_rec x) (e_auth x)]) H T).
    - intros c. rewrite B, bump_events_app, bump_events_bumps. cbn [bump_events flat_map app]. rewrite app_nil_r.
      split.
      + intros Hc. apply in_app_or in Hc as [Hc | Hc]; [right; exact (proj2 (in_rev bs c) Hc) | left; exact Hc].
      + intros [Hc | Hc]; apply in_or_app; [right; exact Hc | left; exact (proj1 (in_rev bs c) Hc)].
    - rewrite bump_events_app, bump_events_bumps. cbn. rewrite app_nil_r. exact Nd.
    - intros c Hc. rewrite bump_events_app, bump_events_bumps in Hc. cbn in Hc. rewrite app_nil_r in Hc.
      exact (proj1 (proj2 (Hbs c Hc))).
    - intros i op0 c Hi Hc.
      destruct (Nat.lt_ge_cases i (length bs)) as [Hlt | Hge].
      + rewrite nth_error_app1 in Hi by (rewrite length_map; exact Hlt). rewrite nth_error_map in Hi.
        destruct (nth_error bs i); [injection Hi as <-; destruct Hc | discriminate].
      + rewrite nth_error_app2 in Hi by (rewrite length_map; exact Hge). rewrite length_map in Hi.
        destruct (i - length bs) as [| k] eqn:Ek; [| destruct k; discriminate].
        injection Hi as <-. cbn [op_colls staged_colls In] in Hc. destruct Hc as [<- | []].
        specialize (Hcov Hu (Hts own (r_coll (e_rec x)))). rewrite B in Hcov.
        apply in_app_or in Hcov as [Hr | Hr]; [| left; exact Hr].
        right. apply (proj2 (in_rev _ _)) in Hr. apply In_nth_error in Hr as [i' Hi'].
        assert (i' < length bs) by (apply nth_error_Some; rewrite Hi'; discriminate).
        exists i'. split; [lia |]. rewrite nth_error_app1 by (rewrite length_map; lia). rewrite nth_error_map, Hi'. reflexivity.
  Qed.

  Lemma uses_counter_not_classes : uses_counter = true -> sh <> Classes.
  Proof. unfold uses_counter. intros H E. rewrite E in H. discriminate. Qed.

  Lemma JB_fire_tail :
    uses_counter = true -> forall W sg inc oi fl j, JB j -> JB (fire_tail W sg inc oi fl j).
  Proof.
    intros Hu W sg inc oi fl j H. destruct (fire_tail_spec W sg inc oi fl j) as (rs & T & _ & B & _ & _ & _ & _ & _ & _ & Hrs).
    rewrite (Hrs (or_intror (uses_counter_not_classes Hu))), app_nil_r in T. exact (JB_ctl j _ H T B).
  Qed.

  Lemma JB_bumps_grow : forall j j', JB j -> JB j' -> ext j j' -> incl (j_bumps j) (j_bumps j').
  Proof.
    intros j j' (H1 & _) (H2 & _) [ops T] c Hc. apply H2. rewrite T, bump_events_app. apply in_or_app. left. apply H1. exact Hc.
  Qed.

  Lemma lift_ops_shape : forall r op0, In op0 (lift_ops r) -> op0 = SLift r.
  Proof.
    intros r op0 H. unfold lift_ops in H. destruct (r_pers r); cbn in H; try contradiction.
    destruct H as [<- | []]; reflexivity.
  Qed.

  Lemma JB_stage_all :
    forall ops j, JB j -> (forall op0, In op0 ops -> (forall c, op0 <> SBump c) /\ (forall c, In c (op_colls op0) -> In c (j_bumps j))) ->
      JB (stage_all j ops) /\ j_bumps (stage_all j ops) = j_bumps j.
  Proof.
    induction ops as [| op0 r IH]; intros j H Hops; [split; [exact H | reflexivity] |].
    cbn [stage_all fold_left]. fold (stage_all (stage j op0) r).
    destruct (Hops op0 (or_introl eq_refl)) as [Hnb Hc].
    assert (H1 : JB (stage j op0)) by (apply JB_stage; assumption).
    destruct (IH (stage j op0) H1) as [H2 B2]; [| split; [exact H2 | rewrite B2; reflexivity]].
    intros op1 Hin. exact (Hops op1 (or_intror Hin)).
  Qed.

  Section BumpRule.
    (* The TOUCH rule: every touched collection is bumped. *)
    Hypothesis touch_all_scope : forall own c, touch_scope own c = true.
    Hypothesis counted : uses_counter = true.

    Lemma JB_offer1 : forall own j x, JB j -> JB (fst (offer1 own j x)).
    Proof.
      intros own j x H. unfold offer1.
      destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |];
        [| exact (JB_store touch_all_scope counted own j x H)].
      pose proof (JB_touch_all own (colls W) j H) as H1.
      destruct (touch_all_spec own (colls W) j) as (bs & _ & _ & _ & _ & _ & _ & _ & _ & _ & _ & Hcov).
      assert (H2 : JB (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x))))).
      { apply JB_stage; [exact H1 | | intros c E; discriminate].
        intros c Hc. exact (Hcov c Hc counted (touch_all_scope own c)). }
      set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                   (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
      assert (H3 : JB j3) by (exact (JB_ctl _ j3 H2 eq_refl eq_refl)).
      pose proof (JB_fire_tail counted W sg (Some p) (e_own x) true j3 H3) as H4.
      destruct (w_peek W); [exact H4 |].
      destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
        cbn [fst]; unfold own_flight; try destruct (e_own x); exact (JB_ctl _ _ H4 eq_refl eq_refl).
    Qed.

    Lemma JB_offer : forall fuel own j x, JB j -> JB (offer fuel own j x).
    Proof.
      induction fuel as [| f IH]; intros own j x H; cbn [offer].
      - unfold own_flight. destruct (e_own x); exact (JB_ctl _ _ H eq_refl eq_refl).
      - pose proof (JB_offer1 own j x H) as H1. destruct (offer1 own j x) as [j' []]; cbn [fst] in H1; [exact (IH own j' x H1) | exact H1].
    Qed.

    Lemma JB_receive1 :
      forall j W, JB j -> incl (colls W) (j_bumps j) ->
        JB (fst (receive1 j W)) /\ incl (colls W) (j_bumps (fst (receive1 j W))).
    Proof.
      intros j W H Hc. unfold receive1.
      destruct (receive_sel (j_view j) W) as [sg |].
      - assert (H2 : JB (stage j (SFire W sg None))) by (apply JB_stage; [exact H | exact Hc | intros c E; discriminate]).
        set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
        assert (H3 : JB j3) by (exact (JB_ctl _ j3 H2 eq_refl eq_refl)).
        pose proof (JB_fire_tail counted W sg None false false j3 H3) as H4.
        destruct (fire_tail_spec W sg None false false j3) as (_ & _ & _ & B4 & _).
        destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)));
          cbn [fst]; (split; [exact (JB_ctl _ _ H4 eq_refl eq_refl) | cbn [set_own set_stop j_bumps]; rewrite B4; exact Hc]).
      - cbn [fst]. unfold park. split; [| exact Hc].
        exact (JB_ctl _ _ (JB_stage j (SPark W) H Hc (fun c E => ltac:(discriminate))) eq_refl eq_refl).
    Qed.

    Lemma JB_receive :
      forall fuel j W, JB j -> incl (colls W) (j_bumps j) -> JB (receive fuel j W).
    Proof.
      induction fuel as [| f IH]; intros j W H Hc; cbn [receive]; [exact (JB_ctl _ _ H eq_refl eq_refl) |].
      destruct (JB_receive1 j W H Hc) as [H1 Hc1]. destruct (receive1 j W) as [j' []]; cbn [fst] in H1, Hc1;
        [exact (IH j' W H1 Hc1) | exact H1].
    Qed.

    Lemma JB_lift_reoffer :
      forall own j W sg, JB j -> (forall e, In e (map fst sg) -> In (r_coll e) (colls W)) -> JB (lift_reoffer own j W sg).
    Proof.
      intros own j W sg H Hsg. unfold lift_reoffer.
      destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn; [| exact H].
      pose proof (JB_touch_all own (colls W) j H) as H1.
      destruct (touch_all_spec own (colls W) j) as (bs & _ & _ & _ & _ & _ & _ & _ & _ & _ & _ & Hcov).
      assert (Hcb : forall c, In c (colls W) -> In c (j_bumps (touch_all own j (colls W)))).
      { intros c Hc. exact (Hcov c Hc counted (touch_all_scope own c)). }
      destruct (JB_stage_all (lift_ops rv) (touch_all own j (colls W)) H1) as [H2 B2].
      { intros op0 Hin. rewrite (lift_ops_shape _ _ Hin). split; [intros c E; discriminate |].
        intros c [<- | []]. apply Hcb, Hsg. apply in_map_iff. exists (rv, mv). split; [reflexivity | exact (nth_error_In _ _ Hn)]. }
      assert (H3 : JB (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv))))).
      { apply JB_stage; [exact H2 | | intros c E; discriminate]. intros c Hc. rewrite B2. exact (Hcb c Hc). }
      set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                   (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
      assert (H4 : JB j4) by (exact (JB_ctl _ j4 H3 eq_refl eq_refl)).
      assert (H5 : JB (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4))
        by exact (JB_fire_tail counted W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4 H4).
      destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W));
        try exact H5; exact (JB_ctl _ _ H5 eq_refl eq_refl).
    Qed.

    Lemma JB_dstep : forall own a j, JB j -> JB (fst (dstep own a j)).
    Proof.
      intros own a j H. unfold dstep. destruct (j_stop j); [exact H |].
      destruct (j_work j) as [| q rest].
      - destruct cascades; [| exact H].
        destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |] eqn:En; [| exact H].
        destruct (cap <=? j_fired j); [exact (JB_ctl _ _ H eq_refl eq_refl) |]. cbn [fst].
        apply JB_lift_reoffer; [exact H |]. intros e He. exact (proj2 (proj2 (proj2 (next_fire_sel _ _ _ _ En)) e He)).
      - destruct (cap <=? j_fired j); [exact (JB_ctl _ _ H eq_refl eq_refl) |]. cbn [fst].
        assert (H0 : JB (with_work rest j)) by (exact (JB_ctl _ _ H eq_refl eq_refl)).
        destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E;
          [apply JB_offer; exact H0 | exact (JB_store touch_all_scope counted own _ _ H0)].
    Qed.

    Lemma JB_drain : forall fuel own a j, JB j -> JB (drain fuel own a j).
    Proof.
      induction fuel as [| f IH]; intros own a j H; cbn [drain]; [exact (JB_ctl _ _ H eq_refl eq_refl) |].
      pose proof (JB_dstep own a j H) as H1. destruct (dstep own a j) as [j' []]; cbn [fst] in H1; [exact (IH own a j' H1) | exact H1].
    Qed.

    Lemma JB_run_join : forall fuel o scope v0, JB (fst (run_join fuel o scope v0)).
    Proof.
      intros fuel o scope v0. unfold run_join.
      assert (H0 : JB (init v0)) by (split; [intros c; split; intros [] | split; [constructor | intros i op0 c Hi; destruct i; discriminate]]).
      pose proof (JB_touch_all (own_of o) (own_of o) (init v0) H0) as H1.
      destruct (touch_all_spec (own_of o) (own_of o) (init v0)) as (bs & _ & _ & _ & _ & _ & _ & _ & _ & _ & _ & Hcov).
      set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
      assert (Hj1 : JB j1) by (exact (JB_ctl _ j1 H1 eq_refl eq_refl)).
      assert (Hown1 : incl (own_of o) (j_bumps j1)).
      { intros c Hc. exact (Hcov c Hc counted (touch_all_scope _ c)). }
      assert (Hd1 : JB (drain fuel (own_of o) (auth_of o) (with_dirty scope j1)))
        by (apply JB_drain; exact (JB_ctl _ _ Hj1 eq_refl eq_refl)).
      destruct (is_settle o); [cbn [fst]; exact Hd1 |].
      set (j3 := if cascades then drain fuel (own_of o) (auth_of o) (with_dirty scope j1) else j1).
      assert (Hj3 : JB j3) by (unfold j3; destruct cascades; [exact Hd1 | exact Hj1]).
      assert (Hown3 : incl (own_of o) (j_bumps j3)).
      { unfold j3. destruct cascades; [| exact Hown1].
        apply (incl_tran Hown1). apply (JB_bumps_grow j1); [exact Hj1 | exact Hd1 |].
        apply (ext_trans _ (with_dirty scope j1)); [apply ext_same; reflexivity | apply ext_drain]. }
      destruct (j_stop j3); [cbn [fst]; exact Hj3 |].
      destruct (plan_at_cap (j_fired (with_dirty [] j3))); [cbn [fst]; exact (JB_ctl _ _ Hj3 eq_refl eq_refl) |].
      cbn [fst]. apply JB_drain.
      set (j4 := with_fired (phase_count (j_fired (with_dirty [] j3))) (with_dirty [] j3)).
      assert (Hj4 : JB j4) by exact (JB_ctl _ _ Hj3 eq_refl eq_refl).
      assert (Hown4 : incl (own_of o) (j_bumps j4)) by exact Hown3.
      assert (Hp : JB (plan fuel o v0 j4)).
      { destruct o as [x c a | W c a | key sd pd rs a]; cbn [plan].
        - apply JB_offer. exact Hj4.
        - apply JB_receive; [exact Hj4 |]. exact Hown4.
        - exact Hj4. }
      exact (JB_ctl _ _ Hp eq_refl eq_refl).
    Qed.
  End BumpRule.

  (* Headline 12 (decision 31, plan 4.3). For a discipline with a counter
     (queue, stack, cell, priority queue), every staging of a transaction on
     a collection follows a bump of the counter of that collection in the
     same transaction, and each counter is bumped at most once: TOUCH bumps
     every collection that the transaction stages on, its own collections
     and the collections reached through the cascade alike. *)
  Theorem cascade_bumps_touched_counters :
    uses_counter = true ->
    forall fuel o scope v0,
      bumps_before (j_trace (fst (run_join fuel o scope v0))) /\
      NoDup (bump_events (j_trace (fst (run_join fuel o scope v0)))).
  Proof.
    intros Hu fuel o scope v0.
    assert (Hts : forall own c, touch_scope own c = true) by (intros own c; unfold touch_scope; reflexivity).
    destruct (JB_run_join Hts Hu fuel o scope v0) as (_ & Hnd & Hbb). split; assumption.
  Qed.

  (* ── 30. Headline 15: a re-issued join keeps its bind ords ─────────── *)

  Lemma ext_run_join :
    forall fuel o scope v0,
      ext (with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))) (fst (run_join fuel o scope v0)).
  Proof.
    intros fuel o scope v0. unfold run_join.
    set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
    assert (Ed : ext j1 (drain fuel (own_of o) (auth_of o) (with_dirty scope j1))).
    { apply (ext_trans _ (with_dirty scope j1)); [apply ext_same; reflexivity | apply ext_drain]. }
    destruct (is_settle o); [cbn [fst]; exact Ed |].
    set (j3 := if cascades then drain fuel (own_of o) (auth_of o) (with_dirty scope j1) else j1).
    assert (E3 : ext j1 j3) by (unfold j3; destruct cascades; [exact Ed | apply ext_refl]).
    destruct (j_stop j3); [cbn [fst]; exact E3 |].
    destruct (plan_at_cap (j_fired (with_dirty [] j3))); [cbn [fst]; eapply ext_trans; [exact E3 | apply ext_same; reflexivity] |].
    cbn [fst]. set (j4 := with_fired (phase_count (j_fired (with_dirty [] j3))) (with_dirty [] j3)).
    assert (E4 : ext j1 j4) by (eapply ext_trans; [exact E3 | apply ext_same; reflexivity]).
    assert (E5 : ext j1 (plan fuel o v0 j4)).
    { eapply ext_trans; [exact E4 |]. destruct o; cbn [plan]; [apply ext_offer | apply ext_receive | apply ext_refl]. }
    eapply ext_trans; [exact E5 |].
    apply (ext_trans _ (with_fired (phase_count (j_fired (plan fuel o v0 j4))) (plan fuel o v0 j4)));
      [apply ext_same; reflexivity | apply ext_drain].
  Qed.

  (* COMMIT re-issues the resume, or a settle job that carries the resume. *)
  Lemma commit_cases :
    forall o v0 j, commit o v0 j = resume_of o v0 j \/
      exists k sd pd, commit o v0 j = Some (OSettle k sd pd (resume_of o v0 j) (auth_of o)).
  Proof.
    intros o v0 j. unfold commit. destruct (j_stop j); [| left; reflexivity].
    match goal with |- context [if ?b then _ else _] => destruct b end; [right; eexists _, _, _; reflexivity | left; reflexivity].
  Qed.

  (* Every end of RUN-JOIN re-issues what COMMIT gives for its last state. *)
  Lemma run_join_commit : forall fuel o scope v0, exists j, snd (run_join fuel o scope v0) = commit o v0 j.
  Proof.
    intros fuel o scope v0. unfold run_join. cbv zeta.
    destruct (is_settle o); [eexists; reflexivity |].
    match goal with |- context [match j_stop ?j with _ => _ end] => destruct (j_stop j) end; [eexists; reflexivity |].
    match goal with |- context [if plan_at_cap ?f then _ else _] => destruct (plan_at_cap f) end; eexists; reflexivity.
  Qed.

  Lemma touch_all_fresh :
    forall own cs j,
      uses_counter = true -> (forall c, touch_scope own c = true) -> NoDup cs -> (forall c, In c cs -> ~ In c (j_bumps j)) ->
      j_trace (touch_all own j cs) = j_trace j ++ map SBump cs.
  Proof.
    intros own cs. induction cs as [| c r IH]; intros j Hu Hts Hnd Hf; [cbn; rewrite app_nil_r; reflexivity |].
    apply NoDup_cons_iff in Hnd as [Hn Hnd]. cbn [touch_all fold_left]. fold (touch_all own (touch own j c) r).
    unfold touch. rewrite Hu, Hts. cbn [andb].
    replace (mem c (j_bumps j)) with false by (symmetry; apply mem_false; exact (Hf c (or_introl eq_refl))).
    cbn [negb]. rewrite IH; [cbn [stage with_bumps j_trace]; rewrite <- app_assoc; reflexivity | exact Hu | exact Hts | exact Hnd |].
    intros c' Hc' Hb. cbn [stage with_bumps j_bumps] in Hb. destruct Hb as [<- | Hb]; [exact (Hn Hc') |].
    exact (Hf c' (or_intror Hc') Hb).
  Qed.

  (* Headline 15 (decision 43, plan 4.11). A re-issued persistent JoinOp
     carries the ords of its first round, one for each bind (equal for
     repeated binds), and a re-issued round carries them again, so every
     round of the operation keeps the ords and the v2 slots of the first
     round. Every re-issue of a round of a persistent JoinOp, directly or as
     the resume of a settle job, carries those ords. Each round bumps the
     counter of every own collection, first. *)
  Theorem join_reissue_keeps_bind_ords :
    (forall W c a v0, w_pers W = true ->
       reissue_op (OReceive W c a) v0 = Some (OReceive W (Some (ords_of v0 W c)) a)) /\
    (forall v0 W os, ords_of v0 W (Some os) = os) /\
    (forall fuel W c a scope v0, w_pers W = true ->
       let os := ords_of v0 W c in
       let r := snd (run_join fuel (OReceive W c a) scope v0) in
       r = None \/ r = Some (OReceive W (Some os) a) \/
       exists k sd pd rs, r = Some (OSettle k sd pd rs a) /\ (rs = None \/ rs = Some (OReceive W (Some os) a))) /\
    (uses_counter = true -> forall fuel o scope v0, NoDup (own_of o) ->
       exists rest, j_trace (fst (run_join fuel o scope v0)) = map SBump (own_of o) ++ rest).
  Proof.
    assert (Hre : forall W c a v0, w_pers W = true ->
               reissue_op (OReceive W c a) v0 = Some (OReceive W (Some (ords_of v0 W c)) a)).
    { intros W c a v0 Hp. unfold reissue_op, carried_ords. rewrite Hp. reflexivity. }
    split; [exact Hre |]. split; [reflexivity |]. split.
    - intros fuel W c a scope v0 Hp os r.
      assert (Hc : forall j, let r0 := commit (OReceive W c a) v0 j in
                     r0 = None \/ r0 = Some (OReceive W (Some os) a) \/
                     exists k sd pd rs, r0 = Some (OSettle k sd pd rs a) /\ (rs = None \/ rs = Some (OReceive W (Some os) a))).
      { intros j r0.
        assert (Hres : resume_of (OReceive W c a) v0 j = None \/ resume_of (OReceive W c a) v0 j = Some (OReceive W (Some os) a)).
        { unfold resume_of. destruct (existsb p_own (j_work j)); [right; exact (Hre W c a v0 Hp) |].
          destruct (j_own j); try (left; reflexivity); right; exact (Hre W c a v0 Hp). }
        unfold r0. destruct (commit_cases (OReceive W c a) v0 j) as [E | (k & sd & pd & E)]; rewrite E.
        - destruct Hres as [-> | ->]; [left | right; left]; reflexivity.
        - right. right. exists k, sd, pd, (resume_of (OReceive W c a) v0 j). split; [reflexivity | exact Hres]. }
      destruct (run_join_commit fuel (OReceive W c a) scope v0) as [j E]. unfold r. rewrite E. apply Hc.
    - intros Hu fuel o scope v0 Hnd.
      destruct (ext_run_join fuel o scope v0) as [rest T]. exists rest. rewrite T. cbn [with_work j_trace].
      rewrite (touch_all_fresh (own_of o) (own_of o) (init v0) Hu); [reflexivity | | exact Hnd | intros c _ []].
      intros c. unfold touch_scope. reflexivity.
  Qed.

  (* ── 31. Headline 7: the cascade terminates ─────────────────────────── *)

  (* CONSUME_ARROWS: no receive both peeks and persists. *)
  Definition wf_waiter (W : wtr) : bool := negb (w_pers W && w_peek W).

  Definition np_wtrs (v : view) : nat := length (filter (fun W => negb (w_pers W)) (v_wtrs v)).
  Definition np_recs (v : view) : nat := length (filter (fun r => negb (r_pers r)) (v_recs v)).

  (* μ = (non-persistent waiters, non-persistent records + pending second
     sends, pending second sends), in lexicographic order. *)
  Definition mu (j : jst) : nat * nat * nat :=
    (np_wtrs (j_view j), np_recs (j_view j) + length (j_work j), length (j_work j)).

  Definition lex_lt (a b : nat * nat * nat) : Prop :=
    match a, b with
    | (a1, a2, a3), (b1, b2, b3) => a1 < b1 \/ (a1 = b1 /\ (a2 < b2 \/ (a2 = b2 /\ a3 < b3)))
    end.

  Lemma np_del_rec_le : forall i l, length (filter (fun r => negb (r_pers r)) (del_rec i l)) <= length (filter (fun r => negb (r_pers r)) l).
  Proof.
    intros i l. induction l as [| r rest IH]; [reflexivity |]. cbn [del_rec].
    destruct (r_id r =? i); cbn [filter]; destruct (negb (r_pers r)); cbn [length]; lia.
  Qed.

  Lemma np_del_rec_in :
    forall l r, NoDup (map r_id l) -> In r l -> r_pers r = false ->
      S (length (filter (fun e => negb (r_pers e)) (del_rec (r_id r) l))) = length (filter (fun e => negb (r_pers e)) l).
  Proof.
    induction l as [| e rest IH]; intros r Hnd Hin Hp; [destruct Hin |].
    cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hn Hnd]. cbn [del_rec].
    destruct Hin as [<- | Hin].
    - rewrite Nat.eqb_refl. cbn [filter]. rewrite Hp. reflexivity.
    - destruct (Nat.eqb_spec (r_id e) (r_id r)) as [E | E].
      + exfalso. apply Hn. rewrite E. apply in_map. exact Hin.
      + cbn [filter]. destruct (negb (r_pers e)); cbn [length]; rewrite <- (IH r Hnd Hin Hp); reflexivity.
  Qed.

  Lemma np_remove_all_le :
    forall rs l, length (filter (fun r => negb (r_pers r)) (remove_all rs l)) <= length (filter (fun r => negb (r_pers r)) l).
  Proof.
    induction rs as [| r rest IH]; intros l; [reflexivity |].
    rewrite remove_all_cons. etransitivity; [apply IH | apply np_del_rec_le].
  Qed.

  Lemma npw_del_wtr_in :
    forall l W, NoDup (map w_id l) -> In W l -> w_pers W = false ->
      S (length (filter (fun W0 => negb (w_pers W0)) (del_wtr (w_id W) l))) = length (filter (fun W0 => negb (w_pers W0)) l).
  Proof.
    induction l as [| W0 rest IH]; intros W Hnd Hin Hp; [destruct Hin |].
    cbn [map] in Hnd. apply NoDup_cons_iff in Hnd as [Hn Hnd]. cbn [del_wtr].
    destruct Hin as [<- | Hin].
    - rewrite Nat.eqb_refl. cbn [filter]. rewrite Hp. reflexivity.
    - destruct (Nat.eqb_spec (w_id W0) (w_id W)) as [E | E].
      + exfalso. apply Hn. rewrite E. apply in_map. exact Hin.
      + cbn [filter]. destruct (negb (w_pers W0)); cbn [length]; rewrite <- (IH W Hnd Hin Hp); reflexivity.
  Qed.

  (* The view after the stagings of a fire step: the bumps, then the fire,
     then the retirements. *)
  Lemma fire_step_view :
    forall j bs W sg inc rs,
      let v := apply (apply_all (j_view j) (map SBump bs)) (SFire W sg inc) in
      np_wtrs (mkView (remove_all rs (v_recs v)) (v_wtrs v) (v_cnt v)) =
        (if w_pers W then np_wtrs (j_view j)
         else length (filter (fun W0 => negb (w_pers W0)) (del_wtr (w_id W) (v_wtrs (j_view j))))) /\
      np_recs (mkView (remove_all rs (v_recs v)) (v_wtrs v) (v_cnt v)) <= np_recs (j_view j).
  Proof.
    intros j bs W sg inc rs v. unfold v. destruct (bumps_view bs (j_view j)) as [Br Bw].
    unfold np_wtrs, np_recs. cbn [apply v_recs v_wtrs v_cnt]. rewrite Br, Bw. split.
    - destruct (w_pers W); reflexivity.
    - etransitivity; [apply np_remove_all_le | apply np_remove_all_le].
  Qed.

  Lemma length_fold_push : forall (L w : list pend), length (fold_left (fun w0 q => q :: w0) L w) = length L + length w.
  Proof. intros L w. rewrite fold_push, length_app, length_rev. reflexivity. Qed.

  Lemma lex_lt_trans : forall a b c, lex_lt a b -> lex_lt b c -> lex_lt a c.
  Proof. intros [[a1 a2] a3] [[b1 b2] b3] [[c1 c2] c3]. cbn [lex_lt]. lia. Qed.

  (* μ with the element in flight counted as one more pending second send:
     the measure of a drain step at the pop of its second send. *)
  Definition mux (j : jst) : nat * nat * nat :=
    (np_wtrs (j_view j), np_recs (j_view j) + S (length (j_work j)), S (length (j_work j))).

  (* One round of OFFER of an element x in flight. A round that goes on
     (b = true: x is persistent and a non-persistent waiter left) keeps the
     stop and the work and removes a non-persistent waiter. A round that
     ends stops or lowers μ below mux: x is stored, x is spent, a peek
     spends its waiter, or a divergent pair leaves x in flight. *)
  Lemma offer1_mu :
    forall own j x j' b,
      (forall W, In W (v_wtrs (j_view j)) -> w_pers W = true -> w_peek W = false) ->
      NoDup (map w_id (v_wtrs (j_view j))) ->
      offer1 own j x = (j', b) ->
      if b then j_stop j' = j_stop j /\ j_work j' = j_work j /\ np_wtrs (j_view j') < np_wtrs (j_view j) /\
                incl (v_wtrs (j_view j')) (v_wtrs (j_view j)) /\ NoDup (map w_id (v_wtrs (j_view j')))
      else j_stop j' <> None \/ lex_lt (mu j') (mux j).
  Proof.
    intros own j x j' b Hwf Hndw H. unfold offer1 in H.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep.
    - destruct (proj1 join_fires_heads_only _ _ _ _ _ Ep) as (_ & HwIn & _).
      destruct (touch_all_spec own (colls W) j) as (bs & T1 & V1 & B1 & D1 & W1 & O1 & S1 & F1 & _).
      set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                   (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))) in H.
      destruct (fire_tail_spec W sg (Some p) (e_own x) true j3) as (rs & _ & V4 & _ & _ & P4 & _ & S4 & _ & _).
      set (j4 := fire_tail W sg (Some p) (e_own x) true j3) in *.
      assert (Ev3 : j_view j3 = apply (apply_all (j_view j) (map SBump bs)) (SFire W sg (Some (p, e_auth x))))
        by (unfold j3; cbn [add_dirty with_dirty with_fired stage j_view]; rewrite V1; reflexivity).
      assert (Ew3 : j_work j3 = j_work j) by (unfold j3; cbn [add_dirty with_dirty with_fired stage j_work]; exact W1).
      assert (Es3 : j_stop j3 = j_stop j) by (unfold j3; cbn [add_dirty with_dirty with_fired stage j_stop]; exact S1).
      rewrite Ev3 in V4. rewrite Ew3 in P4. rewrite Es3 in S4.
      destruct (fire_step_view j bs W sg (Some (p, e_auth x)) rs) as [Hw Hr].
      assert (Hw4 : np_wtrs (j_view j4) = (if w_pers W then np_wtrs (j_view j)
                                            else length (filter (fun W0 => negb (w_pers W0)) (del_wtr (w_id W) (v_wtrs (j_view j))))))
        by (rewrite V4; exact Hw).
      assert (Hr4 : np_recs (j_view j4) <= np_recs (j_view j)) by (rewrite V4; exact Hr).
      assert (Hv4 : v_wtrs (j_view j4) = (if w_pers W then v_wtrs (j_view j) else del_wtr (w_id W) (v_wtrs (j_view j)))).
      { rewrite V4. cbn [v_wtrs apply]. rewrite (proj2 (bumps_view bs (j_view j))). reflexivity. }
      destruct (w_peek W) eqn:Epk.
      + injection H as <- <-. right. unfold mu, mux. cbn [lex_lt]. left. rewrite Hw4.
        destruct (w_pers W) eqn:Eps; [rewrite (Hwf W HwIn Eps) in Epk; discriminate |].
        unfold np_wtrs. rewrite <- (npw_del_wtr_in _ W Hndw HwIn Eps). lia.
      + assert (Hk4 : length (j_work j4) = length (j_work j)) by (rewrite (Permutation_length P4); reflexivity).
        destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W)) eqn:Ea.
        * assert (Hj : j' = (if e_own x then set_own OSpent j4 else j4) /\ b = false)
            by (destruct (e_own x); injection H as <- <-; split; reflexivity).
          destruct Hj as [-> ->]. right.
          assert (Hm : mu (if e_own x then set_own OSpent j4 else j4) = mu j4) by (destruct (e_own x); reflexivity).
          rewrite Hm. unfold mu, mux. rewrite Hk4. cbn [lex_lt]. rewrite Hw4.
          destruct (w_pers W) eqn:Eps.
          -- right. split; [reflexivity |]. left. lia.
          -- left. unfold np_wtrs. rewrite <- (npw_del_wtr_in _ W Hndw HwIn Eps). lia.
        * injection H as <- <-.
          unfold after_fire in Ea. destruct (negb (r_pers (e_rec x)) && negb false); [discriminate |].
          destruct (w_pers W) eqn:Eps; [discriminate |].
          hnf. refine (conj _ (conj _ (conj _ (conj _ _)))).
          -- exact S4.
          -- unfold j4, fire_tail. rewrite Epk.
             destruct (retire_set_binds_spec j3 sg (Some p)) as (_ & _ & _ & _ & _ & Wk & _). rewrite Wk. exact Ew3.
          -- rewrite Hw4. unfold np_wtrs. rewrite <- (npw_del_wtr_in _ W Hndw HwIn Eps). lia.
          -- rewrite Hv4. apply del_wtr_incl.
          -- rewrite Hv4. apply nodup_wtr_ids_del. exact Hndw.
        * assert (Hj : j' = own_flight x j4 /\ b = false) by (injection H as <- <-; split; reflexivity).
          destruct Hj as [-> ->]. right.
          assert (Hm : mu (own_flight x j4) = mu j4) by (unfold own_flight; destruct (e_own x); reflexivity).
          rewrite Hm. unfold mu, mux. rewrite Hk4. cbn [lex_lt]. rewrite Hw4.
          unfold after_fire in Ea. destruct (negb (r_pers (e_rec x)) && negb false); [discriminate |].
          destruct (w_pers W) eqn:Eps; [| destruct (cap <=? S (j_fired j)); discriminate].
          right. split; [reflexivity |]. left. lia.
        * injection H as <- <-. left. cbn [set_stop j_stop]. discriminate.
    - injection H as <- <-. right.
      destruct (store_spec own j x) as (bs & _ & Vs & _ & _ & Wk & _).
      unfold mu, mux. rewrite Vs, Wk, apply_all_app.
      destruct (bumps_view bs (j_view j)) as [Br Bw]. unfold np_recs, np_wtrs.
      cbn [apply_all fold_left apply v_recs v_wtrs filter]. rewrite Br, Bw. cbn [lex_lt].
      destruct (negb (r_pers (e_rec x))); cbn [length]; lia.
  Qed.

  (* OFFER of an element x in flight stops or lowers μ below mux. *)
  Lemma offer_mu :
    forall fuel own j x,
      (forall W, In W (v_wtrs (j_view j)) -> w_pers W = true -> w_peek W = false) ->
      NoDup (map w_id (v_wtrs (j_view j))) ->
      j_stop (offer fuel own j x) <> None \/ lex_lt (mu (offer fuel own j x)) (mux j).
  Proof.
    induction fuel as [| f IH]; intros own j x Hwf Hndw; cbn [offer].
    - left. unfold own_flight. destruct (e_own x); cbn [set_stop set_own j_stop]; discriminate.
    - destruct (offer1 own j x) as [j' b] eqn:E. pose proof (offer1_mu own j x j' b Hwf Hndw E) as H.
      destruct b; [| exact H].
      destruct H as (Hs & Hw & Hlt & Hi & Hnd').
      destruct (IH own j' x (fun W HW => Hwf W (Hi W HW)) Hnd') as [H1 | H1]; [left; exact H1 | right].
      apply (lex_lt_trans _ (mux j') _ H1). unfold mux. rewrite Hw. cbn [lex_lt]. left. exact Hlt.
  Qed.

  Lemma lift_reoffer_mu :
    forall own j W sg,
      In W (v_wtrs (j_view j)) -> (w_pers W = true -> w_peek W = false) ->
      NoDup (map r_id (v_recs (j_view j))) -> NoDup (map w_id (v_wtrs (j_view j))) ->
      (forall e, In e (map fst sg) -> In e (v_recs (j_view j))) -> sg <> [] ->
      j_stop (lift_reoffer own j W sg) <> None \/ lex_lt (mu (lift_reoffer own j W sg)) (mu j).
  Proof.
    intros own j W sg HwIn Hwf Hndr Hndw Hsg Hne. unfold lift_reoffer.
    destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn.
    2: { exfalso. apply nth_error_None in Hn. unfold reoffer_pos in Hn.
         assert (G : forall sg0 k i, first_np k sg0 = Some i -> i - k < length sg0).
         { induction sg0 as [| rm r IH]; intros k i H; [discriminate |]. cbn [first_np] in H.
           destruct (r_pers (fst rm)); [specialize (IH _ _ H); cbn [length]; lia | injection H as <-; cbn [length]; lia]. }
         destruct (first_np 0 sg) as [i |] eqn:Ef; [specialize (G sg 0 i Ef); lia |].
         destruct sg; [contradiction | cbn [length] in Hn; lia]. }
    assert (Hrv : In rv (v_recs (j_view j))) by (apply Hsg; apply in_map_iff; exists (rv, mv); split; [reflexivity | exact (nth_error_In _ _ Hn)]).
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & V1 & B1 & D1 & W1 & O1 & S1 & F1 & _).
    destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (T2 & V2 & B2 & D2 & W2 & O2 & S2 & F2).
    set (j2 := stage_all (touch_all own j (colls W)) (lift_ops rv)) in *.
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j)) (stage j2 (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    assert (Ev4 : j_view j4 = apply (apply_all (apply_all (j_view j) (map SBump bs)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))).
    { unfold j4. cbn [add_dirty with_dirty with_fired stage j_view]. rewrite V2, V1. reflexivity. }
    assert (Ew4 : j_work j4 = j_work j) by (unfold j4; cbn [add_dirty with_dirty with_fired stage j_work]; rewrite W2; exact W1).
    destruct (bumps_view bs (j_view j)) as [Br Bw].
    (* the counts of the view after the lift and the fire *)
    assert (Hcount : np_wtrs (j_view j4) = (if w_pers W then np_wtrs (j_view j)
                                              else length (filter (fun W0 => negb (w_pers W0)) (del_wtr (w_id W) (v_wtrs (j_view j))))) /\
                     np_recs (j_view j4) + (if r_pers rv then 0 else 1) <= np_recs (j_view j)).
    { rewrite Ev4. unfold np_wtrs, np_recs. unfold lift_ops. destruct (r_pers rv) eqn:Epv.
      - cbn [apply_all fold_left apply v_recs v_wtrs]. rewrite Br, Bw. split; [destruct (w_pers W); reflexivity |].
        rewrite Nat.add_0_r. apply np_remove_all_le.
      - cbn [apply_all fold_left apply v_recs v_wtrs]. rewrite Br, Bw. split; [destruct (w_pers W); reflexivity |].
        rewrite <- (np_del_rec_in _ rv Hndr Hrv Epv). pose proof (np_remove_all_le (removed sg (Some (reoffer_pos sg, own_auth rv))) (del_rec (r_id rv) (v_recs (j_view j)))). lia. }
    destruct Hcount as [Hw Hr].
    (* the tail: a peek pushes second sends, another fire retires set members *)
    destruct (fire_tail_spec W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4) as (rs & _ & V5 & _ & _ & P5 & _ & _ & _ & _ & _).
    set (j5 := fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4) in *.
    assert (Hw5 : np_wtrs (j_view j5) = np_wtrs (j_view j4)) by (unfold np_wtrs; rewrite V5; reflexivity).
    assert (Hr5 : np_recs (j_view j5) <= np_recs (j_view j4)) by (unfold np_recs; rewrite V5; apply np_remove_all_le).
    assert (Hk5 : w_peek W = false -> length (j_work j5) = length (j_work j)).
    { intros Ep. rewrite (Permutation_length P5), Ep. cbn [app]. rewrite Ew4. reflexivity. }
    destruct (w_peek W) eqn:Epk.
    - (* a peek spends its waiter *)
      assert (Eps : w_pers W = false).
      { apply Bool.not_true_iff_false. intros E. pose proof (Hwf E) as Hc. rewrite ?Epk in Hc. discriminate. }
      rewrite Eps in Hw. pose proof (npw_del_wtr_in _ W Hndw HwIn Eps) as Hd.
      destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb true) (w_pers W));
        try (left; discriminate); right; unfold mu; cbn [set_stop j_view]; cbn [lex_lt]; left; unfold np_wtrs in *; lia.
    - destruct (w_pers W) eqn:Eps.
      + destruct (r_pers rv) eqn:Epv.
        * (* every record persistent: a divergent pair stops *)
          cbn [after_fire negb andb]. left. discriminate.
        * cbn [after_fire negb andb]. right. unfold mu. rewrite (Hk5 eq_refl). cbn [lex_lt]. right.
          split; [rewrite Hw5; exact Hw |]. left. lia.
      + pose proof (npw_del_wtr_in _ W Hndw HwIn Eps) as Hd.
        destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb false) false);
          try (left; discriminate); right; unfold mu; cbn [set_stop j_view]; cbn [lex_lt]; left; unfold np_wtrs in *; lia.
  Qed.

  Lemma offer1_fires :
    forall own j x j' b, offer1 own j x = (j', b) ->
      fires (j_trace j') + j_fired j = fires (j_trace j) + j_fired j' /\ j_fired j <= j_fired j' <= S (j_fired j) /\
      (b = true -> j_fired j' < cap).
  Proof.
    intros own j x j' b H. unfold offer1 in H.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |].
    - destruct (touch_all_spec own (colls W) j) as (bs & T1 & _ & _ & _ & _ & _ & _ & F1 & _).
      set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                   (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))) in H.
      destruct (fire_tail_spec W sg (Some p) (e_own x) true j3) as (rs & T4 & _ & _ & _ & _ & _ & _ & F4 & _).
      assert (Ht3 : j_trace j3 = j_trace j ++ map SBump bs ++ [SFire W sg (Some (p, e_auth x))])
        by (unfold j3; cbn [add_dirty with_dirty with_fired stage j_trace]; rewrite T1, app_assoc; reflexivity).
      assert (Hf3 : j_fired j3 = S (j_fired j)) by reflexivity.
      assert (Hres : fires (j_trace (fire_tail W sg (Some p) (e_own x) true j3)) + j_fired j =
                     fires (j_trace j) + j_fired (fire_tail W sg (Some p) (e_own x) true j3) /\
                     j_fired (fire_tail W sg (Some p) (e_own x) true j3) = S (j_fired j)).
      { rewrite T4, Ht3, F4, Hf3, !fires_app, fires_bumps, fires_retires. cbn. lia. }
      destruct Hres as [Hr1 Hr2].
      destruct (w_peek W); [injection H as <- <-; split; [exact Hr1 | split; [lia | discriminate]] |].
      destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W)) eqn:Ea;
        cbn [fst] in H; unfold own_flight in H; try destruct (e_own x); injection H as <- <-; cbn [set_own set_stop j_trace j_fired];
        (split; [exact Hr1 | split; [lia | intros Hb]]); try discriminate.
      all: unfold after_fire in Ea; destruct (negb (r_pers (e_rec x)) && negb false); [discriminate |];
           destruct (w_pers W); [discriminate |]; destruct (cap <=? S (j_fired j)) eqn:Ec; [discriminate |];
           apply Nat.leb_gt in Ec; lia.
    - injection H as <- <-. destruct (store_spec own j x) as (bs & T & _ & _ & _ & _ & _ & _ & F & _).
      rewrite T, F, !fires_app, fires_bumps.
      replace (fires [SStore (e_rec x) (e_auth x)]) with 0 by reflexivity. split; [lia | split; [lia | discriminate]].
  Qed.

  Lemma offer_fires :
    forall fuel own j x, j_fired j < cap ->
      fires (j_trace (offer fuel own j x)) + j_fired j = fires (j_trace j) + j_fired (offer fuel own j x) /\
      j_fired j <= j_fired (offer fuel own j x) <= cap.
  Proof.
    induction fuel as [| f IH]; intros own j x Hc; cbn [offer].
    - unfold own_flight. destruct (e_own x); cbn [set_own set_stop j_trace j_fired]; split; lia.
    - destruct (offer1 own j x) as [j' b] eqn:E. destruct (offer1_fires own j x j' b E) as (H1 & H2 & H3).
      destruct b.
      + destruct (IH own j' x (H3 eq_refl)) as [H4 H5]. split; lia.
      + split; lia.
  Qed.

  Lemma lift_reoffer_fires :
    forall own j W sg,
      fires (j_trace (lift_reoffer own j W sg)) + j_fired j = fires (j_trace j) + j_fired (lift_reoffer own j W sg) /\
      j_fired j <= j_fired (lift_reoffer own j W sg) <= S (j_fired j).
  Proof.
    intros own j W sg. unfold lift_reoffer. destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |]; [| split; lia].
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & _ & _ & _ & _ & _ & _ & F1 & _).
    destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (T2 & _ & _ & _ & _ & _ & _ & F2).
    assert (Hl : fires (lift_ops rv) = 0) by (unfold lift_ops; destruct (r_pers rv); reflexivity).
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                 (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    assert (H4 : fires (j_trace j4) + j_fired j = fires (j_trace j) + j_fired j4 /\ j_fired j4 = S (j_fired j)).
    { unfold j4. cbn [add_dirty with_dirty with_fired stage j_trace j_fired]. rewrite T2, T1, !fires_app, fires_bumps, Hl. cbn. lia. }
    destruct H4 as [H4 H5].
    destruct (fire_tail_spec W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4) as (rs & T5 & _ & _ & _ & _ & _ & _ & F5 & _).
    assert (H6 : forall j6, j_trace j6 = j_trace (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4) ->
                   j_fired j6 = j_fired (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4) ->
                   fires (j_trace j6) + j_fired j = fires (j_trace j) + j_fired j6 /\ j_fired j <= j_fired j6 <= S (j_fired j)).
    { intros j6 T6 F6. rewrite T6, F6, T5, F5, fires_app, fires_retires. split; lia. }
    destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W)); apply H6; reflexivity.
  Qed.

  Lemma dstep_fires :
    forall own a j, j_fired j <= cap ->
      fires (j_trace (fst (dstep own a j))) + j_fired j = fires (j_trace j) + j_fired (fst (dstep own a j)) /\
      j_fired j <= j_fired (fst (dstep own a j)) <= cap.
  Proof.
    intros own a j Hc. unfold dstep. destruct (j_stop j); [cbn [fst]; split; lia |].
    destruct (j_work j) as [| q rest].
    - destruct cascades; [| cbn [fst]; split; lia].
      destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |]; [| cbn [fst]; split; lia].
      destruct (cap <=? j_fired j) eqn:Ec; [cbn [fst set_stop j_trace j_fired]; split; lia |].
      apply Nat.leb_gt in Ec. cbn [fst]. destruct (lift_reoffer_fires own j W sg) as [H1 H2]. split; lia.
    - destruct (cap <=? j_fired j) eqn:Ec; [cbn [fst set_stop j_trace j_fired]; split; lia |].
      apply Nat.leb_gt in Ec. cbn [fst].
      destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E.
      + destruct (offer_fires (S (length (v_wtrs (j_view (with_work rest j))))) own (with_work rest j) (resend_elem a q) Ec) as [H1 H2].
        cbn [with_work j_trace j_fired] in *. split; lia.
      + destruct (store_spec own (with_work rest j) (resend_elem a q)) as (bs & T & _ & _ & _ & _ & _ & _ & F & _).
        rewrite T, F, !fires_app, fires_bumps.
        replace (fires [SStore (e_rec (resend_elem a q)) (e_auth (resend_elem a q))]) with 0 by reflexivity.
        cbn [with_work j_trace j_fired]. split; lia.
  Qed.

  (* Headline 7 (plan 4.9). Every step of DRAIN-AND-CASCADE that goes on
     lowers μ = (non-persistent waiters, non-persistent records + pending
     second sends, pending second sends) lexicographically, or stops: a peek
     fire spends its waiter (no receive both peeks and persists), a fire of
     a non-persistent waiter removes it, a fire of a persistent waiter spends
     a non-persistent record (lifted or the second send) or is a divergent
     pair, a K_retire removes a non-persistent record, and a second send that
     is stored lowers the pending count. The pending second sends can hold
     the persistent element of the operation (decision 30, DV-3): an OFFER
     of it that goes on removes a non-persistent waiter in each round. A
     drain that starts at the fire count f stages at most cap - f fires: it
     checks the count before each fire. *)
  Theorem cascade_terminates :
    (forall own a j j',
       (forall W, In W (v_wtrs (j_view j)) -> wf_waiter W = true /\ w_binds W <> []) ->
       NoDup (map r_id (v_recs (j_view j))) -> NoDup (map w_id (v_wtrs (j_view j))) ->
       dstep own a j = (j', true) ->
       j_stop j' <> None \/ lex_lt (mu j') (mu j)) /\
    (forall fuel own a j, j_fired j <= cap ->
       fires (j_trace (drain fuel own a j)) + j_fired j <= fires (j_trace j) + cap).
  Proof.
    split.
    - intros own a j j' Hwf Hndr Hndw H.
      assert (Hpp : forall W, In W (v_wtrs (j_view j)) -> w_pers W = true -> w_peek W = false).
      { intros W Hw Hp. destruct (Hwf W Hw) as [Hw' _]. unfold wf_waiter in Hw'. rewrite Hp in Hw'.
        destruct (w_peek W); [discriminate | reflexivity]. }
      unfold dstep in H. destruct (j_stop j) eqn:Es; [discriminate |].
      destruct (j_work j) as [| q rest] eqn:Ew.
      + destruct cascades; [| discriminate].
        destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |] eqn:En; [| discriminate].
        destruct (cap <=? j_fired j); [discriminate |]. injection H as <-.
        destruct (next_fire_sel _ _ _ _ En) as (HwIn & Hf & Hsg).
        apply lift_reoffer_mu; [exact HwIn | exact (Hpp W HwIn) | exact Hndr | exact Hndw | intros e He; exact (proj1 (Hsg e He)) |].
        (* a fire binds one record for each bind *)
        intros E. subst sg. destruct (Hwf W HwIn) as [_ Hb].
        unfold fireable in Hf. destruct (heads_ok (colls W) W (j_view j)); [| discriminate].
        unfold receive_decision in Hf. destruct (greedy W (j_view j)) as [sg' |] eqn:Eg; [| discriminate].
        destruct (guard_ok W sg'); [injection Hf as -> | discriminate].
        unfold greedy in Eg. apply fill_shape, slots_rel_length in Eg. unfold slots_cons in Eg.
        rewrite length_map, (Permutation_length (gpairs_perm W)), enum_length in Eg.
        destruct (w_binds W); [exact (Hb eq_refl) | discriminate].
      + destruct (cap <=? j_fired j); [discriminate |]. injection H as <-.
        destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E.
        * destruct (offer_mu (S (length (v_wtrs (j_view (with_work rest j))))) own (with_work rest j) (resend_elem a q) Hpp Hndw)
            as [H1 | H1]; [left; exact H1 | right].
          unfold mux in H1. unfold mu. rewrite Ew. cbn [with_work j_view j_work length] in H1 |- *. exact H1.
        * right. destruct (store_spec own (with_work rest j) (resend_elem a q)) as (bs & _ & Vs & _ & _ & Wk & _).
          unfold mu. rewrite Vs, Wk, apply_all_app, Ew. destruct (bumps_view bs (j_view j)) as [Br Bw].
          unfold np_recs, np_wtrs. cbn [with_work j_view j_work apply_all fold_left apply v_recs v_wtrs filter length].
          rewrite Br, Bw. cbn [lex_lt]. destruct (negb (r_pers (e_rec (resend_elem a q)))); cbn [length]; lia.
    - induction fuel as [| f IH]; intros own a j Hc; cbn [drain]; [cbn [set_stop j_trace]; lia |].
      destruct (dstep own a j) as [j' b] eqn:E. pose proof (dstep_fires own a j Hc) as Hd. rewrite E in Hd. cbn [fst] in Hd.
      destruct Hd as [H1 H2]. destruct b; [specialize (IH own a j' (proj2 H2)); lia | lia].
  Qed.

  (* ── 32. Headline 8: one fire count for each transaction ─────────── *)

  Lemma drain_fires :
    forall fuel own a j, j_fired j <= cap ->
      fires (j_trace (drain fuel own a j)) + j_fired j = fires (j_trace j) + j_fired (drain fuel own a j) /\
      j_fired j <= j_fired (drain fuel own a j) <= cap.
  Proof.
    induction fuel as [| f IH]; intros own a j Hc; cbn [drain]; [cbn [set_stop j_trace j_fired]; split; lia |].
    destruct (dstep own a j) as [j' b] eqn:E. pose proof (dstep_fires own a j Hc) as Hd. rewrite E in Hd. cbn [fst] in Hd.
    destruct Hd as [H1 H2]. destruct b; [destruct (IH own a j' (proj2 H2)) as [H3 H4]; split; lia | split; lia].
  Qed.

  Lemma receive1_fires :
    forall j W j' b, receive1 j W = (j', b) ->
      fires (j_trace j') + j_fired j = fires (j_trace j) + j_fired j' /\ j_fired j <= j_fired j' <= S (j_fired j) /\
      (b = true -> j_fired j' < cap).
  Proof.
    intros j W j' b H. unfold receive1 in H.
    destruct (receive_sel (j_view j) W) as [sg |].
    - set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))) in H.
      destruct (fire_tail_spec W sg None false false j3) as (rs & T4 & _ & _ & _ & _ & _ & _ & F4 & _).
      assert (Hr : fires (j_trace (fire_tail W sg None false false j3)) + j_fired j = fires (j_trace j) + j_fired (fire_tail W sg None false false j3) /\
                   j_fired (fire_tail W sg None false false j3) = S (j_fired j)).
      { rewrite T4, F4. unfold j3. cbn [add_dirty with_dirty with_fired stage j_trace j_fired].
        rewrite !fires_app, fires_retires. replace (fires [SFire W sg None]) with 1 by reflexivity. split; lia. }
      destruct Hr as [Hr1 Hr2].
      destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j))) eqn:Ea;
        injection H as <- <-; cbn [set_own set_stop j_trace j_fired]; (split; [exact Hr1 | split; [lia | intros Hb]]); try discriminate.
      unfold after_fire in Ea. destruct (negb (w_pers W)); [discriminate |].
      destruct (rcv_stays W sg (j_view j)); [discriminate |].
      destruct (cap <=? S (j_fired j)) eqn:Ec; [discriminate |]. apply Nat.leb_gt in Ec. lia.
    - injection H as <- <-. unfold park. cbn [set_own stage j_trace j_fired].
      rewrite fires_app. replace (fires [SPark W]) with 0 by reflexivity. split; [lia | split; [lia | discriminate]].
  Qed.

  Lemma receive_fires :
    forall fuel j W, j_fired j < cap ->
      fires (j_trace (receive fuel j W)) + j_fired j = fires (j_trace j) + j_fired (receive fuel j W) /\
      j_fired j <= j_fired (receive fuel j W) <= cap.
  Proof.
    induction fuel as [| f IH]; intros j W Hc; cbn [receive]; [cbn [set_own set_stop j_trace j_fired]; split; lia |].
    destruct (receive1 j W) as [j' b] eqn:E. destruct (receive1_fires j W j' b E) as (H1 & H2 & H3).
    destruct b; [destruct (IH j' W (H3 eq_refl)) as [H4 H5]; split; lia | split; lia].
  Qed.

  Lemma fires_touch_all :
    forall own cs j, fires (j_trace (touch_all own j cs)) = fires (j_trace j) /\ j_fired (touch_all own j cs) = j_fired j.
  Proof.
    intros own cs j. destruct (touch_all_spec own cs j) as (bs & T & _ & _ & _ & _ & _ & _ & F & _).
    rewrite T, F, fires_app, fires_bumps. split; lia.
  Qed.

  (* The count of one transaction, for any rule of the count at the phase
     boundaries that keeps the count. *)
  Lemma run_join_fires_gen :
    (forall f, phase_count f = f) ->
    forall fuel o scope v0,
      fires (j_trace (fst (run_join fuel o scope v0))) = j_fired (fst (run_join fuel o scope v0)) /\
      j_fired (fst (run_join fuel o scope v0)) <= cap.
  Proof.
    intros Hpc fuel o scope v0. unfold run_join.
    set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
    assert (F1 : fires (j_trace j1) = j_fired j1 /\ j_fired j1 = 0).
    { unfold j1. cbn [with_work j_trace j_fired]. destruct (fires_touch_all (own_of o) (own_of o) (init v0)) as [Ft Ff].
      rewrite Ft, Ff. split; reflexivity. }
    destruct F1 as [F1 Z1].
    assert (Hgd : fires (j_trace (drain fuel (own_of o) (auth_of o) (with_dirty scope j1))) =
                 j_fired (drain fuel (own_of o) (auth_of o) (with_dirty scope j1)) /\
                 j_fired (drain fuel (own_of o) (auth_of o) (with_dirty scope j1)) <= cap).
    { destruct (drain_fires fuel (own_of o) (auth_of o) (with_dirty scope j1)) as [H1 H2]; [cbn [with_dirty j_fired]; lia |].
      cbn [with_dirty j_trace j_fired] in H1, H2. split; lia. }
    destruct (is_settle o); [cbn [fst]; exact Hgd |].
    set (j3 := if cascades then drain fuel (own_of o) (auth_of o) (with_dirty scope j1) else j1).
    assert (G3 : fires (j_trace j3) = j_fired j3 /\ j_fired j3 <= cap).
    { unfold j3. destruct cascades; [exact Hgd | split; lia]. }
    destruct G3 as [G3 C3].
    destruct (j_stop j3); [cbn [fst]; split; assumption |].
    destruct (plan_at_cap (j_fired (with_dirty [] j3))) eqn:Eo; [cbn [fst set_stop with_dirty j_trace j_fired]; split; assumption |].
    cbn [fst].
    set (j4 := with_fired (phase_count (j_fired (with_dirty [] j3))) (with_dirty [] j3)).
    assert (G4 : fires (j_trace j4) = j_fired j4 /\ j_fired j4 = j_fired j3).
    { unfold j4. rewrite Hpc. cbn [with_fired with_dirty j_trace j_fired]. split; lia. }
    destruct G4 as [G4 E4].
    assert (Hlt : j_fired j4 < cap).
    { rewrite E4. unfold plan_at_cap in Eo. cbn [with_dirty j_fired] in Eo. apply Nat.leb_gt in Eo. exact Eo. }
    assert (G5 : fires (j_trace (plan fuel o v0 j4)) = j_fired (plan fuel o v0 j4) /\ j_fired (plan fuel o v0 j4) <= cap).
    { destruct o as [x c a | W c a | key sd pd rs a]; cbn [plan own_of].
      - destruct (offer_fires fuel [r_coll x] j4 (mkElem (with_ord x (prod_ord v0 x c)) a true) Hlt) as [H1 H2]. split; lia.
      - destruct (receive_fires fuel j4 (with_ords W (ords_of v0 W c)) Hlt) as [H1 H2]. split; lia.
      - split; lia. }
    destruct G5 as [G5 C5]. rewrite Hpc.
    destruct (drain_fires fuel (own_of o) (auth_of o) (with_fired (j_fired (plan fuel o v0 j4)) (plan fuel o v0 j4)))
      as [H1 H2]; [cbn [with_fired j_fired]; exact C5 |].
    cbn [with_fired j_trace j_fired] in H1, H2. split; lia.
  Qed.

  (* Headline 8 (decision 49, revision I-21). The settle before the plan,
     the plan, the second sends and the cascade of one transaction share one
     fire count, so the transaction stages at most FIRE_ROUND_CAP fires: the
     count of fires of its trace is the fire count of its state, and every
     phase checks it before it fires. A settle job is a transaction of its
     own, and its count starts at 0. *)
  Theorem join_txn_fires_bounded :
    forall fuel o scope v0,
      fires (j_trace (fst (run_join fuel o scope v0))) = j_fired (fst (run_join fuel o scope v0)) /\
      fires (j_trace (fst (run_join fuel o scope v0))) <= cap /\
      j_fired (with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))) = 0.
  Proof.
    intros fuel o scope v0.
    assert (Hpc : forall f, phase_count f = f) by (intros f; unfold phase_count; reflexivity).
    destruct (run_join_fires_gen Hpc fuel o scope v0) as [H1 H2].
    split; [exact H1 | split; [lia |]].
    cbn [with_work j_fired]. exact (proj2 (fires_touch_all (own_of o) (own_of o) (init v0))).
  Qed.

  (* ── 33. Headline 16: the second sends of a peek join ─────────────── *)

  (* The live records: the stored records and the pending second sends. *)
  Definition live (j : jst) : list nat :=
    map r_id (v_recs (j_view j)) ++ map (fun q => r_id (p_rec q)) (j_work j).

  (* The identities of the second sends of a work list that are not the
     element of an operation. *)
  Definition pend_ids (w : list pend) : list nat := map (fun q => r_id (p_rec q)) (filter (fun q => negb (p_own q)) w).

  (* The tracked records: the stored records and the pending second sends
     that are not the element of the operation (pending_work). The element
     of the operation returns to its operation at COMMIT (resume_of), so the
     transaction does not hold it as a record. *)
  Definition tracked (j : jst) : list nat := map r_id (v_recs (j_view j)) ++ pend_ids (j_work j).

  (* The records that a staged operation spends: the non-persistent records
     of a fire of a waiter that does not peek, and a retired member. *)
  Definition spent_by (op0 : staged) : list nat :=
    match op0 with
    | SFire W sg _ =>
        if w_peek W then [] else map (fun rm => r_id (fst rm)) (filter (fun rm => negb (r_pers (fst rm))) sg)
    | SRetire r => [r_id r]
    | SBump _ | SStore _ _ | SPark _ | SLift _ => []
    end.

  Definition spent_ids (tr : list staged) : list nat := flat_map spent_by tr.

  Lemma spent_ids_app : forall a b, spent_ids (a ++ b) = spent_ids a ++ spent_ids b.
  Proof. intros a b. unfold spent_ids. apply flat_map_app. Qed.

  Lemma removed_spec :
    forall sg ainc k e m, nth_error sg k = Some (e, m) -> inc_pos ainc k = false -> r_pers e = false -> In e (removed sg ainc).
  Proof.
    intros sg ainc k e m Hk Hi Hp. unfold removed. apply in_map_iff. exists (k, (e, m)). split; [reflexivity |].
    apply filter_In. split; [apply nth_error_In with k; rewrite enum_nth_error, Hk; reflexivity |].
    cbn [fst snd]. rewrite Hi, Hp. reflexivity.
  Qed.

  Lemma removed_pos :
    forall sg ainc r, In r (removed sg ainc) -> exists k m, nth_error sg k = Some (r, m) /\ inc_pos ainc k = false.
  Proof.
    intros sg ainc r H. unfold removed in H. apply in_map_iff in H as ([k [e m]] & <- & H). apply filter_In in H as [H Hf].
    apply andb_true_iff in Hf as [Hf _]. apply negb_true_iff in Hf. apply (enum_nth _ sg 0) in H as [_ H].
    rewrite Nat.sub_0_r in H. exists k, m. split; [exact H | exact Hf].
  Qed.

  Lemma removed_np : forall sg ainc r, In r (removed sg ainc) -> r_pers r = false.
  Proof.
    intros sg ainc r H. unfold removed in H. apply in_map_iff in H as ([k [e m]] & <- & H). apply filter_In in H as [_ Hf].
    apply andb_true_iff in Hf as [_ Hp]. apply negb_true_iff in Hp. exact Hp.
  Qed.

  (* The view after the lift and the fire of the cascade. *)
  Lemma lift_fire_recs :
    forall v bs W sg vp a rv mv, nth_error sg vp = Some (rv, mv) ->
      v_recs (apply (apply_all (apply_all v (map SBump bs)) (lift_ops rv)) (SFire W sg (Some (vp, a)))) =
        remove_all ((if r_pers rv then [] else [rv]) ++ removed sg (Some (vp, a))) (v_recs v).
  Proof.
    intros v bs W sg vp a rv mv Hn. destruct (bumps_view bs v) as [Br _]. unfold lift_ops. destruct (r_pers rv).
    - cbn [apply_all fold_left apply v_recs app]. rewrite Br. reflexivity.
    - cbn [apply_all fold_left apply v_recs app]. rewrite Br. reflexivity.
  Qed.

  Definition keeps (j j' : jst) : Prop :=
    exists ops, j_trace j' = j_trace j ++ ops /\
      forall id, In id (tracked j) -> In id (tracked j') \/ In id (spent_ids ops).

  Lemma keeps_trans : forall j1 j2 j3, keeps j1 j2 -> keeps j2 j3 -> keeps j1 j3.
  Proof.
    intros j1 j2 j3 (o1 & T1 & K1) (o2 & T2 & K2). exists (o1 ++ o2). split; [rewrite T2, T1, app_assoc; reflexivity |].
    intros id H. destruct (K1 id H) as [H1 | H1].
    - destruct (K2 id H1) as [H2 | H2]; [left; exact H2 | right; rewrite spent_ids_app; apply in_or_app; right; exact H2].
    - right. rewrite spent_ids_app. apply in_or_app. left. exact H1.
  Qed.

  Lemma keeps_same : forall j j', j_trace j' = j_trace j -> v_recs (j_view j') = v_recs (j_view j) -> j_work j' = j_work j -> keeps j j'.
  Proof.
    intros j j' T Hv Hw. exists []. split; [rewrite T, app_nil_r; reflexivity |]. intros id H. left.
    unfold tracked in *. rewrite Hv, Hw. exact H.
  Qed.

  Lemma in_del_rec_id : forall i l id, In id (map r_id l) -> In id (map r_id (del_rec i l)) \/ id = i.
  Proof.
    intros i l. induction l as [| r rest IH]; intros id H; [destruct H |]. cbn [del_rec map] in *.
    destruct (Nat.eqb_spec (r_id r) i) as [E | E].
    - destruct H as [<- | H]; [right; exact E | left; exact H].
    - cbn [map In]. destruct H as [<- | H]; [left; left; reflexivity |].
      destruct (IH id H) as [H1 | H1]; [left; right; exact H1 | right; exact H1].
  Qed.

  Lemma in_remove_all_id :
    forall rs l id, In id (map r_id l) -> In id (map r_id (remove_all rs l)) \/ In id (map r_id rs).
  Proof.
    induction rs as [| r rest IH]; intros l id H; [left; exact H |]. rewrite remove_all_cons.
    destruct (in_del_rec_id (r_id r) l id H) as [H1 | H1].
    - destruct (IH _ id H1) as [H2 | H2]; [left; exact H2 | right; right; exact H2].
    - right. left. symmetry. exact H1.
  Qed.

  (* The tracked identities of a work list, through a permutation. *)
  Lemma pend_ids_perm : forall w w' id, Permutation w w' -> In id (pend_ids w') -> In id (pend_ids w).
  Proof.
    intros w w' id Hp H. unfold pend_ids in *. apply in_map_iff in H as (q & <- & Hq). apply filter_In in Hq as [Hq Ho].
    apply (in_map (fun q0 => r_id (p_rec q0))). apply filter_In. split; [exact (Permutation_in _ (Permutation_sym Hp) Hq) | exact Ho].
  Qed.

  Lemma pend_ids_app : forall a b id, In id (pend_ids (a ++ b)) <-> In id (pend_ids a) \/ In id (pend_ids b).
  Proof. intros a b id. unfold pend_ids. rewrite filter_app, map_app. apply in_app_iff. Qed.

  Lemma is_inc_option_map : forall (ainc : option (nat * A)) k, is_inc (option_map fst ainc) k = inc_pos ainc k.
  Proof. intros [[p a] |] k; reflexivity. Qed.

  (* A record that a fire removes from the store is a second send of a peek
     fire, and not the element of the operation. *)
  Lemma removed_pushed :
    forall sg ainc oi fl id, In id (map r_id (removed sg ainc)) ->
      In id (pend_ids (np_results sg (option_map fst ainc) oi fl)).
  Proof.
    intros sg ainc oi fl id H. apply in_map_iff in H as (r & <- & Hr). unfold removed in Hr.
    apply in_map_iff in Hr as ([k [e m]] & <- & Hr). apply filter_In in Hr as [Hr Hf].
    apply andb_true_iff in Hf as [Hi Hp]. apply negb_true_iff in Hi. cbn [fst snd] in Hi, Hp |- *.
    unfold pend_ids. apply in_map_iff. exists (mkPend e false). split; [reflexivity |].
    apply filter_In. split; [| reflexivity].
    unfold np_results. apply in_map_iff. exists (k, (e, m)). cbn [fst snd].
    rewrite is_inc_option_map, Hi, andb_false_r. split; [reflexivity |].
    apply filter_In. split; [exact Hr |]. unfold sends_at. cbn [fst snd]. rewrite is_inc_option_map, Hi. exact Hp.
  Qed.

  (* The non-persistent element in flight of OFFER is a second send of a
     peek fire, with the own flag of the element. *)
  Lemma inc_pushed :
    forall sg p e m oi, nth_error sg p = Some (e, m) -> r_pers e = false -> In (mkPend e oi) (np_results sg (Some p) oi true).
  Proof.
    intros sg p e m oi Hk Hp. unfold np_results. apply in_map_iff. exists (p, (e, m)). cbn [fst snd is_inc].
    rewrite Nat.eqb_refl, andb_true_r. split; [reflexivity |].
    apply filter_In. split; [apply nth_error_In with p; rewrite enum_nth_error, Hk; reflexivity |].
    unfold sends_at. cbn [fst snd is_inc]. rewrite Nat.eqb_refl. try rewrite Hp. reflexivity.
  Qed.

  (* The cascade pushes every non-persistent bound record, the lifted one
     included, as a second send that is not the element of an operation. *)
  Lemma lift_pushed :
    forall sg vp rv mv k e m, nth_error sg vp = Some (rv, mv) -> nth_error sg k = Some (e, m) -> r_pers e = false ->
      In (mkPend e false) (np_results sg (Some vp) false (negb (r_pers rv))).
  Proof.
    intros sg vp rv mv k e m Hv Hk Hp. unfold np_results. apply in_map_iff. exists (k, (e, m)). cbn [fst snd].
    split; [reflexivity |]. apply filter_In. split; [apply nth_error_In with k; rewrite enum_nth_error, Hk; reflexivity |].
    unfold sends_at. cbn [fst snd is_inc]. destruct (Nat.eqb_spec k vp) as [-> | Ne].
    - rewrite Hv in Hk. injection Hk as -> _. rewrite Hp. reflexivity.
    - rewrite Hp. reflexivity.
  Qed.

  (* A fire and its tail keep each tracked record: a record that leaves the
     store (removed by the fire or lifted) is pushed again by a peek fire or
     spent by a fire that does not peek, and a retired member is spent. *)
  Lemma fire_tail_keeps :
    forall j bs ops W sg ainc j3 inc oi fl R,
      j_trace j3 = j_trace j ++ map SBump bs ++ ops ++ [SFire W sg ainc] ->
      spent_ids ops = [] ->
      v_recs (j_view j3) = remove_all R (v_recs (j_view j)) ->
      j_work j3 = j_work j ->
      (forall r, In r R -> r_pers r = false /\ In r (map fst sg) /\
                 (w_peek W = true -> In (r_id r) (pend_ids (np_results sg inc oi fl)))) ->
      keeps j (fire_tail W sg inc oi fl j3).
  Proof.
    intros j bs ops W sg ainc j3 inc oi fl R T3 Hops V3 W3 HR.
    destruct (fire_tail_spec W sg inc oi fl j3) as (rs & T4 & V4 & _ & _ & P4 & _ & _ & _ & _).
    exists (map SBump bs ++ ops ++ [SFire W sg ainc] ++ map SRetire rs). split; [rewrite T4, T3, !app_assoc; reflexivity |].
    assert (Hfire : forall r, In r (map fst sg) -> r_pers r = false -> w_peek W = false ->
                      In (r_id r) (spent_ids (map SBump bs ++ ops ++ [SFire W sg ainc] ++ map SRetire rs))).
    { intros r Hr Hp Ep. rewrite !spent_ids_app. apply in_or_app. right. apply in_or_app. right. apply in_or_app. left.
      cbn [spent_ids flat_map spent_by]. rewrite Ep, app_nil_r. apply in_map_iff in Hr as ([e m] & E & Hr). cbn [fst] in E. subst e.
      apply in_map_iff. exists (r, m). split; [reflexivity |]. apply filter_In. split; [exact Hr | cbn [fst]; rewrite Hp; reflexivity]. }
    intros id H. unfold tracked in H |- *. rewrite V4. cbn [v_recs]. apply in_app_or in H as [H | H].
    - destruct (in_remove_all_id R (v_recs (j_view j)) id H) as [H1 | H1].
      + rewrite <- V3 in H1. destruct (in_remove_all_id rs (v_recs (j_view j3)) id H1) as [H2 | H2].
        * left. apply in_or_app. left. exact H2.
        * right. rewrite !spent_ids_app. apply in_or_app. right. apply in_or_app. right. apply in_or_app. right.
          apply in_map_iff in H2 as (r & E & Hr). unfold spent_ids. apply in_flat_map. exists (SRetire r).
          split; [apply in_map; exact Hr | left; exact E].
      + apply in_map_iff in H1 as (r & <- & Hr). destruct (HR r Hr) as (Hp & Hsg & Hpush).
        destruct (w_peek W) eqn:Ep.
        * left. apply in_or_app. right.
          apply (pend_ids_perm _ _ _ P4). apply pend_ids_app. left. exact (Hpush eq_refl).
        * right. exact (Hfire r Hsg Hp eq_refl).
    - left. apply in_or_app. right.
      apply (pend_ids_perm _ _ _ P4). apply pend_ids_app. right. rewrite W3. exact H.
  Qed.

  (* The tracked records and extra records (an element in flight). *)
  Definition keeps_with (extra : list nat) (j j' : jst) : Prop :=
    exists ops, j_trace j' = j_trace j ++ ops /\
      forall id, In id (tracked j ++ extra) -> In id (tracked j') \/ In id (spent_ids ops).

  Lemma keeps_with_nil : forall j j', keeps j j' -> keeps_with [] j j'.
  Proof. intros j j' (ops & T & K). exists ops. split; [exact T |]. intros id H. rewrite app_nil_r in H. exact (K id H). Qed.

  Lemma keeps_with_less : forall extra j j', keeps_with extra j j' -> keeps_with [] j j'.
  Proof.
    intros extra j j' (ops & T & K). exists ops. split; [exact T |]. intros id H. rewrite app_nil_r in H.
    apply K. apply in_or_app. left. exact H.
  Qed.

  Lemma keeps_with_keeps : forall extra j1 j2 j3, keeps_with extra j1 j2 -> keeps j2 j3 -> keeps_with extra j1 j3.
  Proof.
    intros extra j1 j2 j3 (o1 & T1 & K1) (o2 & T2 & K2). exists (o1 ++ o2). split; [rewrite T2, T1, app_assoc; reflexivity |].
    intros id H. destruct (K1 id H) as [H1 | H1].
    - destruct (K2 id H1) as [H2 | H2]; [left; exact H2 | right; rewrite spent_ids_app; apply in_or_app; right; exact H2].
    - right. rewrite spent_ids_app. apply in_or_app. left. exact H1.
  Qed.

  Lemma keeps_ctl : forall j j', keeps j j' -> forall j'', j_trace j'' = j_trace j' -> v_recs (j_view j'') = v_recs (j_view j') ->
    j_work j'' = j_work j' -> keeps j j''.
  Proof. intros j j' K j'' T Hv Hw. exact (keeps_trans _ _ _ K (keeps_same _ _ T Hv Hw)). Qed.

  Lemma keeps_with_ctl : forall extra j j', keeps_with extra j j' -> forall j'', j_trace j'' = j_trace j' ->
    v_recs (j_view j'') = v_recs (j_view j') -> j_work j'' = j_work j' -> keeps_with extra j j''.
  Proof. intros extra j j' K j'' T Hv Hw. exact (keeps_with_keeps _ _ _ _ K (keeps_same _ _ T Hv Hw)). Qed.

  Lemma keeps_touch_all : forall own cs j, keeps j (touch_all own j cs).
  Proof.
    intros own cs j. destruct (touch_all_spec own cs j) as (bs & T & Vw & _ & _ & Wk & _).
    exists (map SBump bs). split; [exact T |]. intros id H. left. unfold tracked in *. rewrite Vw, Wk.
    rewrite (proj1 (bumps_view bs (j_view j))). exact H.
  Qed.

  Lemma keeps_store : forall own j x, keeps_with [r_id (e_rec x)] j (store own j x).
  Proof.
    intros own j x. destruct (store_spec own j x) as (bs & T & Vw & _ & _ & Wk & _).
    exists (map SBump bs ++ [SStore (e_rec x) (e_auth x)]). split; [exact T |].
    intros id H. left. unfold tracked in *. rewrite Vw, Wk, apply_all_app. cbn [apply_all fold_left apply v_recs map].
    rewrite (proj1 (bumps_view bs (j_view j))).
    apply in_app_or in H as [H | [<- | []]].
    - apply in_app_or in H as [H | H]; [apply in_or_app; left; right; exact H | apply in_or_app; right; exact H].
    - apply in_or_app. left. left. reflexivity.
  Qed.

  (* The fire step of OFFER keeps the tracked records. *)
  Lemma keeps_offer_fire :
    forall own j x W p sg,
      offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j)) = Some (W, p, sg) ->
      let j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                  (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x))))) in
      keeps j (fire_tail W sg (Some p) (e_own x) true j3).
  Proof.
    intros own j x W p sg Ep j3.
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & V1 & _ & _ & W1 & _).
    apply (fire_tail_keeps j bs [] W sg (Some (p, e_auth x)) j3 (Some p) (e_own x) true (removed sg (Some (p, e_auth x)))).
    - unfold j3. cbn [add_dirty with_dirty with_fired stage j_trace app]. rewrite T1, app_assoc. reflexivity.
    - reflexivity.
    - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_recs]. rewrite V1.
      rewrite (proj1 (bumps_view bs (j_view j))). reflexivity.
    - unfold j3. cbn [add_dirty with_dirty with_fired stage j_work]. exact W1.
    - intros r Hr. split; [exact (removed_np _ _ _ Hr) |]. destruct (removed_pos _ _ _ Hr) as (k & m & Hk & _).
      split; [apply in_map_iff; exists (r, m); split; [reflexivity | exact (nth_error_In _ _ Hk)] |].
      intros _. exact (removed_pushed sg (Some (p, e_auth x)) (e_own x) true (r_id r) (in_map r_id _ r Hr)).
  Qed.

  (* OFFER of a tracked element in flight (not the element of the
     operation, not persistent): the element is pushed again by a peek fire,
     spent by another fire, or stored. *)
  Lemma keeps_offer1 :
    forall own j x, r_pers (e_rec x) = false -> e_own x = false -> keeps_with [r_id (e_rec x)] j (fst (offer1 own j x)).
  Proof.
    intros own j x Hx Ho. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep; [| apply keeps_store].
    destruct (proj1 join_fires_heads_only _ _ _ _ _ Ep) as (_ & _ & _ & mx & Hxp).
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & _).
    pose proof (keeps_offer_fire own j x W p sg Ep) as K4. cbv zeta in K4.
    set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                 (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))) in *.
    (* the incoming record: pushed again by a peek, spent by any other fire *)
    assert (Kx : keeps_with [r_id (e_rec x)] j (fire_tail W sg (Some p) (e_own x) true j3)).
    { destruct K4 as (ops & T & K). exists ops. split; [exact T |]. intros id H.
      apply in_app_or in H as [H | [<- | []]]; [exact (K id H) |].
      destruct (fire_tail_spec W sg (Some p) (e_own x) true j3) as (rs & T4 & _ & _ & _ & P4 & _).
      assert (Hops : ops = map SBump bs ++ [SFire W sg (Some (p, e_auth x))] ++ map SRetire rs).
      { rewrite T in T4. unfold j3 in T4. cbn [add_dirty with_dirty with_fired stage j_trace] in T4. rewrite T1 in T4.
        rewrite <- !app_assoc in T4. apply app_inv_head in T4. exact T4. }
      destruct (w_peek W) eqn:Epk.
      - left. unfold tracked. apply in_or_app. right. apply (pend_ids_perm _ _ _ P4). try rewrite Epk.
        apply pend_ids_app. left. unfold pend_ids. apply in_map_iff.
        exists (mkPend (e_rec x) (e_own x)). split; [reflexivity |]. apply filter_In.
        split; [exact (inc_pushed sg p (e_rec x) mx (e_own x) Hxp Hx) | cbn [p_own]; rewrite Ho; reflexivity].
      - right. rewrite Hops, !spent_ids_app. apply in_or_app. right. apply in_or_app. left.
        cbn [spent_ids flat_map spent_by]. try rewrite Epk. rewrite ?app_nil_r. apply in_map_iff. exists (e_rec x, mx).
        split; [reflexivity |]. apply filter_In. split; [exact (nth_error_In _ _ Hxp) | cbn [fst]; rewrite Hx; reflexivity]. }
    destruct (w_peek W); [exact Kx |].
    destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
      cbn [fst]; unfold own_flight; try destruct (e_own x); apply (keeps_with_ctl _ _ _ Kx); reflexivity.
  Qed.

  Lemma keeps_offer : forall fuel own j x, keeps j (offer fuel own j x).
  Proof.
    induction fuel as [| f IH]; intros own j x; cbn [offer].
    - unfold own_flight. destruct (e_own x); apply keeps_same; reflexivity.
    - assert (K1 : keeps j (fst (offer1 own j x))).
      { unfold offer1.
        destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep.
        - pose proof (keeps_offer_fire own j x W p sg Ep) as K4. cbv zeta in K4.
          destruct (w_peek W); [exact K4 |].
          destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
            cbn [fst]; unfold own_flight; try destruct (e_own x); apply (keeps_ctl _ _ K4); reflexivity.
        - cbn [fst]. destruct (keeps_store own j x) as (ops & T & K). exists ops. split; [exact T |].
          intros id H. apply K. apply in_or_app. left. exact H. }
      destruct (offer1 own j x) as [j' []]; cbn [fst] in K1; [exact (keeps_trans _ _ _ K1 (IH own j' x)) | exact K1].
  Qed.

  Lemma offer1_np_stops : forall own j x, r_pers (e_rec x) = false -> snd (offer1 own j x) = false.
  Proof.
    intros own j x Hx. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |]; [| reflexivity].
    destruct (w_peek W); [reflexivity |]. rewrite Hx. cbn [negb andb after_fire]. destruct (e_own x); reflexivity.
  Qed.

  Lemma keeps_receive1 : forall j W, keeps j (fst (receive1 j W)).
  Proof.
    intros j W. unfold receive1. destruct (receive_sel (j_view j) W) as [sg |].
    - set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
      assert (K4 : keeps j (fire_tail W sg None false false j3)).
      { apply (fire_tail_keeps j [] [] W sg None j3 None false false (removed sg None)).
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_trace map app]. reflexivity.
        - reflexivity.
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_recs]. reflexivity.
        - unfold j3. reflexivity.
        - intros r Hr. split; [exact (removed_np _ _ _ Hr) |]. destruct (removed_pos _ _ _ Hr) as (k & m & Hk & _).
          split; [apply in_map_iff; exists (r, m); split; [reflexivity | exact (nth_error_In _ _ Hk)] |].
          intros _. exact (removed_pushed sg None false false (r_id r) (in_map r_id _ r Hr)). }
      destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)));
        cbn [fst]; apply (keeps_ctl _ _ K4); reflexivity.
    - cbn [fst]. unfold park. exists [SPark W]. split; [reflexivity |]. intros id H. left. exact H.
  Qed.

  Lemma keeps_receive : forall fuel j W, keeps j (receive fuel j W).
  Proof.
    induction fuel as [| f IH]; intros j W; cbn [receive]; [apply keeps_same; reflexivity |].
    pose proof (keeps_receive1 j W) as K1. destruct (receive1 j W) as [j' []]; cbn [fst] in K1;
      [exact (keeps_trans _ _ _ K1 (IH j' W)) | exact K1].
  Qed.

  Lemma keeps_lift_reoffer : forall own j W sg, keeps j (lift_reoffer own j W sg).
  Proof.
    intros own j W sg. unfold lift_reoffer. destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn;
      [| apply keeps_same; reflexivity].
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & V1 & _ & _ & W1 & _).
    destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (T2 & V2 & _ & _ & W2 & _).
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                 (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    destruct (removed_split sg (reoffer_pos sg) rv mv (own_auth rv) Hn) as (A1 & A2 & E1 & E2).
    assert (K5 : keeps j (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4)).
    { apply (fire_tail_keeps j bs (lift_ops rv) W sg (Some (reoffer_pos sg, own_auth rv)) j4
               (Some (reoffer_pos sg)) false (negb (r_pers rv)) (removed sg None)).
      - unfold j4. cbn [add_dirty with_dirty with_fired stage j_trace]. rewrite T2, T1, <- !app_assoc. reflexivity.
      - unfold lift_ops. destruct (r_pers rv); reflexivity.
      - unfold j4. cbn [add_dirty with_dirty with_fired stage j_view]. rewrite V2, V1.
        rewrite (lift_fire_recs (j_view j) bs W sg (reoffer_pos sg) (own_auth rv) rv mv Hn).
        apply remove_all_perm. rewrite E1, E2.
        destruct (r_pers rv); cbn [app]; [apply Permutation_refl | apply Permutation_middle].
      - unfold j4. cbn [add_dirty with_dirty with_fired stage j_work]. rewrite W2. exact W1.
      - intros r Hr. split; [exact (removed_np _ _ _ Hr) |]. destruct (removed_pos _ _ _ Hr) as (k & m & Hk & _).
        split; [apply in_map_iff; exists (r, m); split; [reflexivity | exact (nth_error_In _ _ Hk)] |].
        intros _. unfold pend_ids. apply in_map_iff. exists (mkPend r false). split; [reflexivity |].
        apply filter_In. split; [exact (lift_pushed sg (reoffer_pos sg) rv mv k r m Hn Hk (removed_np _ _ _ Hr)) | reflexivity]. }
    destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W));
      try exact K5; apply (keeps_ctl _ _ K5); reflexivity.
  Qed.

  (* The pending second sends that are not the element of the operation are
     not persistent: they are removed records or lifted records. *)
  Definition work_np (j : jst) : Prop := forall q, In q (j_work j) -> p_own q = false -> r_pers (p_rec q) = false.

  Lemma fire_tail_work_np :
    forall W sg inc oi fl j,
      (forall p r m, inc = Some p -> nth_error sg p = Some (r, m) -> fl = true -> oi = false -> r_pers r = false) ->
      work_np j -> work_np (fire_tail W sg inc oi fl j).
  Proof.
    intros W sg inc oi fl j Hinc H q Hq Ho. destruct (fire_tail_spec W sg inc oi fl j) as (_ & _ & _ & _ & _ & Pw & _).
    apply (Permutation_in _ Pw) in Hq. apply in_app_or in Hq as [Hq | Hq]; [| exact (H q Hq Ho)].
    destruct (w_peek W); [| destruct Hq].
    destruct (np_results_spec _ _ _ _ _ Hq) as [_ [[Hp _] | (Hfl & Hoi & p & m & Hi & Hn)]]; [exact Hp |].
    apply (Hinc p (p_rec q) m Hi Hn Hfl). rewrite <- Hoi. exact Ho.
  Qed.

  Lemma work_np_offer1 :
    forall own j x, (e_own x = false -> r_pers (e_rec x) = false) -> work_np j -> work_np (fst (offer1 own j x)).
  Proof.
    intros own j x Hx H. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep.
    - destruct (proj1 join_fires_heads_only _ _ _ _ _ Ep) as (_ & _ & _ & mx & Hxp).
      set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                   (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
      assert (H3 : work_np j3).
      { intros q Hq Ho. unfold j3 in Hq. cbn [add_dirty with_dirty with_fired stage j_work] in Hq.
        destruct (touch_all_spec own (colls W) j) as (_ & _ & _ & _ & _ & Wk & _). rewrite Wk in Hq. exact (H q Hq Ho). }
      assert (H4 : work_np (fire_tail W sg (Some p) (e_own x) true j3)).
      { apply fire_tail_work_np; [| exact H3]. intros p0 r m E Hn _ Ho. injection E as <-.
        rewrite Hxp in Hn. injection Hn as <- _. exact (Hx Ho). }
      destruct (w_peek W); [exact H4 |].
      destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
        cbn [fst]; unfold own_flight; try destruct (e_own x); exact H4.
    - cbn [fst]. destruct (store_spec own j x) as (_ & _ & _ & _ & _ & Wk & _). intros q Hq. rewrite Wk in Hq. exact (H q Hq).
  Qed.

  Lemma work_np_lift_reoffer : forall own j W sg, work_np j -> work_np (lift_reoffer own j W sg).
  Proof.
    intros own j W sg H. unfold lift_reoffer. destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn; [| exact H].
    destruct (touch_all_spec own (colls W) j) as (_ & _ & _ & _ & _ & W1 & _).
    destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (_ & _ & _ & _ & W2 & _).
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                 (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    assert (H4 : work_np j4) by (intros q Hq; unfold j4 in Hq; cbn [add_dirty with_dirty with_fired stage j_work] in Hq;
                                 rewrite W2, W1 in Hq; exact (H q Hq)).
    assert (H5 : work_np (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4)).
    { apply fire_tail_work_np; [| exact H4]. intros p r m E Hn' Hfl _. injection E as <-.
      rewrite Hn in Hn'. injection Hn' as <- _. apply negb_true_iff in Hfl. exact Hfl. }
    destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W)); exact H5.
  Qed.

  Lemma work_np_offer :
    forall fuel own j x, (e_own x = false -> r_pers (e_rec x) = false) -> work_np j -> work_np (offer fuel own j x).
  Proof.
    induction fuel as [| f IH]; intros own j x Hx H; cbn [offer]; [unfold own_flight; destruct (e_own x); exact H |].
    pose proof (work_np_offer1 own j x Hx H) as H1. destruct (offer1 own j x) as [j' []]; cbn [fst] in H1;
      [exact (IH own j' x Hx H1) | exact H1].
  Qed.

  Lemma work_np_receive : forall fuel j W, work_np j -> work_np (receive fuel j W).
  Proof.
    induction fuel as [| f IH]; intros j W H; cbn [receive]; [exact H |].
    assert (H1 : work_np (fst (receive1 j W))).
    { unfold receive1. destruct (receive_sel (j_view j) W) as [sg |]; [| exact H].
      set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
      assert (H4 : work_np (fire_tail W sg None false false j3))
        by (apply fire_tail_work_np; [intros p r m E; discriminate | exact H]).
      destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j))); exact H4. }
    destruct (receive1 j W) as [j' []]; cbn [fst] in H1; [exact (IH j' W H1) | exact H1].
  Qed.

  Lemma dstep_keeps : forall own a j, work_np j -> keeps j (fst (dstep own a j)) /\ work_np (fst (dstep own a j)).
  Proof.
    intros own a j Hw. unfold dstep. destruct (j_stop j); [split; [apply keeps_same; reflexivity | exact Hw] |].
    destruct (j_work j) as [| q rest] eqn:Ew.
    - destruct cascades; [| split; [apply keeps_same; reflexivity | exact Hw]].
      destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |]; [| split; [apply keeps_same; reflexivity | exact Hw]].
      destruct (cap <=? j_fired j); [cbn [fst]; split; [apply keeps_same; reflexivity | exact Hw] |].
      cbn [fst]. split; [apply keeps_lift_reoffer | apply work_np_lift_reoffer; exact Hw].
    - destruct (cap <=? j_fired j); [cbn [fst]; split; [apply keeps_same; reflexivity | exact Hw] |].
      cbn [fst].
      assert (Hw0 : work_np (with_work rest j)) by (intros q0 H0; apply Hw; rewrite Ew; right; exact H0).
      assert (Hx : e_own (resend_elem a q) = false -> r_pers (e_rec (resend_elem a q)) = false)
        by (intros Ho; apply Hw; [rewrite Ew; left; reflexivity | exact Ho]).
      (* the pop: a tracked second send leaves the work and is offered *)
      assert (Kpop : forall j', keeps_with (if p_own q then [] else [r_id (p_rec q)]) (with_work rest j) j' -> keeps j j').
      { intros j' (ops & T & K). exists ops. split; [exact T |]. intros id H. apply K. unfold tracked, pend_ids in H |- *.
        rewrite Ew in H. cbn [with_work j_view j_work filter] in H |- *. destruct (p_own q); cbn [negb map] in H |- *.
        - rewrite app_nil_r. exact H.
        - apply in_app_or in H as [H | [<- | H]]; apply in_or_app; [left; apply in_or_app; left; exact H | right; left; reflexivity |].
          left. apply in_or_app. right. exact H. }
      destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E.
      + split; [| apply work_np_offer; [exact Hx | exact Hw0]].
        apply Kpop. destruct (p_own q) eqn:Eo.
        * apply keeps_with_nil. apply keeps_offer.
        * cbn [offer]. destruct (offer1 own (with_work rest j) (resend_elem a q)) as [j1 b] eqn:E1.
          assert (Hq : r_pers (e_rec (resend_elem a q)) = false) by (apply Hx; exact Eo).
          pose proof (offer1_np_stops own (with_work rest j) (resend_elem a q) Hq) as Hb. rewrite E1 in Hb. cbn [snd] in Hb. subst b.
          pose proof (keeps_offer1 own (with_work rest j) (resend_elem a q) Hq Eo) as K1. rewrite E1 in K1. exact K1.
      + split.
        * apply Kpop. destruct (p_own q); [exact (keeps_with_less _ _ _ (keeps_store own (with_work rest j) (resend_elem a q))) |].
          exact (keeps_store own (with_work rest j) (resend_elem a q)).
        * destruct (store_spec own (with_work rest j) (resend_elem a q)) as (_ & _ & _ & _ & _ & Wk & _).
          intros q0 H0. rewrite Wk in H0. exact (Hw0 q0 H0).
  Qed.

  Lemma drain_keeps : forall fuel own a j, work_np j -> keeps j (drain fuel own a j) /\ work_np (drain fuel own a j).
  Proof.
    induction fuel as [| f IH]; intros own a j Hw; cbn [drain]; [split; [apply keeps_same; reflexivity | exact Hw] |].
    destruct (dstep_keeps own a j Hw) as [K1 W1]. destruct (dstep own a j) as [j' []]; cbn [fst] in K1, W1.
    - destruct (IH own a j' W1) as [K2 W2]. split; [exact (keeps_trans _ _ _ K1 K2) | exact W2].
    - split; [exact K1 | exact W1].
  Qed.

  (* Headline 16 (decision 45 Rule E, decision 30, plan 4.8, DV-3). (1) A
     peek fire pushes its second sends in receipt (group) order on top of
     the work. (2) A consume-side peek fire sends again its bound records
     that are not persistent. (3) A produce-side peek fire sends again the
     element in flight, whether or not it is persistent, and each other
     bound record that is not persistent. (4) A peek fire of the cascade
     sends again the bound records that are not persistent: the re-offered
     record is in flight only when it was lifted. (5) Only the element in
     flight of the operation carries the own flag. (6) Every popped second
     send runs OFFER with its own authority (Introduction::for_record), or
     the operation's authority for its own element. (7) No record is lost:
     every stored record and every pending second send of a settle job is,
     at the end of the transaction, stored, pending in the work, or spent by
     a fire of a waiter that does not peek or by a retirement. (8) At a stop
     that owes work, the commit carries the pending second sends into the
     re-issued settle job, and none of them is the element of the operation.
     (9) The element of the operation in the work returns to its operation,
     which re-issues. (10) A stop that owes nothing re-issues its resume
     only (DV-5): nothing when the element settled. *)
  Theorem peek_join_restores_stored_candidates :
    (forall j l, j_work (push_second_sends j l) = l ++ j_work j) /\
    (forall sg oi fl, map p_rec (np_results sg None oi fl) = filter (fun r => negb (r_pers r)) (map fst sg)) /\
    (forall sg p oi,
       map p_rec (np_results sg (Some p) oi true) =
         map (fun krm : nat * (rec * M) => fst (snd krm))
             (filter (fun krm : nat * (rec * M) => (fst krm =? p) || negb (r_pers (fst (snd krm)))) (enum 0 sg))) /\
    (forall sg vp rv mv oi, nth_error sg vp = Some (rv, mv) ->
       map p_rec (np_results sg (Some vp) oi (negb (r_pers rv))) = filter (fun r => negb (r_pers r)) (map fst sg)) /\
    (forall sg inc oi fl q, In q (np_results sg inc oi fl) -> p_own q = true ->
       oi = true /\ exists p m, inc = Some p /\ nth_error sg p = Some (p_rec q, m)) /\
    (forall own a j q rest, j_stop j = None -> j_work j = q :: rest -> j_fired j < cap ->
       dstep own a j = (offer (S (length (v_wtrs (j_view j)))) own (with_work rest j) (resend_elem a q), true) /\
       e_rec (resend_elem a q) = p_rec q /\
       e_auth (resend_elem a q) = (if p_own q then a else own_auth (p_rec q))) /\
    (forall fuel o scope v0 id,
       (forall q, In q (pending_of o) -> r_pers (p_rec q) = false) ->
       In id (map r_id (v_recs v0) ++ map (fun q => r_id (p_rec q)) (pending_of o)) ->
       In id (tracked (fst (run_join fuel o scope v0))) \/ In id (spent_ids (j_trace (fst (run_join fuel o scope v0))))) /\
    (forall o v0 j, j_stop j <> None -> pending_work j <> [] ->
       commit o v0 j = Some (OSettle (key_of o) (if cascades then remove Nat.eq_dec (key_of o) (j_dirty j) else [])
                                     (pending_work j) (resume_of o v0 j) (auth_of o)) /\
       (forall q, In q (pending_work j) -> p_own q = false)) /\
    (forall o v0 j, is_settle o = false -> existsb p_own (j_work j) = true -> resume_of o v0 j = reissue_op o v0) /\
    (forall o v0 j, j_stop j <> None -> owes j = false -> commit o v0 j = resume_of o v0 j).
  Proof.
    split; [| split; [| split; [| split; [| split; [| split; [| split; [| split; [| split]]]]]]]].
    - intros j l. unfold push_second_sends, with_work. cbn [j_work]. rewrite fold_push, rev_involutive. reflexivity.
    - exact np_results_none_recs.
    - intros sg p oi. unfold np_results. rewrite map_map. cbn [p_rec].
      f_equal; apply filter_ext; intros [k [e m]]; unfold sends_at; cbn [fst snd is_inc]; destruct (k =? p); reflexivity.
    - exact np_results_lift_recs.
    - intros sg inc oi fl q H Ho.
      destruct (np_results_spec _ _ _ _ _ H) as [_ [[_ Ho'] | (_ & Hoi & p & m & Hi & Hn)]]; [rewrite Ho in Ho'; discriminate |].
      split; [rewrite <- Hoi; exact Ho | exists p, m; split; assumption].
    - intros own a j q rest Hs Hw Hc. split; [| split; reflexivity].
      unfold dstep. rewrite Hs, Hw. replace (cap <=? j_fired j) with false by (symmetry; apply Nat.leb_gt; exact Hc).
      unfold resend. reflexivity.
    - intros fuel o scope v0 id Hpd Hid. unfold run_join.
      set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
      assert (K1 : forall id0, In id0 (map r_id (v_recs v0) ++ map (fun q => r_id (p_rec q)) (pending_of o)) -> In id0 (tracked j1)).
      { intros id0 H0. unfold tracked, pend_ids, j1. cbn [with_work j_view j_work].
        destruct (touch_all_spec (own_of o) (own_of o) (init v0)) as (bs & _ & Vw & _).
        rewrite Vw, (proj1 (bumps_view bs (j_view (init v0)))).
        assert (Hf : filter (fun q => negb (p_own q)) (pending_of o) = pending_of o).
        { apply forallb_filter_id. apply forallb_forall. intros q Hq.
          destruct o as [x c a | W c a | key sd pd rs a]; cbn [pending_of] in Hq; [destruct Hq | destruct Hq |].
          apply in_map_iff in Hq as (q0 & <- & _). reflexivity. }
        rewrite Hf. exact H0. }
      assert (Hs1 : spent_ids (j_trace j1) = []).
      { unfold j1. cbn [with_work j_trace]. destruct (touch_all_spec (own_of o) (own_of o) (init v0)) as (bs & T & _).
        rewrite T. cbn [init j_trace app]. clear. induction bs as [| c r IH]; [reflexivity | exact IH]. }
      assert (Wn1 : work_np j1) by (intros q Hq _; unfold j1 in Hq; cbn [with_work j_work] in Hq; exact (Hpd q Hq)).
      assert (Hend : forall jf, keeps j1 jf -> In id (tracked jf) \/ In id (spent_ids (j_trace jf))).
      { intros jf (ops & T & K). destruct (K id (K1 id Hid)) as [H | H]; [left; exact H |].
        right. rewrite T, spent_ids_app, Hs1. exact H. }
      assert (Kd : keeps j1 (drain fuel (own_of o) (auth_of o) (with_dirty scope j1)) /\
                   work_np (drain fuel (own_of o) (auth_of o) (with_dirty scope j1))).
      { destruct (drain_keeps fuel (own_of o) (auth_of o) (with_dirty scope j1) Wn1) as [K W].
        split; [exact (keeps_trans _ _ _ (keeps_same _ _ eq_refl eq_refl eq_refl) K) | exact W]. }
      destruct (is_settle o); [cbn [fst]; apply Hend; exact (proj1 Kd) |].
      set (j3 := if cascades then drain fuel (own_of o) (auth_of o) (with_dirty scope j1) else j1).
      assert (K3 : keeps j1 j3 /\ work_np j3).
      { unfold j3. destruct cascades; [exact Kd | split; [apply keeps_same; reflexivity | exact Wn1]]. }
      destruct K3 as [K3 W3].
      destruct (j_stop j3); [cbn [fst]; apply Hend; exact K3 |].
      destruct (plan_at_cap (j_fired (with_dirty [] j3)));
        [cbn [fst]; apply Hend; exact (keeps_ctl _ _ K3 _ eq_refl eq_refl eq_refl) |].
      cbn [fst]. apply Hend.
      set (j4 := with_fired (phase_count (j_fired (with_dirty [] j3))) (with_dirty [] j3)).
      assert (K4 : keeps j1 j4 /\ work_np j4) by (split; [exact (keeps_ctl _ _ K3 _ eq_refl eq_refl eq_refl) | exact W3]).
      destruct K4 as [K4 W4].
      assert (K5 : keeps j1 (plan fuel o v0 j4) /\ work_np (plan fuel o v0 j4)).
      { destruct o as [x c a | W c a | key sd pd rs a]; cbn [plan].
        - split; [exact (keeps_trans _ _ _ K4 (keeps_offer _ _ _ _)) | apply work_np_offer; [intros E; discriminate | exact W4]].
        - split; [exact (keeps_trans _ _ _ K4 (keeps_receive _ _ _)) | apply work_np_receive; exact W4].
        - split; [exact K4 | exact W4]. }
      destruct K5 as [K5 W5].
      destruct (drain_keeps fuel (own_of o) (auth_of o) (with_fired (phase_count (j_fired (plan fuel o v0 j4))) (plan fuel o v0 j4)) W5)
        as [K6 _].
      exact (keeps_trans _ _ _ (keeps_ctl _ _ K5 _ eq_refl eq_refl eq_refl) K6).
    - intros o v0 j Hs Hp. split.
      + unfold commit. destruct (j_stop j) as [c |]; [| exfalso; exact (Hs eq_refl)].
        unfold owes. destruct (pending_work j) as [| q rest] eqn:Ep; [exfalso; exact (Hp eq_refl) |].
        cbn [is_nil negb orb]. reflexivity.
      + intros q Hq. unfold pending_work in Hq. apply filter_In in Hq as [_ Ho]. apply negb_true_iff in Ho. exact Ho.
    - intros o v0 j Hns He. unfold resume_of.
      destruct o as [x c a | W c a | key sd pd rs a]; [rewrite He; reflexivity | rewrite He; reflexivity | discriminate Hns].
    - intros o v0 j Hs Ho. unfold commit. destruct (j_stop j) as [c |]; [| exfalso; exact (Hs eq_refl)].
      rewrite Ho. reflexivity.
  Qed.

  (* ── 34. Headline 14: plan, then lock ──────────────────────────────── *)

  Definition closed_set (v : view) (comp : list nat) : Prop := forall c, In c comp -> incl (partners c v) comp.

  Lemma partners_universe : forall v seeds c, incl (partners c v) (universe v seeds).
  Proof.
    intros v seeds c a Ha. unfold partners in Ha. apply in_flat_map in Ha as (W & Hw & Ha).
    unfold waiters_of in Hw. apply filter_In in Hw as [Hw _]. unfold universe. apply nodup_In. apply in_or_app. right.
    apply in_flat_map. exists W. split; assumption.
  Qed.

  Lemma close_step_spec :
    forall v comp,
      incl comp (close_step v comp) /\ NoDup (close_step v comp) /\
      (forall c, In c (close_step v comp) <-> In c comp \/ exists c', In c' comp /\ In c (partners c' v)).
  Proof.
    intros v comp. unfold close_step. split; [| split].
    - intros a Ha. apply nodup_In. apply in_or_app. left. exact Ha.
    - apply NoDup_nodup.
    - intros c. rewrite nodup_In. split.
      + intros H. apply in_app_or in H as [H | H]; [left; exact H |]. right. apply in_flat_map in H. exact H.
      + intros [H | H]; apply in_or_app; [left; exact H | right; apply in_flat_map; exact H].
  Qed.

  Lemma close_iter_closed :
    forall n v comp, closed_set v comp -> closed_set v (close_iter n v comp) /\ (forall c, In c (close_iter n v comp) <-> In c comp).
  Proof.
    induction n as [| n IH]; intros v comp H; [split; [exact H | intros c; reflexivity] |]. cbn [close_iter].
    destruct (close_step_spec v comp) as (_ & _ & Hs).
    assert (Heq : forall c, In c (close_step v comp) <-> In c comp).
    { intros c. rewrite Hs. split; [intros [H1 | (c' & Hc' & Hp)]; [exact H1 | exact (H c' Hc' c Hp)] | intros H1; left; exact H1]. }
    assert (Hc : closed_set v (close_step v comp)).
    { intros c Hc a Ha. apply Heq. apply Heq in Hc. exact (H c Hc a Ha). }
    destruct (IH v (close_step v comp) Hc) as [H1 H2]. split; [exact H1 |]. intros c. rewrite H2. apply Heq.
  Qed.

  Lemma forallb_false_exists :
    forall (X : Type) (f : X -> bool) l, forallb f l = false -> exists x, In x l /\ f x = false.
  Proof.
    intros X f l. induction l as [| x r IH]; intros H; [discriminate |]. cbn [forallb] in H.
    destruct (f x) eqn:E; [destruct (IH H) as (y & Hy & Ey); exists y; split; [right; exact Hy | exact Ey] |].
    exists x. split; [left; reflexivity | exact E].
  Qed.

  (* Closedness is decidable. *)
  Lemma closed_dec :
    forall v comp, closed_set v comp \/ exists c p, In c comp /\ In p (partners c v) /\ ~ In p comp.
  Proof.
    intros v comp.
    destruct (forallb (fun c => forallb (fun p => mem p comp) (partners c v)) comp) eqn:E.
    - left. intros c Hc p Hp. rewrite forallb_forall in E. specialize (E c Hc). rewrite forallb_forall in E.
      apply mem_spec. exact (E p Hp).
    - right. apply forallb_false_exists in E as (c & Hc & E). apply forallb_false_exists in E as (p & Hp & E).
      exists c, p. split; [exact Hc | split; [exact Hp | apply mem_false; exact E]].
  Qed.

  Lemma close_iter_props :
    forall n v comp U,
      NoDup comp -> incl comp U -> (forall c, incl (partners c v) U) ->
      NoDup (close_iter n v comp) /\ incl (close_iter n v comp) U /\ incl comp (close_iter n v comp) /\
      (closed_set v (close_iter n v comp) \/ length comp + n <= length (close_iter n v comp)).
  Proof.
    induction n as [| n IH]; intros v comp U Hnd Hi Hp.
    - cbn [close_iter]. split; [exact Hnd | split; [exact Hi | split; [apply incl_refl | right; lia]]].
    - cbn [close_iter]. destruct (close_step_spec v comp) as (Hinc & Hnd' & Hs).
      assert (Hi' : incl (close_step v comp) U).
      { intros a Ha. apply Hs in Ha as [Ha | (c' & _ & Hpa)]; [exact (Hi a Ha) | exact (Hp c' a Hpa)]. }
      destruct (IH v (close_step v comp) U Hnd' Hi' Hp) as (N1 & I1 & C1 & D1).
      split; [exact N1 |]. split; [exact I1 |]. split; [exact (incl_tran Hinc C1) |].
      destruct (closed_dec v comp) as [Hcl | (c & p & Hc & Hpp & Hnot)].
      + left. destruct (close_iter_closed n v (close_step v comp)) as [H1 _]; [| exact H1].
        intros c Hc a Ha. apply Hs in Hc as [Hc | (c' & Hc' & Hpc)].
        * apply Hs. left. exact (Hcl c Hc a Ha).
        * apply Hs. left. apply (Hcl c' Hc') in Hpc. exact (Hcl c Hpc a Ha).
      + destruct D1 as [D1 | D1]; [left; exact D1 | right].
        assert (Hlt : length comp < length (close_step v comp)).
        { assert (Hsub : incl (p :: comp) (close_step v comp)).
          { intros a [<- | Ha]; [apply Hs; right; exists c; split; assumption | exact (Hinc a Ha)]. }
          pose proof (NoDup_incl_length (NoDup_cons p Hnot Hnd) Hsub) as Hl. cbn [length] in Hl. lia. }
        lia.
  Qed.

  Lemma close_iter_nil : forall n v, close_iter n v [] = [].
  Proof. induction n as [| n IH]; intros v; [reflexivity | cbn [close_iter]; unfold close_step; cbn; exact (IH v)]. Qed.

  (* The closure is closed when its depth reaches the size of the universe. *)
  Lemma closure_closed_gen :
    (forall v seeds, length (universe v seeds) <= closure_depth v seeds) ->
    forall v seeds, closed_set v (closure v seeds) /\ incl seeds (closure v seeds).
  Proof.
    intros Hd v seeds. unfold closure.
    set (U := universe v seeds).
    assert (Hs : incl (nodup Nat.eq_dec seeds) U).
    { intros a Ha. apply nodup_In in Ha. unfold U, universe. apply nodup_In. apply in_or_app. left. exact Ha. }
    destruct (close_iter_props (closure_depth v seeds) v (nodup Nat.eq_dec seeds) U (NoDup_nodup _ _) Hs (partners_universe v seeds))
      as (N & I & C & D).
    split; [| intros a Ha; apply C; apply nodup_In; exact Ha].
    destruct D as [D | D]; [exact D |].
    pose proof (NoDup_incl_length N I) as L. specialize (Hd v seeds). fold U in Hd.
    assert (E : nodup Nat.eq_dec seeds = []).
    { destruct (nodup Nat.eq_dec seeds) as [| a r]; [reflexivity | cbn [length] in D; lia]. }
    rewrite E, close_iter_nil. intros c [].
  Qed.

  Lemma fire_dirty_sub : forall W c, incl (fire_dirty W c) (c :: colls W).
  Proof. intros W c a Ha. unfold fire_dirty in Ha. cbn [In] in *. tauto. Qed.

  (* The coverage of a transaction: the waiters of the view that touch the
     scope stay inside it, and the dirty collections, the pending second
     sends and every staging lie in the scope. *)
  Definition Cov (scope : list nat) (j : jst) : Prop :=
    (forall W, In W (v_wtrs (j_view j)) -> (exists c, In c (colls W) /\ In c scope) -> incl (colls W) scope) /\
    incl (j_dirty j) scope /\
    (forall q, In q (j_work j) -> In (r_coll (p_rec q)) scope) /\
    (forall op0, In op0 (j_trace j) -> incl (staged_colls op0) scope).

  Lemma Cov_step :
    forall scope j j' ops,
      Cov scope j -> j_trace j' = j_trace j ++ ops -> (forall op0, In op0 ops -> incl (staged_colls op0) scope) ->
      (forall W, In W (v_wtrs (j_view j')) -> In W (v_wtrs (j_view j)) \/ incl (colls W) scope) ->
      incl (j_dirty j') scope -> (forall q, In q (j_work j') -> In (r_coll (p_rec q)) scope) ->
      Cov scope j'.
  Proof.
    intros scope j j' ops (Hw & Hd & Hq & Ht) T Hops Hw' Hd' Hq'. split; [| split; [exact Hd' | split; [exact Hq' |]]].
    - intros W HW Hc. destruct (Hw' W HW) as [H | H]; [exact (Hw W H Hc) | exact H].
    - intros op0 Hin. rewrite T in Hin. apply in_app_or in Hin as [Hin | Hin]; [exact (Ht op0 Hin) | exact (Hops op0 Hin)].
  Qed.

  Lemma Cov_touch_all : forall scope own cs j, Cov scope j -> incl cs scope -> Cov scope (touch_all own j cs).
  Proof.
    intros scope own cs j H Hcs. destruct (touch_all_spec own cs j) as (bs & T & Vw & _ & D & Wk & _ & _ & _ & _ & Hbs & _).
    apply (Cov_step scope j _ (map SBump bs) H T).
    - intros op0 Hin. apply in_map_iff in Hin as (c & <- & Hc). intros a [<- | []]. exact (Hcs c (proj1 (Hbs c Hc))).
    - intros W HW. left. rewrite Vw, (proj2 (bumps_view bs (j_view j))) in HW. exact HW.
    - rewrite D. exact (proj1 (proj2 H)).
    - intros q Hq. rewrite Wk in Hq. exact (proj1 (proj2 (proj2 H)) q Hq).
  Qed.

  Lemma Cov_ctl :
    forall scope j j', Cov scope j -> j_trace j' = j_trace j -> v_wtrs (j_view j') = v_wtrs (j_view j) ->
      incl (j_dirty j') scope -> j_work j' = j_work j -> Cov scope j'.
  Proof.
    intros scope j j' H T Vw D Wk. apply (Cov_step scope j j' [] H); [rewrite T, app_nil_r; reflexivity | intros op0 [] | | exact D |].
    - intros W HW. left. rewrite <- Vw. exact HW.
    - intros q Hq. rewrite Wk in Hq. exact (proj1 (proj2 (proj2 H)) q Hq).
  Qed.

  Lemma Cov_store : forall scope own j x, Cov scope j -> In (r_coll (e_rec x)) scope -> Cov scope (store own j x).
  Proof.
    intros scope own j x H Hx. destruct (store_spec own j x) as (bs & T & Vw & _ & D & Wk & _ & _ & _ & _ & Hbs & _).
    apply (Cov_step scope j _ _ H T).
    - intros op0 Hin. apply in_app_or in Hin as [Hin | [<- | []]].
      + apply in_map_iff in Hin as (c & <- & Hc). intros a [<- | []]. rewrite (proj1 (Hbs c Hc)). exact Hx.
      + intros a [<- | []]. exact Hx.
    - intros W HW. left. rewrite Vw, apply_all_app in HW. cbn [apply_all fold_left apply v_wtrs] in HW.
      rewrite (proj2 (bumps_view bs (j_view j))) in HW. exact HW.
    - rewrite D. intros a [<- | Ha]; [exact Hx | exact (proj1 (proj2 H) a Ha)].
    - intros q Hq. rewrite Wk in Hq. exact (proj1 (proj2 (proj2 H)) q Hq).
  Qed.

  (* The fire of W in a covered transaction: the group of W lies in the scope,
     and so do its bound records, its retirements and its second sends. *)
  Lemma Cov_fire_step :
    forall scope j bs W sg ainc oi fl j3,
      Cov scope j -> incl (colls W) scope -> (forall e, In e (map fst sg) -> In (r_coll e) scope) ->
      (forall c, In c bs -> In c scope) ->
      j_trace j3 = j_trace j ++ map SBump bs ++ [SFire W sg ainc] ->
      v_wtrs (j_view j3) = (if w_pers W then v_wtrs (j_view j) else del_wtr (w_id W) (v_wtrs (j_view j))) ->
      incl (j_dirty j3) scope -> j_work j3 = j_work j ->
      Cov scope (fire_tail W sg (option_map fst ainc) oi fl j3).
  Proof.
    intros scope j bs W sg ainc oi fl j3 H HW Hsg Hbs T3 V3 D3 W3.
    destruct (fire_tail_spec W sg (option_map fst ainc) oi fl j3) as (rs & T4 & V4 & _ & D4 & P4 & _ & _ & _ & Hrs & _).
    apply (Cov_step scope j _ (map SBump bs ++ [SFire W sg ainc] ++ map SRetire rs) H).
    - rewrite T4, T3, !app_assoc. reflexivity.
    - intros op0 Hin. apply in_app_or in Hin as [Hin | Hin].
      + apply in_map_iff in Hin as (c & <- & Hc). intros a [<- | []]. exact (Hbs c Hc).
      + apply in_app_or in Hin as [[<- | []] | Hin]; [exact HW |].
        apply in_map_iff in Hin as (e & <- & He). intros a [<- | []].
        destruct (Hrs e He) as (_ & _ & k & e0 & m0 & Hk & Hc). rewrite Hc. apply Hsg. apply in_map_iff.
        exists (e0, m0). split; [reflexivity | exact (nth_error_In _ _ Hk)].
    - intros W' HW'. rewrite V4 in HW'. cbn [v_wtrs] in HW'. rewrite V3 in HW'. left.
      destruct (w_pers W); [exact HW' | exact (del_wtr_incl _ _ W' HW')].
    - rewrite D4. exact D3.
    - intros q Hq. apply (Permutation_in _ P4) in Hq. apply in_app_or in Hq as [Hq | Hq].
      + destruct (w_peek W); [| destruct Hq]. apply np_results_spec in Hq as [Hq _]. exact (Hsg _ Hq).
      + rewrite W3 in Hq. exact (proj1 (proj2 (proj2 H)) q Hq).
  Qed.

  (* The tail of a fire stays in the scope: the retired members and the
     second sends are records of the collections of the fire. *)
  Lemma Cov_fire_tail :
    forall scope W sg inc oi fl j, Cov scope j -> (forall e, In e (map fst sg) -> In (r_coll e) scope) ->
      Cov scope (fire_tail W sg inc oi fl j).
  Proof.
    intros scope W sg inc oi fl j H Hsg.
    destruct (fire_tail_spec W sg inc oi fl j) as (rs & T & Vf & _ & D & Pf & _ & _ & _ & Hrs & _).
    apply (Cov_step scope j _ (map SRetire rs) H T).
    - intros op0 Hin. apply in_map_iff in Hin as (e & <- & He). intros a [<- | []].
      destruct (Hrs e He) as (_ & _ & k & e0 & m0 & Hk & Hc). rewrite Hc. apply Hsg. apply in_map_iff.
      exists (e0, m0). split; [reflexivity | exact (nth_error_In _ _ Hk)].
    - intros W' HW'. left. rewrite Vf in HW'. exact HW'.
    - rewrite D. exact (proj1 (proj2 H)).
    - intros q Hq. apply (Permutation_in _ Pf) in Hq. apply in_app_or in Hq as [Hq | Hq].
      + destruct (w_peek W); [| destruct Hq]. apply np_results_spec in Hq as [Hq _]. exact (Hsg _ Hq).
      + exact (proj1 (proj2 (proj2 H)) q Hq).
  Qed.

  Lemma slots_prod_colls :
    forall W x p v sg, slots_rel (slots_prod W x p v) sg ->
      forall e, In e (map fst sg) -> e = x \/ (In e (v_recs v) /\ In (r_coll e) (colls W)).
  Proof.
    intros W x p v sg Hr e He. apply in_map_iff in He as ([e0 m0] & <- & Hin). apply In_nth_error in Hin as [i Hi].
    cbn [fst].
    assert (Hlen : i < length (slots_prod W x p v)) by (rewrite <- (slots_rel_length _ _ Hr); apply nth_error_Some; rewrite Hi; discriminate).
    destruct (nth_error (slots_prod W x p v) i) as [s |] eqn:Es; [| apply nth_error_None in Es; lia].
    pose proof (slots_rel_nth _ _ _ _ _ Hr Es Hi) as Hm. unfold slots_prod in Es. rewrite nth_error_map in Es.
    destruct (nth_error (enum 0 (gpairs W)) i) as [[k [i0 b]] |] eqn:Ee; [| discriminate]. cbn [option_map] in Es.
    injection Es as <-. cbn [fst snd] in Hm. destruct (forced W x p k b); [left; injection Hm as -> _; reflexivity |].
    cbn [fst] in Hm. apply not_x_incl, visible_incl in Hm.
    destruct (win_in_mem W x (b_coll b) v e0 Hm) as [[-> _] | [Hv Hc]]; [left; reflexivity | right; split; [exact Hv |]].
    rewrite Hc. apply bind_coll. apply nth_error_In in Ee. apply (enum_nth _ _ 0) in Ee as [_ Ee]. rewrite Nat.sub_0_r in Ee.
    exact (gpairs_binds W (i0, b) (nth_error_In _ _ Ee)).
  Qed.

  Lemma offer_pick_in :
    forall x v W p sg, offer_pick x v (offer_cands x v) = Some (W, p, sg) -> In W (waiters_of (r_coll x) v).
  Proof.
    intros x v W p sg H.
    assert (G : forall ws, incl ws (waiters_of (r_coll x) v) -> offer_pick x v ws = Some (W, p, sg) -> In W (waiters_of (r_coll x) v)).
    { induction ws as [| W0 r IH]; intros Hi Hp; [discriminate |]. cbn [offer_pick] in Hp.
      destruct (fires_with W0 x v) as [[p0 sg0] |]; [injection Hp as <- _ _; exact (Hi W0 (or_introl eq_refl)) |].
      apply IH; [intros a Ha; exact (Hi a (or_intror Ha)) | exact Hp]. }
    apply (G (offer_cands x v)); [| exact H]. intros a Ha. unfold offer_cands in Ha.
    assert (Ha' : In a (worder (r_coll x) (waiters_of (r_coll x) v))) by (destruct sh; try exact Ha; apply filter_In in Ha as [Ha _]; exact Ha).
    exact (Permutation_in _ (worder_perm _ _) Ha').
  Qed.

  Lemma waiters_of_coll : forall c v W, In W (waiters_of c v) -> In W (v_wtrs v) /\ In c (colls W).
  Proof. intros c v W H. unfold waiters_of in H. apply filter_In in H as [H Hc]. apply mem_spec in Hc. split; assumption. Qed.

  Lemma Cov_offer1 :
    forall scope own j x, Cov scope j -> In (r_coll (e_rec x)) scope -> Cov scope (fst (offer1 own j x)).
  Proof.
    intros scope own j x H Hx. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep;
      [| exact (Cov_store scope own j x H Hx)].
    destruct (waiters_of_coll _ _ _ (offer_pick_in _ _ _ _ _ Ep)) as [HwIn Hcx].
    assert (HW : incl (colls W) scope) by exact (proj1 H W HwIn (ex_intro _ (r_coll (e_rec x)) (conj Hcx Hx))).
    destruct (proj1 join_fires_heads_only _ _ _ _ _ Ep) as (_ & _ & Hrel & _).
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & V1 & _ & D1 & W1 & _ & _ & _ & _ & Hbs & _).
    set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                 (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
    assert (H4 : Cov scope (fire_tail W sg (option_map fst (Some (p, e_auth x))) (e_own x) true j3)).
    { apply (Cov_fire_step scope j bs W sg (Some (p, e_auth x)) (e_own x) true j3 H HW).
      - intros e He. destruct (slots_prod_colls W (e_rec x) p (j_view j) sg Hrel e He) as [-> | [_ Hc]]; [exact Hx | exact (HW _ Hc)].
      - intros c Hc. exact (HW c (proj1 (Hbs c Hc))).
      - unfold j3. cbn [add_dirty with_dirty with_fired stage j_trace]. rewrite T1, app_assoc. reflexivity.
      - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs]. rewrite V1, (proj2 (bumps_view bs (j_view j))).
        reflexivity.
      - unfold j3. cbn [add_dirty with_dirty with_fired stage j_dirty]. rewrite D1. intros a Ha. apply in_app_or in Ha as [Ha | Ha].
        + apply fire_dirty_sub in Ha as [<- | Ha]; [exact Hx | exact (HW a Ha)].
        + exact (proj1 (proj2 H) a Ha).
      - unfold j3. cbn [add_dirty with_dirty with_fired stage j_work]. exact W1. }
    cbn [option_map fst] in H4.
    destruct (w_peek W); [exact H4 |].
    destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
      cbn [fst]; unfold own_flight; try destruct (e_own x); (apply (Cov_ctl scope _ _ H4); try reflexivity; exact (proj1 (proj2 H4))).
  Qed.

  Lemma Cov_offer : forall scope fuel own j x, Cov scope j -> In (r_coll (e_rec x)) scope -> Cov scope (offer fuel own j x).
  Proof.
    intros scope fuel. induction fuel as [| f IH]; intros own j x H Hx; cbn [offer].
    - unfold own_flight. destruct (e_own x); (apply (Cov_ctl scope _ _ H); try reflexivity; exact (proj1 (proj2 H))).
    - pose proof (Cov_offer1 scope own j x H Hx) as H1. destruct (offer1 own j x) as [j' []]; cbn [fst] in H1;
        [exact (IH own j' x H1 Hx) | exact H1].
  Qed.

  Lemma colls_nonempty : forall W, w_binds W <> [] -> exists c, In c (colls W).
  Proof.
    intros W H. destruct (w_binds W) as [| b r] eqn:E; [contradiction |]. exists (b_coll b).
    apply bind_coll. rewrite E. left. reflexivity.
  Qed.

  Lemma hd_colls : forall W, w_binds W <> [] -> In (hd 0 (colls W)) (colls W).
  Proof.
    intros W H. destruct (colls_nonempty W H) as [c Hc]. destruct (colls W) as [| a r]; [destruct Hc | left; reflexivity].
  Qed.

  Lemma Cov_receive1 :
    forall scope j W, Cov scope j -> incl (colls W) scope -> w_binds W <> [] -> Cov scope (fst (receive1 j W)).
  Proof.
    intros scope j W H HW Hb. unfold receive1.
    destruct (receive_sel (j_view j) W) as [sg |] eqn:Er.
    - set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
      assert (Hgr : greedy W (j_view j) = Some sg).
      { unfold receive_sel in Er. destruct (heads_ok (colls W) W (park_view W (j_view j))); [| discriminate].
        unfold receive_decision in Er. destruct (greedy W (j_view j)) as [sg' |]; [| discriminate].
        destruct (guard_ok W sg'); [injection Er as <-; reflexivity | discriminate]. }
      assert (H4 : Cov scope (fire_tail W sg (option_map fst (@None (nat * A))) false false j3)).
      { apply (Cov_fire_step scope j [] W sg None false false j3 H HW).
        - intros e He. exact (HW _ (proj2 (greedy_colls W (j_view j) sg Hgr e He))).
        - intros c [].
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_trace map app]. reflexivity.
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs]. reflexivity.
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_dirty]. intros a Ha. apply in_app_or in Ha as [Ha | Ha].
          + apply fire_dirty_sub in Ha as [<- | Ha]; [exact (HW _ (hd_colls W Hb)) | exact (HW a Ha)].
          + exact (proj1 (proj2 H) a Ha).
        - reflexivity. }
      cbn [option_map] in H4.
      destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)));
        cbn [fst]; (apply (Cov_ctl scope _ _ H4); try reflexivity; exact (proj1 (proj2 H4))).
    - cbn [fst]. unfold park. apply (Cov_step scope j _ [SPark W] H); [reflexivity | | | |].
      + intros op0 [<- | []]. exact HW.
      + intros W' [<- | HW']; [right; exact HW | left; exact HW'].
      + exact (proj1 (proj2 H)).
      + exact (proj1 (proj2 (proj2 H))).
  Qed.

  Lemma Cov_receive :
    forall scope fuel j W, Cov scope j -> incl (colls W) scope -> w_binds W <> [] -> Cov scope (receive fuel j W).
  Proof.
    intros scope fuel. induction fuel as [| f IH]; intros j W H HW Hb; cbn [receive].
    - apply (Cov_ctl scope _ _ H); try reflexivity. exact (proj1 (proj2 H)).
    - pose proof (Cov_receive1 scope j W H HW Hb) as H1. destruct (receive1 j W) as [j' []]; cbn [fst] in H1;
        [exact (IH j' W H1 HW Hb) | exact H1].
  Qed.

  Lemma Cov_lift_reoffer :
    forall scope own j W sg, Cov scope j -> incl (colls W) scope -> (forall e, In e (map fst sg) -> In (r_coll e) (colls W)) ->
      Cov scope (lift_reoffer own j W sg).
  Proof.
    intros scope own j W sg H HW Hsg. unfold lift_reoffer.
    destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn; [| exact H].
    assert (Hrv : In (r_coll rv) (colls W)) by (apply Hsg; apply in_map_iff; exists (rv, mv); split; [reflexivity | exact (nth_error_In _ _ Hn)]).
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & V1 & _ & D1 & W1 & _ & _ & _ & _ & Hbs & _).
    destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (T2 & V2 & _ & D2 & W2 & _).
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                 (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    assert (H4 : Cov scope j4).
    { apply (Cov_step scope j j4 (map SBump bs ++ lift_ops rv ++ [SFire W sg (Some (reoffer_pos sg, own_auth rv))]) H).
      - unfold j4. cbn [add_dirty with_dirty with_fired stage j_trace]. rewrite T2, T1, <- !app_assoc. reflexivity.
      - intros op0 Hin. apply in_app_or in Hin as [Hin | Hin].
        + apply in_map_iff in Hin as (c & <- & Hc). intros a [<- | []]. exact (HW c (proj1 (Hbs c Hc))).
        + apply in_app_or in Hin as [Hin | [<- | []]]; [| exact HW].
          rewrite (lift_ops_shape _ _ Hin). intros a [<- | []]. exact (HW _ Hrv).
      - intros W' HW'. left. unfold j4 in HW'. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs] in HW'.
        rewrite V2, V1 in HW'.
        assert (Hl : forall v0, v_wtrs (apply_all v0 (lift_ops rv)) = v_wtrs v0) by (intros v0; unfold lift_ops; destruct (r_pers rv); reflexivity).
        rewrite Hl, (proj2 (bumps_view bs (j_view j))) in HW'. destruct (w_pers W); [exact HW' | exact (del_wtr_incl _ _ W' HW')].
      - unfold j4. cbn [add_dirty with_dirty with_fired stage j_dirty]. rewrite D2, D1. intros a Ha. apply in_app_or in Ha as [Ha | Ha].
        + apply fire_dirty_sub in Ha as [<- | Ha]; [exact (HW _ Hrv) | exact (HW a Ha)].
        + exact (proj1 (proj2 H) a Ha).
      - intros q Hq. unfold j4 in Hq. cbn [add_dirty with_dirty with_fired stage j_work] in Hq. rewrite W2, W1 in Hq.
        exact (proj1 (proj2 (proj2 H)) q Hq). }
    assert (H5 : Cov scope (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4))
      by (apply Cov_fire_tail; [exact H4 | intros e He; exact (HW _ (Hsg e He))]).
    destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W));
      try exact H5; (apply (Cov_ctl scope _ _ H5); try reflexivity; exact (proj1 (proj2 H5))).
  Qed.

  Lemma next_fire_dirty : forall d v W sg, next_fire d v = Some (W, sg) -> exists c, In c d /\ In W (waiters_of c v).
  Proof.
    intros d v W sg H. unfold next_fire in H.
    assert (G : forall cs, next_fire_in cs v = Some (W, sg) -> exists c, In c cs /\ In W (heads_of c v)).
    { induction cs as [| c r IH]; intros Hn; [discriminate |]. cbn [next_fire_in] in Hn.
      destruct (find_fire (heads_of c v) v) as [[W0 sg0] |] eqn:Ef.
      - injection Hn as <- <-. exists c. split; [left; reflexivity |].
        assert (G2 : forall ws, find_fire ws v = Some (W0, sg0) -> In W0 ws).
        { induction ws as [| W1 r1 IH1]; intros Hf; [discriminate |]. cbn [find_fire] in Hf.
          destruct (fireable W1 v); [injection Hf as <- _; left; reflexivity | right; exact (IH1 Hf)]. }
        exact (G2 _ Ef).
      - destruct (IH Hn) as (c' & Hc' & Hh). exists c'. split; [right; exact Hc' | exact Hh]. }
    destruct (G _ H) as (c & Hc & Hh). exists c. split.
    - apply (Permutation_in _ (isort_perm _ Nat.leb _)) in Hc. apply nodup_In in Hc. exact Hc.
    - unfold heads_of in Hh. apply filter_In in Hh as [Hh _]. exact (Permutation_in _ (worder_perm _ _) Hh).
  Qed.

  Lemma Cov_dstep : forall scope own a j, Cov scope j -> Cov scope (fst (dstep own a j)).
  Proof.
    intros scope own a j H. unfold dstep. destruct (j_stop j); [exact H |].
    destruct (j_work j) as [| q rest] eqn:Ew.
    - destruct cascades; [| exact H].
      destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |] eqn:En; [| exact H].
      destruct (cap <=? j_fired j); [cbn [fst]; apply (Cov_ctl scope _ _ H); try reflexivity; exact (proj1 (proj2 H)) |].
      cbn [fst]. destruct (next_fire_dirty _ _ _ _ En) as (c & Hc & Hw). destruct (waiters_of_coll _ _ _ Hw) as [HwIn Hcw].
      apply Cov_lift_reoffer; [exact H | | intros e He; exact (proj2 (proj2 (proj2 (next_fire_sel _ _ _ _ En)) e He))].
      exact (proj1 H W HwIn (ex_intro _ c (conj Hcw (proj1 (proj2 H) c Hc)))).
    - destruct (cap <=? j_fired j); [cbn [fst]; apply (Cov_ctl scope _ _ H); try reflexivity; exact (proj1 (proj2 H)) |].
      cbn [fst].
      assert (Hq : In (r_coll (p_rec q)) scope) by (apply (proj1 (proj2 (proj2 H))); rewrite Ew; left; reflexivity).
      assert (H0 : Cov scope (with_work rest j)).
      { apply (Cov_step scope j _ [] H); [rewrite app_nil_r; reflexivity | intros op0 [] | intros W' HW'; left; exact HW' |
          exact (proj1 (proj2 H)) |].
        intros q0 Hq0. apply (proj1 (proj2 (proj2 H))). rewrite Ew. right. exact Hq0. }
      destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E;
        [apply Cov_offer | apply Cov_store]; try exact H0; exact Hq.
  Qed.

  Lemma Cov_drain : forall scope fuel own a j, Cov scope j -> Cov scope (drain fuel own a j).
  Proof.
    intros scope fuel. induction fuel as [| f IH]; intros own a j H; cbn [drain].
    - apply (Cov_ctl scope _ _ H); try reflexivity. exact (proj1 (proj2 H)).
    - pose proof (Cov_dstep scope own a j H) as H1. destruct (dstep own a j) as [j' []]; cbn [fst] in H1;
        [exact (IH own a j' H1) | exact H1].
  Qed.

  (* A receive has at least one bind. *)
  Definition wf_op (o : op) : Prop := match o with OReceive W _ _ => w_binds W <> [] | _ => True end.

  Lemma Cov_run_join :
    forall fuel o scope v0,
      closed_set v0 scope -> incl (seeds_of o ++ own_of o) scope -> wf_op o ->
      Cov scope (fst (run_join fuel o scope v0)).
  Proof.
    intros fuel o scope v0 Hcl Hs Hwf. unfold run_join.
    assert (Hown : incl (own_of o) scope) by (intros c Hc; apply Hs; apply in_or_app; right; exact Hc).
    assert (H0 : Cov scope (init v0)).
    { split; [| split; [intros c [] | split; [intros q [] | intros op0 []]]].
      intros W HW (c & Hc & Hsc). cbn [init j_view] in HW. intros a Ha. apply (Hcl c Hsc).
      unfold partners. apply in_flat_map. exists W. split; [| exact Ha]. unfold waiters_of. apply filter_In.
      split; [exact HW | apply mem_spec; exact Hc]. }
    set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
    assert (H1 : Cov scope j1).
    { pose proof (Cov_touch_all scope (own_of o) (own_of o) (init v0) H0 Hown) as Ht.
      apply (Cov_step scope _ _ [] Ht); [rewrite app_nil_r; reflexivity | intros op0 [] | intros W' HW'; left; exact HW' |
        exact (proj1 (proj2 Ht)) |].
      intros q Hq. cbn [with_work j_work] in Hq. apply Hs. apply in_or_app. left.
      destruct o as [x c a | W c a | key sd pd rs a]; cbn [pending_of] in Hq; try destruct Hq.
      apply in_map_iff in Hq as (q0 & <- & Hq0). cbn [seeds_of p_rec]. right. apply in_or_app. right.
      exact (in_map (fun q1 => r_coll (p_rec q1)) _ _ Hq0). }
    assert (Hd : Cov scope (drain fuel (own_of o) (auth_of o) (with_dirty scope j1))).
    { apply Cov_drain. apply (Cov_ctl scope j1 _ H1); try reflexivity. apply incl_refl. }
    destruct (is_settle o); [cbn [fst]; exact Hd |].
    set (j3 := if cascades then drain fuel (own_of o) (auth_of o) (with_dirty scope j1) else j1).
    assert (H3 : Cov scope j3) by (unfold j3; destruct cascades; [exact Hd | exact H1]).
    destruct (j_stop j3); [cbn [fst]; exact H3 |].
    assert (H4' : Cov scope (with_dirty [] j3)) by (apply (Cov_ctl scope _ _ H3); try reflexivity; intros c []).
    destruct (plan_at_cap (j_fired (with_dirty [] j3))).
    { cbn [fst]. apply (Cov_ctl scope _ _ H4'); try reflexivity. intros c []. }
    cbn [fst]. apply Cov_drain.
    set (j4 := with_fired (phase_count (j_fired (with_dirty [] j3))) (with_dirty [] j3)).
    assert (H4 : Cov scope j4) by (apply (Cov_ctl scope _ _ H4'); try reflexivity; intros c []).
    assert (H5 : Cov scope (plan fuel o v0 j4)).
    { destruct o as [x c a | W c a | key sd pd rs a]; cbn [plan].
      - apply Cov_offer; [exact H4 |]. cbn [e_rec with_ord r_coll]. apply Hown. left. reflexivity.
      - apply Cov_receive; [exact H4 | exact Hown | exact Hwf].
      - exact H4. }
    apply (Cov_ctl scope _ _ H5); try reflexivity. exact (proj1 (proj2 H5)).
  Qed.

  (* The closure contains its seeds and is closed (for the depth of the
     size of the universe). *)
  Lemma join_scope_props :
    (forall v seeds, length (universe v seeds) <= closure_depth v seeds) ->
    forall o v, closed_set v (join_scope o v) /\ incl (seeds_of o ++ own_of o) (join_scope o v).
  Proof. intros Hd o v. unfold join_scope. exact (closure_closed_gen Hd v (seeds_of o ++ own_of o)). Qed.

  Section PlanThenLock.
    (* The states that transaction 1 and transaction 2 read. *)
    Variable v_txn1 v_txn2 : view.
    (* The footprint premise (plan L2): the space job footprint [2]‖sid
       serializes every job of the space, so nothing commits between the two
       transactions. *)
    Hypothesis footprint_serializes : v_txn2 = v_txn1.

    (* Headline 14 (plan 4.1, L1 to L4). Every channel that RUN-JOIN stages
       on lies in the lock keys of the join closure of its operation (L1):
       the closure is closed under the join groups of the state, and the
       transaction stages only on the collections of the groups that it
       fires, parks or touches. With the footprint premise, PREPARE runs the
       plan of the state, once (L2), and its lock check passes. The second
       classification needs no further escalation (L4). Every plan that
       PREPARE returns passed the lock check of the substrate, which fails
       closed otherwise (L3). *)
    Theorem plan_then_lock_stable :
      (forall fuel o v0, wf_op o ->
         forall op0, In op0 (j_trace (fst (run_join fuel o (join_scope o v0) v0))) -> incl (staged_colls op0) (join_scope o v0)) /\
      (forall fuel o, wf_op o -> prepare fuel o v_txn1 v_txn2 = Some (run_join fuel o (join_scope o v_txn1) v_txn1)) /\
      (forall o, covers (join_scope o v_txn1) (join_scope o v_txn2) = true) /\
      (forall fuel o r, prepare fuel o v_txn1 v_txn2 = Some r ->
         locked_ok (if covers (own_of o) (join_scope o v_txn1) then own_of o else join_scope o v_txn1) (j_trace (fst r)) = true).
    Proof.
      assert (Hd : forall v seeds, length (universe v seeds) <= closure_depth v seeds).
      { intros v seeds. unfold closure_depth. apply le_n. }
      assert (Hcov : forall fuel o v0, wf_op o ->
                forall op0, In op0 (j_trace (fst (run_join fuel o (join_scope o v0) v0))) -> incl (staged_colls op0) (join_scope o v0)).
      { intros fuel o v0 Hwf. destruct (join_scope_props Hd o v0) as [Hcl Hs].
        exact (proj2 (proj2 (proj2 (Cov_run_join fuel o (join_scope o v0) v0 Hcl Hs Hwf)))). }
      split; [exact Hcov |]. split; [| split].
      - intros fuel o Hwf. unfold prepare. rewrite footprint_serializes.
        destruct (covers (own_of o) (join_scope o v_txn1)) eqn:Ec.
        + replace (locked_ok (own_of o) (j_trace (fst (run_join fuel o (join_scope o v_txn1) v_txn1)))) with true; [reflexivity |].
          symmetry. unfold locked_ok. apply forallb_forall. intros op0 Hin. apply forallb_forall. intros c Hc.
          unfold covers in Ec. rewrite forallb_forall in Ec. exact (Ec c (Hcov fuel o v_txn1 Hwf op0 Hin c Hc)).
        + replace (locked_ok (join_scope o v_txn1) (j_trace (fst (run_join fuel o (join_scope o v_txn1) v_txn1)))) with true; [reflexivity |].
          symmetry. unfold locked_ok. apply forallb_forall. intros op0 Hin. apply forallb_forall. intros c Hc.
          apply mem_spec. exact (Hcov fuel o v_txn1 Hwf op0 Hin c Hc).
      - intros o. rewrite footprint_serializes. unfold covers. apply forallb_forall. intros c Hc. apply mem_spec. exact Hc.
      - intros fuel o r H. unfold prepare in H.
        destruct (locked_ok (if covers (own_of o) (join_scope o v_txn1) then own_of o else join_scope o v_txn1)
                    (j_trace (fst (run_join fuel o (join_scope o v_txn1)
                                     (if covers (own_of o) (join_scope o v_txn1) then v_txn1 else v_txn2))))) eqn:El;
          [injection H as <-; exact El | discriminate].
    Qed.
  End PlanThenLock.

  (* ── 35. Distinct identities across the view and the pending work ─── *)

  Definition WF (j : jst) : Prop := NoDup (live j) /\ NoDup (map w_id (v_wtrs (j_view j))).

  Lemma nodup_app_iff :
    forall (X : Type) (a b : list X), NoDup (a ++ b) <-> NoDup a /\ NoDup b /\ (forall x, In x a -> ~ In x b).
  Proof.
    intros X a b. split.
    - intros H. split; [| split].
      + exact (NoDup_app_remove_r _ _ H).
      + exact (NoDup_app_remove_l _ _ H).
      + intros x Ha Hb. induction a as [| y r IH]; [destruct Ha |]. cbn [app] in H. apply NoDup_cons_iff in H as [Hn H].
        destruct Ha as [<- | Ha]; [apply Hn; apply in_or_app; right; exact Hb | exact (IH H Ha)].
    - intros (Ha & Hb & Hd). apply NoDup_app; assumption.
  Qed.

  Lemma in_remove_all_ids :
    forall L l id, NoDup (map r_id l) -> (forall r, In r L -> In r l) ->
      In id (map r_id (remove_all L l)) <-> In id (map r_id l) /\ ~ In id (map r_id L).
  Proof.
    intros L l id Hnd HL. split.
    - intros H. apply in_map_iff in H as (e & <- & He). apply (in_remove_all L l e Hnd) in He as [He Hn].
      split; [apply in_map; exact He | exact Hn].
    - intros [H Hn]. apply in_map_iff in H as (e & <- & He). apply in_map.
      apply (in_remove_all L l e Hnd). split; [exact He | exact Hn].
  Qed.

  Lemma nodup_ids_remove_all : forall L l, NoDup (map r_id l) -> NoDup (map r_id (remove_all L l)).
  Proof.
    induction L as [| r rest IH]; intros l H; [exact H |]. rewrite remove_all_cons. apply IH.
    clear -H. induction l as [| e l' IHl]; [constructor |]. cbn [del_rec map] in *. apply NoDup_cons_iff in H as [Hn H].
    destruct (r_id e =? r_id r); [exact H |]. cbn [map]. constructor; [| exact (IHl H)].
    intros Hin. apply Hn. apply in_map_iff in Hin as (y & E & Hy). rewrite <- E. apply in_map. exact (del_rec_incl _ _ y Hy).
  Qed.

  Lemma WF_same :
    forall j j', WF j -> Permutation (live j') (live j) -> NoDup (map w_id (v_wtrs (j_view j'))) -> WF j'.
  Proof. intros j j' [H1 H2] Hp H3. split; [exact (Permutation_NoDup (Permutation_sym Hp) H1) | exact H3]. Qed.

  Lemma WF_touch_all : forall own cs j, WF j -> WF (touch_all own j cs).
  Proof.
    intros own cs j H. destruct (touch_all_spec own cs j) as (bs & _ & Vw & _ & _ & Wk & _).
    destruct (bumps_view bs (j_view j)) as [Br Bw]. apply (WF_same j _ H).
    - unfold live. rewrite Vw, Wk, Br. apply Permutation_refl.
    - rewrite Vw, Bw. exact (proj2 H).
  Qed.

  Lemma WF_store : forall own j x, WF j -> ~ In (r_id (e_rec x)) (live j) -> WF (store own j x).
  Proof.
    intros own j x [H1 H2] Hf. destruct (store_spec own j x) as (bs & _ & Vw & _ & _ & Wk & _).
    destruct (bumps_view bs (j_view j)) as [Br Bw]. split.
    - unfold live. rewrite Vw, Wk, apply_all_app. cbn [apply_all fold_left apply v_recs map]. rewrite Br. cbn [app].
      constructor; [exact Hf | exact H1].
    - rewrite Vw, apply_all_app. cbn [apply_all fold_left apply v_wtrs]. rewrite Bw. exact H2.
  Qed.

  Lemma WF_park : forall j W, WF j -> ~ In (w_id W) (map w_id (v_wtrs (j_view j))) -> WF (park j W).
  Proof.
    intros j W [H1 H2] Hf. split; [exact H1 |]. unfold park. cbn [set_own stage j_view apply v_wtrs map].
    constructor; [exact Hf | exact H2].
  Qed.

  (* The fire step: the removed records leave the view, and a peek pushes
     them again with the incoming record; the others are spent. *)
  Lemma WF_fire_step :
    forall j bs W sg ainc oi fl j3,
      WF j ->
      (forall r, In r (removed sg ainc) -> In r (v_recs (j_view j))) ->
      NoDup (map (fun rm => r_id (fst rm)) sg) ->
      (forall p a e m, ainc = Some (p, a) -> nth_error sg p = Some (e, m) -> ~ In (r_id e) (live j)) ->
      (forall k e m, nth_error sg k = Some (e, m) -> inc_pos ainc k = false -> r_pers e = false -> In e (removed sg ainc)) ->
      (In W (v_wtrs (j_view j)) \/ ~ In (w_id W) (map w_id (v_wtrs (j_view j)))) ->
      j_view j3 = apply (apply_all (j_view j) (map SBump bs)) (SFire W sg ainc) -> j_work j3 = j_work j ->
      WF (fire_tail W sg (option_map fst ainc) oi fl j3).
  Proof.
    intros j bs W sg ainc oi fl j3 [H1 H2] Hrem Hnd Hinc Hcov HW V3 W3.
    destruct (fire_tail_spec W sg (option_map fst ainc) oi fl j3) as (rs & _ & V4 & _ & _ & P4 & _ & _ & _ & Hrs & Hrs0).
    destruct (bumps_view bs (j_view j)) as [Br Bw].
    unfold live in H1. apply nodup_app_iff in H1 as (Nr & Nw & Dj).
    assert (Hrem' : forall r, In r (removed sg ainc) -> In r (v_recs (j_view j))) by exact Hrem.
    set (R1 := remove_all (removed sg ainc) (v_recs (j_view j))).
    assert (V3r : v_recs (j_view j3) = R1) by (rewrite V3; cbn [apply v_recs]; rewrite Br; reflexivity).
    assert (NR1 : NoDup (map r_id R1)) by (apply nodup_ids_remove_all; exact Nr).
    split.
    - unfold live. rewrite V4. cbn [v_recs]. rewrite V3r.
      apply (Permutation_NoDup (Permutation_app_head _ (Permutation_sym (Permutation_map (fun q => r_id (p_rec q)) P4)))).
      rewrite map_app, W3. destruct (w_peek W) eqn:Ep.
      + rewrite (Hrs0 (or_introl eq_refl)). cbn [remove_all fold_left].
        apply nodup_app_iff. split; [exact NR1 |]. split.
        * apply nodup_app_iff. split; [| split; [exact Nw |]].
          -- unfold np_results. rewrite map_map. cbn [p_rec].
             assert (G : forall (f : nat * (rec * M) -> bool) k l, NoDup (map (fun rm => r_id (fst rm)) l) ->
                       NoDup (map (fun krm : nat * (rec * M) => r_id (fst (snd krm))) (filter f (enum k l)))).
             { intros f k l. revert k. induction l as [| rm r IH]; intros k Hl; [constructor |].
               cbn [map] in Hl. apply NoDup_cons_iff in Hl as [Hn Hl]. cbn [enum filter].
               destruct (f (k, rm)); [| exact (IH (S k) Hl)].
               cbn [map snd]. constructor; [| exact (IH (S k) Hl)].
               intros Hin. apply in_map_iff in Hin as ([k' rm'] & E & Hin). apply filter_In in Hin as [Hin _].
               apply (enum_nth _ r (S k)) in Hin as [_ Hn']. apply Hn. cbn [snd] in E. rewrite <- E.
               exact (in_map (fun rm0 : rec * M => r_id (fst rm0)) _ _ (nth_error_In _ _ Hn')). }
             exact (G _ 0 sg Hnd).
          -- intros id Hid Hw. apply in_map_iff in Hid as (q & <- & Hq).
             unfold np_results in Hq. apply in_map_iff in Hq as ([k [e m]] & <- & Hq). apply filter_In in Hq as [Hq Hp].
             apply (enum_nth _ sg 0) in Hq as [_ Hk]. rewrite Nat.sub_0_r in Hk. cbn [p_rec fst snd] in Hw.
             destruct (inc_pos ainc k) eqn:Ei.
             ++ destruct ainc as [[p a] |]; [| discriminate]. cbn [inc_pos] in Ei. apply Nat.eqb_eq in Ei. subst k.
                apply (Hinc p a e m eq_refl Hk). apply in_or_app. right. exact Hw.
             ++ unfold sends_at in Hp. cbn [fst snd] in Hp. rewrite is_inc_option_map, Ei in Hp.
                apply negb_true_iff in Hp. pose proof (Hcov k e m Hk Ei Hp) as Hr. apply Hrem in Hr.
                exact (Dj (r_id e) (in_map _ _ _ Hr) Hw).
        * intros id Hid Hin. apply in_app_or in Hin as [Hin | Hin].
          -- apply in_map_iff in Hin as (q & <- & Hq). unfold np_results in Hq. apply in_map_iff in Hq as ([k [e m]] & <- & Hq).
             apply filter_In in Hq as [Hq Hp]. apply (enum_nth _ sg 0) in Hq as [_ Hk]. rewrite Nat.sub_0_r in Hk.
             cbn [p_rec fst snd] in Hid.
             apply (in_remove_all_ids _ _ _ Nr Hrem) in Hid as [Hid Hnr].
             destruct (inc_pos ainc k) eqn:Ei.
             ++ destruct ainc as [[p a] |]; [| discriminate]. cbn [inc_pos] in Ei. apply Nat.eqb_eq in Ei. subst k.
                apply (Hinc p a e m eq_refl Hk). apply in_or_app. left. exact Hid.
             ++ unfold sends_at in Hp. cbn [fst snd] in Hp. rewrite is_inc_option_map, Ei in Hp.
                apply negb_true_iff in Hp. apply Hnr. apply in_map. exact (Hcov k e m Hk Ei Hp).
          -- apply (in_remove_all_ids _ _ _ Nr Hrem) in Hid as [Hid _]. exact (Dj id Hid Hin).
      + cbn [app]. apply nodup_app_iff. split; [apply nodup_ids_remove_all; exact NR1 |]. split; [exact Nw |].
        intros id Hid Hw. apply in_map_iff in Hid as (e & <- & He). apply remove_all_incl, remove_all_incl in He.
        exact (Dj (r_id e) (in_map _ _ _ He) Hw).
    - rewrite V4. cbn [v_wtrs]. rewrite V3. cbn [apply v_wtrs]. rewrite Bw.
      destruct (w_pers W); [exact H2 | apply nodup_wtr_ids_del; exact H2].
  Qed.

  Definition np_ids (sg : list (rec * M)) : list nat :=
    map (fun rm => r_id (fst rm)) (filter (fun rm => negb (r_pers (fst rm))) sg).

  Lemma np_ids_recs : forall sg, np_ids sg = map r_id (filter (fun r => negb (r_pers r)) (map fst sg)).
  Proof.
    intros sg. unfold np_ids. induction sg as [| [e m] r IH]; [reflexivity |].
    cbn [filter map fst]. destruct (negb (r_pers e)); cbn [map]; rewrite IH; reflexivity.
  Qed.

  (* The identities of the second sends of a consume-side peek fire and of a
     peek fire of the cascade: the bound records that are not persistent. *)
  Lemma np_results_np_ids : forall sg oi fl, map (fun q => r_id (p_rec q)) (np_results sg None oi fl) = np_ids sg.
  Proof. intros sg oi fl. rewrite <- (map_map p_rec r_id), np_results_none_recs, np_ids_recs. reflexivity. Qed.

  Lemma np_results_lift_ids :
    forall sg vp rv mv oi, nth_error sg vp = Some (rv, mv) ->
      map (fun q => r_id (p_rec q)) (np_results sg (Some vp) oi (negb (r_pers rv))) = np_ids sg.
  Proof.
    intros sg vp rv mv oi Hv. rewrite <- (map_map p_rec r_id), (np_results_lift_recs sg vp rv mv oi Hv), np_ids_recs. reflexivity.
  Qed.

  Lemma np_ids_nodup : forall sg, NoDup (map (fun rm => r_id (fst rm)) sg) -> NoDup (np_ids sg).
  Proof.
    intros sg H. unfold np_ids. induction sg as [| rm r IH]; [constructor |]. cbn [map] in H.
    apply NoDup_cons_iff in H as [Hn H]. cbn [filter]. destruct (negb (r_pers (fst rm))); [| exact (IH H)].
    cbn [map]. constructor; [| exact (IH H)]. intros Hin. apply Hn. apply in_map_iff in Hin as (rm' & E & Hin).
    apply filter_In in Hin as [Hin _]. rewrite <- E. exact (in_map (fun rm0 : rec * M => r_id (fst rm0)) _ _ Hin).
  Qed.

  (* The general step of WF: records leave the view (R), and the pushed
     identities are those of non-persistent bound records, each removed
     from the view or fresh. *)
  Lemma WF_gen :
    forall j j' R rs (pushed : list nat),
      WF j -> (forall r, In r R -> In r (v_recs (j_view j))) -> NoDup pushed ->
      (forall id, In id pushed -> In id (map r_id R) \/ ~ In id (live j)) ->
      v_recs (j_view j') = remove_all rs (remove_all R (v_recs (j_view j))) ->
      Permutation (map (fun q => r_id (p_rec q)) (j_work j')) (pushed ++ map (fun q => r_id (p_rec q)) (j_work j)) ->
      NoDup (map w_id (v_wtrs (j_view j'))) -> WF j'.
  Proof.
    intros j j' R rs pushed [H1 H2] HR Np Hp V' P' N'. split; [| exact N'].
    unfold live in H1 |- *. apply nodup_app_iff in H1 as (Nr & Nw & Dj).
    rewrite V'. apply (Permutation_NoDup (Permutation_app_head _ (Permutation_sym P'))).
    assert (NR1 : NoDup (map r_id (remove_all R (v_recs (j_view j))))) by (apply nodup_ids_remove_all; exact Nr).
    assert (Hsub : forall id, In id (map r_id (remove_all rs (remove_all R (v_recs (j_view j))))) ->
                     In id (map r_id (v_recs (j_view j))) /\ ~ In id (map r_id R)).
    { intros id Hid. apply in_map_iff in Hid as (e & <- & He). apply remove_all_incl in He.
      apply (in_remove_all_ids R (v_recs (j_view j)) (r_id e) Nr HR). apply in_map. exact He. }
    apply nodup_app_iff. split; [apply nodup_ids_remove_all; exact NR1 |]. split.
    - apply nodup_app_iff. split; [exact Np |]. split; [exact Nw |].
      intros id Hid Hw. destruct (Hp id Hid) as [HidR | Hf].
      + apply in_map_iff in HidR as (r & <- & Hr). exact (Dj (r_id r) (in_map _ _ _ (HR r Hr)) Hw).
      + apply Hf. apply in_or_app. right. exact Hw.
    - intros id Hid Hin. destruct (Hsub id Hid) as [Hv HnR]. apply in_app_or in Hin as [Hin | Hin].
      + destruct (Hp id Hin) as [HidR | Hf]; [exact (HnR HidR) | apply Hf; apply in_or_app; left; exact Hv].
      + exact (Dj id Hv Hin).
  Qed.

  Lemma nodup_map_inj_on :
    forall (X Y : Type) (f : X -> Y) l, NoDup l -> (forall a b, In a l -> In b l -> f a = f b -> a = b) -> NoDup (map f l).
  Proof.
    intros X Y f l. induction l as [| a r IH]; intros Hnd Hinj; [constructor |]. apply NoDup_cons_iff in Hnd as [Hn Hnd].
    cbn [map]. constructor.
    - intros Hin. apply in_map_iff in Hin as (b & E & Hb). apply Hn.
      rewrite (Hinj a b (or_introl eq_refl) (or_intror Hb) (eq_sym E)). exact Hb.
    - apply IH; [exact Hnd |]. intros a' b' Ha Hb E. exact (Hinj a' b' (or_intror Ha) (or_intror Hb) E).
  Qed.

  (* The bound records of a fire have distinct identities: they are distinct
     records of the view and a fresh incoming record. *)
  Lemma sg_ids_nodup :
    forall (sg : list (rec * M)) x recs,
      NoDup (map fst sg) -> (forall e, In e (map fst sg) -> e = x \/ In e recs) ->
      NoDup (map r_id recs) -> ~ In (r_id x) (map r_id recs) ->
      NoDup (map (fun rm => r_id (fst rm)) sg).
  Proof.
    intros sg x recs Hnd Hin Hr Hx. rewrite <- map_map. apply nodup_map_inj_on; [exact Hnd |].
    intros a b Ha Hb E. destruct (Hin a Ha) as [-> | Ha']; destruct (Hin b Hb) as [-> | Hb']; try reflexivity.
    - exfalso. apply Hx. rewrite E. apply in_map. exact Hb'.
    - exfalso. apply Hx. rewrite <- E. apply in_map. exact Ha'.
    - exact (nodup_map_inj _ _ r_id recs a b Hr Ha' Hb' E).
  Qed.

  Lemma live_fresh_recs : forall j id, ~ In id (live j) -> ~ In id (map r_id (v_recs (j_view j))).
  Proof. intros j id H Hin. apply H. unfold live. apply in_or_app. left. exact Hin. Qed.

  Lemma WF_recs : forall j, WF j -> NoDup (map r_id (v_recs (j_view j))).
  Proof. intros j [H _]. unfold live in H. exact (NoDup_app_remove_r _ _ H). Qed.

  Lemma WF_offer1 :
    forall own j x, WF j -> ~ In (r_id (e_rec x)) (live j) ->
      WF (fst (offer1 own j x)) /\ (snd (offer1 own j x) = true -> ~ In (r_id (e_rec x)) (live (fst (offer1 own j x)))).
  Proof.
    intros own j x H Hx. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep;
      [| split; [exact (WF_store own j x H Hx) | discriminate]].
    destruct (proj1 join_fires_heads_only _ _ _ _ _ Ep) as (_ & HwIn & Hrel & mx & Hxp).
    assert (Hfw : fires_with W (e_rec x) (j_view j) = Some (p, sg)).
    { assert (G : forall ws, offer_pick (e_rec x) (j_view j) ws = Some (W, p, sg) -> fires_with W (e_rec x) (j_view j) = Some (p, sg)).
      { induction ws as [| W0 r IH]; intros Hp; [discriminate |]. cbn [offer_pick] in Hp.
        destruct (fires_with W0 (e_rec x) (j_view j)) as [[p0 sg0] |] eqn:Ef; [injection Hp as <- <- <-; exact Ef | exact (IH Hp)]. }
      exact (G _ Ep). }
    assert (Hsp : search_produce W (e_rec x) (j_view j) = Some (p, sg)).
    { unfold fires_with in Hfw. destruct (heads_ok (heads_required W (e_rec x)) W (j_view j)); [| discriminate].
      destruct (search_produce W (e_rec x) (j_view j)) as [[p0 sg0] |]; [| discriminate].
      destruct (guard_ok W sg0); [injection Hfw as -> ->; reflexivity | discriminate]. }
    destruct (proj2 (proj2 join_repeated_channel_distinct_records) W (e_rec x) (j_view j) p sg Hsp) as [_ Hndr].
    assert (Hxfresh : ~ In (r_id (e_rec x)) (map r_id (v_recs (j_view j)))) by exact (live_fresh_recs j _ Hx).
    assert (Hin : forall e, In e (map fst sg) -> e = e_rec x \/ In e (v_recs (j_view j))).
    { intros e He. destruct (slots_prod_colls W (e_rec x) p (j_view j) sg Hrel e He) as [-> | [He' _]]; [left | right]; auto. }
    assert (Hnd : NoDup (map (fun rm => r_id (fst rm)) sg)) by exact (sg_ids_nodup sg (e_rec x) _ Hndr Hin (WF_recs j H) Hxfresh).
    destruct (touch_all_spec own (colls W) j) as (bs & T1 & V1 & _ & _ & W1 & _).
    set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                 (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
    assert (H4 : WF (fire_tail W sg (option_map fst (Some (p, e_auth x))) (e_own x) true j3)).
    { apply (WF_fire_step j bs W sg (Some (p, e_auth x)) (e_own x) true j3 H).
      - intros r Hr. destruct (removed_pos _ _ _ Hr) as (k & m & Hk & Hi).
        destruct (Hin r (in_map fst _ _ (nth_error_In _ _ Hk))) as [-> | Hr']; [| exact Hr'].
        exfalso. cbn [inc_pos] in Hi. apply Nat.eqb_neq in Hi. apply Hi.
        apply (NoDup_nth_error (map fst sg)); [exact Hndr | rewrite length_map; apply nth_error_Some; rewrite Hk; discriminate |].
        rewrite !nth_error_map, Hk, Hxp. reflexivity.
      - exact Hnd.
      - intros p0 a0 e m E Hk. injection E as <- _. rewrite Hxp in Hk. injection Hk as <- _. exact Hx.
      - intros k e m Hk Hi Hp. exact (removed_spec sg _ k e m Hk Hi Hp).
      - left. exact HwIn.
      - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view]. rewrite V1. reflexivity.
      - unfold j3. cbn [add_dirty with_dirty with_fired stage j_work]. exact W1. }
    cbn [option_map fst] in H4.
    (* the record of a continued offer is persistent: it was neither stored nor pushed *)
    assert (Hlive : w_peek W = false -> incl (live (fire_tail W sg (Some p) (e_own x) true j3)) (live j)).
    { intros Epk id Hid. destruct (fire_tail_spec W sg (Some p) (e_own x) true j3) as (rs & _ & V4 & _ & _ & P4 & _).
      unfold live in Hid |- *. rewrite V4 in Hid. cbn [v_recs] in Hid. rewrite Epk in P4. cbn [app] in P4.
      apply in_app_or in Hid as [Hid | Hid].
      - apply in_map_iff in Hid as (e & <- & He). apply remove_all_incl in He.
        unfold j3 in He. cbn [add_dirty with_dirty with_fired stage j_view apply v_recs] in He. rewrite V1 in He.
        rewrite (proj1 (bumps_view bs (j_view j))) in He. apply remove_all_incl in He.
        apply in_or_app. left. apply in_map. exact He.
      - apply in_or_app. right. apply (Permutation_in _ (Permutation_map _ P4)) in Hid.
        unfold j3 in Hid. cbn [add_dirty with_dirty with_fired stage j_work] in Hid. rewrite W1 in Hid. exact Hid. }
    destruct (w_peek W) eqn:Epk; [split; [exact H4 | discriminate] |].
    destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W)) eqn:Ea;
      cbn [fst snd]; unfold own_flight; try destruct (e_own x);
      (split; [exact (WF_same _ _ H4 (Permutation_refl _) (proj2 H4)) | try discriminate]);
      intros _ Hlv; apply Hx; apply (Hlive eq_refl); exact Hlv.
  Qed.

  Lemma WF_offer :
    forall fuel own j x, WF j -> ~ In (r_id (e_rec x)) (live j) -> WF (offer fuel own j x).
  Proof.
    induction fuel as [| f IH]; intros own j x H Hx; cbn [offer].
    - unfold own_flight. destruct (e_own x); exact (WF_same _ _ H (Permutation_refl _) (proj2 H)).
    - destruct (WF_offer1 own j x H Hx) as [H1 H2]. destruct (offer1 own j x) as [j' []]; cbn [fst snd] in H1, H2;
        [exact (IH own j' x H1 (H2 eq_refl)) | exact H1].
  Qed.

  Lemma WF_receive1 :
    forall j W, WF j -> ~ In (w_id W) (map w_id (v_wtrs (j_view j))) ->
      WF (fst (receive1 j W)) /\ (snd (receive1 j W) = true -> ~ In (w_id W) (map w_id (v_wtrs (j_view (fst (receive1 j W)))))).
  Proof.
    intros j W H Hw. unfold receive1.
    destruct (receive_sel (j_view j) W) as [sg |] eqn:Er; [| split; [exact (WF_park j W H Hw) | discriminate]].
    assert (Hgr : greedy W (j_view j) = Some sg).
    { unfold receive_sel in Er. destruct (heads_ok (colls W) W (park_view W (j_view j))); [| discriminate].
      unfold receive_decision in Er. destruct (greedy W (j_view j)) as [sg' |]; [| discriminate].
      destruct (guard_ok W sg'); [injection Er as <-; reflexivity | discriminate]. }
    destruct (proj1 (proj2 join_repeated_channel_distinct_records) W (j_view j) sg Hgr) as [_ Hndr].
    assert (Hin : forall e, In e (map fst sg) -> In e (v_recs (j_view j))) by (intros e He; exact (proj1 (greedy_colls _ _ _ Hgr e He))).
    assert (Hnd : NoDup (map (fun rm => r_id (fst rm)) sg)).
    { rewrite <- map_map. apply nodup_map_inj_on; [exact Hndr |]. intros a b Ha Hb E.
      exact (nodup_map_inj _ _ r_id _ a b (WF_recs j H) (Hin a Ha) (Hin b Hb) E). }
    set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
    assert (H4 : WF (fire_tail W sg (option_map fst (@None (nat * A))) false false j3)).
    { apply (WF_fire_step j [] W sg None false false j3 H).
      - intros r Hr. destruct (removed_pos _ _ _ Hr) as (k & m & Hk & _). exact (Hin r (in_map fst _ _ (nth_error_In _ _ Hk))).
      - exact Hnd.
      - intros p0 a0 e m E. discriminate.
      - intros k e m Hk Hi Hp. exact (removed_spec sg _ k e m Hk Hi Hp).
      - right. exact Hw.
      - reflexivity.
      - reflexivity. }
    cbn [option_map] in H4.
    assert (Hws : ~ In (w_id W) (map w_id (v_wtrs (j_view (fire_tail W sg None false false j3))))).
    { destruct (fire_tail_spec W sg None false false j3) as (rs & _ & V4 & _). rewrite V4. cbn [v_wtrs].
      unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs].
      intros Hin'. apply Hw. destruct (w_pers W); [exact Hin' |].
      apply in_map_iff in Hin' as (W' & E & HW'). rewrite <- E. apply in_map. exact (del_wtr_incl _ _ W' HW'). }
    destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)));
      cbn [fst snd]; (split; [exact (WF_same _ _ H4 (Permutation_refl _) (proj2 H4)) | try discriminate]); intros _; exact Hws.
  Qed.

  Lemma WF_receive :
    forall fuel j W, WF j -> ~ In (w_id W) (map w_id (v_wtrs (j_view j))) -> WF (receive fuel j W).
  Proof.
    induction fuel as [| f IH]; intros j W H Hw; cbn [receive]; [exact (WF_same _ _ H (Permutation_refl _) (proj2 H)) |].
    destruct (WF_receive1 j W H Hw) as [H1 H2]. destruct (receive1 j W) as [j' []]; cbn [fst snd] in H1, H2;
      [exact (IH j' W H1 (H2 eq_refl)) | exact H1].
  Qed.

  Lemma sg_ids_of_recs :
    forall (sg : list (rec * M)) recs, NoDup (map fst sg) -> (forall e, In e (map fst sg) -> In e recs) -> NoDup (map r_id recs) ->
      NoDup (map (fun rm => r_id (fst rm)) sg).
  Proof.
    intros sg recs Hnd Hin Hr. rewrite <- map_map. apply nodup_map_inj_on; [exact Hnd |].
    intros a b Ha Hb E. exact (nodup_map_inj _ _ r_id recs a b Hr (Hin a Ha) (Hin b Hb) E).
  Qed.

  Lemma WF_lift_reoffer :
    forall own j W sg, WF j -> In W (v_wtrs (j_view j)) -> (forall e, In e (map fst sg) -> In e (v_recs (j_view j))) ->
      NoDup (map fst sg) -> WF (lift_reoffer own j W sg).
  Proof.
    intros own j W sg H HwIn Hsg Hndf. unfold lift_reoffer.
    destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn; [| exact H].
    pose proof (sg_ids_of_recs sg _ Hndf Hsg (WF_recs j H)) as Hnd.
    destruct (touch_all_spec own (colls W) j) as (bs & _ & V1 & _ & _ & W1 & _).
    destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (_ & V2 & _ & _ & W2 & _).
    set (R := (if r_pers rv then [] else [rv]) ++ removed sg (Some (reoffer_pos sg, own_auth rv))).
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                 (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    assert (V4 : v_recs (j_view j4) = remove_all [] (remove_all R (v_recs (j_view j)))).
    { unfold j4. cbn [add_dirty with_dirty with_fired stage j_view]. rewrite V2, V1.
      rewrite (lift_fire_recs (j_view j) bs W sg (reoffer_pos sg) (own_auth rv) rv mv Hn). reflexivity. }
    assert (Wv4 : NoDup (map w_id (v_wtrs (j_view j4)))).
    { unfold j4. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs]. rewrite V2, V1.
      assert (Hl : forall v0, v_wtrs (apply_all v0 (lift_ops rv)) = v_wtrs v0) by (intros v0; unfold lift_ops; destruct (r_pers rv); reflexivity).
      rewrite Hl, (proj2 (bumps_view bs (j_view j))). destruct (w_pers W); [exact (proj2 H) | apply nodup_wtr_ids_del; exact (proj2 H)]. }
    assert (Wk4 : j_work j4 = j_work j) by (unfold j4; cbn [add_dirty with_dirty with_fired stage j_work]; rewrite W2; exact W1).
    assert (HR : forall r, In r R -> In r (v_recs (j_view j))).
    { intros r Hr. unfold R in Hr. apply in_app_or in Hr as [Hr | Hr].
      - destruct (r_pers rv); [destruct Hr | destruct Hr as [<- | []]]. apply Hsg. apply in_map_iff. exists (rv, mv).
        split; [reflexivity | exact (nth_error_In _ _ Hn)].
      - destruct (removed_pos _ _ _ Hr) as (k & m & Hk & _). exact (Hsg r (in_map fst _ _ (nth_error_In _ _ Hk))). }
    assert (Hpush : forall id, In id (np_ids sg) -> In id (map r_id R) \/ ~ In id (live j)).
    { intros id Hid. left. unfold np_ids in Hid. apply in_map_iff in Hid as ([e m] & <- & Hid). apply filter_In in Hid as [Hid Hp].
      cbn [fst] in Hp |- *. apply negb_true_iff in Hp. apply In_nth_error in Hid as [k Hk]. apply in_map. unfold R.
      destruct (Nat.eq_dec k (reoffer_pos sg)) as [-> | Ne].
      + rewrite Hn in Hk. injection Hk as -> _. rewrite Hp. left. reflexivity.
      + apply in_or_app. right. apply (removed_spec sg _ k e m Hk); [cbn [inc_pos]; apply Nat.eqb_neq; exact Ne | exact Hp]. }
    destruct (fire_tail_spec W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4) as (rs & _ & V5 & _ & _ & P5 & _).
    assert (H5 : WF (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4)).
    { apply (WF_gen j _ R rs (if w_peek W then np_ids sg else []) H HR).
      - destruct (w_peek W); [exact (np_ids_nodup sg Hnd) | constructor].
      - intros id Hid. destruct (w_peek W); [exact (Hpush id Hid) | destruct Hid].
      - rewrite V5. cbn [v_recs]. rewrite V4. reflexivity.
      - apply (Permutation_trans (Permutation_map (fun q => r_id (p_rec q)) P5)). rewrite map_app, Wk4.
        destruct (w_peek W); [rewrite (np_results_lift_ids sg (reoffer_pos sg) rv mv false Hn) |]; apply Permutation_refl.
      - rewrite V5. cbn [v_wtrs]. exact Wv4. }
    destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W));
      try exact H5; exact (WF_same _ _ H5 (Permutation_refl _) (proj2 H5)).
  Qed.

  Lemma next_fire_nodup : forall d v W sg, next_fire d v = Some (W, sg) -> NoDup (map fst sg).
  Proof.
    intros d v W sg H. destruct (next_fire_sel _ _ _ _ H) as (_ & Hf & _).
    unfold fireable in Hf. destruct (heads_ok (colls W) W v); [| discriminate].
    unfold receive_decision in Hf. destruct (greedy W v) as [sg' |] eqn:Eg; [| discriminate].
    destruct (guard_ok W sg'); [injection Hf as <- | discriminate].
    exact (proj2 (proj1 (proj2 join_repeated_channel_distinct_records) W v sg' Eg)).
  Qed.

  Lemma WF_dstep : forall own a j, WF j -> WF (fst (dstep own a j)).
  Proof.
    intros own a j H. unfold dstep. destruct (j_stop j); [exact H |].
    destruct (j_work j) as [| q rest] eqn:Ew.
    - destruct cascades; [| exact H].
      destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |] eqn:En; [| exact H].
      destruct (cap <=? j_fired j); [exact (WF_same _ _ H (Permutation_refl _) (proj2 H)) |]. cbn [fst].
      destruct (next_fire_sel _ _ _ _ En) as (HwIn & _ & Hsg).
      apply WF_lift_reoffer; [exact H | exact HwIn | intros e He; exact (proj1 (Hsg e He)) | exact (next_fire_nodup _ _ _ _ En)].
    - destruct (cap <=? j_fired j); [exact (WF_same _ _ H (Permutation_refl _) (proj2 H)) |]. cbn [fst].
      destruct H as [H1 H2]. unfold live in H1. rewrite Ew in H1. cbn [map] in H1.
      assert (H0 : WF (with_work rest j)).
      { split; [| exact H2]. unfold live. cbn [with_work j_view j_work].
        apply (Permutation_NoDup (Permutation_sym (Permutation_middle _ _ _))) in H1. apply NoDup_cons_iff in H1 as [_ H1]. exact H1. }
      assert (Hf : ~ In (r_id (p_rec q)) (live (with_work rest j))).
      { unfold live. cbn [with_work j_view j_work]. apply (Permutation_NoDup (Permutation_sym (Permutation_middle _ _ _))) in H1.
        apply NoDup_cons_iff in H1 as [H1 _]. exact H1. }
      destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E;
        [apply WF_offer; [exact H0 | exact Hf] | apply WF_store; [exact H0 | exact Hf]].
  Qed.

  Lemma WF_drain : forall fuel own a j, WF j -> WF (drain fuel own a j).
  Proof.
    induction fuel as [| f IH]; intros own a j H; cbn [drain]; [exact (WF_same _ _ H (Permutation_refl _) (proj2 H)) |].
    pose proof (WF_dstep own a j H) as H1. destruct (dstep own a j) as [j' []]; cbn [fst] in H1; [exact (IH own a j' H1) | exact H1].
  Qed.

  (* ── 36. The removal of records and the greedy search (monotonicity) ── *)

  Lemma filter_comm :
    forall (X : Type) (f g : X -> bool) l, filter f (filter g l) = filter g (filter f l).
  Proof.
    intros X f g l. induction l as [| x r IH]; [reflexivity |]. cbn [filter].
    destruct (g x) eqn:Eg, (f x) eqn:Ef; cbn [filter]; rewrite ?Eg, ?Ef, IH; reflexivity.
  Qed.

  Lemma filter_and :
    forall (X : Type) (f g : X -> bool) l, filter f (filter g l) = filter (fun x => g x && f x) l.
  Proof.
    intros X f g l. induction l as [| x r IH]; [reflexivity |]. cbn [filter].
    destruct (g x) eqn:Eg; cbn [filter andb]; [destruct (f x); rewrite IH; reflexivity | exact IH].
  Qed.

  Lemma nodup_ids_filter : forall f l, NoDup (map r_id l) -> NoDup (map r_id (filter f l)).
  Proof.
    intros f l. induction l as [| r rest IH]; intros H; [constructor |]. cbn [map] in H.
    apply NoDup_cons_iff in H as [Hn H]. cbn [filter]. destruct (f r); [| exact (IH H)].
    cbn [map]. constructor; [| exact (IH H)]. intros Hin. apply Hn.
    apply in_map_iff in Hin as (e & E & He). apply filter_In in He as [He _]. rewrite <- E. apply in_map. exact He.
  Qed.

  (* With distinct identities, the removal of an identity is a filter. *)
  Lemma del_rec_filter :
    forall id l, NoDup (map r_id l) -> del_rec id l = filter (fun e => negb (r_id e =? id)) l.
  Proof.
    intros id l. induction l as [| r rest IH]; intros Hnd; [reflexivity |]. cbn [map] in Hnd.
    apply NoDup_cons_iff in Hnd as [Hn Hnd]. cbn [del_rec filter].
    destruct (Nat.eqb_spec (r_id r) id) as [E | E]; cbn [negb].
    - symmetry. apply forallb_filter_id. apply forallb_forall. intros e He. apply negb_true_iff, Nat.eqb_neq.
      intros E'. apply Hn. rewrite E, <- E'. apply in_map. exact He.
    - rewrite IH; [reflexivity | exact Hnd].
  Qed.

  Lemma remove_all_filter :
    forall R l, NoDup (map r_id l) -> remove_all R l = filter (fun e => negb (mem (r_id e) (map r_id R))) l.
  Proof.
    induction R as [| r R' IH]; intros l Hnd.
    - symmetry. apply forallb_filter_id. apply forallb_forall. intros e _. reflexivity.
    - rewrite remove_all_cons, (del_rec_filter _ _ Hnd), IH; [| apply nodup_ids_filter; exact Hnd].
      rewrite filter_and. apply filter_ext. intros e. cbn [map mem existsb]. unfold mem.
      rewrite negb_orb. reflexivity.
  Qed.

  (* The visit order commutes with every filter: the insertion hypothesis
     places the order of each pair of entries independently of the others. *)
  Lemma rorder_filter : forall f l, rorder (filter f l) = filter f (rorder l).
  Proof.
    intros f l. remember (length (filter (fun e => negb (f e)) l)) as n eqn:En. revert l En.
    induction n as [| n IH]; intros l En.
    - assert (Hall : forall e, In e l -> f e = true).
      { intros e He. destruct (f e) eqn:Ef; [reflexivity | exfalso].
        assert (Hin : In e (filter (fun e => negb (f e)) l)) by (apply filter_In; split; [exact He | rewrite Ef; reflexivity]).
        symmetry in En. apply length_zero_iff_nil in En. rewrite En in Hin. exact Hin. }
      rewrite (forallb_filter_id f l) by (apply forallb_forall; exact Hall).
      symmetry. apply forallb_filter_id. apply forallb_forall. intros e He. apply Hall.
      exact (Permutation_in _ (rorder_perm l) He).
    - assert (Hex : exists d, In d (filter (fun e => negb (f e)) l)).
      { destruct (filter (fun e => negb (f e)) l) as [| d rest]; [discriminate | exists d; left; reflexivity]. }
      destruct Hex as [d Hd]. apply filter_In in Hd as [Hd Hfd]. apply negb_true_iff in Hfd.
      apply in_split in Hd as (a & b & ->).
      assert (Hn : n = length (filter (fun e => negb (f e)) (a ++ b))).
      { rewrite filter_app, length_app. rewrite filter_app, length_app in En. cbn [filter] in En.
        rewrite Hfd in En. cbn [negb length] in En. lia. }
      destruct (rorder_insert d (a ++ b)) as (l1 & l2 & E1 & E2).
      assert (Ea : filter f (a ++ d :: b) = filter f (a ++ b)) by (rewrite !filter_app; cbn [filter]; rewrite Hfd; reflexivity).
      assert (El : filter f (l1 ++ d :: l2) = filter f (l1 ++ l2)) by (rewrite !filter_app; cbn [filter]; rewrite Hfd; reflexivity).
      assert (Ep : rorder (a ++ d :: b) = rorder (d :: a ++ b)).
      { apply rorder_set. apply Permutation_sym. apply Permutation_middle. }
      rewrite Ea, Ep, E2, El, <- E1. exact (IH (a ++ b) Hn).
  Qed.

  (* A slot after the removal of the entries that keep refuses. *)
  Definition drop_slot (keep : rec -> bool) (s : slot) : slot :=
    match s with Pick L p => Pick (filter keep L) p | Forced e m => Forced e m end.

  (* The pool of a pick after a removal is a subset of the pool before it,
     under claims that hold at most the claims before or removed tags. *)
  Lemma first_fit_mono :
    forall (keep : rec -> bool) C C' p L,
      (forall e, claimed C e = true -> claimed C' e = true \/ keep e = false) ->
      match first_fit C p L with
      | None => first_fit C' p (filter keep L) = None
      | Some (e, m) =>
          match first_fit C' p (filter keep L) with
          | None => True
          | Some (e', _) => keep e = false \/ claimed C' e = true \/ e' = e
          end
      end.
  Proof.
    intros keep C C' p L Hc. induction L as [| e0 more IH]; [reflexivity |].
    cbn [first_fit filter]. destruct (keep e0) eqn:Ek.
    - cbn [first_fit]. destruct (claimed C e0) eqn:E1.
      + destruct (Hc e0 E1) as [E2 | E2]; [rewrite E2; exact IH | rewrite Ek in E2; discriminate].
      + destruct (spatial p (r_val e0)) as [m0 |] eqn:Es.
        * destruct (claimed C' e0) eqn:E2.
          -- destruct (first_fit C' p (filter keep more)) as [[e' m'] |]; [right; left; reflexivity | exact I].
          -- right. right. reflexivity.
        * destruct (claimed C' e0); exact IH.
    - destruct (claimed C e0); [exact IH |].
      destruct (spatial p (r_val e0)) as [m0 |]; [| exact IH].
      destruct (first_fit C' p (filter keep more)) as [[e' m'] |]; [left; exact Ek | exact I].
  Qed.

  (* Monotonicity of the greedy search (plan 3.6): when the claim tag
     decides keep, a search that fails before the removal also fails after
     it. *)
  Lemma ref_fill_mono :
    forall (keep : rec -> bool), (forall e e', tag e = tag e' -> keep e = keep e') ->
      forall ss C C',
        (forall e, claimed C e = true -> claimed C' e = true \/ keep e = false) ->
        ref_fill C ss = None -> ref_fill C' (map (drop_slot keep) ss) = None.
  Proof.
    intros keep Hk ss. induction ss as [| s rest IH]; intros C C' Hc H; [discriminate |].
    destruct s as [L p | e m]; cbn [ref_fill map drop_slot] in H |- *.
    - pose proof (first_fit_mono keep C C' p L Hc) as Hm.
      destruct (first_fit C p L) as [[e m] |] eqn:E1; [| rewrite Hm; reflexivity].
      destruct (first_fit C' p (filter keep L)) as [[e' m'] |] eqn:E2; [| reflexivity].
      destruct (ref_fill (tag e :: C) rest) as [sg |] eqn:E3; [discriminate |].
      rewrite (IH (tag e :: C) (tag e' :: C')); [reflexivity | | exact E3].
      intros e0 H0. unfold claimed in H0 |- *. cbn [tmem existsb] in H0 |- *.
      apply orb_true_iff in H0 as [H0 | H0].
      + apply tag_eqb_spec in H0. destruct Hm as [Hm | [Hm | Hm]].
        * right. rewrite (Hk e0 e H0). exact Hm.
        * left. apply orb_true_iff. right. unfold claimed in Hm. rewrite H0. exact Hm.
        * left. apply orb_true_iff. left. apply tag_eqb_spec. rewrite H0, Hm. reflexivity.
      + destruct (Hc e0 H0) as [H1 | H1]; [left; apply orb_true_iff; right; exact H1 | right; exact H1].
    - destruct (ref_fill C rest) as [sg |] eqn:E1; [discriminate |].
      rewrite (IH C C' Hc E1). reflexivity.
  Qed.

  (* A bag (Scan): the window is every record in the visit order, so a view
     with a filtered record list has the filtered slots. *)
  Lemma slots_cons_filter :
    sh = Scan -> forall W v v' (kid : nat -> bool),
      v_recs v' = filter (fun e => kid (r_id e)) (v_recs v) ->
      slots_cons W v' = map (drop_slot (fun e => kid (r_id e))) (slots_cons W v).
  Proof.
    intros Hsh W v v' kid Hv. unfold slots_cons. rewrite map_map. apply map_ext. intros [i b]. cbn [snd drop_slot].
    f_equal. unfold win, window. rewrite Hsh. unfold recs_of. rewrite Hv.
    rewrite (filter_comm _ (fun r => r_coll r =? b_coll b) (fun e => kid (r_id e))), rorder_filter.
    unfold visible. apply filter_comm.
  Qed.

  Lemma greedy_filter_none :
    sh = Scan -> forall W v v' (kid : nat -> bool),
      v_recs v' = filter (fun e => kid (r_id e)) (v_recs v) -> greedy W v = None -> greedy W v' = None.
  Proof.
    intros Hsh W v v' kid Hv Hg. destruct join_search_is_head_greedy as (_ & Hgr & _).
    rewrite Hgr in Hg |- *. rewrite (slots_cons_filter Hsh W v v' kid Hv).
    assert (Hk : forall e e', tag e = tag e' -> kid (r_id e) = kid (r_id e')).
    { intros e e' E. unfold tag in E. rewrite Hsh in E. injection E as _ E. rewrite E. reflexivity. }
    exact (ref_fill_mono (fun e => kid (r_id e)) Hk (slots_cons W v) [] [] (fun e H => or_introl H) Hg).
  Qed.

  (* INV_scan (plan 3.6): no waiter without a guard has a greedy match. *)
  Definition INV_scan (v : view) : Prop :=
    forall W, In W (v_wtrs v) -> w_guard W = no_guard -> greedy W v = None.

  Lemma INV_scan_ctl : forall v v', INV_scan v -> v_recs v' = v_recs v -> v_wtrs v' = v_wtrs v -> INV_scan v'.
  Proof.
    intros v v' H Vr Vw W HW Hg. rewrite (greedy_frame W v v'); [| intros c _; unfold recs_of; rewrite Vr; reflexivity].
    rewrite Vw in HW. exact (H W HW Hg).
  Qed.

  Lemma INV_scan_filter :
    sh = Scan -> forall v v' (kid : nat -> bool),
      INV_scan v -> v_recs v' = filter (fun e => kid (r_id e)) (v_recs v) -> incl (v_wtrs v') (v_wtrs v) -> INV_scan v'.
  Proof.
    intros Hsh v v' kid H Vr Vw W HW Hg. exact (greedy_filter_none Hsh W v v' kid Vr (H W (Vw W HW) Hg)).
  Qed.

  Lemma INV_scan_remove :
    sh = Scan -> forall v v' R,
      INV_scan v -> NoDup (map r_id (v_recs v)) -> v_recs v' = remove_all R (v_recs v) -> incl (v_wtrs v') (v_wtrs v) ->
      INV_scan v'.
  Proof.
    intros Hsh v v' R H Hnd Vr Vw. apply (INV_scan_filter Hsh v v' (fun i => negb (mem i (map r_id R))) H); [| exact Vw].
    rewrite Vr. exact (remove_all_filter R _ Hnd).
  Qed.

  Lemma sim_drop :
    forall x ss sg, (forall s, In s ss -> exists L p, s = Pick L p) -> length ss = length sg ->
      (forall rm, In rm sg -> r_id (fst rm) <> r_id x) ->
      sim_slots x ss (map (drop_slot (fun e => negb (r_id e =? r_id x))) ss) sg.
  Proof.
    intros x ss. induction ss as [| s rest IH]; intros sg Hp Hl Hx; destruct sg as [| rm sg]; try discriminate; [exact I |].
    cbn [map sim_slots]. split.
    - destruct (Hp s (or_introl eq_refl)) as (L & p & ->). cbn [sim1 drop_slot].
      split; [reflexivity | split; [reflexivity | exact (Hx rm (or_introl eq_refl))]].
    - apply IH; [intros s' Hs'; exact (Hp s' (or_intror Hs')) | cbn [length] in Hl; lia |
        intros rm' Hrm'; exact (Hx rm' (or_intror Hrm'))].
  Qed.

  Lemma at_head_scan : sh = Scan -> forall W c v, at_head W c v = true.
  Proof. intros Hsh W c v. unfold at_head. rewrite Hsh. reflexivity. Qed.

  Lemma heads_ok_scan : sh = Scan -> forall cs W v, heads_ok cs W v = true.
  Proof. intros Hsh cs W v. unfold heads_ok. apply forallb_forall. intros c _. exact (at_head_scan Hsh W c v). Qed.

  Lemma scan_no_cascade : sh = Scan -> cascades = false.
  Proof. intros Hsh. unfold cascades. rewrite Hsh. reflexivity. Qed.

  Lemma scan_not_classes : sh = Scan -> sh <> Classes.
  Proof. intros Hsh E. rewrite Hsh in E. discriminate. Qed.


  (* A bag: when the produce search of x fails and greedy fails without x,
     greedy fails with x stored. Greedy with x either binds x, and then the
     produce search finds a fire (headline 2), or binds stored records only,
     and then the same assignment exists without x. *)
  Lemma greedy_store_scan :
    sh = Scan -> forall W x v, (forall e, In e (v_recs v) -> r_id e <> r_id x) ->
      search_produce W x v = None -> greedy W v = None -> greedy W (store_view x v) = None.
  Proof.
    intros Hsh W x v Hfresh Hsp Hg. destruct (greedy W (store_view x v)) as [sg |] eqn:E; [exfalso | reflexivity].
    destruct (existsb (fun rm => r_id (fst rm) =? r_id x) sg) eqn:Ex.
    - apply existsb_exists in Ex as ([e m] & Hin & Ei). apply Nat.eqb_eq in Ei. cbn [fst] in Ei.
      destruct (greedy_colls W (store_view x v) sg E e (in_map fst _ _ Hin)) as [He _].
      cbn [store_view v_recs] in He. destruct He as [<- | He]; [| exact (Hfresh e He Ei)].
      apply In_nth_error in Hin as [k Hk].
      destruct (proj1 join_produce_search_agrees_with_head W x v sg k m Hfresh E Hk) as (_ & k' & sg' & Hs & _).
      rewrite Hsp in Hs. discriminate.
    - assert (Hnx : forall rm, In rm sg -> r_id (fst rm) <> r_id x).
      { intros rm Hrm Ei. assert (Ht : existsb (fun rm => r_id (fst rm) =? r_id x) sg = true)
          by (apply existsb_exists; exists rm; split; [exact Hrm | apply Nat.eqb_eq; exact Ei]).
        rewrite Ex in Ht. discriminate. }
      destruct join_search_is_head_greedy as (_ & Hgr & _).
      rewrite Hgr in E, Hg.
      assert (Hv : v_recs v = filter (fun e => (fun i => negb (i =? r_id x)) (r_id e)) (v_recs (store_view x v))).
      { cbn [store_view v_recs filter]. rewrite Nat.eqb_refl. cbn [negb]. symmetry. apply forallb_filter_id.
        apply forallb_forall. intros e He. apply negb_true_iff, Nat.eqb_neq. exact (Hfresh e He). }
      rewrite (slots_cons_filter Hsh W (store_view x v) v _ Hv) in Hg. cbv beta in Hg.
      assert (Hpk : forall s, In s (slots_cons W (store_view x v)) -> exists L p, s = Pick L p).
      { intros s Hs. unfold slots_cons in Hs. apply in_map_iff in Hs as (ib & <- & _). eexists. eexists. reflexivity. }
      assert (Hlen : length (slots_cons W (store_view x v)) = length sg)
        by (symmetry; exact (slots_match_length _ _ (ref_fill_match _ _ _ E))).
      assert (Htag : forall e', In e' (pick_lists (slots_cons W (store_view x v))) -> r_id e' <> r_id x -> tag e' <> tag x).
      { intros e' _ Hne Et. unfold tag in Et. rewrite Hsh in Et. injection Et as _ Et. exact (Hne Et). }
      pose proof (ref_fill_sim x _ _ sg [] [] (sim_drop x _ sg Hpk Hlen Hnx) (fun t _ => iff_refl _) Htag E) as Hs.
      rewrite Hs in Hg. discriminate.
  Qed.

  Lemma offer_pick_none : forall x v ws, offer_pick x v ws = None -> forall W, In W ws -> fires_with W x v = None.
  Proof.
    intros x v ws. induction ws as [| W0 r IH]; intros H W HW; [destruct HW |]. cbn [offer_pick] in H.
    destruct (fires_with W0 x v) as [[p sg] |] eqn:E; [discriminate |].
    destruct HW as [<- | HW]; [exact E | exact (IH H W HW)].
  Qed.

  (* A bag: OFFER stores x only when no waiter of its collection fires. *)
  Lemma INV_scan_store :
    sh = Scan -> forall x v, INV_scan v -> (forall e, In e (v_recs v) -> r_id e <> r_id x) ->
      offer_pick x v (offer_cands x v) = None -> INV_scan (store_view x v).
  Proof.
    intros Hsh x v H Hfresh Hp W HW Hg. cbn [store_view v_wtrs] in HW.
    destruct (mem (r_coll x) (colls W)) eqn:Ec.
    - assert (Hc : In W (offer_cands x v)).
      { unfold offer_cands. rewrite Hsh. apply (Permutation_in _ (Permutation_sym (worder_perm _ _))).
        unfold waiters_of. apply filter_In. split; [exact HW | exact Ec]. }
      pose proof (offer_pick_none x v _ Hp W Hc) as Hf.
      assert (Hs : search_produce W x v = None).
      { unfold fires_with in Hf. rewrite (heads_ok_scan Hsh) in Hf.
        destruct (search_produce W x v) as [[p sg] |]; [| reflexivity].
        unfold guard_ok in Hf. rewrite Hg, no_guard_admits in Hf. discriminate. }
      exact (greedy_store_scan Hsh W x v Hfresh Hs (H W HW Hg)).
    - rewrite (greedy_frame W v (store_view x v)); [exact (H W HW Hg) |].
      intros c Hc. rewrite recs_of_store. destruct (Nat.eqb_spec c (r_coll x)) as [-> | E]; [| reflexivity].
      exfalso. apply mem_false in Ec. exact (Ec Hc).
  Qed.

  (* A bag: RECEIVE parks W only when greedy finds no match for it. *)
  Lemma INV_scan_park :
    sh = Scan -> forall W v, INV_scan v -> receive_sel v W = None -> INV_scan (park_view W v).
  Proof.
    intros Hsh W v H Hs W' HW' Hg.
    rewrite (greedy_frame W' v (park_view W v)) by (intros c _; reflexivity).
    cbn [park_view v_wtrs] in HW'. destruct HW' as [<- | HW']; [| exact (H W' HW' Hg)].
    unfold receive_sel in Hs. rewrite (heads_ok_scan Hsh) in Hs. unfold receive_decision in Hs.
    destruct (greedy W v) as [sg |]; [| reflexivity]. unfold guard_ok in Hs. rewrite Hg, no_guard_admits in Hs. discriminate.
  Qed.

  Lemma INV_scan_fire_tail :
    sh = Scan -> forall W sg inc oi fl j3 v R,
      INV_scan v -> NoDup (map r_id (v_recs v)) ->
      v_recs (j_view j3) = remove_all R (v_recs v) -> incl (v_wtrs (j_view j3)) (v_wtrs v) ->
      INV_scan (j_view (fire_tail W sg inc oi fl j3)).
  Proof.
    intros Hsh W sg inc oi fl j3 v R H Hnd Vr Vw.
    destruct (fire_tail_spec W sg inc oi fl j3) as (rs & _ & V4 & _ & _ & _ & _ & _ & _ & _ & Hrs).
    rewrite (Hrs (or_intror (scan_not_classes Hsh))) in V4. rewrite V4.
    apply (INV_scan_remove Hsh v _ R H Hnd); [exact Vr | exact Vw].
  Qed.

  Lemma INV_scan_offer1 :
    sh = Scan -> forall own j x, INV_scan (j_view j) -> WF j -> ~ In (r_id (e_rec x)) (live j) ->
      INV_scan (j_view (fst (offer1 own j x))).
  Proof.
    intros Hsh own j x H Hwf Hx. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep.
    - destruct (touch_all_spec own (colls W) j) as (bs & _ & V1 & _).
      destruct (bumps_view bs (j_view j)) as [Br Bw].
      set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                   (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
      assert (H4 : INV_scan (j_view (fire_tail W sg (Some p) (e_own x) true j3))).
      { apply (INV_scan_fire_tail Hsh W sg (Some p) (e_own x) true j3 (j_view j) (removed sg (Some (p, e_auth x))) H (WF_recs j Hwf)).
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_recs]. rewrite V1, Br. reflexivity.
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs]. rewrite V1, Bw.
          destruct (w_pers W); [apply incl_refl | apply del_wtr_incl]. }
      destruct (w_peek W); [exact H4 |].
      destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
        cbn [fst]; unfold own_flight; try destruct (e_own x); exact H4.
    - cbn [fst]. destruct (store_spec own j x) as (bs & _ & Vw & _).
      destruct (bumps_view bs (j_view j)) as [Br Bw].
      apply (INV_scan_ctl (store_view (e_rec x) (j_view j))).
      + apply (INV_scan_store Hsh (e_rec x) (j_view j) H); [| exact Ep].
        intros e He E. apply Hx. unfold live. apply in_or_app. left. rewrite <- E. apply in_map. exact He.
      + rewrite Vw, apply_all_app. cbn [apply_all fold_left apply v_recs store_view]. rewrite Br. reflexivity.
      + rewrite Vw, apply_all_app. cbn [apply_all fold_left apply v_wtrs store_view]. rewrite Bw. reflexivity.
  Qed.

  Lemma INV_scan_offer :
    sh = Scan -> forall fuel own j x, INV_scan (j_view j) -> WF j -> ~ In (r_id (e_rec x)) (live j) ->
      INV_scan (j_view (offer fuel own j x)).
  Proof.
    intros Hsh fuel. induction fuel as [| f IH]; intros own j x H Hwf Hx; cbn [offer].
    - unfold own_flight. destruct (e_own x); exact H.
    - pose proof (INV_scan_offer1 Hsh own j x H Hwf Hx) as H1. destruct (WF_offer1 own j x Hwf Hx) as [W1 F1].
      destruct (offer1 own j x) as [j' []]; cbn [fst snd] in H1, W1, F1; [exact (IH own j' x H1 W1 (F1 eq_refl)) | exact H1].
  Qed.

  Lemma INV_scan_receive1 :
    sh = Scan -> forall j W, INV_scan (j_view j) -> WF j -> INV_scan (j_view (fst (receive1 j W))).
  Proof.
    intros Hsh j W H Hwf. unfold receive1.
    destruct (receive_sel (j_view j) W) as [sg |] eqn:Er.
    - set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
      assert (H4 : INV_scan (j_view (fire_tail W sg None false false j3))).
      { apply (INV_scan_fire_tail Hsh W sg None false false j3 (j_view j) (removed sg None) H (WF_recs j Hwf)); [reflexivity |].
        unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs].
        destruct (w_pers W); [apply incl_refl | apply del_wtr_incl]. }
      destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)));
        cbn [fst]; exact H4.
    - cbn [fst]. unfold park. exact (INV_scan_park Hsh W (j_view j) H Er).
  Qed.

  Lemma INV_scan_receive :
    sh = Scan -> forall fuel j W, INV_scan (j_view j) -> WF j -> ~ In (w_id W) (map w_id (v_wtrs (j_view j))) ->
      INV_scan (j_view (receive fuel j W)).
  Proof.
    intros Hsh fuel. induction fuel as [| f IH]; intros j W H Hwf Hw; cbn [receive]; [exact H |].
    pose proof (INV_scan_receive1 Hsh j W H Hwf) as H1. destruct (WF_receive1 j W Hwf Hw) as [W1 F1].
    destruct (receive1 j W) as [j' []]; cbn [fst snd] in H1, W1, F1; [exact (IH j' W H1 W1 (F1 eq_refl)) | exact H1].
  Qed.

  (* The pop of a pending second send: the rest stays well formed and the
     popped record is fresh for its OFFER. *)
  Lemma WF_pop :
    forall j q rest, WF j -> j_work j = q :: rest ->
      WF (with_work rest j) /\ ~ In (r_id (p_rec q)) (live (with_work rest j)).
  Proof.
    intros j q rest [H1 H2] Ew. unfold live in H1. rewrite Ew in H1. cbn [map] in H1.
    apply (Permutation_NoDup (Permutation_sym (Permutation_middle _ _ _))) in H1. apply NoDup_cons_iff in H1 as [Hn H1].
    split; [split; [exact H1 | exact H2] | exact Hn].
  Qed.

  Lemma INV_scan_dstep :
    sh = Scan -> forall own a j, INV_scan (j_view j) -> WF j -> INV_scan (j_view (fst (dstep own a j))).
  Proof.
    intros Hsh own a j H Hwf. unfold dstep. destruct (j_stop j); [exact H |].
    destruct (j_work j) as [| q rest] eqn:Ew.
    - rewrite (scan_no_cascade Hsh). exact H.
    - destruct (cap <=? j_fired j); [exact H |]. cbn [fst].
      destruct (WF_pop j q rest Hwf Ew) as [W0 F0]. unfold resend.
      exact (INV_scan_offer Hsh _ own (with_work rest j) (resend_elem a q) H W0 F0).
  Qed.

  Lemma INV_scan_drain :
    sh = Scan -> forall fuel own a j, INV_scan (j_view j) -> WF j -> INV_scan (j_view (drain fuel own a j)).
  Proof.
    intros Hsh fuel. induction fuel as [| f IH]; intros own a j H Hwf; cbn [drain]; [exact H |].
    pose proof (INV_scan_dstep Hsh own a j H Hwf) as H1. pose proof (WF_dstep own a j Hwf) as W1.
    destruct (dstep own a j) as [j' []]; cbn [fst] in H1, W1; [exact (IH own a j' H1 W1) | exact H1].
  Qed.

  (* ── 37. The identities that the engine keeps ───────────────────────── *)

  (* The live identities and the waiters after a step are among those
     before it. *)
  Definition sub_ids (j j' : jst) : Prop :=
    (forall id, In id (live j') -> In id (live j)) /\ incl (v_wtrs (j_view j')) (v_wtrs (j_view j)).

  Lemma sub_ids_ctl : forall j j', j_view j' = j_view j -> j_work j' = j_work j -> sub_ids j j'.
  Proof.
    intros j j' Vw Wk. split; [| rewrite Vw; apply incl_refl].
    intros id H. unfold live in *. rewrite Vw, Wk in H. exact H.
  Qed.

  Lemma sub_ids_trans : forall j1 j2 j3, sub_ids j1 j2 -> sub_ids j2 j3 -> sub_ids j1 j3.
  Proof.
    intros j1 j2 j3 [L1 W1] [L2 W2]. split; [intros id H; exact (L1 id (L2 id H)) | exact (incl_tran W2 W1)].
  Qed.

  Lemma fire_step_ids :
    forall j bs W sg ainc oi fl j3,
      j_view j3 = apply (apply_all (j_view j) (map SBump bs)) (SFire W sg ainc) -> j_work j3 = j_work j ->
      (forall id, In id (live (fire_tail W sg (option_map fst ainc) oi fl j3)) ->
                  In id (live j) \/ In id (map (fun rm => r_id (fst rm)) sg)) /\
      incl (v_wtrs (j_view (fire_tail W sg (option_map fst ainc) oi fl j3))) (v_wtrs (j_view j)).
  Proof.
    intros j bs W sg ainc oi fl j3 V3 W3.
    destruct (fire_tail_spec W sg (option_map fst ainc) oi fl j3) as (rs & _ & V4 & _ & _ & P4 & _).
    destruct (bumps_view bs (j_view j)) as [Br Bw]. split.
    - intros id H. unfold live in H. rewrite V4 in H. cbn [v_recs] in H. apply in_app_or in H as [H | H].
      + left. apply in_or_app. left. apply in_map_iff in H as (e & <- & He). apply remove_all_incl in He.
        rewrite V3 in He. cbn [apply v_recs] in He. rewrite Br in He. apply remove_all_incl in He. apply in_map. exact He.
      + apply (Permutation_in _ (Permutation_map (fun q => r_id (p_rec q)) P4)) in H. rewrite map_app in H.
        apply in_app_or in H as [H | H].
        * right. destruct (w_peek W); [| destruct H]. apply in_map_iff in H as (q & <- & Hq).
          destruct (np_results_spec _ _ _ _ _ Hq) as [Hq1 _]. apply in_map_iff in Hq1 as ([e m] & E & Hrm). cbn [fst] in E.
          rewrite <- E. exact (in_map (fun rm0 : rec * M => r_id (fst rm0)) sg (e, m) Hrm).
        * left. apply in_or_app. right. rewrite W3 in H. exact H.
    - rewrite V4. cbn [v_wtrs]. rewrite V3. cbn [apply v_wtrs]. rewrite Bw.
      destruct (w_pers W); [apply incl_refl | apply del_wtr_incl].
  Qed.

  Lemma store_ids :
    forall own j x,
      (forall id, In id (live (store own j x)) -> In id (live j) \/ id = r_id (e_rec x)) /\
      incl (v_wtrs (j_view (store own j x))) (v_wtrs (j_view j)).
  Proof.
    intros own j x. destruct (store_spec own j x) as (bs & _ & Vw & _ & _ & Wk & _).
    destruct (bumps_view bs (j_view j)) as [Br Bw]. split.
    - intros id H. unfold live in H. rewrite Vw, Wk, apply_all_app in H. cbn [apply_all fold_left apply v_recs map] in H.
      rewrite Br in H. cbn [app] in H. destruct H as [H | H]; [right; exact (eq_sym H) | left; exact H].
    - rewrite Vw, apply_all_app. cbn [apply_all fold_left apply v_wtrs]. rewrite Bw. apply incl_refl.
  Qed.

  Lemma offer1_ids :
    forall own j x,
      (forall id, In id (live (fst (offer1 own j x))) -> In id (live j) \/ id = r_id (e_rec x)) /\
      incl (v_wtrs (j_view (fst (offer1 own j x)))) (v_wtrs (j_view j)).
  Proof.
    intros own j x. unfold offer1.
    destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep;
      [| exact (store_ids own j x)].
    destruct (proj1 join_fires_heads_only _ _ _ _ _ Ep) as (_ & _ & Hrel & _).
    destruct (touch_all_spec own (colls W) j) as (bs & _ & V1 & _ & _ & W1 & _).
    set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                 (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
    destruct (fire_step_ids j bs W sg (Some (p, e_auth x)) (e_own x) true j3) as [L4 I4].
    { unfold j3. cbn [add_dirty with_dirty with_fired stage j_view]. rewrite V1. reflexivity. }
    { unfold j3. cbn [add_dirty with_dirty with_fired stage j_work]. exact W1. }
    cbn [option_map fst] in L4, I4.
    assert (L4' : forall id, In id (live (fire_tail W sg (Some p) (e_own x) true j3)) -> In id (live j) \/ id = r_id (e_rec x)).
    { intros id H. destruct (L4 id H) as [H' | H']; [left; exact H' |].
      apply in_map_iff in H' as ([e m] & <- & He). cbn [fst].
      destruct (slots_prod_colls W (e_rec x) p (j_view j) sg Hrel e (in_map fst _ _ He)) as [-> | [He' _]];
        [right; reflexivity |].
      left. unfold live. apply in_or_app. left. apply in_map. exact He'. }
    destruct (w_peek W); [split; [exact L4' | exact I4] |].
    destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
      cbn [fst]; unfold own_flight; try destruct (e_own x); split; first [exact L4' | exact I4].
  Qed.

  Lemma offer_ids :
    forall fuel own j x,
      (forall id, In id (live (offer fuel own j x)) -> In id (live j) \/ id = r_id (e_rec x)) /\
      incl (v_wtrs (j_view (offer fuel own j x))) (v_wtrs (j_view j)).
  Proof.
    induction fuel as [| f IH]; intros own j x; cbn [offer].
    - unfold own_flight. destruct (e_own x); (split; [intros id H; left; exact H | apply incl_refl]).
    - destruct (offer1_ids own j x) as [L1 I1]. destruct (offer1 own j x) as [j' []]; cbn [fst] in L1, I1.
      + destruct (IH own j' x) as [L2 I2]. split; [| exact (incl_tran I2 I1)].
        intros id H. destruct (L2 id H) as [H' | H']; [exact (L1 id H') | right; exact H'].
      + split; [exact L1 | exact I1].
  Qed.

  Lemma lift_reoffer_ids :
    forall own j W sg, (forall e, In e (map fst sg) -> In e (v_recs (j_view j))) -> sub_ids j (lift_reoffer own j W sg).
  Proof.
    intros own j W sg Hsg. unfold lift_reoffer.
    destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn; [| apply sub_ids_ctl; reflexivity].
    destruct (touch_all_spec own (colls W) j) as (bs & _ & V1 & _ & _ & W1 & _).
    destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (_ & V2 & _ & _ & W2 & _).
    set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                 (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
    assert (V4 : v_recs (j_view j4) =
                 remove_all ((if r_pers rv then [] else [rv]) ++ removed sg (Some (reoffer_pos sg, own_auth rv))) (v_recs (j_view j))).
    { unfold j4. cbn [add_dirty with_dirty with_fired stage j_view]. rewrite V2, V1.
      exact (lift_fire_recs (j_view j) bs W sg (reoffer_pos sg) (own_auth rv) rv mv Hn). }
    assert (I4 : incl (v_wtrs (j_view j4)) (v_wtrs (j_view j))).
    { unfold j4. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs]. rewrite V2, V1.
      assert (Hl : forall v0, v_wtrs (apply_all v0 (lift_ops rv)) = v_wtrs v0)
        by (intros v0; unfold lift_ops; destruct (r_pers rv); reflexivity).
      rewrite Hl, (proj2 (bumps_view bs (j_view j))). destruct (w_pers W); [apply incl_refl | apply del_wtr_incl]. }
    assert (Wk4 : j_work j4 = j_work j) by (unfold j4; cbn [add_dirty with_dirty with_fired stage j_work]; rewrite W2; exact W1).
    assert (Hrec : forall id, In id (map r_id (v_recs (j_view j4))) -> In id (live j)).
    { intros id H. rewrite V4 in H. apply in_map_iff in H as (e & <- & He). apply remove_all_incl in He.
      unfold live. apply in_or_app. left. apply in_map. exact He. }
    destruct (fire_tail_spec W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4) as (rs & _ & V5 & _ & _ & P5 & _).
    assert (S5 : sub_ids j (fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4)).
    { split; [| rewrite V5; exact I4].
      intros id H. unfold live in H. rewrite V5 in H. cbn [v_recs] in H. apply in_app_or in H as [H | H].
      - apply Hrec. apply in_map_iff in H as (e & <- & He). apply remove_all_incl in He. apply in_map. exact He.
      - apply (Permutation_in _ (Permutation_map (fun q => r_id (p_rec q)) P5)) in H. rewrite map_app in H.
        apply in_app_or in H as [H | H].
        + destruct (w_peek W); [| destruct H]. apply in_map_iff in H as (q & <- & Hq).
          destruct (np_results_spec _ _ _ _ _ Hq) as [Hq1 _].
          unfold live. apply in_or_app. left. apply in_map. exact (Hsg _ Hq1).
        + unfold live. apply in_or_app. right. rewrite Wk4 in H. exact H. }
    destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W)); try exact S5;
      (apply (sub_ids_trans _ _ _ S5); apply sub_ids_ctl; reflexivity).
  Qed.

  Lemma dstep_ids : forall own a j, sub_ids j (fst (dstep own a j)).
  Proof.
    intros own a j. unfold dstep. destruct (j_stop j); [apply sub_ids_ctl; reflexivity |].
    destruct (j_work j) as [| q rest] eqn:Ew.
    - destruct cascades; [| apply sub_ids_ctl; reflexivity].
      destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |] eqn:En; [| apply sub_ids_ctl; reflexivity].
      destruct (cap <=? j_fired j); [apply sub_ids_ctl; reflexivity |]. cbn [fst].
      apply lift_reoffer_ids. intros e He. exact (proj1 (proj2 (proj2 (next_fire_sel _ _ _ _ En)) e He)).
    - destruct (cap <=? j_fired j); [apply sub_ids_ctl; reflexivity |]. cbn [fst].
      assert (Hq : forall id, In id (live (with_work rest j)) \/ id = r_id (p_rec q) -> In id (live j)).
      { intros id [H | E].
        - unfold live in H |- *. cbn [with_work j_view j_work] in H. rewrite Ew. cbn [map].
          apply in_app_or in H as [H | H]; apply in_or_app; [left; exact H | right; right; exact H].
        - unfold live. rewrite Ew. cbn [map]. apply in_or_app. right. left. exact (eq_sym E). }
      destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E.
      + destruct (offer_ids (S (length (v_wtrs (j_view (with_work rest j))))) own (with_work rest j) (resend_elem a q))
          as [L I].
        split; [intros id H; apply Hq; exact (L id H) | exact I].
      + destruct (store_ids own (with_work rest j) (resend_elem a q)) as [L I].
        split; [intros id H; apply Hq; exact (L id H) | exact I].
  Qed.

  Lemma drain_ids : forall fuel own a j, sub_ids j (drain fuel own a j).
  Proof.
    induction fuel as [| f IH]; intros own a j; cbn [drain]; [apply sub_ids_ctl; reflexivity |].
    pose proof (dstep_ids own a j) as S1. destruct (dstep own a j) as [j' []]; cbn [fst] in S1;
      [exact (sub_ids_trans _ _ _ S1 (IH own a j')) | exact S1].
  Qed.

  (* ── 38. The cascade invariant of the head shapes ──────────────────── *)

  (* INV_head (plan 3.6): no waiter is fireable. *)
  Definition INV_head (v : view) : Prop := forall W, In W (v_wtrs v) -> fireable W v = None.

  (* CI: every fireable waiter has a dirty collection, so the cascade
     examines it before the transaction ends. *)
  Definition CI (j : jst) : Prop :=
    forall W, In W (v_wtrs (j_view j)) -> fireable W (j_view j) <> None ->
      exists c, In c (colls W) /\ In c (j_dirty j).

  (* The start of a transaction: distinct identities over the view and the
     pending second sends, distinct waiter identities, and a fresh own
     element. *)
  Definition start_ok (o : op) (v0 : view) : Prop :=
    NoDup (map r_id (v_recs v0) ++ map (fun q => r_id (p_rec q)) (pending_of o)) /\ NoDup (map w_id (v_wtrs v0)) /\
    match o with
    | OProduce x _ _ => ~ In (r_id x) (map r_id (v_recs v0))
    | OReceive W _ _ => ~ In (w_id W) (map w_id (v_wtrs v0))
    | OSettle _ _ _ _ _ => True
    end.

  Lemma CI_ctl : forall j j', CI j -> j_view j' = j_view j -> j_dirty j' = j_dirty j -> CI j'.
  Proof. intros j j' H Vj D W HW Hf. rewrite Vj in HW, Hf. rewrite D. exact (H W HW Hf). Qed.

  Lemma INV_head_CI : forall j, INV_head (j_view j) -> CI j.
  Proof. intros j H W HW Hf. exfalso. exact (Hf (H W HW)). Qed.

  (* A change of the view on the collections D that makes D dirty keeps CI:
     a waiter away from D keeps its fireability. *)
  Lemma CI_change :
    forall j j' D,
      CI j -> incl (v_wtrs (j_view j')) (v_wtrs (j_view j)) ->
      (forall c, ~ In c D -> recs_of c (j_view j') = recs_of c (j_view j) /\ waiters_of c (j_view j') = waiters_of c (j_view j)) ->
      incl D (j_dirty j') -> incl (j_dirty j) (j_dirty j') -> CI j'.
  Proof.
    intros j j' D H Hw Hf HD Hd W HW Hfr.
    destruct (existsb (fun c => mem c D) (colls W)) eqn:E.
    - apply existsb_exists in E as (c & Hc & Hm). apply mem_spec in Hm. exists c. split; [exact Hc | exact (HD c Hm)].
    - assert (Hn : forall c, In c (colls W) -> ~ In c D).
      { intros c Hc Hd'. assert (Ht : existsb (fun c => mem c D) (colls W) = true)
          by (apply existsb_exists; exists c; split; [exact Hc | apply mem_spec; exact Hd']).
        rewrite E in Ht. discriminate. }
      rewrite (fireable_frame W (j_view j) (j_view j')) in Hfr; [| intros c Hc; exact (Hf c (Hn c Hc))].
      destruct (H W (Hw W HW) Hfr) as (c & Hc & Hcd). exists c. split; [exact Hc | exact (Hd c Hcd)].
  Qed.

  (* A fire of W removes records of the collections of W and the waiter W:
     the collections away from the group of W keep their records and their
     waiters. *)
  Lemma frame_remove :
    forall v v' W R c,
      NoDup (map r_id (v_recs v)) -> NoDup (map w_id (v_wtrs v)) ->
      (forall r, In r R -> In r (v_recs v) /\ In (r_coll r) (colls W)) ->
      (In W (v_wtrs v) \/ ~ In (w_id W) (map w_id (v_wtrs v))) ->
      v_recs v' = remove_all R (v_recs v) ->
      v_wtrs v' = (if w_pers W then v_wtrs v else del_wtr (w_id W) (v_wtrs v)) ->
      ~ In c (colls W) ->
      recs_of c v' = recs_of c v /\ waiters_of c v' = waiters_of c v.
  Proof.
    intros v v' W R c Hr Hw HR HW Vr Vw Hc. split.
    - unfold recs_of. rewrite Vr. apply filter_coll_remove_all. intros r e Hr' He E Ec.
      destruct (HR r Hr') as [Hrv Hrc]. assert (e = r) by exact (nodup_map_inj _ _ r_id _ e r Hr He Hrv E). subst e.
      apply Hc. rewrite <- Ec. exact Hrc.
    - unfold waiters_of. rewrite Vw. destruct (w_pers W); [reflexivity |].
      apply filter_mem_del_wtr. intros W' HW' E. apply mem_false. intros Hc'.
      destruct HW as [HW | HW].
      + assert (W' = W) by exact (nodup_map_inj _ _ w_id _ W' W Hw HW' HW E). subst W'. exact (Hc Hc').
      + apply HW. rewrite <- E. apply in_map. exact HW'.
  Qed.

  Lemma CI_touch_all : forall own cs j, CI j -> CI (touch_all own j cs).
  Proof.
    intros own cs j H. destruct (touch_all_spec own cs j) as (bs & _ & Vw & _ & D & _).
    destruct (bumps_view bs (j_view j)) as [Br Bw].
    apply (CI_change j _ [] H); [rewrite Vw, Bw; apply incl_refl | | intros a [] | rewrite D; apply incl_refl].
    intros c _. unfold recs_of, waiters_of. rewrite Vw, Br, Bw. split; reflexivity.
  Qed.

  Lemma CI_store : forall own j x, CI j -> CI (store own j x).
  Proof.
    intros own j x H. destruct (store_spec own j x) as (bs & _ & Vw & _ & D & _).
    destruct (bumps_view bs (j_view j)) as [Br Bw].
    apply (CI_change j _ [r_coll (e_rec x)] H).
    - rewrite Vw, apply_all_app. cbn [apply_all fold_left apply v_wtrs]. rewrite Bw. apply incl_refl.
    - intros c Hc. rewrite Vw, apply_all_app. cbn [apply_all fold_left apply]. unfold recs_of, waiters_of.
      cbn [v_recs v_wtrs filter]. rewrite Br, Bw.
      destruct (Nat.eqb_spec (r_coll (e_rec x)) c) as [E | E]; [exfalso; apply Hc; left; exact E |].
      split; reflexivity.
    - rewrite D. intros a [<- | []]. left. reflexivity.
    - rewrite D. intros a Ha. right. exact Ha.
  Qed.

  (* The park of W puts W into the lane queues of its collections: the head
     of each queue is W or the head before. *)
  Lemma at_head_park :
    forall W W' c v, w_id W' <> w_id W -> at_head W' c (park_view W v) = true -> at_head W' c v = true.
  Proof.
    intros W W' c v Hne H. unfold at_head in *. destruct sh; try reflexivity.
    all: apply forallb_forall; intros L HL; pose proof (proj1 (forallb_forall _ _) H L HL) as H';
      unfold lane_queue in H' |- *; cbn [park_view v_wtrs filter] in H';
      destruct (in_lane W c L); [| exact H'];
      destruct (worder_insert c W (filter (fun W0 => in_lane W0 c L) (v_wtrs v))) as (l1 & l2 & E1 & E2);
      rewrite E2 in H'; rewrite E1;
      (destruct l1 as [| W0 l1]; [cbn [app] in H'; apply Nat.eqb_eq in H'; exfalso; exact (Hne (eq_sym H')) | exact H']).
  Qed.

  Lemma fireable_park :
    forall W W' v, w_id W' <> w_id W -> fireable W' (park_view W v) <> None -> fireable W' v <> None.
  Proof.
    intros W W' v Hne H. unfold fireable in *.
    destruct (heads_ok (colls W') W' (park_view W v)) eqn:E; [| exfalso; exact (H eq_refl)].
    assert (Eh : heads_ok (colls W') W' v = true).
    { unfold heads_ok in *. apply forallb_forall. intros c Hc. apply (at_head_park W W' c v Hne).
      exact (proj1 (forallb_forall _ _) E c Hc). }
    rewrite Eh. unfold receive_decision in *. rewrite (greedy_frame W' v (park_view W v)) in H by (intros c _; reflexivity).
    exact H.
  Qed.

  Lemma CI_park :
    forall j W, CI j -> receive_sel (j_view j) W = None -> ~ In (w_id W) (map w_id (v_wtrs (j_view j))) -> CI (park j W).
  Proof.
    intros j W H Hs Hf W' HW' Hfr. change (In W' (W :: v_wtrs (j_view j))) in HW'.
    change (fireable W' (park_view W (j_view j)) <> None) in Hfr. change (exists c, In c (colls W') /\ In c (j_dirty j)).
    destruct HW' as [<- | HW'].
    - exfalso. apply Hfr. unfold fireable. unfold receive_sel in Hs.
      destruct (heads_ok (colls W) W (park_view W (j_view j))); [| reflexivity].
      unfold receive_decision in *. rewrite (greedy_frame W (j_view j) (park_view W (j_view j))) by (intros c _; reflexivity).
      exact Hs.
    - assert (Hne : w_id W' <> w_id W) by (intros E; apply Hf; rewrite <- E; apply in_map; exact HW').
      exact (H W' HW' (fireable_park W W' (j_view j) Hne Hfr)).
  Qed.

  (* The tail of a fire keeps CI when the collection of each bound record is
     dirty: a retirement removes records of those collections only. *)
  Lemma CI_fire_tail :
    forall W sg inc oi fl j, CI j -> NoDup (map r_id (v_recs (j_view j))) ->
      (forall e, In e (map fst sg) -> In (r_coll e) (j_dirty j)) -> CI (fire_tail W sg inc oi fl j).
  Proof.
    intros W sg inc oi fl j H Hnd Hd.
    destruct (fire_tail_spec W sg inc oi fl j) as (rs & _ & Vj & _ & D & _ & _ & _ & _ & Hrs & _).
    apply (CI_change j _ (map r_coll rs) H).
    - rewrite Vj. apply incl_refl.
    - intros c Hc. unfold recs_of, waiters_of. rewrite Vj. cbn [v_recs v_wtrs]. split; [| reflexivity].
      apply filter_coll_remove_all. intros r e Hr He E Ec. apply Hc.
      destruct (Hrs r Hr) as (Hrv & _ & _). assert (e = r) by exact (nodup_map_inj _ _ r_id _ e r Hnd He Hrv E).
      subst e. rewrite <- Ec. apply in_map. exact Hr.
    - intros a Ha. apply in_map_iff in Ha as (r & <- & Hr). rewrite D.
      destruct (Hrs r Hr) as (_ & _ & k & e0 & m0 & Hk & Hc). rewrite Hc. apply Hd. apply in_map_iff.
      exists (e0, m0). split; [reflexivity | exact (nth_error_In _ _ Hk)].
    - rewrite D. apply incl_refl.
  Qed.

  (* NEXT-FIRE completeness: when it finds nothing, no stored waiter with a
     dirty collection is fireable. *)
  Lemma next_fire_none :
    forall d v W c, next_fire d v = None -> In W (v_wtrs v) -> In c (colls W) -> In c d -> fireable W v = None.
  Proof.
    intros d v W c H HW Hc Hd. destruct (fireable W v) as [sg |] eqn:Ef; [exfalso | reflexivity].
    unfold next_fire in H.
    assert (Hcs : In c (isort Nat.leb (nodup Nat.eq_dec d))).
    { apply (Permutation_in _ (Permutation_sym (isort_perm _ Nat.leb _))). apply nodup_In. exact Hd. }
    assert (G : forall cs, next_fire_in cs v = None -> In c cs -> find_fire (heads_of c v) v = None).
    { induction cs as [| c0 r IH]; intros Hn Hin; [destruct Hin |]. cbn [next_fire_in] in Hn.
      destruct (find_fire (heads_of c0 v) v) as [f |] eqn:Ef0; [discriminate |].
      destruct Hin as [<- | Hin]; [exact Ef0 | exact (IH Hn Hin)]. }
    pose proof (G _ H Hcs) as Hf.
    assert (Hh : In W (heads_of c v)).
    { unfold heads_of. apply filter_In. split.
      - apply (Permutation_in _ (Permutation_sym (worder_perm c _))). unfold waiters_of. apply filter_In.
        split; [exact HW | apply mem_spec; exact Hc].
      - unfold fireable in Ef. destruct (heads_ok (colls W) W v) eqn:Eh; [| discriminate].
        unfold heads_ok in Eh. exact (proj1 (forallb_forall _ _) Eh c Hc). }
    assert (G2 : forall ws, find_fire ws v = None -> In W ws -> False).
    { induction ws as [| W1 r1 IH1]; intros Hn Hin; [destruct Hin |]. cbn [find_fire] in Hn.
      destruct (fireable W1 v) as [sg1 |] eqn:E1; [discriminate |].
      destruct Hin as [<- | Hin]; [rewrite Ef in E1; discriminate | exact (IH1 Hn Hin)]. }
    exact (G2 _ Hf Hh).
  Qed.

  Lemma CI_quiescent : forall j, CI j -> next_fire (j_dirty j) (j_view j) = None -> INV_head (j_view j).
  Proof.
    intros j H Hn W HW. destruct (fireable W (j_view j)) as [sg |] eqn:Ef; [| reflexivity].
    destruct (H W HW) as (c & Hc & Hd); [rewrite Ef; discriminate |].
    rewrite <- Ef. exact (next_fire_none _ _ W c Hn HW Hc Hd).
  Qed.

  (* The end of a drain without a stop: no pending work, and NEXT-FIRE finds
     nothing (a discipline that cascades). *)
  Lemma dstep_false :
    forall own a j, snd (dstep own a j) = false -> j_stop (fst (dstep own a j)) = None ->
      j_work (fst (dstep own a j)) = [] /\
      (cascades = true -> next_fire (j_dirty (fst (dstep own a j))) (j_view (fst (dstep own a j))) = None).
  Proof.
    intros own a j. unfold dstep. destruct (j_stop j) as [c |] eqn:Es; cbn [fst snd].
    { intros _ H. rewrite Es in H. discriminate. }
    destruct (j_work j) as [| q rest] eqn:Ew.
    - destruct cascades eqn:Esc; cbn [fst snd]; [| intros _ _; split; [exact Ew | intros H; discriminate]].
      destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |] eqn:En; cbn [fst snd].
      + destruct (cap <=? j_fired j); cbn [fst snd]; [intros _ H; cbn in H; discriminate | intros H; discriminate].
      + intros _ _. split; [exact Ew | intros _; exact En].
    - destruct (cap <=? j_fired j); cbn [fst snd]; [intros _ H; cbn in H; discriminate | intros H; discriminate].
  Qed.

  Lemma drain_end :
    forall fuel own a j, j_stop (drain fuel own a j) = None ->
      j_work (drain fuel own a j) = [] /\
      (cascades = true -> next_fire (j_dirty (drain fuel own a j)) (j_view (drain fuel own a j)) = None).
  Proof.
    induction fuel as [| f IH]; intros own a j H; cbn [drain] in H |- *; [cbn in H; discriminate |].
    pose proof (dstep_false own a j) as Hd. revert H Hd.
    destruct (dstep own a j) as [j' []]; cbn [fst snd]; intros H Hd; [exact (IH own a j' H) | exact (Hd eq_refl H)].
  Qed.

  Lemma run_join_work :
    forall fuel o scope v0, j_stop (fst (run_join fuel o scope v0)) = None -> j_work (fst (run_join fuel o scope v0)) = [].
  Proof.
    intros fuel o scope v0. unfold run_join.
    set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
    destruct (is_settle o); [cbn [fst]; intros H; exact (proj1 (drain_end _ _ _ _ H)) |].
    set (j3 := if cascades then drain fuel (own_of o) (auth_of o) (with_dirty scope j1) else j1).
    destruct (j_stop j3) eqn:Es; [cbn [fst]; intros H; rewrite Es in H; discriminate |].
    destruct (plan_at_cap (j_fired (with_dirty [] j3))); [cbn [fst]; intros H; cbn in H; discriminate |].
    cbn [fst]. intros H. exact (proj1 (drain_end _ _ _ _ H)).
  Qed.

  Section CascadeRule.
    (* The rule of the cascade (plan 4.9): a fire makes every collection of
       its group dirty. Headline 9 discharges it from fire_dirty. *)
    Hypothesis fire_dirty_covers : forall W c, incl (colls W) (fire_dirty W c).

    Lemma CI_fire :
      forall j j3 W R c_in,
        CI j -> WF j ->
        (forall r, In r R -> In r (v_recs (j_view j)) /\ In (r_coll r) (colls W)) ->
        (In W (v_wtrs (j_view j)) \/ ~ In (w_id W) (map w_id (v_wtrs (j_view j)))) ->
        v_recs (j_view j3) = remove_all R (v_recs (j_view j)) ->
        v_wtrs (j_view j3) = (if w_pers W then v_wtrs (j_view j) else del_wtr (w_id W) (v_wtrs (j_view j))) ->
        j_dirty j3 = fire_dirty W c_in ++ j_dirty j -> CI j3.
    Proof.
      intros j j3 W R c_in H Hwf HR HW Vr Vw D.
      apply (CI_change j j3 (colls W) H).
      - rewrite Vw. destruct (w_pers W); [apply incl_refl | apply del_wtr_incl].
      - intros c Hc. exact (frame_remove (j_view j) (j_view j3) W R c (WF_recs j Hwf) (proj2 Hwf) HR HW Vr Vw Hc).
      - rewrite D. intros a Ha. apply in_or_app. left. exact (fire_dirty_covers W c_in a Ha).
      - rewrite D. intros a Ha. apply in_or_app. right. exact Ha.
    Qed.

    Lemma CI_offer1 :
      forall own j x, CI j -> WF j -> ~ In (r_id (e_rec x)) (live j) -> CI (fst (offer1 own j x)).
    Proof.
      intros own j x H Hwf Hx. unfold offer1.
      destruct (offer_pick (e_rec x) (j_view j) (offer_cands (e_rec x) (j_view j))) as [[[W p] sg] |] eqn:Ep;
        [| exact (CI_store own j x H)].
      destruct (proj1 join_fires_heads_only _ _ _ _ _ Ep) as (_ & HwIn & Hrel & mx & Hxp).
      destruct (waiters_of_coll _ _ _ (offer_pick_in _ _ _ _ _ Ep)) as [_ Hcx].
      assert (Hsp : search_produce W (e_rec x) (j_view j) = Some (p, sg)).
      { assert (G : forall ws, offer_pick (e_rec x) (j_view j) ws = Some (W, p, sg) -> fires_with W (e_rec x) (j_view j) = Some (p, sg)).
        { induction ws as [| W0 r IH]; intros Hp; [discriminate |]. cbn [offer_pick] in Hp.
          destruct (fires_with W0 (e_rec x) (j_view j)) as [[p0 sg0] |] eqn:Ef; [injection Hp as <- <- <-; exact Ef | exact (IH Hp)]. }
        pose proof (G _ Ep) as Hfw. unfold fires_with in Hfw.
        destruct (heads_ok (heads_required W (e_rec x)) W (j_view j)); [| discriminate].
        destruct (search_produce W (e_rec x) (j_view j)) as [[p0 sg0] |]; [| discriminate].
        destruct (guard_ok W sg0); [injection Hfw as -> ->; reflexivity | discriminate]. }
      destruct (proj2 (proj2 join_repeated_channel_distinct_records) W (e_rec x) (j_view j) p sg Hsp) as [_ Hndr].
      destruct (touch_all_spec own (colls W) j) as (bs & _ & V1 & _ & D1 & _).
      destruct (bumps_view bs (j_view j)) as [Br Bw].
      set (j3 := add_dirty (fire_dirty W (r_coll (e_rec x)))
                   (with_fired (S (j_fired j)) (stage (touch_all own j (colls W)) (SFire W sg (Some (p, e_auth x)))))).
      assert (H3 : CI j3).
      { apply (CI_fire j j3 W (removed sg (Some (p, e_auth x))) (r_coll (e_rec x)) H Hwf).
        - intros r Hr. destruct (removed_pos _ _ _ Hr) as (k & m & Hk & Hi).
          destruct (slots_prod_colls W (e_rec x) p (j_view j) sg Hrel r (in_map fst _ _ (nth_error_In _ _ Hk))) as [-> | Hr'];
            [| exact Hr'].
          exfalso. cbn [inc_pos] in Hi. apply Nat.eqb_neq in Hi. apply Hi.
          apply (NoDup_nth_error (map fst sg)); [exact Hndr | rewrite length_map; apply nth_error_Some; rewrite Hk; discriminate |].
          rewrite !nth_error_map, Hk, Hxp. reflexivity.
        - left. exact HwIn.
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_recs]. rewrite V1, Br. reflexivity.
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs]. rewrite V1, Bw. reflexivity.
        - unfold j3. cbn [add_dirty with_dirty with_fired stage j_dirty]. rewrite D1. reflexivity. }
      assert (Hnd3 : NoDup (map r_id (v_recs (j_view j3)))).
      { unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_recs]. rewrite V1, Br.
        apply nodup_ids_remove_all. exact (WF_recs j Hwf). }
      assert (Hd3 : forall e, In e (map fst sg) -> In (r_coll e) (j_dirty j3)).
      { intros e He. unfold j3. cbn [add_dirty with_dirty with_fired stage j_dirty]. apply in_or_app. left.
        apply fire_dirty_covers.
        destruct (slots_prod_colls W (e_rec x) p (j_view j) sg Hrel e He) as [-> | [_ Hc]]; [exact Hcx | exact Hc]. }
      pose proof (CI_fire_tail W sg (Some p) (e_own x) true j3 H3 Hnd3 Hd3) as H4.
      destruct (w_peek W); [exact H4 |].
      destruct (after_fire (S (j_fired j)) (negb (r_pers (e_rec x)) && negb false) (w_pers W));
        cbn [fst]; unfold own_flight; try destruct (e_own x); (apply (CI_ctl _ _ H4); reflexivity).
    Qed.

    Lemma CI_offer :
      forall fuel own j x, CI j -> WF j -> ~ In (r_id (e_rec x)) (live j) -> CI (offer fuel own j x).
    Proof.
      induction fuel as [| f IH]; intros own j x H Hwf Hx; cbn [offer].
      - unfold own_flight. destruct (e_own x); (apply (CI_ctl _ _ H); reflexivity).
      - pose proof (CI_offer1 own j x H Hwf Hx) as H1. destruct (WF_offer1 own j x Hwf Hx) as [W1 F1].
        destruct (offer1 own j x) as [j' []]; cbn [fst snd] in H1, W1, F1; [exact (IH own j' x H1 W1 (F1 eq_refl)) | exact H1].
    Qed.

    Lemma CI_receive1 :
      forall j W, CI j -> WF j -> ~ In (w_id W) (map w_id (v_wtrs (j_view j))) -> CI (fst (receive1 j W)).
    Proof.
      intros j W H Hwf Hw. unfold receive1.
      destruct (receive_sel (j_view j) W) as [sg |] eqn:Er; [| exact (CI_park j W H Er Hw)].
      assert (Hgr : greedy W (j_view j) = Some sg).
      { unfold receive_sel in Er. destruct (heads_ok (colls W) W (park_view W (j_view j))); [| discriminate].
        unfold receive_decision in Er. destruct (greedy W (j_view j)) as [sg' |]; [| discriminate].
        destruct (guard_ok W sg'); [injection Er as <-; reflexivity | discriminate]. }
      set (j3 := add_dirty (fire_dirty W (hd 0 (colls W))) (with_fired (S (j_fired j)) (stage j (SFire W sg None)))).
      assert (H3 : CI j3).
      { apply (CI_fire j j3 W (removed sg None) (hd 0 (colls W)) H Hwf); [| right; exact Hw | reflexivity | reflexivity | reflexivity].
        intros r Hr. apply (greedy_colls W (j_view j) sg Hgr r). destruct (removed_pos _ _ _ Hr) as (k & m & Hk & _).
        exact (in_map fst _ _ (nth_error_In _ _ Hk)). }
      assert (Hnd3 : NoDup (map r_id (v_recs (j_view j3)))).
      { unfold j3. cbn [add_dirty with_dirty with_fired stage j_view apply v_recs]. apply nodup_ids_remove_all. exact (WF_recs j Hwf). }
      assert (Hd3 : forall e, In e (map fst sg) -> In (r_coll e) (j_dirty j3)).
      { intros e He. unfold j3. cbn [add_dirty with_dirty with_fired stage j_dirty]. apply in_or_app. left.
        apply fire_dirty_covers. exact (proj2 (greedy_colls W (j_view j) sg Hgr e He)). }
      pose proof (CI_fire_tail W sg None false false j3 H3 Hnd3 Hd3) as H4.
      destruct (after_fire (S (j_fired j)) (negb (w_pers W)) (rcv_stays W sg (j_view j)));
        cbn [fst]; (apply (CI_ctl _ _ H4); reflexivity).
    Qed.

    Lemma CI_receive :
      forall fuel j W, CI j -> WF j -> ~ In (w_id W) (map w_id (v_wtrs (j_view j))) -> CI (receive fuel j W).
    Proof.
      induction fuel as [| f IH]; intros j W H Hwf Hw; cbn [receive]; [apply (CI_ctl _ _ H); reflexivity |].
      pose proof (CI_receive1 j W H Hwf Hw) as H1. destruct (WF_receive1 j W Hwf Hw) as [W1 F1].
      destruct (receive1 j W) as [j' []]; cbn [fst snd] in H1, W1, F1; [exact (IH j' W H1 W1 (F1 eq_refl)) | exact H1].
    Qed.

    Lemma CI_lift_reoffer :
      forall own j W sg, CI j -> WF j -> In W (v_wtrs (j_view j)) ->
        (forall e, In e (map fst sg) -> In e (v_recs (j_view j)) /\ In (r_coll e) (colls W)) ->
        CI (lift_reoffer own j W sg).
    Proof.
      intros own j W sg H Hwf HwIn Hsg. unfold lift_reoffer.
      destruct (nth_error sg (reoffer_pos sg)) as [[rv mv] |] eqn:Hn; [| exact H].
      destruct (touch_all_spec own (colls W) j) as (bs & _ & V1 & _ & D1 & _).
      destruct (stage_all_fields (lift_ops rv) (touch_all own j (colls W))) as (_ & V2 & _ & D2 & _).
      set (R := (if r_pers rv then [] else [rv]) ++ removed sg (Some (reoffer_pos sg, own_auth rv))).
      set (j4 := add_dirty (fire_dirty W (r_coll rv)) (with_fired (S (j_fired j))
                   (stage (stage_all (touch_all own j (colls W)) (lift_ops rv)) (SFire W sg (Some (reoffer_pos sg, own_auth rv)))))).
      assert (Hv4 : v_recs (j_view j4) = remove_all R (v_recs (j_view j))).
      { unfold j4. cbn [add_dirty with_dirty with_fired stage j_view]. rewrite V2, V1.
        exact (lift_fire_recs (j_view j) bs W sg (reoffer_pos sg) (own_auth rv) rv mv Hn). }
      assert (H4 : CI j4).
      { apply (CI_fire j j4 W R (r_coll rv) H Hwf).
        - intros r Hr. unfold R in Hr. apply in_app_or in Hr as [Hr | Hr].
          + destruct (r_pers rv); [destruct Hr | destruct Hr as [<- | []]]. apply Hsg. apply in_map_iff. exists (rv, mv).
            split; [reflexivity | exact (nth_error_In _ _ Hn)].
          + destruct (removed_pos _ _ _ Hr) as (k & m & Hk & _). exact (Hsg r (in_map fst _ _ (nth_error_In _ _ Hk))).
        - left. exact HwIn.
        - exact Hv4.
        - unfold j4. cbn [add_dirty with_dirty with_fired stage j_view apply v_wtrs]. rewrite V2, V1.
          assert (Hl : forall v0, v_wtrs (apply_all v0 (lift_ops rv)) = v_wtrs v0)
            by (intros v0; unfold lift_ops; destruct (r_pers rv); reflexivity).
          rewrite Hl, (proj2 (bumps_view bs (j_view j))). reflexivity.
        - unfold j4. cbn [add_dirty with_dirty with_fired stage j_dirty]. rewrite D2, D1. reflexivity. }
      assert (Hnd4 : NoDup (map r_id (v_recs (j_view j4)))) by (rewrite Hv4; apply nodup_ids_remove_all; exact (WF_recs j Hwf)).
      assert (Hd4 : forall e, In e (map fst sg) -> In (r_coll e) (j_dirty j4)).
      { intros e He. unfold j4. cbn [add_dirty with_dirty with_fired stage j_dirty]. apply in_or_app. left.
        apply fire_dirty_covers. exact (proj2 (Hsg e He)). }
      pose proof (CI_fire_tail W sg (Some (reoffer_pos sg)) false (negb (r_pers rv)) j4 H4 Hnd4 Hd4) as H5.
      destruct (after_fire (S (j_fired j)) (negb (r_pers rv) && negb (w_peek W)) (w_pers W));
        try exact H5; (apply (CI_ctl _ _ H5); reflexivity).
    Qed.

    Lemma CI_dstep : forall own a j, CI j -> WF j -> CI (fst (dstep own a j)).
    Proof.
      intros own a j H Hwf. unfold dstep. destruct (j_stop j); [exact H |].
      destruct (j_work j) as [| q rest] eqn:Ew.
      - destruct cascades; [| exact H].
        destruct (next_fire (j_dirty j) (j_view j)) as [[W sg] |] eqn:En; [| exact H].
        destruct (cap <=? j_fired j); [cbn [fst]; apply (CI_ctl _ _ H); reflexivity |]. cbn [fst].
        destruct (next_fire_sel _ _ _ _ En) as (HwIn & _ & Hsg).
        exact (CI_lift_reoffer own j W sg H Hwf HwIn Hsg).
      - destruct (cap <=? j_fired j); [cbn [fst]; apply (CI_ctl _ _ H); reflexivity |]. cbn [fst].
        destruct (WF_pop j q rest Hwf Ew) as [W0 F0].
        assert (H0 : CI (with_work rest j)) by (apply (CI_ctl _ _ H); reflexivity).
        destruct (resend_cases own a (with_work rest j) q) as [E | E]; rewrite E;
          [exact (CI_offer _ own (with_work rest j) (resend_elem a q) H0 W0 F0) | exact (CI_store own _ _ H0)].
    Qed.

    Lemma CI_drain : forall fuel own a j, CI j -> WF j -> CI (drain fuel own a j).
    Proof.
      induction fuel as [| f IH]; intros own a j H Hwf; cbn [drain]; [apply (CI_ctl _ _ H); reflexivity |].
      pose proof (CI_dstep own a j H Hwf) as H1. pose proof (WF_dstep own a j Hwf) as W1.
      destruct (dstep own a j) as [j' []]; cbn [fst] in H1, W1; [exact (IH own a j' H1 W1) | exact H1].
    Qed.

    (* RUN-JOIN on a discipline that cascades: CI holds at its end, and
       INV_head holds at an end with no stop. *)
    Lemma CI_run_join :
      forall fuel o scope v0,
        cascades = true -> start_ok o v0 ->
        (forall W, In W (v_wtrs v0) -> fireable W v0 <> None -> exists c, In c (colls W) /\ In c scope) ->
        CI (fst (run_join fuel o scope v0)) /\
        (j_stop (fst (run_join fuel o scope v0)) = None -> INV_head (j_view (fst (run_join fuel o scope v0)))).
    Proof.
      intros fuel o scope v0 Hcas (Nd & Nw & Hown) H0. unfold run_join.
      set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
      destruct (touch_all_spec (own_of o) (own_of o) (init v0)) as (bs & _ & V1 & _).
      assert (Vr1 : v_recs (j_view j1) = v_recs v0) by (unfold j1; cbn [with_work j_view]; rewrite V1; exact (proj1 (bumps_view bs v0))).
      assert (Vw1 : v_wtrs (j_view j1) = v_wtrs v0) by (unfold j1; cbn [with_work j_view]; rewrite V1; exact (proj2 (bumps_view bs v0))).
      set (j2 := with_dirty scope j1).
      assert (L2 : live j2 = map r_id (v_recs v0) ++ map (fun q => r_id (p_rec q)) (pending_of o)).
      { unfold live. change (j_view j2) with (j_view j1). change (j_work j2) with (pending_of o). rewrite Vr1. reflexivity. }
      assert (Wf2 : WF j2) by (split; [rewrite L2; exact Nd | change (j_view j2) with (j_view j1); rewrite Vw1; exact Nw]).
      assert (C2 : CI j2).
      { intros W HW Hf. change (j_view j2) with (j_view j1) in HW, Hf. change (j_dirty j2) with scope. rewrite Vw1 in HW.
        rewrite (fireable_frame W v0 (j_view j1)) in Hf by (intros c _; unfold recs_of, waiters_of; rewrite Vr1, Vw1; split; reflexivity).
        exact (H0 W HW Hf). }
      pose proof (CI_drain fuel (own_of o) (auth_of o) j2 C2 Wf2) as C3.
      pose proof (WF_drain fuel (own_of o) (auth_of o) j2 Wf2) as Wf3.
      pose proof (drain_ids fuel (own_of o) (auth_of o) j2) as S3.
      set (j3 := drain fuel (own_of o) (auth_of o) j2) in *.
      assert (Q3 : j_stop j3 = None -> j_work j3 = [] /\ INV_head (j_view j3)).
      { intros Hs. destruct (drain_end fuel (own_of o) (auth_of o) j2 Hs) as [Hw Hn].
        split; [exact Hw | exact (CI_quiescent j3 C3 (Hn Hcas))]. }
      destruct (is_settle o); [cbn [fst]; split; [exact C3 | intros Hs; exact (proj2 (Q3 Hs))] |].
      replace (if cascades then j3 else j1) with j3 by (rewrite Hcas; reflexivity).
      destruct (j_stop j3) eqn:Es3.
      { cbn [fst]. split; [exact C3 | intros Hs; rewrite Es3 in Hs; discriminate]. }
      destruct (Q3 eq_refl) as [Wk3 I3].
      assert (C4' : CI (with_dirty [] j3)) by (apply INV_head_CI; exact I3).
      destruct (plan_at_cap (j_fired (with_dirty [] j3))).
      { cbn [fst]. split; [apply (CI_ctl _ _ C4'); reflexivity | intros Hs; cbn in Hs; discriminate]. }
      cbn [fst].
      set (j4 := with_fired (phase_count (j_fired (with_dirty [] j3))) (with_dirty [] j3)).
      assert (V4 : j_view j4 = j_view j3) by reflexivity.
      assert (Wk4 : j_work j4 = j_work j3) by reflexivity.
      assert (C4 : CI j4) by (apply INV_head_CI; rewrite V4; exact I3).
      assert (Wf4 : WF j4).
      { apply (WF_same j3 j4 Wf3); [unfold live; rewrite V4, Wk4; apply Permutation_refl | rewrite V4; exact (proj2 Wf3)]. }
      assert (S4 : sub_ids j2 j4) by (apply (sub_ids_trans _ _ _ S3); apply sub_ids_ctl; [exact V4 | exact Wk4]).
      assert (C5 : CI (plan fuel o v0 j4) /\ WF (plan fuel o v0 j4)).
      { destruct o as [x c a | W c a | key sd pd rs a]; cbn [plan].
        - assert (Hx : ~ In (r_id x) (live j4)).
          { intros Hin. apply (proj1 S4) in Hin. rewrite L2 in Hin. cbn [pending_of map] in Hin. rewrite app_nil_r in Hin.
            exact (Hown Hin). }
          split; [apply CI_offer | apply WF_offer]; try exact C4; try exact Wf4; exact Hx.
        - assert (Hw : ~ In (w_id W) (map w_id (v_wtrs (j_view j4)))).
          { intros Hin. apply Hown. apply in_map_iff in Hin as (W' & E & HW'). apply (proj2 S4) in HW'.
            change (j_view j2) with (j_view j1) in HW'. rewrite Vw1 in HW'. rewrite <- E. apply in_map. exact HW'. }
          split; [apply CI_receive | apply WF_receive]; try exact C4; try exact Wf4; exact Hw.
        - split; [exact C4 | exact Wf4]. }
      destruct C5 as [C5 Wf5].
      set (j5 := plan fuel o v0 j4) in *.
      set (j6 := with_fired (phase_count (j_fired j5)) j5).
      assert (C6 : CI j6) by (apply (CI_ctl _ _ C5); reflexivity).
      assert (Wf6 : WF j6) by (apply (WF_same j5 j6 Wf5); [apply Permutation_refl | exact (proj2 Wf5)]).
      pose proof (CI_drain fuel (own_of o) (auth_of o) j6 C6 Wf6) as C7.
      split; [exact C7 |].
      intros Hs. destruct (drain_end fuel (own_of o) (auth_of o) j6 Hs) as [_ Hn].
      exact (CI_quiescent _ C7 (Hn Hcas)).
    Qed.
  End CascadeRule.

  (* RUN-JOIN on a bag keeps INV_scan at every end. *)
  Lemma INV_scan_run_join :
    sh = Scan -> forall fuel o scope v0, start_ok o v0 -> INV_scan v0 -> INV_scan (j_view (fst (run_join fuel o scope v0))).
  Proof.
    intros Hsh fuel o scope v0 (Nd & Nw & Hown) H0. pose proof (scan_no_cascade Hsh) as Hsc.
    unfold run_join.
    set (j1 := with_work (pending_of o) (touch_all (own_of o) (init v0) (own_of o))).
    destruct (touch_all_spec (own_of o) (own_of o) (init v0)) as (bs & _ & V1 & _).
    assert (Vr1 : v_recs (j_view j1) = v_recs v0) by (unfold j1; cbn [with_work j_view]; rewrite V1; exact (proj1 (bumps_view bs v0))).
    assert (Vw1 : v_wtrs (j_view j1) = v_wtrs v0) by (unfold j1; cbn [with_work j_view]; rewrite V1; exact (proj2 (bumps_view bs v0))).
    assert (H1 : INV_scan (j_view j1)) by exact (INV_scan_ctl v0 _ H0 Vr1 Vw1).
    assert (L1 : live j1 = map r_id (v_recs v0) ++ map (fun q => r_id (p_rec q)) (pending_of o)).
    { unfold live. change (j_work j1) with (pending_of o). rewrite Vr1. reflexivity. }
    assert (Wf1 : WF j1) by (split; [rewrite L1; exact Nd | rewrite Vw1; exact Nw]).
    destruct (is_settle o); [cbn [fst]; apply INV_scan_drain; [exact Hsh | exact H1 | exact Wf1] |].
    replace (if cascades then drain fuel (own_of o) (auth_of o) (with_dirty scope j1) else j1) with j1
      by (rewrite Hsc; reflexivity).
    destruct (j_stop j1); [cbn [fst]; exact H1 |].
    destruct (plan_at_cap (j_fired (with_dirty [] j1))); [cbn [fst]; exact H1 |].
    cbn [fst].
    set (j4 := with_fired (phase_count (j_fired (with_dirty [] j1))) (with_dirty [] j1)).
    assert (Wf4 : WF j4) by (apply (WF_same j1 j4 Wf1); [apply Permutation_refl | exact (proj2 Wf1)]).
    assert (H4 : INV_scan (j_view j4)) by exact H1.
    assert (H5 : INV_scan (j_view (plan fuel o v0 j4)) /\ WF (plan fuel o v0 j4)).
    { destruct o as [x c a | W c a | key sd pd rs a]; cbn [plan].
      - assert (Hx : ~ In (r_id x) (live j4)).
        { change (live j4) with (live j1). rewrite L1. cbn [pending_of map]. rewrite app_nil_r. exact Hown. }
        split; [apply INV_scan_offer | apply WF_offer]; try exact Hsh; try exact H4; try exact Wf4; exact Hx.
      - assert (Hw : ~ In (w_id W) (map w_id (v_wtrs (j_view j4)))).
        { change (j_view j4) with (j_view j1). rewrite Vw1. exact Hown. }
        split; [apply INV_scan_receive | apply WF_receive]; try exact Hsh; try exact H4; try exact Wf4; exact Hw.
      - split; [exact H4 | exact Wf4]. }
    destruct H5 as [H5 W5].
    apply INV_scan_drain; [exact Hsh | exact H5 | exact W5].
  Qed.

  (* ── 39. Headline 9: the cascade restores the invariant ──────────────── *)

  (* Headline 9 (plan 3.6 and 4.9, decisions 11, 37 and 50). Let a
     transaction start with distinct identities and a fresh own element.
     Head shapes (queue, stack, cell, priority queue) and the set: let every
     fireable waiter of the start have a collection in the scope (the
     cascade seeds, which hold at once when the start satisfies INV_head).
     Then every fireable waiter at the end has a dirty collection (CI). A
     stopped transaction carries its dirty collections as the seeds of its
     settle job. A transaction that ends with no stop ends with no work and
     with INV_head: no waiter is fireable, guarded waiters included, because
     the cascade fires every fireable waiter. The set needs the cascade
     because its classes move (finding F-JF-1, decision 50), see
     set_join_class_shift_breaks_inv_scan after the section. Bag: INV_scan
     at the start gives INV_scan at the end (the monotonicity of greedy and
     the position retry), with no cascade. *)
  Theorem cascade_restores_inv :
    forall fuel o scope v0,
      start_ok o v0 ->
      (sh <> Scan -> forall W, In W (v_wtrs v0) -> fireable W v0 <> None -> exists c, In c (colls W) /\ In c scope) ->
      (sh = Scan -> INV_scan v0) ->
      (sh <> Scan -> CI (fst (run_join fuel o scope v0))) /\
      (sh = Scan -> INV_scan (j_view (fst (run_join fuel o scope v0)))) /\
      (j_stop (fst (run_join fuel o scope v0)) = None ->
         j_work (fst (run_join fuel o scope v0)) = [] /\
         (sh <> Scan -> INV_head (j_view (fst (run_join fuel o scope v0))))).
  Proof.
    intros fuel o scope v0 Hst Hci Hsc.
    assert (Hcov : forall W c, incl (colls W) (fire_dirty W c)) by (intros W c a Ha; unfold fire_dirty; exact Ha).
    (* the head shapes and the set cascade (decisions 11 and 50) *)
    assert (Hcas : sh <> Scan -> cascades = true).
    { intros Hs. unfold cascades. destruct sh; [reflexivity | reflexivity | exfalso; exact (Hs eq_refl) | reflexivity]. }
    split; [| split].
    - intros Hh. exact (proj1 (CI_run_join Hcov fuel o scope v0 (Hcas Hh) Hst (Hci Hh))).
    - intros Hs. exact (INV_scan_run_join Hs fuel o scope v0 Hst (Hsc Hs)).
    - intros Hstop. split; [exact (run_join_work fuel o scope v0 Hstop) |].
      intros Hh. exact (proj2 (CI_run_join Hcov fuel o scope v0 (Hcas Hh) Hst (Hci Hh)) Hstop).
  Qed.

End Join.

(* ── 40. A valid parameterization and two computed witnesses ─────────── *)

(* The payloads are numbers, a pattern None matches every payload and a
   pattern Some w matches the payload w, the guard admits everything, and
   the visit order sorts the records by a total order (level first, as
   DR-75 orders by a key). The head order of the waiters is their list
   order. These parameters satisfy every hypothesis of the section
   (wit_parameters_valid), so the witnesses below are not artifacts of an
   order that no discipline could have. *)
Definition wit_spatial (p : option nat) (x : nat) : option nat :=
  match p with None => Some x | Some w => if w =? x then Some x else None end.

Definition wit_admits (g : unit) (ms : list nat) : bool := true.

(* The lexicographic order of lists of numbers. *)
Fixpoint lex (a b : list nat) : bool :=
  match a, b with
  | [], _ => true
  | _ :: _, [] => false
  | x :: a', y :: b' => (x <? y) || ((x =? y) && lex a' b')
  end.

Lemma lex_total : forall a b, length a = length b -> lex a b = true \/ lex b a = true.
Proof.
  induction a as [| x a IH]; intros [| y b] Hl; cbn [lex length] in Hl |- *; try discriminate; [left; reflexivity |].
  destruct (Nat.lt_trichotomy x y) as [Hxy | [-> | Hxy]].
  - left. apply orb_true_iff. left. apply Nat.ltb_lt. exact Hxy.
  - rewrite Nat.ltb_irrefl, Nat.eqb_refl. cbn [orb andb]. apply IH. lia.
  - right. apply orb_true_iff. left. apply Nat.ltb_lt. exact Hxy.
Qed.

Lemma lex_antisym : forall a b, lex a b = true -> lex b a = true -> a = b.
Proof.
  induction a as [| x a IH]; intros [| y b] H1 H2; cbn [lex] in H1, H2; try discriminate; [reflexivity |].
  apply orb_true_iff in H1, H2. destruct H1 as [H1 | H1]; destruct H2 as [H2 | H2].
  - apply Nat.ltb_lt in H1, H2. lia.
  - apply Nat.ltb_lt in H1. apply andb_true_iff in H2 as [H2 _]. apply Nat.eqb_eq in H2. lia.
  - apply Nat.ltb_lt in H2. apply andb_true_iff in H1 as [H1 _]. apply Nat.eqb_eq in H1. lia.
  - apply andb_true_iff in H1 as [E1 H1]. apply andb_true_iff in H2 as [_ H2]. apply Nat.eqb_eq in E1. subst y.
    f_equal. exact (IH b H1 H2).
Qed.

Lemma lex_trans : forall a b c, lex a b = true -> lex b c = true -> lex a c = true.
Proof.
  induction a as [| x a IH]; intros [| y b] [| z c] H1 H2; cbn [lex] in H1, H2 |- *; try discriminate; try reflexivity.
  apply orb_true_iff in H1, H2. apply orb_true_iff.
  destruct H1 as [H1 | H1]; destruct H2 as [H2 | H2].
  - left. apply Nat.ltb_lt in H1, H2. apply Nat.ltb_lt. lia.
  - left. apply Nat.ltb_lt in H1. apply andb_true_iff in H2 as [H2 _]. apply Nat.eqb_eq in H2. apply Nat.ltb_lt. lia.
  - left. apply Nat.ltb_lt in H2. apply andb_true_iff in H1 as [H1 _]. apply Nat.eqb_eq in H1. apply Nat.ltb_lt. lia.
  - right. apply andb_true_iff in H1 as [E1 H1]. apply andb_true_iff in H2 as [E2 H2]. apply andb_true_iff. split.
    + apply Nat.eqb_eq in E1, E2. apply Nat.eqb_eq. lia.
    + exact (IH b c H1 H2).
Qed.

(* The key of a record: all its fields, the level first. *)
Definition rkey (r : rec nat) : list nat :=
  [r_lvl nat r; r_id nat r; r_coll nat r; r_ord nat r; r_val nat r; if r_pers nat r then 1 else 0].

Definition rle (a b : rec nat) : bool := lex (rkey a) (rkey b).

Lemma rle_total : forall a b, rle a b = true \/ rle b a = true.
Proof. intros a b. apply lex_total. reflexivity. Qed.

Lemma rle_antisym : forall a b, rle a b = true -> rle b a = true -> a = b.
Proof.
  intros [i1 c1 o1 l1 x1 p1] [i2 c2 o2 l2 x2 p2] H1 H2. pose proof (lex_antisym _ _ H1 H2) as E.
  unfold rkey in E. cbn in E. injection E as El Ei Ec Eo Ex Ep. subst.
  destruct p1, p2; try discriminate; reflexivity.
Qed.

Lemma rle_trans : forall a b c, rle a b = true -> rle b c = true -> rle a c = true.
Proof. intros a b c. apply lex_trans. Qed.

Lemma ins_comm : forall x y L, ins rle x (ins rle y L) = ins rle y (ins rle x L).
Proof.
  intros x y L. induction L as [| z L IH].
  - cbn [ins]. destruct (rle x y) eqn:Exy, (rle y x) eqn:Eyx; cbn [ins]; rewrite ?Exy, ?Eyx; try reflexivity.
    + rewrite (rle_antisym x y Exy Eyx). reflexivity.
    + destruct (rle_total x y) as [E | E]; congruence.
  - cbn [ins]. destruct (rle y z) eqn:Eyz, (rle x z) eqn:Exz; cbn [ins]; rewrite ?Eyz, ?Exz.
    + destruct (rle x y) eqn:Exy, (rle y x) eqn:Eyx; try reflexivity.
      * rewrite (rle_antisym x y Exy Eyx). reflexivity.
      * destruct (rle_total x y) as [E | E]; congruence.
    + destruct (rle x y) eqn:Exy; [rewrite (rle_trans x y z Exy Eyz) in Exz; discriminate |]. reflexivity.
    + destruct (rle y x) eqn:Eyx; [rewrite (rle_trans y x z Eyx Exz) in Eyz; discriminate |]. reflexivity.
    + rewrite IH. reflexivity.
Qed.

Definition wit_rorder (l : list (rec nat)) : list (rec nat) := isort rle l.

Definition wit_worder (c : nat) (l : list (wtr (option nat) unit)) : list (wtr (option nat) unit) := l.

Lemma ins_split : forall e L, exists l1 l2, L = l1 ++ l2 /\ ins rle e L = l1 ++ e :: l2.
Proof.
  intros e L. induction L as [| z L IH]; [exists [], []; split; reflexivity |]. cbn [ins].
  destruct (rle e z); [exists [], (z :: L); split; reflexivity |].
  destruct IH as (l1 & l2 & E1 & E2). exists (z :: l1), l2. rewrite E2, E1. split; reflexivity.
Qed.

Lemma wit_parameters_valid :
  (forall ms, wit_admits tt ms = true) /\
  (forall l, Permutation (wit_rorder l) l) /\
  (forall l l', Permutation l l' -> wit_rorder l = wit_rorder l') /\
  (forall e l, exists l1 l2, wit_rorder l = l1 ++ l2 /\ wit_rorder (e :: l) = l1 ++ e :: l2) /\
  (forall c l, Permutation (wit_worder c l) l) /\
  (forall c W l, exists l1 l2, wit_worder c l = l1 ++ l2 /\ wit_worder c (W :: l) = l1 ++ W :: l2).
Proof.
  split; [intros ms; reflexivity |]. split; [intros l; apply isort_perm |]. split.
  { intros l l' H. unfold wit_rorder. induction H as [| x l1 l2 _ IH | x y l | l1 l2 l3 _ IH1 _ IH2].
    - reflexivity.
    - cbn [isort fold_right]. fold (isort rle l1). fold (isort rle l2). rewrite IH. reflexivity.
    - cbn [isort fold_right]. apply ins_comm.
    - rewrite IH1. exact IH2. }
  split; [intros e l; exact (ins_split e (wit_rorder l)) |].
  split; [intros c l; apply Permutation_refl |].
  intros c W l. exists [], l. split; reflexivity.
Qed.

(* Headline 17 on the valid parameters: a bag waiter with the binds
   [c, c], P0 = any payload and P1 = the payload 1, over the records r1
   (payload 1) and r2 (payload 2). *)
Definition wit_b0 : bnd (option nat) := mkBnd (option nat) 1 None None.
Definition wit_b1 : bnd (option nat) := mkBnd (option nat) 1 (Some 1) None.
Definition wit_W : wtr (option nat) unit := mkWtr (option nat) unit 10 [wit_b0; wit_b1] [0; 0] tt false false.
Definition wit_r1 : rec nat := mkRec nat 1 1 0 0 1 false.
Definition wit_r2 : rec nat := mkRec nat 2 1 0 1 2 false.
Definition wit_v : view nat (option nat) unit := mkView nat (option nat) unit [wit_r1; wit_r2] [wit_W] [].

Lemma greedy_join_can_miss_assignment_witness :
  greedy nat (option nat) nat unit wit_spatial Scan (fun c => c) (fun x => x) wit_rorder wit_W wit_v = None /\
  receive_decision nat (option nat) nat unit wit_spatial wit_admits Scan (fun c => c) (fun x => x) wit_rorder wit_W wit_v = None /\
  assignment nat (option nat) nat unit wit_spatial Scan (fun c => c) (fun x => x) wit_rorder wit_W wit_v
    [(wit_r2, 2); (wit_r1, 1)].
Proof.
  exact (greedy_join_can_miss_assignment nat (option nat) nat unit wit_spatial wit_admits tt Scan (fun c => c) (fun x => x)
           wit_rorder wit_W wit_v 1 wit_b0 wit_b1 wit_r1 wit_r2 1 2 1 eq_refl eq_refl eq_refl eq_refl
           ltac:(vm_compute; reflexivity) ltac:(vm_compute; reflexivity) ltac:(vm_compute; discriminate)
           eq_refl eq_refl eq_refl eq_refl).
Qed.

(* Finding F-JF-1 and decision 50: plan 3.6 "Monotonicity" fails for a set
   (Classes), so a joined set collection cascades. A class sits at the
   place of its representative, the first stored member in the visit order.
   The representative r of the class 1 is not persistent, and a later
   member e of the class 1 is persistent. The waiter W1 has the binds
   [c, c] with P0 = any payload and P1 = the payload 1. Greedy binds r to
   P0 and finds no other class 1 member for P1, so W1 parks. A single-bind
   receive V of the payload 1 then consumes r, and no non-persistent member
   of the class 1 is left to retire. The class 1 moves to the place of e,
   after f, and greedy now binds f to P0 and e to P1.
   (1) The computed negative result of the set without the cascade. The
   plan of V (set_plan) starts from a state with INV_scan and ends with no
   stop and no second send. Then W1 has a greedy match and no guard, so
   INV_scan does not hold at the end of the plan. A transaction without
   the cascade (the set before decision 50) ends after its plan and the
   drain of its second sends, and this transaction has no second send. So
   it ends in this state, without INV_scan.
   (2) The cascade of decision 50 repairs the state. After the plan,
   NEXT-FIRE finds W1 on the dirty collection, and RUN-JOIN ends with no
   stop, no work and no parked waiter, so INV_head holds. *)
Definition set_r : rec nat := mkRec nat 1 1 0 0 1 false.
Definition set_f : rec nat := mkRec nat 2 1 0 1 2 false.
Definition set_e : rec nat := mkRec nat 3 1 0 2 1 true.
Definition set_W1 : wtr (option nat) unit :=
  mkWtr (option nat) unit 10 [mkBnd (option nat) 1 None None; mkBnd (option nat) 1 (Some 1) None] [0; 0] tt false false.
Definition set_V : wtr (option nat) unit := mkWtr (option nat) unit 11 [mkBnd (option nat) 1 (Some 1) None] [0] tt false false.
Definition set_v0 : view nat (option nat) unit := mkView nat (option nat) unit [set_r; set_f; set_e] [set_W1] [].
Definition set_op : op nat (option nat) unit nat := OReceive nat (option nat) unit nat set_V None 99.
Definition set_plan : jst nat (option nat) nat unit nat :=
  plan nat (option nat) nat unit nat wit_spatial wit_admits Classes 64 (fun c => c) (fun x => x) wit_rorder wit_worder
    10 set_op set_v0 (init nat (option nat) nat unit nat set_v0).
Definition set_run : jst nat (option nat) nat unit nat * option (op nat (option nat) unit nat) :=
  run_join nat (option nat) nat unit nat wit_spatial wit_admits Classes 64 (fun c => c) (fun x => x) wit_rorder wit_worder
    (fun r => r_id nat r) 10 set_op [1] set_v0.

Lemma set_join_class_shift_breaks_inv_scan :
  start_ok nat (option nat) unit nat set_op set_v0 /\
  INV_scan nat (option nat) nat unit wit_spatial tt Classes (fun c => c) (fun x => x) wit_rorder set_v0 /\
  ~ INV_scan nat (option nat) nat unit wit_spatial tt Classes (fun c => c) (fun x => x) wit_rorder
      (j_view nat (option nat) nat unit nat set_plan) /\
  j_stop nat (option nat) nat unit nat set_plan = None /\
  j_work nat (option nat) nat unit nat set_plan = [] /\
  v_wtrs nat (option nat) unit (j_view nat (option nat) nat unit nat set_plan) = [set_W1] /\
  w_guard (option nat) unit set_W1 = tt /\
  greedy nat (option nat) nat unit wit_spatial Classes (fun c => c) (fun x => x) wit_rorder set_W1
    (j_view nat (option nat) nat unit nat set_plan) = Some [(set_f, 2); (set_e, 1)] /\
  next_fire nat (option nat) nat unit wit_spatial wit_admits Classes (fun c => c) (fun x => x) wit_rorder wit_worder
    (j_dirty nat (option nat) nat unit nat set_plan) (j_view nat (option nat) nat unit nat set_plan) =
    Some (set_W1, [(set_f, 2); (set_e, 1)]) /\
  j_stop nat (option nat) nat unit nat (fst set_run) = None /\
  j_work nat (option nat) nat unit nat (fst set_run) = [] /\
  v_wtrs nat (option nat) unit (j_view nat (option nat) nat unit nat (fst set_run)) = [] /\
  INV_head nat (option nat) nat unit wit_spatial wit_admits Classes (fun c => c) (fun x => x) wit_rorder wit_worder
    (j_view nat (option nat) nat unit nat (fst set_run)).
Proof.
  split.
  { unfold start_ok, set_op, set_v0. cbn. split; [repeat constructor; cbn; intuition discriminate |].
    split; [repeat constructor; cbn; intuition discriminate | intuition discriminate]. }
  split.
  { intros W HW _. destruct HW as [<- | []]. vm_compute. reflexivity. }
  split.
  { intros H.
    assert (Hin : In set_W1 (v_wtrs nat (option nat) unit (j_view nat (option nat) nat unit nat set_plan)))
      by (vm_compute; left; reflexivity).
    pose proof (H set_W1 Hin eq_refl) as Hg. vm_compute in Hg. discriminate Hg. }
  split; [vm_compute; reflexivity |]. split; [vm_compute; reflexivity |]. split; [vm_compute; reflexivity |].
  split; [reflexivity |]. split; [vm_compute; reflexivity |]. split; [vm_compute; reflexivity |].
  split; [vm_compute; reflexivity |]. split; [vm_compute; reflexivity |].
  assert (E : v_wtrs nat (option nat) unit (j_view nat (option nat) nat unit nat (fst set_run)) = [])
    by (vm_compute; reflexivity).
  split; [exact E |].
  intros W HW. rewrite E in HW. destruct HW.
Qed.

(* ── 41. The assumptions of the headlines ───────────────────────────── *)

Print Assumptions join_search_is_head_greedy.
Print Assumptions join_produce_search_agrees_with_head.
Print Assumptions join_incoming_binds_once.
Print Assumptions join_fires_heads_only.
Print Assumptions join_repeated_channel_distinct_records.
Print Assumptions join_set_bind_retires_class.
Print Assumptions cascade_terminates.
Print Assumptions join_txn_fires_bounded.
Print Assumptions cascade_restores_inv.
Print Assumptions cascade_order_independent.
Print Assumptions lift_reoffer_equals_direct_comm.
Print Assumptions cascade_bumps_touched_counters.
Print Assumptions bind_permutation_restores_order.
Print Assumptions plan_then_lock_stable.
Print Assumptions join_reissue_keeps_bind_ords.
Print Assumptions peek_join_restores_stored_candidates.
Print Assumptions greedy_join_can_miss_assignment.
