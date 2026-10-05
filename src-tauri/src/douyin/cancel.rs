use super::LabError;
use std::sync::{Arc, Mutex};
#[derive(Clone)]
pub(super) struct Cancellation {
    signal: tokio::sync::watch::Sender<bool>,
    publication: Arc<Mutex<()>>,
}
impl Default for Cancellation {
    fn default() -> Self {
        Self {
            signal: tokio::sync::watch::channel(false).0,
            publication: Arc::new(Mutex::new(())),
        }
    }
}
impl Cancellation {
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
    #[test]
    fn cancellation_blocks_publication() {
        let token = Cancellation::default();
        token.check().unwrap();
        token.cancel();
        assert_eq!(token.check().unwrap_err().code, "cancelled");
    }
}
