------------------------- MODULE DeployStorageBound -------------------------
(* One deploy's execution under phlo accounting in                           *)
(* rholang/src/rust/interpreter/accounting/costs.rs. Every storage effect     *)
(* (a produce or a consume that leaves data in the tuple space) is charged   *)
(* at least StorageCostPerByte phlo per encoded byte before it is retained,  *)
(* and execution halts when the phlo limit is exhausted. The theorem bounds  *)
(* the bytes a deploy can retain by its phlo limit. It says nothing about    *)
(* how many deploys a block admits; that rate belongs to the consumer.       *)
EXTENDS Naturals, TLC

CONSTANTS PhloLimit,          \* the deploy's phlo limit
          StorageCostPerByte, \* phlo charged per retained byte (1 in costs.rs)
          EffectSizes,        \* encoded byte sizes a single storage effect may have
          ChargeStorage       \* the correction: storage effects are metered

ASSUME /\ PhloLimit \in Nat
       /\ StorageCostPerByte \in Nat \ {0}
       /\ EffectSizes \subseteq Nat \ {0}
       /\ ChargeStorage \in BOOLEAN

VARIABLES phase, phloRemaining, retained

vars == <<phase, phloRemaining, retained>>

Init ==
    /\ phase = "running"
    /\ phloRemaining = PhloLimit
    /\ retained = 0

\* A storage effect of b bytes. Metered execution charges b times the rate
\* first and halts with OutOfPhlo when the charge exceeds what remains; the
\* effect is retained only when the charge succeeded. Unmetered execution
\* retains the bytes and charges nothing.
StorageEffect ==
    /\ phase = "running"
    /\ \E b \in EffectSizes :
         LET charge == IF ChargeStorage THEN b * StorageCostPerByte ELSE 0
         IN IF charge <= phloRemaining
               THEN /\ phloRemaining' = phloRemaining - charge
                    /\ retained' = retained + b
                    /\ UNCHANGED phase
               ELSE /\ phase' = "out-of-phlo"
                    /\ UNCHANGED <<phloRemaining, retained>>

\* A non-storage step (evaluation, comm events) costs phlo and retains nothing.
ComputeStep ==
    /\ phase = "running"
    /\ phloRemaining > 0
    /\ phloRemaining' = phloRemaining - 1
    /\ UNCHANGED <<phase, retained>>

Finish ==
    /\ phase = "running"
    /\ phase' = "finished"
    /\ UNCHANGED <<phloRemaining, retained>>

Next == StorageEffect \/ ComputeStep \/ Finish

Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"running", "out-of-phlo", "finished"}
    /\ phloRemaining \in 0..PhloLimit
    /\ retained \in Nat

\* The theorem: retained bytes never exceed the phlo the deploy could spend
\* on them. The bound a consumer model may cite is PhloLimit \div
\* StorageCostPerByte.
RetainedWithinPhlo == retained * StorageCostPerByte <= PhloLimit
PhloNeverNegative == phloRemaining >= 0
Terminates == <>(phase # "running")
=============================================================================
