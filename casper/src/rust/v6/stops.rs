use models::rust::casper::protocol::casper_message::{
    Event as LogEvent, ProcessedDeploy, ProcessedSystemDeploy,
};
use rholang::rust::interpreter::accounting::stops::{check_stop, Refusal, ReplayStops, StopCheck};
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

use super::surface::BlockPurses;
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

