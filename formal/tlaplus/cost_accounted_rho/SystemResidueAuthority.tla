----------------------------- MODULE SystemResidueAuthority -----------------------------
(***************************************************************************)
(* DR-101 (bug 10056): system residue is never charged to an earlier       *)
(* deployment.                                                             *)
(*                                                                         *)
(* A shared system node (a registry tree node, for example) is written by  *)
(* system bodies that run for the deployment that fires them. Each         *)
(* deployment may write the node, then reads it, and may write and read a  *)
(* user datum. A system write stores a residue seal bound to the paying    *)
(* deployment and the written item's entropy. A read resolves the stored   *)
(* seal in the reading deployment: residue of that same deployment and     *)
(* item charges the payer, and any other residue charges nothing. A merge  *)
(* re-derives the item's entropy, so merged residue never resolves. A user *)
(* write keeps the payer's own seal. Replay recomputes every charge in the *)
(* same deployment context.                                                *)
(*                                                                         *)
(* Mutation selects a negative control:                                    *)
(*   lastWriterSeal        system writes store the payer seal (the old     *)
(*                         rule); breaks NoForeignSystemResidueDemand.     *)
(*   payerOnly             resolution ignores the deployment; breaks       *)
(*                         WriterIndependentCost.                          *)
(*   noInDeploy            residue never resolves; breaks                  *)
(*                         InDeployChargeRefinesInheritance.               *)
(*   resolveByStoredSigner residue charges its writer; breaks              *)
(*                         NoForeignSystemResidueDemand.                   *)
(*   replayWithoutContext  replay resolves nothing; breaks ReplayAgreement.*)
(*                                                                         *)
(* Rust correspondence: accounting/authority/residue.rs, reduce.rs         *)
(* (eval_send, eval_receive, produce_peeks), observation_construction.rs   *)
(* (comm_with_identity), rho_runtime.rs and native replay observations.rs  *)
(* (the residue context). Rocq companion: SystemResidueSeal.v.             *)
(***************************************************************************)
EXTENDS Naturals, Sequences

CONSTANTS Payers, MaxDeploys, Mutation

ASSUME Mutation \in {"none", "lastWriterSeal", "payerOnly", "noInDeploy",
                     "resolveByStoredSigner", "replayWithoutContext"}

NoLane == "noLane"
Merged == 0

Seal(kind, payer, d, entropy) ==
  [kind |-> kind, payer |-> payer, deploy |-> d, entropy |-> entropy]

GenesisSeal == Seal("unit", NoLane, 0, Merged)

VARIABLES deploy, node, userData, charges, replayCharges

vars == <<deploy, node, userData, charges, replayCharges>>

SealKinds == {"unit", "residue", "payer"}
Lanes == Payers \cup {NoLane}

Charge == [deploy : 0..MaxDeploys, reader : Payers, lane : Lanes,
           kind : {"system", "user"}, sameDeploy : BOOLEAN,
           writerKind : SealKinds, writer : Lanes]

TypeOK ==
  /\ deploy \in 0..MaxDeploys
  /\ node.kind \in SealKinds
  /\ userData.kind \in SealKinds
  /\ charges \in Seq(Charge)
  /\ replayCharges \in Seq(Lanes)
  /\ Len(replayCharges) = Len(charges)

(* system_residue_region: the seal that a system body stores while payer p *)
(* pays in deployment d for an item of entropy e.                          *)
SystemStore(p, d, e) ==
  IF Mutation = "lastWriterSeal" THEN Seal("payer", p, d, e) ELSE Seal("residue", p, d, e)

(* resolve_system_residue: the lane that seal s charges in deployment d of *)
(* payer p, for the item's own stored entropy.                            *)
Resolve(p, d, s, replay) ==
  CASE s.kind = "payer" -> s.payer
    [] s.kind = "unit" -> NoLane
    [] OTHER ->
         IF replay /\ Mutation = "replayWithoutContext" THEN NoLane
         ELSE IF Mutation = "noInDeploy" THEN NoLane
         ELSE IF Mutation = "resolveByStoredSigner" THEN s.payer
         ELSE IF Mutation = "payerOnly"
              THEN (IF s.payer = p /\ s.entropy # Merged THEN p ELSE NoLane)
         ELSE (IF s.payer = p /\ s.deploy = d /\ s.entropy = d THEN p ELSE NoLane)

ReadCharge(p, d, s, kind) ==
  [deploy |-> d, reader |-> p, lane |-> Resolve(p, d, s, FALSE), kind |-> kind,
   sameDeploy |-> (s.deploy = d /\ s.entropy = d),
   writerKind |-> s.kind, writer |-> s.payer]

Init ==
  /\ deploy = 0
  /\ node = GenesisSeal
  /\ userData = GenesisSeal
  /\ charges = <<>>
  /\ replayCharges = <<>>

(* One deployment of payer p: it may write the system node (or merge two   *)
(* branches that wrote it), reads the node, and may write a user datum,    *)
(* then reads the user datum when one exists.                              *)
Deploy(p, writeSystem, merge, writeUser) ==
  LET d == deploy + 1
      written == IF ~writeSystem THEN node
                 ELSE IF merge THEN [SystemStore(p, d, d) EXCEPT !.entropy = Merged]
                 ELSE SystemStore(p, d, d)
      user == IF writeUser THEN Seal("payer", p, d, d) ELSE userData
      systemRead == ReadCharge(p, d, written, "system")
      userReads == IF user.kind = "payer" THEN <<ReadCharge(p, d, user, "user")>> ELSE <<>>
      reads == <<systemRead>> \o userReads
      replayed == [i \in 1..Len(reads) |->
                     Resolve(p, d, IF reads[i].kind = "system" THEN written ELSE user, TRUE)]
  IN /\ deploy' = d
     /\ node' = written
     /\ userData' = user
     /\ charges' = charges \o reads
     /\ replayCharges' = replayCharges \o replayed

Next ==
  /\ deploy < MaxDeploys
  /\ \E p \in Payers, writeSystem \in BOOLEAN, merge \in BOOLEAN, writeUser \in BOOLEAN :
       Deploy(p, writeSystem, merge, writeUser)

Spec == Init /\ [][Next]_vars

(* A read of system residue charges the reader or nothing, never a payer *)
(* of an earlier deployment.                                             *)
NoForeignSystemResidueDemand ==
  \A i \in 1..Len(charges) :
    charges[i].kind = "system" => charges[i].lane \in {NoLane, charges[i].reader}

(* Residue written and read in the same deployment charges its payer,    *)
(* exactly as the old inheritance rule did.                              *)
InDeployChargeRefinesInheritance ==
  \A i \in 1..Len(charges) :
    (charges[i].kind = "system" /\ charges[i].sameDeploy /\ charges[i].writerKind # "unit")
      => charges[i].lane = charges[i].reader

(* Residue of any earlier deployment costs nothing, whoever wrote it. *)
WriterIndependentCost ==
  \A i \in 1..Len(charges) :
    (charges[i].kind = "system" /\ ~charges[i].sameDeploy) => charges[i].lane = NoLane

(* Genesis residue has no demand. *)
GenesisSealUnchanged ==
  \A i \in 1..Len(charges) :
    (charges[i].kind = "system" /\ charges[i].writerKind = "unit") => charges[i].lane = NoLane

(* A user-written datum keeps its writer's lane in every deployment. *)
UserTermResidueStillCharged ==
  \A i \in 1..Len(charges) :
    charges[i].kind = "user" => charges[i].lane = charges[i].writer

(* Replay charges every read exactly as play did. *)
ReplayAgreement ==
  \A i \in 1..Len(charges) : replayCharges[i] = charges[i].lane

========================================================================================
