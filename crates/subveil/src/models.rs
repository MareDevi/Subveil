//! Model library state outlives control windows; all filesystem/network work stays in the installer.
use std::path::PathBuf;
use subveil_asr::models::{Download, DownloadSnapshot, Model};

pub struct LocalModels {
    pub catalog: Vec<Model>,
    pub selected: usize,
    pub root: Option<PathBuf>,
    pub snapshot: DownloadSnapshot,
    download: Option<Download>,
}
impl LocalModels {
    pub fn new() -> Self {
        let result = subveil_asr::models::catalog()
            .and_then(|models| Ok((models, subveil_asr::models::data_root()?)));
        match result {
            Ok((catalog, root)) => Self {
                catalog,
                selected: 0,
                root: Some(root),
                snapshot: DownloadSnapshot::default(),
                download: None,
            },
            Err(error) => {
                let mut snapshot = DownloadSnapshot::default();
                snapshot.status = format!("模型库不可用：{error}");
                Self {
                    catalog: Vec::new(),
                    selected: 0,
                    root: None,
                    snapshot,
                    download: None,
                }
            }
        }
    }
    pub fn current(&self) -> Option<&Model> {
        self.catalog.get(self.selected)
    }
    pub fn downloading(&self) -> bool {
        self.download.is_some()
    }
    pub fn select(&mut self, id: &str) -> bool {
        if self.downloading() {
            return false;
        }
        let Some(index) = self.catalog.iter().position(|model| model.id == id) else {
            return false;
        };
        self.selected = index;
        self.snapshot = DownloadSnapshot::default();
        true
    }
    pub fn install(&mut self) {
        if self.downloading() {
            return;
        }
        let result = (|| {
            let model = self
                .current()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("没有可用模型"))?;
            let root = self
                .root
                .clone()
                .ok_or_else(|| anyhow::anyhow!("没有有效模型存储目录"))?;
            Download::start(model, root)
        })();
        match result {
            Ok(download) => {
                if let Some(snapshot) = download.snapshot() {
                    self.snapshot = snapshot;
                }
                self.download = Some(download);
            }
            Err(error) => self.snapshot.status = format!("无法安装模型：{error}"),
        }
    }
    pub fn cancel(&self) {
        if let Some(download) = &self.download {
            download.cancel();
        }
    }
    pub fn poll(&mut self) -> (bool, Option<(PathBuf, Model)>) {
        let Some(snapshot) = self.download.as_ref().and_then(Download::snapshot) else {
            return (false, None);
        };
        let changed = snapshot.status != self.snapshot.status
            || snapshot.completed != self.snapshot.completed
            || snapshot.finished != self.snapshot.finished;
        self.snapshot = snapshot;
        if self.snapshot.finished {
            self.download = None;
            let selected = self.snapshot.manifest.clone().zip(self.current().cloned());
            return (true, selected);
        }
        (changed, None)
    }
}
