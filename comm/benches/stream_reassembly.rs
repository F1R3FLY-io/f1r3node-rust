use std::hint::black_box;
use std::time::{Duration, Instant};

use dashmap::DashMap;

const CHUNK_SIZE: usize = 256 * 1024 - 2 * 1024;

fn current(chunks: &[Vec<u8>]) -> usize {
    let cache = DashMap::<String, Vec<u8>>::new();
    let key = "packet_send/bench".to_string();
    cache.insert(key.clone(), Vec::new());

    for chunk in chunks {
        let received_bytes = chunk.to_vec();
        let mut existing_data = cache
            .get(&key)
            .map(|entry| entry.value().clone())
            .unwrap_or_default();
        existing_data.extend_from_slice(&received_bytes);
        cache.insert(key.clone(), existing_data);
    }

    let len = cache.get(&key).unwrap().len();
    black_box(len)
}

fn in_place(chunks: &[Vec<u8>]) -> usize {
    let cache = DashMap::<String, Vec<u8>>::new();
    let key = "packet_send/bench".to_string();
    cache.insert(key.clone(), Vec::new());

    for chunk in chunks {
        cache
            .entry(key.clone())
            .or_default()
            .value_mut()
            .extend_from_slice(chunk);
    }

    let len = cache.get(&key).unwrap().len();
    black_box(len)
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort_unstable();
    times[times.len() / 2]
}

fn measure(f: fn(&[Vec<u8>]) -> usize, chunks: &[Vec<u8>], repeats: u32, size: usize) -> Duration {
    let start = Instant::now();
    for _ in 0..repeats {
        assert_eq!(f(black_box(chunks)), size);
    }
    start.elapsed() / repeats
}

fn main() {
    for (size, repeats) in [
        (1024 * 1024, 100),
        (8 * 1024 * 1024, 20),
        (16 * 1024 * 1024, 10),
    ] {
        let payload = vec![0x5a; size];
        let chunks: Vec<Vec<u8>> = payload
            .chunks(CHUNK_SIZE)
            .map(|chunk| chunk.to_vec())
            .collect();
        let mut current_times = Vec::new();
        let mut in_place_times = Vec::new();

        for round in 0..7 {
            if round % 2 == 0 {
                in_place_times.push(measure(in_place, &chunks, repeats, size));
                current_times.push(measure(current, &chunks, repeats, size));
            } else {
                current_times.push(measure(current, &chunks, repeats, size));
                in_place_times.push(measure(in_place, &chunks, repeats, size));
            }
        }

        let old = median(current_times);
        let new = median(in_place_times);
        println!(
            "size={}MiB chunks={} current={:.3}ms in_place={:.3}ms ratio={:.1}x",
            size / (1024 * 1024),
            chunks.len(),
            old.as_secs_f64() * 1000.0,
            new.as_secs_f64() * 1000.0,
            old.as_secs_f64() / new.as_secs_f64()
        );
    }
}
