//! Shared test helpers.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tsx2rs_compiler::graph::{ModuleGraph, build};

/// A project in a fresh temporary folder, removed when dropped.
pub(crate) struct Project {
    pub(crate) root: PathBuf,
}

impl Project {
    pub(crate) fn new(files: &[(&str, &str)]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let test = std::thread::current()
            .name()
            .unwrap_or("test")
            .replace("::", "-");
        let root = std::env::temp_dir().join(format!("tsx2rs-{test}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, source) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, source).unwrap();
        }
        Self { root }
    }

    pub(crate) fn graph(&self, entry: &str) -> ModuleGraph {
        build(&self.root, Path::new(entry)).unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
