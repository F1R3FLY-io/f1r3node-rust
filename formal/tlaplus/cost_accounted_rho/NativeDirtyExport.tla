--------------------------- MODULE NativeDirtyExport ---------------------------
(***************************************************************************)
(* D-S3 (epic 8946, Phase D item D-C3; DR-97): the dirty export of the     *)
(* native session store gives the root of the full export.                 *)
(*                                                                         *)
(* The store caches history contents. A cold fill reads the history        *)
(* content of an absent key as a clean entry. A write replaces a cached    *)
(* content and marks the entry dirty. A checkpoint saves the cache and its *)
(* flags, and a restore returns them. The full export writes every cached  *)
(* content over the history; the dirty export writes only the dirty ones.  *)
(* Empty stands for a value without a leaf; writing it deletes the key.    *)
(* DirtyExportEqualsFullExport: the two exports give the same map from     *)
(* keys to contents in every reachable state.                              *)
(*                                                                         *)
(* Rust correspondence: rspace++/src/rspace/hot_store/native_store.rs      *)
(*   (dirty_entries, NativeDirtyEntries::actions);                         *)
(*   rspace++/src/rspace/hot_store/native_index.rs (NativeEntry.dirty,     *)
(*   DigestShards::insert_new, replace, snapshot, restore).                *)
(* Rocq correspondence: theories/NativeDirtyExport.v                       *)
(*   (reachable_dirty_export_equals_full_export,                           *)
(*    write_without_dirty_changes_root).                                   *)
(*                                                                         *)
(* Mutation selects the negative control, which must violate the named    *)
(* invariant in its *Unsafe.cfg:                                           *)
(*   "write_without_dirty"  a write keeps the old flag                     *)
(*                          -> DirtyExportEqualsFullExport                 *)
(*   "restore_drops_dirty"  a restore clears every flag                    *)
(*                          -> DirtyExportEqualsFullExport                 *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS Keys, Values, Mutation

Empty == "empty"
Absent == "absent"

ASSUME /\ Keys # {} /\ IsFiniteSet(Keys)
       /\ Values # {} /\ IsFiniteSet(Values)
       /\ Empty \notin Values /\ Absent \notin Values
       /\ Mutation \in {"none", "write_without_dirty", "restore_drops_dirty"}

Contents == Values \cup {Empty}

VARIABLES history, cache, dirty, savedCache, savedDirty, hasSaved

vars == <<history, cache, dirty, savedCache, savedDirty, hasSaved>>

Init == /\ history \in [Keys -> Contents]
        /\ cache = [k \in Keys |-> Absent]
        /\ dirty = [k \in Keys |-> FALSE]
        /\ savedCache = [k \in Keys |-> Absent]
        /\ savedDirty = [k \in Keys |-> FALSE]
        /\ hasSaved = FALSE

\* A cold fill inserts the history content as a clean entry.
ColdFill(k) == /\ cache[k] = Absent
               /\ cache' = [cache EXCEPT ![k] = history[k]]
               /\ dirty' = [dirty EXCEPT ![k] = FALSE]
               /\ UNCHANGED <<history, savedCache, savedDirty, hasSaved>>

\* A publishing write replaces a cached content and marks the entry dirty.
Write(k, v) == /\ cache[k] # Absent
               /\ cache' = [cache EXCEPT ![k] = v]
               /\ dirty' = IF Mutation = "write_without_dirty"
                           THEN dirty
                           ELSE [dirty EXCEPT ![k] = TRUE]
               /\ UNCHANGED <<history, savedCache, savedDirty, hasSaved>>

Checkpoint == /\ savedCache' = cache
              /\ savedDirty' = dirty
              /\ hasSaved' = TRUE
              /\ UNCHANGED <<history, cache, dirty>>

Restore == /\ hasSaved
           /\ cache' = savedCache
           /\ dirty' = IF Mutation = "restore_drops_dirty"
                       THEN [k \in Keys |-> FALSE]
                       ELSE savedDirty
           /\ UNCHANGED <<history, savedCache, savedDirty, hasSaved>>

Next == \/ \E k \in Keys : ColdFill(k)
        \/ \E k \in Keys, v \in Contents : Write(k, v)
        \/ Checkpoint
        \/ Restore

Spec == Init /\ [][Next]_vars

\* The history after each export: written keys take their cached content.
FullExport == [k \in Keys |-> IF cache[k] # Absent THEN cache[k] ELSE history[k]]

DirtyExport == [k \in Keys |-> IF cache[k] # Absent /\ dirty[k] THEN cache[k] ELSE history[k]]

TypeOK == /\ history \in [Keys -> Contents]
          /\ cache \in [Keys -> Contents \cup {Absent}]
          /\ dirty \in [Keys -> BOOLEAN]
          /\ savedCache \in [Keys -> Contents \cup {Absent}]
          /\ savedDirty \in [Keys -> BOOLEAN]
          /\ hasSaved \in BOOLEAN

AbsentEntriesClean == \A k \in Keys : cache[k] = Absent => ~dirty[k]

CleanEntriesMatchHistory ==
  \A k \in Keys : (cache[k] # Absent /\ ~dirty[k]) => cache[k] = history[k]

SavedEntriesMatchHistory ==
  \A k \in Keys : (savedCache[k] # Absent /\ ~savedDirty[k]) => savedCache[k] = history[k]

DirtyExportEqualsFullExport == DirtyExport = FullExport
================================================================================
