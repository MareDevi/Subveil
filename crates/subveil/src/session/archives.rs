use super::*;

impl Session {
    pub fn export_archive(&mut self, format: &'static str, cx: &mut Context<Self>) {
        if self.archive_busy {
            return;
        }
        let archive = self
            .archive
            .clone()
            .unwrap_or_else(|| crate::archive::Archive::snapshot(&self.subtitles));
        if archive.records.is_empty() {
            self.archive_status = "没有已确定的字幕可以保存".into();
            cx.notify();
            return;
        }
        let directory = self
            .config_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .unwrap_or(std::path::Path::new("/tmp"));
        let picker = cx.prompt_for_new_path(directory, Some(&format!("subveil-session.{format}")));
        self.archive_busy = true;
        cx.notify();
        cx.spawn(async move |owner, cx| {
            let result: anyhow::Result<_> = async {
                let Some(path) = picker.await?? else {
                    return Ok(false);
                };
                cx.background_executor()
                    .spawn(async move {
                        crate::archive::save(&path, &archive.encode(format)?)?;
                        Ok(true)
                    })
                    .await
            }
            .await;
            let _ = owner.update(cx, |s, cx| {
                s.archive_busy = false;
                s.archive_status = match result {
                    Ok(true) => "字幕已保存；文件含原文与已完成译文，请按私人内容管理".into(),
                    Ok(false) => "已取消保存".into(),
                    Err(error) => format!("保存失败：{error}"),
                };
                cx.notify();
            });
        })
        .detach();
    }
    pub fn open_archive(&mut self, cx: &mut Context<Self>) {
        if self.archive_busy {
            return;
        }
        let picker = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("打开 Subveil TOML 字幕档案".into()),
        });
        self.archive_busy = true;
        cx.notify();
        cx.spawn(async move |owner, cx| {
            let result: anyhow::Result<_> = async {
                let Some(path) = picker.await??.and_then(|p| p.into_iter().next()) else {
                    return Ok(None);
                };
                cx.background_executor()
                    .spawn(async move { crate::archive::Archive::load(&path).map(Some) })
                    .await
            }
            .await;
            let _ = owner.update(cx, |s, cx| {
                s.archive_busy = false;
                match result {
                    Ok(Some(archive)) => {
                        s.archive_status = format!(
                            "正在查看已保存档案：{} 段；当前实时会话独立保留",
                            archive.records.len()
                        );
                        s.archive = Some(archive);
                    }
                    Ok(None) => {}
                    Err(error) => s.archive_status = format!("打开失败：{error}"),
                };
                cx.notify();
            });
        })
        .detach();
    }
}
