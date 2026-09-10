---------------------- MODULE AcceptedPublicationCustody ----------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Hashes, Contexts, Old, Unsafe
VARIABLES phases, requested, leases, captured, context, acceptedContext,
          valid, stored, accepted, pending, pendingProvenance, required,
          terminal, publishedTarget, publishedContext, missingMetadata, restorable, restored

vars == <<phases, requested, leases, captured, context, acceptedContext,
          valid, stored, accepted, pending, pendingProvenance, required,
          terminal, publishedTarget, publishedContext, missingMetadata, restorable, restored>>

Init ==
    /\ phases = [h \in Hashes |-> "idle"]
    /\ requested = {}
    /\ leases = {}
    /\ captured = {}
    /\ context \in [Hashes -> Contexts]
    /\ acceptedContext = context
    /\ valid \in SUBSET Hashes
    /\ restorable \in SUBSET valid
    /\ restored = {}
    /\ stored = restorable
    /\ accepted = {}
    /\ pending = {}
    /\ pendingProvenance = {}
    /\ required = {}
    /\ terminal = {}
    /\ missingMetadata = {}
    /\ publishedTarget = [h \in Hashes |-> h]
    /\ publishedContext = context

Solicit(h) ==
    /\ requested' = requested \cup {h}
    /\ UNCHANGED <<phases, leases, captured, context, acceptedContext, valid,
          stored, accepted, pending, pendingProvenance, required, terminal,
          publishedTarget, publishedContext>>

Retire(h) ==
    /\ requested' = requested \ {h}
    /\ UNCHANGED <<phases, leases, captured, context, acceptedContext, valid,
          stored, accepted, pending, pendingProvenance, required, terminal,
          publishedTarget, publishedContext>>

Capture(h) ==
    /\ phases[h] = "idle" /\ h \notin terminal
    /\ h \notin Old \/ h \in requested
    /\ phases' = [phases EXCEPT ![h] = "provisional"]
    /\ leases' = leases \cup {h}
    /\ captured' = IF h \in requested THEN captured \cup {h} ELSE captured
    /\ acceptedContext' = [acceptedContext EXCEPT ![h] = context[h]]
    /\ UNCHANGED <<requested, context, valid, stored, accepted, pending,
          pendingProvenance, required, terminal, publishedTarget, publishedContext>>

Store(h) ==
    /\ phases[h] = "provisional"
    /\ h \in valid \/ Unsafe = "unchecked"
    /\ stored' = stored \cup {h}
    /\ accepted' = accepted \cup {h}
    /\ phases' = [phases EXCEPT ![h] = "accepted"]
    /\ UNCHANGED <<requested, leases, captured, context, acceptedContext, valid,
          pending, pendingProvenance, required, terminal, publishedTarget, publishedContext>>

FailPublication(h) ==
    /\ phases[h] = "accepted"
    /\ phases' = [phases EXCEPT ![h] = "retry"]
    /\ leases' = IF Unsafe = "early-release" THEN leases \ {h} ELSE leases
    /\ captured' = IF Unsafe = "lost-capture" THEN captured \ {h} ELSE captured
    /\ UNCHANGED <<requested, context, acceptedContext, valid, stored, accepted,
          pending, pendingProvenance, required, terminal, publishedTarget, publishedContext>>

ExternalPending(h) ==
    /\ h \notin terminal
    /\ pending' = pending \cup {h}
    /\ pendingProvenance' = pendingProvenance \cup {h}
    /\ required' = required \cup {h}
    /\ UNCHANGED <<phases, requested, leases, captured, context, acceptedContext,
          valid, stored, accepted, terminal, publishedTarget, publishedContext>>

Publish(h, target, targetContext) ==
    /\ phases[h] \in {"accepted", "retry"}
    /\ h \in stored /\ h \in valid
    /\ h \notin terminal \/ Unsafe = "terminal"
       \/ (Unsafe = "missing-metadata" /\ h \in missingMetadata)
    /\ target = h \/ Unsafe = "wrong-hash"
    /\ targetContext = acceptedContext[h] \/ Unsafe = "wrong-context"
    /\ LET provenance == IF Unsafe = "recapture" THEN h \in requested ELSE h \in captured
       IN pendingProvenance' =
          IF provenance THEN pendingProvenance \cup {target}
          ELSE IF Unsafe = "overwrite" THEN pendingProvenance \ {target}
          ELSE pendingProvenance
    /\ pending' = pending \cup {target}
    /\ required' = IF h \in captured THEN required \cup {h} ELSE required
    /\ publishedTarget' = [publishedTarget EXCEPT ![h] = target]
    /\ publishedContext' = [publishedContext EXCEPT ![h] = targetContext]
    /\ phases' = [phases EXCEPT ![h] = "published"]
    /\ UNCHANGED <<requested, leases, captured, context, acceptedContext,
          valid, stored, accepted, terminal>>

Finish(h) ==
    /\ h \in leases
    /\ h \in terminal \/ phases[h] = "published"
    /\ leases' = leases \ {h}
    /\ phases' = [phases EXCEPT ![h] = "done"]
    /\ UNCHANGED <<requested, captured, context, acceptedContext, valid,
          stored, accepted, pending, pendingProvenance, required, terminal,
          publishedTarget, publishedContext>>

Admit(h) ==
    /\ h \notin terminal
    /\ terminal' = terminal \cup {h}
    /\ pending' = pending \ {h}
    /\ pendingProvenance' = pendingProvenance \ {h}
    /\ required' = required \ {h}
    /\ UNCHANGED <<phases, requested, leases, captured, context, acceptedContext,
          valid, stored, accepted, publishedTarget, publishedContext>>

CustodyStep ==
    (\E h \in Hashes : Solicit(h) \/ Retire(h) \/ Capture(h) \/ Store(h)
       \/ FailPublication(h) \/ ExternalPending(h) \/ Finish(h) \/ Admit(h))
    \/ (\E h, target \in Hashes, c \in Contexts : Publish(h, target, c))

LoseMetadata(h) ==
    /\ h \in terminal \ missingMetadata
    /\ missingMetadata' = missingMetadata \cup {h}
    /\ UNCHANGED <<phases, requested, leases, captured, context, acceptedContext,
          valid, stored, accepted, pending, pendingProvenance, required,
          terminal, publishedTarget, publishedContext>>

CaptureStored(h) ==
    /\ phases[h] = "idle" /\ h \notin terminal
    /\ h \in restorable \/ Unsafe = "unverified-restoration"
    /\ phases' = [phases EXCEPT ![h] = "accepted"]
    /\ accepted' = accepted \cup {h}
    /\ leases' = leases \cup {h}
    /\ restored' = restored \cup {h}
    /\ captured' = IF h \in requested THEN captured \cup {h} ELSE captured
    /\ acceptedContext' = [acceptedContext EXCEPT ![h] = context[h]]
    /\ UNCHANGED <<requested, context, valid, stored, pending, pendingProvenance,
          required, terminal, publishedTarget, publishedContext, missingMetadata, restorable>>

Next == (CustodyStep /\ UNCHANGED <<missingMetadata, restorable, restored>>)
        \/ ((\E h \in Hashes : LoseMetadata(h)) /\ UNCHANGED <<restorable, restored>>)
        \/ (\E h \in Hashes : CaptureStored(h))

TypeOK ==
    /\ phases \in [Hashes -> {"idle", "provisional", "accepted", "retry", "published", "done"}]
    /\ requested \subseteq Hashes /\ leases \subseteq Hashes /\ captured \subseteq Hashes
    /\ context \in [Hashes -> Contexts] /\ acceptedContext \in [Hashes -> Contexts]
    /\ valid \subseteq Hashes /\ stored \subseteq Hashes /\ accepted \subseteq Hashes
    /\ pending \subseteq Hashes /\ pendingProvenance \subseteq pending
    /\ required \subseteq Hashes /\ terminal \subseteq Hashes
    /\ missingMetadata \subseteq terminal
    /\ restorable \subseteq valid /\ restored \subseteq Hashes
    /\ publishedTarget \in [Hashes -> Hashes] /\ publishedContext \in [Hashes -> Contexts]
Inv_Verified == accepted \subseteq valid \cap stored
Inv_Custody == accepted \ terminal \subseteq leases \cup pending
Inv_OldCapture == (accepted \cap Old) \ restored \subseteq captured
Inv_Provenance == required \subseteq pendingProvenance
Inv_ExactTarget == \A h \in Hashes : publishedTarget[h] = h
Inv_Context == \A h \in Hashes : publishedContext[h] = acceptedContext[h]
Inv_Terminal == pending \cap terminal = {}
Safety == TypeOK /\ Inv_Verified /\ Inv_Custody /\ Inv_OldCapture
          /\ Inv_Provenance /\ Inv_ExactTarget /\ Inv_Context /\ Inv_Terminal
Spec == Init /\ [][Next]_vars
=============================================================================
