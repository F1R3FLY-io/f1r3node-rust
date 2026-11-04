use models::rust::casper::protocol::casper_message::{
    Event as LogEvent, ProcessedDeploy, ProcessedSystemDeploy,
};
use rholang::rust::interpreter::accounting::stops::{check_stop, Refusal, ReplayStops, StopCheck};
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

use std::collections::BTreeMap;

use rholang::rust::interpreter::accounting::stops::Membrane;

use super::surface::{BlockPurses, Surfaces};
use crate::rust::util::event_converter::to_rspace_event;

pub fn refusals_in(_events: &[LogEvent]) -> Vec<Refusal> { Vec::new() }

pub fn refusals_of(processed: &ProcessedDeploy) -> Vec<Refusal> { refusals_in(&processed.deploy_log) }

fn system_events(deploy: &ProcessedSystemDeploy) -> &[LogEvent] {
    match deploy {
        ProcessedSystemDeploy::Succeeded { event_list, .. } => event_list,
        ProcessedSystemDeploy::Failed { event_list, .. } => event_list,
    }
}

pub fn system_refusals(deploys: &[ProcessedSystemDeploy]) -> Vec<Refusal> {
    deploys.iter().flat_map(|d| refusals_in(system_events(d))).collect()
}

pub fn install_system(deploys: &[ProcessedSystemDeploy]) -> Option<(ReplayStops, Vec<Refusal>)> {
    let refusals = system_refusals(deploys);
    if refusals.is_empty() {
        return None;
    }
    let logged = deploys.iter().flat_map(|d| logged_io_in(system_events(d))).collect::<Vec<_>>();
    Some((ReplayStops::new(&refusals, logged), refusals))
}

pub fn logged_io(processed: &ProcessedDeploy) -> Vec<Vec<u8>> { logged_io_in(&processed.deploy_log) }

pub fn logged_io_in(events: &[LogEvent]) -> Vec<Vec<u8>> {
    let mut hashes = Vec::new();
    for event in events.iter().map(to_rspace_event) {
        match event {
            Event::IoEvent(IOEvent::Produce(produce)) => hashes.push(produce.hash.bytes()),
            Event::IoEvent(IOEvent::Consume(consume)) => hashes.push(consume.hash.bytes()),
            Event::Comm(comm) => {
                hashes.push(comm.consume.hash.bytes());
                hashes.extend(comm.produces.iter().map(|produce| produce.hash.bytes()));
            }
        }
    }
    hashes
}

pub fn install(processed: &ProcessedDeploy) -> Option<(ReplayStops, Vec<Refusal>)> {
    let refusals = refusals_of(processed);
    if refusals.is_empty() {
        return None;
    }
    let stops = ReplayStops::new(&refusals, logged_io(processed));
    Some((stops, refusals))
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopFailure {
    ClientMembrane,
    NoPurse,
    Check(StopCheck),
    NoRecordedWork,
    WorkNotSettled { recorded: u64, expected: u64 },
}

pub fn settled_work(
    refusals: &[Refusal],
    replayed: Surfaces,
    recorded: Option<&Surfaces>,
    refunds: &dyn Fn(&Membrane) -> u64,
) -> Result<Surfaces, StopFailure> {
    if refusals.is_empty() {
        return Ok(replayed);
    }
    let recorded = recorded.ok_or(StopFailure::NoRecordedWork)?;
    let mut drawn: BTreeMap<Membrane, u64> = BTreeMap::new();
    for refusal in refusals {
        drawn
            .entry(refusal.membrane.clone())
            .and_modify(|d| *d = (*d).min(refusal.drawn))
            .or_insert(refusal.drawn);
    }
    let mut work: BTreeMap<Membrane, u64> = replayed
        .into_iter()
        .filter(|(membrane, _)| !drawn.contains_key(membrane))
        .collect();
    for (membrane, drawn) in &drawn {
        let net = recorded
            .iter()
            .find(|(m, _)| m == membrane)
            .map_or(0, |(_, w)| *w);
        let expected = drawn.saturating_sub(refunds(membrane));
        if net != expected {
            return Err(StopFailure::WorkNotSettled {
                recorded: net,
                expected,
            });
        }
        if net > 0 {
            work.insert(membrane.clone(), net);
        }
    }
    Ok(work.into_iter().collect())
}

pub fn check(
    refusals: &[Refusal],
    stops: &ReplayStops,
    purses: &BlockPurses,
    own_envelope: &[u8],
) -> Result<(), StopFailure> {
    for refusal in refusals {
        let surface = match &refusal.membrane {
            Some(surface) if surface.envelope.as_slice() != own_envelope => surface,
            _ => return Err(StopFailure::ClientMembrane),
        };
        let available = purses.available(surface).ok_or(StopFailure::NoPurse)?;
        check_stop(
            refusal,
            available,
            stops.gross(&refusal.membrane),
            stops.found(&refusal.step),
        )
        .map_err(StopFailure::Check)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::accounting::comm_ledger::Surface;

    use super::*;

    fn membrane(owner: u8) -> Membrane {
        Some(Surface {
            owner: vec![owner],
            envelope: vec![9],
            phlo_price: 1,
        })
    }

    fn refusal(owner: u8, drawn: u64) -> Refusal {
        Refusal {
            membrane: membrane(owner),
            step: vec![owner],
            drawn,
            cost: 10,
        }
    }

    fn no_refunds(_: &Membrane) -> u64 { 0 }

    #[test]
    fn without_a_stop_the_replayed_work_stands() {
        let replayed = vec![(None, 30), (membrane(1), 40)];
        assert_eq!(settled_work(&[], replayed.clone(), None, &no_refunds), Ok(replayed));
    }

    #[test]
    fn a_stopped_membrane_settles_the_recorded_work() {
        let replayed = vec![(None, 30), (membrane(1), 120), (membrane(2), 5)];
        let recorded = vec![(None, 30), (membrane(1), 95), (membrane(2), 5)];
        assert_eq!(
            settled_work(&[refusal(1, 95)], replayed, Some(&recorded), &no_refunds),
            Ok(vec![(None, 30), (membrane(1), 95), (membrane(2), 5)])
        );
    }

    #[test]
    fn the_recorded_work_is_the_drawn_amount_less_the_refunds() {
        let recorded = vec![(membrane(1), 80)];
        let refunds = |m: &Membrane| if *m == membrane(1) { 15 } else { 0 };
        assert_eq!(
            settled_work(&[refusal(1, 95)], vec![(membrane(1), 130)], Some(&recorded), &refunds),
            Ok(vec![(membrane(1), 80)])
        );
    }

    #[test]
    fn recorded_work_below_the_drawn_amount_is_refused() {
        let recorded = vec![(membrane(1), 60)];
        assert_eq!(
            settled_work(&[refusal(1, 95)], vec![(membrane(1), 95)], Some(&recorded), &no_refunds),
            Err(StopFailure::WorkNotSettled {
                recorded: 60,
                expected: 95
            })
        );
    }

    #[test]
    fn recorded_work_above_the_drawn_amount_is_refused() {
        let recorded = vec![(membrane(1), 96)];
        assert_eq!(
            settled_work(&[refusal(1, 95)], vec![(membrane(1), 96)], Some(&recorded), &no_refunds),
            Err(StopFailure::WorkNotSettled {
                recorded: 96,
                expected: 95
            })
        );
    }

    #[test]
    fn the_least_drawn_amount_of_a_membrane_is_used() {
        let recorded = vec![(membrane(1), 85)];
        assert_eq!(
            settled_work(&[refusal(1, 95), refusal(1, 85)], Vec::new(), Some(&recorded), &no_refunds),
            Ok(vec![(membrane(1), 85)])
        );
    }

    #[test]
    fn refunds_at_or_above_the_drawn_amount_settle_to_nothing() {
        let refunds = |_: &Membrane| 120;
        assert_eq!(
            settled_work(&[refusal(1, 95)], vec![(membrane(1), 40)], Some(&Vec::new()), &refunds),
            Ok(Vec::new())
        );
        assert_eq!(
            settled_work(&[refusal(1, 95)], Vec::new(), Some(&vec![(membrane(1), 1)]), &refunds),
            Err(StopFailure::WorkNotSettled {
                recorded: 1,
                expected: 0
            })
        );
    }

    #[test]
    fn a_stop_without_the_recorded_work_is_refused() {
        assert_eq!(
            settled_work(&[refusal(1, 95)], Vec::new(), None, &no_refunds),
            Err(StopFailure::NoRecordedWork)
        );
    }
}
