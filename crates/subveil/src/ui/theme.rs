//! Product palette definitions. Components consume semantic roles, never palette literals.
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, px, rgb};
use subveil_core::config::InterfaceTheme;

/// The same structural roles exist in every palette. Status colors come from the
/// matching upstream light/dark theme, keeping meaning independent of brand accent.
pub(super) struct Palette {
    pub canvas: Hsla,
    pub panel: Hsla,
    pub elevated: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub secondary: Hsla,
    pub accent: Hsla,
    pub selection: Hsla,
}

pub(super) fn palette(scheme: InterfaceTheme) -> Palette {
    let (canvas, panel, elevated, border, text, secondary, accent, selection) = match scheme {
        InterfaceTheme::GraphiteDark => (
            0x1e2227, 0x252a31, 0x30363e, 0x3e4651, 0xe3e7ed, 0xa6aebb, 0x416ecc, 0x34445d,
        ),
        InterfaceTheme::GraphiteLight => (
            0xfafafa, 0xf0f1f3, 0xe6e8ec, 0xc8cdd5, 0x252a32, 0x606977, 0x315fb8, 0xdce6f7,
        ),
        InterfaceTheme::IndigoDark => (
            0x1c2030, 0x23283c, 0x2e3550, 0x424d6b, 0xe0e5f4, 0xa6b1cf, 0x6864c9, 0x3c3c65,
        ),
        InterfaceTheme::IndigoLight => (
            0xf8f9ff, 0xeeeff9, 0xe1e4f4, 0xc5c9e1, 0x292f49, 0x626b8b, 0x6055b5, 0xe2ddfa,
        ),
        InterfaceTheme::SandDark => (
            0x25231f, 0x2e2b25, 0x3c372e, 0x504a3e, 0xe9e2d5, 0xb8ae9c, 0x97662a, 0x50412e,
        ),
        InterfaceTheme::SandLight => (
            0xfaf6ed, 0xf0eadd, 0xe8dfcd, 0xcec2ac, 0x39342b, 0x706552, 0x865b22, 0xedddbf,
        ),
    };
    Palette {
        canvas: rgb(canvas).into(),
        panel: rgb(panel).into(),
        elevated: rgb(elevated).into(),
        border: rgb(border).into(),
        text: rgb(text).into(),
        secondary: rgb(secondary).into(),
        accent: rgb(accent).into(),
        selection: rgb(selection).into(),
    }
}

pub(super) fn apply(scheme: InterfaceTheme, cx: &mut App) {
    // Change mode first: it reloads upstream defaults, including semantic status
    // colors. A second update synchronizes Component colors, paint tokens and Base.
    Theme::change(
        if scheme.is_dark() {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let p = palette(scheme);
    Theme::update(cx, |theme| {
        theme.radius = px(4.);
        theme.radius_lg = px(8.);
        theme.shadow = false;
        let c = &mut theme.colors;
        c.background = p.canvas;
        c.foreground = p.text;
        c.muted = p.elevated;
        c.muted_foreground = p.secondary;
        c.border = p.border;
        c.input = p.border;
        c.ring = p.accent;
        c.accent = p.selection;
        c.accent_foreground = p.text;
        c.primary = p.accent;
        c.primary_hover = Hsla {
            l: (p.accent.l + 0.025).min(1.),
            ..p.accent
        };
        c.primary_active = Hsla {
            l: (p.accent.l - 0.04).max(0.),
            ..p.accent
        };
        c.primary_foreground = rgb(0xffffff).into();
        c.button_primary = c.primary;
        c.button_primary_hover = c.primary_hover;
        c.button_primary_active = c.primary_active;
        c.button_primary_foreground = c.primary_foreground;
        c.button = p.panel;
        c.button_hover = p.elevated;
        c.button_active = p.selection;
        c.button_foreground = p.text;
        c.secondary = p.elevated;
        c.secondary_foreground = p.text;
        c.secondary_hover = p.selection;
        c.secondary_active = p.selection;
        c.sidebar = p.panel;
        c.sidebar_foreground = p.secondary;
        c.sidebar_border = p.border;
        c.sidebar_accent = p.selection;
        c.sidebar_accent_foreground = p.text;
        c.sidebar_primary = p.accent;
        c.sidebar_primary_foreground = c.primary_foreground;
        c.title_bar = p.panel;
        c.title_bar_border = p.border;
        c.status_bar = p.panel;
        c.status_bar_border = p.border;
        c.tab = p.panel;
        c.tab_bar = p.panel;
        c.tab_active = p.canvas;
        c.tab_foreground = p.secondary;
        c.tab_active_foreground = p.text;
        c.tab_bar_segmented = p.elevated;
        c.group_box = p.panel;
        c.group_box_foreground = p.text;
        c.popover = p.panel;
        c.popover_foreground = p.text;
        c.list = p.panel;
        c.list_hover = p.elevated;
        c.list_active = p.selection;
        c.list_active_border = p.accent;
        c.list_head = p.panel;
        c.selection = p.selection;
        c.scrollbar_thumb = p.border;
        c.scrollbar_thumb_hover = p.secondary;
        c.window_border = p.border;
        c.link = p.secondary;
        c.link_hover = p.text;
        c.link_active = p.text;
        c.caret = p.text;
    });
}
