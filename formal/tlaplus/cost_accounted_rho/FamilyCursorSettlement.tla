-------------------- MODULE FamilyCursorSettlement --------------------
EXTENDS MonetaryAllocation

CONSTANTS Workers, Transactions, ScopeCount, ExposureLimit,
          CheckFreshness, CheckReceipts, PublishRealized,
          CommitResourceCursor, CommitFeeCursor, AbortChangesCursor,
          SerializeScopes, AbstractRestrictedResource, PreserveAliasGroups
ASSUME /\ Workers # {} /\ IsFiniteSet(Workers)
       /\ Transactions # {} /\ IsFiniteSet(Transactions)
       /\ ScopeCount \in Nat \ {0}
       /\ CapacityBound >= 2
       /\ ExposureLimit \in Nat
       /\ CheckFreshness \in BOOLEAN
       /\ CheckReceipts \in BOOLEAN
       /\ PublishRealized \in BOOLEAN
       /\ CommitResourceCursor \in BOOLEAN
       /\ CommitFeeCursor \in BOOLEAN
       /\ AbortChangesCursor \in BOOLEAN
       /\ SerializeScopes \in BOOLEAN
       /\ AbstractRestrictedResource \in BOOLEAN
       /\ PreserveAliasGroups \in BOOLEAN

Scopes == 0..(ScopeCount - 1)
Groups == 0..2
Outcomes == 0..3
GroupOf(outcome) == IF outcome = 3 THEN 0 ELSE outcome
CapturedGroup(outcome) ==
    IF PreserveAliasGroups THEN GroupOf(outcome)
    ELSE IF outcome = 3 THEN 1 ELSE GroupOf(outcome)
Maximum(left, right) == IF left < right THEN right ELSE left
ZeroDraw == [p \in Payers |-> 0]
ResourceAmount(group) == IF group = 0 THEN 1 ELSE 0
FeeAmount(group) == IF group = 2 THEN 0 ELSE 1

AbstractBranch(caps, resourceStart, feeStart, group) ==
    LET resourceAmount == ResourceAmount(group)
        feeAmount == FeeAmount(group)
        resource == Allocation(caps, resourceAmount, resourceStart)
        residual == [p \in Payers |-> caps[p] - resource[p]]
    IN [resource |-> resource,
        fee |-> Allocation(residual, feeAmount, feeStart),
        resourceAfter |-> IF resourceAmount = 0 THEN resourceStart
                          ELSE IF AbstractRestrictedResource
                               THEN (resourceStart + 1) % PayerCount
                               ELSE NextCursor(caps, resourceAmount, resourceStart),
        feeAfter |-> IF feeAmount = 0 THEN feeStart
                     ELSE NextCursor(residual, feeAmount, feeStart)]

AbstractFamilyCertificate(caps, resourceStart, feeStart) ==
    [g \in Groups |-> AbstractBranch(caps, resourceStart, feeStart, g)]

CertificateHolds(certificate) ==
    [p \in Payers |->
      LET candidate == {certificate[g].resource[p] + certificate[g].fee[p] : g \in Groups}
      IN CHOOSE amount \in candidate : \A other \in candidate : other <= amount]

PermittedCertificate(caps, resourceStart, feeStart, certificate) ==
    /\ DOMAIN certificate = Groups
    /\ Sum(CertificateHolds(certificate)) <= ExposureLimit
    /\ \A p \in Payers : CertificateHolds(certificate)[p] <= caps[p]
    /\ \A g \in Groups :
         /\ certificate[g].resource \in [Payers -> Nat]
         /\ certificate[g].fee \in [Payers -> Nat]
         /\ Sum(certificate[g].resource) = ResourceAmount(g)
         /\ Sum(certificate[g].fee) = FeeAmount(g)
         /\ certificate[g].resourceAfter \in Payers
         /\ certificate[g].feeAfter \in Payers
         /\ (ResourceAmount(g) = 0 => certificate[g].resourceAfter = resourceStart)
         /\ (FeeAmount(g) = 0 => certificate[g].feeAfter = feeStart)

ScopeState ==
    [balance : [Payers -> 0..CapacityBound],
     resourcePosition : Payers, feePosition : Payers, revision : Nat]

VARIABLES ledger, pending, completed, receipts, initialLedger, lastEffect
vars == <<ledger, pending, completed, receipts, initialLedger, lastEffect>>
Empty == [active |-> FALSE]
NoEffect == [present |-> FALSE]

Init ==
    /\ ledger = [s \in Scopes |->
         [balance |-> [p \in Payers |-> CapacityBound],
          resourcePosition |-> 0, feePosition |-> 0, revision |-> 0]]
    /\ initialLedger = ledger
    /\ pending = [w \in Workers |-> Empty]
    /\ completed = [s \in Scopes |-> {}]
    /\ receipts = [s \in Scopes |-> <<>>]
    /\ lastEffect = NoEffect

Ready(w, scope, tx) ==
    /\ ~pending[w].active
    /\ (~CheckReceipts \/ tx \notin completed[scope])
    /\ Sum(ledger[scope].balance) >= 2
    /\ PermittedCertificate(ledger[scope].balance,
         ledger[scope].resourcePosition, ledger[scope].feePosition,
         AbstractFamilyCertificate(ledger[scope].balance,
           ledger[scope].resourcePosition, ledger[scope].feePosition))

PrepareEnabled(w, scope, tx) ==
    /\ Ready(w, scope, tx)
    /\ (~SerializeScopes \/ \A other \in Workers : ~pending[other].active)

Prepare(w, scope, tx) ==
    /\ PrepareEnabled(w, scope, tx)
    /\ pending' = [pending EXCEPT ![w] =
         [active |-> TRUE, chosen |-> FALSE, scope |-> scope, transaction |-> tx,
          before |-> ledger[scope],
          certificate |-> AbstractFamilyCertificate(ledger[scope].balance,
            ledger[scope].resourcePosition, ledger[scope].feePosition)]]
    /\ UNCHANGED <<ledger, completed, receipts, initialLedger, lastEffect>>

ChooseRealizedOutcome(w, outcome) ==
    /\ pending[w].active
    /\ ~pending[w].chosen
    /\ pending' = [pending EXCEPT ![w] =
         [active |-> TRUE, chosen |-> TRUE,
          scope |-> pending[w].scope, transaction |-> pending[w].transaction,
          before |-> pending[w].before, certificate |-> pending[w].certificate,
          outcome |-> outcome]]
    /\ UNCHANGED <<ledger, completed, receipts, initialLedger, lastEffect>>

Commit(w) ==
    /\ pending[w].active
    /\ pending[w].chosen
    /\ LET captured == pending[w]
           scope == captured.scope
           realized == CapturedGroup(captured.outcome)
           published == IF PublishRealized THEN realized ELSE (realized + 1) % 3
           plan == captured.certificate[published]
           current == ledger[scope]
       IN /\ (~CheckReceipts \/ captured.transaction \notin completed[scope])
          /\ (~CheckFreshness \/ captured.before = current)
          /\ \A p \in Payers : plan.resource[p] + plan.fee[p] <= current.balance[p]
          /\ ledger' = [ledger EXCEPT ![scope] =
               [balance |-> [p \in Payers |-> current.balance[p] - plan.resource[p] - plan.fee[p]],
                resourcePosition |-> IF CommitResourceCursor THEN plan.resourceAfter ELSE current.resourcePosition,
                feePosition |-> IF CommitFeeCursor THEN plan.feeAfter ELSE current.feePosition,
                revision |-> current.revision + 1]]
          /\ completed' = [completed EXCEPT ![scope] = @ \cup {captured.transaction}]
          /\ receipts' = [receipts EXCEPT ![scope] = Append(@,
               [transaction |-> captured.transaction, outcome |-> captured.outcome,
                group |-> realized, captured |-> captured.before, before |-> current,
                certificate |-> captured.certificate, published |-> published,
                after |-> ledger'[scope]])]
          /\ lastEffect' = [present |-> TRUE, scope |-> scope, before |-> ledger, after |-> ledger']
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ UNCHANGED initialLedger

Abort(w) ==
    /\ pending[w].active
    /\ pending' = [pending EXCEPT ![w] = Empty]
    /\ ledger' = IF AbortChangesCursor
                 THEN [ledger EXCEPT ![pending[w].scope].resourcePosition = (@ + 1) % PayerCount]
                 ELSE ledger
    /\ UNCHANGED <<completed, receipts, initialLedger, lastEffect>>

Next ==
    \/ \E w \in Workers, scope \in Scopes, tx \in Transactions : Prepare(w, scope, tx)
    \/ \E w \in Workers, outcome \in Outcomes : ChooseRealizedOutcome(w, outcome)
    \/ \E w \in Workers : Commit(w) \/ Abort(w)
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ ledger \in [Scopes -> ScopeState]
    /\ initialLedger \in [Scopes -> ScopeState]
    /\ completed \in [Scopes -> SUBSET Transactions]
    /\ DOMAIN pending = Workers
    /\ DOMAIN receipts = Scopes
    /\ \A w \in Workers :
         pending[w] = Empty \/
         /\ pending[w].active = TRUE
         /\ pending[w].scope \in Scopes
         /\ pending[w].transaction \in Transactions
         /\ pending[w].before \in ScopeState
         /\ pending[w].chosen \in BOOLEAN
         /\ (pending[w].chosen => pending[w].outcome \in Outcomes)

ReceiptAccounting ==
    \A scope \in Scopes :
      \A p \in Payers :
        ledger[scope].balance[p] +
          Sum([index \in 1..Len(receipts[scope]) |->
            LET receipt == receipts[scope][index]
                plan == receipt.certificate[receipt.group]
            IN plan.resource[p] + plan.fee[p]]) = initialLedger[scope].balance[p]

OnceOnlyReceipts ==
    \A scope \in Scopes :
      /\ Cardinality(completed[scope]) = Len(receipts[scope])
      /\ completed[scope] = {receipts[scope][index].transaction : index \in 1..Len(receipts[scope])}
      /\ ledger[scope].revision = Len(receipts[scope])

EveryReceiptUsesCapturedCertificate ==
    \A scope \in Scopes :
      \A index \in 1..Len(receipts[scope]) :
        LET receipt == receipts[scope][index]
        IN /\ receipt.captured = receipt.before
           /\ receipt.certificate = AbstractFamilyCertificate(receipt.before.balance,
                receipt.before.resourcePosition, receipt.before.feePosition)
           /\ PermittedCertificate(receipt.before.balance, receipt.before.resourcePosition,
                receipt.before.feePosition, receipt.certificate)

OnlyRealizedGroupPublishes ==
    \A scope \in Scopes :
      \A index \in 1..Len(receipts[scope]) :
        LET receipt == receipts[scope][index]
            plan == receipt.certificate[GroupOf(receipt.outcome)]
        IN /\ receipt.group = GroupOf(receipt.outcome)
           /\ receipt.published = receipt.group
           /\ receipt.after.balance =
                [p \in Payers |-> receipt.before.balance[p] - plan.resource[p] - plan.fee[p]]
           /\ receipt.after.resourcePosition = plan.resourceAfter
           /\ receipt.after.feePosition = plan.feeAfter
           /\ receipt.after.revision = receipt.before.revision + 1

CursorsChangeOnlyWithSettlement ==
    \A scope \in Scopes :
      ledger[scope] =
        IF receipts[scope] = <<>> THEN initialLedger[scope]
        ELSE receipts[scope][Len(receipts[scope])].after

DuplicateAliasesShareTransitions ==
    \A w \in Workers :
      pending[w].active =>
        \A left, right \in Outcomes :
          GroupOf(left) = GroupOf(right) =>
            pending[w].certificate[CapturedGroup(left)] = pending[w].certificate[CapturedGroup(right)]

DisjointScopesRemainUnchanged ==
    lastEffect.present =>
      \A scope \in Scopes \ {lastEffect.scope} : lastEffect.before[scope] = lastEffect.after[scope]

DisjointPreparationRemainsEnabled ==
    \A w \in Workers, scope \in Scopes, tx \in Transactions :
      (Ready(w, scope, tx) /\
       \E other \in Workers :
         pending[other].active /\ pending[other].scope # scope) =>
        PrepareEnabled(w, scope, tx)

=============================================================================
