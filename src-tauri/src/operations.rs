//! One admitted audio operation, with cancellation owned by its lifetime.
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub id: u64,
    pub kind: String,
    pub cancelled: bool,
}
struct Active {
    status: Status,
    cancel: tokio::sync::watch::Sender<bool>,
    permit: Permit,
}
#[derive(Clone)]
pub struct Permit(Arc<AtomicBool>);
impl Permit {
    pub fn valid(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
#[derive(Default)]
pub struct Coordinator {
    active: Mutex<Option<Active>>,
    sequence: AtomicU64,
}
pub struct Lease {
    owner: Arc<Coordinator>,
    pub id: u64,
    cancel: tokio::sync::watch::Receiver<bool>,
    permit: Permit,
}
impl Coordinator {
    pub fn begin(self: &Arc<Self>, kind: &str) -> Result<Lease, String> {
        let mut active = self.active.lock();
        if active.is_some() {
            return Err(
                "Já existe uma operação de áudio ativa. Aguarde ou cancele antes de iniciar outra."
                    .into(),
            );
        }
        let id = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let permit = Permit(Arc::new(AtomicBool::new(true)));
        *active = Some(Active {
            status: Status {
                id,
                kind: kind.into(),
                cancelled: false,
            },
            cancel: sender,
            permit: permit.clone(),
        });
        Ok(Lease {
            owner: self.clone(),
            id,
            cancel: receiver,
            permit,
        })
    }
    pub fn status(&self) -> Option<Status> {
        self.active.lock().as_ref().map(|job| job.status.clone())
    }
    pub fn permit(&self) -> Option<Permit> {
        self.active
            .lock()
            .as_ref()
            .map(|active| active.permit.clone())
    }
    pub fn cancel(&self) {
        if let Some(active) = self.active.lock().as_mut() {
            active.status.cancelled = true;
            active.permit.0.store(false, Ordering::Release);
            active.cancel.send_replace(true);
        }
    }
}
impl Lease {
    pub async fn cancelled(&self) {
        let mut receiver = self.cancel.clone();
        if *receiver.borrow_and_update() {
            return;
        }
        while receiver.changed().await.is_ok() {
            if *receiver.borrow_and_update() {
                return;
            }
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.permit.0.store(false, Ordering::Release);
        let mut active = self.owner.active.lock();
        if active.as_ref().is_some_and(|job| job.status.id == self.id) {
            active.take();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn exclusivity_and_cancel_hold_until_work_is_dropped() {
        let coordinator = Arc::new(Coordinator::default());
        let lease = coordinator.begin("microphone").unwrap();
        let permit = coordinator.permit().unwrap();
        assert!(coordinator.begin("upload").is_err());
        coordinator.cancel();
        assert!(!permit.valid());
        lease.cancelled().await;
        assert!(coordinator.begin("retry").is_err());
        drop(lease);
        let next = coordinator.begin("retry").unwrap();
        assert!(!permit.valid());
        let next_permit = coordinator.permit().unwrap();
        assert!(next_permit.valid());
        drop(next);
        assert!(!next_permit.valid());
    }
}
