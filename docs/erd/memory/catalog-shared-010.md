# catalog shared 010

[Inventory](inventory.md) | [PlantUML](catalog-shared-010.puml)

![catalog-shared-010](catalog-shared-010.svg)

## Cardinality and Size

[Method, units, limits, and runtime evidence](sizing.md). These are declaration-level assessments, not measured live counts.

### MessageVisitor

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `0` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |

### LoggingConfig

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `filter` | 0..n UTF-8 bytes | 24 B. String header on the generator target | capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count |
| `format` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `sink` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `file` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |

### LogFormat

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Json` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Pretty` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### LogSink

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Stdout` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `File` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Both` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### LogFileConfig

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `rotation` | 1 value | Compiler layout required | Follow the referenced declaration or external implementation. No allocation bound established. |
| `retention` | 1 scalar | 8 B | None |
| `max_file_size_bytes` | 1 scalar | 8 B | None |
| `max_total_size_bytes` | 1 scalar | 8 B | None |

### LogRotation

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `Never` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Minutely` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Hourly` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |
| `Daily` | 1 mutually exclusive alternative | No payload. Enum discriminant layout required | None for this unit variant |

### TracingGuards

Cardinality: 0..n live values. Current count and owner-level limits are not derivable from this declaration.

Size: Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout.

| Field or variant | Cardinality | Inline bytes or layout | Additional storage or wire encoding |
| --- | --- | --- | --- |
| `_file` | 0..1 present payload | Option layout includes any tag or niche optimization. Do not assume I(T) + 1 | None when absent. Present payload: 1 value. Follow the referenced declaration or external implementation. No allocation bound established. |

## Cross-Page References

No resolved cross-page references.
