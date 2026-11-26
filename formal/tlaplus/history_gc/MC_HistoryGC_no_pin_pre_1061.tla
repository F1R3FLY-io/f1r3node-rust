--------- MODULE MC_HistoryGC_no_pin_pre_1061 ---------
EXTENDS HistoryGC

\* Pre-fix control for #1061, race type 2 (not-yet-in-DAG roots).
\* The writer does not pin its new root before writing. The caller-side
\* retention set does not name the new root (DAG hasn't caught up), so
\* the sweep marks only the DAG roots and deletes the writer's new
\* subtree. Expected: `Inv_NoDanglingLiveReference` violated.

MCChildrenRel == [ h \in {"h1","h2","h3","h4"} |->
                     IF h = "h1" THEN {"h2"}
                     ELSE IF h = "h3" THEN {"h4","h2"}
                     ELSE {} ]

MCWriterPlans == { [root |-> "h3", subtree |-> {"h3","h4","h2"}] }
========================================================
