-------------------- MODULE FeeCursorCells --------------------
EXTENDS Naturals, FiniteSets, Sequences

\* Changed by DR-119 (bug 11004): FastPath adds the lock-free lookup of
\* ensureCostCursor, which runs before the locked creation of a cursor.
\* CONSTANTS Workers, Scopes, RecheckInitialization, CheckRevision,
\*           RestoreActual, PublishPosition
CONSTANTS Workers, Scopes, RecheckInitialization, CheckRevision,
          RestoreActual, PublishPosition, FastPath
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Scopes # {} /\ IsFiniteSet(Scopes)
       /\ RecheckInitialization \in BOOLEAN /\ CheckRevision \in BOOLEAN
       /\ RestoreActual \in BOOLEAN /\ PublishPosition \in BOOLEAN
       /\ FastPath \in BOOLEAN

VARIABLES selected, nextPosition, marker, revisionCells, positionCells,
          logical, fees, phase, held, payload, emitRevision, emitPosition
vars == <<selected, nextPosition, marker, revisionCells, positionCells,
          logical, fees, phase, held, payload, emitRevision, emitPosition>>
Zero == [revision |-> 0, position |-> 0]

Init ==
    /\ selected \in [Workers -> Scopes]
    /\ nextPosition \in [Workers -> 0..1]
    /\ marker = [s \in Scopes |-> FALSE]
    /\ revisionCells = [s \in Scopes |-> <<>>]
    /\ positionCells = [s \in Scopes |-> <<>>]
    /\ logical = [s \in Scopes |-> Zero]
    /\ fees = [s \in Scopes |-> 0]
    /\ phase = [w \in Workers |-> "new"]
    /\ held = [w \in Workers |-> Zero]
    /\ payload = [w \in Workers |-> Zero]
    /\ emitRevision = [w \in Workers |-> FALSE]
    /\ emitPosition = [w \in Workers |-> FALSE]

\* Added by DR-119: the lock-free lookup. A worker whose scope already has a
\* cursor goes to "waiting" with no lock and no creation. A worker that finds
\* no cursor goes on to the locked creation.
Peek(w) ==
    /\ FastPath
    /\ phase[w] = "new"
    /\ phase' = [phase EXCEPT ![w] = IF marker[selected[w]] THEN "waiting" ELSE "absent"]
    /\ UNCHANGED <<selected, nextPosition, marker, revisionCells, positionCells,
                    logical, fees, held, payload, emitRevision, emitPosition>>

\* Changed by DR-119: the creation runs under the lock of the scope's bucket.
\* After a lookup that found no cursor, the lookup can be stale, so the
\* creation checks again under the lock (RecheckInitialization).
\* Initialize(w) ==
\*     /\ phase[w] = "new"
Initialize(w) ==
    /\ phase[w] = IF FastPath THEN "absent" ELSE "new"
    /\ LET create == ~marker[selected[w]] \/ ~RecheckInitialization
       IN /\ marker' = [marker EXCEPT ![selected[w]] = TRUE]
          /\ emitRevision' = [emitRevision EXCEPT ![w] = create]
          /\ emitPosition' = [emitPosition EXCEPT ![w] = create]
    /\ phase' = [phase EXCEPT ![w] = "waiting"]
    /\ UNCHANGED <<selected, nextPosition, revisionCells, positionCells,
                    logical, fees, held, payload>>

Acquire(w) ==
    /\ phase[w] = "waiting"
    /\ ~emitRevision[w] /\ ~emitPosition[w]
    /\ Len(revisionCells[selected[w]]) > 0 /\ Len(positionCells[selected[w]]) > 0
    /\ held' = [held EXCEPT ![w] =
         [revision |-> Head(revisionCells[selected[w]]), position |-> Head(positionCells[selected[w]])]]
    /\ revisionCells' = [revisionCells EXCEPT ![selected[w]] = Tail(@)]
    /\ positionCells' = [positionCells EXCEPT ![selected[w]] = Tail(@)]
    /\ phase' = [phase EXCEPT ![w] = "owned"]
    /\ UNCHANGED <<selected, nextPosition, marker, logical, fees, payload,
                    emitRevision, emitPosition>>

Decide(w, abort) ==
    /\ phase[w] = "owned"
    /\ LET accepted == ~abort /\ (~CheckRevision \/ held[w].revision = 0)
                              /\ held[w].position = 0
           after == IF accepted THEN [revision |-> 1, position |-> nextPosition[w]]
                    ELSE IF RestoreActual THEN held[w] ELSE Zero
       IN /\ payload' = [payload EXCEPT ![w] = after]
          /\ logical' = IF accepted THEN [logical EXCEPT ![selected[w]] = after] ELSE logical
          /\ fees' = IF accepted THEN [fees EXCEPT ![selected[w]] = @ + 1] ELSE fees
    /\ phase' = [phase EXCEPT ![w] = "publishing"]
    /\ emitRevision' = [emitRevision EXCEPT ![w] = TRUE]
    /\ emitPosition' = [emitPosition EXCEPT ![w] = PublishPosition]
    /\ UNCHANGED <<selected, nextPosition, marker, revisionCells, positionCells, held>>

PublishRevision(w) ==
    /\ emitRevision[w]
    /\ revisionCells' = [revisionCells EXCEPT ![selected[w]] = Append(@, payload[w].revision)]
    /\ emitRevision' = [emitRevision EXCEPT ![w] = FALSE]
    /\ UNCHANGED <<selected, nextPosition, marker, positionCells, logical, fees,
                    phase, held, payload, emitPosition>>

PublishCursorPosition(w) ==
    /\ emitPosition[w]
    /\ positionCells' = [positionCells EXCEPT ![selected[w]] = Append(@, payload[w].position)]
    /\ emitPosition' = [emitPosition EXCEPT ![w] = FALSE]
    /\ UNCHANGED <<selected, nextPosition, marker, revisionCells, logical, fees,
                    phase, held, payload, emitRevision>>

Finish(w) ==
    /\ phase[w] = "publishing" /\ ~emitRevision[w] /\ ~emitPosition[w]
    /\ phase' = [phase EXCEPT ![w] = "done"]
    /\ UNCHANGED <<selected, nextPosition, marker, revisionCells, positionCells,
                    logical, fees, held, payload, emitRevision, emitPosition>>

Next == (\E w \in Workers : Peek(w) \/ Initialize(w) \/ Acquire(w) \/ Finish(w)
                            \/ PublishRevision(w) \/ PublishCursorPosition(w))
     \/ (\E w \in Workers, abort \in BOOLEAN : Decide(w, abort))
Spec == Init /\ [][Next]_vars

Owners(s) == {w \in Workers : selected[w] = s /\ phase[w] = "owned"}
RevisionPopulation(s) == Len(revisionCells[s]) + Cardinality(Owners(s))
    + Cardinality({w \in Workers : selected[w] = s /\ emitRevision[w]})
PositionPopulation(s) == Len(positionCells[s]) + Cardinality(Owners(s))
    + Cardinality({w \in Workers : selected[w] = s /\ emitPosition[w]})
ExactlyOnePairPerInitializedScope ==
    \A s \in Scopes : /\ RevisionPopulation(s) = IF marker[s] THEN 1 ELSE 0
                      /\ PositionPopulation(s) = IF marker[s] THEN 1 ELSE 0
PublishedCellsMatchLogicalCursor ==
    \A s \in Scopes : /\ \A i \in 1..Len(revisionCells[s]) : revisionCells[s][i] = logical[s].revision
                      /\ \A i \in 1..Len(positionCells[s]) : positionCells[s][i] = logical[s].position
EveryFeeHasOneRevision == \A s \in Scopes : fees[s] = logical[s].revision
OwnedPairWasCoherent ==
    \A w \in Workers : phase[w] = "owned" => held[w] = logical[selected[w]]

=============================================================================
