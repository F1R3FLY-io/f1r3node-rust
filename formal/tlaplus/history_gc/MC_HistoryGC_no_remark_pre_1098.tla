--------- MODULE MC_HistoryGC_no_remark_pre_1098 ---------
EXTENDS HistoryGC

\* Pre-fix control for #1098: the batched sweep does NOT re-mark pins
\* between delete batches. Writers can commit between the initial pin
\* snapshot and a later batch, having pinned their new root; without
\* the re-mark the batch's scan sees the writer's nodes but marks
\* nothing about them, and the delete list includes the writer-
\* committed subtree. Expected: `Inv_NoDanglingLiveReference` violated.

MCChildrenRel == [ h \in {"h1","h2","h3","h4"} |->
                     IF h = "h1" THEN {"h2"}
                     ELSE IF h = "h3" THEN {"h4","h2"}
                     ELSE {} ]

MCWriterPlans == { [root |-> "h3", subtree |-> {"h3","h4","h2"}] }
===========================================================
