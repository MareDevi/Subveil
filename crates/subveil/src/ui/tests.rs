//! Exercise production controls without devices, credentials, helpers or network.
#![allow(clippy::expect_used)]
use super::*;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{Task, TestAppContext};

#[gpui_kit::test]
fn stop_preserves_history_clear_rejects_running_and_navigation_restores_focus(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
    });
    let (handle, view, session) = cx.update(|cx| {
        let session = cx.new(|_| {
            let mut session = Session::initial(Default::default(), None, None, Task::ready(()));
            session.restoring_local = false;
            session.running = true;
            crate::mock::advance(&mut session.subtitles, 8, 2000);
            crate::mock::advance(&mut session.subtitles, 12, 3000);
            session
        });
        let owner = session.clone();
        let (handle, view) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1100.), px(2000.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| ControlView::new(owner, window, cx)),
        )
        .expect("open production view");
        (handle, view, session)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("clear-history").disabled(), Some(true));
        window.click("clear-history", cx);
        assert_eq!(session.read(cx).subtitles.history().len(), 1);
        window.click("stop", cx);
        assert!(!session.read(cx).running);
        assert_eq!(session.read(cx).subtitles.history().len(), 1);
        window.click("toml", cx);
    })
    .expect("export through production button");
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path());
    let path =
        std::env::temp_dir().join(format!("subveil-view-export-{}.toml", std::process::id()));
    assert!(!path.exists(), "no automatic transcript write");
    cx.simulate_new_path_selection(|_| Some(path.clone()));
    cx.run_until_parked();
    let saved = crate::archive::Archive::load(&path).expect("read explicit export");
    assert_eq!(saved.records.len(), 1);
    assert!(saved.records[0].translation.is_some());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::remove_file(path).expect("remove test export");
    cx.update_window(handle, |_, window, cx| {
        window.click("clear-history", cx);
        assert!(session.read(cx).subtitles.history().is_empty());
        window.click("configure-session", cx);
    })
    .expect("dispatch production buttons");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(view.read(cx).page, 1);
        assert!(view.read(cx).focus.is_focused(window));
        window.press("ctrl-1", cx);
    })
    .expect("navigate from restored focus");
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).page, 0));
    cx.simulate_window_resize(handle, size(px(1000.), px(800.)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("start-live-subtitles").visible());
        let preview = window.find("live-preview");
        assert!(preview.visible());
        assert!(
            preview.bounds().bottom() <= px(800.),
            "primary preview should fit first viewport"
        );
    })
    .expect("verify compact production layout");
}

#[gpui_kit::test]
fn model_search_preserves_selection_and_endpoint_requires_explicit_apply(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
    });
    let (handle, session) = cx.update(|cx| {
        let owner = cx.new(|_| {
            let mut s = Session::initial(Default::default(), None, None, Task::ready(()));
            s.restoring_local = false;
            s
        });
        let session = owner.clone();
        let (handle, _) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1100.), px(2000.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| ControlView::new(owner, window, cx)),
        )
        .expect("model library window");
        (handle, session)
    });
    let original = cx.update(|cx| {
        session
            .read(cx)
            .local_models
            .current()
            .map(|m| m.id.clone())
    });
    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-5", cx);
    })
    .expect("open models");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.click("model-search", cx);
        window.input("no-match-97531", cx);
    })
    .expect("type model filter");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        let s = session.read(cx);
        assert_eq!(s.model_filter.query, "no-match-97531");
        assert_eq!(s.local_models.current().map(|m| m.id.clone()), original);
        assert!(
            s.local_models
                .catalog
                .iter()
                .all(|m| !s.model_filter.matches(m))
        );
        window.click("download-origin", cx);
        window.press("ctrl-a", cx);
        window.input("https://mirror.example", cx);
        assert_eq!(
            session.read(cx).local_models.source.origin(),
            "https://huggingface.co"
        );
        window.click("apply-download-origin", cx);
    })
    .expect("apply explicit endpoint");
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(
            session.read(cx).local_models.source.origin(),
            "https://mirror.example"
        );
        assert!(!session.read(cx).local_models.downloading());
        window.click("download-origin", cx);
        window.press("ctrl-a", cx);
        window.input("http://insecure.example", cx);
        window.click("apply-download-origin", cx);
    })
    .expect("reject insecure endpoint");
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            session.read(cx).local_models.source.origin(),
            "https://mirror.example"
        )
    });
}

#[gpui_kit::test]
fn hy_mt_install_requires_terms_and_cancel_does_not_download(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
    });
    let (handle, session) = cx.update(|cx| {
        let session = cx.new(|_| {
            let mut s = Session::initial(Default::default(), None, None, Task::ready(()));
            s.restoring_local = false;
            s.translation_models.selected = s
                .translation_models
                .catalog
                .iter()
                .position(|m| m.kind == subveil_asr::ModelKind::HyMtTranslation)
                .expect("HY-MT catalog");
            s
        });
        let owner = session.clone();
        let (handle, _) = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| ControlView::new(owner, window, cx))
        })
        .expect("open view");
        (handle, session)
    });
    cx.update_window(handle, |_, window, cx| {
        session.update(cx, |s, cx| s.confirm_translation_install(window, cx));
        assert!(!session.read(cx).translation_models.downloading());
    })
    .expect("request consent");
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    let (_, detail) = cx.pending_prompt().expect("terms prompt");
    assert!(detail.contains("欧盟、英国及韩国"));
    assert!(detail.contains("265b2e615a7dc9b06c435dc878829ad99a512ba2/License.txt"));
    cx.simulate_prompt_answer("取消");
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!session.read(cx).importing_model);
        assert!(!session.read(cx).translation_models.downloading());
        assert!(session.read(cx).translation_model.is_none());
    });
}
