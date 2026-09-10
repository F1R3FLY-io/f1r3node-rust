use std::future::poll_fn;
use std::task::Poll;

use tokio::sync::mpsc;
use tokio::time::Interval;

use super::recovery_service_rotation::ServiceRotation;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum RecoveryEvent<Item, Command> {
    Item(Option<Item>),
    Command(Option<Command>),
    Tick,
}

pub(super) struct RecoveryInbox<Item, Command> {
    items: mpsc::Receiver<Item>,
    commands: mpsc::Receiver<Command>,
    resend: Interval,
    items_open: bool,
    commands_open: bool,
    rotation: ServiceRotation<3>,
}

impl<Item, Command> RecoveryInbox<Item, Command> {
    pub(super) fn new(
        items: mpsc::Receiver<Item>,
        commands: mpsc::Receiver<Command>,
        resend: Interval,
    ) -> Self {
        Self {
            items,
            commands,
            resend,
            items_open: true,
            commands_open: true,
            rotation: ServiceRotation::default(),
        }
    }

    pub(super) async fn next(&mut self) -> Option<RecoveryEvent<Item, Command>> {
        poll_fn(|cx| {
            if !self.items_open && !self.commands_open {
                return Poll::Ready(None);
            }
            self.rotation.poll(|lane| match lane {
                0 if self.items_open => self.items.poll_recv(cx).map(|item| {
                    self.items_open = item.is_some();
                    Some(RecoveryEvent::Item(item))
                }),
                1 if self.commands_open => self.commands.poll_recv(cx).map(|command| {
                    self.commands_open = command.is_some();
                    Some(RecoveryEvent::Command(command))
                }),
                2 => self.resend.poll_tick(cx).map(|_| Some(RecoveryEvent::Tick)),
                _ => Poll::Pending,
            })
        })
        .await
    }

    pub(super) fn try_command(&mut self) -> Result<Command, mpsc::error::TryRecvError> {
        self.commands.try_recv()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use super::*;

    fn inbox<Item, Command>() -> (
        mpsc::Sender<Item>,
        mpsc::Sender<Command>,
        RecoveryInbox<Item, Command>,
    ) {
        let (items_tx, items_rx) = mpsc::channel(4);
        let (commands_tx, commands_rx) = mpsc::channel(4);
        let mut resend = tokio::time::interval(Duration::from_secs(10));
        resend.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        (
            items_tx,
            commands_tx,
            RecoveryInbox::new(items_rx, commands_rx, resend),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn continuous_items_cannot_starve_commands_or_due_ticks() {
        let (items, commands, mut inbox) = inbox::<usize, usize>();
        items.send(0).await.unwrap();
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Item(Some(0))));
        for turn in 0..100 {
            items.send(turn + 1).await.unwrap();
            commands.send(turn).await.unwrap();
            assert_eq!(inbox.next().await, Some(RecoveryEvent::Command(Some(turn))));
            tokio::time::advance(Duration::from_secs(10)).await;
            assert_eq!(inbox.next().await, Some(RecoveryEvent::Tick));
            assert_eq!(
                inbox.next().await,
                Some(RecoveryEvent::Item(Some(turn + 1)))
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn closed_channels_drain_before_shutdown_despite_the_timer() {
        let (items, commands, mut inbox) = inbox::<usize, usize>();
        for value in 0..4 {
            items.send(value).await.unwrap();
            commands.send(value).await.unwrap();
        }
        drop(items);
        drop(commands);
        let mut seen_items = Vec::new();
        let mut seen_commands = Vec::new();
        let mut closed = [0; 2];
        for _ in 0..20 {
            tokio::time::advance(Duration::from_secs(10)).await;
            match inbox.next().await {
                Some(RecoveryEvent::Item(Some(item))) => seen_items.push(item),
                Some(RecoveryEvent::Command(Some(command))) => seen_commands.push(command),
                Some(RecoveryEvent::Item(None)) => closed[0] += 1,
                Some(RecoveryEvent::Command(None)) => closed[1] += 1,
                Some(RecoveryEvent::Tick) => {}
                None => {
                    assert_eq!(seen_items, vec![0, 1, 2, 3]);
                    assert_eq!(seen_commands, vec![0, 1, 2, 3]);
                    assert_eq!(closed, [1, 1]);
                    assert_eq!(inbox.next().await, None);
                    return;
                }
            }
        }
        panic!("closed inputs must terminate within the finite drain bound");
    }

    #[tokio::test(start_paused = true)]
    async fn a_closed_lane_does_not_close_the_other_lane() {
        let (items, commands, mut inbox) = inbox::<usize, usize>();
        drop(items);
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Item(None)));
        commands.send(4).await.unwrap();
        commands.send(5).await.unwrap();
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Command(Some(4))));
        assert_eq!(inbox.try_command(), Ok(5));
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Tick));
        commands.send(6).await.unwrap();
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Command(Some(6))));
        drop(commands);
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Command(None)));
        assert_eq!(inbox.next().await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_a_pending_poll_preserves_inputs_and_rotation() {
        use std::future::Future;

        let (items, commands, mut inbox) = inbox::<usize, usize>();
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Tick));
        {
            let mut next = std::pin::pin!(inbox.next());
            let mut cx = Context::from_waker(std::task::Waker::noop());
            assert_eq!(next.as_mut().poll(&mut cx), Poll::Pending);
        }
        items.send(1).await.unwrap();
        commands.send(2).await.unwrap();
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Item(Some(1))));
        assert_eq!(inbox.next().await, Some(RecoveryEvent::Command(Some(2))));
    }

    #[tokio::test]
    async fn cancelling_an_actor_releases_queued_payloads_and_both_receivers() {
        let (items, commands, inbox) = inbox::<Arc<usize>, Arc<usize>>();
        let item = Arc::new(1);
        let command = Arc::new(2);
        items.send(item.clone()).await.unwrap();
        commands.send(command.clone()).await.unwrap();
        let actor = tokio::spawn(async move {
            let _inbox = inbox;
            std::future::pending::<()>().await;
        });
        actor.abort();
        assert!(actor.await.unwrap_err().is_cancelled());
        assert_eq!(Arc::strong_count(&item), 1);
        assert_eq!(Arc::strong_count(&command), 1);
        assert!(items.is_closed());
        assert!(commands.is_closed());
    }
}
