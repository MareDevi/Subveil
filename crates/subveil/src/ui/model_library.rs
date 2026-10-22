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
            .child(Input::new(&self.model_search).id("model-search").aria_label("搜索模型"))
            .child(self.render_choice(Choice::ModelLanguage, "筛选语言", false, cx))
            .child(self.render_choice(Choice::ModelMode, "筛选模式", false, cx))
            .child(help(format!("符合条件：识别 {} / 翻译 {}；当前所选模型始终保留，筛选不自动切换模型。",
                session.local_models.catalog.iter().filter(|m| session.model_filter.matches(m)).count(),
                session.translation_models.catalog.iter().filter(|m| session.model_filter.matches(m)).count()), cx))
            .child(help("选用建议：中英低延迟优先原生 Zipformer；日语目前使用 SenseVoice 句末识别或 Whisper 分块。显存紧张选 Qwen 0.5B，1.5B 需要更多内存。建议不是质量排名，仍须按实际语料比较。", cx))
            .child(section("下载源", "修改只影响显式安装；固定版本和 SHA-256 校验不变，不会自动回退其他站点。", cx))
            .child(Input::new(&self.download_origin).id("download-origin").aria_label("模型下载源").disabled(busy))
            .child({
                let input = self.download_origin.clone();
                let owner = self.session.clone();
                Button::new("apply-download-origin").label("应用下载源").disabled(busy)
                    .on_click(move |_,_,cx| {
                        let value = input.read(cx).value().to_string();
                        owner.update(cx,|s,cx|s.set_download_source(&value,cx));
                    })
            })
            .child(help(format!("当前下载端点：{}", if session.local_models.source_error { "设置无效，已暂停联网安装" } else { session.local_models.source.origin() }), cx))
            .child(help("自定义地址仅支持 HTTPS 站点根地址；不含音频或字幕，仅请求公开权重。离线模式仍禁止联网。", cx))
            .child(section(
                "语音识别",
                "选择适合源语言的模型；安装前查看来源、许可证和所需空间。",
                cx,
            ))
            .child(self.render_local_models(cx))
            .child(section("语音检测", "可选 Silero 神经 VAD；关闭时保留能量分段对照。", cx))
            .child(help(if session.asr_config.vad_model.is_some() { "当前使用 Silero VAD" } else { "当前使用能量阈值" }, cx))
            .when_some(session.vad_models.current(), |view, model| view
                .child(help(model.summary.clone(), cx))
                .child(Link::new("vad-source").href(model.source()).child("VAD 来源"))
                .child(Link::new("vad-license").href(model.license_url.clone()).child("VAD 许可：MIT")))
            .child(row()
                .child(button("install-vad", if session.offline { "校验并启用本机 VAD" } else { "安装并启用 Silero VAD" }, &self.session, |s, cx| {
                    s.vad_models.install(!s.offline); cx.notify();
                }).disabled(busy || session.vad_models.downloading()))
                .child(button("disable-vad", "使用能量分段", &self.session, |s, cx| { s.asr_config.vad_model = None; cx.notify(); }).disabled(busy))
                .child(button("cancel-vad", "取消 VAD 安装", &self.session, |s, cx| {s.vad_models.cancel();cx.notify();}).disabled(!session.vad_models.downloading())))
            .child(help(session.vad_models.snapshot.status.clone(), cx))
            .child(section(
                "本地翻译",
                "模型在本机运行；语音服务中选择本地翻译后生效。",
                cx,
            ))
            .child(self.render_local_translation(cx))
            .child(help("卸载仅删除所选目录模型的固定版本缓存；运行或安装期间不可卸载。", cx))
            .child(row().children([(0u8, "uninstall-asr", "卸载所选识别模型"), (1, "uninstall-mt", "卸载所选翻译模型"), (2, "uninstall-vad", "卸载 VAD")].into_iter().map(|(group, id, label)| {
                let owner = self.session.clone();
                Button::new(id).label(label).disabled(session.configuration_busy() || session.audio_active())
                    .on_click(move |_, window, cx| owner.update(cx, |s,cx|s.uninstall_model(group,window,cx)))
            })))
            .child(section("研究候选", "以下尚未接入，不能安装或启动；与已支持的模型分开展示。", cx))
            .child(help("Fujie Japanese Streaming Zipformer：原生 RNN-T，320 ms 块；输出假名与标记。CC BY 4.0；本轮下载元数据返回 401，未实测。", cx))
            .child(Link::new("candidate-ja-zipformer").href("https://huggingface.co/fujie/sherpa_onnx_asr_csj_cejc_pron_comp_zipformer_stream_c320_la0").child("日语流式模型卡"))
            .child(help("Vosk：连续识别，需新增运行时与质量对照。NeMo Streaming FastConformer：需缓存式运行时。Qwen3-ASR：须按具体实现验证缓存复用。", cx))
            .child(help("HY-MT1.5、MarianMT / OPUS-MT、NLLB、M2M100：需专用运行时或提示词适配，不等于原生同时翻译。SeamlessStreaming：端到端研究项，CC BY-NC 4.0，不作为默认后端。", cx))
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
        let mut view = column()
            .gap_2()
            .child(self.render_choice(Choice::Compute, "翻译运行设备", busy, cx))
            .child(help(session.gpu_status.clone(), cx))
            .child(help(
                "翻译支持 CPU / Vulkan；Whisper 可选 CPU / CUDA。切换设备前请停止识别。",
                cx,
            ));
        view = view.child(self.render_choice(Choice::TranslationModel, "翻译模型", busy, cx));
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
            view = view
                .child(self.render_choice(Choice::AsrCompute, "语音识别设备", busy, cx))
                .child(self.render_choice(Choice::WhisperBlock, "Whisper 分块", busy, cx))
                .child(help("短块更快显示字幕，可能切断词句；长块保留更多上下文。CUDA 需要专用构建，失败不会自动回退。", cx));
        }
        view.when(installing, |view| {
            view.child(
                Progress::new("asr-model-progress")
                    .small()
                    .accessibility_label("识别模型下载进度")
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
                        "install-model",
                        "安装并选用",
                        &self.session,
                        Session::install_local_model,
                    )
                    .disabled(busy || models.current().is_none()),
                )
                .child(
                    button(
                        "cancel-model",
                        "取消下载",
                        &self.session,
                        Session::cancel_model_install,
                    )
                    .disabled(!installing),
                )
                .child(
                    button(
                        "import-model",
                        "导入已有模型…",
                        &self.session,
                        Session::import_model,
                    )
                    .disabled(busy),
                ),
        )
        .child(help(
            "安装使用上方显示的下载端点，不上传音频。已下载文件校验后复用；安装不自动开始识别。",
            cx,
        ))
        .child(help(
            format!(
                "{} · {:.1} / {:.1} MB",
                models.snapshot.status,
                models.snapshot.completed as f64 / 1_000_000.,
                models.snapshot.total as f64 / 1_000_000.
            ),
            cx,
        ))
        .child(help(session.model_info.clone(), cx))
        .when_some(models.snapshot.available_bytes, |view, bytes| {
            view.child(help(
                format!("模型磁盘可用：{:.2} GiB", bytes as f64 / 1_073_741_824.),
                cx,
            ))
        })
    }
}
