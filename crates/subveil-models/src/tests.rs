use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Instant,
};

fn fixture() -> anyhow::Result<Model> {
    let mut model = catalog()?
        .into_iter()
        .find(|m| m.kind == ModelKind::SenseVoice)
        .context("fixture")?;
    for file in &mut model.files {
        file.size = 4;
        file.sha256 = format!("{:x}", Sha256::digest(b"test"));
    }
    Ok(model)
}
fn state(model: &Model) -> Shared {
    Shared {
        stop: AtomicBool::new(false),
        snapshot: Mutex::new(DownloadSnapshot {
            total: model.size(),
            ..Default::default()
        }),
    }
}
fn root() -> PathBuf {
    std::env::temp_dir().join(format!("subveil-library-test-{}", uuid::Uuid::new_v4()))
}
fn server(
    count: usize,
    body: &'static str,
) -> anyhow::Result<(String, thread::JoinHandle<anyhow::Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let address = format!("http://{}", listener.local_addr()?);
    let worker = thread::spawn(move || {
        let limit = Instant::now() + Duration::from_secs(3);
        let mut requests = 0;
        while requests < count && Instant::now() < limit {
            let (mut socket, _) = match listener.accept() {
                Ok(socket) => socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            socket.set_read_timeout(Some(Duration::from_secs(1)))?;
            let mut buffer = [0; 4096];
            ensure!(socket.read(&mut buffer)? > 0, "empty request");
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            requests += 1;
        }
        ensure!(requests == count, "missing test request");
        Ok(())
    });
    Ok((address, worker))
}
#[test]
fn curated_catalog_has_pinned_hashes_and_preserves_backend_semantics() -> anyhow::Result<()> {
    let models = catalog()?;
    assert_eq!(models.len(), 9);
    for model in models {
        let dir = root();
        fs::create_dir_all(&dir)?;
        let path = dir.join("model.toml");
        fs::write(&path, manifest_text(&model)?)?;
        let metadata: toml::Table = toml::from_str(&fs::read_to_string(&path)?)?;
        assert_eq!(
            metadata.get("kind"),
            Some(&toml::Value::try_from(model.kind)?)
        );
        assert_eq!(
            metadata.get("languages"),
            Some(&toml::Value::try_from(&model.languages)?)
        );
        fs::remove_dir_all(dir)?;
    }
    Ok(())
}
#[test]
fn valid_download_publishes_manifest_last_and_reuses_cache_without_network() -> anyhow::Result<()> {
    let model = fixture()?;
    let root = root();
    let shared = state(&model);
    let (url, server) = server(model.files.len(), "test")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let path = runtime.block_on(install(&model, &root, &shared, Some(&url), true))?;
    server
        .join()
        .map_err(|_| anyhow::anyhow!("server panicked"))??;
    assert!(path.is_file());
    assert!(toml::from_str::<toml::Table>(&fs::read_to_string(&path)?).is_ok());
    assert_eq!(
        shared
            .snapshot
            .lock()
            .map_err(|_| anyhow::anyhow!("poison"))?
            .completed,
        model.size()
    );
    // A closed port proves verified cached files do not trigger a GET.
    assert_eq!(
        runtime.block_on(install(
            &model,
            &root,
            &shared,
            Some("http://127.0.0.1:1"),
            true
        ))?,
        path
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn offline_install_reuses_verified_files_and_rejects_missing_or_corrupt_files() -> anyhow::Result<()>
{
    let model = fixture()?;
    let root = root();
    let shared = state(&model);
    let directory = model
        .manifest_path(&root)
        .parent()
        .context("parent")?
        .to_path_buf();
    fs::create_dir_all(&directory)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    assert!(
        runtime
            .block_on(install(
                &model,
                &root,
                &shared,
                Some("http://127.0.0.1:1"),
                false
            ))
            .is_err()
    );
    for file in &model.files {
        fs::write(directory.join(&file.path), b"test")?;
    }
    assert!(
        runtime
            .block_on(install(
                &model,
                &root,
                &shared,
                Some("http://127.0.0.1:1"),
                false
            ))
            .is_ok()
    );
    fs::write(directory.join(&model.files[0].path), b"evil")?;
    assert!(
        runtime
            .block_on(install(
                &model,
                &root,
                &shared,
                Some("http://127.0.0.1:1"),
                false
            ))
            .is_err()
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn translation_weights_are_verified_and_asr_manifests_are_rejected() -> anyhow::Result<()> {
    let mut model = catalog()?
        .into_iter()
        .find(|m| m.kind == ModelKind::QwenTranslation)
        .context("translation model")?;
    let root = root();
    fs::create_dir_all(&root)?;
    let path = root.join("model.toml");
    for file in &mut model.files {
        file.size = 4;
        file.sha256 = format!("{:x}", Sha256::digest(b"test"));
        fs::write(root.join(&file.path), b"test")?;
    }
    fs::write(&path, manifest_text(&model)?)?;
    assert_eq!(verified_translation(&path)?.1, ModelKind::QwenTranslation);
    model.kind = ModelKind::HyMtTranslation;
    fs::write(&path, manifest_text(&model)?)?;
    assert_eq!(verified_translation(&path)?.1, ModelKind::HyMtTranslation);
    fs::write(root.join(&model.files[0].path), b"evil")?;
    assert!(verified_translation_model(&path).is_err());
    fs::write(&path, manifest_text(&fixture()?)?)?;
    assert!(verified_translation_model(&path).is_err());
    fs::remove_dir_all(root)?;
    Ok(())
}
#[test]
fn corrupted_and_oversized_downloads_never_publish_or_leave_part_files() -> anyhow::Result<()> {
    for body in ["evil", "oversized"] {
        let model = fixture()?;
        let root = root();
        let shared = state(&model);
        let (url, server) = server(1, body)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        assert!(
            runtime
                .block_on(install(&model, &root, &shared, Some(&url), true))
                .is_err()
        );
        server
            .join()
            .map_err(|_| anyhow::anyhow!("server panicked"))??;
        let manifest = model.manifest_path(&root);
        assert!(!manifest.exists());
        let entries = fs::read_dir(manifest.parent().context("parent")?)?;
        assert!(
            entries
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".part"))
        );
        fs::remove_dir_all(root)?;
    }
    Ok(())
}
#[test]
fn cancellation_and_parallel_installer_lock_prevent_activation() -> anyhow::Result<()> {
    let model = fixture()?;
    let root = root();
    let shared = state(&model);
    let directory = model
        .manifest_path(&root)
        .parent()
        .context("parent")?
        .to_path_buf();
    fs::create_dir_all(&directory)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(".install.lock"))?;
    lock.try_lock()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    assert!(
        runtime
            .block_on(install(
                &model,
                &root,
                &shared,
                Some("http://127.0.0.1:1"),
                true
            ))
            .is_err()
    );
    drop(lock);
    shared.stop.store(true, Ordering::Relaxed);
    assert!(
        runtime
            .block_on(install(
                &model,
                &root,
                &shared,
                Some("http://127.0.0.1:1"),
                true
            ))
            .is_err()
    );
    assert!(!model.manifest_path(&root).exists());
    fs::remove_dir_all(root)?;
    Ok(())
}
#[test]
fn catalog_rejects_traversal_and_unpinned_versions() -> anyhow::Result<()> {
    let mut model = fixture()?;
    model.files[0].path = "../model.onnx".into();
    assert!(model.validate().is_err());
    let mut model = fixture()?;
    model.revision = "main".into();
    assert!(model.validate().is_err());
    Ok(())
}

#[test]
fn cancellation_interrupts_stalled_http_without_activation() -> anyhow::Result<()> {
    let model = fixture()?;
    let root = root();
    let shared = Arc::new(state(&model));
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    listener.set_nonblocking(true)?;
    let (sent, received) = std::sync::mpsc::channel();
    let server = thread::spawn(move || -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok((mut socket, _)) = listener.accept() {
                socket.set_read_timeout(Some(Duration::from_secs(1)))?;
                let mut buffer = [0; 4096];
                ensure!(socket.read(&mut buffer)? > 0, "empty request");
                socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nt",
                )?;
                sent.send(())?;
                thread::sleep(Duration::from_millis(400));
                return Ok(());
            }
            ensure!(Instant::now() < deadline, "no test connection");
            thread::sleep(Duration::from_millis(5));
        }
    });
    let cancel_state = shared.clone();
    let canceller = thread::spawn(move || {
        if received.recv_timeout(Duration::from_secs(2)).is_ok() {
            cancel_state.stop.store(true, Ordering::Relaxed);
        }
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(install_with_cancel(
        &model,
        &root,
        &shared,
        Some(&url),
        true,
        &DownloadSource::default(),
    ));
    assert!(result.is_err_and(|error| error.to_string() == "下载已取消"));
    canceller
        .join()
        .map_err(|_| anyhow::anyhow!("canceller panic"))?;
    server
        .join()
        .map_err(|_| anyhow::anyhow!("server panic"))??;
    assert!(!model.manifest_path(&root).exists());
    for entry in fs::read_dir(model.manifest_path(&root).parent().context("parent")?)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().ends_with(".part") {
            assert!(entry.metadata()?.len() <= 4);
        }
    }
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn whisper_weights_require_matching_kind_rate_and_integrity() -> anyhow::Result<()> {
    let model = catalog()?
        .into_iter()
        .find(|m| m.kind == ModelKind::Whisper)
        .context("whisper")?;
    let dir = root();
    fs::create_dir_all(&dir)?;
    let mut text: toml::Table = toml::from_str(&manifest_text(&model)?)?;
    let artifact = text
        .get_mut("model")
        .and_then(toml::Value::as_table_mut)
        .context("artifact")?;
    artifact.insert(
        "sha256".into(),
        toml::Value::String(format!("{:x}", Sha256::digest(b"test"))),
    );
    fs::write(dir.join("ggml-large-v3-turbo-q5_0.bin"), b"test")?;
    let path = dir.join("model.toml");
    fs::write(&path, toml::to_string(&text)?)?;
    assert!(verified_whisper_model(&path).is_ok());
    assert!(verified_translation_model(&path).is_err());
    fs::write(dir.join("ggml-large-v3-turbo-q5_0.bin"), b"evil")?;
    assert!(verified_whisper_model(&path).is_err());
    fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn resume_validates_range_and_handles_servers_ignoring_it() -> anyhow::Result<()> {
    for mode in ["range", "ignored", "wrong", "corrupt"] {
        let model = fixture()?;
        let root = root();
        let directory = model
            .manifest_path(&root)
            .parent()
            .context("parent")?
            .to_path_buf();
        fs::create_dir_all(&directory)?;
        for file in &model.files {
            fs::write(directory.join(&file.path), b"test")?;
        }
        let file = &model.files[0];
        fs::remove_file(directory.join(&file.path))?;
        let part = directory.join(part_name(file));
        fs::write(&part, if mode == "corrupt" { b"xx" } else { b"te" })?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let origin = format!("http://{}", listener.local_addr()?);
        listener.set_nonblocking(true)?;
        let server = thread::spawn(move || -> anyhow::Result<()> {
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
                        let mut bytes = [0; 4096];
                        let len = stream.read(&mut bytes)?;
                        assert!(
                            String::from_utf8_lossy(&bytes[..len])
                                .to_ascii_lowercase()
                                .contains("range: bytes=2-")
                        );
                        let response = match mode {
                            "range" | "corrupt" => {
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\nContent-Range: bytes 2-3/4\r\nConnection: close\r\n\r\nst"
                            }
                            "ignored" => {
                                "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntest"
                            }
                            _ => {
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\nContent-Range: bytes 1-2/4\r\nConnection: close\r\n\r\nst"
                            }
                        };
                        stream.write_all(response.as_bytes())?;
                        return Ok(());
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
                ensure!(Instant::now() < deadline, "missing resume request");
                thread::sleep(Duration::from_millis(5));
            }
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let result = runtime.block_on(install(&model, &root, &state(&model), Some(&origin), true));
        server
            .join()
            .map_err(|_| anyhow::anyhow!("server panic"))??;
        if mode == "corrupt" {
            assert!(result.is_err());
            assert!(!model.manifest_path(&root).exists());
            assert!(!part.exists());
        } else if mode == "wrong" {
            assert!(result.is_err());
            assert!(!model.manifest_path(&root).exists());
            assert_eq!(fs::read(part)?, b"te");
        } else {
            assert!(result?.is_file());
            assert_eq!(fs::read(directory.join(&file.path))?, b"test");
            assert!(!part.exists());
        }
        fs::remove_dir_all(root)?;
    }
    Ok(())
}

#[test]
fn uninstall_respects_install_lock_and_preserves_unknown_files() -> anyhow::Result<()> {
    let model = fixture()?;
    let root = root();
    let directory = model
        .manifest_path(&root)
        .parent()
        .context("parent")?
        .to_path_buf();
    fs::create_dir_all(&directory)?;
    fs::write(directory.join("model.toml"), "manifest")?;
    for file in &model.files {
        fs::write(directory.join(&file.path), b"test")?;
    }
    fs::write(directory.join("user-notes.txt"), b"preserve")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(".install.lock"))?;
    lock.try_lock()?;
    assert!(model.remove_cached(&root).is_err());
    assert!(model.manifest_path(&root).exists());
    drop(lock);
    assert!(model.remove_cached(&root)? >= model.size());
    assert!(!model.manifest_path(&root).exists());
    assert_eq!(fs::read(directory.join("user-notes.txt"))?, b"preserve");
    assert!(directory.join(".install.lock").exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn explicit_mirror_preserves_publisher_paths_and_rejects_credentials() -> anyhow::Result<()> {
    let source = DownloadSource::parse("https://mirror.example:8443/")?;
    let model = fixture()?;
    assert_eq!(
        source.file_url(&model, &model.files[0]),
        format!(
            "https://mirror.example:8443/{}/resolve/{}/{}",
            model.repository, model.revision, model.files[0].path
        )
    );
    for invalid in [
        "http://mirror.example",
        "https://key@mirror.example",
        "https://u:p@mirror.example",
        "https://mirror.example/path",
        "https://mirror.example?token=x",
        "https://mirror.example#x",
        "https://mirror.example\n",
    ] {
        assert!(DownloadSource::parse(invalid).is_err());
    }
    for url in [
        "http://mirror.example/x",
        "https://mirror.example.evil/x",
        "https://other.example/x",
        "https://x@mirror.example:8443/x",
    ] {
        assert!(!source.allows_redirect(&reqwest::Url::parse(url)?));
    }
    assert!(source.allows_redirect(&reqwest::Url::parse("https://mirror.example:8443/x")?));
    assert!(source.allows_redirect(&reqwest::Url::parse("https://cdn-lfs.hf.co/x")?));
    Ok(())
}
