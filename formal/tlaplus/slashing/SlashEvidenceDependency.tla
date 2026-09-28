---------------------- MODULE SlashEvidenceDependency ----------------------
EXTENDS FiniteSets, TLC

CONSTANTS
    Deploys,
    Hashes,
    SlashTarget,
    SlashTargetsAreDependencies,
    TrackerWitnessSatisfiesSlashDependency

VARIABLES
    canonicalEvidence,
    receiverEvidence,
    trackerWitnesses,
    submitted,
    waiting,
    requested,
    accepted,
    rejectedAsUnauthorized,
    localAbsenceRejected

vars == <<canonicalEvidence, receiverEvidence, trackerWitnesses, submitted,
          waiting, requested, accepted, rejectedAsUnauthorized,
          localAbsenceRejected>>

TypeOK ==
    /\ canonicalEvidence \in SUBSET Hashes
    /\ receiverEvidence \in SUBSET Hashes
    /\ trackerWitnesses \in SUBSET Hashes
    /\ submitted \in SUBSET Deploys
    /\ waiting \in SUBSET Deploys
    /\ requested \in SUBSET Hashes
    /\ accepted \in SUBSET Deploys
    /\ rejectedAsUnauthorized \in SUBSET Deploys
    /\ localAbsenceRejected \in BOOLEAN
    /\ SlashTarget \in [Deploys -> Hashes]
    /\ SlashTargetsAreDependencies \in BOOLEAN
    /\ TrackerWitnessSatisfiesSlashDependency \in BOOLEAN

Init ==
    /\ canonicalEvidence = {}
    /\ receiverEvidence = {}
    /\ trackerWitnesses = {}
    /\ submitted = {}
    /\ waiting = {}
    /\ requested = {}
    /\ accepted = {}
    /\ rejectedAsUnauthorized = {}
    /\ localAbsenceRejected = FALSE

RecordEvidence(h) ==
    /\ h \in Hashes
    /\ canonicalEvidence' = canonicalEvidence \cup {h}
    /\ UNCHANGED <<receiverEvidence, trackerWitnesses, submitted, waiting,
                    requested, accepted, rejectedAsUnauthorized,
                    localAbsenceRejected>>

RecordTrackerWitness(h) ==
    /\ h \in canonicalEvidence
    /\ trackerWitnesses' = trackerWitnesses \cup {h}
    /\ UNCHANGED <<canonicalEvidence, receiverEvidence, submitted, waiting,
                    requested, accepted, rejectedAsUnauthorized,
                    localAbsenceRejected>>

SubmitAuthorizedSlash(d) ==
    /\ d \in Deploys
    /\ SlashTarget[d] \in canonicalEvidence
    /\ d \notin submitted
    /\ submitted' = submitted \cup {d}
    /\ UNCHANGED <<canonicalEvidence, receiverEvidence, trackerWitnesses,
                    waiting, requested, accepted, rejectedAsUnauthorized,
                    localAbsenceRejected>>

ReceiveAuthorizedSlash(d) ==
    /\ d \in submitted
    /\ d \notin waiting \cup accepted \cup rejectedAsUnauthorized
    /\ IF SlashTarget[d] \in receiverEvidence
       THEN
         /\ accepted' = accepted \cup {d}
         /\ UNCHANGED <<waiting, requested, rejectedAsUnauthorized,
                         localAbsenceRejected>>
       ELSE IF TrackerWitnessSatisfiesSlashDependency
               /\ SlashTarget[d] \in trackerWitnesses
       THEN
         /\ rejectedAsUnauthorized' = rejectedAsUnauthorized \cup {d}
         /\ localAbsenceRejected' = TRUE
         /\ UNCHANGED <<waiting, requested, accepted>>
       ELSE IF SlashTargetsAreDependencies
       THEN
         /\ waiting' = waiting \cup {d}
         /\ requested' = requested \cup {SlashTarget[d]}
         /\ UNCHANGED <<accepted, rejectedAsUnauthorized,
                         localAbsenceRejected>>
       ELSE
         /\ rejectedAsUnauthorized' = rejectedAsUnauthorized \cup {d}
         /\ localAbsenceRejected' = TRUE
         /\ UNCHANGED <<waiting, requested, accepted>>
    /\ UNCHANGED <<canonicalEvidence, receiverEvidence, trackerWitnesses,
                    submitted>>

FetchEvidence(h) ==
    /\ h \in requested
    /\ h \in canonicalEvidence
    /\ receiverEvidence' = receiverEvidence \cup {h}
    /\ requested' = requested \ {h}
    /\ UNCHANGED <<canonicalEvidence, trackerWitnesses, submitted, waiting,
                    accepted, rejectedAsUnauthorized, localAbsenceRejected>>

ResumeAuthorizedSlash(d) ==
    /\ d \in waiting
    /\ SlashTarget[d] \in receiverEvidence
    /\ waiting' = waiting \ {d}
    /\ accepted' = accepted \cup {d}
    /\ UNCHANGED <<canonicalEvidence, receiverEvidence, trackerWitnesses,
                    submitted, requested, rejectedAsUnauthorized,
                    localAbsenceRejected>>

Next ==
    \/ \E h \in Hashes : RecordEvidence(h)
    \/ \E h \in Hashes : RecordTrackerWitness(h)
    \/ \E d \in Deploys : SubmitAuthorizedSlash(d)
    \/ \E d \in Deploys : ReceiveAuthorizedSlash(d)
    \/ \E h \in Hashes : FetchEvidence(h)
    \/ \E d \in Deploys : ResumeAuthorizedSlash(d)

Fairness ==
    /\ \A d \in Deploys : WF_vars(ReceiveAuthorizedSlash(d))
    /\ \A h \in Hashes : WF_vars(FetchEvidence(h))
    /\ \A d \in Deploys : WF_vars(ResumeAuthorizedSlash(d))

Spec == Init /\ [][Next]_vars /\ Fairness

Inv_NoCanonicalEvidenceRejectedForLocalAbsence ==
    ~ localAbsenceRejected

Inv_WaitingDependencyTracked ==
    \A d \in waiting : SlashTarget[d] \in requested \cup receiverEvidence

Inv_RequestedDependencyIsCanonical ==
    requested \subseteq canonicalEvidence

Inv_TrackerWitnessIsCanonical ==
    trackerWitnesses \subseteq canonicalEvidence

Inv_AcceptedDependencyWasFetched ==
    \A d \in accepted : SlashTarget[d] \in receiverEvidence

Inv_ClassificationsDisjoint ==
    /\ waiting \cap accepted = {}
    /\ waiting \cap rejectedAsUnauthorized = {}
    /\ accepted \cap rejectedAsUnauthorized = {}

Live_SubmittedSlashEventuallyClassified ==
    \A d \in Deploys :
        d \in submitted ~> d \in accepted \cup rejectedAsUnauthorized

=============================================================================
