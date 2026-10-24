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
    pub on_accent: Hsla,
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
        InterfaceTheme::CatppuccinLatte => (
            0xeff1f5, 0xe6e9ef, 0xccd0da, 0xbcc0cc, 0x4c4f69, 0x5c5f77, 0x1e66f5, 0xccd0da,
        ),
        InterfaceTheme::CatppuccinFrappe => (
            0x303446, 0x292c3c, 0x414559, 0x626880, 0xc6d0f5, 0xb5bfe2, 0x8caaee, 0x51576d,
        ),
        InterfaceTheme::CatppuccinMacchiato => (
            0x24273a, 0x1e2030, 0x363a4f, 0x5b6078, 0xcad3f5, 0xb8c0e0, 0x8aadf4, 0x494d64,
        ),
        InterfaceTheme::CatppuccinMocha => (
            0x1e1e2e, 0x181825, 0x313244, 0x585b70, 0xcdd6f4, 0xbac2de, 0x89b4fa, 0x45475a,
        ),
        InterfaceTheme::NordDark => (
            0x2e3440, 0x3b4252, 0x434c5e, 0x4c566a, 0xeceff4, 0xd8dee9, 0x88c0d0, 0x434c5e,
        ),
        InterfaceTheme::NordLight => (
            0xeceff4, 0xe5e9f0, 0xd8dee9, 0x81a1c1, 0x2e3440, 0x434c5e, 0x88c0d0, 0xd8dee9,
        ),
        InterfaceTheme::SolarizedDark => (
            0x002b36, 0x073642, 0x073642, 0x586e75, 0x93a1a1, 0x839496, 0x2aa198, 0x073642,
        ),
        InterfaceTheme::SolarizedLight => (
            0xfdf6e3, 0xeee8d5, 0xeee8d5, 0x93a1a1, 0x073642, 0x073642, 0x2aa198, 0xeee8d5,
        ),
    };
    let on_accent = match scheme {
        InterfaceTheme::CatppuccinFrappe
        | InterfaceTheme::CatppuccinMacchiato
        | InterfaceTheme::CatppuccinMocha => canvas,
        InterfaceTheme::NordDark | InterfaceTheme::NordLight => 0x2e3440,
        InterfaceTheme::SolarizedDark | InterfaceTheme::SolarizedLight => 0x002b36,
        _ => 0xffffff,
    };
    Palette {
        canvas: rgb(canvas).into(),
        panel: rgb(panel).into(),
        elevated: rgb(elevated).into(),
        border: rgb(border).into(),
        text: rgb(text).into(),
        secondary: rgb(secondary).into(),
        accent: rgb(accent).into(),
        on_accent: rgb(on_accent).into(),
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
        // Move away from the label luminance so hover/pressed retain contrast.
        let direction = if p.on_accent.l < 0.5 { 1. } else { -1. };
        c.primary_hover = Hsla {
            l: (p.accent.l + direction * 0.025).clamp(0., 1.),
            ..p.accent
        };
        c.primary_active = Hsla {
            l: (p.accent.l + direction * 0.05).clamp(0., 1.),
            ..p.accent
        };
        c.primary_foreground = p.on_accent;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Hsla) -> f32 {
        let c = color.to_rgb();
        let linear = |value: f32| {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }

    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn every_palette_keeps_body_and_primary_action_readable() {
        for scheme in InterfaceTheme::ALL {
            let p = palette(scheme);
            for background in [p.canvas, p.panel, p.elevated, p.selection] {
                assert!(contrast(p.text, background) >= 4.5, "{scheme:?}: body text");
            }
            assert!(
                contrast(p.accent, p.on_accent) >= 4.5,
                "{scheme:?}: primary action"
            );
        }
    }
}
