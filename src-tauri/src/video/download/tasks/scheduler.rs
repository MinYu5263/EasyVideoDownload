use std::collections::{BTreeSet, VecDeque};

#[derive(Default)]
pub(crate) struct Scheduler {
    running: BTreeSet<String>,
    queued: VecDeque<String>,
    exiting: bool,
}
impl Scheduler {
    pub fn enqueue(&mut self, id: String) -> Vec<String> {
        self.queued.push_back(id);
        self.dispatch()
    }
    pub fn finish(&mut self, id: &str) -> Vec<String> {
        self.running.remove(id);
        self.dispatch()
    }
    pub fn cancel_queued(&mut self, id: &str) -> bool {
        let old = self.queued.len();
        self.queued.retain(|s| s != id);
        old != self.queued.len()
    }
    fn dispatch(&mut self) -> Vec<String> {
        let mut ready = Vec::new();
        while !self.exiting && self.running.len() < 2 {
            let Some(id) = self.queued.pop_front() else {
                break;
            };
            self.running.insert(id.clone());
            ready.push(id);
        }
        ready
    }
    pub fn stop(&mut self) {
        self.exiting = true;
    }
    pub fn resume(&mut self) {
        self.exiting = false;
    }
    pub fn running_count(&self) -> usize {
        self.running.len()
    }
}
