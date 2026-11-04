use std::collections::{BTreeMap, HashSet};

use models::rust::casper::protocol::casper_message::Event;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsEndVal;

pub const READ: u32 = 1;
pub const APPEND: u32 = 2;
pub const JOIN: u32 = 4;
pub const WRITE: u32 = 8;
pub const MERGE: u32 = 16;
const ACCESSES: u32 = READ | APPEND | JOIN | WRITE | MERGE;

fn clashes_with(access: u32) -> u32 {
    match access {
        READ => WRITE | MERGE,
        APPEND => JOIN | WRITE | MERGE,
        JOIN => APPEND | WRITE | MERGE,
        WRITE => READ | APPEND | JOIN | WRITE | MERGE,
        MERGE => READ | APPEND | JOIN | WRITE,
        _ => 0,
    }
}

fn clash(a: u32, b: u32) -> bool {
    [READ, APPEND, JOIN, WRITE, MERGE]
        .into_iter()
        .any(|access| a & access != 0 && clashes_with(access) & b != 0)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footprint {
    cells: Vec<(Bytes, u32)>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct FootprintCellProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub channel: Bytes,
    #[prost(uint32, tag = "2")]
    pub access: u32,
}

impl Footprint {
    pub fn cells(&self) -> &[(Bytes, u32)] { &self.cells }

    pub fn is_empty(&self) -> bool { self.cells.is_empty() }

    pub fn conflicts_with(&self, other: &Footprint) -> bool {
        let (mut i, mut j) = (0, 0);
        while i < self.cells.len() && j < other.cells.len() {
            let (a, b) = (&self.cells[i], &other.cells[j]);
            match a.0.cmp(&b.0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    if clash(a.1, b.1) {
                        return true;
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
        false
    }

    pub fn to_proto(&self) -> Vec<FootprintCellProto> {
        self.cells
            .iter()
            .map(|(channel, access)| FootprintCellProto {
                channel: channel.clone(),
                access: *access,
            })
            .collect()
    }

    pub fn from_proto(cells: Vec<FootprintCellProto>) -> Result<Self, String> {
        let cells: Vec<(Bytes, u32)> = cells.into_iter().map(|c| (c.channel, c.access)).collect();
        for (channel, access) in &cells {
            if channel.len() != 32 {
                return Err("a footprint channel is 32 bytes".into());
            }
            if *access == 0 || access & !ACCESSES != 0 {
                return Err(format!("unknown footprint access {access}"));
            }
            if access & MERGE != 0 && *access != MERGE {
                return Err("a mergeable channel has no other access".into());
            }
        }
        if cells.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err("footprint channels are sorted and distinct".into());
        }
        Ok(Footprint { cells })
    }
}

pub fn footprint<E>(
    log: &[Event],
    number_channels: &NumberChannelsEndVal,
    base_joins: impl Fn(&Bytes) -> Result<Vec<Vec<Bytes>>, E>,
) -> Result<Footprint, E> {
    let mut created_produces: HashSet<&Bytes> = HashSet::new();
    let mut created_consumes: HashSet<&Bytes> = HashSet::new();
    for event in log {
        match event {
            Event::Produce(p) => {
                created_produces.insert(&p.hash);
            }
            Event::Consume(c) => {
                created_consumes.insert(&c.hash);
            }
            Event::Comm(_) | Event::Refusal(_) => {}
        }
    }
    let mut cells: BTreeMap<Bytes, u32> = BTreeMap::new();
    let mut mark = |channel: &Bytes, access: u32| {
        *cells.entry(channel.clone()).or_insert(0) |= access;
    };
    let mut destroyed_produces: HashSet<&Bytes> = HashSet::new();
    let mut destroyed_consumes: HashSet<&Bytes> = HashSet::new();
    for event in log {
        let Event::Comm(comm) = event else { continue };
        let consume = &comm.consume;
        if created_consumes.contains(&consume.hash) {
            if !consume.persistent {
                destroyed_consumes.insert(&consume.hash);
            }
        } else {
            let access = if consume.persistent { READ } else { WRITE };
            consume.channels_hashes.iter().for_each(|ch| mark(ch, access));
        }
        for (index, produce) in comm.produces.iter().enumerate() {
            let peeked = comm.peeks.iter().any(|p| p.channel_index as usize == index);
            let kept = peeked || produce.persistent;
            if created_produces.contains(&produce.hash) {
                if !kept {
                    destroyed_produces.insert(&produce.hash);
                }
            } else {
                mark(&produce.channels_hash, if kept { READ } else { WRITE });
            }
        }
    }
    let mut appended: Vec<&Bytes> = Vec::new();
    for event in log {
        match event {
            Event::Produce(p) if !destroyed_produces.contains(&p.hash) => {
                mark(&p.channels_hash, APPEND);
                appended.push(&p.channels_hash);
            }
            Event::Consume(c) if !destroyed_consumes.contains(&c.hash) => {
                c.channels_hashes.iter().for_each(|ch| mark(ch, WRITE));
            }
            _ => {}
        }
    }
    appended.sort();
    appended.dedup();
    for channel in appended {
        for join in base_joins(channel)? {
            if join.len() > 1 {
                join.iter()
                    .filter(|other| *other != channel)
                    .for_each(|other| mark(other, JOIN));
            }
        }
    }
    for channel in number_channels.keys() {
        cells.insert(Bytes::from(channel.bytes()), MERGE);
    }
    Ok(Footprint {
        cells: cells.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::{
        CommEvent, ConsumeEvent, Peek, ProduceEvent,
    };
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

    use super::*;

    fn ch(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }

    fn produce(channel: u8, id: u8, persistent: bool) -> ProduceEvent {
        ProduceEvent {
            channels_hash: ch(channel),
            hash: Bytes::from(vec![id, 0x50]),
            persistent,
            times_repeated: 0,
            is_deterministic: true,
            output_value: Vec::new(),
            failed: false,
        }
    }

    fn consume(channels: &[u8], id: u8, persistent: bool) -> ConsumeEvent {
        ConsumeEvent {
            channels_hashes: channels.iter().map(|c| ch(*c)).collect(),
            hash: Bytes::from(vec![id, 0xC0]),
            persistent,
        }
    }

    fn comm(c: ConsumeEvent, ps: Vec<ProduceEvent>, peeks: &[i32]) -> Event {
        Event::Comm(CommEvent {
            consume: c,
            produces: ps,
            peeks: peeks.iter().map(|i| Peek { channel_index: *i }).collect(),
        })
    }

    fn no_joins(_: &Bytes) -> Result<Vec<Vec<Bytes>>, ()> { Ok(Vec::new()) }

    fn fp(log: &[Event]) -> Footprint {
        footprint(log, &NumberChannelsEndVal::new(), no_joins).unwrap()
    }

    fn access(f: &Footprint, channel: u8) -> u32 {
        f.cells()
            .iter()
            .find(|(c, _)| *c == ch(channel))
            .map(|(_, a)| *a)
            .unwrap_or(0)
    }

    #[test]
    fn a_send_left_in_the_space_appends() {
        let f = fp(&[Event::Produce(produce(1, 1, false))]);
        assert_eq!(f.cells(), &[(ch(1), APPEND)]);
    }

    #[test]
    fn a_call_to_a_contract_in_the_pre_state_reads_it() {
        let p = produce(2, 1, false);
        let f = fp(&[Event::Produce(p.clone()), comm(consume(&[2], 9, true), vec![p], &[])]);
        assert_eq!(f.cells(), &[(ch(2), READ)]);
    }

    #[test]
    fn a_message_and_its_receiver_made_in_the_run_leave_nothing() {
        let p = produce(3, 1, false);
        let c = consume(&[3], 2, false);
        let f = fp(&[
            Event::Produce(p.clone()),
            Event::Consume(c.clone()),
            comm(c, vec![p], &[]),
        ]);
        assert!(f.is_empty());
    }

    #[test]
    fn taking_a_message_from_the_pre_state_writes() {
        let c = consume(&[4], 2, false);
        let f = fp(&[Event::Consume(c.clone()), comm(c, vec![produce(4, 7, false)], &[])]);
        assert_eq!(access(&f, 4), WRITE);
    }

    #[test]
    fn a_peek_at_the_pre_state_reads() {
        let c = consume(&[5], 2, false);
        let f = fp(&[Event::Consume(c.clone()), comm(c, vec![produce(5, 7, false)], &[0])]);
        assert_eq!(access(&f, 5), READ);
    }

    #[test]
    fn a_waiting_receiver_writes_every_channel_of_its_join() {
        let f = fp(&[Event::Consume(consume(&[6, 7], 2, false))]);
        assert_eq!(f.cells(), &[(ch(6), WRITE), (ch(7), WRITE)]);
    }

    #[test]
    fn a_send_into_a_join_of_the_pre_state_marks_the_other_channels() {
        let joins = |c: &Bytes| -> Result<Vec<Vec<Bytes>>, ()> {
            Ok(if *c == ch(8) { vec![vec![ch(8), ch(9)], vec![ch(8)]] } else { vec![] })
        };
        let f = footprint(&[Event::Produce(produce(8, 1, false))], &NumberChannelsEndVal::new(), joins)
            .unwrap();
        assert_eq!(f.cells(), &[(ch(8), APPEND), (ch(9), JOIN)]);
        let other = fp(&[Event::Produce(produce(9, 2, false))]);
        assert!(f.conflicts_with(&other));
        assert!(other.conflicts_with(&f));
    }

    #[test]
    fn mergeable_channels_merge_only() {
        let mut numbers = NumberChannelsEndVal::new();
        numbers.insert(
            Blake2b256Hash::from_bytes(ch(10).to_vec()),
            (1.into(), MergeType::IntegerAdd),
        );
        let f = footprint(&[Event::Produce(produce(10, 1, false))], &numbers, no_joins).unwrap();
        assert_eq!(f.cells(), &[(ch(10), MERGE)]);
        assert!(!f.conflicts_with(&f.clone()));
        assert!(f.conflicts_with(&fp(&[Event::Produce(produce(10, 2, false))])));
    }

    #[test]
    fn the_conflict_relation() {
        let all = [READ, APPEND, JOIN, WRITE, MERGE];
        let commuting = [(READ, READ), (READ, APPEND), (READ, JOIN), (APPEND, APPEND), (JOIN, JOIN), (MERGE, MERGE)];
        for a in all {
            for b in all {
                let expected = !commuting.contains(&(a, b)) && !commuting.contains(&(b, a));
                assert_eq!(clash(a, b), expected, "{a} against {b}");
                let fa = Footprint { cells: vec![(ch(1), a)] };
                let fb = Footprint { cells: vec![(ch(1), b)] };
                assert_eq!(fa.conflicts_with(&fb), expected);
                let elsewhere = Footprint { cells: vec![(ch(2), b)] };
                assert!(!fa.conflicts_with(&elsewhere));
            }
        }
    }

    #[test]
    fn the_encoding_is_canonical() {
        let f = Footprint { cells: vec![(ch(1), READ | APPEND), (ch(2), MERGE)] };
        assert_eq!(Footprint::from_proto(f.to_proto()), Ok(f));
        let unsorted = vec![
            FootprintCellProto { channel: ch(2), access: READ },
            FootprintCellProto { channel: ch(1), access: READ },
        ];
        assert!(Footprint::from_proto(unsorted).is_err());
        let unknown = vec![FootprintCellProto { channel: ch(1), access: 32 }];
        assert!(Footprint::from_proto(unknown).is_err());
        let mixed = vec![FootprintCellProto { channel: ch(1), access: MERGE | READ }];
        assert!(Footprint::from_proto(mixed).is_err());
        let short = vec![FootprintCellProto { channel: Bytes::from_static(b"x"), access: READ }];
        assert!(Footprint::from_proto(short).is_err());
    }
}
