#[path = "paired.rs"]
mod paired;

use std::ops::Range;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::epathmap_trie_codec::{
    canonical_map_value_body_at_ordered_keys, encode, EPathMapRepr,
};
use models::rust::rholang::protobuf_encoder;
use paired::{loadavg, measure_arms, REPS, WARMUP};
use pathmap::PathMap;
use rspace_plus_plus::rspace::pathmap_codec::CanonicalCompactTrieCursor;

const IMAGE_SIZES: [(usize, usize); 3] = [(256, 4), (4_096, 4), (16_384, 5)];
const IMAGE_ENTRIES_PER_PASS: usize = 1 << 20;
const PROFILE_ARM_VARIABLE: &str = "EPM1_LOOKUP_ARM";
const PROFILE_CASE_VARIABLE: &str = "EPM1_LOOKUP_CASE";
const PROFILE_ITERATIONS_VARIABLE: &str = "EPM1_LOOKUP_ITERATIONS";

type Arm = (&'static str, fn(&LookupCase<'_>) -> usize);

const ARMS: [Arm; 2] = [("lookup", whole_lookup), ("validation", validation_only)];

#[derive(Clone, Copy)]
enum LookupKind {
    FirstHit,
    LastHit,
    Miss,
}

impl LookupKind {
    const ALL: [LookupKind; 3] = [LookupKind::FirstHit, LookupKind::LastHit, LookupKind::Miss];

    fn label(self) -> &'static str {
        match self {
            LookupKind::FirstHit => "first-hit",
            LookupKind::LastHit => "last-hit",
            LookupKind::Miss => "miss",
        }
    }

    fn key_index(self, entries: usize) -> usize {
        match self {
            LookupKind::FirstHit => 0,
            LookupKind::LastHit => entries - 1,
            LookupKind::Miss => entries,
        }
    }

    fn found_index(self, entries: usize) -> Option<usize> {
        match self {
            LookupKind::FirstHit | LookupKind::LastHit => Some(self.key_index(entries)),
            LookupKind::Miss => None,
        }
    }

    fn walk_calls(self, entries: usize) -> usize {
        match self {
            LookupKind::FirstHit => 1,
            LookupKind::LastHit => entries,
            LookupKind::Miss => entries + 1,
        }
    }
}

struct LookupImage {
    entries: usize,
    width: usize,
    bytes: Vec<u8>,
    arena: Range<usize>,
}

impl LookupImage {
    fn build(entries: usize, width: usize) -> Self {
        let mut map: PathMap<Par> = PathMap::new();
        for index in 0..entries {
            map.insert(image_key(index, width), integer_par(index));
        }
        let bytes = encode(&EPathMapRepr::Map(map));
        let arena = act_arena_range(&bytes);
        Self {
            entries,
            width,
            bytes,
            arena,
        }
    }

    fn arena(&self) -> &[u8] { &self.bytes[self.arena.clone()] }

    fn verify(&self) {
        let mut cursor = CanonicalCompactTrieCursor::new(self.arena())
            .expect("the benchmark-local EPM1 prefix parse locates a canonical ACT image");
        assert_eq!(
            cursor.value_summary().value_count(),
            self.entries,
            "the ACT image declares one value per image entry"
        );
        let mut walked = 0usize;
        while let Some(endpoint) = cursor.next_value_endpoint() {
            assert_eq!(
                endpoint.path,
                image_key(walked, self.width).as_bytes(),
                "the ACT walk yields the image keys in ascending order"
            );
            walked += 1;
        }
        assert_eq!(
            walked, self.entries,
            "the ACT walk yields every image key once"
        );
    }
}

struct LookupCase<'i> {
    label: String,
    entries: usize,
    kind: LookupKind,
    image: &'i [u8],
    arena: &'i [u8],
    key: Vec<u8>,
    expected_body: Option<Vec<u8>>,
    calls_per_pass: usize,
}

impl<'i> LookupCase<'i> {
    fn new(image: &'i LookupImage, kind: LookupKind) -> Self {
        Self {
            label: format!("{}-{}", kind.label(), image.entries),
            entries: image.entries,
            kind,
            image: &image.bytes,
            arena: image.arena(),
            key: image_key(kind.key_index(image.entries), image.width).into_bytes(),
            expected_body: kind
                .found_index(image.entries)
                .map(|index| protobuf_encoder::encode_to_vec(&integer_par(index))),
            calls_per_pass: IMAGE_ENTRIES_PER_PASS / image.entries,
        }
    }

    fn verify(&self) {
        let found = canonical_map_value_body_at_ordered_keys(self.image, &[self.key.as_slice()])
            .expect("the benchmark image is a canonical EPM1 map snapshot");
        assert_eq!(
            found,
            self.expected_body.as_deref(),
            "case {}: the lookup returns the body of the requested key",
            self.label
        );
    }
}

fn image_key(index: usize, width: usize) -> String { format!("same/prefix/{index:0width$}") }

fn integer_par(index: usize) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(
            i64::try_from(index).expect("an image index fits an i64"),
        )),
    }])
}

fn act_arena_range(bytes: &[u8]) -> Range<usize> {
    match bytes {
        [b'E', b'P', b'M', b'1', 1, _mode, rest @ ..] => {
            let mut cursor = rest;
            let length = prost::encoding::decode_varint(&mut cursor)
                .expect("an EPM1 image carries its ACT length as a varint");
            let start = bytes.len() - cursor.len();
            let end = start + usize::try_from(length).expect("the ACT length fits a usize");
            assert!(
                end <= bytes.len(),
                "the ACT image lies inside the EPM1 image"
            );
            start..end
        }
        _ => panic!("the benchmark image is not an EPM1 version 1 snapshot"),
    }
}

fn whole_lookup(case: &LookupCase<'_>) -> usize {
    canonical_map_value_body_at_ordered_keys(case.image, &[case.key.as_slice()])
        .expect("the benchmark image is a canonical EPM1 map snapshot")
        .map_or(0, <[u8]>::len)
}

fn validation_only(case: &LookupCase<'_>) -> usize {
    CanonicalCompactTrieCursor::new(case.arena)
        .expect("the benchmark arena is a canonical ACT image")
        .value_summary()
        .value_count()
}

fn lookup_cases(images: &[LookupImage]) -> Vec<LookupCase<'_>> {
    let mut cases = Vec::with_capacity(images.len() * LookupKind::ALL.len());
    for image in images {
        for kind in LookupKind::ALL {
            cases.push(LookupCase::new(image, kind));
        }
    }
    cases
}

fn measure_case<'c, 'i, const N: usize>(
    case: &'c LookupCase<'i>,
    arms: [Arm; N],
) -> paired::Arms<N> {
    let workload = vec![case; case.calls_per_pass];
    let labels = arms.map(|(label, _)| label);
    let mut calls = arms.map(|(_, arm)| move |item: &&'c LookupCase<'i>| arm(item));
    let mut dispatch = calls
        .each_mut()
        .map(|call| call as &mut dyn FnMut(&&'c LookupCase<'i>) -> usize);
    measure_arms(labels, &workload, &mut dispatch, &mut |products| {
        for product in products {
            std::hint::black_box(product);
        }
    })
}

fn environment(cases: &[LookupCase<'_>]) {
    println!("ENVIRONMENT");
    println!("  loadavg {}", loadavg());
    println!("  profile {}", match cfg!(debug_assertions) {
        true => "debug",
        false => "release",
    });
    println!("  repetitions {REPS} retained after {WARMUP} warm-up, every arm timed in each");
    println!("  arms {}", ARMS.map(|(label, _)| label).join(" "));
    println!(
        "  cases {}",
        cases
            .iter()
            .map(|case| case.label.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!();
}

fn measure(cases: &[LookupCase<'_>]) {
    environment(cases);
    for case in cases {
        println!(
            "CASE {} entries {} kind {} image bytes {} arena bytes {} walk calls {} calls per pass {}",
            case.label,
            case.entries,
            case.kind.label(),
            case.image.len(),
            case.arena.len(),
            case.kind.walk_calls(case.entries),
            case.calls_per_pass
        );
        let arms = measure_case(case, ARMS);
        arms.print();
        arms.report(&format!("{} lookup over validation", case.label), 0, 1);
        println!(
            "SHARE\t{}\t{:.6}",
            case.label,
            arms.pair(0, 1).median_ratio()
        );
        for (label, sample) in arms.labels.iter().zip(&arms.samples) {
            for (repetition, pass_ns) in sample.times_ns.iter().enumerate() {
                println!(
                    "RAW\t{}\t{}\t{}\t{repetition}\t{label}\t{pass_ns:.0}",
                    case.label, case.entries, case.calls_per_pass
                );
            }
        }
        println!();
    }
}

fn smoke(cases: &[LookupCase<'_>]) {
    for case in cases {
        for (label, arm) in ARMS {
            let product = arm(case);
            println!("smoke {} {label} product {product}", case.label);
        }
    }
    println!(
        "smoke: {} cases, {} arm calls, no time measured",
        cases.len(),
        cases.len() * ARMS.len()
    );
}

fn profile_one_arm(cases: &[LookupCase<'_>], arm_label: &str) {
    let case_label = std::env::var(PROFILE_CASE_VARIABLE)
        .expect("EPM1_LOOKUP_CASE names the case to profile, for example miss-4096");
    let iterations: usize = std::env::var(PROFILE_ITERATIONS_VARIABLE)
        .expect("EPM1_LOOKUP_ITERATIONS gives the number of calls to profile")
        .parse()
        .expect("EPM1_LOOKUP_ITERATIONS is a decimal count");
    let case = cases
        .iter()
        .find(|case| case.label == case_label)
        .expect("EPM1_LOOKUP_CASE names one of the nine cases");
    let (_, arm) = ARMS
        .into_iter()
        .find(|(label, _)| *label == arm_label)
        .expect("EPM1_LOOKUP_ARM names an arm: lookup or validation");
    let mut checksum = 0usize;
    for _ in 0..iterations {
        checksum = checksum.wrapping_add(arm(std::hint::black_box(case)));
    }
    println!("profile {arm_label} {case_label} iterations {iterations} checksum {checksum}");
}

fn main() {
    let images = IMAGE_SIZES.map(|(entries, width)| LookupImage::build(entries, width));
    for image in &images {
        image.verify();
    }
    let cases = lookup_cases(&images);
    for case in &cases {
        case.verify();
    }
    match std::env::var(PROFILE_ARM_VARIABLE) {
        Ok(arm_label) => profile_one_arm(&cases, &arm_label),
        Err(std::env::VarError::NotPresent) => {
            match std::env::args().any(|argument| argument == "--test") {
                true => smoke(&cases),
                false => measure(&cases),
            }
        }
        Err(error) => panic!("EPM1_LOOKUP_ARM is not valid unicode: {error}"),
    }
}
