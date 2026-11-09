----------------------- MODULE SameBlockFundingGroups -----------------------
(***************************************************************************)
(* DR-65 (gap G2): several offered-funded deploys in one block.            *)
(*                                                                         *)
(* A batch is a list of members in canonical order (DR-65 item 1). The     *)
(* canonical order is the order of the member numbers 1, 2 and 3. Each     *)
(* member has its own custodies, a hold on each custody and a measured     *)
(* use. A hold is a fit check at the block pre-state R0 and moves no       *)
(* funds (DR-123 item 8). A key stands for one canonical physical custody  *)
(* or for one grant (O-8). The supply of a key is its balance or the       *)
(* remaining ceiling of the grant at R0.                                   *)
(*                                                                         *)
(* The decision at R0 (DR-65 items 2 to 4):                                *)
(*   1. A member whose holds do not fit its own keys is rejected alone.    *)
(*   2. The other members that share a key form one funding group.         *)
(*   3. A group whose holds on some key exceed the supply is rejected,     *)
(*      all or none.                                                       *)
(* The decision folds the members in a given order. A sequential union     *)
(* merges the groups that each new member touches.                         *)
(*                                                                         *)
(* The execution of the accepted batch (DR-65 item 5, option B of design   *)
(* section 8.4):                                                           *)
(*   - Member i executes from R_{i-1}. Its later holds are the holds of    *)
(*     the batch members after it.                                         *)
(*   - Its selection capacity on an own key is the balance at R_{i-1}      *)
(*     minus the later holds, on the success path and on the rollback      *)
(*     path. The selection fills the keys in canonical custody order.      *)
(*   - The code of the member can drain any key, also a key outside its    *)
(*     custody. A transfer moves all of its amount or nothing.             *)
(*   - The held-capacity guard at the settlement root checks every         *)
(*     debited key and every key with a later hold:                        *)
(*       balance_after(k) - debit(k) >= later holds on k.                  *)
(*     A guard failure rolls the member back to R_{i-1} and charges it     *)
(*     within the cap.                                                     *)
(*   - A run that fails by itself (a classified user failure) also rolls   *)
(*     back and pays within the cap. The guard does not apply to it. The   *)
(*     constant UserFailures enables these runs.                           *)
(*                                                                         *)
(* The play budget can stop the batch after member k < n. The proposer     *)
(* then runs the shipped prefix again from R0, with no budget, when        *)
(*   (a) a shipped member and a dropped member share a key, or             *)
(*   (b) a shipped member settled as a held-capacity-guard rollback.       *)
(* Otherwise the executed results stand (g2-plan-v2 section 1).            *)
(* Validators replay the shipped prefix as a batch of its own.             *)
(*                                                                         *)
(* Rust correspondence:                                                    *)
(*   decision at R0      execution/funding_groups.rs decide_groups         *)
(*   later holds         execution/funding_groups.rs later_holds           *)
(*   capped selection    DirectWalletPolicySnapshot capped_by_later_holds  *)
(*   guard               execution/gate.rs held_capacity_guard             *)
(*   truncation rule     execution/funding_groups.rs rerun_after_truncation*)
(*   batch play          execution/offered_batch.rs                        *)
(*                       compute_offered_batch_state                       *)
(*                                                                         *)
(* Defect selects one negative control. Each control must violate the      *)
(* named invariant in its *Unsafe.cfg:                                     *)
(*   "PrefixAdmission"       accept members in order while the holds fit   *)
(*                           -> GroupAllOrNone                             *)
(*   "IndependentCapacity"   check each member alone and select with the   *)
(*                           full supply at R0 -> NoOverdraft              *)
(*   "NoHeldGuard"           the guard ignores the later holds             *)
(*                           -> HeldCapacityGuard                          *)
(*   "KeepTruncatedResults"  ship the executed prefix and never run it     *)
(*                           again -> TruncationConsistency                *)
(*                                                                         *)
(* G3 (DR-135, design v2 section 5): owner draws in a batch.               *)
(*   - The run of member i can fire stored processes of other parties.     *)
(*     Their owners pay (v2 R1 to R4). The owner table T_i of member i     *)
(*     lists the owner purses that its run consulted (ownerTable), and     *)
(*     ownerUse is the owner share that the run asks of each of them.      *)
(*   - The budget of member i on an owner purse k is                       *)
(*       budget_i(k) = max(0, max(0, bal(k, R_{i-1}) - later_i(k))         *)
(*                             - own_hold_i(k)).                           *)
(*     A draw that fits the budget is charged in full. A draw that does    *)
(*     not fit is refused with no charge (Option 2 of G3).                 *)
(*   - The owner debits are debits of member i, so the guard covers them,  *)
(*     and a guard rollback or a user failure charges them from the        *)
(*     restored balance.                                                   *)
(*   - Finding F6-a. v2 section 5 says that "a client debit never exceeds  *)
(*     its hold". The selection of option B can place more than the hold   *)
(*     of one purse on that purse (SameBlockFundingGroups.v, header), so   *)
(*     the client debit plus the owner debit can exceed the cap. The model *)
(*     therefore gives the settlement selection of member i the cap of     *)
(*     each purse minus the owner debits of member i on that purse. The    *)
(*     cap stays at least the own hold, so the selection still places the  *)
(*     whole use. Control "OwnerBlindSelection" selects with the full cap  *)
(*     and violates NoOverdraft.                                           *)
(*   - Truncation condition (c): a shipped member s and a dropped member d *)
(*     need the re-run when the custodies of d meet the owner table of s.  *)
(*     The budgets of s counted the holds of d, and validators recompute   *)
(*     the budgets over the shipped prefix only.                           *)
(*   G3 controls:                                                          *)
(*   "OwnerStraddleIgnored"  the truncation rule omits condition (c)       *)
(*                           -> TruncationConsistency                      *)
(*   "OwnerBlindSelection"   the settlement selection ignores the owner    *)
(*                           debits of the member (finding F6-a)           *)
(*                           -> NoOverdraft                                *)
(* The constant MaxOwnerUse bounds the owner shares. With MaxOwnerUse = 0  *)
(* every owner table is empty, and the model is the G2 model.              *)
(* G3 step S11 (DR-135 item 13): the signed limit and the hold of a caller *)
(* cover only the work that the caller pays. This model agrees and needs   *)
(* no change. The use of a member is its client charge, at most its holds, *)
(* and its owner shares (ownerUse) come only from the owner budgets, never *)
(* from its holds. Owner work at price 0 counts against the limit of the   *)
(* caller but costs 0, so it changes no balance in this model.             *)
(* Rust correspondence of the G3 parts (v2 S7 and S8; G3 code does not     *)
(* exist yet): the owner budgets and the table in the budget recording of  *)
(* direct_wallet_funding/execution.rs, the owner rows of                   *)
(* execution/settlement.rs, the guard of execution/gate.rs, and condition  *)
(* (c) in funding_groups.rs rerun_after_truncation.                        *)
(*                                                                         *)
(* The model has no recursion. Sums and folds unroll over the three        *)
(* member slots, so the model checks at most three members.                *)
(***************************************************************************)
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS
  \* @type: Str;
  Defect,
  \* @type: Int;
  NumMembers,
  \* @type: Int;
  MinHold,
  \* @type: Int;
  MaxHold,
  \* @type: Int;
  MaxSupply,
  \* @type: Int;
  MaxDrain,
  \* @type: Bool;
  UserFailures,
  \* Added by G3 (DR-135): the bound of the owner shares.
  \* @type: Int;
  MaxOwnerUse

\* Changed by G3 (DR-135): the two G3 controls.
\* ASSUME Defect \in {
\*   "None",
\*   "PrefixAdmission",
\*   "IndependentCapacity",
\*   "NoHeldGuard",
\*   "KeepTruncatedResults"
\* }
ASSUME Defect \in {
  "None",
  "PrefixAdmission",
  "IndependentCapacity",
  "NoHeldGuard",
  "KeepTruncatedResults",
  "OwnerStraddleIgnored",
  "OwnerBlindSelection"
}

\* Changed by G3 (DR-135): MaxOwnerUse.
\* ASSUME /\ NumMembers \in 1..3
\*        /\ MinHold \in 0..MaxHold
\*        /\ MaxHold \in 0..3
\*        /\ MaxSupply \in 0..6
\*        /\ MaxDrain \in 0..6
\*        /\ UserFailures \in BOOLEAN
ASSUME /\ NumMembers \in 1..3
       /\ MinHold \in 0..MaxHold
       /\ MaxHold \in 0..3
       /\ MaxSupply \in 0..6
       /\ MaxDrain \in 0..6
       /\ UserFailures \in BOOLEAN
       /\ MaxOwnerUse \in 0..2

\* The unrolling bound of every sum and fold.
Slots == 1..3

Members == 1..NumMembers

Keys == {"p", "q"}

CustodyChoices == {{"p"}, {"q"}, {"p", "q"}}

Phases == {"Gate", "Execute", "Stopped", "Shipped"}

Outcomes == {"None", "Settled", "GuardRollback", "Failed", "Unfunded"}

\* Changed by G3 (DR-135): a member also charges its owner debits.
\* \* A member charges at most 2 * MaxHold, and at most three members charge.
\* Balances == (-(6 * MaxHold))..MaxSupply
\* A member charges at most 2 * MaxHold for its client and MaxOwnerUse on
\* each purse for its owners, and at most three members charge.
Balances == (-(6 * MaxHold + 3 * MaxOwnerUse))..MaxSupply

\* Added by G3 (DR-135): the owner tables that the model can choose.
\* @type: Set(Set(Str));
OwnerTables == IF MaxOwnerUse = 0 THEN {{}} ELSE SUBSET Keys

\* @type: Seq(Int);
Canonical ==
  CASE NumMembers = 1 -> <<1>>
    [] NumMembers = 2 -> <<1, 2>>
    [] OTHER -> <<1, 2, 3>>

\* @type: Set(Seq(Int));
Orders ==
  CASE NumMembers = 1 -> {<<1>>}
    [] NumMembers = 2 -> {<<1, 2>>, <<2, 1>>}
    [] OTHER -> {<<1, 2, 3>>, <<1, 3, 2>>, <<2, 1, 3>>,
                 <<2, 3, 1>>, <<3, 1, 2>>, <<3, 2, 1>>}

Min(a, b) == IF a <= b THEN a ELSE b

Max0(a) == IF a < 0 THEN 0 ELSE a

\* The first member of a nonempty set of slots in canonical order.
\* @type: Set(Int) => Int;
NextIn(S) == IF 1 \in S THEN 1 ELSE IF 2 \in S THEN 2 ELSE 3

VARIABLES
  \* @type: Int -> Set(Str);
  custody,
  \* @type: Int -> (Str -> Int);
  hold,
  \* @type: Str -> Int;
  supply,
  \* @type: Int -> (Str -> Int);
  drain,
  \* @type: Int -> Int;
  use,
  \* @type: Int -> Bool;
  fails,
  \* @type: Str;
  phase,
  \* @type: Set(Int);
  gated,
  \* @type: Set(Int);
  accepted,
  \* @type: Set(Int);
  batch,
  \* @type: Set(Int);
  done,
  \* @type: Str -> Int;
  bal,
  \* @type: Int -> Str;
  outcome,
  \* @type: Int -> (Str -> Int);
  post,
  \* @type: Bool;
  budget,
  \* @type: Bool;
  reran,
  \* @type: Set(Int);
  shipped,
  \* Added by G3 (DR-135): the owner table of each member.
  \* @type: Int -> Set(Str);
  ownerTable,
  \* Added by G3 (DR-135): the owner share that each member asks of a purse.
  \* @type: Int -> (Str -> Int);
  ownerUse,
  \* Added by G3 (DR-135): the owner debits of each executed member.
  \* @type: Int -> (Str -> Int);
  ownerDebit,
  \* Added by G3 (DR-135): the owner budgets of each executed member.
  \* @type: Int -> (Str -> Int);
  ownerBudget

\* Changed by G3 (DR-135): the owner tables and the owner shares.
\* params == <<custody, hold, supply, drain, use, fails>>
params == <<custody, hold, supply, drain, use, fails, ownerTable, ownerUse>>

\* Changed by G3 (DR-135): the owner variables.
\* vars == <<custody, hold, supply, drain, use, fails, phase, gated, accepted,
\*           batch, done, bal, outcome, post, budget, reran, shipped>>
vars == <<custody, hold, supply, drain, use, fails, phase, gated, accepted,
          batch, done, bal, outcome, post, budget, reran, shipped,
          ownerTable, ownerUse, ownerDebit, ownerBudget>>

-----------------------------------------------------------------------------
(* The decision at R0.                                                     *)

\* The sum of the holds of the members of S on key k.
\* @type: (Set(Int), Str) => Int;
SumHold(S, k) ==
  (IF 1 \in S THEN hold[1][k] ELSE 0)
    + (IF 2 \in S THEN hold[2][k] ELSE 0)
    + (IF 3 \in S THEN hold[3][k] ELSE 0)

\* The holds of the members of S fit every key.
\* @type: Set(Int) => Bool;
Fits(S) == \A k \in Keys : SumHold(S, k) <= supply[k]

\* The check of one member on its own keys (DR-64).
\* @type: Int => Bool;
AloneOk(m) == \A k \in custody[m] : hold[m][k] <= supply[k]

\* Two members share a key.
\* @type: (Int, Int) => Bool;
Linked(a, b) == custody[a] \cap custody[b] # {}

\* Member m joins every earlier group that shares a key with it. The label
\* of a member names its group, and 0 means "not placed".
\* @type: (Int -> Int, Int) => (Int -> Int);
JoinStep(label, m) ==
  LET touching == {x \in Slots : label[x] # 0 /\ Linked(x, m)}
      merged == {label[x] : x \in touching}
  IN [x \in Slots |-> IF x = m \/ label[x] \in merged THEN m ELSE label[x]]

\* The k-th step of the sequential grouping of the members of S in order.
\* @type: (Set(Int), Seq(Int), Int, Int -> Int) => (Int -> Int);
GroupStep(S, order, k, label) ==
  IF k <= Len(order)
    THEN IF order[k] \in S /\ AloneOk(order[k])
           THEN JoinStep(label, order[k])
           ELSE label
    ELSE label

\* @type: (Set(Int), Seq(Int)) => (Int -> Int);
Labels(S, order) ==
  LET l0 == [x \in Slots |-> 0]
      l1 == GroupStep(S, order, 1, l0)
      l2 == GroupStep(S, order, 2, l1)
      l3 == GroupStep(S, order, 3, l2)
  IN l3

\* @type: (Int -> Int, Int, Int) => Bool;
SameGroupIn(label, a, b) == label[a] # 0 /\ label[a] = label[b]

\* @type: (Int -> Int, Int) => Set(Int);
GroupMembers(label, a) == {x \in Slots : SameGroupIn(label, a, x)}

\* The rule of DR-65: a placed member is accepted when its whole group fits.
\* @type: (Set(Int), Seq(Int)) => Set(Int);
GroupDecision(S, order) ==
  LET label == Labels(S, order)
  IN {m \in S : label[m] # 0 /\ Fits(GroupMembers(label, m))}

\* Control "PrefixAdmission": accept the members in order while their
\* cumulative holds fit.
\* @type: (Set(Int), Seq(Int), Int, Set(Int)) => Set(Int);
PrefixStep(S, order, k, acc) ==
  IF k <= Len(order)
    THEN IF order[k] \in S /\ AloneOk(order[k]) /\ Fits(acc \cup {order[k]})
           THEN acc \cup {order[k]}
           ELSE acc
    ELSE acc

\* @type: (Set(Int), Seq(Int)) => Set(Int);
PrefixDecision(S, order) ==
  LET a1 == PrefixStep(S, order, 1, {})
      a2 == PrefixStep(S, order, 2, a1)
      a3 == PrefixStep(S, order, 3, a2)
  IN a3

\* Control "IndependentCapacity": each member is checked alone, with no
\* group check.
\* @type: Set(Int) => Set(Int);
IndependentDecision(S) == {m \in S : AloneOk(m)}

\* The accepted members when the candidates are S, folded in the given order.
\* @type: (Set(Int), Seq(Int)) => Set(Int);
AcceptedIn(S, order) ==
  IF Defect = "PrefixAdmission"
    THEN PrefixDecision(S, order)
    ELSE IF Defect = "IndependentCapacity"
      THEN IndependentDecision(S)
      ELSE GroupDecision(S, order)

\* The decision over all members in one order: the accepted members and the
\* group relation.
\* @type: Seq(Int) => {accepted: Set(Int), groups: Set(<<Int, Int>>)};
DecisionOf(order) ==
  LET label == Labels(Members, order)
  IN [accepted |-> AcceptedIn(Members, order),
      groups |-> {pair \in Members \X Members : SameGroupIn(label, pair[1], pair[2])}]

\* An order-free statement of the groups, for the invariants. A path between
\* two of at most three members has at most two edges.
Remaining == {m \in Members : AloneOk(m)}

\* @type: (Set(Int), Int, Int) => Bool;
Connected(S, a, b) ==
  \/ a = b
  \/ Linked(a, b)
  \/ \E c \in S : Linked(a, c) /\ Linked(c, b)

\* @type: (Set(Int), Int) => Set(Int);
TrueGroup(S, a) == {x \in S : Connected(S, a, x)}

-----------------------------------------------------------------------------
(* The execution of one member.                                            *)

\* The sum of the holds of the members of B after member m on key k.
\* @type: (Set(Int), Int, Str) => Int;
LaterIn(B, m, k) == SumHold({j \in B : j > m}, k)

\* A transfer of x out of a balance b moves all of x or nothing.
\* @type: (Int, Int) => Int;
Drained(b, x) == IF x <= b THEN x ELSE 0

\* The selection capacity of member m on key k at the balances b (option B).
\* @type: (Str -> Int, Set(Int), Int, Str) => Int;
Capacity(b, B, m, k) ==
  IF k \notin custody[m]
    THEN 0
    ELSE IF Defect = "IndependentCapacity"
      THEN supply[k]
      ELSE Max0(b[k] - LaterIn(B, m, k))

\* The later holds that the guard of member m counts on key k.
\* @type: (Set(Int), Int, Str) => Int;
GuardLater(B, m, k) == IF Defect = "NoHeldGuard" THEN 0 ELSE LaterIn(B, m, k)

\* The guard at key k: the key is in scope when it is debited or carries a
\* later hold.
\* @type: (Set(Int), Int, Str, Int, Int) => Bool;
GuardAt(B, m, k, afterRun, debit) ==
  (debit > 0 \/ GuardLater(B, m, k) > 0) => debit + GuardLater(B, m, k) <= afterRun

\* Added by G3 (DR-135): the owner budget of member m on purse k at the
\* balances b (v2 R3 and section 5): the balance minus the later holds,
\* minus the own hold of m, never below zero. hold[m][k] is 0 outside the
\* custody of m.
\* @type: (Str -> Int, Set(Int), Int, Str) => Int;
OwnerBudgetAt(b, B, m, k) ==
  IF k \in ownerTable[m]
    THEN Max0(Max0(b[k] - LaterIn(B, m, k)) - hold[m][k])
    ELSE 0

\* Added by G3 (DR-135): an owner share that fits the budget is charged in
\* full. A share that does not fit is refused with no charge (Option 2).
\* @type: (Str -> Int, Set(Int), Int, Str) => Int;
OwnerDrawAt(b, B, m, k) ==
  IF ownerUse[m][k] <= OwnerBudgetAt(b, B, m, k) THEN ownerUse[m][k] ELSE 0

\* Added by G3 (DR-135, finding F6-a): the selection capacity of member m on
\* key k after its owner debits on k. Control "OwnerBlindSelection" keeps
\* the full capacity.
\* Note (G3, DR-135): the code has no owner-aware selection capacity, and
\* the F6-a case cannot occur in the code. The payer lane of every listed
\* source is a client lane (execution/gate.rs source_lanes, with no
\* exception for on-chain sources), and a region on a client lane is not an
\* owner region (owner_funding.rs is_owner_region). So the own source purse
\* of a member is never one of its owner purses, the owner debit of a
\* member on a key of its own custody is 0, and this capacity equals
\* Capacity there. The model keeps the general case: a custody key can
\* also be in the owner table of the member.
\* @type: (Str -> Int, Set(Int), Int, Str) => Int;
SelectionCapacity(b, B, m, k) ==
  IF Defect = "OwnerBlindSelection"
    THEN Capacity(b, B, m, k)
    ELSE Max0(Capacity(b, B, m, k) - OwnerDrawAt(b, B, m, k))

\* Changed by G3 (DR-135): the owner draws, the owner-aware selection and
\* the owner budgets of the result.
\* The old annotation is kept without its @ so that Apalache does not read it.
\* \* The settlement of member m of batch B from the balances b at R_{i-1}.
\* \* The selection places the measured use on "p" first and then on "q". The
\* \* rollback uses the same capacities, so it places the same charge.
\* \* type: (Str -> Int, Set(Int), Int) => {bal: Str -> Int, out: Str};
\* StepResult(b, B, m) ==
\*   LET capP == Capacity(b, B, m, "p")
\*       capQ == Capacity(b, B, m, "q")
\*       placedP == Min(use[m], capP)
\*       placedQ == use[m] - placedP
\*       placed == [k \in Keys |-> IF k = "p" THEN placedP ELSE placedQ]
\*       afterRun == [k \in Keys |-> b[k] - Drained(b[k], drain[m][k])]
\*       guardHolds == \A k \in Keys : GuardAt(B, m, k, afterRun[k], placed[k])
\*       restoredMinusCharge == [k \in Keys |-> b[k] - placed[k]]
\*   IN IF placedQ > capQ
\*        THEN [bal |-> b, out |-> "Unfunded"]
\*        ELSE IF fails[m]
\*          THEN [bal |-> restoredMinusCharge, out |-> "Failed"]
\*          ELSE IF guardHolds
\*            THEN [bal |-> [k \in Keys |-> afterRun[k] - placed[k]], out |-> "Settled"]
\*            ELSE [bal |-> restoredMinusCharge, out |-> "GuardRollback"]
\* The settlement of member m of batch B from the balances b at R_{i-1}.
\* The owner draws of the run come first. The selection then places the
\* measured use on "p" first and then on "q", within the capacities after
\* the owner draws. The guard covers the client and the owner debits. The
\* rollback uses the same capacities, so it places the same charge, and it
\* charges the owner debits from the restored balance.
\* @type: (Str -> Int, Set(Int), Int) => {bal: Str -> Int, out: Str, own: Str -> Int, budget: Str -> Int};
StepResult(b, B, m) ==
  LET own == [k \in Keys |-> OwnerDrawAt(b, B, m, k)]
      ownBudget == [k \in Keys |-> OwnerBudgetAt(b, B, m, k)]
      capP == SelectionCapacity(b, B, m, "p")
      capQ == SelectionCapacity(b, B, m, "q")
      placedP == Min(use[m], capP)
      placedQ == use[m] - placedP
      placed == [k \in Keys |-> IF k = "p" THEN placedP ELSE placedQ]
      debit == [k \in Keys |-> placed[k] + own[k]]
      afterRun == [k \in Keys |-> b[k] - Drained(b[k], drain[m][k])]
      guardHolds == \A k \in Keys : GuardAt(B, m, k, afterRun[k], debit[k])
      restoredMinusCharge == [k \in Keys |-> b[k] - debit[k]]
  IN IF placedQ > capQ
       THEN [bal |-> b, out |-> "Unfunded",
             own |-> [k \in Keys |-> 0], budget |-> ownBudget]
       ELSE IF fails[m]
         THEN [bal |-> restoredMinusCharge, out |-> "Failed",
               own |-> own, budget |-> ownBudget]
         ELSE IF guardHolds
           THEN [bal |-> [k \in Keys |-> afterRun[k] - debit[k]], out |-> "Settled",
                 own |-> own, budget |-> ownBudget]
           ELSE [bal |-> restoredMinusCharge, out |-> "GuardRollback",
                 own |-> own, budget |-> ownBudget]

\* Changed by G3 (DR-135): the replay also records the owner debits and the
\* owner budgets.
\* The old annotation is kept without its @ so that Apalache does not read it.
\* \* The replay of a batch B from R0, unrolled over the slots.
\* \* type: ({bal: Str -> Int, out: Int -> Str, post: Int -> (Str -> Int)}, Set(Int), Int) => {bal: Str -> Int, out: Int -> Str, post: Int -> (Str -> Int)};
\* ReplayStep(s, B, m) ==
\*   IF m \notin B
\*     THEN s
\*     ELSE LET r == StepResult(s.bal, B, m)
\*          IN [bal |-> r.bal,
\*              out |-> [s.out EXCEPT ![m] = r.out],
\*              post |-> [s.post EXCEPT ![m] = r.bal]]
\* The replay of a batch B from R0, unrolled over the slots.
\* @type: ({bal: Str -> Int, out: Int -> Str, post: Int -> (Str -> Int), own: Int -> (Str -> Int), budget: Int -> (Str -> Int)}, Set(Int), Int) => {bal: Str -> Int, out: Int -> Str, post: Int -> (Str -> Int), own: Int -> (Str -> Int), budget: Int -> (Str -> Int)};
ReplayStep(s, B, m) ==
  IF m \notin B
    THEN s
    ELSE LET r == StepResult(s.bal, B, m)
         IN [bal |-> r.bal,
             out |-> [s.out EXCEPT ![m] = r.out],
             post |-> [s.post EXCEPT ![m] = r.bal],
             own |-> [s.own EXCEPT ![m] = r.own],
             budget |-> [s.budget EXCEPT ![m] = r.budget]]

NoOutcome == [m \in Slots |-> "None"]

NoPost == [m \in Slots |-> [k \in Keys |-> 0]]

\* Changed by G3 (DR-135): the owner fields.
\* The old annotation is kept without its @ so that Apalache does not read it.
\* \* type: Set(Int) => {bal: Str -> Int, out: Int -> Str, post: Int -> (Str -> Int)};
\* Replay(B) ==
\*   LET s0 == [bal |-> supply, out |-> NoOutcome, post |-> NoPost]
\*       s1 == ReplayStep(s0, B, 1)
\*       s2 == ReplayStep(s1, B, 2)
\*       s3 == ReplayStep(s2, B, 3)
\*   IN s3
\* @type: Set(Int) => {bal: Str -> Int, out: Int -> Str, post: Int -> (Str -> Int), own: Int -> (Str -> Int), budget: Int -> (Str -> Int)};
Replay(B) ==
  LET s0 == [bal |-> supply, out |-> NoOutcome, post |-> NoPost,
             own |-> NoPost, budget |-> NoPost]
      s1 == ReplayStep(s0, B, 1)
      s2 == ReplayStep(s1, B, 2)
      s3 == ReplayStep(s2, B, 3)
  IN s3

\* The truncation rule of the proposer.
Straddles == \E s \in done, d \in batch \ done : Linked(s, d)

ShippedGuardRollback == \E s \in done : outcome[s] = "GuardRollback"

\* Added by G3 (DR-135): condition (c). A dropped member holds a custody
\* that a shipped member's owner table lists.
OwnerStraddles ==
  \E s \in done, d \in batch \ done : custody[d] \cap ownerTable[s] # {}

\* Changed by G3 (DR-135): condition (c). Control "OwnerStraddleIgnored"
\* omits it.
\* NeedsRerun == Straddles \/ ShippedGuardRollback
NeedsRerun ==
  \/ Straddles
  \/ ShippedGuardRollback
  \/ (Defect # "OwnerStraddleIgnored" /\ OwnerStraddles)

-----------------------------------------------------------------------------
(* The behavior.                                                           *)


Init ==
  /\ custody \in [Slots -> CustodyChoices \cup {{}}]
  /\ \A m \in Slots : (custody[m] = {}) <=> (m \notin Members)
  /\ hold \in [Slots -> [Keys -> 0..MaxHold]]
  /\ \A m \in Slots, k \in Keys :
       IF k \in custody[m] THEN hold[m][k] >= MinHold ELSE hold[m][k] = 0
  /\ supply \in [Keys -> 0..MaxSupply]
  /\ drain \in [Slots -> [Keys -> 0..MaxDrain]]
  /\ \A m \in Slots \ Members, k \in Keys : drain[m][k] = 0
  /\ use \in [Slots -> 0..(2 * MaxHold)]
  /\ \A m \in Slots : use[m] <= hold[m]["p"] + hold[m]["q"]
  /\ fails \in [Slots -> BOOLEAN]
  /\ \A m \in Slots : fails[m] => (UserFailures /\ m \in Members)
  \* Added by G3 (DR-135): the owner tables and the owner shares. A member
  \* asks an owner share of at least one unit of each purse that its table
  \* lists (the run consulted it for an owner region), and nothing of any
  \* other purse.
  /\ ownerTable \in [Slots -> OwnerTables]
  /\ \A m \in Slots \ Members : ownerTable[m] = {}
  /\ ownerUse \in [Slots -> [Keys -> 0..MaxOwnerUse]]
  /\ \A m \in Slots, k \in Keys :
       IF k \in ownerTable[m] THEN ownerUse[m][k] >= 1 ELSE ownerUse[m][k] = 0
  /\ ownerDebit = NoPost
  /\ ownerBudget = NoPost
  /\ phase = "Gate"
  /\ gated = {}
  /\ accepted = {}
  /\ batch = {}
  /\ done = {}
  /\ bal = supply
  /\ outcome = NoOutcome
  /\ post = NoPost
  /\ budget = TRUE
  /\ reran = FALSE
  /\ shipped = {}

\* The check of the next member at R0, in canonical order.
Gate ==
  /\ phase = "Gate"
  /\ Members \ gated # {}
  /\ gated' = gated \cup {NextIn(Members \ gated)}
  \* Changed by G3 (DR-135): the owner variables are unchanged.
  \* /\ UNCHANGED <<custody, hold, supply, drain, use, fails, phase, accepted,
  \*                batch, done, bal, outcome, post, budget, reran, shipped>>
  /\ UNCHANGED <<custody, hold, supply, drain, use, fails, phase, accepted,
                 batch, done, bal, outcome, post, budget, reran, shipped,
                 ownerTable, ownerUse, ownerDebit, ownerBudget>>

\* The decision over the gated members, in canonical order.
Decide ==
  /\ phase = "Gate"
  /\ gated = Members
  /\ accepted' = AcceptedIn(Members, Canonical)
  /\ batch' = AcceptedIn(Members, Canonical)
  /\ phase' = "Execute"
  \* Changed by G3 (DR-135): the owner variables are unchanged.
  \* /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, done, bal,
  \*                outcome, post, budget, reran, shipped>>
  /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, done, bal,
                 outcome, post, budget, reran, shipped,
                 ownerTable, ownerUse, ownerDebit, ownerBudget>>

\* The next member executes from R_{i-1}, which is the current balance.
Execute ==
  /\ phase = "Execute"
  /\ batch \ done # {}
  /\ LET m == NextIn(batch \ done)
         r == StepResult(bal, batch, m)
     IN /\ bal' = r.bal
        /\ outcome' = [outcome EXCEPT ![m] = r.out]
        /\ post' = [post EXCEPT ![m] = r.bal]
        /\ done' = done \cup {m}
        \* Added by G3 (DR-135): the owner debits and budgets of the member.
        /\ ownerDebit' = [ownerDebit EXCEPT ![m] = r.own]
        /\ ownerBudget' = [ownerBudget EXCEPT ![m] = r.budget]
  \* Changed by G3 (DR-135): the owner tables and shares are unchanged.
  \* /\ UNCHANGED <<custody, hold, supply, drain, use, fails, phase, gated,
  \*                accepted, batch, budget, reran, shipped>>
  /\ UNCHANGED <<custody, hold, supply, drain, use, fails, phase, gated,
                 accepted, batch, budget, reran, shipped,
                 ownerTable, ownerUse>>

\* Dev's play budget: one member always runs, and the budget can stop the
\* batch after any member that is not the last.
StopByBudget ==
  /\ phase = "Execute"
  /\ budget
  /\ done # {}
  /\ batch \ done # {}
  /\ phase' = "Stopped"
  \* Changed by G3 (DR-135): the owner variables are unchanged.
  \* /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, accepted,
  \*                batch, done, bal, outcome, post, budget, reran, shipped>>
  /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, accepted,
                 batch, done, bal, outcome, post, budget, reran, shipped,
                 ownerTable, ownerUse, ownerDebit, ownerBudget>>

\* The shipped prefix runs again from R0 with no budget. Its decision is
\* computed again over the prefix.
Rerun ==
  /\ phase = "Stopped"
  /\ Defect # "KeepTruncatedResults"
  /\ NeedsRerun
  /\ batch' = AcceptedIn(done, Canonical)
  /\ done' = {}
  /\ bal' = supply
  /\ outcome' = NoOutcome
  /\ post' = NoPost
  \* Added by G3 (DR-135): the re-run records its own owner results.
  /\ ownerDebit' = NoPost
  /\ ownerBudget' = NoPost
  /\ budget' = FALSE
  /\ reran' = TRUE
  /\ phase' = "Execute"
  \* Changed by G3 (DR-135): the owner tables and shares are unchanged.
  \* /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, accepted,
  \*                shipped>>
  /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, accepted,
                 shipped, ownerTable, ownerUse>>

\* The block carries the executed members with their results.
Ship ==
  /\ \/ phase = "Execute" /\ batch \ done = {}
     \/ phase = "Stopped" /\ (Defect = "KeepTruncatedResults" \/ ~NeedsRerun)
  /\ shipped' = done
  /\ phase' = "Shipped"
  \* Changed by G3 (DR-135): the owner variables are unchanged.
  \* /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, accepted,
  \*                batch, done, bal, outcome, post, budget, reran>>
  /\ UNCHANGED <<custody, hold, supply, drain, use, fails, gated, accepted,
                 batch, done, bal, outcome, post, budget, reran,
                 ownerTable, ownerUse, ownerDebit, ownerBudget>>

Next ==
  \/ Gate
  \/ Decide
  \/ Execute
  \/ StopByBudget
  \/ Rerun
  \/ Ship

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* Invariants.                                                             *)

TypeOK ==
  /\ custody \in [Slots -> CustodyChoices \cup {{}}]
  /\ hold \in [Slots -> [Keys -> 0..MaxHold]]
  /\ supply \in [Keys -> 0..MaxSupply]
  /\ drain \in [Slots -> [Keys -> 0..MaxDrain]]
  /\ use \in [Slots -> 0..(2 * MaxHold)]
  /\ fails \in [Slots -> BOOLEAN]
  /\ phase \in Phases
  /\ gated \subseteq Members
  /\ accepted \subseteq Members
  /\ batch \subseteq accepted
  /\ done \subseteq batch
  /\ bal \in [Keys -> Balances]
  /\ outcome \in [Slots -> Outcomes]
  /\ post \in [Slots -> [Keys -> Balances]]
  /\ budget \in BOOLEAN
  /\ reran \in BOOLEAN
  /\ shipped \subseteq batch
  \* Added by G3 (DR-135): the owner variables.
  /\ ownerTable \in [Slots -> OwnerTables]
  /\ ownerUse \in [Slots -> [Keys -> 0..MaxOwnerUse]]
  /\ ownerDebit \in [Slots -> [Keys -> 0..MaxOwnerUse]]
  /\ ownerBudget \in [Slots -> [Keys -> 0..MaxSupply]]

\* DR-65 items 2 to 4: a member that fails alone is never accepted, and a
\* group is accepted entirely when its holds fit and rejected entirely when
\* they do not.
GroupAllOrNone ==
  phase # "Gate" =>
    /\ accepted \subseteq Remaining
    /\ \A a, b \in Remaining :
         Connected(Remaining, a, b) => (a \in accepted <=> b \in accepted)
    /\ \A a \in Remaining : a \in accepted <=> Fits(TrueGroup(Remaining, a))

\* Every settlement charge fits: the selection finds a placement within the
\* capacities, and no key balance falls below zero.
NoOverdraft ==
  /\ \A k \in Keys : bal[k] >= 0
  /\ \A m \in Slots : outcome[m] # "Unfunded"

\* No member executes before every member was checked at R0.
GateBeforeExecute ==
  /\ phase = "Gate" => done = {} /\ outcome = NoOutcome
  /\ phase # "Gate" => gated = Members

\* The balances cover the holds of the members that did not execute yet.
HeldCapacityGuard ==
  phase \in {"Execute", "Stopped"} =>
    \A k \in Keys : bal[k] >= SumHold(batch \ done, k)

\* The decision of the canonical order equals the decision of every order.
DecisionPermutationInvariant ==
  phase # "Gate" =>
    /\ accepted = DecisionOf(Canonical).accepted
    /\ \A order \in Orders : DecisionOf(order) = DecisionOf(Canonical)

\* Changed by G3 (DR-135): the owner debits and the owner budgets of each
\* shipped member also equal the replay. A listed budget that validators
\* compute otherwise is a rejection (v2 R3).
\* \* Validators replay the shipped prefix as a batch of its own. Its decision
\* \* accepts every shipped member, and each shipped outcome, each post-state
\* \* balance and the final balances equal the replay.
\* TruncationConsistency ==
\*   phase = "Shipped" =>
\*     LET r == Replay(shipped)
\*     IN /\ AcceptedIn(shipped, Canonical) = shipped
\*        /\ \A m \in shipped : outcome[m] = r.out[m] /\ post[m] = r.post[m]
\*        /\ bal = r.bal
\* Validators replay the shipped prefix as a batch of its own. Its decision
\* accepts every shipped member, and each shipped outcome, each post-state
\* balance, each owner debit, each owner budget and the final balances equal
\* the replay.
TruncationConsistency ==
  phase = "Shipped" =>
    LET r == Replay(shipped)
    IN /\ AcceptedIn(shipped, Canonical) = shipped
       /\ \A m \in shipped :
            /\ outcome[m] = r.out[m]
            /\ post[m] = r.post[m]
            /\ ownerDebit[m] = r.own[m]
            /\ ownerBudget[m] = r.budget[m]
       /\ bal = r.bal

=============================================================================
