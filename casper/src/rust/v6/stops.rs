use models::rust::casper::protocol::casper_message::ProcessedDeploy;
use rholang::rust::interpreter::accounting::stops::{check_stop, Refusal, ReplayStops, StopCheck};
use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

use super::surface::BlockPurses;
use crate::rust::util::event_converter::to_rspace_event;

pub fn refusals_of(_processed: &ProcessedDeploy) -> Vec<Refusal> { Vec::new() }

pub fn logged_io(processed: &ProcessedDeploy) -> Vec<Vec<u8>> {
    let mut hashes = Vec::new();
    for event in processed.deploy_log.iter().map(to_rspace_event) {
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
