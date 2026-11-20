------------------------ MODULE MC_HistoryGC ------------------------
EXTENDS HistoryGC

\* Post-fix model instance. All three protections are on; TLC must find
\* no invariant violations.
\*
\* Scenario:
\*   - Four hashes h1..h4.
\*   - The DAG retains root h1, whose subtree is {h1, h2}.
\*   - A writer plans to commit a new state at root h3 whose subtree is
\*     {h3, h4, h2} — the leaf h2 is SHARED with h1's subtree, mirroring
\*     content-addressed reuse of an existing leaf.
\*   - InitialStore = {h1, h2}. The writer's commit must add {h3, h4};
\*     h2 is skipped by `pending_writes`.
\*
\* Expected: safety holds across every interleaving TLC explores.

MCChildrenRel == [ h \in {"h1","h2","h3","h4"} |->
                     IF h = "h1" THEN {"h2"}
                     ELSE IF h = "h3" THEN {"h4","h2"}
                     ELSE {} ]

MCWriterPlans == { [root |-> "h3", subtree |-> {"h3","h4","h2"}] }
=====================================================================
