-------------------- MODULE NativeSettlementRoots --------------------
EXTENDS Naturals
CONSTANTS Workers, RequireOriginalRuntime, RecaptureFunding, CheckFundingRoot, CheckRuntimeRoot
Roots == 0..1
Phases == {"Awaiting", "Bound", "Done", "Rejected"}

VARIABLES phase, original, exported, failed, authorized, complete,
          runtime, funding, working, accepted, acceptedRuntime
vars == <<phase, original, exported, failed, authorized, complete,
          runtime, funding, working, accepted, acceptedRuntime>>
Expected(w) == IF failed[w] THEN original[w] ELSE exported[w]

Init ==
  /\ phase = [w \in Workers |-> "Awaiting"]
  /\ original \in [Workers -> Roots]
  /\ exported \in [Workers -> Roots]
  /\ failed \in [Workers -> BOOLEAN]
  /\ authorized \in [Workers -> BOOLEAN]
  /\ complete \in [Workers -> BOOLEAN]
  /\ runtime = [w \in Workers |-> Expected(w)]
  /\ funding = original
  /\ working = original
  /\ accepted = original
  /\ acceptedRuntime = original

Bind(w) ==
  /\ phase[w] = "Awaiting"
  /\ authorized[w] /\ complete[w]
  /\ phase' = [phase EXCEPT ![w] = "Bound"]
  /\ funding' = [funding EXCEPT ![w] = IF RecaptureFunding THEN runtime[w] ELSE original[w]]
  /\ working' = [working EXCEPT ![w] = Expected(w)]
  /\ UNCHANGED <<original, exported, failed, authorized, complete, runtime, accepted, acceptedRuntime>>

Accepts(w, snapshot, observed) ==
  (~CheckFundingRoot \/ snapshot = funding[w]) /\
  (~CheckRuntimeRoot \/ observed = IF RequireOriginalRuntime THEN funding[w] ELSE working[w])

Settle(w, snapshot, observed) ==
  /\ phase[w] = "Bound"
  /\ Accepts(w, snapshot, observed)
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ accepted' = [accepted EXCEPT ![w] = snapshot]
  /\ acceptedRuntime' = [acceptedRuntime EXCEPT ![w] = observed]
  /\ UNCHANGED <<original, exported, failed, authorized, complete, runtime, funding, working>>

Reject(w) ==
  /\ phase[w] = "Awaiting"
  /\ ~(authorized[w] /\ complete[w])
  /\ phase' = [phase EXCEPT ![w] = "Rejected"]
  /\ UNCHANGED <<original, exported, failed, authorized, complete, runtime, funding, working, accepted, acceptedRuntime>>

Next == \E w \in Workers : Bind(w) \/ Reject(w) \/
  (\E snapshot, observed \in Roots : Settle(w, snapshot, observed))
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Workers -> Phases]
  /\ original \in [Workers -> Roots]
  /\ exported \in [Workers -> Roots]
  /\ failed \in [Workers -> BOOLEAN]
  /\ authorized \in [Workers -> BOOLEAN]
  /\ complete \in [Workers -> BOOLEAN]
  /\ runtime \in [Workers -> Roots]
  /\ funding \in [Workers -> Roots]
  /\ working \in [Workers -> Roots]
  /\ accepted \in [Workers -> Roots]
  /\ acceptedRuntime \in [Workers -> Roots]
OriginalFundingPreserved == \A w \in Workers : funding[w] = original[w]
OnlyCompleteAuthorized == \A w \in Workers : phase[w] \in {"Bound", "Done"} => authorized[w] /\ complete[w]
ExactWorkingRoot == \A w \in Workers : phase[w] \in {"Bound", "Done"} => working[w] = Expected(w)
ExactAcceptedFunding == \A w \in Workers : phase[w] = "Done" => accepted[w] = original[w]
ExactAcceptedRuntime == \A w \in Workers : phase[w] = "Done" => acceptedRuntime[w] = Expected(w)
IndependentSettlementEnabled == \A w \in Workers : phase[w] = "Bound" => ENABLED Settle(w, original[w], runtime[w])
=============================================================================
