---------------------- MODULE BufferPruningObligations ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS BlockCount, ResidentCap, Topology, Unsafe

Blocks == 1..BlockCount
Certificate == BlockCount + 1
Dependencies == Blocks \cup {Certificate}
Required(b) ==
    CASE Topology = "chain" -> IF b = 1 THEN {Certificate} ELSE {b - 1}
      [] Topology = "shared" -> {Certificate}
      [] Topology = "isolated" -> {}
      [] Topology = "join" -> IF b = BlockCount THEN Blocks \ {b} ELSE {Certificate}

VARIABLES seen, rows, parents, resident, requested, certificateRequests,
          resolved, cursor, sampled, lostCertificateCoverage

vars == <<seen, rows, parents, resident, requested, certificateRequests,
          resolved, cursor, sampled, lostCertificateCoverage>>

Init ==
    /\ seen = {}
    /\ rows = {}
    /\ parents = [b \in Blocks |-> {}]
    /\ resident = {}
    /\ requested = {}
    /\ certificateRequests = {}
    /\ resolved = {}
    /\ cursor = 0
    /\ sampled = {}
    /\ lostCertificateCoverage = FALSE

Buffer(b) ==
    /\ b \notin seen
    /\ seen' = seen \cup {b}
    /\ rows' = rows \cup {b}
    /\ parents' = [parents EXCEPT ![b] = Required(b) \ resolved]
    /\ requested' = requested \cup {b}
    /\ UNCHANGED <<resident, certificateRequests, resolved, cursor, sampled,
                    lostCertificateCoverage>>

Acknowledge(b) ==
    /\ b \in rows \cap requested
    /\ requested' = requested \ {b}
    /\ UNCHANGED <<seen, rows, parents, resident, certificateRequests,
                    resolved, cursor, sampled, lostCertificateCoverage>>

Minimum(keys) == CHOOSE k \in keys : \A other \in keys : k <= other
NextKey ==
    LET after == {b \in rows : b > cursor}
    IN Minimum(IF after = {} THEN rows ELSE after)

ReadPage ==
    /\ rows # {}
    /\ LET b == NextKey
       IN /\ cursor' = b
          /\ sampled' = sampled \cup {b}
          /\ IF b \in resident \/ Cardinality(resident) < ResidentCap
                THEN resident' = resident \cup {b}
                ELSE \E victim \in resident :
                    resident' = (resident \ {victim}) \cup {b}
    /\ UNCHANGED <<seen, rows, parents, requested, certificateRequests, resolved,
                    lostCertificateCoverage>>

Cold(b) ==
    /\ b \in resident
    /\ resident' = resident \ {b}
    /\ UNCHANGED <<seen, rows, parents, requested, certificateRequests,
                    resolved, cursor, sampled, lostCertificateCoverage>>

Prunable(b) ==
    /\ b \in rows
    /\ parents[b] = {} \/ \E p \in parents[b] : p \notin rows

Evict(b) ==
    /\ b \in resident
    /\ Prunable(b)
    /\ Unsafe = "forget-retry" => b \notin requested
    /\ IF Unsafe \in {"resolve-on-eviction", "forget-retry"}
          THEN /\ rows' = rows \ {b}
               /\ parents' = [c \in Blocks |->
                    IF c = b THEN {}
                    ELSE IF Unsafe = "resolve-on-eviction"
                            THEN parents[c] \ {b}
                            ELSE parents[c]]
          ELSE /\ UNCHANGED <<rows, parents>>
    /\ resident' = resident \ {b}
    /\ UNCHANGED <<seen, requested, certificateRequests, resolved, cursor, sampled,
                    lostCertificateCoverage>>

NeededCertificates(keys) ==
    IF \E b \in keys : Certificate \in parents[b]
       THEN {Certificate} ELSE {}

ReconcileCertificates ==
    /\ certificateRequests' = NeededCertificates(
           IF Unsafe = "hot-only-certificates" THEN resident ELSE rows)
    /\ lostCertificateCoverage' =
           ~(NeededCertificates(rows) \subseteq certificateRequests')
    /\ UNCHANGED <<seen, rows, parents, resident, requested, resolved, cursor, sampled>>

CertificateArrives ==
    /\ Certificate \in certificateRequests
    /\ Certificate \notin resolved
    /\ resolved' = resolved \cup {Certificate}
    /\ parents' = [b \in Blocks |-> parents[b] \ {Certificate}]
    /\ certificateRequests' = {}
    /\ UNCHANGED <<seen, rows, resident, requested, cursor, sampled,
                    lostCertificateCoverage>>

Admit(b) ==
    /\ b \in rows
    /\ Required(b) \subseteq resolved
    /\ resolved' = resolved \cup {b}
    /\ rows' = rows \ {b}
    /\ parents' = [c \in Blocks |-> IF c = b THEN {} ELSE parents[c] \ {b}]
    /\ resident' = resident \ {b}
    /\ requested' = requested \ {b}
    /\ UNCHANGED <<seen, certificateRequests, cursor, sampled, lostCertificateCoverage>>

Restart ==
    /\ resident' = IF Unsafe = "restore-all-resident" THEN rows ELSE {}
    /\ requested' = {}
    /\ certificateRequests' = {}
    /\ cursor' = 0
    /\ sampled' = {}
    /\ UNCHANGED <<seen, rows, parents, resolved, lostCertificateCoverage>>

FailedWrite == UNCHANGED vars

Next ==
    \/ \E b \in Blocks : Buffer(b) \/ Acknowledge(b) \/ Cold(b) \/ Evict(b) \/ Admit(b)
    \/ ReadPage
    \/ ReconcileCertificates
    \/ CertificateArrives
    \/ Restart
    \/ FailedWrite

TypeOK ==
    /\ seen \subseteq Blocks
    /\ rows \subseteq seen
    /\ parents \in [Blocks -> SUBSET Dependencies]
    /\ resident \subseteq rows
    /\ requested \subseteq seen
    /\ certificateRequests \subseteq {Certificate}
    /\ resolved \subseteq Dependencies
    /\ cursor \in 0..BlockCount
    /\ sampled \subseteq seen
    /\ lostCertificateCoverage \in BOOLEAN

Inv_Obligations ==
    \A b \in rows : Required(b) \ resolved \subseteq parents[b]
Inv_DurableRetry == seen \ resolved \subseteq rows
Inv_ResidentBound == Cardinality(resident) <= ResidentCap
Inv_ResolvedOnly == \A b \in seen \cap resolved : Required(b) \subseteq resolved
Inv_CanonicalRows == \A b \in rows : parents[b] = Required(b) \ resolved

Inv_CertificateCoverage == ~lostCertificateCoverage

Safety == TypeOK /\ Inv_Obligations /\ Inv_DurableRetry /\ Inv_ResidentBound
          /\ Inv_ResolvedOnly /\ Inv_CanonicalRows /\ Inv_CertificateCoverage

Spec == Init /\ [][Next]_vars
=============================================================================
