------------------------ MODULE RSpaceDagStateTransfer ------------------------
(***************************************************************************
Source-bound DAG state transfer and durable resume (task 7938, plan section
4.6), layered on the unchanged affine import lifecycle.

A transfer session is the tuple (binding digest, catalog generation, bridge
version, transfer version, profile), and the resume token is (session,
acknowledged frontier).  The binding digest is abstract: it names the exact
coupled semantic-plus-compatibility closure and the physical representation
that the source published at one catalog generation.  Its durable carrier and
the read path are pending rulings and are not modelled.

The affine session `target` is the importer's adoption capability.  Only cold
admission (its terminal page), seal and publication touch it, so a refused or
aborted transfer leaves it unused.  The two other affine sessions keep the
inherited lifecycle and race the transfer for the current root.
***************************************************************************)
EXTENDS RSpaceAffineImportLifecycle

(***************************************************************************
Bounded domains.  Generation 2 restamps the same semantic state in another
physical representation (RetentionV2Format theorems 14 and 15): the state
version is unchanged while the binding digest differs.  Object 1 is the root
of a closure and object 2 its child.
***************************************************************************)
ClosureSize == 2
Positions == 1..ClosureSize
Frontiers == 0..ClosureSize
SourceGenerations == {1, 2}
BindingDigests == {"b1", "b2"}
Objects == BindingDigests \X Positions
BindingOf(g) == IF g = 1 THEN "b1" ELSE "b2"
StateVersionOf(g) == "s1"
Closure(b) == {b} \X Positions

\* PreservedLegacyV1 is the only activated bridge; DirectManifestEpoch stands
\* for every unactivated identity version (CI-10, CI-11, CI-12).
Bridges == {"PreservedLegacyV1", "DirectManifestEpoch"}
SupportedBridges == {"PreservedLegacyV1"}

\* The durable importer speaks two DAG transfer versions and never the legacy
\* page protocol (plan 4.5 corollary).
WireVersions == {"legacy-pages", "dag-v1", "dag-v2"}
ImporterVersions == {"dag-v1", "dag-v2"}
NoVersion == "none"
Profiles == {"p1", "p2"}
ImporterProfile == "p1"

\* A frontier means something only under the export order of one transfer
\* version.  dag-v1 is parent-first; a later version may reorder.
ExportOrder(t) == IF t = "dag-v2" THEN <<2, 1>> ELSE <<1, 2>>
ObjectAt(b, t, i) == <<b, ExportOrder(t)[i]>>

\* The source configurations a (re)connection may meet.
Peers == {"legacy-only", "dag-v1", "dag-v2", "dag-epoch", "dag-p2"}
Offers(p) ==
    CASE p = "legacy-only" -> {"legacy-pages"}
      [] p = "dag-v2" -> {"legacy-pages", "dag-v2"}
      [] OTHER -> {"legacy-pages", "dag-v1"}
PeerBridge(p) ==
    IF p = "dag-epoch" THEN "DirectManifestEpoch" ELSE "PreservedLegacyV1"
PeerProfile(p) == IF p = "dag-p2" THEN "p2" ELSE "p1"
Negotiated(p) ==
    IF Offers(p) \cap ImporterVersions = {}
    THEN NoVersion
    ELSE CHOOSE t \in Offers(p) \cap ImporterVersions: TRUE

TransferSessions ==
    [binding: BindingDigests, catalogGeneration: SourceGenerations,
     bridge: Bridges, transfer: WireVersions, profile: Profiles]
NoSession ==
    [binding |-> "none", catalogGeneration |-> 0, bridge |-> "none",
     transfer |-> NoVersion, profile |-> "none"]
SessionFor(p, g) ==
    [binding |-> BindingOf(g), catalogGeneration |-> g,
     bridge |-> PeerBridge(p), transfer |-> Negotiated(p),
     profile |-> PeerProfile(p)]
Acknowledged(s, f) == {ObjectAt(s.binding, s.transfer, i): i \in 1..f}
NextFrontier(f) == IF f < ClosureSize THEN f + 1 ELSE ClosureSize
Events ==
    {"none", "begun", "resumed", "refused-transfer", "refused-bridge",
     "refused-profile", "refused-pin", "aborted", "admitted"}

VARIABLES target, peer, link, token, staged,
          sourceGeneration, retained, sourcePins, lastEvent, abortedOnce

transferVars ==
    <<target, peer, link, token, staged,
      sourceGeneration, retained, sourcePins, lastEvent, abortedOnce>>
allVars == <<vars, transferVars>>

PinnedGenerations == {s.catalogGeneration: s \in sourcePins}

IdentityRefusal(p) ==
    IF Negotiated(p) = NoVersion THEN "refused-transfer"
    ELSE IF PeerBridge(p) \notin SupportedBridges THEN "refused-bridge"
    ELSE IF PeerProfile(p) # ImporterProfile THEN "refused-profile"
    ELSE "ok"

ResumeRefusal(s, p) ==
    IF Negotiated(p) # s.transfer THEN "refused-transfer"
    ELSE IF PeerBridge(p) # s.bridge THEN "refused-bridge"
    ELSE IF PeerProfile(p) # s.profile THEN "refused-profile"
    ELSE IF s \notin sourcePins THEN "refused-pin"
    ELSE "ok"

TransferInit ==
    /\ Init
    /\ target \in {"activate-a", "preserve"}
    /\ peer = "none"
    /\ link = "down"
    /\ token = [session |-> NoSession, frontier |-> 0]
    /\ staged = {}
    /\ sourceGeneration = 1
    /\ retained = {1}
    /\ sourcePins = {}
    /\ lastEvent = "none"
    /\ abortedOnce = FALSE

(***************************************************************************
Negotiation binds a new session to the source's current catalog generation
and pins it, or refuses an unsupported identity.  A refusal opens nothing
and selects no other version, bridge, profile or generation.
***************************************************************************)
Negotiate(p) ==
    LET fresh == SessionFor(p, sourceGeneration)
    IN  /\ token.session = NoSession
        /\ link = "down"
        /\ phase[target] = "open"
        /\ ~closureVerified[target]
        /\ peer' = p
        /\ IF IdentityRefusal(p) = "ok"
             THEN /\ token' = [session |-> fresh, frontier |-> 0]
                  /\ sourcePins' = sourcePins \cup {fresh}
                  /\ link' = "streaming"
                  /\ lastEvent' = "begun"
             ELSE /\ lastEvent' = IdentityRefusal(p)
                  /\ UNCHANGED <<token, sourcePins, link>>
        /\ UNCHANGED <<target, staged, sourceGeneration, retained, abortedOnce>>
        /\ UNCHANGED vars

(***************************************************************************
The source serves the next page from the generation its pin holds, in the
order of the transfer version spoken on the link; the importer stages the
object privately (content-addressed and unreachable until publication).
***************************************************************************)
ServePage ==
    /\ link = "streaming"
    /\ token.frontier < ClosureSize
    /\ token.session \in sourcePins
    /\ staged' = staged \cup {ObjectAt(BindingOf(token.session.catalogGeneration),
                                       Negotiated(peer), token.frontier + 1)}
    /\ UNCHANGED <<target, peer, link, token, sourceGeneration, retained,
                   sourcePins, lastEvent, abortedOnce>>
    /\ UNCHANGED vars

\* The frontier is persisted only after the object at that position, in the
\* session's own order, is durably staged.
AcknowledgeFrontier ==
    /\ token.session # NoSession
    /\ token.frontier < ClosureSize
    /\ ObjectAt(token.session.binding, token.session.transfer,
                token.frontier + 1) \in staged
    /\ token' = [token EXCEPT !.frontier = @ + 1]
    /\ UNCHANGED <<target, peer, link, staged, sourceGeneration, retained,
                   sourcePins, lastEvent, abortedOnce>>
    /\ UNCHANGED vars

\* Importer crash, network loss or source restart loses only the link; the
\* token, the staged objects and the source pins are durable.
Disconnect ==
    /\ link = "streaming"
    /\ link' = "down"
    /\ UNCHANGED <<target, peer, token, staged, sourceGeneration, retained,
                   sourcePins, lastEvent, abortedOnce>>
    /\ UNCHANGED vars

\* The source drops its transfer pins (restart without durable pins, or lease
\* expiry) while no stream depends on them.
PinLapse ==
    /\ link = "down"
    /\ sourcePins # {}
    /\ sourcePins' = {}
    /\ UNCHANGED <<target, peer, link, token, staged, sourceGeneration,
                   retained, lastEvent, abortedOnce>>
    /\ UNCHANGED vars

\* The source publishes generation 2, the same state restamped in another
\* physical representation.  Publication never moves a pin.
SourcePublish ==
    /\ sourceGeneration = 1
    /\ sourceGeneration' = 2
    /\ retained' = retained \cup {2}
    /\ UNCHANGED <<target, peer, link, token, staged, sourcePins, lastEvent,
                   abortedOnce>>
    /\ UNCHANGED vars

\* The source reclaims the superseded generation when no transfer pins it.
SourceReclaim ==
    /\ sourceGeneration = 2
    /\ 1 \in retained
    /\ 1 \notin PinnedGenerations
    /\ retained' = retained \ {1}
    /\ UNCHANGED <<target, peer, link, token, staged, sourceGeneration,
                   sourcePins, lastEvent, abortedOnce>>
    /\ UNCHANGED vars

(***************************************************************************
Resume presents the token.  The stream continues only if it speaks the
token's transfer version, bridge and profile and the source still pins the
token's exact session; otherwise the token is kept and nothing streams.
***************************************************************************)
Reconnect(p) ==
    /\ token.session # NoSession
    /\ link = "down"
    /\ ~closureVerified[target]
    /\ peer' = p
    /\ IF ResumeRefusal(token.session, p) = "ok"
         THEN /\ link' = "streaming"
              /\ lastEvent' = "resumed"
         ELSE /\ link' = "down"
              /\ lastEvent' = ResumeRefusal(token.session, p)
    /\ UNCHANGED <<target, token, staged, sourceGeneration, retained,
                   sourcePins, abortedOnce>>
    /\ UNCHANGED vars

\* Abandoning the token before admission discards its staging and releases
\* its pin (release or lease expiry).  Nothing is published.
Abort ==
    /\ token.session # NoSession
    /\ ~closureVerified[target]
    /\ token' = [session |-> NoSession, frontier |-> 0]
    /\ staged' = {}
    /\ sourcePins' = sourcePins \ {token.session}
    /\ link' = "down"
    /\ peer' = "none"
    /\ lastEvent' = "aborted"
    /\ abortedOnce' = TRUE
    /\ UNCHANGED <<target, sourceGeneration, retained>>
    /\ UNCHANGED vars

(***************************************************************************
Full cold admission: at the complete frontier, with the whole closure of the
session binding staged, the capability stages its terminal page and verifies
closure through the inherited StageTerminalPage.  The pin is released.
***************************************************************************)
AdmitClosure ==
    /\ token.session # NoSession
    /\ token.frontier = ClosureSize
    /\ Closure(token.session.binding) \subseteq staged
    /\ StageTerminalPage(target)
    /\ link' = "down"
    /\ peer' = "none"
    /\ sourcePins' = sourcePins \ {token.session}
    /\ lastEvent' = "admitted"
    /\ UNCHANGED <<target, token, staged, sourceGeneration, retained,
                   abortedOnce>>

\* Seal and publication of the capability are the inherited actions.
SealTarget ==
    /\ SealComplete(target)
    /\ UNCHANGED transferVars

PublishTarget ==
    /\ \/ PublishPreservingCurrent(target)
       \/ PublishActivation(target)
    /\ UNCHANGED transferVars

RejectTarget ==
    /\ RejectStaleActivation(target)
    /\ UNCHANGED transferVars

\* The other affine sessions keep the inherited free lifecycle.
OtherAffineStep ==
    /\ \E s \in Sessions \ {target}:
          \/ StageTerminalPage(s)
          \/ SealComplete(s)
          \/ PublishPreservingCurrent(s)
          \/ PublishActivation(s)
          \/ RejectStaleActivation(s)
    /\ UNCHANGED transferVars

TransferNext ==
    \/ \E p \in Peers: Negotiate(p)
    \/ \E p \in Peers: Reconnect(p)
    \/ ServePage
    \/ AcknowledgeFrontier
    \/ Disconnect
    \/ PinLapse
    \/ SourcePublish
    \/ SourceReclaim
    \/ Abort
    \/ AdmitClosure
    \/ SealTarget
    \/ PublishTarget
    \/ RejectTarget
    \/ OtherAffineStep

TransferSpec == TransferInit /\ [][TransferNext]_allVars

(***************************************************************************
Invariants.
***************************************************************************)
TransferTypeInvariant ==
    /\ TypeInvariant
    /\ target \in {"activate-a", "preserve"}
    /\ peer \in Peers \cup {"none"}
    /\ link \in {"down", "streaming"}
    /\ token \in [session: TransferSessions \cup {NoSession}, frontier: Frontiers]
    /\ staged \subseteq Objects
    /\ sourceGeneration \in SourceGenerations
    /\ retained \subseteq SourceGenerations
    /\ sourcePins \subseteq TransferSessions
    /\ lastEvent \in Events
    /\ abortedOnce \in BOOLEAN

\* The session names the exact binding digest its source published at the
\* session's catalog generation.
SessionBindingIsExact ==
    token.session # NoSession =>
        token.session.binding = BindingOf(token.session.catalogGeneration)

\* No session exists under an unsupported identity: no legacy-page fallback,
\* no unactivated bridge, no foreign profile.
SessionIdentityIsSupported ==
    token.session # NoSession =>
        /\ token.session.transfer \in ImporterVersions
        /\ token.session.bridge \in SupportedBridges
        /\ token.session.profile = ImporterProfile

\* Every live stream, fresh or resumed, speaks exactly the token's transfer
\* version, bridge and profile: no mixed-version resume.
StreamMatchesSessionIdentity ==
    link = "streaming" =>
        /\ token.session # NoSession
        /\ Negotiated(peer) = token.session.transfer
        /\ PeerBridge(peer) = token.session.bridge
        /\ PeerProfile(peer) = token.session.profile

\* Authoritative pins: a live stream is served under the source's pin of the
\* token's exact session, and no pinned generation is reclaimed.
ServingIsPinned == link = "streaming" => token.session \in sourcePins
PinnedGenerationsAreRetained == PinnedGenerations \subseteq retained

\* No mixed session: every staged object belongs to the token's binding.
StagingIsSessionBound == \A o \in staged: o[1] = token.session.binding

\* No skipped frontier: the acknowledged prefix is staged and at most the next
\* page lies beyond it.
FrontierIsContiguous ==
    /\ Acknowledged(token.session, token.frontier) \subseteq staged
    /\ staged \subseteq Acknowledged(token.session, NextFrontier(token.frontier))

\* Complete closure and full cold admission precede known or current adoption.
ColdAdmissionRequiresCompleteClosure ==
    closureVerified[target] =>
        /\ token.session # NoSession
        /\ token.frontier = ClosureSize
        /\ Closure(token.session.binding) \subseteq staged

AdoptionRequiresAdmittedClosure ==
    phase[target] = "published" =>
        /\ closureVerified[target]
        /\ token.session # NoSession
        /\ token.frontier = ClosureSize
        /\ staged = Closure(token.session.binding)
        /\ token.session.transfer \in ImporterVersions
        /\ token.session.bridge \in SupportedBridges
        /\ token.session.profile = ImporterProfile

\* Without a session the importer holds no staging, frontier or stream, and
\* the capability is unused.
NoSessionHoldsNothing ==
    token.session = NoSession =>
        /\ staged = {}
        /\ token.frontier = 0
        /\ link = "down"
        /\ phase[target] = "open"
        /\ pendingPages[target] = 1
        /\ ~closureVerified[target]

(***************************************************************************
Properties.  The inherited Spec holds of every transfer behavior, so each
step is an affine step or leaves the affine state unchanged; within one
session the frontier never regresses or skips.
***************************************************************************)
AffineLifecycleIsPreserved == Spec

FrontierAdvancesByOne ==
    [][token.session' = token.session =>
           token.frontier' \in {token.frontier, token.frontier + 1}]_allVars

(***************************************************************************
Reachability witnesses: TransferSpec must violate each.
***************************************************************************)
NeverAdoptsCurrent == ~(phase[target] = "published" /\ Mode(target) = "activate")
NeverAdoptsKnown == ~(phase[target] = "published" /\ Mode(target) = "preserve")
NeverRefusesLegacyOnlyPeer ==
    ~(/\ token.session = NoSession
      /\ peer = "legacy-only"
      /\ lastEvent = "refused-transfer")
NeverRefusesUnsupportedBridge ==
    ~(token.session = NoSession /\ lastEvent = "refused-bridge")
NeverRefusesProfileMismatch ==
    ~(token.session = NoSession /\ lastEvent = "refused-profile")
NeverResumesAfterDisconnect == lastEvent # "resumed"
NeverRefusesResumeOnLegacyOnlyPeer ==
    ~(/\ token.session # NoSession
      /\ peer = "legacy-only"
      /\ lastEvent = "refused-transfer")
NeverRefusesResumeOnChangedTransferVersion ==
    ~(/\ token.session # NoSession
      /\ peer \in {"dag-v1", "dag-v2"}
      /\ lastEvent = "refused-transfer")
NeverRefusesResumeOnBridgeChange ==
    ~(token.session # NoSession /\ lastEvent = "refused-bridge")
NeverRefusesResumeOnProfileChange ==
    ~(token.session # NoSession /\ lastEvent = "refused-profile")
NeverRefusesResumeAfterPinLapse == lastEvent # "refused-pin"
NeverResumesAcrossSourcePublication ==
    ~(lastEvent = "resumed" /\ token.session.catalogGeneration < sourceGeneration)
NeverStagesBeyondFrontier ==
    ~(/\ link = "down"
      /\ token.session # NoSession
      /\ staged # Acknowledged(token.session, token.frontier))
NeverAdoptsAfterAbort == ~(phase[target] = "published" /\ abortedOnce)
NeverRejectsTransferActivation == phase[target] # "rejected"
NeverReclaimsSupersededGeneration == 1 \in retained

(***************************************************************************
Negative controls.  Each weakens one guard or update of the faithful model.
***************************************************************************)
\* Plan mutant 1, cross-version resume: the resume check consults only the pin.
CrossVersionResumeMutant(p) ==
    /\ token.session # NoSession
    /\ link = "down"
    /\ ~closureVerified[target]
    /\ token.session \in sourcePins
    /\ peer' = p
    /\ link' = "streaming"
    /\ lastEvent' = "resumed"
    /\ UNCHANGED <<target, token, staged, sourceGeneration, retained,
                   sourcePins, abortedOnce>>
    /\ UNCHANGED vars

\* Plan mutant 2, skipped frontier: the page after the next one is accepted
\* and its position acknowledged.
SkippedFrontierMutant ==
    /\ link = "streaming"
    /\ token.frontier + 2 <= ClosureSize
    /\ token.session \in sourcePins
    /\ staged' = staged \cup {ObjectAt(BindingOf(token.session.catalogGeneration),
                                       Negotiated(peer), token.frontier + 2)}
    /\ token' = [token EXCEPT !.frontier = @ + 2]
    /\ UNCHANGED <<target, peer, link, sourceGeneration, retained, sourcePins,
                   lastEvent, abortedOnce>>
    /\ UNCHANGED vars

\* Plan mutant 3, mixed session: a new session is opened over an existing token
\* and inherits its staging and frontier.
MixedSessionMutant(p) ==
    /\ token.session # NoSession
    /\ link = "down"
    /\ ~closureVerified[target]
    /\ IdentityRefusal(p) = "ok"
    /\ peer' = p
    /\ token' = [session |-> SessionFor(p, sourceGeneration),
                 frontier |-> token.frontier]
    /\ sourcePins' = sourcePins \cup {SessionFor(p, sourceGeneration)}
    /\ link' = "streaming"
    /\ lastEvent' = "begun"
    /\ UNCHANGED <<target, staged, sourceGeneration, retained, abortedOnce>>
    /\ UNCHANGED vars

\* RetentionV2Format theorem 15 analogue: the token is matched by state version
\* and refreshed to the generation now holding that state, keeping its digest.
StateVersionOnlyResumeMutant(p) ==
    LET refreshed == [token.session EXCEPT !.catalogGeneration = sourceGeneration]
    IN  /\ token.session # NoSession
        /\ link = "down"
        /\ ~closureVerified[target]
        /\ Negotiated(p) = token.session.transfer
        /\ PeerBridge(p) = token.session.bridge
        /\ PeerProfile(p) = token.session.profile
        /\ StateVersionOf(token.session.catalogGeneration) = StateVersionOf(sourceGeneration)
        /\ peer' = p
        /\ token' = [token EXCEPT !.session = refreshed]
        /\ sourcePins' = (sourcePins \ {token.session}) \cup {refreshed}
        /\ link' = "streaming"
        /\ lastEvent' = "resumed"
        /\ UNCHANGED <<target, staged, sourceGeneration, retained, abortedOnce>>
        /\ UNCHANGED vars

\* The resume check omits the authoritative pin.
ResumeWithoutPinMutant(p) ==
    /\ token.session # NoSession
    /\ link = "down"
    /\ ~closureVerified[target]
    /\ Negotiated(p) = token.session.transfer
    /\ PeerBridge(p) = token.session.bridge
    /\ PeerProfile(p) = token.session.profile
    /\ peer' = p
    /\ link' = "streaming"
    /\ lastEvent' = "resumed"
    /\ UNCHANGED <<target, token, staged, sourceGeneration, retained,
                   sourcePins, abortedOnce>>
    /\ UNCHANGED vars

\* Reclamation ignores transfer pins.
ReclaimPinnedMutant ==
    /\ sourceGeneration = 2
    /\ 1 \in retained
    /\ retained' = retained \ {1}
    /\ UNCHANGED <<target, peer, link, token, staged, sourceGeneration,
                   sourcePins, lastEvent, abortedOnce>>
    /\ UNCHANGED vars

\* Plan 4.5 corollary: with no common DAG version the importer falls back to
\* the legacy page protocol.
LegacyFallbackMutant(p) ==
    LET fallback == [SessionFor(p, sourceGeneration) EXCEPT !.transfer = "legacy-pages"]
    IN  /\ token.session = NoSession
        /\ link = "down"
        /\ phase[target] = "open"
        /\ ~closureVerified[target]
        /\ Negotiated(p) = NoVersion
        /\ "legacy-pages" \in Offers(p)
        /\ peer' = p
        /\ token' = [session |-> fallback, frontier |-> 0]
        /\ sourcePins' = sourcePins \cup {fallback}
        /\ link' = "streaming"
        /\ lastEvent' = "begun"
        /\ UNCHANGED <<target, staged, sourceGeneration, retained, abortedOnce>>
        /\ UNCHANGED vars

\* An unactivated bridge identity is answered by selecting the older
\* PreservedLegacyV1 authority instead of refusing.
OlderAuthorityMutant(p) ==
    LET older == [SessionFor(p, sourceGeneration) EXCEPT !.bridge = "PreservedLegacyV1"]
    IN  /\ token.session = NoSession
        /\ link = "down"
        /\ phase[target] = "open"
        /\ ~closureVerified[target]
        /\ Negotiated(p) # NoVersion
        /\ PeerBridge(p) \notin SupportedBridges
        /\ PeerProfile(p) = ImporterProfile
        /\ peer' = p
        /\ token' = [session |-> older, frontier |-> 0]
        /\ sourcePins' = sourcePins \cup {older}
        /\ link' = "streaming"
        /\ lastEvent' = "begun"
        /\ UNCHANGED <<target, staged, sourceGeneration, retained, abortedOnce>>
        /\ UNCHANGED vars

\* Cold admission hands the capability its terminal page before the frontier
\* covers the closure.
EarlyAdmissionMutant ==
    /\ token.session # NoSession
    /\ token.frontier < ClosureSize
    /\ StageTerminalPage(target)
    /\ link' = "down"
    /\ peer' = "none"
    /\ sourcePins' = sourcePins \ {token.session}
    /\ lastEvent' = "admitted"
    /\ UNCHANGED <<target, token, staged, sourceGeneration, retained,
                   abortedOnce>>

\* The frontier is persisted before the object it covers is staged.
FrontierBeforeObjectsMutant ==
    /\ token.session # NoSession
    /\ token.frontier < ClosureSize
    /\ token' = [token EXCEPT !.frontier = @ + 1]
    /\ UNCHANGED <<target, peer, link, staged, sourceGeneration, retained,
                   sourcePins, lastEvent, abortedOnce>>
    /\ UNCHANGED vars

\* The inherited incomplete publication, applied to the transfer capability.
AffineIncompleteAdoptionMutant ==
    /\ PublishIncompleteMutant(target)
    /\ UNCHANGED transferVars

\* The transfer is abandoned after cold admission, leaving the capability
\* holding a closure that no session backs.
AbortAfterAdmissionMutant ==
    /\ token.session # NoSession
    /\ closureVerified[target]
    /\ token' = [session |-> NoSession, frontier |-> 0]
    /\ staged' = {}
    /\ sourcePins' = sourcePins \ {token.session}
    /\ link' = "down"
    /\ peer' = "none"
    /\ lastEvent' = "aborted"
    /\ abortedOnce' = TRUE
    /\ UNCHANGED <<target, sourceGeneration, retained>>
    /\ UNCHANGED vars

CrossVersionResumeMutantNext ==
    TransferNext \/ (\E p \in Peers: CrossVersionResumeMutant(p))
CrossVersionResumeMutantSpec ==
    TransferInit /\ [][CrossVersionResumeMutantNext]_allVars
SkippedFrontierMutantNext == TransferNext \/ SkippedFrontierMutant
SkippedFrontierMutantSpec ==
    TransferInit /\ [][SkippedFrontierMutantNext]_allVars
MixedSessionMutantNext ==
    TransferNext \/ (\E p \in Peers: MixedSessionMutant(p))
MixedSessionMutantSpec ==
    TransferInit /\ [][MixedSessionMutantNext]_allVars
StateVersionOnlyResumeMutantNext ==
    TransferNext \/ (\E p \in Peers: StateVersionOnlyResumeMutant(p))
StateVersionOnlyResumeMutantSpec ==
    TransferInit /\ [][StateVersionOnlyResumeMutantNext]_allVars
ResumeWithoutPinMutantNext ==
    TransferNext \/ (\E p \in Peers: ResumeWithoutPinMutant(p))
ResumeWithoutPinMutantSpec ==
    TransferInit /\ [][ResumeWithoutPinMutantNext]_allVars
ReclaimPinnedMutantNext == TransferNext \/ ReclaimPinnedMutant
ReclaimPinnedMutantSpec ==
    TransferInit /\ [][ReclaimPinnedMutantNext]_allVars
LegacyFallbackMutantNext ==
    TransferNext \/ (\E p \in Peers: LegacyFallbackMutant(p))
LegacyFallbackMutantSpec ==
    TransferInit /\ [][LegacyFallbackMutantNext]_allVars
OlderAuthorityMutantNext ==
    TransferNext \/ (\E p \in Peers: OlderAuthorityMutant(p))
OlderAuthorityMutantSpec ==
    TransferInit /\ [][OlderAuthorityMutantNext]_allVars
EarlyAdmissionMutantNext == TransferNext \/ EarlyAdmissionMutant
EarlyAdmissionMutantSpec ==
    TransferInit /\ [][EarlyAdmissionMutantNext]_allVars
FrontierBeforeObjectsMutantNext == TransferNext \/ FrontierBeforeObjectsMutant
FrontierBeforeObjectsMutantSpec ==
    TransferInit /\ [][FrontierBeforeObjectsMutantNext]_allVars
AffineIncompleteAdoptionMutantNext == TransferNext \/ AffineIncompleteAdoptionMutant
AffineIncompleteAdoptionMutantSpec ==
    TransferInit /\ [][AffineIncompleteAdoptionMutantNext]_allVars
AbortAfterAdmissionMutantNext == TransferNext \/ AbortAfterAdmissionMutant
AbortAfterAdmissionMutantSpec ==
    TransferInit /\ [][AbortAfterAdmissionMutantNext]_allVars

=============================================================================
