use super::{error, RequiredToolError};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Usage {
    readers: BTreeMap<PathBuf, usize>,
    publishers: Vec<PathBuf>,
}
#[derive(Clone, Default)]
pub(crate) struct ToolUsageRegistry(Arc<Mutex<Usage>>);
pub(crate) struct ToolUsageLease {
    registry: ToolUsageRegistry,
    paths: Vec<PathBuf>,
    publication: bool,
}

fn normalized(path: &Path) -> PathBuf {
    let canonical =
        path.canonicalize()
            .unwrap_or_else(|_| match (path.parent(), path.file_name()) {
                (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
                    normalized(parent).join(name)
                }
                _ => path.to_path_buf(),
            });
    if cfg!(windows) {
        PathBuf::from(canonical.to_string_lossy().to_lowercase())
    } else {
        canonical
    }
}
impl ToolUsageRegistry {
    pub fn acquire(&self, paths: &[PathBuf]) -> Result<ToolUsageLease, RequiredToolError> {
        let paths = paths
            .iter()
            .map(|path| normalized(path))
            .collect::<Vec<_>>();
        let mut state = self.0.lock().map_err(|e| error("busy", "", e))?;
        if state
            .publishers
            .iter()
            .any(|root| paths.iter().any(|p| p.starts_with(root)))
        {
            return Err(error("busy", "", "Tool files are being replaced"));
        }
        for path in &paths {
            *state.readers.entry(path.clone()).or_default() += 1;
        }
        Ok(ToolUsageLease {
            registry: self.clone(),
            paths,
            publication: false,
        })
    }
    pub fn publication(&self, root: &Path) -> Result<ToolUsageLease, RequiredToolError> {
        let root = normalized(root);
        let mut state = self.0.lock().map_err(|e| error("busy", "", e))?;
        if state.readers.keys().any(|p| p.starts_with(&root))
            || state
            .publishers
            .iter()
            .any(|p| p.starts_with(&root) || root.starts_with(p))
        {
            return Err(error(
                "busy",
                "",
                "Tool files are in use by a download task",
            ));
        }
        state.publishers.push(root.clone());
        Ok(ToolUsageLease {
            registry: self.clone(),
            paths: vec![root.to_owned()],
            publication: true,
        })
    }
}
impl Drop for ToolUsageLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.registry.0.lock() {
            for path in &self.paths {
                if self.publication {
                    state.publishers.retain(|p| p != path);
                } else if let Some(count) = state.readers.get_mut(path) {
                    *count -= 1;
                    if *count == 0 {
                        state.readers.remove(path);
                    }
                }
            }
        }
    }
}
