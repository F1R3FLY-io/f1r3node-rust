use std::hint::black_box;

use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::utils::{new_elist_par, new_etuple_par, new_gint_par, new_gstring_par};

const POOL_SEED: u64 = 0x0487_0004;
const NAME_ID_LEN: usize = 32;
const SMALL_COLD_POOL_LEN: usize = 16_384;
const SMALL_WARM_POOL_LEN: usize = 1_024;
const LARGE_COLD_POOL_LEN: usize = 1_024;
const LARGE_WARM_POOL_LEN: usize = 64;
const FNV_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
const SHORT_STRINGS: [&str; 16] = [
    "ack", "add", "balance", "deposit", "get", "init", "lookup", "nonce", "owner", "purse", "read",
    "result", "set", "status", "transfer", "value",
];
const CELL_KINDS: [CellKind; 3] = [CellKind::Unforgeable, CellKind::Tuple, CellKind::Large];
const TEMPERATURES: [Temperature; 2] = [Temperature::Cold, Temperature::Warm];

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    const fn new(seed: u64) -> Self { Self { state: seed } }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    fn name_id(&mut self) -> [u8; NAME_ID_LEN] {
        let mut output = [0u8; NAME_ID_LEN];
        for chunk in output.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
        output
    }

    fn below(&mut self, bound: u64) -> u64 { self.next_u64() % bound }
}

#[derive(Clone, Copy)]
enum Arm {
    Baseline,
    Encode,
    Digest,
}

impl Arm {
    fn parse(text: &str) -> Self {
        match text {
            "none" => Self::Baseline,
            "encode" => Self::Encode,
            "digest" => Self::Digest,
            other => panic!("{other:?} is not an arm; use none, encode or digest"),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Baseline => "none",
            Self::Encode => "encode",
            Self::Digest => "digest",
        }
    }

    fn construct(self) -> fn(&Par) -> usize {
        match self {
            Self::Baseline => construct_none,
            Self::Encode => construct_encode,
            Self::Digest => construct_digest,
        }
    }
}

#[derive(Clone, Copy)]
enum CellKind {
    Unforgeable,
    Tuple,
    Large,
}

impl CellKind {
    fn parse(text: &str) -> Self {
        match text {
            "unforgeable" => Self::Unforgeable,
            "tuple" => Self::Tuple,
            "large" => Self::Large,
            other => panic!("{other:?} is not a cell kind; use unforgeable, tuple or large"),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Unforgeable => "unforgeable",
            Self::Tuple => "tuple",
            Self::Large => "large",
        }
    }
}

#[derive(Clone, Copy)]
enum Temperature {
    Cold,
    Warm,
}

impl Temperature {
    fn parse(text: &str) -> Self {
        match text {
            "cold" => Self::Cold,
            "warm" => Self::Warm,
            other => panic!("{other:?} is not a temperature; use cold or warm"),
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
        }
    }
}

fn gprivate_par(id: &[u8]) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: id.to_vec() })),
    }])
}

fn gstring_par(text: &str) -> Par { new_gstring_par(text.to_string(), Vec::new(), false) }

fn tuple_name_string_par(id: &[u8], text: &str) -> Par {
    new_etuple_par(vec![gprivate_par(id), gstring_par(text)])
}

fn large_compound_par(generator: &mut SplitMix64) -> Par {
    let mut elements = Vec::with_capacity(SHORT_STRINGS.len());
    for (index, text) in SHORT_STRINGS.iter().enumerate() {
        let id = generator.name_id();
        let ordinal = i64::try_from(index).expect("a short-string index fits in i64");
        elements.push(new_etuple_par(vec![
            gprivate_par(&id),
            gstring_par(text),
            new_gint_par(ordinal, Vec::new(), false),
        ]));
    }
    new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
}

const fn pool_len(kind: CellKind, temperature: Temperature) -> usize {
    match (kind, temperature) {
        (CellKind::Large, Temperature::Cold) => LARGE_COLD_POOL_LEN,
        (CellKind::Large, Temperature::Warm) => LARGE_WARM_POOL_LEN,
        (_, Temperature::Cold) => SMALL_COLD_POOL_LEN,
        (_, Temperature::Warm) => SMALL_WARM_POOL_LEN,
    }
}

fn build_pool(kind: CellKind, temperature: Temperature) -> Vec<Par> {
    let len = pool_len(kind, temperature);
    let mut pool = Vec::with_capacity(len);
    let mut generator = SplitMix64::new(POOL_SEED);
    for _ in 0..len {
        pool.push(match kind {
            CellKind::Unforgeable => gprivate_par(&generator.name_id()),
            CellKind::Tuple => {
                let id = generator.name_id();
                let text_index = usize::try_from(generator.below(SHORT_STRINGS.len() as u64))
                    .expect("a short-string index is below 16");
                tuple_name_string_par(&id, SHORT_STRINGS[text_index])
            }
            CellKind::Large => large_compound_par(&mut generator),
        });
    }
    pool
}

fn construct_none(par: &Par) -> usize {
    black_box(par);
    0
}

fn construct_encode(par: &Par) -> usize { black_box(encode_trie_path(black_box(par))).len() }

fn construct_digest(par: &Par) -> usize {
    let path = encode_trie_path(black_box(par));
    let mut digest = FNV_OFFSET_BASIS;
    for byte in &path {
        digest = (digest ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
    }
    black_box(digest as usize)
}

fn run_cell(arm: Arm, kind: CellKind, temperature: Temperature, iterations: usize) {
    let pool = build_pool(kind, temperature);
    let construct = black_box(arm.construct());
    let mut warmup_checksum = 0usize;
    match temperature {
        Temperature::Cold => assert!(
            iterations <= pool.len(),
            "a cold run keys each pre-built channel at most once"
        ),
        Temperature::Warm => {
            assert_eq!(
                iterations % pool.len(),
                0,
                "a warm run cycles the pool a whole number of times"
            );
            for par in &pool {
                warmup_checksum = warmup_checksum.wrapping_add(construct(par));
            }
        }
    }
    let mut checksum = 0usize;
    for iteration in 0..iterations {
        checksum = checksum.wrapping_add(construct(&pool[iteration % pool.len()]));
    }
    println!(
        "arm={} kind={} temperature={} iterations={iterations} pool_len={} \
         warmup_checksum={warmup_checksum} checksum={checksum}",
        arm.label(),
        kind.label(),
        temperature.label(),
        pool.len(),
    );
}

fn main() {
    let selection = [
        std::env::var("ENCODE_TRIE_PATH_ARM"),
        std::env::var("ENCODE_TRIE_PATH_KIND"),
        std::env::var("ENCODE_TRIE_PATH_TEMPERATURE"),
        std::env::var("ENCODE_TRIE_PATH_ITERATIONS"),
    ];
    match selection {
        [Ok(arm), Ok(kind), Ok(temperature), Ok(iterations)] => run_cell(
            Arm::parse(&arm),
            CellKind::parse(&kind),
            Temperature::parse(&temperature),
            iterations
                .parse::<usize>()
                .expect("ENCODE_TRIE_PATH_ITERATIONS is a non-negative integer"),
        ),
        [Err(_), Err(_), Err(_), Err(_)] => {
            for kind in CELL_KINDS {
                for temperature in TEMPERATURES {
                    run_cell(Arm::Digest, kind, temperature, pool_len(kind, temperature));
                }
            }
        }
        _ => panic!(
            "set all of ENCODE_TRIE_PATH_ARM, ENCODE_TRIE_PATH_KIND, \
             ENCODE_TRIE_PATH_TEMPERATURE and ENCODE_TRIE_PATH_ITERATIONS, or none of them"
        ),
    }
}
