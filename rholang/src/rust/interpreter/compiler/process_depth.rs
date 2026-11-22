use rholang_parser::ast::{AnnProc, Name, Proc};
use rholang_parser::DfsEventExt;

/// The deepest nesting of processes in `proc`, counting the processes quoted
/// inside names. Parallel composition adds no depth: the normalizer flattens
/// it. The walk is iterative, so the depth it measures cannot exhaust the
/// stack.
pub fn process_depth<'a>(proc: &'a AnnProc<'a>) -> usize {
    struct Frame<'a> {
        events: Box<dyn Iterator<Item = DfsEventExt<'a>> + 'a>,
        base: usize,
        open: usize,
    }

    let mut frames = vec![Frame {
        events: Box::new(proc.iter_dfs_event_with_names()),
        base: 0,
        open: 0,
    }];
    let mut deepest = 0;

    while let Some(frame) = frames.last_mut() {
        match frame.events.next() {
            Some(DfsEventExt::Enter(entered)) => {
                if !matches!(entered.proc, Proc::Par { .. }) {
                    frame.open += 1;
                    deepest = deepest.max(frame.base + frame.open);
                }
            }
            Some(DfsEventExt::Exit(exited)) => {
                if !matches!(exited.proc, Proc::Par { .. }) {
                    frame.open -= 1;
                }
            }
            Some(DfsEventExt::Name(Name::Quote(quoted))) => {
                let base = frame.base + frame.open;
                frames.push(Frame {
                    events: Box::new(quoted.iter_dfs_event_with_names()),
                    base,
                    open: 0,
                });
            }
            Some(DfsEventExt::Name(Name::NameVar(_))) => {}
            None => {
                frames.pop();
            }
        }
    }

    deepest
}
