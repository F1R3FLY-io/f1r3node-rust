use std::collections::BTreeMap;

use models::rust::casper::protocol::casper_message::{
    Event as LogEvent, ProcessedDeploy, ProcessedSystemDeploy, RefusalEvent,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::comm_ledger::Surface;
use rholang::rust::interpreter::accounting::membrane::{Refusal, RefusalKind};

use super::surface::Surfaces;

pub type Membrane = Option<Surface>;

pub fn to_event(refusal: &Refusal) -> LogEvent {
    LogEvent::Refusal(RefusalEvent {
        owner: Bytes::copy_from_slice(&refusal.surface.owner),
        envelope: Bytes::copy_from_slice(&refusal.surface.envelope),
        phlo_price: refusal.surface.phlo_price,
        step: Bytes::copy_from_slice(&refusal.step),
        kind: refusal.kind.tag(),
        drawn: refusal.drawn,
        charge: refusal.charge,
        surface_drawn: refusal.surface_drawn,
    })
}

pub fn refusals_in(events: &[LogEvent]) -> Result<Vec<Refusal>, String> {
    events
        .iter()
        .filter_map(|event| match event {
            LogEvent::Refusal(r) => Some(r),
            _ => None,
        })
        .map(|r| {
            Ok(Refusal {
                surface: Surface {
                    owner: r.owner.to_vec(),
                    envelope: r.envelope.to_vec(),
                    phlo_price: r.phlo_price,
                },
                step: r.step.to_vec(),
                kind: RefusalKind::from_tag(r.kind)
                    .ok_or_else(|| format!("unknown refusal kind {}", r.kind))?,
                drawn: r.drawn,
                charge: r.charge,
                surface_drawn: r.surface_drawn,
            })
        })
        .collect()
}

pub fn refusals_of(processed: &ProcessedDeploy) -> Result<Vec<Refusal>, String> {
    refusals_in(&processed.deploy_log)
}

pub fn system_events(deploy: &ProcessedSystemDeploy) -> &[LogEvent] {
    match deploy {
        ProcessedSystemDeploy::Succeeded { event_list, .. } => event_list,
        ProcessedSystemDeploy::Failed { event_list, .. } => event_list,
    }
}

pub fn system_refusals(deploys: &[ProcessedSystemDeploy]) -> Result<Vec<Refusal>, String> {
    let mut all = Vec::new();
    for deploy in deploys {
        all.extend(refusals_in(system_events(deploy))?);
    }
    Ok(all)
}

pub fn with_refusals(deploy: ProcessedSystemDeploy, refusals: &[Refusal]) -> ProcessedSystemDeploy {
    let events = refusals.iter().map(to_event);
    match deploy {
        ProcessedSystemDeploy::Succeeded { mut event_list, system_deploy } => {
            event_list.extend(events);
            ProcessedSystemDeploy::Succeeded { event_list, system_deploy }
        }
        ProcessedSystemDeploy::Failed { mut event_list, error_msg } => {
            event_list.extend(events);
            ProcessedSystemDeploy::Failed { event_list, error_msg }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopFailure {
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
        let phlo = refusal.surface_drawn / refusal.surface.phlo_price.max(1);
        drawn
            .entry(Some(refusal.surface.clone()))
            .and_modify(|d| *d = (*d).min(phlo))
            .or_insert(phlo);
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

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::accounting::comm_ledger::Surface;
    use rholang::rust::interpreter::accounting::membrane::RefusalKind;

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
            surface: Surface {
                owner: vec![owner],
                envelope: vec![9],
                phlo_price: 1,
            },
            step: vec![owner, drawn as u8],
            kind: RefusalKind::Charge,
            drawn,
            charge: 10,
            surface_drawn: drawn,
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

    #[test]
    fn a_refusal_survives_the_deploy_log_encoding() {
        let refusal = Refusal {
            kind: RefusalKind::Cell,
            ..refusal(3, 70)
        };
        let proto = to_event(&refusal).to_proto();
        let back = LogEvent::from_proto(proto).unwrap();
        assert_eq!(refusals_in(&[back]).unwrap(), vec![refusal]);
    }

    #[test]
    fn an_unknown_refusal_kind_is_refused() {
        let mut event = to_event(&refusal(3, 70));
        if let LogEvent::Refusal(r) = &mut event {
            r.kind = 9;
        }
        assert!(refusals_in(&[event]).is_err());
    }
}
