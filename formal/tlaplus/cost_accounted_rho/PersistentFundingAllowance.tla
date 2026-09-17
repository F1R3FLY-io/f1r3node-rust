--------------------- MODULE PersistentFundingAllowance ---------------------
EXTENDS Naturals, Integers, FiniteSets, TLC

CONSTANTS Workers, Grants, Ids, Initial, Fault

VARIABLES available, consumed, issued, generation, reservations, pending,
          usedWorkers, receiptCount, receiptDebit, expanded, topUps,
          transferCount, completedAfterTransfer, ownerTerms, receiptTerms,
          custody, walletAvailable, walletDebited, walletCredits

vars == <<available, consumed, issued, generation, reservations, pending,
          usedWorkers, receiptCount, receiptDebit, expanded, topUps,
          transferCount, completedAfterTransfer, ownerTerms, receiptTerms,
          custody, walletAvailable, walletDebited, walletCredits>>

Ceilings(term) == IF term = 0 THEN <<3, 5, 8>> ELSE <<1, 2, 3, 4>>
PriceAllowed(price, term) == \A i \in DOMAIN Ceilings(term) : price <= Ceilings(term)[i]
PriceGuard(price, term) == IF Fault = "highest-ceiling"
                         THEN \E i \in DOMAIN Ceilings(term) : price <= Ceilings(term)[i]
                         ELSE PriceAllowed(price, term)

RECURSIVE Sum(_)
Sum(values) == IF values = {} THEN 0
              ELSE LET entry == CHOOSE entry \in values : TRUE
                   IN entry[2] + Sum(values \ {entry})

Reserved(g) == Sum({<<id, reservations[id].amount>> :
                    id \in {i \in Ids : reservations[i].grant = g /\
                                        reservations[i].active}})
Debited(g) == Sum({<<id, receiptDebit[id]>> :
                   id \in {i \in Ids : reservations[i].grant = g}})
WalletReserved(purse) == Sum({<<id, reservations[id].amount>> :
  id \in {i \in Ids : custody[reservations[i].grant] = purse /\ reservations[i].active}})
Empty == [grant |-> CHOOSE g \in Grants : TRUE,
          amount |-> 0, active |-> FALSE, used |-> FALSE, version |-> 0,
          terms |-> 0, price |-> 0]
Idle == [kind |-> "idle", grant |-> CHOOSE g \in Grants : TRUE,
         id |-> CHOOSE id \in Ids : TRUE, amount |-> 0,
         version |-> 0, capacity |-> 0, terms |-> 0, price |-> 0,
         walletCapacity |-> 0]

Init ==
  /\ available = [g \in Grants |-> Initial]
  /\ consumed = [g \in Grants |-> 0]
  /\ issued = [g \in Grants |-> Initial]
  /\ generation = [g \in Grants |-> 0]
  /\ reservations = [id \in Ids |-> Empty]
  /\ pending = [w \in Workers |-> Idle]
  /\ usedWorkers = {}
  /\ receiptCount = [id \in Ids |-> 0]
  /\ receiptDebit = [id \in Ids |-> 0]
  /\ expanded = [g \in Grants |-> 0]
  /\ topUps = [g \in Grants |-> 0]
  /\ transferCount = [g \in Grants |-> 0]
  /\ completedAfterTransfer = FALSE
  /\ ownerTerms = [g \in Grants |-> 0]
  /\ receiptTerms = [id \in Ids |-> 0]
  /\ custody \in [Grants -> Grants]
  /\ walletAvailable = [purse \in Grants |-> Initial]
  /\ walletDebited = [purse \in Grants |-> 0]
  /\ walletCredits = [purse \in Grants |-> 0]

Prepare(w, kind, g, id, amount, price) ==
  /\ w \notin usedWorkers
  /\ pending[w].kind = "idle"
  /\ kind = "reserve" \/ price = 1
  /\ CASE kind = "reserve" ->
            ~reservations[id].used /\ amount > 0 /\ amount <= available[g]
       [] kind \in {"settle", "abort"} ->
            reservations[id].active /\ reservations[id].grant = g /\
            amount <= reservations[id].amount /\ (kind = "abort" => amount = 0)
       [] OTHER -> id = (CHOOSE i \in Ids : TRUE) /\ amount = 1
  /\ pending' = [pending EXCEPT ![w] =
       [kind |-> kind, grant |-> g, id |-> id, amount |-> amount,
        version |-> generation[g], capacity |-> available[g],
        terms |-> ownerTerms[g], price |-> price,
        walletCapacity |-> walletAvailable[custody[g]]]]
  /\ usedWorkers' = usedWorkers \cup {w}
  /\ UNCHANGED <<available, consumed, issued, generation, reservations,
       receiptCount, receiptDebit, expanded, topUps, transferCount,
       completedAfterTransfer, ownerTerms, receiptTerms,
       custody, walletAvailable, walletDebited, walletCredits>>

Reserve(w) ==
  LET p == pending[w] IN
  /\ p.kind = "reserve"
  /\ generation[p.grant] = p.version
  /\ ~reservations[p.id].used
  /\ PriceGuard(p.price, p.terms)
  /\ p.amount <= IF Fault = "stale-capacity" THEN p.capacity
                  ELSE available[p.grant]
  /\ Fault = "ignore-custody" \/
       p.amount <= IF Fault = "stale-wallet" THEN p.walletCapacity
                   ELSE walletAvailable[custody[p.grant]]
  /\ available' = IF Fault = "publish-consent-only" THEN available
       ELSE [available EXCEPT ![p.grant] = @ - p.amount]
  /\ reservations' = IF Fault = "publish-quantity-only" THEN reservations
       ELSE [reservations EXCEPT ![p.id] =
       [grant |-> p.grant, amount |-> p.amount, active |-> TRUE,
        used |-> TRUE, version |-> p.version, terms |-> p.terms, price |-> p.price]]
  /\ pending' = [pending EXCEPT ![w] = Idle]
  /\ walletAvailable' = [walletAvailable EXCEPT ![custody[p.grant]] = @ - p.amount]
  /\ UNCHANGED <<usedWorkers, consumed, issued, generation, receiptCount, receiptDebit,
       expanded, topUps, transferCount, completedAfterTransfer, ownerTerms, receiptTerms,
       custody, walletDebited, walletCredits>>

Transfer(w) ==
  LET p == pending[w] IN
  /\ p.kind = "transfer"
  /\ generation[p.grant] = p.version
  /\ generation' = [generation EXCEPT ![p.grant] = @ + 1]
  /\ transferCount' = [transferCount EXCEPT ![p.grant] = @ + 1]
  /\ ownerTerms' = [ownerTerms EXCEPT ![p.grant] = 1 - @]
  /\ available' = IF Fault = "reset-consumed"
       THEN [available EXCEPT ![p.grant] = @ + consumed[p.grant]] ELSE available
  /\ consumed' = IF Fault = "reset-consumed"
       THEN [consumed EXCEPT ![p.grant] = 0] ELSE consumed
  /\ pending' = [pending EXCEPT ![w] = Idle]
  /\ UNCHANGED <<usedWorkers, issued, reservations, receiptCount, receiptDebit,
       expanded, topUps, completedAfterTransfer, receiptTerms,
       custody, walletAvailable, walletDebited, walletCredits>>

Finish(w) ==
  LET p == pending[w]
      r == reservations[p.id]
      debit == IF p.kind = "abort" THEN 0 ELSE p.amount
  IN
  /\ p.kind \in {"settle", "abort"}
  /\ r.used /\ (r.active \/ Fault = "repeat-settlement")
  /\ debit <= r.amount
  /\ available' = [available EXCEPT ![r.grant] =
       @ + IF Fault = "lose-refund" THEN 0 ELSE r.amount - debit]
  /\ consumed' = [consumed EXCEPT ![r.grant] = @ + debit]
  /\ reservations' = [reservations EXCEPT ![p.id].active = FALSE]
  /\ receiptCount' = [receiptCount EXCEPT ![p.id] = @ + 1]
  /\ receiptDebit' = [receiptDebit EXCEPT ![p.id] = @ + debit]
  /\ receiptTerms' = [receiptTerms EXCEPT ![p.id] =
       IF Fault = "current-owner" THEN ownerTerms[r.grant] ELSE r.terms]
  /\ walletAvailable' = [walletAvailable EXCEPT ![custody[r.grant]] = @ + r.amount - debit]
  /\ walletDebited' = [walletDebited EXCEPT ![custody[r.grant]] = @ + debit]
  /\ completedAfterTransfer' =
       (completedAfterTransfer \/ generation[r.grant] > r.version)
  /\ pending' = [pending EXCEPT ![w] = Idle]
  /\ UNCHANGED <<usedWorkers, issued, generation, expanded, topUps, transferCount, ownerTerms,
       custody, walletCredits>>

Expand(w) ==
  LET p == pending[w] IN
  /\ p.kind = "expand"
  /\ generation[p.grant] = p.version
  /\ available' = [available EXCEPT ![p.grant] = @ + p.amount]
  /\ issued' = [issued EXCEPT ![p.grant] = @ + p.amount]
  /\ expanded' = [expanded EXCEPT ![p.grant] = @ + p.amount]
  /\ generation' = [generation EXCEPT ![p.grant] = @ + 1]
  /\ pending' = [pending EXCEPT ![w] = Idle]
  /\ UNCHANGED <<usedWorkers, consumed, reservations, receiptCount, receiptDebit,
       topUps, transferCount, completedAfterTransfer, ownerTerms, receiptTerms,
       custody, walletAvailable, walletDebited, walletCredits>>

TopUp(g) ==
  /\ topUps[g] = 0
  /\ topUps' = [topUps EXCEPT ![g] = 1]
  /\ available' = IF Fault = "topup-expands"
       THEN [available EXCEPT ![g] = @ + 1] ELSE available
  /\ issued' = IF Fault = "topup-expands"
       THEN [issued EXCEPT ![g] = @ + 1] ELSE issued
  /\ walletAvailable' = [walletAvailable EXCEPT ![custody[g]] = @ + 1]
  /\ walletCredits' = [walletCredits EXCEPT ![custody[g]] = @ + 1]
  /\ UNCHANGED <<consumed, generation, reservations, pending, usedWorkers,
       receiptCount, receiptDebit, expanded, transferCount, completedAfterTransfer,
       ownerTerms, receiptTerms, custody, walletDebited>>

Discard(w) ==
  /\ pending[w].kind # "idle"
  /\ available' = IF Fault = "restore-snapshot"
       THEN [available EXCEPT ![pending[w].grant] = pending[w].capacity]
       ELSE available
  /\ pending' = [pending EXCEPT ![w] = Idle]
  /\ UNCHANGED <<consumed, issued, generation, reservations, usedWorkers,
       receiptCount, receiptDebit, expanded, topUps, transferCount,
       completedAfterTransfer, ownerTerms, receiptTerms,
       custody, walletAvailable, walletDebited, walletCredits>>

Next ==
  \/ \E w \in Workers, kind \in {"reserve", "settle", "abort", "transfer", "expand"},
        g \in Grants, id \in Ids, amount \in 0..Initial, price \in {1, 2, 4} :
        Prepare(w, kind, g, id, amount, price)
  \/ \E w \in Workers : Reserve(w) \/ Transfer(w) \/ Finish(w) \/ Expand(w) \/ Discard(w)
  \/ \E g \in Grants : TopUp(g)

Conservation == \A g \in Grants : available[g] + Reserved(g) + consumed[g] = issued[g]
NoOverdraw == \A g \in Grants : available[g] >= 0
ConsumptionMatchesReceipts == \A g \in Grants : consumed[g] = Debited(g)
NoImplicitExpansion == \A g \in Grants : issued[g] = Initial + expanded[g]
AtMostOneClose == \A id \in Ids : receiptCount[id] <= 1
NoSettlementAfterTransfer == ~completedAfterTransfer
ReservedPriceAuthorized == \A id \in Ids :
  reservations[id].used => PriceAllowed(reservations[id].price, reservations[id].terms)
SettlementUsesCapturedConsent == \A id \in Ids :
  receiptCount[id] > 0 => receiptTerms[id] = reservations[id].terms
WalletBackingConserved == \A purse \in Grants :
  walletAvailable[purse] + WalletReserved(purse) + walletDebited[purse] =
    Initial + walletCredits[purse]
NoWalletOverdraw == \A purse \in Grants : walletAvailable[purse] >= 0

Spec == Init /\ [][Next]_vars
WorkerSymmetry == Permutations(Workers)
=============================================================================
