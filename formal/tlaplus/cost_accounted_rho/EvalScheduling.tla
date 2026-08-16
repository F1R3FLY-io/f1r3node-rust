--------------------------- MODULE EvalScheduling -----------------------------
(****************************************************************************)
(* Models the eval loop scheduling problem that motivates the migration     *)
(* from two-phase dispatch to pure FuturesUnordered.                        *)
(*                                                                          *)
(* The key property: under the internalized cost model (fuel tokens),       *)
(* the total cost is identical regardless of which order COMM bodies        *)
(* are dispatched. This is what makes FuturesUnordered safe for consensus.  *)
(*                                                                          *)
(* We model N bodies, each requiring one fuel token to execute. The model   *)
(* explores all N! orderings and verifies that the terminal cost is the     *)
(* same in every case.                                                      *)
(*                                                                          *)
(* Contrast with the BROKEN externalized model (also modeled here),         *)
(* where produce-first vs consume-first yields different intermediate       *)
(* charges, making total cost order-dependent.                              *)
(****************************************************************************)

EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
    Bodies,             \* Set of body identifiers (e.g., {"b1", "b2", "b3"})
    CostPerToken        \* Nat: cost of consuming one fuel token (e.g., 1)

VARIABLES
    executed,           \* Set of Bodies that have completed execution
    totalCost,          \* Nat: running total cost under internalized model
    extCost,            \* Nat: running total cost under externalized model
    orderSoFar          \* Sequence of Bodies: execution order trace

(*--------------------------------------------------------------------------*)
(* In the internalized model, each body costs exactly CostPerToken          *)
(* (one fuel token consumed), regardless of which other bodies have         *)
(* already run.                                                             *)
(*                                                                          *)
(* In the externalized model, the cost of a body depends on whether         *)
(* it runs as a "producer" or "consumer" in its COMM interaction.           *)
(* We model this asymmetry: the first body to touch a shared channel        *)
(* pays StorageCostA; the second pays StorageCostB ≠ StorageCostA.          *)
(* This makes extCost order-dependent.                                      *)
(*--------------------------------------------------------------------------*)
CONSTANTS
    StorageCostA,       \* Cost when body stores first (e.g., produce-first)
    StorageCostB        \* Cost when body stores second (e.g., consume-first)

ASSUME StorageCostA # StorageCostB  \* The asymmetry that causes the bug
ASSUME StorageCostA \in Nat /\ StorageCostB \in Nat
ASSUME CostPerToken \in Nat

(*--------------------------------------------------------------------------*)
(* Shared channel state: tracks how many bodies have already interacted     *)
(* with the shared channel. Used only by the externalized cost model.       *)
(*--------------------------------------------------------------------------*)
VARIABLE channelTouches  \* Nat: number of bodies that have touched the channel

(*--------------------------------------------------------------------------*)
(* The paper's validator supply notation maps to the existing canonical       *)
(* SystemVault selected by the validator key. We reuse Bodies as validator    *)
(* identities. [supply] is that native vault balance, written by authenticated *)
(* PoS minting and conserving fee transfers, never by user reduction.         *)
(*--------------------------------------------------------------------------*)
VARIABLE supply           \* [Bodies -> Nat]: canonical SystemVault balance
VARIABLE halted           \* SUBSET Bodies: validators whose minting is halted
VARIABLE mintedThisEpoch  \* SUBSET Bodies: validators already minted this epoch

CONSTANT MintAmount       \* Nat: epochPhlogiston credited per eligible mint

ASSUME MintAmount \in Nat /\ MintAmount > 0

(*--------------------------------------------------------------------------*)
(* FeeExtract is realized as an atomic transfer between two canonical          *)
(* SystemVault balances. [feeTransfers] records each modeled payer/proposer     *)
(* pair once, bounding the state space while retaining arbitrary interleavings. *)
(* No fee pool, conversion epoch, or parallel economic ledger exists.           *)
(*--------------------------------------------------------------------------*)
VARIABLE feeTransfers      \* SUBSET (Bodies \X Bodies): completed transfers

CONSTANT FeeAmount         \* Nat: per-COLLECTION fee credit (the flat FeeExtract)

ASSUME FeeAmount \in Nat

FeePairs == {<<payer, proposer>> \in Bodies \X Bodies : payer # proposer}

RECURSIVE SumBalances(_, _)
SumBalances(balances, identities) ==
    IF identities = {}
    THEN 0
    ELSE LET identity == CHOOSE member \in identities : TRUE
         IN balances[identity]
            + SumBalances(balances, identities \ {identity})

(*--------------------------------------------------------------------------*)
(* Cost-Accounted Rho WD-D2: the per-signature ACCEPTANCE GATE + settlement  *)
(* debit at block assembly (cost-accounted-rho §7.6/§7.7;                     *)
(* casper/.../util/rholang/acceptance.rs). We reuse Bodies as the per-block   *)
(* candidate deployments sharing ONE signature supply pool Σ⟦s⟧. CanonOrder   *)
(* is the consensus-canonical deploy order (block_creator.rs:315-324) the gate *)
(* re-imposes on the nondeterministic HashSet; Demand[b] is the deployment's  *)
(* static Δ_s. The gate admits the LARGEST canonical PREFIX whose cumulative  *)
(* Δ_s fits the pool, rejecting the first non-fitting deploy and ALL after it *)
(* (§7.7 reject-both). Only admitted deploys execute (gate-before-execute),   *)
(* and the pool is debited ΣΔ_admitted exactly once at settlement             *)
(* (post = pre − ΣΔ).                                                          *)
(*--------------------------------------------------------------------------*)
CONSTANT CanonOrder       \* Seq(Bodies): canonical deploy order (a permutation)
CONSTANT Demand           \* [Bodies -> Nat]: per-deploy static Δ_s
CONSTANT PoolSupply       \* Nat: the shared signature pool's pre-state Σ⟦s⟧

ASSUME PoolSupply \in Nat
ASSUME Demand \in [Bodies -> Nat]

(*--------------------------------------------------------------------------*)
(* CA-P-171 "concurrent non-locking admission" (cost-accounted-rho §2.3,     *)
(* tex:309-378): a SECOND, signature-DISJOINT acceptance-gate group. The spec *)
(* §2.3(ii) "Concurrent acceptance" promises "Multiple deployments can be     *)
(* active simultaneously, each drawing from its own committed token supply.   *)
(* RSpace is never locked by a single deployment", and §2.3(iv) makes the     *)
(* disjoint-signature case explicit: "deployments signed by DIFFERENT         *)
(* signatures draw from DISJOINT token pools and cannot conflict. They may be  *)
(* executed in parallel". The Remark "From blocking to budgeting" gives the    *)
(* structural form: the funding question "can be answered independently for    *)
(* each deployment" — i.e. there is NO global execution lock / turn variable.  *)
(*                                                                           *)
(* The single-pool gate above (CanonOrder / Demand / PoolSupply, group "A") is *)
(* the §7.6/§7.7 acceptance-gate already modeled. Here we add a SECOND group   *)
(* "B" — a SEPARATE signature pool Σ⟦sB⟧ with its OWN canonical deploy order,  *)
(* per-deploy demand, and supply (CanonOrderB / DemandB / PoolSupplyB). Group  *)
(* B's pool is DISJOINT from group A's (different signature ⇒ ChannelSeparation *)
(* / lane_pool_disjoint), so the two groups CANNOT conflict. Group B's gate is  *)
(* a faithful parallel copy of group A's machinery; CRUCIALLY each group's gate *)
(* actions are enabled by ITS OWN phase/supply/demand ALONE (AcceptanceGateB    *)
(* never reads gatePhase/poolBalance, and AcceptanceGate never reads            *)
(* gatePhaseB/poolBalanceB) — there is no shared lock, turn, or mutual-exclusion *)
(* variable. The liveness property DisjointPoolsAdmitConcurrentlyNoGlobalLock   *)
(* (below) is the machine-checkable witness that, under INDEPENDENT per-group   *)
(* weak fairness, BOTH groups reach their settled/admitted state — neither is   *)
(* blocked waiting on the other.                                                *)
(*--------------------------------------------------------------------------*)
CONSTANT CanonOrderB      \* Seq(Bodies): group-B canonical deploy order
CONSTANT DemandB          \* [Bodies -> Nat]: group-B per-deploy static Δ_sB
CONSTANT PoolSupplyB      \* Nat: group-B disjoint signature pool's pre-state Σ⟦sB⟧

ASSUME PoolSupplyB \in Nat
ASSUME DemandB \in [Bodies -> Nat]

VARIABLE gatePhase        \* {"pregate","executing","settled"}: block-assembly phase
VARIABLE admittedLen      \* Nat: length of the admitted canonical prefix
VARIABLE poolBalance      \* Nat: current Σ⟦s⟧ balance (pre, then post-settle)
VARIABLE gateExecuted     \* SUBSET Bodies: admitted deploys that have executed

(* CA-P-171: group-B gate state — a faithful, signature-DISJOINT copy of the   *)
(* group-A gate variables above. Each is touched ONLY by group-B's own gate     *)
(* actions, so group B's admission progress is structurally independent of      *)
(* group A's (no shared lock/turn variable couples them).                       *)
VARIABLE gatePhaseB       \* {"pregate","executing","settled"}: group-B phase
VARIABLE admittedLenB     \* Nat: length of group-B's admitted canonical prefix
VARIABLE poolBalanceB     \* Nat: current Σ⟦sB⟧ balance (pre, then post-settle)
VARIABLE gateExecutedB    \* SUBSET Bodies: group-B admitted deploys that executed

(* Cumulative demand of the first k deploys in canonical order. *)
RECURSIVE CumDemand(_)
CumDemand(k) ==
    IF k = 0 THEN 0
    ELSE Demand[CanonOrder[k]] + CumDemand(k - 1)

(* The admitted prefix LENGTH: the largest k (0..Len(CanonOrder)) whose
   cumulative demand fits PoolSupply. Computed by choosing the maximal fitting
   prefix — the spec analogue of the Rust residual walk. *)
FittingLens == { k \in 0..Len(CanonOrder) : CumDemand(k) <= PoolSupply }
AdmittedPrefixLen == CHOOSE k \in FittingLens :
                        \A j \in FittingLens : j <= k

(* The admitted deploy set: the first AdmittedPrefixLen deploys in canon order. *)
AdmittedSet(len) == { CanonOrder[i] : i \in 1..len }

(* CA-P-171: the group-B analogues. CumDemandB/AdmittedPrefixLenB/AdmittedSetB   *)
(* read ONLY group-B's own canon order, demand, and supply — never group A's.    *)
RECURSIVE CumDemandB(_)
CumDemandB(k) ==
    IF k = 0 THEN 0
    ELSE DemandB[CanonOrderB[k]] + CumDemandB(k - 1)

FittingLensB == { k \in 0..Len(CanonOrderB) : CumDemandB(k) <= PoolSupplyB }
AdmittedPrefixLenB == CHOOSE k \in FittingLensB :
                        \A j \in FittingLensB : j <= k

AdmittedSetB(len) == { CanonOrderB[i] : i \in 1..len }

vars == <<executed, totalCost, extCost, orderSoFar, channelTouches,
          supply, halted, mintedThisEpoch,
          gatePhase, admittedLen, poolBalance, gateExecuted,
          feeTransfers,
          gatePhaseB, admittedLenB, poolBalanceB, gateExecutedB>>

(* CA-P-171: the group-B gate variables alone, used as the UNCHANGED footprint  *)
(* for every group-A / orthogonal action (each of which stutters group B).      *)
varsB == <<gatePhaseB, admittedLenB, poolBalanceB, gateExecutedB>>

TypeOK ==
    /\ executed   \in SUBSET Bodies
    /\ totalCost  \in Nat
    /\ extCost    \in Nat
    /\ orderSoFar \in Seq(Bodies)
    /\ channelTouches \in Nat
    /\ supply \in [Bodies -> Nat]
    /\ halted \in SUBSET Bodies
    /\ mintedThisEpoch \in SUBSET Bodies
    /\ gatePhase \in {"pregate", "executing", "settled"}
    /\ admittedLen \in 0..Len(CanonOrder)
    /\ poolBalance \in Nat
    /\ gateExecuted \in SUBSET Bodies
    /\ feeTransfers \in SUBSET FeePairs
    \* CA-P-171 group-B gate.
    /\ gatePhaseB \in {"pregate", "executing", "settled"}
    /\ admittedLenB \in 0..Len(CanonOrderB)
    /\ poolBalanceB \in Nat
    /\ gateExecutedB \in SUBSET Bodies

Init ==
    /\ executed       = {}
    /\ totalCost      = 0
    /\ extCost        = 0
    /\ orderSoFar     = << >>
    /\ channelTouches = 0
    /\ supply          = [b \in Bodies |-> 0]
    /\ mintedThisEpoch = {}
    \* A nondeterministic initial halt set lets TLC explore halted AND
    \* unhalted validators (the slash having already halted some validators).
    /\ halted \in SUBSET Bodies
    \* WD-D2: the block starts BEFORE the gate; the pool carries its pre-state
    \* Σ⟦s⟧ = PoolSupply; nothing admitted or executed yet.
    /\ gatePhase    = "pregate"
    /\ admittedLen  = 0
    /\ poolBalance  = PoolSupply
    /\ gateExecuted = {}
    /\ feeTransfers = {}
    \* CA-P-171: group B starts BEFORE its own gate, carrying its disjoint
    \* pre-state Σ⟦sB⟧ = PoolSupplyB; nothing admitted or executed in B yet.
    /\ gatePhaseB    = "pregate"
    /\ admittedLenB  = 0
    /\ poolBalanceB  = PoolSupplyB
    /\ gateExecutedB = {}

(*--------------------------------------------------------------------------*)
(* Action: Execute body b.                                                  *)
(*                                                                          *)
(* Internalized cost: always CostPerToken (one token consumed).             *)
(* Externalized cost: StorageCostA if first to touch channel,               *)
(*                    StorageCostB if second, etc.                           *)
(*--------------------------------------------------------------------------*)
ExecuteBody(b) ==
    /\ b \notin executed
    /\ executed'       = executed \cup {b}
    /\ totalCost'      = totalCost + CostPerToken
    /\ extCost'        = extCost + (IF channelTouches = 0
                                     THEN StorageCostA
                                     ELSE StorageCostB)
    /\ channelTouches' = channelTouches + 1
    /\ orderSoFar'     = Append(orderSoFar, b)
    \* A user reduction step never touches SystemVault custody or gate state.
    /\ UNCHANGED <<supply, halted, mintedThisEpoch,
                   gatePhase, admittedLen, poolBalance, gateExecuted,
                   feeTransfers>>
    \* CA-P-171: orthogonal to group B's disjoint gate.
    /\ UNCHANGED varsB

(*--------------------------------------------------------------------------*)
(* An eligible validator receives an authenticated protocol mint directly in *)
(* its canonical SystemVault and is recorded in the epoch mint ledger.        *)
(*--------------------------------------------------------------------------*)
MintValidator(b) ==
    /\ b \notin halted
    /\ b \notin mintedThisEpoch
    /\ supply'          = [supply EXCEPT ![b] = supply[b] + MintAmount]
    /\ mintedThisEpoch' = mintedThisEpoch \cup {b}
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches, halted,
                   gatePhase, admittedLen, poolBalance, gateExecuted,
                   feeTransfers>>
    \* CA-P-171: orthogonal to group B's disjoint gate.
    /\ UNCHANGED varsB

(*--------------------------------------------------------------------------*)
(* FeeExtract atomically moves existing REV from the payer's canonical         *)
(* SystemVault to the eligible proposer's canonical SystemVault.               *)
(*--------------------------------------------------------------------------*)
TransferFee(payer, proposer) ==
    /\ <<payer, proposer>> \in FeePairs \ feeTransfers
    /\ proposer \notin halted
    /\ supply[payer] >= FeeAmount
    /\ supply' =
         [identity \in Bodies |->
           IF identity = payer
           THEN supply[identity] - FeeAmount
           ELSE IF identity = proposer
                THEN supply[identity] + FeeAmount
                ELSE supply[identity]]
    /\ feeTransfers' = feeTransfers \cup {<<payer, proposer>>}
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   halted, mintedThisEpoch, gatePhase, admittedLen,
                   poolBalance, gateExecuted>>
    /\ UNCHANGED varsB

(*--------------------------------------------------------------------------*)
(* WD-D2 Action: the ACCEPTANCE GATE. From the "pregate" phase, compute the   *)
(* admitted canonical prefix (the largest prefix whose cumulative Δ_s fits     *)
(* PoolSupply) and transition to "executing". This is the O(AST) gate that     *)
(* runs at block assembly BEFORE any admitted deploy executes (tex 1726-1729). *)
(* The pool balance is untouched here — the DEBIT happens at settlement, after *)
(* execution.                                                                  *)
(*--------------------------------------------------------------------------*)
AcceptanceGate ==
    /\ gatePhase = "pregate"
    /\ admittedLen' = AdmittedPrefixLen
    /\ gatePhase'   = "executing"
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   supply, halted, mintedThisEpoch, poolBalance, gateExecuted,
                   feeTransfers>>
    \* CA-P-171: group A's gate NEVER reads or writes group B's pool/phase —
    \* the two groups share no lock/turn variable (no global execution lock).
    /\ UNCHANGED varsB

(*--------------------------------------------------------------------------*)
(* WD-D2 Action: execute an ADMITTED deploy. Only possible AFTER the gate     *)
(* ("executing" phase) and only for deploys in the admitted prefix — the       *)
(* gate-before-execute discipline (a rejected deploy NEVER executes). Order     *)
(* among admitted deploys is nondeterministic (the gate's funding decision is   *)
(* schedule-independent).                                                       *)
(*--------------------------------------------------------------------------*)
ExecuteAdmitted(b) ==
    /\ gatePhase = "executing"
    /\ b \in AdmittedSet(admittedLen)
    /\ b \notin gateExecuted
    /\ gateExecuted' = gateExecuted \cup {b}
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   supply, halted, mintedThisEpoch, gatePhase, admittedLen,
                   poolBalance, feeTransfers>>
    \* CA-P-171: orthogonal to group B's disjoint gate.
    /\ UNCHANGED varsB

(*--------------------------------------------------------------------------*)
(* WD-D2 Action: SETTLE the block. Once every admitted deploy has executed,    *)
(* debit the pool by ΣΔ_admitted exactly once (post Σ⟦s⟧ = pre − ΣΔ). The       *)
(* authority is prepaid RSpace custody, distinct from the SystemVault ledger.   *)
(* fits PoolSupply by construction, so the debit never underflows.             *)
(*--------------------------------------------------------------------------*)
SettleBlock ==
    /\ gatePhase = "executing"
    /\ gateExecuted = AdmittedSet(admittedLen)
    /\ poolBalance' = PoolSupply - CumDemand(admittedLen)
    /\ gatePhase'   = "settled"
    /\ UNCHANGED <<executed, totalCost, extCost, orderSoFar, channelTouches,
                   supply, halted, mintedThisEpoch, admittedLen, gateExecuted,
                   feeTransfers>>
    \* CA-P-171: settling group A leaves group B's disjoint pool untouched.
    /\ UNCHANGED varsB

(*--------------------------------------------------------------------------*)
(* CA-P-171: the group-B gate actions — a faithful copy of AcceptanceGate /    *)
(* ExecuteAdmitted / SettleBlock, but every enabling condition and update      *)
(* reads/writes ONLY group-B state (gatePhaseB / admittedLenB / poolBalanceB / *)
(* gateExecutedB) and the group-B CONSTANTS (CanonOrderB / DemandB /           *)
(* PoolSupplyB). No group-A variable (gatePhase / poolBalance / ...) and no     *)
(* shared lock or turn variable appears in any guard — so group B's admission   *)
(* progress is structurally independent of group A's (§2.3 Remark: the funding  *)
(* question "can be answered independently for each deployment"). All non-B     *)
(* state is stuttered.                                                          *)
(*--------------------------------------------------------------------------*)
AOthers == <<executed, totalCost, extCost, orderSoFar, channelTouches,
             supply, halted, mintedThisEpoch,
             gatePhase, admittedLen, poolBalance, gateExecuted,
             feeTransfers>>

AcceptanceGateB ==
    /\ gatePhaseB = "pregate"
    /\ admittedLenB' = AdmittedPrefixLenB
    /\ gatePhaseB'   = "executing"
    /\ UNCHANGED <<poolBalanceB, gateExecutedB>>
    /\ UNCHANGED AOthers

ExecuteAdmittedB(b) ==
    /\ gatePhaseB = "executing"
    /\ b \in AdmittedSetB(admittedLenB)
    /\ b \notin gateExecutedB
    /\ gateExecutedB' = gateExecutedB \cup {b}
    /\ UNCHANGED <<gatePhaseB, admittedLenB, poolBalanceB>>
    /\ UNCHANGED AOthers

SettleBlockB ==
    /\ gatePhaseB = "executing"
    /\ gateExecutedB = AdmittedSetB(admittedLenB)
    /\ poolBalanceB' = PoolSupplyB - CumDemandB(admittedLenB)
    /\ gatePhaseB'   = "settled"
    /\ UNCHANGED <<admittedLenB, gateExecutedB>>
    /\ UNCHANGED AOthers

Next ==
    \/ \E b \in Bodies : ExecuteBody(b)
    \/ \E b \in Bodies : MintValidator(b)
    \/ \E payer, proposer \in Bodies : TransferFee(payer, proposer)
    \/ AcceptanceGate
    \/ \E b \in Bodies : ExecuteAdmitted(b)
    \/ SettleBlock
    \* CA-P-171: group-B gate actions, interleaved with everything above.
    \/ AcceptanceGateB
    \/ \E b \in Bodies : ExecuteAdmittedB(b)
    \/ SettleBlockB

Spec == Init /\ [][Next]_vars

(*==========================================================================*)
(* INVARIANTS                                                               *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Internalized cost is always deterministic: at termination, totalCost     *)
(* equals |Bodies| * CostPerToken regardless of execution order.            *)
(*--------------------------------------------------------------------------*)
AllDone == executed = Bodies

InternalizedCostDeterministic ==
    AllDone => totalCost = Cardinality(Bodies) * CostPerToken

(*--------------------------------------------------------------------------*)
(* Externalized cost is NOT deterministic when |Bodies| >= 2 and the        *)
(* bodies share a channel. We don't assert this as an invariant (it would   *)
(* fail); instead we track extCost for observational comparison.            *)
(* The ABSENCE of a similar invariant for extCost is the bug.               *)
(*--------------------------------------------------------------------------*)

(*--------------------------------------------------------------------------*)
(* Token conservation (internalized): cost increases by exactly             *)
(* CostPerToken per body, never more.                                       *)
(*--------------------------------------------------------------------------*)
InternalizedCostBounded ==
    totalCost <= Cardinality(Bodies) * CostPerToken

(*==========================================================================*)
(* Native SystemVault economic invariants                                   *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* A validator already halted at this model's initial state cannot mint or be *)
(* selected as a fee recipient, so its canonical vault remains empty.         *)
(*--------------------------------------------------------------------------*)
HaltedValidatorVaultEmpty ==
    \A b \in Bodies : b \in halted => supply[b] = 0

(*--------------------------------------------------------------------------*)
(* Protocol mint is the only operation that changes total native custody.     *)
(* Direct fees only redistribute that custody between canonical vaults.       *)
(*--------------------------------------------------------------------------*)
CanonicalVaultTotalBacked ==
    SumBalances(supply, Bodies) = Cardinality(mintedThisEpoch) * MintAmount

(*--------------------------------------------------------------------------*)
(* Every recorded fee recipient was eligible when the transfer committed.     *)
(*--------------------------------------------------------------------------*)
NoFeeToHaltedValidator ==
    \A transfer \in feeTransfers : transfer[2] \notin halted

(*--------------------------------------------------------------------------*)
(* Any canonical vault mutation is attributable to a new mint-ledger entry or *)
(* a newly committed direct fee transfer.                                      *)
(*--------------------------------------------------------------------------*)
VaultMutationIsProtocolAccounted ==
    [][ (\E b \in Bodies : supply'[b] # supply[b]) =>
          \/ mintedThisEpoch' # mintedThisEpoch
          \/ feeTransfers' # feeTransfers
      ]_vars

(*--------------------------------------------------------------------------*)
(* If no mint-ledger entry is added, the aggregate canonical balance is exact. *)
(*--------------------------------------------------------------------------*)
VaultTotalChangesOnlyByMint ==
    [][ mintedThisEpoch' = mintedThisEpoch =>
          SumBalances(supply', Bodies) = SumBalances(supply, Bodies)
      ]_vars

(*--------------------------------------------------------------------------*)
(* A halted validator's canonical vault never changes across any step.       *)
(*--------------------------------------------------------------------------*)
HaltedVaultFrozenStep ==
    [][ \A b \in Bodies : b \in halted => supply'[b] = supply[b] ]_vars

(*==========================================================================*)
(* Cost-Accounted Rho WD-D2 ACCEPTANCE-GATE INVARIANTS                       *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* NoDoubleSpendAtBlock: the signature pool Σ⟦s⟧ never goes negative. The     *)
(* gate admits only a prefix whose cumulative Δ_s fits the pool, so the        *)
(* settlement debit (post = pre − ΣΔ) lands on a non-negative balance — no     *)
(* deploy spends supply that a previously-admitted deploy already committed    *)
(* (cost-accounted-rho §7.7 duplicate-deploy / TM-CA-153 double-spend).        *)
(* [poolBalance] is [Nat]-typed, so this is the substantive statement that the *)
(* admitted demand never exceeds the pre-state supply at settlement.           *)
(*--------------------------------------------------------------------------*)
NoDoubleSpendAtBlock ==
    gatePhase = "settled" => poolBalance = PoolSupply - CumDemand(admittedLen)
                             /\ CumDemand(admittedLen) <= PoolSupply

(*--------------------------------------------------------------------------*)
(* RejectBothOnOversubscription: the admitted set is exactly a canonical      *)
(* PREFIX — if a deploy at canonical index i is rejected (i > admittedLen),    *)
(* then EVERY deploy at a later index is also rejected (no admitted deploy      *)
(* follows a rejected one). This is the §7.7 reject-both / no-partial          *)
(* discipline. Stated as: the admitted set equals the first [admittedLen]      *)
(* deploys in canon order, AND that prefix is maximal-fitting (admitting one    *)
(* more would oversubscribe).                                                   *)
(*--------------------------------------------------------------------------*)
RejectBothOnOversubscription ==
    (gatePhase \in {"executing", "settled"}) =>
        /\ AdmittedSet(admittedLen) = { CanonOrder[i] : i \in 1..admittedLen }
        /\ (admittedLen < Len(CanonOrder) =>
              CumDemand(admittedLen + 1) > PoolSupply)

(*--------------------------------------------------------------------------*)
(* GateBeforeExecute: no deploy executes before the gate has run, and only    *)
(* admitted deploys ever execute. So [gateExecuted] is empty in "pregate" and  *)
(* is always a subset of the admitted prefix (tex 1726-1729 accept-then-       *)
(* execute; rejected deploys never execute).                                   *)
(*--------------------------------------------------------------------------*)
GateBeforeExecute ==
    /\ (gatePhase = "pregate" => gateExecuted = {})
    /\ gateExecuted \subseteq AdmittedSet(admittedLen)

(*--------------------------------------------------------------------------*)
(* SupplyConservation: at settlement, the post-state pool plus the debited     *)
(* admitted demand EQUALS the pre-state pool — the settlement neither creates  *)
(* nor destroys supply (post + ΣΔ = pre). The TLA+ analogue of the Rocq        *)
(* [settlement_conserves] / [accept_commit_conserves].                         *)
(*--------------------------------------------------------------------------*)
SupplyConservation ==
    gatePhase = "settled" => poolBalance + CumDemand(admittedLen) = PoolSupply

(*--------------------------------------------------------------------------*)
(* Inv_StrictRejectsAbsent keeps its historical name and states the mandatory  *)
(* §7.6 step-5 rejection of underfunded demand. An ABSENT supply pool has       *)
(* balance 0 (the paper's [supply(s) = 0] for an absent pool). This invariant   *)
(* models that pool as [PoolSupply = 0] and asserts the consequence: once the   *)
(* gate has run (phase "executing"/"settled"), NO admitted deploy has positive  *)
(* demand — i.e. a [Δ > 0] deploy is never admitted against a zero (absent)     *)
(* pool. Client and validator provisioning make PoolSupply positive; they do    *)
(* not activate a separate enforcement mode.                                   *)
(*                                                                              *)
(* This is the TLA+ analogue of the universal Rust proposal/replay rule and of   *)
(* Rocq corollary [strict_reject_when_underfunded] ([is_funded_balance 0 f =    *)
(* false] when [delta_s f > 0]). It holds in EVERY phase: in "pregate"           *)
(* [admittedLen = 0] so [AdmittedSet] is empty (vacuously true), and after the  *)
(* gate the admitted prefix's cumulative demand is [<= PoolSupply = 0], which   *)
(* (with non-negative per-deploy [Demand]) forces every admitted deploy's       *)
(* demand to 0.                                                                  *)
(*--------------------------------------------------------------------------*)
Inv_StrictRejectsAbsent ==
    PoolSupply = 0 =>
        \A b \in AdmittedSet(admittedLen) : Demand[b] = 0

(*--------------------------------------------------------------------------*)
(* Canonical vault writes arise only from authenticated protocol mint or direct *)
(* fee transfer. Located-pool writes arise only from atomic settlement.          *)
(*--------------------------------------------------------------------------*)
VaultAndLocatedPoolWritersAreSeparated ==
    [][ /\ (\A b \in Bodies : supply'[b] # supply[b] =>
              ~ UNCHANGED mintedThisEpoch \/ ~ UNCHANGED feeTransfers)
        /\ (poolBalance' # poolBalance => gatePhase' = "settled" /\ gatePhase = "executing")
      ]_vars

(*==========================================================================*)
(* CA-P-171 ACCEPTANCE-GATE INVARIANTS FOR THE GROUP-B DISJOINT POOL          *)
(*==========================================================================*)
(* The group-A gate invariants (NoDoubleSpendAtBlock, RejectBothOnOver-       *)
(* subscription, GateBeforeExecute, SupplyConservation) are mirrored here for *)
(* the disjoint group-B pool, so the second pool is held to the SAME safety   *)
(* discipline (its admission is a real linear-proof gate, not a free pass).   *)

(* Group B's signature pool Σ⟦sB⟧ never goes negative; the settlement debit    *)
(* lands on a non-negative balance (the admitted prefix fits PoolSupplyB).     *)
NoDoubleSpendAtBlockB ==
    gatePhaseB = "settled" => poolBalanceB = PoolSupplyB - CumDemandB(admittedLenB)
                              /\ CumDemandB(admittedLenB) <= PoolSupplyB

(* Group B admits exactly a canonical PREFIX (reject-both / no-partial).       *)
RejectBothOnOversubscriptionB ==
    (gatePhaseB \in {"executing", "settled"}) =>
        /\ AdmittedSetB(admittedLenB) = { CanonOrderB[i] : i \in 1..admittedLenB }
        /\ (admittedLenB < Len(CanonOrderB) =>
              CumDemandB(admittedLenB + 1) > PoolSupplyB)

(* Group B never executes a deploy before its gate ran, and only admitted ones.*)
GateBeforeExecuteB ==
    /\ (gatePhaseB = "pregate" => gateExecutedB = {})
    /\ gateExecutedB \subseteq AdmittedSetB(admittedLenB)

(* Group B's settlement conserves its pool (post + ΣΔ = pre).                  *)
SupplyConservationB ==
    gatePhaseB = "settled" => poolBalanceB + CumDemandB(admittedLenB) = PoolSupplyB

(*--------------------------------------------------------------------------*)
(* CA-P-171 PoolGatesDisjoint (state invariant): the two acceptance gates     *)
(* operate on DISJOINT signature pools and never co-mingle their accounting.  *)
(* Group A's settled balance is determined SOLELY by group-A demand against    *)
(* PoolSupply, and group B's SOLELY by group-B demand against PoolSupplyB —    *)
(* neither pool's post-state depends on the other group's demand or phase.     *)
(* This is the state-space image of ChannelSeparation.v / lane_pool_disjoint:  *)
(* different signatures ⇒ disjoint pools ⇒ no cross-pool coupling. (It is the   *)
(* SAFETY companion of the no-global-lock LIVENESS property below: not only is  *)
(* progress decoupled, the ACCOUNTING is decoupled.)                           *)
(*--------------------------------------------------------------------------*)
PoolGatesDisjoint ==
    /\ (gatePhase  = "settled" => poolBalance  = PoolSupply  - CumDemand(admittedLen))
    /\ (gatePhaseB = "settled" => poolBalanceB = PoolSupplyB - CumDemandB(admittedLenB))

(*--------------------------------------------------------------------------*)
(* Progress: every body eventually executes (with fairness).                *)
(*--------------------------------------------------------------------------*)
Fairness == \A b \in Bodies : WF_vars(ExecuteBody(b))

(*--------------------------------------------------------------------------*)
(* CA-P-171 PER-POOL fairness. Each group's gate progress is driven by its    *)
(* OWN weak-fairness conditions, with NO shared scheduler/lock between them.   *)
(* GateFairnessA forces group A's gate (AcceptanceGate → ExecuteAdmitted* →    *)
(* SettleBlock) to run when continuously enabled; GateFairnessB does the same  *)
(* for group B — INDEPENDENTLY. The two fairness bundles share no action, so   *)
(* TLC must witness B's completion WITHOUT assuming anything forces A, and     *)
(* vice versa. This is the formal content of "no global execution lock":       *)
(* progress of one pool is not contingent on progress of the other.           *)
(*--------------------------------------------------------------------------*)
GateFairnessA ==
    /\ WF_vars(AcceptanceGate)
    /\ \A b \in Bodies : WF_vars(ExecuteAdmitted(b))
    /\ WF_vars(SettleBlock)

GateFairnessB ==
    /\ WF_vars(AcceptanceGateB)
    /\ \A b \in Bodies : WF_vars(ExecuteAdmittedB(b))
    /\ WF_vars(SettleBlockB)

(* LiveSpec carries the original body-execution fairness PLUS the two          *)
(* INDEPENDENT per-pool gate-fairness bundles (no shared fairness term).       *)
LiveSpec == Spec /\ Fairness /\ GateFairnessA /\ GateFairnessB

AllEventuallyDone == <>(executed = Bodies)

(*--------------------------------------------------------------------------*)
(* CA-P-171 DisjointPoolsAdmitConcurrentlyNoGlobalLock (the headline LIVENESS  *)
(* property): two deployments drawing on DISJOINT signature pools (group A on  *)
(* Σ⟦s⟧, group B on Σ⟦sB⟧) are BOTH admitted/executed/settled — neither is     *)
(* blocked by the other, and no global lock serializes them.                   *)
(*                                                                           *)
(* Faithful to cost-accounted-rho §2.3 (tex:309-378):                          *)
(*   (ii)  "Multiple deployments can be active simultaneously, each drawing    *)
(*         from its own committed token supply. RSpace is never locked by a    *)
(*         single deployment."                                                 *)
(*   (iv)  "deployments signed by DIFFERENT signatures draw from DISJOINT      *)
(*         token pools and cannot conflict. They may be executed in parallel". *)
(*   Remark ("From blocking to budgeting"): the funding question "can be       *)
(*         answered independently for each deployment".                        *)
(*                                                                           *)
(* HOW THIS WITNESSES "no global lock": LiveSpec supplies group A and group B  *)
(* with SEPARATE, INDEPENDENT weak-fairness bundles (GateFairnessA /           *)
(* GateFairnessB) that share NO action. Each group's gate actions are enabled  *)
(* by that group's OWN phase/supply/demand ALONE (AcceptanceGateB never reads  *)
(* gatePhase/poolBalance; AcceptanceGate never reads gatePhaseB/poolBalanceB). *)
(* So if reaching "both settled" were contingent on a serializing lock/turn,   *)
(* one group's fairness could not discharge it — yet TLC verifies the          *)
(* conjunction. Because the group-A instance is deliberately OVERSUBSCRIBED    *)
(* (it admits only a PREFIX and rejects the tail), group B's full admission is *)
(* reached EVEN THOUGH group A never admits its whole order: B's progress does *)
(* NOT wait on A's completion. The reject-both tail of A is also irrelevant to *)
(* B — disjoint pools cannot conflict.                                         *)
(*                                                                           *)
(* WHAT "admitted+executed" MEANS HERE (so the property BITES — an empty-        *)
(* admission settle must NOT satisfy it). For the FULLY-FUNDED disjoint group B, *)
(* "its deployment is admitted and executed" is:                                 *)
(*     gatePhaseB = "settled"                          (block assembled & debited)*)
(*  ∧  admittedLenB = Len(CanonOrderB)                 (its WHOLE order admitted — *)
(*                                                       NOT an empty prefix)      *)
(*  ∧  gateExecutedB = AdmittedSetB(admittedLenB)      (every admitted deploy ran).*)
(* If group B were UNFUNDED (Σ⟦sB⟧ < Δ_sB), the strict gate would admit NOTHING  *)
(* (admittedLenB = 0 ≠ Len(CanonOrderB)) and this conjunct would be FALSE even    *)
(* though gatePhaseB still reaches "settled" — so the property genuinely requires *)
(* concurrent ADMISSION+EXECUTION, not a vacuous phase advance. (Verified by the  *)
(* negative-control instance MCEvalNoLockNeg / EvalNoLockNeg.cfg, which sets      *)
(* PoolSupplyB = 0 and confirms TLC REFUTES this property.)                       *)
(*                                                                           *)
(* For the OVERSUBSCRIBED group A, "admitted+executed+settled" is just            *)
(* gatePhase = "settled" (∧ the always-true gateExecuted = AdmittedSet(admittedLen)*)
(* that SettleBlock's guard enforces): A's gate ran, its admitted PREFIX executed, *)
(* and it settled. We do NOT require A to admit its whole order (it cannot — it is  *)
(* oversubscribed); the point is precisely that A's PARTIAL admission and B's FULL  *)
(* admission both complete, concurrently, with neither blocking the other.         *)
(*--------------------------------------------------------------------------*)
GroupASettled ==
    /\ gatePhase = "settled"
    /\ gateExecuted = AdmittedSet(admittedLen)   \* admitted prefix fully executed

GroupBAdmittedExecuted ==
    /\ gatePhaseB = "settled"
    /\ admittedLenB = Len(CanonOrderB)            \* fully-funded ⇒ WHOLE order admitted
    /\ gateExecutedB = AdmittedSetB(admittedLenB) \* every admitted deploy executed

DisjointPoolsAdmitConcurrentlyNoGlobalLock ==
    <>(GroupASettled /\ GroupBAdmittedExecuted)

(*--------------------------------------------------------------------------*)
(* CA-P-171 EachPoolAdmittedIndependently: a finer-grained companion stating   *)
(* that EACH pool's admission+execution is INDEPENDENTLY inevitable. Each        *)
(* conjunct is discharged by that group's OWN fairness bundle alone, so neither  *)
(* group's progress waits on the other — the "answered independently for each    *)
(* deployment" content of the §2.3 Remark. Group A independently reaches its      *)
(* settled (admitted-prefix-executed) state; group B independently reaches its    *)
(* fully-admitted-and-executed state. The CONJUNCTION being checked under         *)
(* SEPARATE per-pool fairness (no shared term) is the machine-checkable witness   *)
(* that no global lock couples them.                                              *)
(*--------------------------------------------------------------------------*)
EachPoolAdmittedIndependently ==
    /\ <>GroupASettled
    /\ <>GroupBAdmittedExecuted

=============================================================================
