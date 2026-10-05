use super::LabError;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct PauseClock {
    started: Instant,
    paused_at: Option<Instant>,
    paused_duration: Duration,
}
#[derive(Clone)]
pub(crate) struct Cancellation {
    signal: tokio::sync::watch::Sender<bool>,
    paused: tokio::sync::watch::Sender<bool>,
    publication: Arc<Mutex<()>>,
    clock: Arc<Mutex<PauseClock>>,
}
impl Default for Cancellation {
    fn default() -> Self {
        Self {
            signal: tokio::sync::watch::channel(false).0,
            paused: tokio::sync::watch::channel(false).0,
            publication: Arc::new(Mutex::new(())),
            clock: Arc::new(Mutex::new(PauseClock {
                started: Instant::now(),
                paused_at: None,
                paused_duration: Duration::ZERO,
            })),
        }
    }
}
impl Cancellation {
    pub fn set_paused(&self, paused: bool) {
        let mut clock = self.clock.lock().expect("pause clock lock");
        if paused {
            if clock.paused_at.is_none() {
                clock.paused_at = Some(Instant::now());
            }
        } else if let Some(started) = clock.paused_at.take() {
            clock.paused_duration += started.elapsed();
        }
        self.paused.send_replace(paused);
    }
    pub fn active_elapsed(&self) -> Duration {
        let clock = self.clock.lock().expect("pause clock lock");
        clock
            .paused_at
            .unwrap_or_else(Instant::now)
            .duration_since(clock.started)
            .saturating_sub(clock.paused_duration)
    }
    pub async fn wait_until_resumed(&self) -> Result<(), LabError> {
        let mut paused = self.paused.subscribe();
        loop {
            self.check()?;
            if !*paused.borrow_and_update() {
                return Ok(());
            }
            tokio::select! {
                biased;
                _ = self.cancelled() => return Err(LabError::cancelled()),
                _ = paused.changed() => {},
            }
        }
    }
    pub async fn active_timeout(&self, limit: std::time::Duration) {
        let mut paused = self.paused.subscribe();
        let mut remaining = limit;
        loop {
            let is_paused = *paused.borrow_and_update();
            if is_paused {
                let _ = paused.changed().await;
            } else {
                let started = tokio::time::Instant::now();
                tokio::select! {
                    _ = tokio::time::sleep(remaining) => return,
                    _ = paused.changed() => remaining = remaining.saturating_sub(started.elapsed()),
                }
            }
        }
    }
    pub fn cancel(&self) {
        if let Ok(_guard) = self.publication.lock() {
            self.signal.send_replace(true);
        }
    }
    pub fn check(&self) -> Result<(), LabError> {
        if *self.signal.borrow() {
            Err(LabError::cancelled())
        } else {
            Ok(())
        }
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.signal.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
    pub async fn run<T>(
        &self,
        future: impl std::future::Future<Output=Result<T, LabError>>,
    ) -> Result<T, LabError> {
        self.wait_until_resumed().await?;
        tokio::select! { biased; _ = self.cancelled() => Err(LabError::cancelled()), result = future => result }
    }
    pub fn commit<T>(&self, work: impl FnOnce() -> Result<T, LabError>) -> Result<T, LabError> {
        let _guard = self
            .publication
            .lock()
            .map_err(|_| LabError::new("internalError", "Publication lock unavailable"))?;
        self.check()?;
        work()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn active_elapsed_remains_fixed_while_paused() {
        let token = Cancellation::default();
        token.set_paused(true);
        let elapsed = token.active_elapsed();
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        assert_eq!(token.active_elapsed(), elapsed);
        token.set_paused(false);
        assert!(token.active_elapsed() >= elapsed);
    }
    #[tokio::test]
    async fn paused_transfer_waits_for_resume_and_remains_cancellable() {
        let token = Cancellation::default();
        token.set_paused(true);
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(50),
            token.wait_until_resumed()
        )
            .await
            .is_err());
        token.set_paused(false);
        token.wait_until_resumed().await.unwrap();
        token.set_paused(true);
        token.cancel();
        assert_eq!(
            token.wait_until_resumed().await.unwrap_err().code,
            "cancelled"
        );
    }
    #[tokio::test]
    async fn pause_time_does_not_consume_transfer_timeout() {
        let token = Cancellation::default();
        token.set_paused(true);
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(60),
            token.active_timeout(std::time::Duration::from_millis(20))
        )
            .await
            .is_err());
        token.set_paused(false);
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            token.active_timeout(std::time::Duration::from_millis(20)),
        )
            .await
            .unwrap();
    }
    #[test]
    fn cancellation_blocks_publication() {
        let token = Cancellation::default();
        token.check().unwrap();
        token.cancel();
        assert_eq!(token.check().unwrap_err().code, "cancelled");
    }
}
