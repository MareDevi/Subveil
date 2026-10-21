use super::*;

impl Session {
    /// Only curated cache revisions can be removed; arbitrary imports are never deleted.
    pub fn uninstall_model(
        &mut self,
        group: u8,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if self.configuration_busy() || self.audio_active() {
            return;
        }
        let library = match group {
            0 => &self.local_models,
            1 => &self.translation_models,
            _ => &self.vad_models,
        };
        let Some((model, root)) = library.current().cloned().zip(library.root.clone()) else {
            return;
        };
        let prompt = window.prompt(
            gpui_kit::PromptLevel::Warning,
            &format!("卸载 {}？", model.name),
            Some("删除该版本缓存和续传片段。导入目录与其他模型不会删除。需要时可重新安装。"),
            &["取消", "卸载"],
            cx,
        );
        self.importing_model = true;
        cx.notify();
        cx.spawn(async move |owner, cx| {
            let result: anyhow::Result<_> = async {
                if prompt.await? != 1 {
                    return Ok(None);
                }
                cx.background_executor()
                    .spawn(async move {
                        let manifest = model.manifest_path(&root);
                        model
                            .remove_cached(&root)
                            .map(|bytes| Some((manifest, bytes)))
                    })
                    .await
            }
            .await;
            let _ = owner.update(cx, |session, cx| {
                session.importing_model = false;
                match result {
                    Ok(Some((path, bytes))) => {
                        if session.asr_config.local_model.as_ref() == Some(&path) {
                            session.asr_config.local_model = None;
                        }
                        if session.translation_model.as_ref() == Some(&path) {
                            session.translation_model = None;
                        }
                        if session.asr_config.vad_model.as_ref() == Some(&path) {
                            session.asr_config.vad_model = None;
                        }
                        session.message =
                            format!("模型已卸载，释放 {:.1} MB", bytes as f64 / 1_000_000.);
                        session.save_local_settings(cx);
                    }
                    Ok(None) => {}
                    Err(error) => session.message = format!("卸载失败：{error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
