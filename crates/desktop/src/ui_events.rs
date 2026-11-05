//! Native callbacks wake GPUI instead of waiting for the maintenance timer.
//! Keep the existing queues (including their bounds) and coalesce wakeups.
use std::{cell::RefCell, sync::mpsc};

#[derive(Clone, Default)]
struct Wake(Option<async_channel::Sender<()>>);
thread_local! {
    static CURRENT: RefCell<Wake> = RefCell::new(Wake::default());
}
impl Wake {
    fn current() -> Self {
        CURRENT.with(|wake| wake.borrow().clone())
    }
    fn notify(&self) {
        if let Some(sender) = &self.0 {
            let _ = sender.try_send(());
        }
    }
}
pub fn listen() -> async_channel::Receiver<()> {
    let (sender, receiver) = async_channel::bounded(1);
    CURRENT.with(|wake| *wake.borrow_mut() = Wake(Some(sender)));
    receiver
}
pub fn wake() {
    Wake::current().notify();
}
pub struct Sender<T>(mpsc::Sender<T>, Wake);
pub struct SyncSender<T>(mpsc::SyncSender<T>, Wake);
impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}
impl<T> Clone for SyncSender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}
pub fn channel<T>() -> (Sender<T>, mpsc::Receiver<T>) {
    let (sender, receiver) = mpsc::channel();
    (Sender(sender, Wake::current()), receiver)
}
pub fn sync_channel<T>(capacity: usize) -> (SyncSender<T>, mpsc::Receiver<T>) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    (SyncSender(sender, Wake::current()), receiver)
}
impl<T> Sender<T> {
    pub fn send(&self, value: T) -> Result<(), mpsc::SendError<T>> {
        self.0.send(value)?;
        self.1.notify();
        Ok(())
    }
}
impl<T> SyncSender<T> {
    pub fn try_send(&self, value: T) -> Result<(), mpsc::TrySendError<T>> {
        self.0.try_send(value)?;
        self.1.notify();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesces_wakeups_without_losing_events_or_queue_bounds() {
        let wake = listen();
        let (sender, events) = sync_channel(2);
        sender.try_send(1).unwrap();
        sender.clone().try_send(2).unwrap();
        assert!(matches!(
            sender.try_send(3),
            Err(mpsc::TrySendError::Full(3))
        ));
        assert_eq!(wake.len(), 1);
        wake.try_recv().unwrap();
        assert_eq!(events.try_iter().collect::<Vec<_>>(), vec![1, 2]);
        // A producer on another thread still wakes the captured UI receiver.
        std::thread::spawn(move || sender.try_send(4).unwrap())
            .join()
            .unwrap();
        assert_eq!(wake.try_recv(), Ok(()));
        assert_eq!(events.try_recv(), Ok(4));
    }
}
