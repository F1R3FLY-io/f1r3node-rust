use std::mem::size_of;

use quote::ToTokens;
use serde::Serialize;
use syn::{GenericArgument, PathArguments, Type};

#[derive(Debug, Serialize)]
pub struct Assessment {
    pub cardinality: String,
    pub inline_bytes: Option<usize>,
    pub inline_layout: String,
    pub additional_storage: String,
}

fn assessment(
    count: &str,
    bytes: Option<usize>,
    layout: impl Into<String>,
    heap: impl Into<String>,
) -> Assessment {
    Assessment {
        cardinality: count.into(),
        inline_bytes: bytes,
        inline_layout: layout.into(),
        additional_storage: heap.into(),
    }
}

fn unknown() -> Assessment {
    assessment("1 value", None, "Compiler layout required", "Follow the referenced declaration or external implementation. No allocation bound established.")
}

fn arguments(segment: &syn::PathSegment) -> Vec<&Type> {
    match &segment.arguments {
        PathArguments::AngleBracketed(a) => a
            .args
            .iter()
            .filter_map(|a| match a {
                GenericArgument::Type(t) => Some(t),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn rust_type(ty: &Type) -> Assessment {
    match ty {
        Type::Paren(t) => rust_type(&t.elem),
        Type::Group(t) => rust_type(&t.elem),
        Type::Array(a) => {
            let count = a.len.to_token_stream().to_string();
            let child = rust_type(&a.elem);
            let n = match &a.len {
                syn::Expr::Lit(l) => match &l.lit {
                    syn::Lit::Int(i) => i.base10_parse::<usize>().ok(),
                    _ => None,
                },
                _ => None,
            };
            assessment(
                &format!("{count} elements"),
                n.zip(child.inline_bytes)
                    .and_then(|(n, b)| n.checked_mul(b)),
                format!("{count} * I(element), inline array"),
                format!(
                    "Sum of element-owned allocations. Element: {}",
                    child.additional_storage
                ),
            )
        }
        Type::Tuple(t) if t.elems.is_empty() => assessment("1 unit", Some(0), "0 B", "None"),
        Type::Tuple(t) => assessment(
            &format!("{} component values", t.elems.len()),
            None,
            "Sum of component inline sizes plus compiler padding",
            "Sum of component-owned allocations",
        ),
        Type::Reference(t) => assessment(
            "1 borrowed reference",
            None,
            "Pointer or fat pointer. Referent layout required",
            format!(
                "No owned allocation. Referent: {}",
                rust_type(&t.elem).cardinality
            ),
        ),
        Type::Ptr(_) => assessment(
            "1 pointer, possibly null",
            None,
            "Pointer or fat pointer. Pointee layout required",
            "Ownership and lifetime are not established by a raw pointer",
        ),
        Type::Slice(_) => assessment(
            "0..n elements",
            None,
            "Unsized slice",
            "Storage belongs to its owner. Length and capacity require measurement",
        ),
        Type::BareFn(_) => assessment(
            "1 function pointer",
            Some(size_of::<fn()>()),
            "Function pointer on the generator target",
            "None for the pointer itself",
        ),
        Type::Path(p) => {
            let Some(s) = p.path.segments.last() else {
                return unknown();
            };
            let args = arguments(s);
            let child = args.first().map(|t| rust_type(t));
            let width = match s.ident.to_string().as_str() {
                "u8" | "i8" | "bool" => Some(1),
                "u16" | "i16" => Some(2),
                "u32" | "i32" | "f32" | "char" => Some(4),
                "u64" | "i64" | "f64" => Some(8),
                "u128" | "i128" => Some(16),
                "usize" | "isize" => Some(size_of::<usize>()),
                _ => None,
            };
            if let Some(b) = width {
                return assessment("1 scalar", Some(b), format!("{b} B"), "None");
            }
            match s.ident.to_string().as_str() {
                "String" => assessment("0..n UTF-8 bytes", Some(size_of::<String>()), "String header on the generator target", "capacity() bytes, excluding allocator overhead. UTF-8 byte length is not character count"),
                "Vec" => assessment("0..n elements", Some(size_of::<Vec<()>>()), "Vec header on the generator target", format!("capacity * I(element), plus live element-owned allocations. Zero-sized elements need no backing allocation. Element: {}", child.map_or_else(|| "unknown".into(), |c| c.additional_storage))),
                "HashMap" | "BTreeMap" | "DashMap" | "IndexMap" | "LruCache" => assessment("0..n key/value pairs", None, "Container and hasher implementation required", "n keys and n values, their owned allocations, spare capacity, buckets or tree nodes, and control bytes. No type-level count cap"),
                "HashSet" | "BTreeSet" | "IndexSet" => assessment("0..n distinct elements", None, "Container and hasher implementation required", "Element storage, owned allocations, spare capacity and index overhead. No type-level count cap"),
                "VecDeque" | "LinkedList" | "BinaryHeap" => assessment("0..n elements", None, "Container implementation required", "Element storage, owned allocations, spare capacity or per-node links. No type-level count cap"),
                "Option" => assessment("0..1 present payload", None, "Option layout includes any tag or niche optimization. Do not assume I(T) + 1", format!("None when absent. Present payload: {}. {}", child.as_ref().map_or("unknown", |c| c.cardinality.as_str()), child.as_ref().map_or("Allocation bound unknown", |c| c.additional_storage.as_str()))),
                "Result" => assessment("1 of 2 alternatives", None, "Maximum variant layout plus compiler tag and padding", "Only the active Ok or Err payload contributes owned allocations"),
                "Arc" | "Rc" => assessment("1 shared strong reference", None, "Pointer or fat pointer, depending on target type", "One shared allocation holds the target, counters and alignment. Count it once across all handles"),
                "Weak" => assessment("0..1 live shared target", None, "Pointer or fat pointer, depending on target type", "May keep the control allocation alive after the value is dropped. Do not multiply target bytes by handle count"),
                "Box" => assessment("1 owned target", None, "Pointer or fat pointer, depending on target type", "I(target) and its owned allocations, plus allocator overhead. Zero-sized targets need no allocation"),
                "Mutex" | "RwLock" | "RefCell" | "Cell" | "UnsafeCell" | "OnceLock" | "OnceCell" | "LazyLock" | "Lazy" => assessment("1 wrapper slot", None, "Wrapper implementation and payload layout required", "Payload allocations and any lock/waiter allocations. Once and lazy payloads can be uninitialized"),
                "PhantomData" => assessment("0 runtime payloads", Some(0), "0 B", "None"),
                "AtomicUsize" | "AtomicIsize" | "AtomicU8" | "AtomicU16" | "AtomicU32" | "AtomicU64" | "AtomicBool" => assessment("1 atomic value", None, "Target atomic layout required", "None for the atomic value itself"),
                _ => unknown(),
            }
        }
        _ => unknown(),
    }
}

pub fn field(kind: &str, ty: &str, name: &str) -> Assessment {
    if kind == "protobuf enum" {
        return assessment(
            "1 mutually exclusive alternative",
            None,
            "Generated Rust enum layout required",
            "On wire: enum varint, 1..10 B, plus field tag when present",
        );
    }
    if kind == "protobuf" {
        let repeated = ty.starts_with("repeated ");
        let element = ty.strip_prefix("repeated ").unwrap_or(ty);
        let payload = match element {
            "Bool" => "1 B",
            "Uint32" | "Sint32" => "1..5 B varint",
            "Int32" | "Int64" | "Uint64" | "Sint64" => "1..10 B varint",
            "Fixed32" | "Sfixed32" | "Float" => "4 B",
            "Fixed64" | "Sfixed64" | "Double" => "8 B",
            "String" | "Bytes" => "v(L) + L bytes, where L is the byte length",
            _ => "Nested message: v(P) + P bytes. Enum: 1..10 B varint. Resolve the referenced declaration",
        };
        let occurrence = if repeated {
            "0..n encoded values"
        } else if name.contains(" oneof ") {
            "0..1, exclusive within oneof"
        } else {
            "0..1 encoded value for these proto3 schemas"
        };
        let wire = if repeated {
            format!("Wire element: {payload}. Unpacked: sum(tag + element). Packed numeric field: tag + v(sum(elements)) + sum(elements). Encoding depends on field options")
        } else {
            format!("Wire when present: tag + {payload}. Defaults can be omitted. tag = v((field_number << 3) | wire_type)")
        };
        return assessment(
            occurrence,
            None,
            "Generated Rust field layout required. Wire size is not heap size",
            wire,
        );
    }
    if ty == "variant" || ty.starts_with("variant = ") {
        return assessment(
            "1 mutually exclusive alternative",
            None,
            "No payload. Enum discriminant layout required",
            "None for this unit variant",
        );
    }
    let mut result = syn::parse_str::<Type>(ty).map_or_else(|_| unknown(), |t| rust_type(&t));
    if kind == "enum" {
        result.cardinality.push_str(" when this variant is active");
    }
    result
}

pub fn entity_count(kind: &str, scope: &str) -> &'static str {
    match kind {
        "alias" => "Alias, not a separate allocation. Count values of the target type",
        "static" | "lazy static" if scope.contains("thread_local") => "One slot per accessing thread. Live payload can be uninitialized",
        "static" | "lazy static" => "One slot per linked process instance. Lazy payload can be uninitialized",
        _ => "0..n live values. Current count and owner-level limits are not derivable from this declaration",
    }
}

pub fn entity_size(kind: &str) -> &'static str {
    match kind {
        "alias" => "Same layout and allocations as target. Do not add alias bytes again",
        "enum" | "protobuf enum" => "Compiler enum layout plus allocations of the active variant only. Do not sum alternative variants",
        "protobuf" => "Generated Rust layout plus owned allocations. Encoded size is the sum of present field encodings",
        "static" | "lazy static" => "Declared slot or lazy wrapper layout plus initialized owned allocations",
        _ => "Sum of field inline layouts plus compiler padding, then uniquely owned allocations. repr and custom implementations can change layout",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_array_and_zero_sized_layouts() {
        assert_eq!(field("struct", "u64", "x").inline_bytes, Some(8));
        assert_eq!(field("struct", "[u8; 32]", "x").inline_bytes, Some(32));
        assert_eq!(field("struct", "[u64; 0]", "x").inline_bytes, Some(0));
        assert_eq!(field("struct", "[u8; LIMIT]", "x").inline_bytes, None);
        assert_eq!(field("struct", "PhantomData<T>", "x").inline_bytes, Some(0));
    }

    #[test]
    fn containers_report_capacity_not_just_length() {
        let vec = field("struct", "Vec<Vec<u8>>", "x");
        assert_eq!(vec.inline_bytes, Some(size_of::<Vec<()>>()));
        assert!(vec.additional_storage.contains("capacity * I(element)"));
        assert_eq!(
            field("struct", "HashMap<K, V>", "x").cardinality,
            "0..n key/value pairs"
        );
        assert!(field("struct", "String", "x")
            .additional_storage
            .contains("not character count"));
    }

    #[test]
    fn optional_nested_collections_preserve_presence_and_length() {
        let optional = field("struct", "Option<Vec<u64>>", "x");
        assert_eq!(optional.cardinality, "0..1 present payload");
        assert!(optional.additional_storage.contains("0..n elements"));
        assert_eq!(optional.inline_bytes, None);
        assert!(optional.inline_layout.contains("niche"));
    }

    #[test]
    fn shared_borrowed_and_opaque_values_do_not_invent_heap_sizes() {
        assert!(field("struct", "Arc<dyn Value>", "x")
            .additional_storage
            .contains("once"));
        assert!(field("struct", "&[u8]", "x")
            .additional_storage
            .contains("No owned"));
        assert_eq!(field("struct", "Custom<T>", "x").inline_bytes, None);
        assert!(entity_count("alias", "").contains("not a separate allocation"));
        assert!(entity_count("static", "thread_local").contains("per accessing thread"));
        assert!(entity_size("enum").contains("active variant only"));
    }

    #[test]
    fn protobuf_wire_sizes_are_not_rust_layout_sizes() {
        let negative = field("protobuf", "Int32", "number #1");
        assert!(negative.additional_storage.contains("1..10 B"));
        assert_eq!(negative.inline_bytes, None);
        let packed = field("protobuf", "repeated Uint64", "values #2");
        assert!(packed.additional_storage.contains("Packed numeric"));
        assert!(field("protobuf", "Bytes", "a #3 oneof choice")
            .cardinality
            .contains("exclusive"));
    }
}
