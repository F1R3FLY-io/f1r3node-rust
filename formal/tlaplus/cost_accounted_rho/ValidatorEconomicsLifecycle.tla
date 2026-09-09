---------------- MODULE ValidatorEconomicsLifecycle ----------------
EXTENDS Integers, FiniteSets

CONSTANTS
  \* @type: Str;
  Defect

ASSUME Defect \in {
  "None",
  "HandlerFromGeneral",
  "EpochToGeneral",
  "FeeToFuel",
  "TopUpWithoutGeneralDebit",
  "FuelToGeneralWithdrawal",
  "FreshBondSubsidy",
  "RebondSubsidy",
  "CapacityFromGeneral",
  "SameCandidateTopUp",
  "SlashGeneral",
  "SlashLeavesFuel",
  "DirectRedemptionMint",
  "DuplicateGuiltyPenalty",
  "BurnWithoutSupplyReduction",
  "PartialEpochPublication",
  "PartialSlashRedemption",
  "ReplayRoleSubstitution",
  "AddressOnlyRoleCollapse",
  "StaleGenerationResolution",
  "SiblingAggregateFuelOverdraw",
  "GlobalValidatorEconomicsLock"
}

Validators == {"A", "B"}
Proposals == {"p1", "p2", "p3"}
Phases == {
  "Absent", "Active", "Withdrawing", "Quarantined", "Burned",
  "SlashPending", "ResolvePending"
}
ViolationNames == {
  "HandlerFromGeneral",
  "EpochToGeneral",
  "FeeToFuel",
  "TopUpWithoutGeneralDebit",
  "FuelToGeneralWithdrawal",
  "FreshBondSubsidy",
  "RebondSubsidy",
  "CapacityFromGeneral",
  "SameCandidateTopUp",
  "SlashGeneral",
  "SlashLeavesFuel",
  "DirectRedemptionMint",
  "DuplicateGuiltyPenalty",
  "BurnWithoutSupplyReduction",
  "PartialEpochPublication",
  "PartialSlashRedemption",
  "ReplayRoleSubstitution",
  "AddressOnlyRoleCollapse",
  "StaleGenerationResolution",
  "SiblingAggregateFuelOverdraw",
  "GlobalValidatorEconomicsLock"
}

HandlerCost == 3
ClientFee == 1
BondAmount == 2
EpochAmount == 1
MaxSteps == 6
InitialGeneral == [v \in Validators |-> IF v = "A" THEN 4 ELSE 6]
InitialFuel == [v \in Validators |-> 6]
InitialStake == [v \in Validators |-> IF v = "A" THEN BondAmount ELSE 0]
InitialTotal == 24
\* @type: (Str, Int) => <<Str, Int>>;
ResolutionKey(v, generationValue) == <<v, generationValue>>
ResolutionKeyType == Validators \X (-1..2)

VARIABLES
  \* @type: Int;
  step,
  \* @type: Str -> Int;
  general,
  \* @type: Str -> Int;
  fuel,
  \* @type: Str -> Int;
  stake,
  \* @type: Str -> Int;
  quarantine,
  \* @type: Str -> Str;
  phase,
  \* @type: Str -> Int;
  generation,
  \* @type: Str -> Int;
  quarantineGeneration,
  \* @type: Str -> Bool;
  halted,
  \* @type: Int;
  issued,
  \* @type: Int;
  burned,
  \* @type: Set(Str);
  minted,
  \* @type: Set(<<Str, Int>>);
  resolved,
  \* @type: Str -> Int;
  reserved,
  \* @type: Str -> Str;
  owner,
  \* @type: Set(Str);
  violations,
  \* @type: Str -> Int;
  replayGeneral,
  \* @type: Str -> Int;
  replayFuel,
  \* @type: Str;
  globalLock

vars == <<
  step, general, fuel, stake, quarantine, phase, generation,
  quarantineGeneration, halted, issued, burned, minted, resolved,
  reserved, owner, violations, replayGeneral, replayFuel, globalLock
>>

RoleMirror ==
  /\ replayGeneral' = general'
  /\ replayFuel' = fuel'

Advance ==
  /\ step < MaxSteps
  /\ step' = step + 1

Init ==
  /\ step = 0
  /\ general = InitialGeneral
  /\ fuel = InitialFuel
  /\ stake = InitialStake
  /\ quarantine = [v \in Validators |-> 0]
  /\ phase = [v \in Validators |-> IF v = "A" THEN "Active" ELSE "Absent"]
  /\ generation = [v \in Validators |-> IF v = "A" THEN 0 ELSE -1]
  /\ quarantineGeneration = [v \in Validators |-> -1]
  /\ halted = [v \in Validators |-> FALSE]
  /\ issued = 0
  /\ burned = 0
  /\ minted = {}
  /\ resolved = {}
  /\ reserved = [v \in Validators |-> 0]
  /\ owner = [p \in Proposals |-> "None"]
  /\ violations = {}
  /\ replayGeneral = InitialGeneral
  /\ replayFuel = InitialFuel
  /\ globalLock = "None"

TopUp(source, validator, amount) ==
  /\ Advance
  /\ source \in Validators
  /\ validator \in Validators
  /\ amount \in 1..3
  /\ phase[validator] # "Quarantined"
  /\ general[source] >= amount
  /\ general' = [general EXCEPT ![source] = @ - amount]
  /\ fuel' = [fuel EXCEPT ![validator] = @ + amount]
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, burned, minted, resolved, reserved, owner,
                  violations, globalLock>>

Execute(client, proposer) ==
  /\ Advance
  /\ client \in Validators
  /\ proposer \in Validators
  /\ phase[proposer] = "Active"
  /\ ~halted[proposer]
  /\ fuel[proposer] - reserved[proposer] >= HandlerCost
  /\ general[client] >= ClientFee
  /\ general' = [v \in Validators |->
       general[v]
       - (IF v = client THEN ClientFee ELSE 0)
       + (IF v = proposer THEN ClientFee ELSE 0)]
  /\ fuel' = [fuel EXCEPT ![proposer] = @ - HandlerCost]
  /\ burned' = burned + HandlerCost
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, minted, resolved, reserved, owner,
                  violations, globalLock>>

ReserveHandler(proposal, validator) ==
  /\ Advance
  /\ proposal \in Proposals
  /\ validator \in Validators
  /\ owner[proposal] = "None"
  /\ phase[validator] = "Active"
  /\ ~halted[validator]
  /\ fuel[validator] - reserved[validator] >= HandlerCost
  /\ reserved' = [reserved EXCEPT ![validator] = @ + HandlerCost]
  /\ owner' = [owner EXCEPT ![proposal] = validator]
  /\ UNCHANGED <<general, fuel, stake, quarantine, phase, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, violations, replayGeneral, replayFuel, globalLock>>

CommitHandler(proposal) ==
  /\ Advance
  /\ proposal \in Proposals
  /\ owner[proposal] \in Validators
  /\ LET validator == owner[proposal]
     IN /\ reserved[validator] >= HandlerCost
        /\ fuel[validator] >= HandlerCost
        /\ fuel' = [fuel EXCEPT ![validator] = @ - HandlerCost]
        /\ reserved' = [reserved EXCEPT ![validator] = @ - HandlerCost]
  /\ owner' = [owner EXCEPT ![proposal] = "None"]
  /\ general' = general
  /\ burned' = burned + HandlerCost
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, minted, resolved, violations, globalLock>>

CancelHandler(proposal) ==
  /\ Advance
  /\ proposal \in Proposals
  /\ owner[proposal] \in Validators
  /\ LET validator == owner[proposal]
     IN /\ reserved[validator] >= HandlerCost
        /\ reserved' = [reserved EXCEPT ![validator] = @ - HandlerCost]
  /\ owner' = [owner EXCEPT ![proposal] = "None"]
  /\ UNCHANGED <<general, fuel, stake, quarantine, phase, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, violations, replayGeneral, replayFuel, globalLock>>

IssueEpoch(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Active"
  /\ ~halted[validator]
  /\ validator \notin minted
  /\ fuel' = [fuel EXCEPT ![validator] = @ + EpochAmount]
  /\ issued' = issued + EpochAmount
  /\ minted' = minted \cup {validator}
  /\ general' = general
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, burned, resolved, reserved, owner, violations,
                  globalLock>>

RequestWithdrawal(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Active"
  /\ phase' = [phase EXCEPT ![validator] = "Withdrawing"]
  /\ UNCHANGED <<general, fuel, stake, quarantine, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, reserved, owner, violations, replayGeneral,
                  replayFuel, globalLock>>

CompleteWithdrawal(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Withdrawing"
  /\ general' = [general EXCEPT ![validator] = @ + stake[validator]]
  /\ stake' = [stake EXCEPT ![validator] = 0]
  /\ phase' = [phase EXCEPT ![validator] = "Absent"]
  /\ fuel' = fuel
  /\ RoleMirror
  /\ UNCHANGED <<quarantine, generation, quarantineGeneration, halted,
                  issued, burned, minted, resolved, reserved, owner,
                  violations, globalLock>>

Bond(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Absent"
  /\ generation[validator] < 2
  /\ general[validator] >= BondAmount
  /\ general' = [general EXCEPT ![validator] = @ - BondAmount]
  /\ stake' = [stake EXCEPT ![validator] = BondAmount]
  /\ phase' = [phase EXCEPT ![validator] = "Active"]
  /\ generation' = [generation EXCEPT ![validator] = @ + 1]
  /\ fuel' = fuel
  /\ RoleMirror
  /\ UNCHANGED <<quarantine, quarantineGeneration, halted, issued, burned,
                  minted, resolved, reserved, owner, violations, globalLock>>

Slash(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] \in {"Active", "Withdrawing"}
  /\ reserved[validator] = 0
  /\ quarantine' = [quarantine EXCEPT ![validator] = fuel[validator]]
  /\ fuel' = [fuel EXCEPT ![validator] = 0]
  /\ phase' = [phase EXCEPT ![validator] = "Quarantined"]
  /\ quarantineGeneration' =
       [quarantineGeneration EXCEPT ![validator] = generation[validator]]
  /\ halted' = [halted EXCEPT ![validator] = TRUE]
  /\ general' = general
  /\ RoleMirror
  /\ UNCHANGED <<stake, generation, issued, burned, minted, resolved,
                  reserved, owner, violations, globalLock>>

Vindicate(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Quarantined"
  /\ quarantineGeneration[validator] = generation[validator]
  /\ fuel' = [fuel EXCEPT ![validator] = @ + quarantine[validator]]
  /\ quarantine' = [quarantine EXCEPT ![validator] = 0]
  /\ phase' = [phase EXCEPT ![validator] = "Active"]
  /\ halted' = [halted EXCEPT ![validator] = FALSE]
  /\ resolved' = resolved \cup {ResolutionKey(validator, generation[validator])}
  /\ general' = general
  /\ RoleMirror
  /\ UNCHANGED <<stake, generation, quarantineGeneration, issued, burned,
                  minted, reserved, owner, violations, globalLock>>

Guilty(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Quarantined"
  /\ quarantineGeneration[validator] = generation[validator]
  /\ LET penalty == IF quarantine[validator] = 0 THEN 0 ELSE 1
         beneficiary == IF validator = "A" THEN "B" ELSE "A"
     IN /\ general' = [general EXCEPT ![beneficiary] = @ + penalty]
        /\ fuel' = [fuel EXCEPT ![validator] = @ + quarantine[validator] - penalty]
  /\ quarantine' = [quarantine EXCEPT ![validator] = 0]
  /\ phase' = [phase EXCEPT ![validator] = "Active"]
  /\ halted' = [halted EXCEPT ![validator] = FALSE]
  /\ resolved' = resolved \cup {ResolutionKey(validator, generation[validator])}
  /\ RoleMirror
  /\ UNCHANGED <<stake, generation, quarantineGeneration, issued, burned,
                  minted, reserved, owner, violations, globalLock>>

BurnQuarantine(validator) ==
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Quarantined"
  /\ quarantineGeneration[validator] = generation[validator]
  /\ burned' = burned + quarantine[validator]
  /\ quarantine' = [quarantine EXCEPT ![validator] = 0]
  /\ phase' = [phase EXCEPT ![validator] = "Burned"]
  /\ resolved' = resolved \cup {ResolutionKey(validator, generation[validator])}
  /\ UNCHANGED <<general, fuel, stake, generation, quarantineGeneration,
                  halted, issued, minted, reserved, owner, violations,
                  replayGeneral, replayFuel, globalLock>>

UnsafeHandlerFromGeneral ==
  /\ Defect = "HandlerFromGeneral"
  /\ Advance
  /\ phase["A"] = "Active"
  /\ general["A"] >= HandlerCost
  /\ general["B"] >= ClientFee
  /\ general' = [general EXCEPT
       !["A"] = @ - HandlerCost + ClientFee,
       !["B"] = @ - ClientFee]
  /\ fuel' = fuel
  /\ burned' = burned + HandlerCost
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, minted, resolved, reserved, owner, globalLock>>

UnsafeEpochToGeneral ==
  /\ Defect = "EpochToGeneral"
  /\ Advance
  /\ "A" \notin minted
  /\ general' = [general EXCEPT !["A"] = @ + EpochAmount]
  /\ fuel' = fuel
  /\ issued' = issued + EpochAmount
  /\ minted' = minted \cup {"A"}
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, burned, resolved, reserved, owner, globalLock>>

UnsafeFeeToFuel ==
  /\ Defect = "FeeToFuel"
  /\ Advance
  /\ phase["A"] = "Active"
  /\ fuel["A"] >= HandlerCost
  /\ general["B"] >= ClientFee
  /\ general' = [general EXCEPT !["B"] = @ - ClientFee]
  /\ fuel' = [fuel EXCEPT !["A"] = @ - HandlerCost + ClientFee]
  /\ burned' = burned + HandlerCost
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, minted, resolved, reserved, owner, globalLock>>

UnsafeTopUpWithoutDebit ==
  /\ Defect = "TopUpWithoutGeneralDebit"
  /\ Advance
  /\ general' = general
  /\ fuel' = [fuel EXCEPT !["A"] = @ + 1]
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, burned, minted, resolved, reserved, owner,
                  globalLock>>

UnsafeFuelWithdrawal ==
  /\ Defect = "FuelToGeneralWithdrawal"
  /\ Advance
  /\ fuel["A"] >= 1
  /\ general' = [general EXCEPT !["A"] = @ + 1]
  /\ fuel' = [fuel EXCEPT !["A"] = @ - 1]
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, burned, minted, resolved, reserved, owner,
                  globalLock>>

UnsafeBondSubsidy(validator, defectName, firstGeneration) ==
  /\ Defect = defectName
  /\ Advance
  /\ validator \in Validators
  /\ phase[validator] = "Absent"
  /\ generation[validator] < 2
  /\ IF firstGeneration THEN generation[validator] = -1 ELSE generation[validator] >= 0
  /\ general[validator] >= BondAmount
  /\ general' = [general EXCEPT ![validator] = @ - BondAmount]
  /\ fuel' = [fuel EXCEPT ![validator] = @ + 1]
  /\ stake' = [stake EXCEPT ![validator] = BondAmount]
  /\ phase' = [phase EXCEPT ![validator] = "Active"]
  /\ generation' = [generation EXCEPT ![validator] = @ + 1]
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<quarantine, quarantineGeneration, halted, issued, burned,
                  minted, resolved, reserved, owner, globalLock>>

UnsafeCapacityFromGeneral ==
  /\ Defect = "CapacityFromGeneral"
  /\ Advance
  /\ phase["A"] = "Active"
  /\ fuel["A"] - reserved["A"] < HandlerCost
  /\ general["A"] >= HandlerCost
  /\ violations' = violations \cup {Defect}
  /\ UNCHANGED <<general, fuel, stake, quarantine, phase, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, reserved, owner, replayGeneral, replayFuel,
                  globalLock>>

UnsafeSameCandidateTopUp ==
  /\ Defect = "SameCandidateTopUp"
  /\ Advance
  /\ phase["A"] = "Active"
  /\ fuel["A"] < HandlerCost
  /\ general["B"] >= HandlerCost - fuel["A"] + ClientFee
  /\ general' = [general EXCEPT
       !["A"] = @ + ClientFee,
       !["B"] = @ - (HandlerCost - fuel["A"] + ClientFee)]
  /\ fuel' = [fuel EXCEPT !["A"] = 0]
  /\ burned' = burned + HandlerCost
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, minted, resolved, reserved, owner, globalLock>>

UnsafeSlashGeneral ==
  /\ Defect = "SlashGeneral"
  /\ Advance
  /\ phase["A"] = "Active"
  /\ general' = [general EXCEPT !["A"] = 0]
  /\ fuel' = fuel
  /\ quarantine' = [quarantine EXCEPT !["A"] = general["A"]]
  /\ phase' = [phase EXCEPT !["A"] = "Quarantined"]
  /\ quarantineGeneration' = [quarantineGeneration EXCEPT !["A"] = generation["A"]]
  /\ halted' = [halted EXCEPT !["A"] = TRUE]
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, generation, issued, burned, minted, resolved,
                  reserved, owner, globalLock>>

UnsafeSlashLeavesFuel ==
  /\ Defect = "SlashLeavesFuel"
  /\ Advance
  /\ phase["A"] = "Active"
  /\ phase' = [phase EXCEPT !["A"] = "Quarantined"]
  /\ quarantineGeneration' = [quarantineGeneration EXCEPT !["A"] = generation["A"]]
  /\ halted' = [halted EXCEPT !["A"] = TRUE]
  /\ violations' = violations \cup {Defect}
  /\ UNCHANGED <<general, fuel, stake, quarantine, generation, issued, burned,
                  minted, resolved, reserved, owner, replayGeneral, replayFuel,
                  globalLock>>

UnsafeDirectRedemptionMint ==
  /\ Defect = "DirectRedemptionMint"
  /\ Advance
  /\ phase["A"] = "Quarantined"
  /\ fuel' = [fuel EXCEPT !["A"] = @ + quarantine["A"] + 1]
  /\ quarantine' = [quarantine EXCEPT !["A"] = 0]
  /\ phase' = [phase EXCEPT !["A"] = "Active"]
  /\ halted' = [halted EXCEPT !["A"] = FALSE]
  /\ resolved' = resolved \cup {ResolutionKey("A", generation["A"])}
  /\ general' = general
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, generation, quarantineGeneration, issued, burned,
                  minted, reserved, owner, globalLock>>

UnsafeDuplicateGuiltyPenalty ==
  /\ Defect = "DuplicateGuiltyPenalty"
  /\ Advance
  /\ ResolutionKey("A", generation["A"]) \in resolved
  /\ fuel["A"] >= 1
  /\ fuel' = [fuel EXCEPT !["A"] = @ - 1]
  /\ general' = [general EXCEPT !["B"] = @ + 1]
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, burned, minted, resolved, reserved, owner,
                  globalLock>>

UnsafeBurnWithoutSupplyReduction ==
  /\ Defect = "BurnWithoutSupplyReduction"
  /\ Advance
  /\ fuel["A"] >= HandlerCost
  /\ fuel' = [fuel EXCEPT !["A"] = @ - HandlerCost]
  /\ general' = general
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, burned, minted, resolved, reserved, owner,
                  globalLock>>

UnsafePartialEpoch ==
  /\ Defect = "PartialEpochPublication"
  /\ Advance
  /\ fuel' = [fuel EXCEPT !["A"] = @ + EpochAmount]
  /\ general' = general
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, quarantine, phase, generation, quarantineGeneration,
                  halted, issued, burned, minted, resolved, reserved, owner,
                  globalLock>>

UnsafePartialSlashRedemption ==
  /\ Defect = "PartialSlashRedemption"
  /\ Advance
  /\ phase["A"] = "Active"
  /\ fuel["A"] > 0
  /\ fuel' = [fuel EXCEPT !["A"] = @ - 1]
  /\ quarantine' = [quarantine EXCEPT !["A"] = @ + 1]
  /\ phase' = [phase EXCEPT !["A"] = "SlashPending"]
  /\ general' = general
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, generation, quarantineGeneration, halted, issued,
                  burned, minted, resolved, reserved, owner, globalLock>>

UnsafeReplayRoleSubstitution ==
  /\ Defect = "ReplayRoleSubstitution"
  /\ Advance
  /\ replayGeneral' = fuel
  /\ replayFuel' = general
  /\ violations' = violations \cup {Defect}
  /\ UNCHANGED <<general, fuel, stake, quarantine, phase, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, reserved, owner, globalLock>>

UnsafeAddressOnlyRoleCollapse ==
  /\ Defect = "AddressOnlyRoleCollapse"
  /\ Advance
  /\ replayGeneral' = [v \in Validators |-> general[v] + fuel[v]]
  /\ replayFuel' = [v \in Validators |-> 0]
  /\ violations' = violations \cup {Defect}
  /\ UNCHANGED <<general, fuel, stake, quarantine, phase, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, reserved, owner, globalLock>>

UnsafeStaleGenerationResolution ==
  /\ Defect = "StaleGenerationResolution"
  /\ Advance
  /\ phase["A"] = "Quarantined"
  /\ fuel' = [fuel EXCEPT !["A"] = @ + quarantine["A"]]
  /\ quarantine' = [quarantine EXCEPT !["A"] = 0]
  /\ phase' = [phase EXCEPT !["A"] = "Active"]
  /\ halted' = [halted EXCEPT !["A"] = FALSE]
  /\ general' = general
  /\ violations' = violations \cup {Defect}
  /\ RoleMirror
  /\ UNCHANGED <<stake, generation, quarantineGeneration, issued, burned,
                  minted, resolved, reserved, owner, globalLock>>

UnsafeSiblingOverdraw(proposal, validator) ==
  /\ Defect = "SiblingAggregateFuelOverdraw"
  /\ Advance
  /\ proposal \in Proposals
  /\ validator \in Validators
  /\ owner[proposal] = "None"
  /\ fuel[validator] >= HandlerCost
  /\ fuel[validator] - reserved[validator] < HandlerCost
  /\ reserved' = [reserved EXCEPT ![validator] = @ + HandlerCost]
  /\ owner' = [owner EXCEPT ![proposal] = validator]
  /\ violations' = violations \cup {Defect}
  /\ UNCHANGED <<general, fuel, stake, quarantine, phase, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, replayGeneral, replayFuel, globalLock>>

UnsafeGlobalLock ==
  /\ Defect = "GlobalValidatorEconomicsLock"
  /\ Advance
  /\ globalLock = "None"
  /\ globalLock' = "A"
  /\ violations' = violations \cup {Defect}
  /\ UNCHANGED <<general, fuel, stake, quarantine, phase, generation,
                  quarantineGeneration, halted, issued, burned, minted,
                  resolved, reserved, owner, replayGeneral, replayFuel>>

SafeNext ==
  \/ \E source, validator \in Validators, amount \in 1..3 :
       TopUp(source, validator, amount)
  \/ \E client, proposer \in Validators : Execute(client, proposer)
  \/ \E proposal \in Proposals, validator \in Validators :
       ReserveHandler(proposal, validator)
  \/ \E proposal \in Proposals : CommitHandler(proposal)
  \/ \E proposal \in Proposals : CancelHandler(proposal)
  \/ \E validator \in Validators : IssueEpoch(validator)
  \/ \E validator \in Validators : RequestWithdrawal(validator)
  \/ \E validator \in Validators : CompleteWithdrawal(validator)
  \/ \E validator \in Validators : Bond(validator)
  \/ \E validator \in Validators : Slash(validator)
  \/ \E validator \in Validators : Vindicate(validator)
  \/ \E validator \in Validators : Guilty(validator)
  \/ \E validator \in Validators : BurnQuarantine(validator)

UnsafeNext ==
  \/ UnsafeHandlerFromGeneral
  \/ UnsafeEpochToGeneral
  \/ UnsafeFeeToFuel
  \/ UnsafeTopUpWithoutDebit
  \/ UnsafeFuelWithdrawal
  \/ UnsafeBondSubsidy("B", "FreshBondSubsidy", TRUE)
  \/ UnsafeBondSubsidy("A", "RebondSubsidy", FALSE)
  \/ UnsafeCapacityFromGeneral
  \/ UnsafeSameCandidateTopUp
  \/ UnsafeSlashGeneral
  \/ UnsafeSlashLeavesFuel
  \/ UnsafeDirectRedemptionMint
  \/ UnsafeDuplicateGuiltyPenalty
  \/ UnsafeBurnWithoutSupplyReduction
  \/ UnsafePartialEpoch
  \/ UnsafePartialSlashRedemption
  \/ UnsafeReplayRoleSubstitution
  \/ UnsafeAddressOnlyRoleCollapse
  \/ UnsafeStaleGenerationResolution
  \/ \E proposal \in Proposals, validator \in Validators :
       UnsafeSiblingOverdraw(proposal, validator)
  \/ UnsafeGlobalLock

Next == SafeNext \/ UnsafeNext

Spec == Init /\ [][Next]_vars

CurrentCustody ==
  general["A"] + general["B"]
  + fuel["A"] + fuel["B"]
  + stake["A"] + stake["B"]
  + quarantine["A"] + quarantine["B"]

TypeOK ==
  /\ step \in 0..MaxSteps
  /\ general \in [Validators -> 0..24]
  /\ fuel \in [Validators -> 0..24]
  /\ stake \in [Validators -> 0..BondAmount]
  /\ quarantine \in [Validators -> 0..24]
  /\ phase \in [Validators -> Phases]
  /\ generation \in [Validators -> -1..2]
  /\ quarantineGeneration \in [Validators -> -1..2]
  /\ halted \in [Validators -> BOOLEAN]
  /\ issued \in 0..6
  /\ burned \in 0..24
  /\ minted \subseteq Validators
  /\ resolved \subseteq ResolutionKeyType
  /\ reserved \in [Validators -> 0..12]
  /\ owner \in [Proposals -> Validators \cup {"None"}]
  /\ violations \subseteq ViolationNames
  /\ replayGeneral \in [Validators -> 0..24]
  /\ replayFuel \in [Validators -> 0..24]
  /\ globalLock \in Validators \cup {"None"}

CustodyConserved == CurrentCustody + burned = InitialTotal + issued
HandlerUsesValidatorFuel == "HandlerFromGeneral" \notin violations
EpochCreditsValidatorFuel == "EpochToGeneral" \notin violations
FeeCreditsGeneralCustody == "FeeToFuel" \notin violations
TopUpConservesCustody == "TopUpWithoutGeneralDebit" \notin violations
FuelHasNoWithdrawalPath == "FuelToGeneralWithdrawal" \notin violations
FreshBondCreatesNoFuel == "FreshBondSubsidy" \notin violations
RebondCreatesNoFuel == "RebondSubsidy" \notin violations
CapacityUsesValidatorFuel == "CapacityFromGeneral" \notin violations
CandidateCannotSelfFundHandler == "SameCandidateTopUp" \notin violations
SlashPreservesGeneralCustody == "SlashGeneral" \notin violations
SlashRemovesAvailableFuel == "SlashLeavesFuel" \notin violations
RedemptionCreatesNoFuel == "DirectRedemptionMint" \notin violations
GuiltyResolutionIsIdempotent == "DuplicateGuiltyPenalty" \notin violations
BurnReducesCirculatingSupply == "BurnWithoutSupplyReduction" \notin violations
EpochPublicationIsAtomic == "PartialEpochPublication" \notin violations
SlashResolutionIsAtomic ==
  /\ "PartialSlashRedemption" \notin violations
  /\ \A validator \in Validators :
       phase[validator] \notin {"SlashPending", "ResolvePending"}
PlayReplayRolesAgree ==
  /\ "ReplayRoleSubstitution" \notin violations
  /\ replayGeneral = general
  /\ replayFuel = fuel
AddressAndRoleIdentifyCustody == "AddressOnlyRoleCollapse" \notin violations
ResolutionUsesCurrentGeneration == "StaleGenerationResolution" \notin violations
SiblingReservationsAreBounded ==
  /\ "SiblingAggregateFuelOverdraw" \notin violations
  /\ \A validator \in Validators : reserved[validator] <= fuel[validator]
ValidatorOperationsRemainIndependent ==
  /\ "GlobalValidatorEconomicsLock" \notin violations
  /\ globalLock = "None"
NoEconomicViolation == violations = {}

=====================================================================
