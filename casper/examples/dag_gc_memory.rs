use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::casper::CasperShardConf;
use casper::rust::util::mergeable_channels_gc::{collect_garbage_with, GcProgress, GcSweep};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

#[path = "../../block-storage/examples/support/counting_fixture.rs"]
mod counting_fixture;
use counting_fixture::{hash, manager, reset_peaks, usable_json, LIVE, PEAK};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut path = None;
    let mut window = 1024i64;
    let mut backlog = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => {
                path = Some(PathBuf::from(
                    args.next().ok_or("missing --data-dir value")?,
                ))
            }
            "--window" => window = args.next().ok_or("missing --window value")?.parse()?,
            "--backlog" => backlog = true,
            "--help" => {
                println!("dag_gc_memory --data-dir EXISTING_DAG_MEMORY_FIXTURE [--window 1024] [--backlog]\nRun after block-storage's dag_memory fixture for each history size. This mutates only that experiment's floor/GC tables. Default measures the production paged GC machine with one finalized low-height candidate whose absent body retains its retry entry; --backlog starts with a fresh discovery range and retains every discovered body-absent candidate. The deletion callback is an inert replacement for RuntimeManager. Runtime allocator overhead and real deletion cost are excluded. Scratch diagnostic scans use pages of at most 1024 and are timed separately. Use small fresh fixtures first for --backlog to quantify repeated traversal cost.");
                return Ok(());
            }
            _ => return Err(format!("unknown argument {arg}").into()),
        }
    }
    let path = path.ok_or("--data-dir from the dag_memory experiment is required")?;
    let data_file = path.join("dagstorage/data.mdb");
    if !data_file.is_file() {
        return Err("existing LMDB experiment fixture is required".into());
    }
    let mut store_manager = manager(path);
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut store_manager, window).await?;
    let dag = storage.get_representation()?;
    let mut held = 0usize;
    let mut after = None;
    loop {
        let page = dag.scan_membership_page(after.as_deref(), 1024)?;
        held += page.rows.len();
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    let lfb = dag.last_finalized_block();
    let lfb_height = dag.block_number_unsafe(&lfb)?;
    if lfb_height <= 4 {
        return Err("fixture needs finalized history above height 4".into());
    }
    dag.put_cached_floor(lfb.clone(), lfb)?;
    let candidate = hash(1);
    if !dag.is_finalized(&candidate)? {
        return Err("height-one candidate must be finalized".into());
    }
    let gc = storage.gc_store();
    if gc
        .retry_page(None)?
        .rows
        .iter()
        .any(|(hash, _)| hash != &candidate)
    {
        return Err(
            "fixture contains unrelated GC retries; use a fresh experiment directory".into(),
        );
    }
    let target = lfb_height - 4;
    if backlog {
        if gc.discovery_state()?.claimed_ceiling.is_some() || !gc.retry_page(None)?.rows.is_empty()
        {
            return Err("--backlog requires a fixture without previous GC discovery".into());
        }
    } else {
        gc.reserve_discovery(target)?;
        gc.commit_discovery_page(
            &[(candidate.clone(), dag.block_number_unsafe(&candidate)?)],
            None,
            Some(target),
        )?;
    }
    drop(dag);
    let mut body_manager = InMemoryStoreManager::new();
    let blocks = KeyValueBlockStore::create_from_kvm(&mut body_manager).await?;
    let mut conf = CasperShardConf::new();
    conf.max_parent_depth = 3;
    conf.mergeable_channels_gc_depth_buffer = 1;
    let mut sweep = GcSweep::new();
    let baseline = LIVE.load(Ordering::Relaxed);
    reset_peaks();
    let disk_before = std::fs::metadata(&data_file)?.len();
    let mut disk_high = disk_before;
    let mut scratch_observed = 0u64;
    let mut gc_seconds = 0.0;
    let mut diagnostics_seconds = 0.0;
    let started = Instant::now();
    let mut turns = 0usize;
    loop {
        let turn_start = Instant::now();
        let progress = collect_garbage_with(&mut sweep, &storage, &blocks, &conf, &mut |_| {
            Err(
                shared::rust::store::key_value_store::KvStoreError::InvalidArgument(
                    "body-absent experiment must never invoke deletion".into(),
                ),
            )
        })?;
        gc_seconds += turn_start.elapsed().as_secs_f64();
        turns += 1;
        let diagnostics_start = Instant::now();
        disk_high = disk_high.max(std::fs::metadata(&data_file)?.len());
        if turns <= 100 || turns.is_multiple_of(100) {
            scratch_observed = scratch_observed.max(gc.scratch_bytes()?);
        }
        diagnostics_seconds += diagnostics_start.elapsed().as_secs_f64();
        if matches!(progress, GcProgress::Complete { .. }) {
            break;
        }
    }
    let stats = sweep.work_stats();
    let live = LIVE.load(Ordering::Relaxed);
    let peak = PEAK.load(Ordering::Relaxed);
    assert_eq!(
        gc.scratch_bytes()?,
        0,
        "completed job must reclaim scratch rows"
    );
    let mut pending = 0usize;
    let mut after = None;
    loop {
        let page = gc.retry_page(after.as_deref())?;
        pending += page.rows.len();
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    if backlog {
        assert!(pending > 1);
    } else {
        assert_eq!(pending, 1, "missing body must preserve low candidate");
    }
    assert!(
        stats.parent_edges > 0,
        "benchmark must execute an ancestry proof"
    );
    println!("{{\"blocks\":{held},\"window\":{window},\"backlog\":{backlog},\"runtime_allocator_included\":false,\"baseline_live_bytes\":{baseline},\"final_live_bytes\":{live},\"peak_live_bytes\":{peak},\"peak_above_baseline_bytes\":{},\"parent_edges\":{},\"work_records\":{},\"turns\":{turns},\"yields\":{},\"gc_seconds\":{gc_seconds},\"diagnostic_seconds\":{diagnostics_seconds},\"elapsed_seconds\":{},\"scratch_observed_bytes\":{scratch_observed},\"scratch_remaining_bytes\":0,\"environment_growth_bytes\":{},\"pending_candidates\":{pending},{}}}", peak.saturating_sub(baseline), stats.parent_edges, stats.work_records, turns.saturating_sub(1), started.elapsed().as_secs_f64(), disk_high.saturating_sub(disk_before), usable_json());
    Ok(())
}
