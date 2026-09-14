# Atomic record publication

R3 verifies that every minimal record reaches its final path through an atomic rename.
The records are the guardian breach record, the protection breach record, the early-exit record, the persisted state, the summary text, and the summary JSON.
A reader of a record under its final name therefore sees complete content, never a partial write.
Durability of the content is a separate bounded step that the bounded publication cycle covers.

The fixture runs the production driver in Docker isolation as uid 65534.
A substituted `mv` command records each rename from a temporary name to a final path.
The disk probe drops below the hard floor after the workload starts, so the guardian fires during the first iteration.
The verdict requires an observed rename for each record and rejects any record that reached its final path without one.

The corrected driver writes each record to a temporary name and renames it into place.
The matched GREEN exits 0.
Every record shows an observed atomic rename, and the verdict rejects an in-place control that published the same records without a rename.
The in-place control writes the final files directly and logs no rename.

## Correspondence

| Model element | Production or fixture boundary |
| --- | --- |
| `WriteTempPartial`, `WriteTempFull` | The driver writes the record content to a temporary name. |
| `Rename` | The driver renames the temporary file into place. The final path becomes complete at once. |
| `WriteInPlacePartial`, `WriteInPlaceFull` | The rejected in-place control writes the final path directly. |
| `VisibleImpliesComplete` | A record under its final name is complete, or absent. |
| `Publishes` | The record eventually reaches its final path. |

The in-place configuration violates `VisibleImpliesComplete` with TLC exit 12.
The atomic configuration passes with 6 distinct states.

## Limits

The fixture observes the rename through a substituted command and does not prove filesystem atomicity of the rename itself.
The verdict tests atomic visibility, not durability, which the bounded publication cycle verifies separately.
The model represents one record and does not represent concurrent publishers.
Producer failure is verified separately by the record-producer cycle.

No native service manager, real Docker daemon, or disposable runner executed this cycle.
Construction is not applicable to this Bash-driver model.
D2 and claim discharge remain pending.
