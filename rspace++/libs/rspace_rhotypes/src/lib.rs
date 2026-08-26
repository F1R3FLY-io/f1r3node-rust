// FFI boundary -- raw pointer args are required by the Scala JNA caller
// surface. clippy::not_unsafe_ptr_arg_deref would otherwise force every
// extern "C" wrapper to be marked unsafe, which would change the Scala side.
#![allow(clippy::not_unsafe_ptr_arg_deref, clippy::unnecessary_unwrap)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::runtime::Runtime;

fn blocking_runtime() -> &'static Runtime {
    static RT: std::sync::OnceLock<Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| Runtime::new().unwrap())
}

// Tracks total bytes currently allocated and leaked to JNA callers
static ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

#[no_mangle]
pub extern "C" fn get_allocated_bytes() -> usize { ALLOCATED_BYTES.load(Ordering::SeqCst) }

#[no_mangle]
pub extern "C" fn reset_allocated_bytes() { ALLOCATED_BYTES.store(0, Ordering::SeqCst) }
use std::ffi::{CStr, c_char};
use std::sync::{Arc, Mutex};

use models::rhoapi::*;
use models::rspace_plus_plus_types::*;
use prost::Message;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use rspace_plus_plus::rspace::history::cold_store::{ColdLeafAddress, PersistedDataKind};
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::GB;
use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;
use rspace_plus_plus::rspace::shared::trie_exporter::NodePath;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::import_session::{
    SealedStateImport, StateImportError, StateImportPage, StateImportPageReceipt,
    StateImportPublication, StateImportPublicationMode, StateImportSession, StateImportSpec,
};
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};
use shared::rust::ByteVector;

/*
 * This library contains predefined types for Channel, Pattern, Data, and
 * Continuation - RhoTypes
 * These types (C, P, A, K) MUST MATCH the corresponding types on the Scala
 * side in 'RSpacePlusPlus_RhoTypes.scala'
 */
#[repr(C)]
pub struct Space {
    rspace: Mutex<RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>,
}

#[repr(C)]
pub struct ReplaySpace {
    replay_space: Mutex<ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>>,
}

#[no_mangle]
pub extern "C" fn space_new(path: *const c_char) -> *mut Space {
    let c_str = unsafe { CStr::from_ptr(path) };
    let data_dir = c_str.to_str().unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let rspace = rt
        .block_on(async {
            // println!("\nHit space_new");

            // let mut kvm = mk_rspace_store_manager(lmdb_path.into(), 1 * GB);
            // let store = kvm.r_space_stores().await.unwrap();

            let mut kvm = mk_rspace_store_manager((&format!("{}/rspace++/", data_dir)).into(), GB);
            let store = kvm.r_space_stores().await.unwrap();

            // println!("\nhistory store: {:?}",
            // store.history.lock().unwrap().to_map().unwrap()); println!("\
            // nroots store: {:?}", store.roots.lock().unwrap().to_map().unwrap());
            // roots_store.print_store();
            // println!("\ncold store: {:?}",
            // store.cold.lock().unwrap().to_map().unwrap().len()); cold_store.
            // print_store();

            // let store =
            //     get_or_create_rspace_store(&format!("{}/rspace++_{}/", data_dir,
            // unique_id), 1 * GB)         .expect("Error getting RSpaceStore:
            // "); let store =
            // get_or_create_rspace_store(&format!("{}/rspace++/", data_dir), 1 * GB)
            //     .expect("Error getting RSpaceStore: ");

            RSpace::create(store, Arc::new(Box::new(Matcher)))
        })
        .unwrap();

    Box::into_raw(Box::new(Space {
        rspace: Mutex::new(rspace),
    }))
}

#[no_mangle]
pub extern "C" fn space_new_replay(rspace: *mut Space) -> *mut ReplaySpace {
    let rspace = unsafe { (*rspace).rspace.lock().unwrap() };

    let replay_space = ReplayRSpace::apply(
        rspace.get_history_repository(),
        rspace.get_store(),
        Arc::new(Box::new(Matcher)),
    );

    Box::into_raw(Box::new(ReplaySpace {
        replay_space: Mutex::new(replay_space),
    }))
}

#[no_mangle]
pub extern "C" fn space_print(rspace: *mut Space) {
    unsafe { (*rspace).rspace.lock().unwrap().get_store().print() }
}

#[no_mangle]
pub extern "C" fn space_clear(rspace: *mut Space) {
    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    blocking_runtime().block_on(async {
        space
            .clear()
            .await
            .expect("Rust RSpacePlusPlus Library: Failed to clear")
    });
}

#[no_mangle]
pub extern "C" fn spatial_match_result(
    payload_pointer: *const u8,
    target_bytes_len: usize,
    pattern_bytes_len: usize,
) -> *const u8 {
    let payload_slice = unsafe {
        std::slice::from_raw_parts(payload_pointer, target_bytes_len + pattern_bytes_len)
    };
    let (target_slice, pattern_slice) = payload_slice.split_at(target_bytes_len);

    let target = models::rust::rholang::protobuf_decoder::decode_par(target_slice).unwrap();
    let pattern = models::rust::rholang::protobuf_decoder::decode_par(pattern_slice).unwrap();

    let mut spatial_matcher = SpatialMatcherContext::new();
    let result_option = spatial_matcher.spatial_match_result(target, pattern);

    match result_option {
        Some(free_map) => {
            let free_map_proto = FreeMapProto {
                entries: free_map.clone(),
            };

            let mut bytes = free_map_proto.encode_to_vec();
            let len = bytes.len() as u32;
            let len_bytes = len.to_le_bytes().to_vec();
            let mut result = len_bytes;
            result.append(&mut bytes);
            let total_len = result.len();
            let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
            ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
            ptr
        }
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn produce(
    rspace: *mut Space,
    payload_pointer: *const u8,
    channel_bytes_len: usize,
    data_bytes_len: usize,
    persist: bool,
) -> *const u8 {
    // println!("\nHit produce");

    let payload_slice =
        unsafe { std::slice::from_raw_parts(payload_pointer, channel_bytes_len + data_bytes_len) };
    let (channel_slice, data_slice) = payload_slice.split_at(channel_bytes_len);

    let channel = models::rust::rholang::protobuf_decoder::decode_par(channel_slice).unwrap();
    let data =
        models::rust::rholang::protobuf_decoder::decode_list_par_with_random(data_slice).unwrap();

    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    let result_option = blocking_runtime()
        .block_on(async { space.produce(channel, data, persist).await })
        .unwrap();

    match result_option {
        Some((cont_result, rspace_results, produce_event)) => {
            let protobuf_cont_result = ContResultProto {
                continuation: Some(cont_result.continuation.clone()),
                persistent: cont_result.persistent,
                channels: cont_result.channels,
                patterns: cont_result.patterns,
                peek: cont_result.peek,
            };

            let protobuf_results = rspace_results
                .into_iter()
                .map(|result| RSpaceResultProto {
                    channel: Some(result.channel),
                    matched_datum: Some(result.matched_datum),
                    removed_datum: Some(result.removed_datum),
                    persistent: result.persistent,
                })
                .collect();

            let protobuf_produce = ProduceProto {
                channel_hash: produce_event.channel_hash.bytes(),
                hash: produce_event.hash.bytes(),
                persistent: produce_event.persistent,
                is_deterministic: produce_event.is_deterministic,
                output_value: produce_event.output_value,
            };

            let maybe_action_result = ProduceResult {
                cont_result: Some(protobuf_cont_result),
                results: protobuf_results,
                produce: Some(protobuf_produce),
            };

            let mut bytes = maybe_action_result.encode_to_vec();
            let len = bytes.len() as u32;
            let len_bytes = len.to_le_bytes().to_vec();
            let mut result = len_bytes;
            result.append(&mut bytes);
            let total_len = result.len();
            let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
            ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
            ptr
        }
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn consume(
    rspace: *mut Space,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
) -> *const u8 {
    // println!("\nHit rust consume");

    // let thread_id = thread::current().id();
    // let current_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // println!("Thread ID: {:?}, Current Time: {}", thread_id, current_time);

    let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer, payload_bytes_len) };
    let consume_params = ConsumeParams::decode(payload_slice).unwrap();

    let channels = consume_params.channels;
    let patterns = consume_params.patterns;
    let continuation = consume_params.continuation.unwrap();
    let persist = consume_params.persist;
    let peeks = consume_params.peeks.into_iter().map(|e| e.value).collect();

    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    let result_option = blocking_runtime()
        .block_on(async {
            space
                .consume(channels, patterns, continuation, persist, peeks)
                .await
        })
        .unwrap();

    match result_option {
        Some((cont_result, rspace_results)) => {
            let protobuf_cont_result = ContResultProto {
                continuation: Some(cont_result.continuation.clone()),
                persistent: cont_result.persistent,
                channels: cont_result.channels,
                patterns: cont_result.patterns,
                peek: cont_result.peek,
            };

            let protobuf_results = rspace_results
                .into_iter()
                .map(|result| RSpaceResultProto {
                    channel: Some(result.channel),
                    matched_datum: Some(result.matched_datum),
                    removed_datum: Some(result.removed_datum),
                    persistent: result.persistent,
                })
                .collect();

            let maybe_action_result = ConsumeResult {
                cont_result: Some(protobuf_cont_result),
                results: protobuf_results,
            };

            let mut bytes = maybe_action_result.encode_to_vec();
            let len = bytes.len() as u32;
            let len_bytes = len.to_le_bytes().to_vec();
            let mut result = len_bytes;
            result.append(&mut bytes);

            // println!("\nlen: {:?}", len);
            let total_len = result.len();
            let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
            ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
            ptr
        }
        None => {
            // println!("\nnone in rust consume");
            std::ptr::null()
        }
    }
}

#[no_mangle]
pub extern "C" fn install(
    rspace: *mut Space,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
) -> *const u8 {
    // println!("\nHit install");

    let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer, payload_bytes_len) };
    let consume_params = InstallParams::decode(payload_slice).unwrap();

    let channels = consume_params.channels;
    let patterns = consume_params.patterns;
    let continuation = consume_params.continuation.unwrap();

    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    let result_option = blocking_runtime()
        .block_on(async { space.install(channels, patterns, continuation).await })
        .unwrap();

    match result_option {
        None => std::ptr::null(),
        Some(_) => {
            panic!("RUST ERROR: Installing can be done only on startup")
        }
    }
}

// #[no_mangle]
// pub extern "C" fn create_checkpoint(rspace: *mut Space) -> *const u8 {
//     let checkpoint = unsafe {
//         (*rspace)
//             .rspace
//             .lock()
//             .unwrap()
//             .create_checkpoint()
//             .expect("Rust RSpacePlusPlus Library: Failed to create
// checkpoint")     };

//     let log = checkpoint.log;
//     let log_proto: Vec<EventProto> = log
//         .into_iter()
//         .map(|event| match event {
//             Event::Comm(comm) => {
//                 let comm_proto = CommProto {
//                     consume: {
//                         Some(ConsumeProto {
//                             channel_hashes: comm
//                                 .consume
//                                 .channel_hashes
//                                 .iter()
//                                 .map(|hash| hash.bytes())
//                                 .collect(),
//                             hash: comm.consume.hash.bytes(),
//                             persistent: comm.consume.persistent,
//                         })
//                     },
//                     produces: {
//                         comm.produces
//                             .into_iter()
//                             .map(|produce| ProduceProto {
//                                 channel_hash: produce.channel_hash.bytes(),
//                                 hash: produce.hash.bytes(),
//                                 persistent: produce.persistent,
//                             })
//                             .collect()
//                     },
//                     peeks: {
//                         comm.peeks
//                             .into_iter()
//                             .map(|peek| SortedSetElement { value: peek as i32
// })                             .collect()
//                     },
//                     times_repeated: {
//                         let mut produce_counter_map_entries:
// Vec<ProduceCounterMapEntry> =                             Vec::new();
//                         for (key, value) in comm.times_repeated {
//                             let produce = ProduceProto {
//                                 channel_hash: key.channel_hash.bytes(),
//                                 hash: key.hash.bytes(),
//                                 persistent: key.persistent,
//                             };

//
// produce_counter_map_entries.push(ProduceCounterMapEntry {
// key: Some(produce),                                 value,
//                             });
//                         }
//                         produce_counter_map_entries
//                     },
//                 };

//                 EventProto {
//                     event_type:
// Some(event_proto::EventType::Comm(comm_proto)),                 }
//             }
//             Event::IoEvent(io_event) => match io_event {
//                 IOEvent::Produce(produce) => {
//                     let produce_proto = ProduceProto {
//                         channel_hash: produce.channel_hash.bytes(),
//                         hash: produce.hash.bytes(),
//                         persistent: produce.persistent,
//                     };
//                     EventProto {
//                         event_type:
// Some(event_proto::EventType::IoEvent(IoEventProto {
// io_event_type: Some(io_event_proto::IoEventType::Produce(
// produce_proto,                             )),
//                         })),
//                     }
//                 }
//                 IOEvent::Consume(consume) => {
//                     let consume_proto = ConsumeProto {
//                         channel_hashes: consume
//                             .channel_hashes
//                             .iter()
//                             .map(|hash| hash.bytes())
//                             .collect(),
//                         hash: consume.hash.bytes(),
//                         persistent: consume.persistent,
//                     };
//                     EventProto {
//                         event_type:
// Some(event_proto::EventType::IoEvent(IoEventProto {
// io_event_type: Some(io_event_proto::IoEventType::Consume(
// consume_proto,                             )),
//                         })),
//                     }
//                 }
//             },
//         })
//         .collect();

//     let checkpoint_proto = CheckpointProto {
//         root: checkpoint.root.bytes(),
//         log: log_proto,
//     };

//     let mut bytes = checkpoint_proto.encode_to_vec();
//     let len = bytes.len() as u32;
//     let len_bytes = len.to_le_bytes().to_vec();
//     let mut result = len_bytes;
//     result.append(&mut bytes);
//     Box::leak(result.into_boxed_slice()).as_ptr()
// }

#[no_mangle]
pub extern "C" fn reset_rspace(
    rspace: *mut Space,
    root_pointer: *const u8,
    root_bytes_len: usize,
) -> i32 {
    let root_slice = unsafe { std::slice::from_raw_parts(root_pointer, root_bytes_len) };
    let root = Blake2b256Hash::from_bytes(root_slice.to_vec());

    let rs = match unsafe { (*rspace).rspace.lock() } {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("ERROR: failed to lock rspace in reset: poisoned");
            poisoned.into_inner()
        }
    };

    match blocking_runtime().block_on(async { rs.reset(&root).await }) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("ERROR: rspace reset failed: {:?}", e);
            1
        }
    }
}

// #[no_mangle]
// pub extern "C" fn get_data(
//     rspace: *mut Space,
//     channel_pointer: *const u8,
//     channel_bytes_len: usize,
// ) -> *const u8 {
//     let channel_slice = unsafe { std::slice::from_raw_parts(channel_pointer,
// channel_bytes_len) };     let channel =
// models::rust::rholang::protobuf_decoder::decode_par(channel_slice).unwrap();

//     // let rt = tokio::runtime::Runtime::new().unwrap();
//     // let datums =
//     //     rt.block_on(async { unsafe {
// (*rspace).rspace.lock().unwrap().get_data(channel).await } });     let datums
// = unsafe { (*rspace).rspace.lock().unwrap().get_data(channel) };

//     // println!("\ndatums in rust get_data: {:?}", datums);

//     let datums_protos: Vec<DatumProto> = datums
//         .into_iter()
//         .map(|datum| DatumProto {
//             a: Some(std::sync::Arc::unwrap_or_clone(datum.a)),
//             persist: datum.persist,
//             source: Some(ProduceProto {
//                 channel_hash: datum.source.channel_hash.bytes(),
//                 hash: datum.source.hash.bytes(),
//                 persistent: datum.source.persistent,
//             }),
//         })
//         .collect();

//     let datums_proto = DatumsProto {
//         datums: datums_protos,
//     };

//     let mut bytes = datums_proto.encode_to_vec();
//     let len = bytes.len() as u32;
//     let len_bytes = len.to_le_bytes().to_vec();
//     let mut result = len_bytes;
//     result.append(&mut bytes);
//     Box::leak(result.into_boxed_slice()).as_ptr()
// }

// #[no_mangle]
// pub extern "C" fn get_waiting_continuations(
//     rspace: *mut Space,
//     channels_pointer: *const u8,
//     channels_bytes_len: usize,
// ) -> *const u8 {
//     let channels_slice =
//         unsafe { std::slice::from_raw_parts(channels_pointer,
// channels_bytes_len) };     let channels_proto =
// ChannelsProto::decode(channels_slice).unwrap();

//     let wks = unsafe {
//         (*rspace)
//             .rspace
//             .lock()
//             .unwrap()
//             .get_waiting_continuations(channels_proto.channels)
//     };

//     let wks_protos: Vec<WaitingContinuationProto> = wks
//         .into_iter()
//         .map(|wk| {
//             let res = WaitingContinuationProto {
//                 patterns: wk.patterns,
//                 continuation: Some(wk.continuation.clone()),
//                 persist: wk.persist,
//                 peeks: wk
//                     .peeks
//                     .into_iter()
//                     .map(|peek| SortedSetElement { value: peek as i32 })
//                     .collect(),
//                 source: Some(ConsumeProto {
//                     channel_hashes: wk
//                         .source
//                         .channel_hashes
//                         .iter()
//                         .map(|hash| hash.bytes())
//                         .collect(),
//                     hash: wk.source.hash.bytes(),
//                     persistent: wk.source.persistent,
//                 }),
//             };

//             res
//         })
//         .collect();

//     let wks_proto = WaitingContinuationsProto { wks: wks_protos };

//     let mut bytes = wks_proto.encode_to_vec();
//     let len = bytes.len() as u32;
//     let len_bytes = len.to_le_bytes().to_vec();
//     let mut result = len_bytes;
//     result.append(&mut bytes);
//     Box::leak(result.into_boxed_slice()).as_ptr()
// }

// #[no_mangle]
// pub extern "C" fn get_joins(
//     rspace: *mut Space,
//     channel_pointer: *const u8,
//     channel_bytes_len: usize,
// ) -> *const u8 {
//     let channel_slice = unsafe { std::slice::from_raw_parts(channel_pointer,
// channel_bytes_len) };     let channel =
// models::rust::rholang::protobuf_decoder::decode_par(channel_slice).unwrap();

//     let joins = unsafe { (*rspace).rspace.lock().unwrap().get_joins(&channel)
// };

//     let vec_join: Vec<JoinProto> = joins.into_iter().map(|join| JoinProto {
// join }).collect();     let joins_proto = JoinsProto { joins: vec_join };

//     let mut bytes = joins_proto.encode_to_vec();
//     let len = bytes.len() as u32;
//     let len_bytes = len.to_le_bytes().to_vec();
//     let mut result = len_bytes;
//     result.append(&mut bytes);
//     Box::leak(result.into_boxed_slice()).as_ptr()
// }

#[no_mangle]
pub extern "C" fn to_map(rspace: *mut Space) -> *const u8 {
    let hot_store_mapped = unsafe { (*rspace).rspace.lock().unwrap().get_store().to_map() };

    let mut map_entries: Vec<StoreToMapEntry> = Vec::new();

    for (key, value) in hot_store_mapped {
        let datums = value
            .data
            .into_iter()
            .map(|datum| DatumProto {
                a: Some(std::sync::Arc::unwrap_or_clone(datum.a)),
                persist: datum.persist,
                source: Some(ProduceProto {
                    channel_hash: datum.source.channel_hash.bytes(),
                    hash: datum.source.hash.bytes(),
                    persistent: datum.source.persistent,
                    is_deterministic: datum.source.is_deterministic,
                    output_value: datum.source.output_value,
                }),
            })
            .collect();

        let wks = value
            .wks
            .into_iter()
            .map(|wk| {
                let res = WaitingContinuationProto {
                    patterns: std::sync::Arc::unwrap_or_clone(wk.patterns),
                    continuation: Some((*wk.continuation).clone()),
                    persist: wk.persist,
                    peeks: wk
                        .peeks
                        .into_iter()
                        .map(|peek| SortedSetElement { value: peek })
                        .collect(),
                    source: Some(ConsumeProto {
                        channel_hashes: wk
                            .source
                            .channel_hashes
                            .iter()
                            .map(|hash| hash.bytes())
                            .collect(),
                        hash: wk.source.hash.bytes(),
                        persistent: wk.source.persistent,
                    }),
                };

                res
            })
            .collect();

        let value = StoreToMapValue { data: datums, wks };
        map_entries.push(StoreToMapEntry {
            key,
            value: Some(value),
        });
    }

    let to_map_result = StoreToMapResult { map_entries };

    let mut bytes = to_map_result.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn spawn(rspace: *mut Space) -> *mut Space {
    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    let new_rspace = space
        .spawn()
        .expect("Rust RSpacePlusPlus Library: Failed to spawn");

    Box::into_raw(Box::new(Space {
        rspace: Mutex::new(new_rspace),
    }))
}

// #[no_mangle]
// pub extern "C" fn create_soft_checkpoint(rspace: *mut Space) -> *const u8 {
//     // println!("\nhit rust lib create_soft_checkpoint");
//     let soft_checkpoint = unsafe {
// (*rspace).rspace.lock().unwrap().create_soft_checkpoint() };

//     let mut conts_map_entries: Vec<StoreStateContMapEntry> = Vec::new();
//     let mut installed_conts_map_entries: Vec<StoreStateInstalledContMapEntry>
// = Vec::new();     let mut data_map_entries: Vec<StoreStateDataMapEntry> =
// Vec::new();     let mut joins_map_entries: Vec<StoreStateJoinsMapEntry> =
// Vec::new();     let mut installed_joins_map_entries:
// Vec<StoreStateInstalledJoinsMapEntry> = Vec::new();

//     let hot_store_state = soft_checkpoint.cache_snapshot;

//     for (key, value) in hot_store_state.continuations.clone().into_iter() {
//         let wks: Vec<WaitingContinuationProto> = value
//             .into_iter()
//             .map(|wk| {
//                 let res = WaitingContinuationProto {
//                     patterns: wk.patterns,
//                     continuation: Some(wk.continuation.clone()),
//                     persist: wk.persist,
//                     peeks: wk
//                         .peeks
//                         .into_iter()
//                         .map(|peek| SortedSetElement { value: peek as i32 })
//                         .collect(),
//                     source: Some(ConsumeProto {
//                         channel_hashes: wk
//                             .source
//                             .channel_hashes
//                             .iter()
//                             .map(|hash| hash.bytes())
//                             .collect(),
//                         hash: wk.source.hash.bytes(),
//                         persistent: wk.source.persistent,
//                     }),
//                 };

//                 res
//             })
//             .collect();

//         conts_map_entries.push(StoreStateContMapEntry { key, value: wks });
//     }

//     for (key, value) in
// hot_store_state.installed_continuations.clone().into_iter() {         let wk
// = WaitingContinuationProto {             patterns: value.patterns,
//             continuation: Some(value.continuation.clone()),
//             persist: value.persist,
//             peeks: value
//                 .peeks
//                 .into_iter()
//                 .map(|peek| SortedSetElement { value: peek as i32 })
//                 .collect(),
//             source: Some(ConsumeProto {
//                 channel_hashes: value
//                     .source
//                     .channel_hashes
//                     .iter()
//                     .map(|hash| hash.bytes())
//                     .collect(),
//                 hash: value.source.hash.bytes(),
//                 persistent: value.source.persistent,
//             }),
//         };

//         installed_conts_map_entries.push(StoreStateInstalledContMapEntry {
//             key,
//             value: Some(wk),
//         });
//     }

//     for (key, value) in hot_store_state.data.clone().into_iter() {
//         let datums = value
//             .into_iter()
//             .map(|datum| DatumProto {
//                 a: Some(std::sync::Arc::unwrap_or_clone(datum.a)),
//                 persist: datum.persist,
//                 source: Some(ProduceProto {
//                     channel_hash: datum.source.channel_hash.bytes(),
//                     hash: datum.source.hash.bytes(),
//                     persistent: datum.source.persistent,
//                 }),
//             })
//             .collect();

//         data_map_entries.push(StoreStateDataMapEntry {
//             key: Some(key),
//             value: datums,
//         });
//     }

//     for (key, value) in hot_store_state.joins.clone().into_iter() {
//         let joins = value.into_iter().map(|join| JoinProto { join
// }).collect();

//         joins_map_entries.push(StoreStateJoinsMapEntry {
//             key: Some(key),
//             value: joins,
//         });
//     }

//     for (key, value) in hot_store_state.installed_joins.clone().into_iter() {
//         let joins = value.into_iter().map(|join| JoinProto { join
// }).collect();

//         installed_joins_map_entries.push(StoreStateInstalledJoinsMapEntry {
//             key: Some(key),
//             value: joins,
//         });
//     }

//     let hot_store_state_proto = HotStoreStateProto {
//         continuations: conts_map_entries,
//         installed_continuations: installed_conts_map_entries,
//         data: data_map_entries,
//         joins: joins_map_entries,
//         installed_joins: installed_joins_map_entries,
//     };

//     let log = soft_checkpoint.log;
//     let log_proto: Vec<EventProto> = log
//         .into_iter()
//         .map(|event| match event {
//             Event::Comm(comm) => {
//                 let comm_proto = CommProto {
//                     consume: {
//                         Some(ConsumeProto {
//                             channel_hashes: comm
//                                 .consume
//                                 .channel_hashes
//                                 .iter()
//                                 .map(|hash| hash.bytes())
//                                 .collect(),
//                             hash: comm.consume.hash.bytes(),
//                             persistent: comm.consume.persistent,
//                         })
//                     },
//                     produces: {
//                         comm.produces
//                             .into_iter()
//                             .map(|produce| ProduceProto {
//                                 channel_hash: produce.channel_hash.bytes(),
//                                 hash: produce.hash.bytes(),
//                                 persistent: produce.persistent,
//                             })
//                             .collect()
//                     },
//                     peeks: {
//                         comm.peeks
//                             .into_iter()
//                             .map(|peek| SortedSetElement { value: peek as i32
// })                             .collect()
//                     },
//                     times_repeated: {
//                         let mut produce_counter_map_entries:
// Vec<ProduceCounterMapEntry> =                             Vec::new();
//                         for (key, value) in comm.times_repeated {
//                             let produce = ProduceProto {
//                                 channel_hash: key.channel_hash.bytes(),
//                                 hash: key.hash.bytes(),
//                                 persistent: key.persistent,
//                             };

//
// produce_counter_map_entries.push(ProduceCounterMapEntry {
// key: Some(produce),                                 value,
//                             });
//                         }
//                         produce_counter_map_entries
//                     },
//                 };

//                 EventProto {
//                     event_type:
// Some(event_proto::EventType::Comm(comm_proto)),                 }
//             }
//             Event::IoEvent(io_event) => match io_event {
//                 IOEvent::Produce(produce) => {
//                     let produce_proto = ProduceProto {
//                         channel_hash: produce.channel_hash.bytes(),
//                         hash: produce.hash.bytes(),
//                         persistent: produce.persistent,
//                     };
//                     EventProto {
//                         event_type:
// Some(event_proto::EventType::IoEvent(IoEventProto {
// io_event_type: Some(io_event_proto::IoEventType::Produce(
// produce_proto,                             )),
//                         })),
//                     }
//                 }
//                 IOEvent::Consume(consume) => {
//                     let consume_proto = ConsumeProto {
//                         channel_hashes: consume
//                             .channel_hashes
//                             .iter()
//                             .map(|hash| hash.bytes())
//                             .collect(),
//                         hash: consume.hash.bytes(),
//                         persistent: consume.persistent,
//                     };
//                     EventProto {
//                         event_type:
// Some(event_proto::EventType::IoEvent(IoEventProto {
// io_event_type: Some(io_event_proto::IoEventType::Consume(
// consume_proto,                             )),
//                         })),
//                     }
//                 }
//             },
//         })
//         .collect();

//     let mut produce_counter_map_entries: Vec<ProduceCounterMapEntry> =
// Vec::new();     let produce_counter_map = soft_checkpoint.produce_counter;

//     for (key, value) in produce_counter_map {
//         let produce = ProduceProto {
//             channel_hash: key.channel_hash.bytes(),
//             hash: key.hash.bytes(),
//             persistent: key.persistent,
//         };

//         produce_counter_map_entries.push(ProduceCounterMapEntry {
//             key: Some(produce),
//             value,
//         });
//     }

//     let soft_checkpoint_proto = SoftCheckpointProto {
//         cache_snapshot: Some(hot_store_state_proto),
//         log: log_proto,
//         produce_counter: produce_counter_map_entries,
//     };

//     let mut bytes = soft_checkpoint_proto.encode_to_vec();
//     let len = bytes.len() as u32;
//     let len_bytes = len.to_le_bytes().to_vec();
//     let mut result = len_bytes;
//     result.append(&mut bytes);
//     Box::leak(result.into_boxed_slice()).as_ptr()
// }

// #[no_mangle]
// pub extern "C" fn revert_to_soft_checkpoint(
//     rspace: *mut Space,
//     payload_pointer: *const u8,
//     payload_bytes_len: usize,
// ) -> () {
//     let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer,
// payload_bytes_len) };     let soft_checkpoint_proto =
// SoftCheckpointProto::decode(payload_slice).unwrap();
//     let cache_snapshot_proto = soft_checkpoint_proto.cache_snapshot.unwrap();

//     let conts_map: DashMap<Vec<Par>, Vec<WaitingContinuation<BindPattern,
// TaggedContinuation>>> =         cache_snapshot_proto
//             .continuations
//             .into_iter()
//             .map(|map_entry| {
//                 let key = map_entry.key;
//                 let value = map_entry
//                     .value
//                     .into_iter()
//                     .map(|cont_proto| WaitingContinuation {
//                         patterns: cont_proto.patterns,
//                         continuation: cont_proto.continuation.unwrap(),
//                         persist: cont_proto.persist,
//                         peeks: cont_proto
//                             .peeks
//                             .iter()
//                             .map(|element| element.value)
//                             .collect(),
//                         source: {
//                             let consume_proto = cont_proto.source.unwrap();
//                             Consume {
//                                 channel_hashes: consume_proto
//                                     .channel_hashes
//                                     .iter()
//                                     .map(|hash_bytes| {
//
// Blake2b256Hash::from_bytes(hash_bytes.to_vec())
// })                                     .collect(),
//                                 hash:
// Blake2b256Hash::from_bytes(consume_proto.hash),
// persistent: consume_proto.persistent,                             }
//                         },
//                     })
//                     .collect();

//                 (key, value)
//             })
//             .collect();

//     let installed_conts_map: DashMap<
//         Vec<Par>,
//         WaitingContinuation<BindPattern, TaggedContinuation>,
//     > = cache_snapshot_proto .installed_continuations .into_iter()
//     > .map(|map_entry| { let key = map_entry.key; let wk_proto =
//     > map_entry.value.unwrap(); let value = WaitingContinuation { patterns:
//     > wk_proto.patterns, continuation: wk_proto.continuation.unwrap(),
//     > persist: wk_proto.persist, peeks: wk_proto.peeks.iter().map(|element|
//     > element.value).collect(), source: { let consume_proto =
//     > wk_proto.source.unwrap(); Consume { channel_hashes: consume_proto
//     > .channel_hashes .iter() .map(|hash_bytes|
//     > Blake2b256Hash::from_bytes(hash_bytes.to_vec())) .collect(), hash:
//     > Blake2b256Hash::from_bytes(consume_proto.hash), persistent:
//     > consume_proto.persistent, } }, };

//             (key, value)
//         })
//         .collect();

//     let datums_map: DashMap<Par, Vec<Datum<ListParWithRandom>>> =
// cache_snapshot_proto         .data
//         .into_iter()
//         .map(|map_entry| {
//             let key = map_entry.key.unwrap();
//             let value = map_entry
//                 .value
//                 .into_iter()
//                 .map(|datum_proto| Datum {
//                     a: datum_proto.a.unwrap(),
//                     persist: datum_proto.persist,
//                     source: {
//                         let produce_proto = datum_proto.source.unwrap();
//                         Produce {
//                             channel_hash:
// Blake2b256Hash::from_bytes(produce_proto.channel_hash),
// hash: Blake2b256Hash::from_bytes(produce_proto.hash),
// persistent: produce_proto.persistent,                         }
//                     },
//                 })
//                 .collect();

//             (key, value)
//         })
//         .collect();

//     let joins_map: DashMap<Par, Vec<Vec<Par>>> = cache_snapshot_proto
//         .joins
//         .into_iter()
//         .map(|map_entry| {
//             let key = map_entry.key.unwrap();
//             let value = map_entry
//                 .value
//                 .into_iter()
//                 .map(|join_proto| join_proto.join)
//                 .collect();

//             (key, value)
//         })
//         .collect();

//     let installed_joins_map: DashMap<Par, Vec<Vec<Par>>> =
// cache_snapshot_proto         .installed_joins
//         .into_iter()
//         .map(|map_entry| {
//             let key = map_entry.key.unwrap();
//             let value = map_entry
//                 .value
//                 .into_iter()
//                 .map(|join_proto| join_proto.join)
//                 .collect();

//             (key, value)
//         })
//         .collect();

//     let log: Vec<Event> = soft_checkpoint_proto
//         .log
//         .into_iter()
//         .map(|log_entry| match log_entry.event_type.unwrap() {
//             event_proto::EventType::Comm(comm_proto) => {
//                 let consume_proto = comm_proto.consume.unwrap();
//                 let comm = COMM {
//                     consume: {
//                         Consume {
//                             channel_hashes: {
//                                 consume_proto
//                                     .channel_hashes
//                                     .iter()
//                                     .map(|hash|
// Blake2b256Hash::from_bytes(hash.clone()))
// .collect()                             },
//                             hash:
// Blake2b256Hash::from_bytes(consume_proto.hash),
// persistent: consume_proto.persistent,                         }
//                     },
//                     produces: {
//                         comm_proto
//                             .produces
//                             .into_iter()
//                             .map(|produce_proto| Produce {
//                                 channel_hash: Blake2b256Hash::from_bytes(
//                                     produce_proto.channel_hash,
//                                 ),
//                                 hash:
// Blake2b256Hash::from_bytes(produce_proto.hash),
// persistent: produce_proto.persistent,                             })
//                             .collect()
//                     },
//                     peeks: {
//                         comm_proto
//                             .peeks
//                             .iter()
//                             .map(|element| element.value)
//                             .collect()
//                     },
//                     times_repeated: {
//                         comm_proto
//                             .times_repeated
//                             .into_iter()
//                             .map(|map_entry| {
//                                 let key_proto = map_entry.key.unwrap();
//                                 let produce = Produce {
//                                     channel_hash: Blake2b256Hash::from_bytes(
//                                         key_proto.channel_hash,
//                                     ),
//                                     hash:
// Blake2b256Hash::from_bytes(key_proto.hash),
// persistent: key_proto.persistent,                                 };

//                                 let value = map_entry.value;

//                                 (produce, value)
//                             })
//                             .collect()
//                     },
//                 };
//                 Event::Comm(comm)
//             }
//             event_proto::EventType::IoEvent(io_event) => match
// io_event.io_event_type.unwrap() {
// io_event_proto::IoEventType::Produce(produce_proto) => {
// let produce = Produce {                         channel_hash:
// Blake2b256Hash::from_bytes(produce_proto.channel_hash),
// hash: Blake2b256Hash::from_bytes(produce_proto.hash),
// persistent: produce_proto.persistent,                     };
//                     Event::IoEvent(IOEvent::Produce(produce))
//                 }
//                 io_event_proto::IoEventType::Consume(consume_proto) => {
//                     let consume = Consume {
//                         channel_hashes: {
//                             consume_proto
//                                 .channel_hashes
//                                 .iter()
//                                 .map(|hash|
// Blake2b256Hash::from_bytes(hash.clone()))
// .collect()                         },
//                         hash: Blake2b256Hash::from_bytes(consume_proto.hash),
//                         persistent: consume_proto.persistent,
//                     };
//                     Event::IoEvent(IOEvent::Consume(consume))
//                 }
//             },
//         })
//         .collect();

//     let produce_counter_map: BTreeMap<Produce, i32> = soft_checkpoint_proto
//         .produce_counter
//         .into_iter()
//         .map(|map_entry| {
//             let key_proto = map_entry.key.unwrap();
//             let produce = Produce {
//                 channel_hash:
// Blake2b256Hash::from_bytes(key_proto.channel_hash),                 hash:
// Blake2b256Hash::from_bytes(key_proto.hash),                 persistent:
// key_proto.persistent,             };

//             let value = map_entry.value;

//             (produce, value)
//         })
//         .collect();

//     let cache_snapshot = HotStoreState {
//         continuations: conts_map,
//         installed_continuations: installed_conts_map,
//         data: datums_map,
//         joins: joins_map,
//         installed_joins: installed_joins_map,
//     };

//     let soft_checkpoint = SoftCheckpoint {
//         cache_snapshot,
//         log,
//         produce_counter: produce_counter_map,
//     };

//     unsafe {
//         (*rspace)
//             .rspace
//             .lock()
//             .unwrap()
//             .revert_to_soft_checkpoint(soft_checkpoint)
//             .expect("Rust RSpacePlusPlus Library: Failed to revert to soft
// checkpoint")     }
// }

/* HistoryRepo */

#[no_mangle]
pub extern "C" fn history_repo_root(rspace: *mut Space) -> *const u8 {
    let root = unsafe {
        (*rspace)
            .rspace
            .lock()
            .unwrap()
            .get_history_repository()
            .root()
    };

    let hash = hash(&root);
    let hash_proto = HashProto { hash: hash.bytes() };

    let mut bytes = hash_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

/* Exporter */
// #[no_mangle]
// pub extern "C" fn get_nodes(
//     rspace: *mut Space,
//     payload_pointer: *const u8,
//     payload_bytes_len: usize,
// ) -> *const u8 {
//     let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer,
// payload_bytes_len) };     let exporter_params =
// ExporterParams::decode(payload_slice).unwrap();

//     let start_path: Vec<(Blake2b256Hash, Option<Byte>)> = exporter_params
//         .start_path
//         .into_iter()
//         .map(|path_proto| {
//             let key_hash = Blake2b256Hash::from_bytes(path_proto.key_hash);
//             let value: Option<Byte> = if path_proto.optional_byte.len() > 0 {
//                 Some(path_proto.optional_byte[0])
//             } else {
//                 None
//             };

//             (key_hash, value)
//         })
//         .collect();

//     let nodes = unsafe {
//         let space = (*rspace).rspace.lock().unwrap();
//         space
//             .history_repository
//             .exporter()
//             .lock()
//             .unwrap()
//             .get_nodes(
//                 start_path,
//                 exporter_params.skip.try_into().unwrap(),
//                 exporter_params.take.try_into().unwrap(),
//             )
//     };

//     let trie_nodes_proto_vec: Vec<TrieNodeProto> = nodes
//         .into_iter()
//         .map(|node| TrieNodeProto {
//             hash: node.hash.bytes(),
//             is_leaf: node.is_leaf,
//             path: node
//                 .path
//                 .into_iter()
//                 .map(|path| PathProto {
//                     key_hash: path.0.bytes(),
//                     optional_byte: if path.1.is_some() {
//                         vec![path.1.unwrap()]
//                     } else {
//                         Vec::new()
//                     },
//                 })
//                 .collect(),
//         })
//         .collect();

//     let trie_nodes_proto_vec = TrieNodesProto {
//         nodes: trie_nodes_proto_vec,
//     };

//     let mut bytes = trie_nodes_proto_vec.encode_to_vec();
//     let len = bytes.len() as u32;
//     let len_bytes = len.to_le_bytes().to_vec();
//     let mut result = len_bytes;
//     result.append(&mut bytes);
//     Box::leak(result.into_boxed_slice()).as_ptr()
// }

#[no_mangle]
pub extern "C" fn get_history_items(
    rspace: *mut Space,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
) -> *const u8 {
    let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer, payload_bytes_len) };
    let keys_proto = KeysProto::decode(payload_slice).unwrap();

    let keys = keys_proto
        .keys
        .into_iter()
        .map(|key| Blake2b256Hash::from_bytes(key.hash))
        .collect();

    let history_items = unsafe {
        (*rspace)
            .rspace
            .lock()
            .unwrap()
            .get_history_repository()
            .exporter()
            .get_history_items(keys)
            .unwrap()
    };

    let history_items_proto = ItemsProto {
        items: history_items
            .into_iter()
            .map(|history_item| ItemProto {
                key_hash: history_item.0.bytes(),
                value: history_item.1,
            })
            .collect(),
    };

    let mut bytes = history_items_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn get_data_items(
    rspace: *mut Space,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
) -> *const u8 {
    let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer, payload_bytes_len) };
    let Ok(protobuf_addresses) = ColdLeafAddressesProto::decode(payload_slice) else {
        return std::ptr::null();
    };

    let addresses = protobuf_addresses
        .addresses
        .into_iter()
        .map(|protobuf_address| {
            let kind = match PersistedDataKindProto::try_from(protobuf_address.kind).ok()? {
                PersistedDataKindProto::PersistedDataKindData => PersistedDataKind::Data,
                PersistedDataKindProto::PersistedDataKindContinuations => {
                    PersistedDataKind::Continuations
                }
                PersistedDataKindProto::PersistedDataKindJoins => PersistedDataKind::Joins,
                PersistedDataKindProto::PersistedDataKindUnspecified => return None,
            };
            let digest = Blake2b256Hash::from_bytes(protobuf_address.digest);
            ColdLeafAddress::new(kind, digest).ok()
        })
        .collect::<Option<Vec<_>>>();
    let Some(addresses) = addresses else {
        return std::ptr::null();
    };

    let data_items = unsafe {
        (*rspace)
            .rspace
            .lock()
            .unwrap()
            .get_history_repository()
            .exporter()
            .get_data_items(addresses)
    };
    let Ok(data_items) = data_items else {
        return std::ptr::null();
    };

    let data_items_proto = ItemsProto {
        items: data_items
            .into_iter()
            .map(|data_item| ItemProto {
                key_hash: data_item.0.bytes(),
                value: data_item.1,
            })
            .collect(),
    };

    let mut bytes = data_items_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn get_history_and_data(
    rspace: *mut Space,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
) -> *const u8 {
    let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer, payload_bytes_len) };
    let exporter_params = ExporterParams::decode(payload_slice).unwrap();

    let start_path: Vec<(Blake2b256Hash, Option<u8>)> = exporter_params
        .path
        .into_iter()
        .map(|path_element| {
            (
                Blake2b256Hash::from_bytes(path_element.key_hash),
                if !path_element.optional_byte.is_empty() {
                    Some(path_element.optional_byte[0])
                } else {
                    None
                },
            )
        })
        .collect();

    let exporter = unsafe {
        (*rspace)
            .rspace
            .lock()
            .unwrap()
            .get_history_repository()
            .exporter()
    };

    let history_and_data_items = RSpaceExporterItems::get_history_and_data(
        exporter,
        start_path,
        exporter_params.skip,
        exporter_params.take,
    );
    let Ok((history_items, data_items)) = history_and_data_items else {
        return std::ptr::null();
    };

    let history_items_proto = StoreItemsProto {
        items: history_items
            .items
            .into_iter()
            .map(|history_item| ItemProto {
                key_hash: history_item.0.bytes(),
                value: history_item.1,
            })
            .collect(),
        last_path: history_items
            .last_path
            .into_iter()
            .map(|path| PathElement {
                key_hash: path.0.bytes(),
                optional_byte: {
                    if path.1.is_some() {
                        vec![path.1.unwrap()]
                    } else {
                        Vec::new()
                    }
                },
            })
            .collect(),
    };

    let data_items_proto = StoreItemsProto {
        items: data_items
            .items
            .into_iter()
            .map(|data_item| ItemProto {
                key_hash: data_item.0.bytes(),
                value: data_item.1,
            })
            .collect(),
        last_path: data_items
            .last_path
            .into_iter()
            .map(|path| PathElement {
                key_hash: path.0.bytes(),
                optional_byte: {
                    if path.1.is_some() {
                        vec![path.1.unwrap()]
                    } else {
                        Vec::new()
                    }
                },
            })
            .collect(),
    };

    let history_and_data_items_proto = HistoryAndDataItems {
        history_items: Some(history_items_proto),
        data_items: Some(data_items_proto),
    };

    let mut bytes = history_and_data_items_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn get_exporter_root(rspace: *mut Space) -> *const u8 {
    let root = unsafe {
        (*rspace)
            .rspace
            .lock()
            .unwrap()
            .get_history_repository()
            .exporter()
            .get_root()
            .unwrap()
    };

    let hash = hash(&root);
    let hash_proto = HashProto { hash: hash.bytes() };

    let mut bytes = hash_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

// #[no_mangle]
// pub extern "C" fn traverse_history(
//     rspace: *mut Space,
//     payload_pointer: *const u8,
//     payload_bytes_len: usize,
// ) -> *const u8 {
//     let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer,
// payload_bytes_len) };     let exporter_params =
// ExporterParams::decode(payload_slice).unwrap();

//     let start_path: Vec<(Blake2b256Hash, Option<Byte>)> = exporter_params
//         .start_path
//         .into_iter()
//         .map(|path_proto| {
//             let key_hash = Blake2b256Hash::from_bytes(path_proto.key_hash);
//             let value: Option<Byte> = if path_proto.optional_byte.len() > 0 {
//                 Some(path_proto.optional_byte[0])
//             } else {
//                 None
//             };

//             (key_hash, value)
//         })
//         .collect();

//     let nodes = RSpaceExporterInstance::traverse_history(
//         start_path,
//         exporter_params.skip.try_into().unwrap(),
//         exporter_params.take.try_into().unwrap(),
//         get_from_history,
//     );

//     let trie_nodes_proto_vec: Vec<TrieNodeProto> = nodes
//         .into_iter()
//         .map(|node| TrieNodeProto {
//             hash: node.hash.bytes(),
//             is_leaf: node.is_leaf,
//             path: node
//                 .path
//                 .into_iter()
//                 .map(|path| PathProto {
//                     key_hash: path.0.bytes(),
//                     optional_byte: if path.1.is_some() {
//                         vec![path.1.unwrap()]
//                     } else {
//                         Vec::new()
//                     },
//                 })
//                 .collect(),
//         })
//         .collect();

//     let trie_nodes_proto_vec = TrieNodesProto {
//         nodes: trie_nodes_proto_vec,
//     };

//     let mut bytes = trie_nodes_proto_vec.encode_to_vec();
//     let len = bytes.len() as u32;
//     let len_bytes = len.to_le_bytes().to_vec();
//     let mut result = len_bytes;
//     result.append(&mut bytes);
//     Box::leak(result.into_boxed_slice()).as_ptr()
// }

/* Importer */

/// Opaque, mutable state-import capability returned by [`state_import_begin`].
///
/// The field is private so foreign callers cannot manufacture or inspect the
/// capability. `state_import_seal` and `state_import_abort_open` consume the
/// allocation exactly once; using the pointer again is a foreign-caller error.
#[repr(C)]
pub struct OpenStateImportHandle {
    session: Mutex<Option<Box<dyn StateImportSession>>>,
}

/// Opaque, fully verified root-publication capability returned by
/// [`state_import_seal`]. `state_import_publish` and
/// `state_import_abort_sealed` consume the allocation exactly once.
#[repr(C)]
pub struct SealedStateImportHandle {
    session: Mutex<Option<Box<dyn SealedStateImport>>>,
}

const PROTOBUF_STATE_IMPORT_ERROR_NULL_POINTER: i32 =
    StateImportErrorCodeProto::StateImportErrorCodeNullPointer as i32;
const PROTOBUF_STATE_IMPORT_ERROR_INVALID_LENGTH: i32 =
    StateImportErrorCodeProto::StateImportErrorCodeInvalidLength as i32;
const PROTOBUF_STATE_IMPORT_ERROR_DECODE: i32 =
    StateImportErrorCodeProto::StateImportErrorCodeProtobufDecode as i32;
const PROTOBUF_STATE_IMPORT_ERROR_INVALID_HANDLE_STATE: i32 =
    StateImportErrorCodeProto::StateImportErrorCodeInvalidHandleState as i32;
const PROTOBUF_STATE_IMPORT_ERROR_SPACE_STATE: i32 =
    StateImportErrorCodeProto::StateImportErrorCodeSpaceState as i32;
const PROTOBUF_STATE_IMPORT_ERROR_IMPORT: i32 =
    StateImportErrorCodeProto::StateImportErrorCodeImport as i32;
const PROTOBUF_STATE_IMPORT_ERROR_PANIC: i32 =
    StateImportErrorCodeProto::StateImportErrorCodePanic as i32;

const PROTOBUF_STATE_IMPORT_SUCCESS_OPENED: i32 =
    StateImportSuccessKindProto::StateImportSuccessKindOpened as i32;
const PROTOBUF_STATE_IMPORT_SUCCESS_PAGE_STAGED: i32 =
    StateImportSuccessKindProto::StateImportSuccessKindPageStaged as i32;
const PROTOBUF_STATE_IMPORT_SUCCESS_SEALED: i32 =
    StateImportSuccessKindProto::StateImportSuccessKindSealed as i32;
const PROTOBUF_STATE_IMPORT_SUCCESS_PUBLISHED: i32 =
    StateImportSuccessKindProto::StateImportSuccessKindPublished as i32;
const PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED: i32 =
    StateImportSuccessKindProto::StateImportSuccessKindAborted as i32;

#[derive(Debug)]
struct StateImportFfiFailure {
    code: i32,
    message: String,
}

impl StateImportFfiFailure {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn null_pointer(field: &'static str) -> Self {
        Self::new(PROTOBUF_STATE_IMPORT_ERROR_NULL_POINTER, format!("{field} is null"))
    }

    fn invalid_length(field: &'static str, expected: &'static str, actual: usize) -> Self {
        Self::new(
            PROTOBUF_STATE_IMPORT_ERROR_INVALID_LENGTH,
            format!("{field} must contain {expected}, found {actual} bytes"),
        )
    }

    fn numeric_range(field: &'static str, actual: usize) -> Self {
        Self::new(
            PROTOBUF_STATE_IMPORT_ERROR_INVALID_LENGTH,
            format!("{field} value {actual} is not representable by protobuf uint64"),
        )
    }

    fn decode(message: impl Into<String>) -> Self {
        Self::new(PROTOBUF_STATE_IMPORT_ERROR_DECODE, message)
    }

    fn invalid_handle(message: impl Into<String>) -> Self {
        Self::new(PROTOBUF_STATE_IMPORT_ERROR_INVALID_HANDLE_STATE, message)
    }

    fn space_state(message: impl Into<String>) -> Self {
        Self::new(PROTOBUF_STATE_IMPORT_ERROR_SPACE_STATE, message)
    }

    fn import(error: StateImportError) -> Self {
        Self::new(PROTOBUF_STATE_IMPORT_ERROR_IMPORT, error.to_string())
    }
}

fn protobuf_state_import_success(
    kind: i32,
    page_receipt: Option<StateImportPageReceiptProto>,
    publication: Option<StateImportPublicationProto>,
) -> StateImportSuccessProto {
    StateImportSuccessProto {
        kind,
        page_receipt,
        publication,
    }
}

fn protobuf_state_import_result(
    operation: &'static str,
    result: Result<StateImportSuccessProto, StateImportFfiFailure>,
) -> StateImportResultProto {
    use state_import_result_proto::Outcome;

    let outcome = match result {
        Ok(protobuf_success) => Outcome::Success(protobuf_success),
        Err(error) => Outcome::Error(StateImportErrorProto {
            code: error.code,
            operation: operation.to_string(),
            message: error.message,
        }),
    };
    StateImportResultProto {
        outcome: Some(outcome),
    }
}

fn leak_length_prefixed_state_import_result(
    mut protobuf_result: StateImportResultProto,
) -> *const u8 {
    let mut protobuf_bytes = protobuf_result.encode_to_vec();
    if u32::try_from(protobuf_bytes.len()).is_err() {
        protobuf_result = protobuf_state_import_result(
            "state_import_result",
            Err(StateImportFfiFailure::new(
                PROTOBUF_STATE_IMPORT_ERROR_INVALID_LENGTH,
                "encoded state-import result exceeds the u32 JNA framing length",
            )),
        );
        protobuf_bytes = protobuf_result.encode_to_vec();
    }

    // The fallback above contains only fixed-size fields and one short literal.
    let protobuf_len = protobuf_bytes.len() as u32;
    let mut framed = Vec::with_capacity(4 + protobuf_bytes.len());
    framed.extend_from_slice(&protobuf_len.to_le_bytes());
    framed.append(&mut protobuf_bytes);
    let framed_len = framed.len();
    let pointer = Box::leak(framed.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(framed_len, Ordering::SeqCst);
    pointer
}

fn panic_payload_message(payload: &(dyn std::any::Any + std::marker::Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "state-import operation panicked with a non-string payload".to_string()
    }
}

/// Run one import operation behind an unwind barrier and serialize exactly one
/// structured result. The second barrier prevents even result construction
/// from unwinding through an `extern "C"` frame.
fn state_import_ffi_boundary<F>(operation: &'static str, call: F) -> *const u8
where F: FnOnce() -> Result<StateImportSuccessProto, StateImportFfiFailure> {
    let result = catch_unwind(AssertUnwindSafe(|| {
        leak_length_prefixed_state_import_result(protobuf_state_import_result(operation, call()))
    }));
    match result {
        Ok(pointer) => pointer,
        Err(payload) => catch_unwind(AssertUnwindSafe(|| {
            leak_length_prefixed_state_import_result(protobuf_state_import_result(
                operation,
                Err(StateImportFfiFailure::new(
                    PROTOBUF_STATE_IMPORT_ERROR_PANIC,
                    panic_payload_message(payload.as_ref()),
                )),
            ))
        }))
        .unwrap_or(std::ptr::null()),
    }
}

unsafe fn required_protobuf_input<'a>(
    pointer: *const u8,
    length: usize,
    field: &'static str,
) -> Result<&'a [u8], StateImportFfiFailure> {
    if pointer.is_null() {
        return Err(StateImportFfiFailure::null_pointer(field));
    }
    // SAFETY: after the explicit null check, the foreign caller owns the ABI
    // obligation that `pointer` names at least `length` readable bytes for the
    // duration of this call.
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
}

fn protobuf_digest(
    bytes: Vec<u8>,
    field: &'static str,
) -> Result<Blake2b256Hash, StateImportFfiFailure> {
    if bytes.len() != 32 {
        return Err(StateImportFfiFailure::invalid_length(field, "exactly 32 bytes", bytes.len()));
    }
    Ok(Blake2b256Hash::from_bytes(bytes))
}

fn protobuf_node_path(
    protobuf_path: Vec<PathElement>,
    field: &'static str,
) -> Result<NodePath, StateImportFfiFailure> {
    protobuf_path
        .into_iter()
        .map(|protobuf_element| {
            let digest = protobuf_digest(protobuf_element.key_hash, field)?;
            let edge = match protobuf_element.optional_byte.as_slice() {
                [] => None,
                [edge] => Some(*edge),
                bytes => {
                    return Err(StateImportFfiFailure::invalid_length(
                        field,
                        "zero or one edge byte",
                        bytes.len(),
                    ));
                }
            };
            Ok((digest, edge))
        })
        .collect()
}

fn protobuf_items(
    protobuf_items: Vec<ItemProto>,
    field: &'static str,
) -> Result<Vec<(Blake2b256Hash, ByteVector)>, StateImportFfiFailure> {
    protobuf_items
        .into_iter()
        .map(|protobuf_item| {
            Ok((protobuf_digest(protobuf_item.key_hash, field)?, protobuf_item.value))
        })
        .collect()
}

fn state_import_spec_from_protobuf(
    protobuf_spec: StateImportSpecProto,
) -> Result<StateImportSpec, StateImportFfiFailure> {
    let roots = protobuf_spec
        .target_roots
        .into_iter()
        .map(|root| protobuf_digest(root, "state-import target root"))
        .collect::<Result<Vec<_>, _>>()?;
    let protobuf_publication_mode = StateImportPublicationModeProto::try_from(
        protobuf_spec.publication_mode,
    )
    .map_err(|_| {
        StateImportFfiFailure::decode(format!(
            "unknown state-import publication mode {}",
            protobuf_spec.publication_mode
        ))
    })?;
    let publication_mode = match protobuf_publication_mode {
        StateImportPublicationModeProto::StateImportPublicationModePreserveCurrent => {
            if !protobuf_spec.activation_root.is_empty() {
                return Err(StateImportFfiFailure::invalid_length(
                    "preserve-current activation root",
                    "zero bytes",
                    protobuf_spec.activation_root.len(),
                ));
            }
            StateImportPublicationMode::PreserveCurrent
        }
        StateImportPublicationModeProto::StateImportPublicationModeActivate => {
            StateImportPublicationMode::Activate(protobuf_digest(
                protobuf_spec.activation_root,
                "state-import activation root",
            )?)
        }
        StateImportPublicationModeProto::StateImportPublicationModeUnspecified => {
            return Err(StateImportFfiFailure::decode(
                "state-import publication mode is unspecified",
            ));
        }
    };
    StateImportSpec::new(roots, publication_mode).map_err(StateImportFfiFailure::import)
}

fn state_import_page_from_protobuf(
    protobuf_page: StateImportPageProto,
) -> Result<StateImportPage, StateImportFfiFailure> {
    Ok(StateImportPage {
        start_path: protobuf_node_path(protobuf_page.start_path, "state-import start path")?,
        advertised_next_path: protobuf_node_path(
            protobuf_page.advertised_next_path,
            "state-import advertised next path",
        )?,
        skip: protobuf_page.skip,
        take: protobuf_page.take,
        history_items: protobuf_items(protobuf_page.history_items, "state-import history digest")?,
        data_items: protobuf_items(protobuf_page.data_items, "state-import cold-leaf digest")?,
    })
}

fn path_to_protobuf(path: NodePath) -> Vec<PathElement> {
    path.into_iter()
        .map(|(digest, edge)| PathElement {
            key_hash: digest.bytes(),
            optional_byte: edge.into_iter().collect(),
        })
        .collect()
}

fn receipt_to_protobuf(
    receipt: StateImportPageReceipt,
) -> Result<StateImportPageReceiptProto, StateImportFfiFailure> {
    let history_objects_staged = u64::try_from(receipt.history_objects_staged).map_err(|_| {
        StateImportFfiFailure::numeric_range(
            "history-object staging count",
            receipt.history_objects_staged,
        )
    })?;
    let cold_leaves_staged = u64::try_from(receipt.cold_leaves_staged).map_err(|_| {
        StateImportFfiFailure::numeric_range("cold-leaf staging count", receipt.cold_leaves_staged)
    })?;
    Ok(StateImportPageReceiptProto {
        authenticated_next_path: path_to_protobuf(receipt.next_path),
        terminal: receipt.terminal,
        history_objects_staged,
        cold_leaves_staged,
    })
}

fn publication_to_protobuf(publication: StateImportPublication) -> StateImportPublicationProto {
    StateImportPublicationProto {
        verified_roots: publication
            .verified_roots
            .into_iter()
            .map(|root| root.bytes())
            .collect(),
        activation: publication
            .activation
            .map(|activation| StateImportActivationProto {
                root: activation.root.bytes(),
                generation: activation.generation,
            }),
    }
}

unsafe fn importer_from_space(
    rspace: *mut Space,
) -> Result<Arc<dyn RSpaceImporter>, StateImportFfiFailure> {
    // SAFETY: the caller-created `Space` allocation remains live for the call;
    // null is rejected before dereference.
    let space =
        unsafe { rspace.as_ref() }.ok_or_else(|| StateImportFfiFailure::null_pointer("rspace"))?;
    let repository = space
        .rspace
        .lock()
        .map_err(|_| StateImportFfiFailure::space_state("rspace mutex is poisoned"))?
        .get_history_repository();
    Ok(repository.importer())
}

unsafe fn initialize_open_handle_output(
    output: *mut *mut OpenStateImportHandle,
) -> Result<(), StateImportFfiFailure> {
    if output.is_null() {
        return Err(StateImportFfiFailure::null_pointer("open-handle output"));
    }
    // SAFETY: the non-null out-pointer is writable by the foreign caller.
    unsafe { output.write(std::ptr::null_mut()) };
    Ok(())
}

unsafe fn initialize_sealed_handle_output(
    output: *mut *mut SealedStateImportHandle,
) -> Result<(), StateImportFfiFailure> {
    if output.is_null() {
        return Err(StateImportFfiFailure::null_pointer("sealed-handle output"));
    }
    // SAFETY: the non-null out-pointer is writable by the foreign caller.
    unsafe { output.write(std::ptr::null_mut()) };
    Ok(())
}

unsafe fn consume_open_handle(
    handle: *mut OpenStateImportHandle,
) -> Result<Box<dyn StateImportSession>, StateImportFfiFailure> {
    if handle.is_null() {
        return Err(StateImportFfiFailure::null_pointer("open state-import handle"));
    }
    // SAFETY: open handles originate exclusively from `state_import_begin` and
    // this consuming operation owns the allocation exactly once.
    let handle = unsafe { Box::from_raw(handle) };
    let session = handle.session.into_inner().map_err(|_| {
        StateImportFfiFailure::invalid_handle(
            "open state-import handle was poisoned by an earlier panic",
        )
    })?;
    session.ok_or_else(|| StateImportFfiFailure::invalid_handle("open handle is already consumed"))
}

unsafe fn consume_sealed_handle(
    handle: *mut SealedStateImportHandle,
) -> Result<Box<dyn SealedStateImport>, StateImportFfiFailure> {
    if handle.is_null() {
        return Err(StateImportFfiFailure::null_pointer("sealed state-import handle"));
    }
    // SAFETY: sealed handles originate exclusively from `state_import_seal`
    // and this consuming operation owns the allocation exactly once.
    let handle = unsafe { Box::from_raw(handle) };
    let session = handle.session.into_inner().map_err(|_| {
        StateImportFfiFailure::invalid_handle(
            "sealed state-import handle was poisoned by an earlier panic",
        )
    })?;
    session
        .ok_or_else(|| StateImportFfiFailure::invalid_handle("sealed handle is already consumed"))
}

/// Begin one affine state-import transaction.
///
/// On success `out_open_handle` receives a new opaque capability and the
/// returned pointer names a length-prefixed `StateImportResultProto`. On every
/// failure the out-pointer is set to null and the result contains a structured
/// error.
///
/// # Safety
///
/// `rspace` must be null or point to a live [`Space`]. `payload_pointer` must
/// point to `payload_bytes_len` readable bytes. `out_open_handle` must be null
/// or point to writable storage for one pointer, and must not race another
/// access for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn state_import_begin(
    rspace: *mut Space,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
    out_open_handle: *mut *mut OpenStateImportHandle,
) -> *const u8 {
    state_import_ffi_boundary("state_import_begin", || {
        unsafe { initialize_open_handle_output(out_open_handle) }?;
        let payload =
            unsafe { required_protobuf_input(payload_pointer, payload_bytes_len, "import spec") }?;
        let protobuf_spec = StateImportSpecProto::decode(payload).map_err(|error| {
            StateImportFfiFailure::decode(format!("cannot decode state-import spec: {error}"))
        })?;
        let spec = state_import_spec_from_protobuf(protobuf_spec)?;
        let importer = unsafe { importer_from_space(rspace) }?;
        let session = importer
            .begin_import(spec)
            .map_err(StateImportFfiFailure::import)?;
        let handle = Box::into_raw(Box::new(OpenStateImportHandle {
            session: Mutex::new(Some(session)),
        }));
        // SAFETY: initialization above established a non-null writable out-pointer.
        unsafe { out_open_handle.write(handle) };
        Ok(protobuf_state_import_success(PROTOBUF_STATE_IMPORT_SUCCESS_OPENED, None, None))
    })
}

/// Validate and stage one complete authenticated page without publishing any
/// root authority. The open handle remains valid after an ordinary validation
/// error, but becomes poisoned if the underlying implementation panics.
///
/// # Safety
///
/// `open_handle` must be null or a live capability returned by
/// [`state_import_begin`], and it must not be concurrently consumed.
/// `payload_pointer` must point to `payload_bytes_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn state_import_stage_page(
    open_handle: *mut OpenStateImportHandle,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
) -> *const u8 {
    state_import_ffi_boundary("state_import_stage_page", || {
        if open_handle.is_null() {
            return Err(StateImportFfiFailure::null_pointer("open state-import handle"));
        }
        let payload =
            unsafe { required_protobuf_input(payload_pointer, payload_bytes_len, "import page") }?;
        let protobuf_page = StateImportPageProto::decode(payload).map_err(|error| {
            StateImportFfiFailure::decode(format!("cannot decode state-import page: {error}"))
        })?;
        let page = state_import_page_from_protobuf(protobuf_page)?;
        // SAFETY: the handle is non-null, live, and not concurrently consumed;
        // the mutex permits sequential hand-off between foreign threads.
        let handle = unsafe { &*open_handle };
        let mut session = handle.session.lock().map_err(|_| {
            StateImportFfiFailure::invalid_handle(
                "open state-import handle was poisoned by an earlier panic",
            )
        })?;
        let session = session.as_deref_mut().ok_or_else(|| {
            StateImportFfiFailure::invalid_handle("open state-import handle is consumed")
        })?;
        let receipt = session
            .validate_and_stage_page(page)
            .map_err(StateImportFfiFailure::import)?;
        Ok(protobuf_state_import_success(
            PROTOBUF_STATE_IMPORT_SUCCESS_PAGE_STAGED,
            Some(receipt_to_protobuf(receipt)?),
            None,
        ))
    })
}

/// Consume an open capability, verify the complete staged closure, and return
/// a distinct sealed publication capability.
///
/// # Safety
///
/// `open_handle` must be null or a live, uniquely owned capability returned by
/// [`state_import_begin`]. A non-null handle is consumed exactly once by this
/// call, even when sealing fails. `out_sealed_handle` must be null or point to
/// writable storage for one pointer.
#[no_mangle]
pub unsafe extern "C" fn state_import_seal(
    open_handle: *mut OpenStateImportHandle,
    out_sealed_handle: *mut *mut SealedStateImportHandle,
) -> *const u8 {
    state_import_ffi_boundary("state_import_seal", || {
        unsafe { initialize_sealed_handle_output(out_sealed_handle) }?;
        let session = unsafe { consume_open_handle(open_handle) }?;
        let sealed = session.seal().map_err(StateImportFfiFailure::import)?;
        let handle = Box::into_raw(Box::new(SealedStateImportHandle {
            session: Mutex::new(Some(sealed)),
        }));
        // SAFETY: initialization above established a non-null writable out-pointer.
        unsafe { out_sealed_handle.write(handle) };
        Ok(protobuf_state_import_success(PROTOBUF_STATE_IMPORT_SUCCESS_SEALED, None, None))
    })
}

/// Consume a sealed capability and atomically publish its verified roots.
///
/// # Safety
///
/// `sealed_handle` must be null or a live, uniquely owned capability returned
/// by [`state_import_seal`]. A non-null handle is consumed exactly once.
#[no_mangle]
pub unsafe extern "C" fn state_import_publish(
    sealed_handle: *mut SealedStateImportHandle,
) -> *const u8 {
    state_import_ffi_boundary("state_import_publish", || {
        let sealed = unsafe { consume_sealed_handle(sealed_handle) }?;
        let publication = sealed.publish().map_err(StateImportFfiFailure::import)?;
        Ok(protobuf_state_import_success(
            PROTOBUF_STATE_IMPORT_SUCCESS_PUBLISHED,
            None,
            Some(publication_to_protobuf(publication)),
        ))
    })
}

/// Consume and drop an unpublished open capability.
///
/// # Safety
///
/// `open_handle` must be null or a live, uniquely owned capability returned by
/// [`state_import_begin`]. A non-null handle is consumed exactly once.
#[no_mangle]
pub unsafe extern "C" fn state_import_abort_open(
    open_handle: *mut OpenStateImportHandle,
) -> *const u8 {
    state_import_ffi_boundary("state_import_abort_open", || {
        let session = unsafe { consume_open_handle(open_handle) }?;
        drop(session);
        Ok(protobuf_state_import_success(PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED, None, None))
    })
}

/// Consume and drop an unpublished sealed capability.
///
/// # Safety
///
/// `sealed_handle` must be null or a live, uniquely owned capability returned
/// by [`state_import_seal`]. A non-null handle is consumed exactly once.
#[no_mangle]
pub unsafe extern "C" fn state_import_abort_sealed(
    sealed_handle: *mut SealedStateImportHandle,
) -> *const u8 {
    state_import_ffi_boundary("state_import_abort_sealed", || {
        let session = unsafe { consume_sealed_handle(sealed_handle) }?;
        drop(session);
        Ok(protobuf_state_import_success(PROTOBUF_STATE_IMPORT_SUCCESS_ABORTED, None, None))
    })
}

#[no_mangle]
pub extern "C" fn get_history_item(
    rspace: *mut Space,
    hash_pointer: *const u8,
    hash_bytes_len: usize,
) -> *const u8 {
    let hash_slice = unsafe { std::slice::from_raw_parts(hash_pointer, hash_bytes_len) };
    let hash = Blake2b256Hash::from_bytes(hash_slice.to_vec());

    let history_item = unsafe {
        (*rspace)
            .rspace
            .lock()
            .unwrap()
            .get_history_repository()
            .importer()
            .get_history_item(hash)
    };

    if let Ok(Some(history_item)) = history_item {
        let history_item_proto = ByteVectorProto {
            byte_vector: history_item,
        };

        let mut bytes = history_item_proto.encode_to_vec();
        let len = bytes.len() as u32;
        let len_bytes = len.to_le_bytes().to_vec();
        let mut result = len_bytes;
        result.append(&mut bytes);
        let total_len = result.len();
        let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
        ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
        ptr
    } else {
        std::ptr::null()
    }
}

/* HistoryReader */
#[no_mangle]
pub extern "C" fn history_reader_root(
    rspace: *mut Space,
    state_hash_pointer: *const u8,
    state_hash_bytes_len: usize,
) -> *const u8 {
    let state_hash_slice =
        unsafe { std::slice::from_raw_parts(state_hash_pointer, state_hash_bytes_len) };
    let state_hash = Blake2b256Hash::from_bytes(state_hash_slice.to_vec());

    let root = unsafe {
        (*rspace)
            .rspace
            .lock()
            .unwrap()
            .get_history_repository()
            .get_history_reader(&state_hash)
            .unwrap()
            .root()
    };

    let hash = hash(&root);
    let hash_proto = HashProto { hash: hash.bytes() };

    let mut bytes = hash_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn get_history_data(
    rspace: *mut Space,
    payload_pointer: *const u8,
    state_hash_bytes_len: usize,
    key_bytes_len: usize,
) -> *const u8 {
    let payload_slice = unsafe {
        std::slice::from_raw_parts(payload_pointer, state_hash_bytes_len + key_bytes_len)
    };
    let (state_hash_slice, key_slice) = payload_slice.split_at(state_hash_bytes_len);

    let state_hash = Blake2b256Hash::from_bytes(state_hash_slice.to_vec());
    let key = Blake2b256Hash::from_bytes(key_slice.to_vec());

    let datums = unsafe {
        let space = (*rspace).rspace.lock().unwrap();
        space
            .get_history_repository()
            .get_history_reader(&state_hash)
            .unwrap()
            .get_data(&key)
            .unwrap()
    };

    let datums_protos: Vec<DatumProto> = datums
        .into_iter()
        .map(|datum| DatumProto {
            a: Some(std::sync::Arc::unwrap_or_clone(datum.a)),
            persist: datum.persist,
            source: Some(ProduceProto {
                channel_hash: datum.source.channel_hash.bytes(),
                hash: datum.source.hash.bytes(),
                persistent: datum.source.persistent,
                is_deterministic: datum.source.is_deterministic,
                output_value: datum.source.output_value,
            }),
        })
        .collect();

    let datums_proto = DatumsProto {
        datums: datums_protos,
    };

    let mut bytes = datums_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn get_history_waiting_continuations(
    rspace: *mut Space,
    payload_pointer: *const u8,
    state_hash_bytes_len: usize,
    key_bytes_len: usize,
) -> *const u8 {
    let payload_slice = unsafe {
        std::slice::from_raw_parts(payload_pointer, state_hash_bytes_len + key_bytes_len)
    };
    let (state_hash_slice, key_slice) = payload_slice.split_at(state_hash_bytes_len);

    let state_hash = Blake2b256Hash::from_bytes(state_hash_slice.to_vec());
    let key = Blake2b256Hash::from_bytes(key_slice.to_vec());

    let wks = unsafe {
        let space = (*rspace).rspace.lock().unwrap();
        space
            .get_history_repository()
            .get_history_reader(&state_hash)
            .unwrap()
            .get_continuations(&key)
            .unwrap()
    };

    let wks_protos: Vec<WaitingContinuationProto> = wks
        .into_iter()
        .map(|wk| {
            let res = WaitingContinuationProto {
                patterns: std::sync::Arc::unwrap_or_clone(wk.patterns),
                continuation: Some((*wk.continuation).clone()),
                persist: wk.persist,
                peeks: wk
                    .peeks
                    .into_iter()
                    .map(|peek| SortedSetElement { value: peek })
                    .collect(),
                source: Some(ConsumeProto {
                    channel_hashes: wk
                        .source
                        .channel_hashes
                        .iter()
                        .map(|hash| hash.bytes())
                        .collect(),
                    hash: wk.source.hash.bytes(),
                    persistent: wk.source.persistent,
                }),
            };

            res
        })
        .collect();

    let wks_proto = WaitingContinuationsProto { wks: wks_protos };

    let mut bytes = wks_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn get_history_joins(
    rspace: *mut Space,
    payload_pointer: *const u8,
    state_hash_bytes_len: usize,
    key_bytes_len: usize,
) -> *const u8 {
    let payload_slice = unsafe {
        std::slice::from_raw_parts(payload_pointer, state_hash_bytes_len + key_bytes_len)
    };
    let (state_hash_slice, key_slice) = payload_slice.split_at(state_hash_bytes_len);

    let state_hash = Blake2b256Hash::from_bytes(state_hash_slice.to_vec());
    let key = Blake2b256Hash::from_bytes(key_slice.to_vec());

    let joins = unsafe {
        let space = (*rspace).rspace.lock().unwrap();
        space
            .get_history_repository()
            .get_history_reader(&state_hash)
            .unwrap()
            .get_joins(&key)
            .unwrap()
    };

    let vec_join: Vec<JoinProto> = joins.into_iter().map(|join| JoinProto { join }).collect();
    let joins_proto = JoinsProto { joins: vec_join };

    let mut bytes = joins_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

/* ReplayRSpace */

#[no_mangle]
pub extern "C" fn replay_produce(
    rspace: *mut Space,
    payload_pointer: *const u8,
    channel_bytes_len: usize,
    data_bytes_len: usize,
    persist: bool,
) -> *const u8 {
    // println!("\nHit produce");

    let payload_slice =
        unsafe { std::slice::from_raw_parts(payload_pointer, channel_bytes_len + data_bytes_len) };
    let (channel_slice, data_slice) = payload_slice.split_at(channel_bytes_len);

    let channel = models::rust::rholang::protobuf_decoder::decode_par(channel_slice).unwrap();
    let data =
        models::rust::rholang::protobuf_decoder::decode_list_par_with_random(data_slice).unwrap();

    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    let result_option = blocking_runtime()
        .block_on(async { space.produce(channel, data, persist).await })
        .unwrap();

    match result_option {
        Some((cont_result, rspace_results, produce_event)) => {
            let protobuf_cont_result = ContResultProto {
                continuation: Some(cont_result.continuation.clone()),
                persistent: cont_result.persistent,
                channels: cont_result.channels,
                patterns: cont_result.patterns,
                peek: cont_result.peek,
            };

            let protobuf_results = rspace_results
                .into_iter()
                .map(|result| RSpaceResultProto {
                    channel: Some(result.channel),
                    matched_datum: Some(result.matched_datum),
                    removed_datum: Some(result.removed_datum),
                    persistent: result.persistent,
                })
                .collect();

            let protobuf_produce = ProduceProto {
                channel_hash: produce_event.channel_hash.bytes(),
                hash: produce_event.hash.bytes(),
                persistent: produce_event.persistent,
                is_deterministic: produce_event.is_deterministic,
                output_value: produce_event.output_value,
            };

            let maybe_action_result = ProduceResult {
                cont_result: Some(protobuf_cont_result),
                results: protobuf_results,
                produce: Some(protobuf_produce),
            };

            let mut bytes = maybe_action_result.encode_to_vec();
            let len = bytes.len() as u32;
            let len_bytes = len.to_le_bytes().to_vec();
            let mut result = len_bytes;
            result.append(&mut bytes);
            let total_len = result.len();
            let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
            ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
            ptr
        }
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn replay_consume(
    rspace: *mut Space,
    payload_pointer: *const u8,
    payload_bytes_len: usize,
) -> *const u8 {
    // println!("\nHit rust consume");

    // let thread_id = thread::current().id();
    // let current_time = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // println!("Thread ID: {:?}, Current Time: {}", thread_id, current_time);

    let payload_slice = unsafe { std::slice::from_raw_parts(payload_pointer, payload_bytes_len) };
    let consume_params = ConsumeParams::decode(payload_slice).unwrap();

    let channels = consume_params.channels;
    let patterns = consume_params.patterns;
    let continuation = consume_params.continuation.unwrap();
    let persist = consume_params.persist;
    let peeks = consume_params.peeks.into_iter().map(|e| e.value).collect();

    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    let result_option = blocking_runtime()
        .block_on(async {
            space
                .consume(channels, patterns, continuation, persist, peeks)
                .await
        })
        .unwrap();

    match result_option {
        Some((cont_result, rspace_results)) => {
            let protobuf_cont_result = ContResultProto {
                continuation: Some(cont_result.continuation.clone()),
                persistent: cont_result.persistent,
                channels: cont_result.channels,
                patterns: cont_result.patterns,
                peek: cont_result.peek,
            };

            let protobuf_results = rspace_results
                .into_iter()
                .map(|result| RSpaceResultProto {
                    channel: Some(result.channel),
                    matched_datum: Some(result.matched_datum),
                    removed_datum: Some(result.removed_datum),
                    persistent: result.persistent,
                })
                .collect();

            let maybe_action_result = ConsumeResult {
                cont_result: Some(protobuf_cont_result),
                results: protobuf_results,
            };

            let mut bytes = maybe_action_result.encode_to_vec();
            let len = bytes.len() as u32;
            let len_bytes = len.to_le_bytes().to_vec();
            let mut result = len_bytes;
            result.append(&mut bytes);

            // println!("\nlen: {:?}", len);
            let total_len = result.len();
            let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
            ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
            ptr
        }
        None => {
            // println!("\nnone in rust consume");
            std::ptr::null()
        }
    }
}

#[no_mangle]
pub extern "C" fn replay_create_checkpoint(rspace: *mut Space) -> *const u8 {
    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    let checkpoint = blocking_runtime().block_on(async {
        space
            .create_checkpoint()
            .await
            .expect("Rust RSpacePlusPlus Library: Failed to create checkpoint")
    });

    let log = checkpoint.log;
    let log_proto: Vec<EventProto> = log
        .into_iter()
        .map(|event| match event {
            Event::Comm(comm) => {
                let comm_proto = CommProto {
                    consume: {
                        Some(ConsumeProto {
                            channel_hashes: comm
                                .consume
                                .channel_hashes
                                .iter()
                                .map(|hash| hash.bytes())
                                .collect(),
                            hash: comm.consume.hash.bytes(),
                            persistent: comm.consume.persistent,
                        })
                    },
                    produces: {
                        comm.produces
                            .into_iter()
                            .map(|produce| ProduceProto {
                                channel_hash: produce.channel_hash.bytes(),
                                hash: produce.hash.bytes(),
                                persistent: produce.persistent,
                                is_deterministic: produce.is_deterministic,
                                output_value: produce.output_value,
                            })
                            .collect()
                    },
                    peeks: {
                        comm.peeks
                            .into_iter()
                            .map(|peek| SortedSetElement { value: peek })
                            .collect()
                    },
                    times_repeated: {
                        let mut produce_counter_map_entries: Vec<ProduceCounterMapEntry> =
                            Vec::new();
                        for (key, value) in comm.times_repeated {
                            let produce = ProduceProto {
                                channel_hash: key.channel_hash.bytes(),
                                hash: key.hash.bytes(),
                                persistent: key.persistent,
                                is_deterministic: key.is_deterministic,
                                output_value: key.output_value,
                            };

                            produce_counter_map_entries.push(ProduceCounterMapEntry {
                                key: Some(produce),
                                value,
                            });
                        }
                        produce_counter_map_entries
                    },
                };

                EventProto {
                    event_type: Some(event_proto::EventType::Comm(comm_proto)),
                }
            }
            Event::IoEvent(io_event) => match io_event {
                IOEvent::Produce(produce) => {
                    let produce_proto = ProduceProto {
                        channel_hash: produce.channel_hash.bytes(),
                        hash: produce.hash.bytes(),
                        persistent: produce.persistent,
                        is_deterministic: produce.is_deterministic,
                        output_value: produce.output_value,
                    };
                    EventProto {
                        event_type: Some(event_proto::EventType::IoEvent(IoEventProto {
                            io_event_type: Some(io_event_proto::IoEventType::Produce(
                                produce_proto,
                            )),
                        })),
                    }
                }
                IOEvent::Consume(consume) => {
                    let consume_proto = ConsumeProto {
                        channel_hashes: consume
                            .channel_hashes
                            .iter()
                            .map(|hash| hash.bytes())
                            .collect(),
                        hash: consume.hash.bytes(),
                        persistent: consume.persistent,
                    };
                    EventProto {
                        event_type: Some(event_proto::EventType::IoEvent(IoEventProto {
                            io_event_type: Some(io_event_proto::IoEventType::Consume(
                                consume_proto,
                            )),
                        })),
                    }
                }
            },
        })
        .collect();

    let checkpoint_proto = CheckpointProto {
        root: checkpoint.root.bytes(),
        log: log_proto,
    };

    let mut bytes = checkpoint_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn replay_clear(rspace: *mut Space) {
    let space = unsafe { (*rspace).rspace.lock().unwrap() };
    blocking_runtime().block_on(async {
        space
            .clear()
            .await
            .expect("Rust RSpacePlusPlus Library: Failed to clear")
    });
}

#[no_mangle]
pub extern "C" fn replay_spawn(replay_rspace_ptr: *mut ReplaySpace) -> *mut ReplaySpace {
    let replay_space = unsafe { (*replay_rspace_ptr).replay_space.lock().unwrap() };
    let new_replay_space = replay_space
        .spawn()
        .expect("Rust RSpacePlusPlus Library: Failed to spawn");

    Box::into_raw(Box::new(ReplaySpace {
        replay_space: Mutex::new(new_replay_space),
    }))
}

/* IReplayRSpace */

// #[no_mangle]
// pub extern "C" fn rig(rspace: *mut Space, log_pointer: *const u8,
// log_bytes_len: usize) -> () {     let log_slice = unsafe {
// std::slice::from_raw_parts(log_pointer, log_bytes_len) };     let log_proto =
// LogProto::decode(log_slice).unwrap();

//     let log: Vec<Event> = log_proto
//         .log
//         .into_iter()
//         .map(|log_entry| match log_entry.event_type.unwrap() {
//             event_proto::EventType::Comm(comm_proto) => {
//                 let consume_proto = comm_proto.consume.unwrap();
//                 let comm = COMM {
//                     consume: {
//                         Consume {
//                             channel_hashes: {
//                                 consume_proto
//                                     .channel_hashes
//                                     .iter()
//                                     .map(|hash|
// Blake2b256Hash::from_bytes(hash.clone()))
// .collect()                             },
//                             hash:
// Blake2b256Hash::from_bytes(consume_proto.hash),
// persistent: consume_proto.persistent,                         }
//                     },
//                     produces: {
//                         comm_proto
//                             .produces
//                             .into_iter()
//                             .map(|produce_proto| Produce {
//                                 channel_hash: Blake2b256Hash::from_bytes(
//                                     produce_proto.channel_hash,
//                                 ),
//                                 hash:
// Blake2b256Hash::from_bytes(produce_proto.hash),
// persistent: produce_proto.persistent,                             })
//                             .collect()
//                     },
//                     peeks: {
//                         comm_proto
//                             .peeks
//                             .iter()
//                             .map(|element| element.value)
//                             .collect()
//                     },
//                     times_repeated: {
//                         comm_proto
//                             .times_repeated
//                             .into_iter()
//                             .map(|map_entry| {
//                                 let key_proto = map_entry.key.unwrap();
//                                 let produce = Produce {
//                                     channel_hash: Blake2b256Hash::from_bytes(
//                                         key_proto.channel_hash,
//                                     ),
//                                     hash:
// Blake2b256Hash::from_bytes(key_proto.hash),
// persistent: key_proto.persistent,                                 };

//                                 let value = map_entry.value;

//                                 (produce, value)
//                             })
//                             .collect()
//                     },
//                 };
//                 Event::Comm(comm)
//             }
//             event_proto::EventType::IoEvent(io_event) => match
// io_event.io_event_type.unwrap() {
// io_event_proto::IoEventType::Produce(produce_proto) => {
// let produce = Produce {                         channel_hash:
// Blake2b256Hash::from_bytes(produce_proto.channel_hash),
// hash: Blake2b256Hash::from_bytes(produce_proto.hash),
// persistent: produce_proto.persistent,                     };
//                     Event::IoEvent(IOEvent::Produce(produce))
//                 }
//                 io_event_proto::IoEventType::Consume(consume_proto) => {
//                     let consume = Consume {
//                         channel_hashes: {
//                             consume_proto
//                                 .channel_hashes
//                                 .iter()
//                                 .map(|hash|
// Blake2b256Hash::from_bytes(hash.clone()))
// .collect()                         },
//                         hash: Blake2b256Hash::from_bytes(consume_proto.hash),
//                         persistent: consume_proto.persistent,
//                     };
//                     Event::IoEvent(IOEvent::Consume(consume))
//                 }
//             },
//         })
//         .collect();

//     unsafe {
//         (*rspace).rspace.lock().unwrap().rig(log).unwrap();
//     }
// }

// #[no_mangle]
// pub extern "C" fn check_replay_data(rspace: *mut Space) -> () {
//     unsafe {
//         (*rspace)
//             .rspace
//             .lock()
//             .unwrap()
//             .check_replay_data()
//             .unwrap();
//     }
// }

/* Helper Functions */
#[no_mangle]
pub extern "C" fn hash_channel(channel_pointer: *const u8, channel_bytes_len: usize) -> *const u8 {
    let channel_slice = unsafe { std::slice::from_raw_parts(channel_pointer, channel_bytes_len) };
    let channel = models::rust::rholang::protobuf_decoder::decode_par(channel_slice).unwrap();

    let hash = hash(&channel);
    let hash_proto = HashProto { hash: hash.bytes() };

    let mut bytes = hash_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn hash_channels(
    channels_pointer: *const u8,
    channels_bytes_len: usize,
) -> *const u8 {
    let channels_slice =
        unsafe { std::slice::from_raw_parts(channels_pointer, channels_bytes_len) };
    let channels_proto = ChannelsProto::decode(channels_slice).unwrap();

    let hash = hash_from_vec(&channels_proto.channels);
    let hash_proto = HashProto { hash: hash.bytes() };

    let mut bytes = hash_proto.encode_to_vec();
    let len = bytes.len() as u32;
    let len_bytes = len.to_le_bytes().to_vec();
    let mut result = len_bytes;
    result.append(&mut bytes);
    let total_len = result.len();
    let ptr = Box::leak(result.into_boxed_slice()).as_ptr();
    ALLOCATED_BYTES.fetch_add(total_len, Ordering::SeqCst);
    ptr
}

#[no_mangle]
pub extern "C" fn deallocate_memory(ptr: *mut u8, len: usize) {
    // SAFETY: The caller must guarantee that `ptr` is a valid pointer to a memory
    // block allocated by Rust, and that `len` is the correct size of the block.
    unsafe {
        // Convert the raw pointer back to a Box to allow Rust to deallocate the memory.
        let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len));
        // The Box goes out of scope here, and Rust automatically deallocates
        // the memory.
    }
    ALLOCATED_BYTES.fetch_sub(len, Ordering::SeqCst);
}

#[cfg(test)]
#[path = "../tests/affine_state_import_ffi/mod.rs"]
mod affine_state_import_ffi_tests;
