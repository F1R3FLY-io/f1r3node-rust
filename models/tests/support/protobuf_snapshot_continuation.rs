use prost::Message;

use super::*;
use crate::rust::epathmap_trie_codec::payload_mapping_tests::capture_allocations;
use crate::rust::utils::new_gint_par;

fn snapshot(count: usize) -> Vec<u8> {
    crate::rhoapi::EPathMap::new_map(
        (0..count).map(|index| {
            (
                new_gint_par(index as i64, Vec::new(), false),
                new_gint_par(index as i64 + 1, Vec::new(), false),
            )
        }),
        Vec::new(),
        false,
        None,
    )
    .encode_to_vec()
}

fn machine(bytes: &[u8]) -> Machine<&[u8]> {
    Machine::new(
        bytes,
        Node::EPathMap {
            value: Default::default(),
            limit: 0,
        },
        true,
    )
}

fn first_snapshot(machine: &mut Machine<&[u8]>) -> Box<EpmFrame> {
    loop {
        match machine.frames.pop().unwrap() {
            Frame::Epm(frame) => return frame,
            Frame::Node(node) => machine.step_node(node).unwrap(),
            Frame::Attach(attach) => machine.attach(attach).unwrap(),
        }
    }
}

#[test]
fn snapshot_payload_does_not_enlarge_common_frame_slots() {
    assert_eq!(
        std::mem::size_of::<Box<EpmFrame>>(),
        std::mem::size_of::<usize>()
    );
    assert!(std::mem::size_of::<Frame>() < std::mem::size_of::<EpmFrame>());
    assert!(std::mem::size_of::<Attach>() < std::mem::size_of::<EpmFrame>());
    eprintln!(
        "snapshot continuation layout: frame={} attach={} node={} snapshot={}",
        std::mem::size_of::<Frame>(),
        std::mem::size_of::<Attach>(),
        std::mem::size_of::<Node>(),
        std::mem::size_of::<EpmFrame>()
    );
}

#[test]
fn repeated_suspend_resume_moves_one_owner_without_allocating() {
    let bytes = snapshot(1);
    let mut machine = machine(&bytes);
    let frame = first_snapshot(&mut machine);
    let address = &*frame as *const EpmFrame;
    let (frame, counts) = capture_allocations(|| {
        let mut frame = frame;
        for _ in 0..4096 {
            let work = Frame::Epm(frame);
            let Frame::Epm(value) = std::hint::black_box(work) else {
                unreachable!()
            };
            let attach = Attach::EPathMapSnapshotValue { frame: value };
            let Attach::EPathMapSnapshotValue { frame: value } = std::hint::black_box(attach)
            else {
                unreachable!()
            };
            frame = value;
            assert_eq!(&*frame as *const EpmFrame, address);
        }
        frame
    });
    assert_eq!(counts.allocations, 0);
    assert_eq!(counts.freed, 0);
    assert!(!counts.overflow);
    machine.frames.push(Frame::Epm(frame));
    machine.run().unwrap();
}

#[test]
fn actual_multi_value_decoder_reuses_snapshot_box_and_restores_input() {
    for count in [0, 1, 16, 256] {
        let bytes = snapshot(count);
        let mut machine = machine(&bytes);
        let mut address = None;
        let mut visits = 0;
        let mut attaches = 0;
        while let Some(frame) = machine.frames.pop() {
            match frame {
                Frame::Node(node) => machine.step_node(node).unwrap(),
                Frame::Attach(attach) => {
                    if let Attach::EPathMapSnapshotValue { frame } = &attach {
                        assert_eq!(address, Some(&**frame as *const EpmFrame));
                        attaches += 1;
                    }
                    machine.attach(attach).unwrap();
                }
                Frame::Epm(frame) => {
                    let current = &*frame as *const EpmFrame;
                    if let Some(address) = address {
                        assert_eq!(address, current);
                    }
                    address = Some(current);
                    visits += 1;
                    machine.step_epm_snapshot(frame).unwrap();
                }
            }
        }
        assert_eq!(visits, count + 1);
        assert_eq!(attaches, count);
        assert_eq!(machine.epm_depth, 0);
        assert!(machine.input_stack.is_empty());
        assert_eq!(machine.buf.remaining(), 0);
        assert_eq!(machine.values.len(), 1);
        machine.dismantle();
        assert!(machine.is_fully_drained());
    }
}
