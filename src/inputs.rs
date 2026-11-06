use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering::Relaxed},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    time::Duration,
};

use crate::rgb_matrix::UpdateThreadError;

/// A change of the GPIO inputs.
///
/// A bit that toggled more than once since the last change was received can be in both `rising` and
/// `falling`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputChange {
    /// The current level of the input bits.
    pub state: u32,
    /// The bits that went from low to high since the last change was received.
    pub rising: u32,
    /// The bits that went from high to low since the last change was received.
    pub falling: u32,
}

/// The input state shared between the update thread and the reader.
#[derive(Debug, Default)]
struct SharedInputs {
    state: AtomicU32,
    rising: AtomicU32,
    falling: AtomicU32,
}

/// The update thread's side of the inputs. Updating them never blocks or allocates.
pub(crate) struct InputWriter {
    shared: Arc<SharedInputs>,
    last_state: u32,
    /// Wakes up the reader. Holds at most one pending wakeup.
    wake_sender: SyncSender<()>,
}

/// Reads the GPIO inputs.
///
/// The display update thread samples the inputs once per frame, or every 10 ms while the display is
/// suspended. Pulses shorter than that can be missed, and the inputs are not debounced.
#[derive(Debug)]
pub struct InputReader {
    shared: Arc<SharedInputs>,
    wake_receiver: Receiver<()>,
}

pub(crate) fn input_channel() -> (InputWriter, InputReader) {
    let shared = Arc::new(SharedInputs::default());
    let (wake_sender, wake_receiver) = sync_channel(1);
    let writer = InputWriter {
        shared: shared.clone(),
        last_state: 0,
        wake_sender,
    };
    (
        writer,
        InputReader {
            shared,
            wake_receiver,
        },
    )
}

impl InputWriter {
    /// Set the state the inputs had at startup, without reporting it as a change.
    pub(crate) fn set_initial(&mut self, state: u32) {
        self.last_state = state;
        self.shared.state.store(state, Relaxed);
    }

    pub(crate) fn update(&mut self, state: u32) {
        let changed = state ^ self.last_state;
        if changed == 0 {
            return;
        }
        self.last_state = state;
        self.shared.state.store(state, Relaxed);
        self.shared.rising.fetch_or(changed & state, Relaxed);
        self.shared.falling.fetch_or(changed & !state, Relaxed);
        // Fails if a wakeup is already pending or there is no reader, both of which are fine.
        self.wake_sender.try_send(()).ok();
    }
}

impl InputReader {
    /// The current level of the input bits.
    #[must_use]
    pub fn state(&self) -> u32 {
        self.shared.state.load(Relaxed)
    }

    /// Wait for the inputs to change and return the changes since the last call. Returns
    /// `None` if they didn't change, and rarely also before the timeout.
    ///
    /// Once the matrix was dropped or its update thread panicked, this returns an error after all changes
    /// have been received.
    pub fn wait_for_change(
        &self,
        timeout: Duration,
    ) -> Result<Option<InputChange>, UpdateThreadError> {
        match self.wake_receiver.recv_timeout(timeout) {
            // The changes of a wakeup can already have been taken along with earlier ones.
            Ok(()) => Ok(self.take_change()),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(UpdateThreadError(String::from(
                "The matrix was dropped or the thread panicked.",
            ))),
        }
    }

    fn take_change(&self) -> Option<InputChange> {
        let rising = self.shared.rising.swap(0, Relaxed);
        let falling = self.shared.falling.swap(0, Relaxed);
        (rising | falling != 0).then(|| InputChange {
            state: self.state(),
            rising,
            falling,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{InputChange, input_channel};

    #[test]
    fn test_changes_are_combined() {
        let (mut writer, reader) = input_channel();
        writer.set_initial(0b01);
        assert_eq!(reader.state(), 0b01);
        assert_eq!(reader.wait_for_change(Duration::ZERO).unwrap(), None);

        writer.update(0b10);
        writer.update(0b11);
        writer.update(0b11);
        let expected = InputChange {
            state: 0b11,
            rising: 0b11,
            falling: 0b01,
        };
        assert_eq!(
            reader.wait_for_change(Duration::ZERO).unwrap(),
            Some(expected)
        );
        assert_eq!(reader.wait_for_change(Duration::ZERO).unwrap(), None);
    }

    #[test]
    fn test_change_before_disconnect() {
        let (mut writer, reader) = input_channel();
        writer.update(0b1);
        drop(writer);
        assert!(reader.wait_for_change(Duration::MAX).unwrap().is_some());
        assert!(reader.wait_for_change(Duration::MAX).is_err());
    }
}
