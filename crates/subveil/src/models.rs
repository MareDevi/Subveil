//! Model library state outlives control windows; all filesystem/network work stays in the installer.
use std::path::PathBuf;
use subveil_asr::models::{Download, DownloadSnapshot, Model};

pub struct LocalModels {
    pub catalog: Vec<Model>,
    pub selected: usize,
    pub root: Option<PathBuf>,
    pub snapshot: DownloadSnapshot,
    pub source: subveil_asr::models::DownloadSource,
    pub source_error: bool,
    download: Option<Download>,
}
impl LocalModels {
    pub fn new() -> Self {
        Self::for_translation(false)
    }
    pub fn for_translation(translation: bool) -> Self {
        Self::filtered(move |kind| {
            kind != subveil_asr::ModelKind::SileroVad
                && (kind == subveil_asr::ModelKind::QwenTranslation) == translation
        })
    }
    pub fn vad() -> Self {
        Self::filtered(|kind| kind == subveil_asr::ModelKind::SileroVad)
    }
    fn filtered(accept: impl Fn(subveil_asr::ModelKind) -> bool) -> Self {
        let result = subveil_asr::models::catalog().and_then(|models| {
            Ok((
                models
                    .into_iter()
                    .filter(|model| accept(model.kind))
                    .collect(),
                subveil_asr::models::data_root()?,
            ))
        });
        match result {
            Ok((catalog, root)) => Self {
                source_error: false,
                source: Default::default(),
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
                    source_error: false,
                    source: Default::default(),
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
    pub fn install(&mut self, allow_network: bool) {
        if self.downloading() {
            return;
        }
        let result = (|| {
            anyhow::ensure!(
                !allow_network || !self.source_error,
                "下载源设置无效；请显式重新应用地址"
            );
            let model = self
                .current()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("没有可用模型"))?;
            let root = self
                .root
                .clone()
                .ok_or_else(|| anyhow::anyhow!("没有有效模型存储目录"))?;
            Download::start_with_source(model, root, allow_network, self.source.clone())
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

/// Transient model-library filters never mutate the installed or selected package.
#[derive(Default)]
pub struct Filter {
    pub query: String,
    pub language: String,
    pub mode: String,
}
impl Filter {
    pub fn matches(&self, model: &Model) -> bool {
        (self.language.is_empty() || model.languages.contains(&self.language))
            && match self.mode.as_str() {
                "原生流式" => model.kind == subveil_asr::ModelKind::StreamingTransducer,
                "离线分段" => matches!(
                    model.kind,
                    subveil_asr::ModelKind::SenseVoice | subveil_asr::ModelKind::Whisper
                ),
                "文字翻译" => model.kind == subveil_asr::ModelKind::QwenTranslation,
                _ => true,
            }
            && (self.query.trim().is_empty()
                || format!(
                    "{} {} {} {}",
                    model.name, model.id, model.summary, model.repository
                )
                .to_lowercase()
                .contains(&self.query.trim().to_lowercase()))
    }
    pub fn names(&self, models: &LocalModels) -> Vec<String> {
        models
            .catalog
            .iter()
            .filter(|m| {
                self.matches(m) || models.current().is_some_and(|selected| selected.id == m.id)
            })
            .map(|m| m.name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_keep_selection_and_distinguish_native_streaming() {
        let models = LocalModels::new();
        let filter = Filter {
            language: "ja".into(),
            mode: "原生流式".into(),
            query: String::new(),
        };
        assert!(
            models
                .catalog
                .iter()
                .filter(|m| filter.matches(m))
                .all(|m| m.languages.iter().any(|l| l == "ja")
                    && m.kind == subveil_asr::ModelKind::StreamingTransducer)
        );
        let none = Filter {
            query: "no-such-model-97531".into(),
            ..Default::default()
        };
        assert_eq!(
            none.names(&models),
            models
                .current()
                .map(|m| vec![m.name.clone()])
                .unwrap_or_default()
        );
        assert_eq!(models.selected, 0);
    }
}
