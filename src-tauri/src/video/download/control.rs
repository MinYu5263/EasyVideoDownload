use crate::{douyin::cancel::Cancellation, required_tools::process_tree::ProcessTree};
use std::{
    io,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(crate) struct DownloadControl {
    tree: Mutex<Option<Arc<ProcessTree>>>,
    pub native: Cancellation,
}

impl DownloadControl {
    pub fn set_paused(&self, paused: bool) -> io::Result<()> {
        let tree = self
            .tree
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        if let Some(tree) = &*tree {
            tree.set_paused(paused)?;
        }
        self.native.set_paused(paused);
        Ok(())
    }
    pub fn attach<'a>(&'a self, tree: Arc<ProcessTree>) -> ProcessRegistration<'a> {
        *self.tree.lock().expect("download control lock") = Some(tree);
        ProcessRegistration(self)
    }
}

pub(crate) struct ProcessRegistration<'a>(&'a DownloadControl);
impl Drop for ProcessRegistration<'_> {
    fn drop(&mut self) {
        self.0.tree.lock().expect("download control lock").take();
    }
}
