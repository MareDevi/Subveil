use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Instant,
};

fn fixture() -> anyhow::Result<Model> {
    let mut model = catalog()?.remove(0);
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
    assert_eq!(models.len(), 3);
    for model in models {
        let dir = root();
        fs::create_dir_all(&dir)?;
        let path = dir.join("model.toml");
        fs::write(&path, manifest_text(&model)?)?;
        let metadata = crate::model_metadata(&path)?;
        assert_eq!(metadata.kind, model.kind);
        assert_eq!(metadata.languages, model.languages);
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
    let path = runtime.block_on(install(&model, &root, &shared, Some(&url)))?;
    server
        .join()
        .map_err(|_| anyhow::anyhow!("server panicked"))??;
    assert!(path.is_file());
    assert!(crate::model_metadata(&path).is_ok());
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
        runtime.block_on(install(&model, &root, &shared, Some("http://127.0.0.1:1")))?,
        path
    );
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
                .block_on(install(&model, &root, &shared, Some(&url)))
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
            .block_on(install(&model, &root, &shared, Some("http://127.0.0.1:1")))
            .is_err()
    );
    drop(lock);
    shared.stop.store(true, Ordering::Relaxed);
    assert!(
        runtime
            .block_on(install(&model, &root, &shared, Some("http://127.0.0.1:1")))
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
fn cancellation_interrupts_stalled_http_and_cleans_unverified_bytes() -> anyhow::Result<()> {
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
    let result = runtime.block_on(install_with_cancel(&model, &root, &shared, Some(&url)));
    assert!(result.is_err_and(|error| error.to_string() == "下载已取消"));
    canceller
        .join()
        .map_err(|_| anyhow::anyhow!("canceller panic"))?;
    server
        .join()
        .map_err(|_| anyhow::anyhow!("server panic"))??;
    assert!(!model.manifest_path(&root).exists());
    assert!(
        fs::read_dir(model.manifest_path(&root).parent().context("parent")?)?
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().ends_with(".part"))
    );
    fs::remove_dir_all(root)?;
    Ok(())
}
