use super::*;

impl ControlView {
    pub(super) fn render_models(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let busy = session.asr_active()
            || session.asr_stopping
            || session.importing_model
            || session.local_models.downloading()
            || session.translation_models.downloading()
            || session.restoring_local;
        column()
            .child(heading(
                "模型库",
                "安装、校验和选用本地模型。开始识别仍需前往语音服务。",
                cx,
            ))
            .child(section(
                "语音识别",
                "选择适合源语言的模型；安装前查看来源、许可证和所需空间。",
                cx,
            ))
            .child(self.render_local_models(cx))
            .child(section(
                "本地翻译",
                "模型在本机运行；语音服务中选择本地翻译后生效。",
                cx,
            ))
            .child(self.render_local_translation(cx))
            .child(
                button(
                    "save-model-settings",
                    "保存本地设置",
                    &self.session,
                    Session::save_local_settings,
                )
                .disabled(busy || session.saving_local)
                .loading(session.saving_local),
            )
    }

    pub(super) fn render_local_translation(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let models = &session.translation_models;
        let busy = session.asr_active()
            || session.asr_stopping
            || session.importing_model
            || session.local_models.downloading()
            || models.downloading()
            || session.restoring_local;
        let compute = match session.compute {
            subveil_translate::Compute::Cpu => "CPU".to_owned(),
            subveil_translate::Compute::Gpu(index) => session
                .gpu_devices
                .iter()
                .find(|d| d.index == index)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "GPU 不可用".into()),
        };
        let mut view = column()
            .gap_2()
            .child(
                button(
                    "translation-compute",
                    format!("翻译运行设备：{compute}"),
                    &self.session,
                    Session::cycle_compute,
                )
                .disabled(busy || session.gpu_devices.is_empty()),
            )
            .child(help(session.gpu_status.clone(), cx))
            .child(help(
                "翻译支持 CPU / Vulkan；Whisper 可选 CPU / CUDA。切换设备前请停止识别。",
                cx,
            ));
        if let Some(model) = models.current() {
            view = view
                .child(help(
                    format!(
                        "{} · {:.1} MB · 中 / 英 / 日",
                        model.name,
                        model.size() as f64 / 1_000_000.
                    ),
                    cx,
                ))
                .child(help(model.summary.clone(), cx))
                .child(
                    row()
                        .child(
                            Link::new("translation-source")
                                .href(model.source())
                                .child("模型来源"),
                        )
                        .child(
                            Link::new("translation-license")
                                .href(model.license_url.clone())
                                .child(format!("许可：{}", model.license)),
                        ),
                );
        }
        view.when(models.downloading(), |view| {
            view.child(
                Progress::new("translation-progress")
                    .small()
                    .accessibility_label("翻译模型下载进度")
                    .loading(models.snapshot.total == 0)
                    .value(
                        models.snapshot.completed as f32 / models.snapshot.total.max(1) as f32
                            * 100.,
                    ),
            )
        })
        .child(
            row()
                .child(
                    button(
                        "install-translation",
                        if session.offline {
                            "校验并选用本机翻译模型"
                        } else {
                            "安装并选用翻译模型"
                        },
                        &self.session,
                        Session::install_translation_model,
                    )
                    .disabled(busy),
                )
                .child(
                    button(
                        "cancel-translation",
                        "取消翻译模型下载",
                        &self.session,
                        Session::cancel_translation_install,
                    )
                    .disabled(!models.downloading()),
                )
                .child(
                    button(
                        "import-translation",
                        "导入翻译清单…",
                        &self.session,
                        Session::import_model,
                    )
                    .disabled(busy),
                ),
        )
        .child(help(
            format!(
                "{} · {:.1} / {:.1} MB",
                models.snapshot.status,
                models.snapshot.completed as f64 / 1_000_000.,
                models.snapshot.total as f64 / 1_000_000.
            ),
            cx,
        ))
        .child(help(
            format!(
                "翻译模型：{}",
                session
                    .translation_model
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "尚未选用".into())
            ),
            cx,
        ))
        .when_some(models.snapshot.available_bytes, |view, bytes| {
            view.child(help(
                format!("模型磁盘可用：{:.2} GiB", bytes as f64 / 1_073_741_824.),
                cx,
            ))
        })
    }

    pub(super) fn render_local_models(&self, cx: &App) -> gpui_kit::Div {
        let session = self.session.read(cx);
        let models = &session.local_models;
        let installing = models.downloading();
        let busy = session.asr_active()
            || session.asr_stopping
            || session.importing_model
            || installing
            || session.translation_models.downloading()
            || session.restoring_local;
        let mut view = column().child(field(
            "识别模型",
            Select::new(&self.local_model)
                .disabled(busy)
                .accessibility_label("本地模型目录")
                .w_full(),
            cx,
        ));
        if let Some(model) = models.current() {
            view = view
                .child(help(
                    format!(
                        "{} · {} · {:.1} MB · {}",
                        model.kind.name(),
                        model.languages.join(" / "),
                        model.size() as f64 / 1_000_000.,
                        if model.kind == subveil_asr::ModelKind::Whisper {
                            "CPU / CUDA"
                        } else {
                            "CPU"
                        }
                    ),
                    cx,
                ))
                .child(help(model.summary.clone(), cx))
                .child(help(format!("来源：{}", model.repository), cx))
                .child(
                    row()
                        .child(
                            Link::new("model-source")
                                .href(model.source())
                                .child("模型来源"),
                        )
                        .child(
                            Link::new("model-license")
                                .href(model.license_url.clone())
                                .child(format!("许可：{}", model.license)),
                        ),
                )
                .child(help(
                    format!(
                        "存储目录：{}",
                        models
                            .root
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "不可用".into())
                    ),
                    cx,
                ));
        }
        if models
            .current()
            .is_some_and(|m| m.kind == subveil_asr::ModelKind::Whisper)
            || session.local_kind == subveil_asr::ModelKind::Whisper
        {
            let device = match session.asr_config.local_compute {
                subveil_translate::Compute::Cpu => "CPU",
                _ => "NVIDIA CUDA0",
            };
            view = view
                .child(field("语音识别设备", button("asr-compute", device, &self.session, Session::cycle_asr_compute).disabled(busy), cx))
                .child(field("Whisper 分块", button("whisper-block", format!("{} 秒", session.asr_config.whisper_block_ms / 1000), &self.session, Session::cycle_whisper_block).disabled(busy), cx))
                .child(help("短块更快显示字幕，可能切断词句；长块保留更多上下文。CUDA 需要专用构建，失败不会自动回退。", cx));
        }
        view
            .when(installing, |view| view.child(
                Progress::new("asr-model-progress").small().accessibility_label("识别模型下载进度")
                    .loading(models.snapshot.total == 0)
                    .value(models.snapshot.completed as f32 / models.snapshot.total.max(1) as f32 * 100.)))
            .child(row()
                .child(button("install-model", "安装并选用", &self.session, Session::install_local_model).disabled(busy || models.current().is_none()))
                .child(button("cancel-model", "取消下载", &self.session, Session::cancel_model_install).disabled(!installing))
                .child(button("import-model", "导入已有模型…", &self.session, Session::import_model).disabled(busy)))
            .child(help("安装会从 Hugging Face 下载模型文件，不上传音频。已下载文件校验后复用；安装不自动开始识别。", cx))
            .child(help(format!("{} · {:.1} / {:.1} MB", models.snapshot.status, models.snapshot.completed as f64 / 1_000_000., models.snapshot.total as f64 / 1_000_000.), cx))
            .child(help(session.model_info.clone(), cx))
            .when_some(models.snapshot.available_bytes, |view, bytes| view.child(help(format!("模型磁盘可用：{:.2} GiB", bytes as f64 / 1_073_741_824.), cx)))
    }
}
