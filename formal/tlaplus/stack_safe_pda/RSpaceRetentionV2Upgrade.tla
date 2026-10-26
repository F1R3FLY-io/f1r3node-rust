------------------------ MODULE RSpaceRetentionV2Upgrade ------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Old-reader compatibility of the shared retention V2 format.

The durable catalog keeps two slots.  A publication writes the next epoch
into the inactive slot and is acknowledged only when that slot is valid; a
crash during the write leaves the inactive slot torn while the active slot
keeps the newest acknowledged epoch.

The V2 format changes no catalog envelope byte.  It changes only which object
the opaque retention reference appoints: a V1 retention manifest
(RSpaceDAG.Retn) or a V2 retention anchor (RSpaceDAG.RetA).  Catalog
selection reads only the envelope, so it is blind to the format and a V1-only
reader selects the same slot as a dual-version reader.  A reader then reads
the appointed retention object: it adopts the selected state when it supports
the appointed format and refuses otherwise.  Refusal is terminal: no reader
falls back to the other slot, so no reader adopts a stale state.

The V1-to-V2 migration is one-way: after the first acknowledged V2
publication every later publication appoints a V2 anchor.  Every catalog
change closes the readers, which then reopen against the new catalog, so a
reader view always describes the current catalog.

Four negative controls each break one design decision: a V2 envelope change,
format-aware selection, fallback after refusal, and adoption of unsupported
content.  Three non-vacuity witnesses show that old readers do refuse V2,
dual readers do adopt V2, and readers adopt the previous authority behind a
torn successor.
***************************************************************************)

CONSTANT MaxEpoch

ASSUME MaxEpoch \in Nat /\ MaxEpoch >= 2

Slots == {"A", "B"}
Images == {"empty", "valid", "torn"}
Formats == {"V1", "V2"}
Envelopes == {"catalog-v1", "changed"}
Readers == {"old", "dual"}
Outcomes == {"closed", "adopted", "refused"}

Supports(reader) == IF reader = "old" THEN {"V1"} ELSE {"V1", "V2"}

SlotRecords ==
    [epoch : 0..MaxEpoch, image : Images, format : Formats,
     envelope : Envelopes]

ViewRecords ==
    [outcome : Outcomes, selected : Slots \cup {"none"},
     epoch : 0..MaxEpoch, format : Formats]

Closed ==
    [outcome |-> "closed", selected |-> "none", epoch |-> 0, format |-> "V1"]

NoAuthority ==
    [outcome |-> "refused", selected |-> "none", epoch |-> 0, format |-> "V1"]

VARIABLES slot, published, migrated, view

vars == <<slot, published, migrated, view>>

Init ==
    /\ slot = [s \in Slots |->
                 IF s = "A"
                 THEN [epoch |-> 0, image |-> "valid", format |-> "V1",
                       envelope |-> "catalog-v1"]
                 ELSE [epoch |-> 0, image |-> "empty", format |-> "V1",
                       envelope |-> "catalog-v1"]]
    /\ published = 0
    /\ migrated = FALSE
    /\ view = [reader \in Readers |-> Closed]

(* The slot holding the newest acknowledged publication, and its sibling. *)
ActiveSlot ==
    CHOOSE s \in Slots : slot[s].image = "valid" /\ slot[s].epoch = published

InactiveSlot == CHOOSE s \in Slots : s # ActiveSlot

AppointableFormats == IF migrated THEN {"V2"} ELSE Formats

WriteSuccessor(format, envelope) ==
    /\ published < MaxEpoch
    /\ format \in AppointableFormats
    /\ slot' = [slot EXCEPT ![InactiveSlot] =
                  [epoch |-> published + 1, image |-> "valid",
                   format |-> format, envelope |-> envelope]]
    /\ published' = published + 1
    /\ migrated' = (migrated \/ format = "V2")
    /\ view' = [reader \in Readers |-> Closed]

Publish == \E format \in Formats : WriteSuccessor(format, "catalog-v1")

CrashDuringPublish ==
    /\ published < MaxEpoch
    /\ \E format \in AppointableFormats :
         slot' = [slot EXCEPT ![InactiveSlot] =
                    [epoch |-> published + 1, image |-> "torn",
                     format |-> format, envelope |-> "catalog-v1"]]
    /\ view' = [reader \in Readers |-> Closed]
    /\ UNCHANGED <<published, migrated>>

(* Catalog selection: the newest slot whose envelope the reader can parse.
   Only the negative control below writes an envelope a V1 reader cannot
   parse. *)
EnvelopeReadable(reader, s) ==
    /\ slot[s].image = "valid"
    /\ (slot[s].envelope = "catalog-v1" \/ reader = "dual")

Selectable(reader) == {s \in Slots : EnvelopeReadable(reader, s)}

NewestOf(candidates) ==
    CHOOSE s \in candidates :
      \A other \in candidates : slot[other].epoch <= slot[s].epoch

Adopt(s) ==
    [outcome |-> "adopted", selected |-> s, epoch |-> slot[s].epoch,
     format |-> slot[s].format]

Refuse(s) ==
    [outcome |-> "refused", selected |-> s, epoch |-> slot[s].epoch,
     format |-> slot[s].format]

(* Reading the appointed retention object of the selected slot. *)
ReadAppointed(reader, s) ==
    IF slot[s].format \in Supports(reader) THEN Adopt(s) ELSE Refuse(s)

Open(reader) ==
    /\ view[reader].outcome = "closed"
    /\ view' = [view EXCEPT ![reader] =
                  IF Selectable(reader) = {}
                  THEN NoAuthority
                  ELSE ReadAppointed(reader, NewestOf(Selectable(reader)))]
    /\ UNCHANGED <<slot, published, migrated>>

OpenAny == \E reader \in Readers : Open(reader)

Next == Publish \/ CrashDuringPublish \/ OpenAny

Spec == Init /\ [][Next]_vars

RetentionV2TypeInvariant ==
    /\ slot \in [Slots -> SlotRecords]
    /\ published \in 0..MaxEpoch
    /\ migrated \in BOOLEAN
    /\ view \in [Readers -> ViewRecords]

AdoptionIsNewestPublished ==
    \A reader \in Readers :
      view[reader].outcome = "adopted" =>
        /\ view[reader].selected = ActiveSlot
        /\ view[reader].epoch = published

NoReaderAdoptsUnsupportedContent ==
    \A reader \in Readers :
      view[reader].outcome = "adopted" =>
        view[reader].format \in Supports(reader)

ReadersSelectTheSameSlot ==
    \A first, second \in Readers :
      (view[first].outcome # "closed" /\ view[second].outcome # "closed") =>
        view[first].selected = view[second].selected

(***************************************************************************
Non-vacuity witnesses.  Each must be violated by the faithful specification.
***************************************************************************)
NeverOldReaderRefusesV2 ==
    ~(view["old"].outcome = "refused" /\ view["old"].format = "V2")

NeverDualReaderAdoptsV2 ==
    ~(view["dual"].outcome = "adopted" /\ view["dual"].format = "V2")

NeverAdoptsBehindTornSuccessor ==
    ~(\E reader \in Readers :
        /\ view[reader].outcome = "adopted"
        /\ slot[InactiveSlot].image = "torn")

(***************************************************************************
Negative control: a V2 publication changes the catalog envelope.  A V1 reader
cannot parse the new envelope, selects the older slot and adopts its stale
V1 state.
***************************************************************************)
PublishChangedEnvelopeMutant == WriteSuccessor("V2", "changed")

CatalogEnvelopeMutantNext == Next \/ PublishChangedEnvelopeMutant
CatalogEnvelopeMutantSpec == Init /\ [][CatalogEnvelopeMutantNext]_vars

(***************************************************************************
Negative control: selection consults the appointed format.  A V1 reader
selects the newest slot whose format it supports, which is not the dual
reader's selection after a V2 publication.
***************************************************************************)
OpenFormatAwareMutant(reader) ==
    /\ view[reader].outcome = "closed"
    /\ LET supported ==
             {s \in Selectable(reader) : slot[s].format \in Supports(reader)}
       IN view' = [view EXCEPT ![reader] =
                     IF supported = {} THEN NoAuthority
                     ELSE Adopt(NewestOf(supported))]
    /\ UNCHANGED <<slot, published, migrated>>

SelectionDivergenceMutantNext ==
    Next \/ \E reader \in Readers : OpenFormatAwareMutant(reader)
SelectionDivergenceMutantSpec ==
    Init /\ [][SelectionDivergenceMutantNext]_vars

(***************************************************************************
Negative control: after refusing the unsupported content of the selected
slot, a reader falls back to the other valid slot and adopts its state.  The
catalog selection agrees with the dual reader; the adopted state is stale.
***************************************************************************)
OpenWithFallbackMutant(reader) ==
    /\ view[reader].outcome = "closed"
    /\ Selectable(reader) # {}
    /\ LET chosen == NewestOf(Selectable(reader))
           fallbacks ==
             {s \in Selectable(reader) \ {chosen} :
                slot[s].format \in Supports(reader)}
       IN /\ slot[chosen].format \notin Supports(reader)
          /\ fallbacks # {}
          /\ view' = [view EXCEPT ![reader] =
                        [outcome |-> "adopted", selected |-> chosen,
                         epoch |-> slot[NewestOf(fallbacks)].epoch,
                         format |-> slot[NewestOf(fallbacks)].format]]
    /\ UNCHANGED <<slot, published, migrated>>

FallbackOnUnsupportedMutantNext ==
    Next \/ \E reader \in Readers : OpenWithFallbackMutant(reader)
FallbackOnUnsupportedMutantSpec ==
    Init /\ [][FallbackOnUnsupportedMutantNext]_vars

(***************************************************************************
Negative control: a reader adopts the selected state without checking the
appointed format, as a V1 reader would if it parsed a V2 anchor as a V1
retention manifest.
***************************************************************************)
OpenAdoptAnyMutant(reader) ==
    /\ view[reader].outcome = "closed"
    /\ Selectable(reader) # {}
    /\ view' = [view EXCEPT ![reader] = Adopt(NewestOf(Selectable(reader)))]
    /\ UNCHANGED <<slot, published, migrated>>

AdoptUnsupportedMutantNext ==
    Next \/ \E reader \in Readers : OpenAdoptAnyMutant(reader)
AdoptUnsupportedMutantSpec == Init /\ [][AdoptUnsupportedMutantNext]_vars

=============================================================================
