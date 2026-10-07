---------------- MODULE NumericMergeFundingCorrespondence ----------------
EXTENDS Integers, MonetaryAllocation

CONSTANT Fault
ASSUME PayerCount = 3

Validators == {1, 2}
Writers == {1, 2}
Consumer == 2
Unit == 3
RootIds == 0..2
Zero == [p \in Payers |-> 0]
Bindings == {
    [r \in Writers |-> r - 1],
    [r \in Writers |-> 0],
    [r \in Writers |-> IF r = 1 THEN Unit ELSE 1]}

VARIABLES accepted, binding, ready, pending, regions, phase, view, available,
          pre, captured, frontier, capacity, attempts, receipts, replayed,
          independent, merged, retained, charged, independentKept, depositAuthorized,
          mergedBalances, mergeRoot, independentRoot, independentCaptured
vars == <<accepted, binding, ready, pending, regions, phase, view, available,
          pre, captured, frontier, capacity, attempts, receipts, replayed,
          independent, merged, retained, charged, independentKept, depositAuthorized,
          mergedBalances, mergeRoot, independentRoot, independentCaptured>>

InitialBalances == [p \in Payers |-> 12]
WriterDebit(p) == Cardinality({w \in accepted : w - 1 = p})
BaseBalances == [p \in Payers |-> InitialBalances[p] - WriterDebit(p)]
RootBalances(root) ==
    CASE root = 0 -> BaseBalances
      [] root = 1 -> [p \in Payers |-> IF p = 0 THEN 0 ELSE BaseBalances[p]]
      [] OTHER -> [p \in Payers |->
           IF p = 0 THEN 6 ELSE IF p = Consumer THEN BaseBalances[p] - 7 ELSE BaseBalances[p]]
RootBurn(root) == IF root = 0 THEN 0 ELSE BaseBalances[0]
RootFees(root) == Sum([p \in Payers |-> WriterDebit(p)]) + IF root = 2 THEN 1 ELSE 0
Units(authority) == [p \in Payers |->
    Cardinality({r \in authority : binding[r] = p}) + IF p = Consumer THEN 1 ELSE 0]
Cost(authority) == [p \in Payers |-> 2 * Units(authority)[p]]
FeeCaps(root, authority) == [p \in Payers |->
    IF p = Consumer /\ RootBalances(root)[p] > Cost(authority)[p]
    THEN RootBalances(root)[p] - Cost(authority)[p] ELSE 0]
Fee(root, authority) ==
    IF Sum(FeeCaps(root, authority)) >= 1 THEN Allocation(FeeCaps(root, authority), 1, 0) ELSE Zero
Draw(root, authority) == [p \in Payers |-> Cost(authority)[p] + Fee(root, authority)[p]]
Authorized(authority) == {Consumer} \cup ({binding[r] : r \in authority} \ {Unit})
Backing(root, principals) == Sum([p \in principals \cap Payers |-> RootBalances(root)[p]])
ExecutionCapacity(root, principals) ==
    IF Backing(root, principals) > 0 THEN Backing(root, principals) - 1 ELSE 0
IndependentDraw == [p \in Payers |-> IF p = 0 THEN independent ELSE 0]
ReceiptDraw(effects) == [p \in Payers |-> Sum([r \in effects |-> r.draw[p]])]
MergeBase(effects) == IF effects = {} THEN independentRoot ELSE (CHOOSE r \in effects : TRUE).root
Compatible(effects) == \A p \in Payers :
    ReceiptDraw(effects)[p] + IndependentDraw[p] <= RootBalances(MergeBase(effects))[p]
FreshReceipt(v) == [validator |-> v, attempt |-> attempts[v], root |-> pre[v],
    used |-> pre[v], authority |-> {}, draw |-> Zero, fee |-> Zero,
    effect |-> FALSE, success |-> FALSE, after |-> captured[v]]

Init ==
    /\ accepted \in (SUBSET Writers) \ {{}}
    /\ binding \in Bindings
    /\ ready = {}
    /\ pending = [v \in Validators |-> Writers]
    /\ regions = [v \in Validators |-> {}]
    /\ phase = [v \in Validators |-> "collect"]
    /\ view = [v \in Validators |-> 0]
    /\ available = {0}
    /\ pre = [v \in Validators |-> 0]
    /\ captured = [v \in Validators |-> RootBalances(0)]
    /\ frontier = [v \in Validators |-> {}]
    /\ capacity = [v \in Validators |-> 0]
    /\ attempts = [v \in Validators |-> 0]
    /\ receipts = {}
    /\ replayed = {}
    /\ independent = 0
    /\ merged = FALSE
    /\ retained = {}
    /\ charged = {}
    /\ independentKept = FALSE
    /\ depositAuthorized = TRUE
    /\ mergedBalances = Zero
    /\ mergeRoot = 0
    /\ independentRoot = 0
    /\ independentCaptured = RootBalances(0)

PrepareWriter(w) ==
    /\ w \notin ready
    /\ ready' = ready \cup {w}
    /\ UNCHANGED <<accepted, binding, pending, regions, phase, view, available, pre,
         captured, frontier, capacity, attempts, receipts, replayed, independent,
         merged, retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

ReadWriter(v, w) ==
    /\ phase[v] = "collect" /\ w \in ready \cap pending[v]
    /\ pending' = [pending EXCEPT ![v] = @ \ {w}]
    /\ regions' = [regions EXCEPT ![v] = @ \cup (IF w \in accepted THEN {w} ELSE {})]
    /\ UNCHANGED <<accepted, binding, ready, phase, view, available, pre, captured,
         frontier, capacity, attempts, receipts, replayed, independent,
         merged, retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

PublishMerge(v) ==
    /\ phase[v] = "collect" /\ pending[v] = {}
    /\ phase' = [phase EXCEPT ![v] = "idle"]
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, view, available, pre,
         captured, frontier, capacity, attempts, receipts, replayed, independent,
         merged, retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

PublishFundingRoot(root, authorized) ==
    /\ ready = Writers /\ root \in {1, 2} \ available
    /\ root - 1 \in available
    /\ root = 1 \/ authorized \/ Fault = "unauthorizedDeposit"
    /\ available' = available \cup {root}
    /\ depositAuthorized' = IF root = 2 THEN authorized ELSE depositAuthorized
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, phase, view, pre,
         captured, frontier, capacity, attempts, receipts, replayed, independent,
         merged, retained, charged, independentKept, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

Observe(v, root) ==
    /\ root \in available /\ root > view[v]
    /\ view' = [view EXCEPT ![v] = root]
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, phase, available, pre,
         captured, frontier, capacity, attempts, receipts, replayed, independent,
         merged, retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

Prepare(v) ==
    /\ phase[v] = "idle" /\ attempts[v] < 2 /\ ~merged
    /\ phase' = [phase EXCEPT ![v] = "prepared"]
    /\ pre' = [pre EXCEPT ![v] = view[v]]
    /\ captured' = [captured EXCEPT ![v] = RootBalances(view[v])]
    /\ frontier' = [frontier EXCEPT ![v] = {Consumer}]
    /\ capacity' = [capacity EXCEPT ![v] = ExecutionCapacity(view[v], {Consumer})]
    /\ attempts' = [attempts EXCEPT ![v] = @ + 1]
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, view, available,
         receipts, replayed, independent, merged, retained, charged, independentKept,
         depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

Discover(v) ==
    /\ phase[v] = "prepared"
    /\ LET observed == IF Fault = "missingFrontier" THEN {Consumer}
                       ELSE Authorized(regions[v])
           recorded == observed \cup (IF Fault = "inventedAuthority" THEN {99} ELSE {})
       IN /\ frontier' = [frontier EXCEPT ![v] = recorded]
          /\ capacity' = [capacity EXCEPT ![v] = ExecutionCapacity(pre[v], recorded)]
    /\ phase' = [phase EXCEPT ![v] = "discovered"]
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, view, available, pre,
         captured, attempts, receipts, replayed, independent, merged, retained, charged,
         independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

Settle(v) ==
    /\ phase[v] = "discovered"
    /\ LET used == IF Fault = "newerRoot" THEN view[v] ELSE pre[v]
           draw == Draw(used, regions[v])
           enough == IF Fault = "scalar"
                     THEN Sum(RootBalances(used)) >= Sum(draw)
                     ELSE (\A p \in Payers : draw[p] <= RootBalances(used)[p])
           ok == enough /\ Sum(Fee(used, regions[v])) = 1
                 /\ capacity[v] >= Sum(Cost(regions[v]))
                 /\ frontier[v] \subseteq Authorized(regions[v])
           baseReceipt == FreshReceipt(v)
           receipt == [baseReceipt EXCEPT
             !.used = used, !.authority = regions[v], !.success = ok,
             !.effect = ok \/ Fault = "partial",
             !.draw = IF ok THEN draw ELSE Zero,
             !.fee = IF ok THEN Fee(used, regions[v]) ELSE Zero,
             !.after = IF ok THEN [p \in Payers |-> RootBalances(used)[p] - draw[p]]
                       ELSE captured[v]]
       IN /\ receipts' = receipts \cup {receipt}
          /\ phase' = [phase EXCEPT ![v] = IF ok THEN "settled" ELSE "rejected"]
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, view, available, pre,
         captured, frontier, capacity, attempts, replayed, independent, merged,
         retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

Replay(v) ==
    /\ phase[v] = "settled"
    /\ LET receipt == CHOOSE r \in receipts : r.validator = v /\ r.success
           root == IF Fault = "replayRoot" THEN view[v] ELSE receipt.root
       IN replayed' = replayed \cup {[receipt |-> receipt, root |-> root,
            after |-> [p \in Payers |-> RootBalances(root)[p] - Draw(root, receipt.authority)[p]]]}
    /\ phase' = [phase EXCEPT ![v] = "replayed"]
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, view, available, pre,
         captured, frontier, capacity, attempts, receipts, independent, merged,
         retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

Retry(v) ==
    /\ phase[v] = "rejected" /\ attempts[v] < 2
    /\ phase' = [phase EXCEPT ![v] = "idle"]
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, view, available, pre,
         captured, frontier, capacity, attempts, receipts, replayed, independent,
         merged, retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot, independentRoot, independentCaptured>>

PrepareIndependent ==
    /\ independent = 0 /\ independent' \in {1, 10}
    /\ independentRoot' \in available
    /\ independent' <= RootBalances(independentRoot')[0]
    /\ independentCaptured' = RootBalances(independentRoot')
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, phase, view, available,
         pre, captured, frontier, capacity, attempts, receipts, replayed, merged,
         retained, charged, independentKept, depositAuthorized, mergedBalances, mergeRoot>>

MergeConsumers ==
    /\ ~merged /\ independent > 0
    /\ \A v \in Validators : phase[v] \in {"replayed", "rejected"}
    /\ LET successes == {r \in receipts : r.success}
           roots == {r.root : r \in successes}
           selected == {r \in successes : \A other \in successes : r.validator <= other.validator}
       IN /\ Cardinality(roots) <= 1
          /\ (roots \subseteq {independentRoot} \/ Fault = "independentRootSwap")
          /\ retained' = IF Fault = "doubleConsume" THEN successes ELSE selected
          /\ charged' = IF Fault \in {"rejectedMoney", "doubleConsume"} THEN successes ELSE selected
          /\ independentKept' = ((Compatible(selected) \/ Fault = "aggregateOverdraw") /\
                 ~(Fault = "globalGuard" /\ \E r \in successes : view[r.validator] # r.root))
          /\ mergeRoot' = MergeBase(selected)
          /\ mergedBalances' = [p \in Payers |-> RootBalances(mergeRoot')[p]
                 - ReceiptDraw(charged')[p]
                 - IF independentKept' /\ Fault # "independentMoney" THEN IndependentDraw[p] ELSE 0]
    /\ merged' = TRUE
    /\ UNCHANGED <<accepted, binding, ready, pending, regions, phase, view, available,
         pre, captured, frontier, capacity, attempts, receipts, replayed, independent,
         depositAuthorized, independentRoot, independentCaptured>>

Next == (\E w \in Writers : PrepareWriter(w))
     \/ (\E v \in Validators, w \in Writers : ReadWriter(v, w))
     \/ (\E v \in Validators : PublishMerge(v) \/ Prepare(v) \/ Discover(v) \/ Settle(v) \/ Replay(v) \/ Retry(v))
     \/ (\E root \in {1, 2}, authorized \in BOOLEAN : PublishFundingRoot(root, authorized))
     \/ (\E v \in Validators, root \in RootIds : Observe(v, root))
     \/ PrepareIndependent \/ MergeConsumers
Spec == Init /\ [][Next]_vars
RecoveryNext == Next /\ ((2 \notin available /\ 2 \in available') =>
    \E failed \in receipts : ~failed.success /\ failed.root = 1 /\ failed.attempt = 1)
RecoverySpec == Init /\ accepted = Writers /\ binding = [r \in Writers |-> r - 1]
    /\ [][RecoveryNext]_vars

RetainedWriterAuthority == \A v \in Validators : phase[v] # "collect" => regions[v] = accepted
ImmutablePrestate == \A v \in Validators : captured[v] = RootBalances(pre[v])
AuthenticatedFrontierOnly == \A v \in Validators : frontier[v] \subseteq Authorized(regions[v])
CompleteDiscoveredFrontier == \A v \in Validators :
    phase[v] \in {"discovered", "settled", "rejected", "replayed"} => frontier[v] = Authorized(regions[v])
FrontierCapacity == \A v \in Validators : phase[v] # "collect" =>
    capacity[v] = ExecutionCapacity(pre[v], frontier[v])
IndependentPrestateBinding ==
    /\ independentCaptured = RootBalances(independentRoot)
    /\ independent <= independentCaptured[0]
    /\ (merged => mergeRoot = independentRoot)
CertifiedRootBinding == \A r \in receipts : r.used = r.root
ExactInheritedDemand == \A r \in receipts : r.success =>
    /\ r.authority = accepted /\ r.draw = Draw(r.root, accepted)
PerPurseSufficiency == \A r \in receipts : r.success =>
    \A p \in Payers : 0 <= r.after[p] /\ r.draw[p] <= RootBalances(r.root)[p]
NoRejectedCandidateEffects == \A r \in receipts : ~r.success =>
    /\ ~r.effect /\ r.draw = Zero /\ r.fee = Zero /\ r.after = RootBalances(r.root)
CertifiedPrestateFee == \A r \in receipts : r.success => r.fee = Fee(r.root, r.authority)
ReplayCorrespondence == \A replay \in replayed :
    /\ replay.root = replay.receipt.root /\ replay.after = replay.receipt.after
WholeEffectRetention == merged => charged = retained
NoDoubleConsumption == Cardinality(retained) <= 1
IndependentEffectPreservation == merged => (independentKept = Compatible(retained))
MergedPerPurseConservation == merged => \A p \in Payers :
    mergedBalances[p] + ReceiptDraw(retained)[p]
      + (IF independentKept THEN IndependentDraw[p] ELSE 0) = RootBalances(mergeRoot)[p]
MergedPerPurseSufficiency == merged => \A p \in Payers : mergedBalances[p] >= 0
NoRecoveredReplay == ~\E replay \in replayed :
    /\ replay.receipt.attempt = 2 /\ replay.root = 2
    /\ \E failed \in receipts : ~failed.success /\ failed.root = 1
         /\ failed.validator = replay.receipt.validator /\ failed.attempt = 1
DepositConservation == /\ (2 \in available => depositAuthorized)
    /\ \A root \in available : Sum(RootBalances(root)) + RootBurn(root) + RootFees(root) = Sum(InitialBalances)
SettlementConservation == \A r \in receipts : r.success =>
    Sum(r.after) + Sum(Cost(r.authority)) + Sum(r.fee) = Sum(RootBalances(r.root))

=============================================================================
