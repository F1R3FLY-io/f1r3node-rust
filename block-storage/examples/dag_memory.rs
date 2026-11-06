use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{Bond, Justification};
use models::rust::validator::Validator;

#[path = "support/counting_fixture.rs"]
mod counting_fixture;
#[path = "support/backend.rs"]
mod backend;
use counting_fixture::{hash, manager, reset_peaks, usable_json, LIVE, PEAK};

fn sample(
    storage: &BlockDagKeyValueStorage,
    window: i64,
    stage: &str,
    blocks: usize,
    elapsed: f64,
) {
    let dag = storage.get_representation().unwrap();
    let (resident, minimum, edges) = backend::resident(&dag);
    let height_end_exclusive = dag.get_max_height();
    let height = height_end_exclusive.saturating_sub(1);
    let lfb = dag
        .block_number_unsafe(&dag.last_finalized_block())
        .unwrap();
    let cutoff = if backend::WINDOWED {
        lfb.saturating_sub(window).max(0)
    } else {
        0
    };
    if backend::WINDOWED {
        assert!(minimum.is_none_or(|height| height >= cutoff));
        assert!(resident <= (height - cutoff + 1) as usize * 7);
    }
    assert!(edges <= resident * 7);
    drop(dag);
    let live = LIVE.load(Ordering::Relaxed);
    let peak = PEAK.load(Ordering::Relaxed);
    println!(
        "{{\"stage\":\"{stage}\",\"blocks\":{blocks},\"max_height\":{height},\"height_end_exclusive\":{height_end_exclusive},\"lfb\":{lfb},\"cutoff\":{cutoff},\"resident_blocks\":{resident},\"resident_edges\":{edges},\"allocator_live_bytes\":{live},\"allocator_peak_bytes\":{peak},\"elapsed_seconds\":{elapsed},{}}}", usable_json()
    );
}

struct LatencySample {
    samples: Vec<u64>,
    seen: u64,
    random: u64,
}

impl LatencySample {
    fn new() -> Self {
        Self {
            samples: Vec::with_capacity(4096),
            seen: 0,
            random: 1,
        }
    }
    fn record(&mut self, elapsed: u128) {
        self.seen += 1;
        let nanos = elapsed.min(u64::MAX as u128) as u64;
        if self.samples.len() < 4096 {
            self.samples.push(nanos);
            return;
        }
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        let position = self.random % self.seen;
        if position < 4096 {
            self.samples[position as usize] = nanos;
        }
    }
    fn report(&mut self, operation: &str, from: usize, through: usize, warmup: usize) {
        self.samples.sort_unstable();
        if self.samples.is_empty() {
            return;
        }
        let count = self.samples.len();
        println!("{{\"stage\":\"latency\",\"warmup_excluded\":true,\"interval_from_blocks\":{from},\"interval_through_blocks\":{through},\"warmup_blocks\":{warmup},\"operation\":\"{operation}\",\"observations\":{},\"reservoir_samples\":{count},\"p50_nanoseconds\":{},\"p95_nanoseconds\":{}}}", self.seen, self.samples[count / 2], self.samples[(count * 95 / 100).min(count - 1)]);
    }
}

fn copy_closed_fixture(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::create_dir(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_closed_fixture(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut blocks = 120_000usize;
    let mut window = 1024i64;
    let mut path = None;
    let mut legacy_migration = false;
    let mut hold_view = false;
    let mut reopen_existing = false;
    let mut checkpoints = Vec::<usize>::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--blocks" => blocks = args.next().ok_or("missing --blocks value")?.parse()?,
            "--window" => window = args.next().ok_or("missing --window value")?.parse()?,
            "--legacy-migration" => legacy_migration = true,
            "--hold-view" => hold_view = true,
            "--reopen-existing" => reopen_existing = true,
            "--checkpoints" => {
                checkpoints = args
                    .next()
                    .ok_or("missing --checkpoints value")?
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
            }
            "--data-dir" => {
                path = Some(PathBuf::from(
                    args.next().ok_or("missing --data-dir value")?,
                ))
            }
            "--help" => {
                println!("dag_memory --blocks 120000 --window 1024 --data-dir NEW_DIRECTORY [--legacy-migration] [--hold-view] [--checkpoints 120000,240000,480000]\nCheckpoints must end at --blocks. They continue one deterministic history, close the writer at each exact size, save a sibling .checkpoint-SIZE fixture, and reopen before continuing. Each interval excludes its first min(10000, interval_blocks/2) insertions from latency quantiles; peak counters reset after reopening. Checkpoint mode cannot combine with hold-view or legacy-migration; copied fixtures support separate experiments. Run three independent processes per implementation. JSONL reports Rust requested allocation bytes; Linux GNU reports malloc_usable_size live/peak and allocation rounding overhead separately; allocator arenas/free fragmentation, LMDB mappings and OS page cache are excluded. Other targets report usable sizes unavailable. Fixture: ten validators, seven blocks per height, finality lag 32 heights. --legacy-migration removes only the new schema from this fresh fixture before reopening. --hold-view pins a view across the final 10000 insertions and reports release. Latency quantiles use a bounded deterministic reservoir of 4096 samples, not all observations. Proposal/validation and GC experiments run separately.");
                println!("--reopen-existing uses an EXISTING stopped-writer checkpoint instead of generating blocks; --blocks must match its count. Add --legacy-migration to remove that copy's new schema before measuring migration. Use separate copies for normal reopen, migration, and GC experiments. It cannot combine with checkpoints or hold-view.");
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if blocks < 2 || window <= 0 {
        return Err("blocks must be >= 2 and window must be positive".into());
    }
    if hold_view && blocks <= 10_000 {
        return Err("--hold-view requires more than 10000 blocks".into());
    }
    if !checkpoints.is_empty()
        && (hold_view
            || legacy_migration
            || checkpoints[0] < 2
            || checkpoints.last() != Some(&blocks)
            || checkpoints.windows(2).any(|pair| pair[0] >= pair[1]))
    {
        return Err("checkpoints must increase from >=2 through --blocks and cannot combine with hold-view or legacy-migration".into());
    }
    let path = path.ok_or("--data-dir NEW_DIRECTORY is required")?;
    if reopen_existing {
        if !checkpoints.is_empty() || hold_view || !path.join("dagstorage/data.mdb").is_file() {
            return Err(
                "--reopen-existing requires an existing fixture and forbids checkpoints/hold-view"
                    .into(),
            );
        }
        let path = std::fs::canonicalize(path)?;
        if legacy_migration {
            backend::remove_new_schema(path.clone()).await?;
        }
        let baseline = LIVE.load(Ordering::Relaxed);
        println!("{{\"stage\":\"existing_fixture_baseline\",\"blocks\":{blocks},\"allocator_live_bytes\":{baseline},\"window\":{window},\"windowed\":{}}}", backend::WINDOWED);
        reset_peaks();
        let start = Instant::now();
        let mut store_manager = manager(path);
        let storage = backend::open(&mut store_manager, window).await?;
        let view = storage.get_representation()?;
        assert!(backend::visible(&view, &hash(blocks - 1))?);
        assert!(!backend::visible(&view, &hash(blocks))?);
        assert_eq!(view.get_max_height(), ((blocks - 2) / 7 + 2) as i64);
        drop(view);
        sample(
            &storage,
            window,
            if legacy_migration {
                "migration"
            } else {
                "reopen"
            },
            blocks,
            start.elapsed().as_secs_f64(),
        );
        return Ok(());
    }
    std::fs::create_dir(&path)?;
    let path = std::fs::canonicalize(path)?;
    let baseline = LIVE.load(Ordering::Relaxed);
    let checkpoint_mode = !checkpoints.is_empty();
    let mut interval_from = 0;
    let mut warmup_blocks = (checkpoints.first().copied().unwrap_or(blocks) / 2).min(10_000);
    println!("{{\"stage\":\"baseline\",\"allocator_live_bytes\":{baseline},\"window\":{window},\"windowed\":{},\"checkpoint_mode\":{checkpoint_mode},\"warmup_blocks\":{warmup_blocks},\"validators\":10,\"blocks_per_height\":7,\"finality_lag\":32}}", backend::WINDOWED);
    let mut store_manager = manager(path.clone());
    let mut storage = backend::open(&mut store_manager, window).await?;
    let validators: Vec<Validator> = (0..10).map(|v| vec![v + 1; 65].into()).collect();
    let bonds = validators
        .iter()
        .cloned()
        .map(|validator| Bond {
            validator,
            stake: 1,
        })
        .collect();
    let mut block = get_random_block(
        Some(0),
        Some(0),
        None,
        None,
        Some(validators[0].clone()),
        None,
        Some(0),
        Some(vec![]),
        Some(vec![]),
        Some(vec![]),
        Some(vec![]),
        Some(bonds),
        None,
        None,
    );
    block.block_hash = hash(0);
    drop(storage.insert(&block, InsertMode::Approved)?);
    let mut latest = vec![hash(0); 10];
    let start = Instant::now();
    let mut insert_nanos = 0u128;
    let mut insert_max_nanos = 0u128;
    let mut insertion_latency = LatencySample::new();
    let mut snapshot_latency = LatencySample::new();
    let mut cold_latency = LatencySample::new();
    let mut held_view = None;
    for index in 1..blocks {
        if hold_view && index == blocks - 10_000 {
            held_view = Some(storage.get_representation()?);
            sample(
                &storage,
                window,
                "before_view_hold",
                index,
                start.elapsed().as_secs_f64(),
            );
        }
        let height = (index - 1) / 7 + 1;
        let sender = index % 10;
        block.block_hash = hash(index);
        block.sender = validators[sender].clone();
        block.seq_num = index.div_ceil(10) as i32;
        block.body.state.block_number = height as i64;
        block.header.parents_hash_list = if height == 1 {
            vec![hash(0)]
        } else {
            ((height - 2) * 7 + 1..=(height - 1) * 7)
                .map(hash)
                .collect()
        };
        block.justifications = validators
            .iter()
            .cloned()
            .zip(latest.iter().cloned())
            .map(|(validator, latest_block_hash)| Justification {
                validator,
                latest_block_hash,
            })
            .collect();
        let insert_start = Instant::now();
        drop(storage.insert(&block, InsertMode::Normal)?);
        let nanos = insert_start.elapsed().as_nanos();
        insert_nanos += nanos;
        insert_max_nanos = insert_max_nanos.max(nanos);
        if index >= interval_from + warmup_blocks {
            insertion_latency.record(nanos);
        }
        latest[sender] = block.block_hash.clone();
        if index % 7 == 0 && height > 32 {
            let anchor = (height - 33) * 7 + 1;
            storage
                .record_directly_finalized(hash(anchor), 1.0, |_| async { Ok(()) })
                .await?;
        }
        if index % 10_000 == 0 {
            sample(
                &storage,
                window,
                "growth",
                index + 1,
                start.elapsed().as_secs_f64(),
            );
        }
        if index % 100 == 0 {
            let now = Instant::now();
            let view = storage.get_representation()?;
            if index >= interval_from + warmup_blocks {
                snapshot_latency.record(now.elapsed().as_nanos());
            }
            if height as i64 > window + 33 {
                let now = Instant::now();
                assert!(view.lookup(&hash(0))?.is_some());
                if index >= interval_from + warmup_blocks {
                    cold_latency.record(now.elapsed().as_nanos());
                }
            }
        }
        if checkpoints.contains(&(index + 1)) {
            let count = index + 1;
            if height > 32 {
                storage
                    .record_directly_finalized(hash((height - 33) * 7 + 1), 1.0, |_| async {
                        Ok(())
                    })
                    .await?;
            }
            let view = storage.get_representation()?;
            assert!(backend::visible(&view, &hash(count - 1))?);
            assert!(!backend::visible(&view, &hash(count))?);
            assert_eq!(view.get_max_height(), height as i64 + 1);
            let lfb = view.block_number_unsafe(&view.last_finalized_block())?;
            assert_eq!(lfb, (height as i64 - 32).max(0));
            drop(view);
            sample(
                &storage,
                window,
                "checkpoint",
                count,
                start.elapsed().as_secs_f64(),
            );
            insertion_latency.report("insertion", interval_from, count, warmup_blocks);
            snapshot_latency.report("snapshot", interval_from, count, warmup_blocks);
            cold_latency.report("cold_metadata", interval_from, count, warmup_blocks);
            drop(storage);
            drop(store_manager);
            let fixture = PathBuf::from(format!("{}.checkpoint-{count}", path.display()));
            let copy_start = Instant::now();
            copy_closed_fixture(&path, &fixture)?;
            println!("{{\"stage\":\"checkpoint_fixture\",\"blocks\":{count},\"closed_writer\":true,\"copy_seconds\":{},\"sibling_suffix\":\".checkpoint-{count}\"}}", copy_start.elapsed().as_secs_f64());
            store_manager = manager(path.clone());
            reset_peaks();
            let reopen_start = Instant::now();
            storage = backend::open(&mut store_manager, window).await?;
            sample(
                &storage,
                window,
                "checkpoint_reopen",
                count,
                reopen_start.elapsed().as_secs_f64(),
            );
            reset_peaks();
            interval_from = count;
            warmup_blocks = checkpoints
                .iter()
                .find(|next| **next > count)
                .map_or(0, |next| ((next - count) / 2).min(10_000));
            insertion_latency = LatencySample::new();
            snapshot_latency = LatencySample::new();
            cold_latency = LatencySample::new();
        }
    }
    let final_height = (blocks - 2) / 7 + 1;
    if final_height > 32 {
        storage
            .record_directly_finalized(hash((final_height - 33) * 7 + 1), 1.0, |_| async { Ok(()) })
            .await?;
    }
    if let Some(view) = held_view.take() {
        assert!(backend::visible(&view, &hash(blocks - 10_001))?);
        sample(
            &storage,
            window,
            "view_held",
            blocks,
            start.elapsed().as_secs_f64(),
        );
        let before = LIVE.load(Ordering::Relaxed);
        drop(view);
        let after = LIVE.load(Ordering::Relaxed);
        assert!(
            after < before,
            "dropping the old view must release retained allocations"
        );
        println!("{{\"stage\":\"view_released\",\"live_before\":{before},\"live_after\":{after},\"released_bytes\":{}}}", before - after);
    }
    sample(
        &storage,
        window,
        "steady",
        blocks,
        start.elapsed().as_secs_f64(),
    );
    println!("{{\"stage\":\"insertion\",\"includes_warmup\":true,\"mean_nanoseconds\":{},\"max_nanoseconds\":{insert_max_nanos}}}", insert_nanos / (blocks - 1) as u128);
    insertion_latency.report("insertion", interval_from, blocks, warmup_blocks);
    snapshot_latency.report("snapshot", interval_from, blocks, warmup_blocks);
    cold_latency.report("cold_metadata", interval_from, blocks, warmup_blocks);
    drop(storage);
    drop(store_manager);
    if legacy_migration {
        backend::remove_new_schema(path.clone()).await?;
    }
    reset_peaks();
    let start = Instant::now();
    let mut store_manager = manager(path);
    let storage = backend::open(&mut store_manager, window).await?;
    sample(
        &storage,
        window,
        if legacy_migration {
            "migration"
        } else {
            "reopen"
        },
        blocks,
        start.elapsed().as_secs_f64(),
    );
    Ok(())
}
