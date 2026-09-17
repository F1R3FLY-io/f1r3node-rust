-------------------- MODULE FundingConsentPublication --------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

CONSTANTS Workers, Rights, ReservationIds, AttemptsPerWorker,
          IgnoreGeneration, UseCurrentOwner, RestoreTransferSnapshot,
          RestoreTransferFlags, OverwriteReservation, RepeatSettlement,
          RestoreAbortSnapshot, ChangeOtherRight, RequireGlobalFreshness,
          UseHighestCeiling

ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Cardinality(Rights) >= 2 /\ IsFiniteSet(Rights)
       /\ ReservationIds # {} /\ IsFiniteSet(ReservationIds)
       /\ AttemptsPerWorker \in Nat \ {0}
       /\ \A flag \in {IgnoreGeneration, UseCurrentOwner, RestoreTransferSnapshot,
                       RestoreTransferFlags, OverwriteReservation, RepeatSettlement,
                       RestoreAbortSnapshot, ChangeOtherRight, RequireGlobalFreshness,
                       UseHighestCeiling} : flag \in BOOLEAN

Term(p) == IF p = 0
          THEN [owners |-> <<10, 11>>, ceilings |-> <<2, 4>>,
                limit |-> <<TRUE>>, asset |-> 1, schedule |-> 7]
          ELSE [owners |-> <<20, 21, 22>>, ceilings |-> <<4, 6, 8>>,
                limit |-> <<FALSE>>, asset |-> 1, schedule |-> 8]
Policies == {Term(0), Term(1)}
Permitted(terms, price) ==
    \A i \in 1..Len(terms.ceilings) : price <= terms.ceilings[i]
ImplementationPermitted(terms, price) ==
    IF UseHighestCeiling
    THEN \E i \in 1..Len(terms.ceilings) : price <= terms.ceilings[i]
    ELSE Permitted(terms, price)
FirstRight == CHOOSE r \in Rights : TRUE
FirstId == CHOOSE id \in ReservationIds : TRUE
EmptyReservation == [present |-> FALSE, right |-> FirstRight, generation |-> 0,
                     terms |-> Term(0), root |-> 0, price |-> 0]

VARIABLES rights, reservations, originalReservations, issued, settlementCounts,
          actualSettled, attempts, pending, epoch, expectedRights, admissionValid,
          transferFresh, settlementReceipts, abortPreserved
vars == <<rights, reservations, originalReservations, issued, settlementCounts,
          actualSettled, attempts, pending, epoch, expectedRights, admissionValid,
          transferFresh, settlementReceipts, abortPreserved>>

EmptyPending == [phase |-> "idle", right |-> FirstRight, id |-> FirstId,
    capturedRight |-> [generation |-> 0, terms |-> Term(0)],
    replacement |-> Term(0), root |-> 0, price |-> 0,
    capturedReservation |-> EmptyReservation,
    savedRights |-> [r \in Rights |-> [generation |-> 0, terms |-> Term(0)]],
    savedReservations |-> [id \in ReservationIds |-> EmptyReservation],
    savedSettled |-> [id \in ReservationIds |-> FALSE]]

Init ==
    /\ rights = [r \in Rights |-> [generation |-> 0, terms |-> Term(0)]]
    /\ expectedRights = rights
    /\ reservations = [id \in ReservationIds |-> EmptyReservation]
    /\ originalReservations = reservations
    /\ issued = {}
    /\ actualSettled = [id \in ReservationIds |-> FALSE]
    /\ settlementCounts = [id \in ReservationIds |-> 0]
    /\ attempts = [w \in Workers |-> 0]
    /\ pending = [w \in Workers |-> EmptyPending]
    /\ epoch = 0
    /\ admissionValid = TRUE
    /\ transferFresh = TRUE
    /\ abortPreserved = TRUE
    /\ settlementReceipts = {}

CanPrepare(w) == pending[w].phase = "idle" /\ attempts[w] < AttemptsPerWorker

Prepare(w, kind, right, id, replacement, price) ==
    /\ CanPrepare(w)
    /\ kind \in {"transfer", "reserve", "settle"}
    /\ (kind = "settle" => reservations[id].present /\ ~actualSettled[id])
    /\ pending' = [pending EXCEPT ![w] =
         [phase |-> kind, right |-> right, id |-> id,
          capturedRight |-> rights[right], replacement |-> replacement,
          root |-> epoch, price |-> price, capturedReservation |-> reservations[id],
          savedRights |-> rights, savedReservations |-> reservations,
          savedSettled |-> actualSettled]]
    /\ attempts' = [attempts EXCEPT ![w] = @ + 1]
    /\ UNCHANGED <<rights, reservations, originalReservations, issued,
         settlementCounts, actualSettled, epoch, expectedRights, admissionValid,
         transferFresh, settlementReceipts, abortPreserved>>

FreshRight(plan) == plan.capturedRight.generation = rights[plan.right].generation
GlobalGuard(plan) == ~RequireGlobalFreshness \/ plan.root = epoch

CommitTransfer(w) ==
    /\ pending[w].phase = "transfer"
    /\ LET plan == pending[w]
           replacement == [generation |-> plan.capturedRight.generation + 1,
                           terms |-> plan.replacement]
           target == IF ChangeOtherRight
                     THEN CHOOSE r \in Rights : r # plan.right
                     ELSE plan.right
       IN /\ (IgnoreGeneration \/ FreshRight(plan))
          /\ GlobalGuard(plan)
          /\ rights' = [rights EXCEPT ![target] = replacement]
          /\ expectedRights' = [expectedRights EXCEPT ![plan.right] = replacement]
          /\ reservations' = IF RestoreTransferSnapshot
                             THEN plan.savedReservations ELSE reservations
          /\ actualSettled' = IF RestoreTransferFlags THEN plan.savedSettled ELSE actualSettled
          /\ transferFresh' = (transferFresh /\ FreshRight(plan))
    /\ epoch' = epoch + 1
    /\ pending' = [pending EXCEPT ![w] = EmptyPending]
    /\ UNCHANGED <<originalReservations, issued, settlementCounts, attempts,
         admissionValid, settlementReceipts, abortPreserved>>

LocallyEligibleReservation(w) ==
    /\ pending[w].phase = "reserve"
    /\ FreshRight(pending[w])
    /\ pending[w].id \notin issued
    /\ Permitted(pending[w].capturedRight.terms, pending[w].price)

CommitReservation(w) ==
    /\ pending[w].phase = "reserve"
    /\ LET plan == pending[w]
           record == [present |-> TRUE, right |-> plan.right,
                      generation |-> plan.capturedRight.generation,
                      terms |-> plan.capturedRight.terms, root |-> plan.root,
                      price |-> plan.price]
       IN /\ (IgnoreGeneration \/ FreshRight(plan))
          /\ (OverwriteReservation \/ plan.id \notin issued)
          /\ ImplementationPermitted(plan.capturedRight.terms, plan.price)
          /\ GlobalGuard(plan)
          /\ reservations' = [reservations EXCEPT ![plan.id] = record]
          /\ originalReservations' = IF plan.id \in issued THEN originalReservations
              ELSE [originalReservations EXCEPT ![plan.id] = record]
          /\ issued' = issued \cup {plan.id}
          /\ admissionValid' = (admissionValid /\ FreshRight(plan)
              /\ Permitted(plan.capturedRight.terms, plan.price))
    /\ epoch' = epoch + 1
    /\ pending' = [pending EXCEPT ![w] = EmptyPending]
    /\ UNCHANGED <<rights, expectedRights, settlementCounts, actualSettled,
         attempts, transferFresh, settlementReceipts, abortPreserved>>

CommitSettlement(w) ==
    /\ pending[w].phase = "settle"
    /\ LET plan == pending[w]
           captured == plan.capturedReservation
           receipt == IF UseCurrentOwner
                      THEN [captured EXCEPT !.terms = rights[captured.right].terms]
                      ELSE captured
       IN /\ reservations[plan.id] = captured
          /\ captured.present
          /\ (RepeatSettlement \/ ~actualSettled[plan.id])
          /\ actualSettled' = [actualSettled EXCEPT ![plan.id] = TRUE]
          /\ settlementCounts' = [settlementCounts EXCEPT ![plan.id] = @ + 1]
          /\ settlementReceipts' = settlementReceipts \cup
              {[id |-> plan.id, evidence |-> receipt,
                liveRight |-> rights[captured.right]]}
    /\ epoch' = epoch + 1
    /\ pending' = [pending EXCEPT ![w] = EmptyPending]
    /\ UNCHANGED <<rights, expectedRights, reservations, originalReservations,
         issued, attempts, admissionValid, transferFresh, abortPreserved>>

Abort(w) ==
    /\ pending[w].phase # "idle"
    /\ LET restoredRights == IF RestoreAbortSnapshot THEN pending[w].savedRights ELSE rights
           restoredReservations == IF RestoreAbortSnapshot
                                   THEN pending[w].savedReservations ELSE reservations
           restoredFlags == IF RestoreAbortSnapshot THEN pending[w].savedSettled ELSE actualSettled
       IN /\ rights' = restoredRights
          /\ reservations' = restoredReservations
          /\ actualSettled' = restoredFlags
          /\ abortPreserved' = (abortPreserved /\ restoredRights = rights
              /\ restoredReservations = reservations /\ restoredFlags = actualSettled)
    /\ pending' = [pending EXCEPT ![w] = EmptyPending]
    /\ UNCHANGED <<expectedRights, originalReservations, issued, settlementCounts,
         epoch, attempts, admissionValid, transferFresh, settlementReceipts>>

Next == (\E w \in Workers, right \in Rights, replacement \in Policies :
             Prepare(w, "transfer", right, FirstId, replacement, 0))
     \/ (\E w \in Workers, right \in Rights, id \in ReservationIds, price \in {1, 3} :
             Prepare(w, "reserve", right, id, Term(0), price))
     \/ (\E w \in Workers, id \in ReservationIds :
             Prepare(w, "settle", FirstRight, id, Term(0), 0))
     \/ (\E w \in Workers : CommitTransfer(w) \/ CommitReservation(w)
                               \/ CommitSettlement(w) \/ Abort(w))
Spec == Init /\ [][Next]_vars

OwnerReturnNext ==
    /\ Next
    /\ \A r \in Rights :
        rights'[r].generation = rights[r].generation + 1 =>
          rights'[r].terms = (IF rights[r].generation = 0 THEN Term(1) ELSE Term(0))
OwnerReturnSpec == Init /\ [][OwnerReturnNext]_vars

CapturedReservationsRemainImmutable ==
    \A id \in issued : reservations[id] = originalReservations[id]
SettlementUsesCapturedConsent ==
    \A receipt \in settlementReceipts : receipt.evidence = originalReservations[receipt.id]
AtMostOneSettlement == \A id \in ReservationIds : settlementCounts[id] <= 1
SettledFlagsMatchReceipts ==
    \A id \in ReservationIds : actualSettled[id] = (settlementCounts[id] > 0)
TransfersUseFreshAuthorization == transferFresh
ReservationsUseRequiredConsent == admissionValid
RightChangesStayInScope == rights = expectedRights
AbortPreservesCommittedState == abortPreserved
IndependentReservationRemainsEnabled ==
    \A w \in Workers : LocallyEligibleReservation(w) => ENABLED CommitReservation(w)
NoOwnerReturnWithStaleReservation ==
    ~\E w \in Workers :
        /\ pending[w].phase = "reserve"
        /\ pending[w].capturedRight.generation = 0
        /\ rights[pending[w].right].generation = 2
        /\ rights[pending[w].right].terms = Term(0)
NoSettlementAcrossOwnershipTransfer ==
    ~\E receipt \in settlementReceipts :
        /\ receipt.evidence.generation < receipt.liveRight.generation
        /\ receipt.evidence.terms # receipt.liveRight.terms

=============================================================================
