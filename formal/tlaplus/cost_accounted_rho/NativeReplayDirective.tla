-------------------- MODULE NativeReplayDirective --------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Workers, Channels, EarlyPublish, MissingCandidateDenial
VARIABLES channel, phase, held, version, observed, decision, valid,
          changed, result
vars == <<channel, phase, held, version, observed, decision, valid, changed, result>>

Init ==
  /\ channel \in [Workers -> Channels]
  /\ phase = [w \in Workers |-> "Ready"]
  /\ held = {}
  /\ version = [c \in Channels |-> 0]
  /\ observed = [w \in Workers |-> 0]
  /\ decision = [w \in Workers |-> "Unset"]
  /\ valid = [w \in Workers |-> FALSE]
  /\ changed = {}
  /\ result = [w \in Workers |-> "Unset"]

Acquire(w) ==
  /\ phase[w] = "Ready"
  /\ \A other \in held : channel[other] # channel[w]
  /\ held' = held \cup {w}
  /\ phase' = [phase EXCEPT ![w] = "Check"]
  /\ observed' = [observed EXCEPT ![w] = version[channel[w]]]
  /\ UNCHANGED <<channel, version, decision, valid, changed, result>>

Check(w) ==
  /\ phase[w] = "Check"
  /\ decision' \in { [decision EXCEPT ![w] = d] : d \in {"Grant", "Deny", "Error"} }
  /\ valid' \in { [valid EXCEPT ![w] = b] : b \in BOOLEAN }
  /\ phase' = [phase EXCEPT ![w] = "Publish"]
  /\ changed' = IF EarlyPublish THEN changed \cup {w} ELSE changed
  /\ UNCHANGED <<channel, held, version, observed, result>>

Publish(w) ==
  /\ phase[w] = "Publish"
  /\ LET grant == valid[w] /\ decision[w] = "Grant"
         denied == (valid[w] \/ MissingCandidateDenial) /\ decision[w] = "Deny"
     IN /\ version' = IF grant THEN [version EXCEPT ![channel[w]] = @ + 1] ELSE version
        /\ changed' = IF grant THEN changed \cup {w} ELSE changed
        /\ result' = [result EXCEPT ![w] = IF grant THEN "Committed" ELSE IF denied THEN "Denied" ELSE "Mismatch"]
  /\ phase' = [phase EXCEPT ![w] = "Done"]
  /\ held' = held \ {w}
  /\ UNCHANGED <<channel, observed, decision, valid>>

Next == \E w \in Workers : Acquire(w) \/ Check(w) \/ Publish(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ channel \in [Workers -> Channels]
  /\ phase \in [Workers -> {"Ready", "Check", "Publish", "Done"}]
  /\ held \subseteq Workers
  /\ version \in [Channels -> 0..Cardinality(Workers)]
  /\ observed \in [Workers -> 0..Cardinality(Workers)]
  /\ decision \in [Workers -> {"Unset", "Grant", "Deny", "Error"}]
  /\ valid \in [Workers -> BOOLEAN]
  /\ changed \subseteq Workers
  /\ result \in [Workers -> {"Unset", "Committed", "Denied", "Mismatch"}]

AuthenticatedPublication == \A w \in changed : valid[w] /\ decision[w] = "Grant" /\ phase[w] = "Done"
ExactDenial == \A w \in Workers : result[w] = "Denied" => valid[w] /\ decision[w] = "Deny"
LockedSnapshot == \A w \in held : observed[w] = version[channel[w]]
ExclusiveChannels == \A w, other \in held : w = other \/ channel[w] # channel[other]
IndependentProgress == \A w \in Workers :
  (phase[w] = "Ready" /\ (\A other \in held : channel[other] # channel[w])) => ENABLED Acquire(w)
=============================================================================
