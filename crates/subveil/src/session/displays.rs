//! Output identity and hotplug policy; subtitle and recognition state outlive surfaces.
use super::*;
use gpui_kit::App;

const AUTOMATIC: &str = "自动 · 最左侧屏幕";

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Output {
    pub uuid: String,
    pub label: String,
}

pub(super) fn available(cx: &App) -> Vec<Output> {
    let mut displays = cx.displays();
    displays.sort_by(|a, b| {
        let a = a.bounds().origin;
        let b = b.bounds().origin;
        f32::from(a.x)
            .total_cmp(&f32::from(b.x))
            .then_with(|| f32::from(a.y).total_cmp(&f32::from(b.y)))
    });
    let mut outputs = Vec::new();
    for display in displays {
        let Ok(uuid) = display.uuid() else { continue };
        let uuid = uuid.to_string();
        // A duplicate stable ID cannot safely identify one output.
        if cx
            .displays()
            .iter()
            .filter(|d| d.uuid().is_ok_and(|id| id.to_string() == uuid))
            .count()
            != 1
        {
            continue;
        }
        let bounds = display.bounds();
        outputs.push(Output {
            label: format!(
                "{} · {:.0}×{:.0} · ({:.0}, {:.0})",
                uuid,
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y)
            ),
            uuid,
        });
    }
    outputs
}

impl Session {
    pub(super) fn watch_outputs(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        self._display_subscription = Some(cx.observe_displays(move |_, cx| {
            let weak = weak.clone();
            // Platform callbacks may occur inside a window update.
            cx.defer(move |cx| {
                let _ = weak.update(cx, |s, cx| s.refresh_outputs(cx));
            });
        }));
        self.outputs = available(cx);
    }

    pub(super) fn refresh_outputs(&mut self, cx: &mut Context<Self>) {
        self.outputs = available(cx);
        if self.overlay_requested {
            self.close_overlay(cx);
            self.show_overlay(cx);
        }
        cx.notify();
    }

    pub(super) fn output_labels(&self) -> Vec<String> {
        let mut labels = vec![AUTOMATIC.into()];
        labels.extend(self.outputs.iter().map(|o| o.label.clone()));
        let selected = self.output_value();
        if !labels.contains(&selected) {
            labels.push(selected);
        }
        labels
    }

    pub(super) fn output_value(&self) -> String {
        match &self.settings.output_uuid {
            None => AUTOMATIC.into(),
            Some(id) => self
                .outputs
                .iter()
                .find(|o| o.uuid == *id)
                .map(|o| o.label.clone())
                .unwrap_or_else(|| format!("已断开 · {id}")),
        }
    }

    pub(super) fn select_output(&mut self, label: &str, cx: &mut Context<Self>) {
        let uuid = if label == AUTOMATIC {
            None
        } else if let Some(output) = self.outputs.iter().find(|o| o.label == label) {
            Some(output.uuid.clone())
        } else {
            return;
        };
        self.change_settings(|s| s.output_uuid = uuid, cx);
    }
}

impl Session {
    pub fn toggle_desktop_service(&mut self, shortcuts: bool, cx: &mut Context<Self>) {
        let (service, status) = if shortcuts {
            (&mut self.shortcuts_service, &mut self.shortcuts_status)
        } else {
            (&mut self.tray_service, &mut self.tray_status)
        };
        if service.take().is_some() {
            *status = "已请求停用桌面服务".into();
        } else {
            match crate::desktop::Service::start(shortcuts) {
                Ok(worker) => {
                    *status = worker.poll().1;
                    *service = Some(worker);
                }
                Err(error) => *status = format!("桌面服务启动失败：{error}"),
            }
        }
        cx.notify();
    }
    pub(super) fn poll_desktop(&mut self, cx: &mut Context<Self>) {
        let mut commands = 0;
        let mut changed = false;
        for (service, status) in [
            (&self.tray_service, &mut self.tray_status),
            (&self.shortcuts_service, &mut self.shortcuts_status),
        ] {
            if let Some(service) = service {
                let (pending, text) = service.poll();
                commands = ((commands | pending) & !crate::desktop::TOGGLE)
                    | ((commands ^ pending) & crate::desktop::TOGGLE);
                if *status != text {
                    *status = text;
                    changed = true;
                }
            }
        }
        // Stop wins over visibility changes. Desktop commands never start capture or upload.
        if commands & crate::desktop::STOP != 0 {
            self.stop(cx);
        }
        if commands & crate::desktop::QUIT != 0 {
            self.stop(cx);
            cx.quit();
            return;
        }
        if commands & crate::desktop::RESTORE != 0 {
            if let Some(handle) = self.control {
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, _| window.activate_window());
                });
            } else {
                self.restore_control(cx);
            }
        }
        if commands & crate::desktop::TOGGLE != 0 {
            self.toggle_overlay(cx);
        }
        if changed {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AppContext, DisplayId, TestAppContext};

    #[gpui_kit::test]
    #[allow(clippy::expect_used)]
    fn hotplug_preserves_fixed_selection_and_subtitles(cx: &mut TestAppContext) {
        let owner = cx.update(|cx| {
            cx.new(|cx| {
                let mut session = Session::initial(Default::default(), None, None, Task::ready(()));
                session.watch_outputs(cx);
                session.running = true;
                crate::mock::advance(&mut session.subtitles, 8, 2000);
                session
            })
        });
        cx.simulate_display_added(DisplayId::new(42), None);
        cx.update(|_| {});
        cx.run_until_parked();
        let selected = owner.update(cx, |s, cx| {
            assert_eq!(s.outputs.len(), 2);
            let output = s.outputs.last().expect("second output").clone();
            s.select_output(&output.label, cx);
            output.uuid
        });
        cx.simulate_display_removed(DisplayId::new(42));
        cx.update(|_| {});
        cx.run_until_parked();
        owner.update(cx, |s, cx| {
            assert_eq!(s.outputs.len(), 1);
            assert_eq!(s.settings.output_uuid.as_deref(), Some(selected.as_str()));
            assert!(s.output_value().starts_with("已断开"));
            assert!(s.running);
            assert!(!s.subtitles.history().is_empty());
            s.select_output(AUTOMATIC, cx);
            assert!(s.settings.output_uuid.is_none());
        });
    }
}
