//! Pinned public model packages. Network access occurs only after explicit installation.

use anyhow::{Context as _, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// Recognition semantics; offline models never claim incremental streaming output.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    /// Stateful online transducer emitting real Partial and Final revisions.
    #[default]
    StreamingTransducer,
    /// SenseVoice called on completed, bounded VAD segments; Final only.
    SenseVoice,
    /// GGUF instruction model for local text translation, never an ASR backend.
    QwenTranslation,
}
impl ModelKind {
    /// User-facing distinction between real streaming and segmented offline inference.
    pub fn name(self) -> &'static str {
        match self {
            Self::StreamingTransducer => "Streaming · 增量识别",
            Self::SenseVoice => "离线分段 · 句末识别",
            Self::QwenTranslation => "本地文字翻译 · GGUF",
        }
    }
}

/// One immutable, checksummed file in a curated package.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ModelFile {
    /// Flat filename; never a destination path supplied by a server.
    pub path: String,
    /// Exact expected file length.
    pub size: u64,
    /// Expected SHA-256 from publisher metadata or a verified Git blob.
    pub sha256: String,
}
/// A small curated model package, independent of GUI and audio capture.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Model {
    /// Stable application identifier.
    pub id: String,
    /// User-facing model name.
    pub name: String,
    /// Suitability and limitations.
    pub summary: String,
    /// Publisher repository on Hugging Face.
    pub repository: String,
    /// Immutable publisher commit.
    pub revision: String,
    /// Streaming or segmented offline inference.
    pub kind: ModelKind,
    /// Audio input rate; zero for text translation packages.
    pub sample_rate: u32,
    /// Supported source hints; first is the default.
    pub languages: Vec<String>,
    /// Model weights' license; distinct from the runtime license.
    pub license: String,
    /// Full terms shown before downloading.
    pub license_url: String,
    /// Files required to run this model, including its source README.
    pub files: Vec<ModelFile>,
}
impl Model {
    /// Full publisher URL, suitable for disclosure before installation.
    pub fn source(&self) -> String {
        format!(
            "https://huggingface.co/{}/tree/{}",
            self.repository, self.revision
        )
    }
    /// Total installed download size, excluding tiny local metadata.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
    /// Published manifest path; absent until all required files verify.
    pub fn manifest_path(&self, root: &Path) -> PathBuf {
        root.join(&self.id).join(&self.revision).join("model.toml")
    }
    fn validate(&self) -> anyhow::Result<()> {
        let token = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        };
        ensure!(
            token(&self.id)
                && !self.id.starts_with('.')
                && self.revision.len() == 40
                && self.revision.bytes().all(|c| c.is_ascii_hexdigit()),
            "模型 ID 或版本无效"
        );
        ensure!(
            (self.repository.starts_with("csukuangfj/")
                || self.repository == "Qwen/Qwen2.5-1.5B-Instruct-GGUF")
                && self.repository.split('/').count() == 2
                && self.repository.split('/').all(token),
            "模型来源不在目录支持范围"
        );
        ensure!(
            ((self.kind == ModelKind::QwenTranslation && self.sample_rate == 0)
                || (self.kind != ModelKind::QwenTranslation && self.sample_rate == 16000))
                && !self.languages.is_empty()
                && self.languages.len() <= 3
                && self
                    .languages
                    .iter()
                    .all(|s| matches!(s.as_str(), "en" | "ja" | "zh")),
            "模型采样率或语言无效"
        );
        ensure!(
            !self.files.is_empty()
                && self.files.len() <= 8
                && !self.license.is_empty()
                && self.license_url.starts_with("https://"),
            "模型文件或许可证无效"
        );
        let mut names = std::collections::HashSet::new();
        for file in &self.files {
            ensure!(
                token(&file.path)
                    && !file.path.starts_with('.')
                    && names.insert(&file.path)
                    && file.size > 0
                    && file.size <= 1_500_000_000
                    && file.sha256.len() == 64
                    && file.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "模型文件清单无效"
            );
        }
        ensure!(
            (self.kind == ModelKind::QwenTranslation || names.contains(&"tokens.txt".to_owned()))
                && names.contains(&"README.md".to_owned()),
            "模型包缺少 tokens 或来源说明"
        );
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    models: Vec<Model>,
}
/// Load the built-in, version-pinned catalog. Does not read disk or access network.
pub fn catalog() -> anyhow::Result<Vec<Model>> {
    let catalog: Catalog = toml::from_str(include_str!("catalog.toml"))?;
    let mut ids = std::collections::HashSet::new();
    for model in &catalog.models {
        model.validate()?;
        ensure!(ids.insert(&model.id), "重复模型 ID");
    }
    Ok(catalog.models)
}
/// XDG data location; no directory is created until the user installs a model.
pub fn data_root() -> anyhow::Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(".local/share"))
        })
        .context("无法确定 XDG 模型数据目录")?;
    Ok(base.join("subveil/models"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationManifest {
    kind: ModelKind,
    language: String,
    languages: Vec<String>,
    sample_rate: u32,
    source: String,
    license: String,
    model: TranslationArtifact,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationArtifact {
    path: PathBuf,
    sha256: String,
}

/// Verify a translation manifest and GGUF before native loading, without network access.
pub fn verified_translation_model(path: &Path) -> anyhow::Result<PathBuf> {
    ensure!(fs::metadata(path)?.len() <= 65_536, "模型清单超过 64 KiB");
    let manifest: TranslationManifest = toml::from_str(&fs::read_to_string(path)?)?;
    ensure!(
        manifest.kind == ModelKind::QwenTranslation
            && !manifest.source.is_empty()
            && !manifest.license.is_empty(),
        "需要声明来源和许可证的翻译模型"
    );
    ensure!(
        manifest.languages.contains(&manifest.language)
            && manifest
                .languages
                .iter()
                .all(|s| matches!(s.as_str(), "en" | "ja" | "zh"))
            && manifest.sample_rate == 0,
        "翻译模型元数据无效"
    );
    let model = path
        .parent()
        .context("清单缺少目录")?
        .join(manifest.model.path);
    let metadata = fs::metadata(&model)?;
    ensure!(
        metadata.is_file()
            && metadata.len() > 0
            && manifest.model.sha256.len() == 64
            && manifest.model.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
        "模型文件或 SHA-256 无效"
    );
    let mut reader = fs::File::open(&model)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let len = reader.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    ensure!(
        format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&manifest.model.sha256),
        "模型 SHA-256 不匹配"
    );
    Ok(model)
}
/// Progress contains no audio, transcript, signed URL or credentials.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct DownloadSnapshot {
    /// Available bytes on the destination filesystem, measured by the worker.
    pub available_bytes: Option<u64>,
    /// Verified cached bytes plus bytes received for the current file.
    pub completed: u64,
    /// Exact expected total bytes.
    pub total: u64,
    /// Current operation or sanitized error.
    pub status: String,
    /// Worker completed, failed or acknowledged cancellation.
    pub finished: bool,
    /// Available only after all files verify and the manifest is published.
    pub manifest: Option<PathBuf>,
}
struct Shared {
    stop: AtomicBool,
    snapshot: Mutex<DownloadSnapshot>,
}
impl Shared {
    fn progress(&self, completed: u64, status: String) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.completed = completed;
            snapshot.status = status;
        }
    }
    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}
/// Explicit model installation; drop cancels without blocking the GUI.
pub struct Download {
    shared: Arc<Shared>,
}
impl Download {
    /// Start installation after the UI displays source, license and download size.
    pub fn start(model: Model, root: PathBuf) -> anyhow::Result<Self> {
        Self::start_with_network(model, root, true)
    }
    /// Verify cached packages offline; missing files fail before any network request.
    pub fn start_with_network(
        model: Model,
        root: PathBuf,
        allow_network: bool,
    ) -> anyhow::Result<Self> {
        model.validate()?;
        ensure!(root.is_absolute(), "模型目录必须为绝对路径");
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            snapshot: Mutex::new(DownloadSnapshot {
                total: model.size(),
                status: "准备安装…".into(),
                ..Default::default()
            }),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let state = shared.clone();
        thread::Builder::new()
            .name("subveil-model-download".into())
            .spawn(move || {
                let result = runtime.block_on(install_with_cancel(
                    &model,
                    &root,
                    &state,
                    None,
                    allow_network,
                ));
                if let Ok(mut snapshot) = state.snapshot.lock() {
                    snapshot.finished = true;
                    if state.cancelled() {
                        snapshot.status = "下载已取消；已验证文件保留供重试复用".into();
                    } else {
                        match result {
                            Ok(manifest) => {
                                snapshot.manifest = Some(manifest);
                                snapshot.status = "模型已安装并通过校验".into();
                            }
                            Err(error) => snapshot.status = format!("安装失败：{error}"),
                        }
                    }
                }
            })?;
        Ok(Self { shared })
    }
    /// Read progress without waiting for filesystem or network I/O.
    pub fn snapshot(&self) -> Option<DownloadSnapshot> {
        self.shared.snapshot.try_lock().ok().map(|s| s.clone())
    }
    /// Request cancellation of network, validation and subsequent writes.
    pub fn cancel(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}
impl Drop for Download {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        // A crash may leave .part files; next installer removes them while holding the package lock.
        let _ = fs::remove_file(&self.0);
    }
}
fn valid_file(path: &Path, file: &ModelFile, shared: &Shared) -> anyhow::Result<bool> {
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).context("无法读取模型文件属性"),
    };
    if !metadata.is_file() || metadata.len() != file.size {
        return Ok(false);
    }
    let mut reader = fs::File::open(path).context("无法校验模型文件")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        ensure!(!shared.cancelled(), "下载已取消");
        let len = reader.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    Ok(format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&file.sha256))
}
fn http_client() -> anyhow::Result<reqwest::Client> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let redirect = reqwest::redirect::Policy::custom(|attempt| {
        let host = attempt.url().host_str().unwrap_or("");
        if attempt.previous().len() >= 5
            || attempt.url().scheme() != "https"
            || !(host == "huggingface.co"
                || host.ends_with(".huggingface.co")
                || host.ends_with(".hf.co"))
        {
            attempt.error("模型下载重定向目标不受支持")
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .redirect(redirect)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(600))
        .build()
        .context("无法初始化模型下载客户端")
}
async fn install_with_cancel(
    model: &Model,
    root: &Path,
    shared: &Shared,
    test_origin: Option<&str>,
    allow_network: bool,
) -> anyhow::Result<PathBuf> {
    let install = install(model, root, shared, test_origin, allow_network);
    tokio::pin!(install);
    tokio::select! {
        result = &mut install => result,
        _ = async { while !shared.cancelled() { tokio::time::sleep(Duration::from_millis(25)).await; } } => Err(anyhow::anyhow!("下载已取消")),
    }
}
async fn install(
    model: &Model,
    root: &Path,
    shared: &Shared,
    test_origin: Option<&str>,
    allow_network: bool,
) -> anyhow::Result<PathBuf> {
    model.validate()?;
    let manifest = model.manifest_path(root);
    let directory = manifest.parent().context("无效模型目录")?;
    fs::create_dir_all(directory).context("无法创建模型目录；检查磁盘空间与权限")?;
    let available = fs2::available_space(directory).context("无法读取模型磁盘剩余空间")?;
    if let Ok(mut snapshot) = shared.snapshot.lock() {
        snapshot.available_bytes = Some(available);
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(".install.lock"))?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("该模型正在被另一个进程安装，或文件系统不支持安装锁"))?;
    // The OS releases the lock after a crash; do not unlink its inode while installers may wait on it.
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.file_name().to_string_lossy().ends_with(".part") {
            fs::remove_file(entry.path())?;
        }
    }
    let client = http_client()?;
    let mut completed = 0;
    for file in &model.files {
        ensure!(!shared.cancelled(), "下载已取消");
        let destination = directory.join(&file.path);
        shared.progress(completed, format!("校验 {}", file.path));
        if valid_file(&destination, file, shared)? {
            completed += file.size;
            shared.progress(completed, format!("复用 {}", file.path));
            continue;
        }
        ensure!(
            allow_network,
            "离线模式：模型文件缺失或损坏；请联网安装后重试"
        );
        ensure!(
            fs2::available_space(directory)? >= file.size.saturating_add(1_048_576),
            "磁盘空间不足；请释放空间后重试"
        );
        let url = match test_origin {
            Some(origin) => format!("{origin}/{}", file.path),
            None => format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                model.repository, model.revision, file.path
            ),
        };
        let mut response = client
            .get(url)
            .send()
            .await
            .context("模型下载连接失败或超时")?;
        ensure!(
            response.status().is_success(),
            "模型下载返回 HTTP {}",
            response.status().as_u16()
        );
        if let Some(length) = response.content_length() {
            ensure!(length == file.size, "模型文件长度与目录不符");
        }
        let temporary =
            Temporary(directory.join(format!(".{}-{}.part", file.path, uuid::Uuid::new_v4())));
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary.0)
            .context("无法写入下载文件；检查磁盘空间与权限")?;
        let mut received = 0_u64;
        let mut hash = Sha256::new();
        while let Some(chunk) = response.chunk().await.context("模型下载中断或超时")? {
            ensure!(!shared.cancelled(), "下载已取消");
            received = received
                .checked_add(chunk.len() as u64)
                .context("模型文件长度溢出")?;
            ensure!(received <= file.size, "模型文件超过目录声明的大小");
            output
                .write_all(&chunk)
                .context("模型写入失败；检查磁盘空间与权限")?;
            hash.update(&chunk);
            shared.progress(completed + received, format!("下载 {}", file.path));
        }
        ensure!(
            received == file.size
                && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&file.sha256),
            "模型长度或 SHA-256 校验失败；未激活"
        );
        output.sync_all()?;
        drop(output);
        ensure!(!shared.cancelled(), "下载已取消");
        fs::rename(&temporary.0, &destination).context("无法发布已校验模型文件")?;
        completed += received;
    }
    ensure!(!shared.cancelled(), "下载已取消");
    let text = manifest_text(model)?;
    let temporary = Temporary(directory.join(format!(".manifest-{}.part", uuid::Uuid::new_v4())));
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary.0)?;
    output.write_all(text.as_bytes())?;
    output.sync_all()?;
    drop(output);
    fs::rename(&temporary.0, &manifest).context("无法发布模型清单")?;
    Ok(manifest)
}
fn manifest_text(model: &Model) -> anyhow::Result<String> {
    let mut table = toml::Table::new();
    table.insert("kind".into(), toml::Value::try_from(model.kind)?);
    table.insert(
        "language".into(),
        toml::Value::String(model.languages.first().context("缺少模型语言")?.clone()),
    );
    table.insert("languages".into(), toml::Value::try_from(&model.languages)?);
    table.insert(
        "sample_rate".into(),
        toml::Value::Integer(i64::from(model.sample_rate)),
    );
    table.insert("source".into(), toml::Value::String(model.source()));
    table.insert(
        "license".into(),
        toml::Value::String(format!("{} — {}", model.license, model.license_url)),
    );
    for (key, filename) in match model.kind {
        ModelKind::StreamingTransducer => vec![
            ("encoder", "encoder-epoch-99-avg-1.int8.onnx"),
            ("decoder", "decoder-epoch-99-avg-1.int8.onnx"),
            ("joiner", "joiner-epoch-99-avg-1.int8.onnx"),
            ("tokens", "tokens.txt"),
        ],
        ModelKind::SenseVoice => vec![("model", "model.int8.onnx"), ("tokens", "tokens.txt")],
        ModelKind::QwenTranslation => vec![("model", "qwen2.5-1.5b-instruct-q4_k_m.gguf")],
    } {
        let file = model
            .files
            .iter()
            .find(|f| f.path == filename)
            .context("模型包缺少必要文件")?;
        let mut artifact = toml::Table::new();
        artifact.insert("path".into(), toml::Value::String(file.path.clone()));
        artifact.insert("sha256".into(), toml::Value::String(file.sha256.clone()));
        table.insert(key.into(), toml::Value::Table(artifact));
    }
    Ok(toml::to_string(&table)?)
}

#[cfg(test)]
mod tests;
