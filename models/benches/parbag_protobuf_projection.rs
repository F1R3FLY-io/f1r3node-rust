//! ParBag protobuf egress projection-policy experiment.
//!
//! Five policy arms share the production RPB3 zipper encoder:
//!
//! - `direct` writes resident protobuf bodies to the final sink and retains no
//!   second image;
//! - `cold_cached` explicitly constructs the bounded atomic component
//!   projection and then emits it around root metadata;
//! - `warm_cached` reuses that prepared projection;
//! - `cold_adaptive` takes the proposed cache-hit-or-direct route with no
//!   existing projection;
//! - `warm_adaptive` exercises the same route with a shared projection.
//!
//! Fixture construction, output allocation, and byte comparison are outside
//! the timed interval. Warm preparation is also outside it; `cold_cached`
//! deliberately times preparation plus emission. A counting allocator thus
//! isolates allocations caused by the selected egress policy. Every
//! observation is rejected unless it is byte-identical to the direct RPB3
//! authority.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, EList, Expr, GPrivate, GUnforgeable, If, Match, New, Par,
    Receive, Send,
};
use models::rust::channel_image::ChannelImageV2;
use models::rust::par::ParProtobufSinkError;
use models::rust::par_bag::{Multiplicity, ParBag};
use models::rust::rholang::protobuf_encoder;
use prost::Message as _;

const WIDTHS: [usize; 4] = [1, 16, 256, 4_096];
const BODY_BYTES: [usize; 3] = [16, 256, 4_096];
const MULTIPLICITIES: [u64; 3] = [1, 2, 16];
const DEPTHS: [usize; 3] = [16, 256, 4_096];
const DEFAULT_WARMUP: usize = 5;
const DEFAULT_REPS: usize = 31;
const DEFAULT_PROFILE_ITERATIONS: usize = 1_000;

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, old, new_size) };
        if !replacement.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        replacement
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Direct,
    ColdCached,
    WarmCached,
    ColdAdaptive,
    WarmAdaptive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SinkKind {
    Vec,
    BytesMut,
    Blake2b,
    BufferedWrite,
}

impl SinkKind {
    const ALL: [Self; 4] = [
        Self::Vec,
        Self::BytesMut,
        Self::Blake2b,
        Self::BufferedWrite,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Vec => "vec",
            Self::BytesMut => "bytes_mut",
            Self::Blake2b => "blake2b256",
            Self::BufferedWrite => "buffered_write",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sink| sink.label() == value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Send,
    Receive,
    New,
    Expr,
    Match,
    GUnforgeable,
    Connective,
    Bundle,
    If,
}

impl Family {
    const ALL: [Self; 9] = [
        Self::Send,
        Self::Receive,
        Self::New,
        Self::Expr,
        Self::Match,
        Self::GUnforgeable,
        Self::Connective,
        Self::Bundle,
        Self::If,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::Receive => "receive",
            Self::New => "new",
            Self::Expr => "expr",
            Self::Match => "match",
            Self::GUnforgeable => "g_unforgeable",
            Self::Connective => "connective",
            Self::Bundle => "bundle",
            Self::If => "if",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.label() == value)
    }
}

impl Arm {
    const ALL: [Self; 5] = [
        Self::Direct,
        Self::ColdCached,
        Self::WarmCached,
        Self::ColdAdaptive,
        Self::WarmAdaptive,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::ColdCached => "cold_cached",
            Self::WarmCached => "warm_cached",
            Self::ColdAdaptive => "cold_adaptive",
            Self::WarmAdaptive => "warm_adaptive",
        }
    }

    fn parse(value: &str) -> Option<Self> { Self::ALL.into_iter().find(|arm| arm.label() == value) }
}

struct Fixture {
    family: &'static str,
    depth: usize,
    base: Par,
    expected: Vec<u8>,
    expected_hash: [u8; 32],
}

impl Fixture {
    fn new(width: usize, body_bytes: usize, multiplicity: u64) -> Self {
        assert!(WIDTHS.contains(&width), "width must be preregistered");
        assert!(
            BODY_BYTES.contains(&body_bytes),
            "body size must be preregistered"
        );
        assert!(
            MULTIPLICITIES.contains(&multiplicity),
            "multiplicity must be preregistered"
        );
        let multiplicity = Multiplicity::new(multiplicity).expect("positive multiplicity");
        let payload_bytes = byte_array_payload_len(body_bytes);
        let mut base = ParBag::new();
        for ordinal in 0..width {
            let value = byte_array_expr(payload_bytes, ordinal);
            assert_eq!(protobuf_encoder::encoded_len(&value), body_bytes);
            base.insert_many(&value, multiplicity)
                .expect("fixture cardinality fits u64");
        }
        assert_eq!(base.distinct_len(), width);
        Self::from_par(
            "expr_bytes",
            0,
            Par::from_parts(base, vec![0x05, 0x80], true),
        )
    }

    fn new_family(family: Family, width: usize, multiplicity: u64) -> Self {
        assert!(WIDTHS.contains(&width), "width must be preregistered");
        assert!(
            MULTIPLICITIES.contains(&multiplicity),
            "multiplicity must be preregistered"
        );
        let multiplicity = Multiplicity::new(multiplicity).expect("positive multiplicity");
        let mut components = ParBag::new();
        for ordinal in 0..width {
            insert_family_occurrence(&mut components, family, ordinal, multiplicity);
        }
        assert_eq!(components.distinct_len(), width);
        Self::from_par(
            family.label(),
            0,
            Par::from_parts(components, vec![0x05, 0x80], true),
        )
    }

    fn new_deep(depth: usize, multiplicity: u64) -> Self {
        assert!(DEPTHS.contains(&depth), "depth must be preregistered");
        assert!(
            MULTIPLICITIES.contains(&multiplicity),
            "multiplicity must be preregistered"
        );
        let mut base = leaf(0);
        for _ in 0..depth {
            base = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![base],
                    ..EList::default()
                })),
            }]);
        }
        base = Par::from_parts(
            base.components()
                .scale(multiplicity)
                .expect("deep fixture multiplicity fits"),
            vec![0x05, 0x80],
            true,
        );
        Self::from_par("deep_elist", depth, base)
    }

    fn from_par(family: &'static str, depth: usize, base: Par) -> Self {
        let expected = base.protobuf_bytes().expect("direct fixture authority");
        let expected_hash = Blake2b::<U32>::digest(&expected).into();
        let fixture = Self {
            family,
            depth,
            base,
            expected,
            expected_hash,
        };
        fixture.preflight();
        fixture
    }

    fn fresh(&self) -> Par {
        Par::from_parts(
            ParBag::try_from_bag(self.base.components().as_bag().clone())
                .expect("internally constructed fixture remains canonical"),
            self.base.locally_free().to_vec(),
            self.base.connective_used(),
        )
    }

    fn prepared(&self, arm: Arm) -> Par {
        let bag = self.fresh();
        assert_eq!(
            bag.components()
                .protobuf_projection_cache_state_experiment(),
            (false, 0),
            "fresh imported roots must not carry a projection"
        );
        if matches!(arm, Arm::WarmCached | Arm::WarmAdaptive) {
            let retained_bytes = bag
                .components()
                .protobuf_components_encoded_len()
                .expect("component projection length");
            bag.prepare_protobuf_projection(retained_bytes)
                .expect("prewarm cached projection");
            assert_eq!(
                bag.components()
                    .protobuf_projection_cache_state_experiment(),
                (
                    true,
                    bag.components()
                        .protobuf_components_encoded_len()
                        .expect("component length")
                )
            );
        }
        bag
    }

    fn visit<E>(
        &self,
        arm: Arm,
        bag: &Par,
        visitor: impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), ParProtobufSinkError<E>> {
        match arm {
            Arm::Direct => bag.try_visit_protobuf_chunks_direct(visitor),
            Arm::ColdCached => {
                let retained_bytes = bag
                    .components()
                    .protobuf_components_encoded_len()
                    .expect("component projection length");
                bag.prepare_protobuf_projection(retained_bytes)
                    .expect("cold reusable projection");
                bag.try_visit_protobuf_chunks(visitor)
            }
            Arm::WarmCached | Arm::ColdAdaptive | Arm::WarmAdaptive => {
                bag.try_visit_protobuf_chunks(visitor)
            }
        }
    }

    fn execute(&self, arm: Arm, bag: &Par, output: &mut Vec<u8>) {
        self.visit(arm, bag, |chunk| {
            output.extend_from_slice(chunk);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("fixture output is representable");
    }

    fn preflight(&self) {
        for arm in Arm::ALL {
            let bag = self.prepared(arm);
            let before = bag
                .components()
                .protobuf_projection_cache_state_experiment();
            let mut output = Vec::with_capacity(self.expected.len());
            self.execute(arm, &bag, &mut output);
            assert_eq!(output, self.expected, "{} changed bytes", arm.label());
            let after = bag
                .components()
                .protobuf_projection_cache_state_experiment();
            let retained = bag
                .components()
                .protobuf_components_encoded_len()
                .expect("component length");
            match arm {
                Arm::Direct | Arm::ColdAdaptive => assert_eq!(after, before),
                Arm::ColdCached => assert_eq!(after, (true, retained)),
                Arm::WarmCached | Arm::WarmAdaptive => assert_eq!(after, before),
            }
        }
    }
}

fn leaf(ordinal: usize) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(ordinal as i64)),
    }])
}

fn insert_family_occurrence(
    components: &mut ParBag,
    family: Family,
    ordinal: usize,
    multiplicity: Multiplicity,
) {
    match family {
        Family::Send => components.insert_many(
            &Send {
                chan: Some(leaf(ordinal)),
                ..Send::default()
            },
            multiplicity,
        ),
        Family::Receive => components.insert_many(
            &Receive {
                body: Some(leaf(ordinal)),
                ..Receive::default()
            },
            multiplicity,
        ),
        Family::New => components.insert_many(
            &New {
                p: Some(leaf(ordinal)),
                ..New::default()
            },
            multiplicity,
        ),
        Family::Expr => components.insert_many(
            &Expr {
                expr_instance: Some(ExprInstance::GInt(ordinal as i64)),
            },
            multiplicity,
        ),
        Family::Match => components.insert_many(
            &Match {
                target: Some(leaf(ordinal)),
                ..Match::default()
            },
            multiplicity,
        ),
        Family::GUnforgeable => components.insert_many(
            &GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: (ordinal as u64).to_be_bytes().to_vec(),
                })),
            },
            multiplicity,
        ),
        Family::Connective => components.insert_many(
            &Connective {
                connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                    ps: vec![leaf(ordinal)],
                })),
            },
            multiplicity,
        ),
        Family::Bundle => components.insert_many(
            &Bundle {
                body: Some(leaf(ordinal)),
                ..Bundle::default()
            },
            multiplicity,
        ),
        Family::If => components.insert_many(
            &If {
                condition: Some(leaf(ordinal)),
                if_true: Some(Par::default()),
                if_false: Some(Par::default()),
                ..If::default()
            },
            multiplicity,
        ),
    }
    .expect("fixture family cardinality fits u64");
}

fn byte_array_payload_len(body_bytes: usize) -> usize {
    let key_bytes = prost::encoding::key_len(25);
    (8..=body_bytes)
        .find(|payload_bytes| {
            key_bytes + prost::encoding::encoded_len_varint(*payload_bytes as u64) + payload_bytes
                == body_bytes
        })
        .unwrap_or_else(|| panic!("no GByteArray Expr has an exact {body_bytes}-byte body"))
}

fn byte_array_expr(payload_bytes: usize, ordinal: usize) -> Expr {
    let mut payload = vec![0xa5; payload_bytes];
    payload[payload_bytes - 8..].copy_from_slice(&(ordinal as u64).to_be_bytes());
    Expr {
        expr_instance: Some(ExprInstance::GByteArray(payload)),
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not an integer: {error}"))
        })
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a u64: {error}"))
        })
        .unwrap_or(default)
}

fn selected_usizes(name: &str, values: &[usize]) -> Vec<usize> {
    match std::env::var(name) {
        Ok(value) => {
            let selected = value
                .parse::<usize>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not an integer: {error}"));
            assert!(values.contains(&selected), "{name} is not preregistered");
            vec![selected]
        }
        Err(std::env::VarError::NotPresent) => values.to_vec(),
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn selected_u64s(name: &str, values: &[u64]) -> Vec<u64> {
    match std::env::var(name) {
        Ok(value) => {
            let selected = value
                .parse::<u64>()
                .unwrap_or_else(|error| panic!("{name}={value:?} is not a u64: {error}"));
            assert!(values.contains(&selected), "{name} is not preregistered");
            vec![selected]
        }
        Err(std::env::VarError::NotPresent) => values.to_vec(),
        Err(error) => panic!("{name} is not valid Unicode: {error}"),
    }
}

fn selected_sinks() -> Vec<SinkKind> {
    match std::env::var("PARBAG_PROTOBUF_SINK") {
        Ok(value) => vec![SinkKind::parse(&value)
            .unwrap_or_else(|| panic!("PARBAG_PROTOBUF_SINK={value:?} is not registered"))],
        Err(std::env::VarError::NotPresent) => SinkKind::ALL.to_vec(),
        Err(error) => panic!("PARBAG_PROTOBUF_SINK is not valid Unicode: {error}"),
    }
}

fn selected_profile_sink() -> SinkKind {
    match std::env::var("PARBAG_PROTOBUF_SINK") {
        Ok(value) => SinkKind::parse(&value)
            .unwrap_or_else(|| panic!("PARBAG_PROTOBUF_SINK={value:?} is not registered")),
        Err(std::env::VarError::NotPresent) => SinkKind::Vec,
        Err(error) => panic!("PARBAG_PROTOBUF_SINK is not valid Unicode: {error}"),
    }
}

struct DigestWriter {
    hasher: Blake2b<U32>,
    bytes: usize,
    digest: Option<[u8; 32]>,
}

impl DigestWriter {
    fn new() -> Self {
        Self {
            hasher: Blake2b::<U32>::new(),
            bytes: 0,
            digest: None,
        }
    }

    fn finish_digest(&mut self) { self.digest = Some(self.hasher.clone().finalize().into()) }
}

impl std::io::Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(bytes);
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

enum BenchSink {
    Vec(Vec<u8>),
    BytesMut(prost::bytes::BytesMut),
    Blake2b {
        hasher: Option<Blake2b<U32>>,
        digest: Option<[u8; 32]>,
    },
    BufferedWrite(std::io::BufWriter<DigestWriter>),
}

impl BenchSink {
    fn new(kind: SinkKind, output_bytes: usize) -> Self {
        match kind {
            SinkKind::Vec => Self::Vec(Vec::with_capacity(output_bytes)),
            SinkKind::BytesMut => {
                Self::BytesMut(prost::bytes::BytesMut::with_capacity(output_bytes))
            }
            SinkKind::Blake2b => Self::Blake2b {
                hasher: Some(Blake2b::<U32>::new()),
                digest: None,
            },
            SinkKind::BufferedWrite => Self::BufferedWrite(std::io::BufWriter::with_capacity(
                64 * 1024,
                DigestWriter::new(),
            )),
        }
    }

    fn write_chunk(&mut self, chunk: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Vec(output) => {
                output.extend_from_slice(chunk);
                Ok(())
            }
            Self::BytesMut(output) => {
                output.extend_from_slice(chunk);
                Ok(())
            }
            Self::Blake2b { hasher, .. } => {
                hasher
                    .as_mut()
                    .expect("hash sink cannot receive chunks after finalization")
                    .update(chunk);
                Ok(())
            }
            Self::BufferedWrite(output) => output.write_all(chunk),
        }
    }

    fn finish(&mut self) -> std::io::Result<()> {
        match self {
            Self::Vec(_) | Self::BytesMut(_) => Ok(()),
            Self::Blake2b { hasher, digest } => {
                *digest = Some(
                    hasher
                        .take()
                        .expect("hash sink is finalized exactly once")
                        .finalize()
                        .into(),
                );
                Ok(())
            }
            Self::BufferedWrite(output) => {
                output.flush()?;
                output.get_mut().finish_digest();
                Ok(())
            }
        }
    }

    fn verify(&self, fixture: &Fixture) {
        match self {
            Self::Vec(output) => assert_eq!(output, &fixture.expected),
            Self::BytesMut(output) => assert_eq!(output.as_ref(), fixture.expected.as_slice()),
            Self::Blake2b { digest, .. } => assert_eq!(*digest, Some(fixture.expected_hash)),
            Self::BufferedWrite(output) => {
                assert_eq!(output.get_ref().bytes, fixture.expected.len());
                assert_eq!(output.get_ref().digest, Some(fixture.expected_hash));
            }
        }
    }

    fn checksum(&self) -> u64 {
        match self {
            Self::Vec(output) => {
                output.len() as u64 ^ u64::from(output.first().copied().unwrap_or(0))
            }
            Self::BytesMut(output) => {
                output.len() as u64 ^ u64::from(output.first().copied().unwrap_or(0))
            }
            Self::Blake2b { digest, .. } => u64::from_be_bytes(
                digest.expect("finalized hash sink")[..8]
                    .try_into()
                    .unwrap(),
            ),
            Self::BufferedWrite(output) => u64::from_be_bytes(
                output.get_ref().digest.expect("finalized writer sink")[..8]
                    .try_into()
                    .unwrap(),
            ),
        }
    }
}

struct Observation {
    elapsed_ns: u128,
    allocations: u64,
    allocated_bytes: u64,
    pathmap_traversals: u64,
    sink_input_bytes: usize,
    newly_retained_projection_bytes: usize,
    retained_projection_bytes: usize,
}

fn measure(fixture: &Fixture, arm: Arm, sink_kind: SinkKind) -> Observation {
    let bag = fixture.prepared(arm);
    let cache_before = bag
        .components()
        .protobuf_projection_cache_state_experiment();
    let mut output = BenchSink::new(sink_kind, fixture.expected.len());
    ParBag::reset_protobuf_pathmap_traversals_experiment();
    ALLOC_CALLS.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    let started = Instant::now();
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
    fixture
        .visit(arm, black_box(&bag), |chunk| {
            black_box(&mut output).write_chunk(black_box(chunk))
        })
        .expect("benchmark sink accepts the complete protobuf stream");
    output.finish().expect("benchmark sink finalization");
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let elapsed = started.elapsed().as_nanos();
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
    output.verify(fixture);
    let cache_after = bag
        .components()
        .protobuf_projection_cache_state_experiment();
    let pathmap_traversals = ParBag::take_protobuf_pathmap_traversals_experiment();
    let prepared_during_measurement = !cache_before.0 && cache_after.0;
    Observation {
        elapsed_ns: elapsed,
        allocations: ALLOC_CALLS.load(Ordering::Relaxed),
        allocated_bytes: ALLOC_BYTES.load(Ordering::Relaxed),
        pathmap_traversals,
        sink_input_bytes: fixture.expected.len(),
        newly_retained_projection_bytes: if prepared_during_measurement {
            cache_after.1
        } else {
            0
        },
        retained_projection_bytes: cache_after.1,
    }
}

fn print_metadata(mode: &str, reps: usize, warmup: usize) {
    println!(
        "META\tformat=parbag-protobuf-projection-v5\tmode={mode}\treps={reps}\twarmup={warmup}\twidths=1,16,256,4096\tbody_bytes=16,256,4096\tmultiplicities=1,2,16\tdepths=16,256,4096\tfamilies=send,receive,new,expr,match,g_unforgeable,connective,bundle,if\tsinks=vec,bytes_mut,blake2b256,buffered_write\texpr_payload_topology=shared-prefix-last-8-byte-ordinal\tordering=balanced-five-arm-rotation\tbyte_metrics=sink-input-exact,new-retained-projection-exact"
    );
    println!(
        "HEADER\tfamily\twidth\tbody_bytes\tdepth\tmultiplicity\toutput_bytes\tarm\tsink\tsample\tposition\telapsed_ns\tallocations\tallocated_bytes\tpathmap_traversals\tsink_input_bytes\tnewly_retained_projection_bytes\tretained_projection_bytes"
    );
    if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
        println!("LOADAVG\t{}", loadavg.trim());
    }
}

/// Production-consumer anti-vacuity control. These entry points are one-shot
/// or use a different canonical identity; none may silently populate the
/// protobuf projection of a cold authoritative ParBag.
fn verify_cold_consumer_controls() {
    let fixture = Fixture::new(1, 16, 1);
    let value = fixture.fresh();
    let cold = || {
        assert_eq!(
            value
                .components()
                .protobuf_projection_cache_state_experiment(),
            (false, 0)
        );
    };
    cold();
    let _channel_image = ChannelImageV2::from_par(&value).expect("resident RCI2 control");
    cold();
    let _bincode = bincode::serialize(&value).expect("resident bincode control");
    cold();
    let _prost = value.encode_to_vec();
    cold();
    let _generated = protobuf_encoder::encode_to_vec(&value);
    cold();
    let mut digest = Blake2b::<U32>::new();
    value
        .try_visit_protobuf_chunks(|chunk| {
            digest.update(chunk);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("one-shot hash control");
    let digest: [u8; 32] = digest.finalize().into();
    assert_eq!(digest, fixture.expected_hash);
    cold();
}

fn run_fixture(
    fixture: &Fixture,
    width: usize,
    body_bytes: usize,
    multiplicity: u64,
    reps: usize,
    warmup: usize,
    sinks: &[SinkKind],
) {
    for sink in sinks {
        for repetition in 0..(warmup + reps) {
            // Rotate the first arm to counter first-in-batch thermal and
            // scheduler effects without changing the prepared fixture.
            for slot in 0..Arm::ALL.len() {
                let arm = Arm::ALL[(repetition + slot) % Arm::ALL.len()];
                let observation = measure(fixture, arm, *sink);
                if repetition >= warmup {
                    println!(
                        "OBS\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        fixture.family,
                        width,
                        body_bytes,
                        fixture.depth,
                        multiplicity,
                        fixture.expected.len(),
                        arm.label(),
                        sink.label(),
                        repetition - warmup,
                        slot,
                        observation.elapsed_ns,
                        observation.allocations,
                        observation.allocated_bytes,
                        observation.pathmap_traversals,
                        observation.sink_input_bytes,
                        observation.newly_retained_projection_bytes,
                        observation.retained_projection_bytes,
                    );
                }
            }
        }
    }
}

fn run_matrix() {
    let reps = env_usize("PARBAG_PROTOBUF_REPS", DEFAULT_REPS);
    let warmup = env_usize("PARBAG_PROTOBUF_WARMUP", DEFAULT_WARMUP);
    let sinks = selected_sinks();
    assert!(reps != 0, "PARBAG_PROTOBUF_REPS must be nonzero");
    print_metadata("matrix", reps, warmup);
    for width in selected_usizes("PARBAG_PROTOBUF_WIDTH", &WIDTHS) {
        for body_bytes in selected_usizes("PARBAG_PROTOBUF_BODY_BYTES", &BODY_BYTES) {
            for multiplicity in selected_u64s("PARBAG_PROTOBUF_MULTIPLICITY", &MULTIPLICITIES) {
                let fixture = Fixture::new(width, body_bytes, multiplicity);
                run_fixture(
                    &fixture,
                    width,
                    body_bytes,
                    multiplicity,
                    reps,
                    warmup,
                    &sinks,
                );
            }
        }
    }
}

fn run_family_matrix(families: &[Family]) {
    let reps = env_usize("PARBAG_PROTOBUF_REPS", DEFAULT_REPS);
    let warmup = env_usize("PARBAG_PROTOBUF_WARMUP", DEFAULT_WARMUP);
    let sinks = selected_sinks();
    assert!(reps != 0, "PARBAG_PROTOBUF_REPS must be nonzero");
    print_metadata("family_matrix", reps, warmup);
    for family in families {
        for width in selected_usizes("PARBAG_PROTOBUF_WIDTH", &WIDTHS) {
            for multiplicity in selected_u64s("PARBAG_PROTOBUF_MULTIPLICITY", &MULTIPLICITIES) {
                let fixture = Fixture::new_family(*family, width, multiplicity);
                run_fixture(&fixture, width, 0, multiplicity, reps, warmup, &sinks);
            }
        }
    }
}

fn run_deep_matrix() {
    let reps = env_usize("PARBAG_PROTOBUF_REPS", DEFAULT_REPS);
    let warmup = env_usize("PARBAG_PROTOBUF_WARMUP", DEFAULT_WARMUP);
    let sinks = selected_sinks();
    assert!(reps != 0, "PARBAG_PROTOBUF_REPS must be nonzero");
    print_metadata("deep_matrix", reps, warmup);
    for depth in selected_usizes("PARBAG_PROTOBUF_DEPTH", &DEPTHS) {
        for multiplicity in selected_u64s("PARBAG_PROTOBUF_MULTIPLICITY", &MULTIPLICITIES) {
            let fixture = Fixture::new_deep(depth, multiplicity);
            run_fixture(&fixture, 1, 0, multiplicity, reps, warmup, &sinks);
        }
    }
}

fn run_profile(arm: Arm) {
    let width = env_usize("PARBAG_PROTOBUF_WIDTH", 256);
    let body_bytes = env_usize("PARBAG_PROTOBUF_BODY_BYTES", 256);
    let multiplicity = env_u64("PARBAG_PROTOBUF_MULTIPLICITY", 1);
    let iterations = env_usize("PARBAG_PROTOBUF_ITERATIONS", DEFAULT_PROFILE_ITERATIONS);
    let sink_kind = selected_profile_sink();
    assert!(WIDTHS.contains(&width));
    assert!(BODY_BYTES.contains(&body_bytes));
    assert!(MULTIPLICITIES.contains(&multiplicity));
    assert!(iterations != 0);
    print_metadata("profile", iterations, 0);
    let fixture = match std::env::var("PARBAG_PROTOBUF_DEPTH") {
        Ok(value) => {
            let depth = value.parse::<usize>().unwrap_or_else(|error| {
                panic!("PARBAG_PROTOBUF_DEPTH={value:?} is not an integer: {error}")
            });
            Fixture::new_deep(depth, multiplicity)
        }
        Err(std::env::VarError::NotPresent) => Fixture::new(width, body_bytes, multiplicity),
        Err(error) => panic!("PARBAG_PROTOBUF_DEPTH is not valid Unicode: {error}"),
    };
    let shared_bag = fixture.prepared(arm);

    let mut control = BenchSink::new(sink_kind, fixture.expected.len());
    let cold_control = matches!(arm, Arm::ColdCached).then(|| fixture.prepared(arm));
    let control_bag = cold_control.as_ref().unwrap_or(&shared_bag);
    fixture
        .visit(arm, control_bag, |chunk| control.write_chunk(chunk))
        .expect("profile control sink accepts the stream");
    control.finish().expect("profile control sink finalization");
    control.verify(&fixture);

    let mut checksum = 0u64;
    for _ in 0..iterations {
        let cold_bag = matches!(arm, Arm::ColdCached).then(|| fixture.prepared(arm));
        let bag = cold_bag.as_ref().unwrap_or(&shared_bag);
        let mut output = BenchSink::new(sink_kind, fixture.expected.len());
        fixture
            .visit(arm, black_box(bag), |chunk| {
                black_box(&mut output).write_chunk(black_box(chunk))
            })
            .expect("profile sink accepts the stream");
        output.finish().expect("profile sink finalization");
        checksum = checksum.rotate_left(7) ^ black_box(output.checksum());
    }
    let (reported_width, reported_body_bytes) = if fixture.depth == 0 {
        (width, body_bytes)
    } else {
        (1, 0)
    };
    println!(
        "PROFILE\tarm={}\tsink={}\twidth={}\tbody_bytes={}\tdepth={}\tmultiplicity={}\titerations={}\toutput_bytes={}\tchecksum={checksum:016x}",
        arm.label(),
        sink_kind.label(),
        reported_width,
        reported_body_bytes,
        fixture.depth,
        multiplicity,
        iterations,
        fixture.expected.len()
    );
}

fn main() {
    verify_cold_consumer_controls();
    match std::env::var("PARBAG_PROTOBUF_ARM") {
        Ok(value) => run_profile(
            Arm::parse(&value)
                .unwrap_or_else(|| panic!("PARBAG_PROTOBUF_ARM={value:?} is not registered")),
        ),
        Err(std::env::VarError::NotPresent) => match std::env::var("PARBAG_PROTOBUF_TOPOLOGY") {
            Ok(value) if value == "deep" => run_deep_matrix(),
            Ok(value) => {
                panic!("PARBAG_PROTOBUF_TOPOLOGY={value:?} is not registered")
            }
            Err(std::env::VarError::NotPresent) => match std::env::var("PARBAG_PROTOBUF_FAMILY") {
                Ok(value) if value == "all" => run_family_matrix(&Family::ALL),
                Ok(value) => run_family_matrix(&[Family::parse(&value).unwrap_or_else(|| {
                    panic!("PARBAG_PROTOBUF_FAMILY={value:?} is not registered")
                })]),
                Err(std::env::VarError::NotPresent) => run_matrix(),
                Err(error) => {
                    panic!("PARBAG_PROTOBUF_FAMILY is not valid Unicode: {error}")
                }
            },
            Err(error) => {
                panic!("PARBAG_PROTOBUF_TOPOLOGY is not valid Unicode: {error}")
            }
        },
        Err(error) => panic!("PARBAG_PROTOBUF_ARM is not valid Unicode: {error}"),
    }
}
