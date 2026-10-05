use std::collections::{BTreeSet, VecDeque};

pub(crate) struct Scheduler {
    running: BTreeSet<String>,
    queued: VecDeque<String>,
    exiting: bool,
    limit: usize,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            running: BTreeSet::new(),
            queued: VecDeque::new(),
            exiting: false,
            limit: crate::database::DEFAULT_DOWNLOAD_LIMIT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dispatches_three_tasks_and_queues_the_fourth() {
        let mut scheduler = Scheduler::default();
        for id in ["a", "b", "c"] {
            assert_eq!(scheduler.enqueue(id.into()), vec![id]);
        }
        assert!(scheduler.enqueue("d".into()).is_empty());
        assert_eq!(scheduler.finish("b"), vec!["d"]);
    }

    #[test]
    fn raising_limit_dispatches_fifo_and_lowering_does_not_interrupt_running_tasks() {
        let mut scheduler = Scheduler::default();
        scheduler.set_limit(1);
        assert_eq!(scheduler.enqueue("a".into()), vec!["a"]);
        for id in ["b", "c", "d", "e"] {
            assert!(scheduler.enqueue(id.into()).is_empty());
        }
        assert_eq!(scheduler.set_limit(4), vec!["b", "c", "d"]);
        assert!(scheduler.set_limit(2).is_empty());
        assert_eq!(scheduler.running_count(), 4);
        assert!(scheduler.finish("a").is_empty());
        assert!(scheduler.finish("b").is_empty());
        assert_eq!(scheduler.finish("c"), vec!["e"]);
    }

    #[test]
    fn raising_limit_while_stopping_does_not_dispatch() {
        let mut scheduler = Scheduler::default();
        scheduler.stop();
        assert!(scheduler.enqueue("a".into()).is_empty());
        assert!(scheduler.set_limit(6).is_empty());
        assert_eq!(scheduler.running_count(), 0);
    }

    #[test]
    fn a_task_that_finishes_while_queued_is_never_dispatched_again() {
        let mut scheduler = Scheduler::default();
        scheduler.set_limit(2);
        assert_eq!(scheduler.enqueue("a".into()), vec!["a"]);
        assert_eq!(scheduler.enqueue("b".into()), vec!["b"]);
        assert!(scheduler.enqueue("resumed".into()).is_empty());
        assert!(scheduler.finish("resumed").is_empty());
        assert!(scheduler.finish("a").is_empty());
        assert_eq!(scheduler.running_count(), 1);
    }
}
impl Scheduler {
    pub fn set_limit(&mut self, limit: usize) -> Vec<String> {
        self.limit = limit;
        self.dispatch()
    }
    pub fn enqueue(&mut self, id: String) -> Vec<String> {
        self.queued.push_back(id);
        self.dispatch()
    }
    pub fn finish(&mut self, id: &str) -> Vec<String> {
        self.running.remove(id);
        self.cancel_queued(id);
        self.dispatch()
    }
    pub fn cancel_queued(&mut self, id: &str) -> bool {
        let old = self.queued.len();
        self.queued.retain(|s| s != id);
        old != self.queued.len()
    }
    fn dispatch(&mut self) -> Vec<String> {
        let mut ready = Vec::new();
        while !self.exiting && self.running.len() < self.limit {
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
    pub fn is_running(&self, id: &str) -> bool {
        self.running.contains(id)
    }
}
