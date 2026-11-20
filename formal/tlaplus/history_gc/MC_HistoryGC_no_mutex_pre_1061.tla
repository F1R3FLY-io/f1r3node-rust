--------- MODULE MC_HistoryGC_no_mutex_pre_1061 ---------
EXTENDS HistoryGC

\* Pre-fix control for #1061, race type 1 (content-addressed skip).
\* The writer and sweep do not serialise via the PruneGuard lock, so
\* the sweep can delete a hash the writer's `pending_writes` just
\* skipped — the writer commits its new state and it references the
\* deleted node. Expected: `Inv_CommittedWritersPreserved` violated.

MCChildrenRel == [ h \in {"h1","h2","h3","h4"} |->
                     IF h = "h1" THEN {"h2"}
                     ELSE IF h = "h3" THEN {"h4","h2"}
                     ELSE {} ]

MCWriterPlans == { [root |-> "h3", subtree |-> {"h3","h4","h2"}] }
==========================================================
