use egui::{Color32, CornerRadius, Shadow, Stroke, Style, Theme, Visuals, style::ScrollAnimation};

pub const ACCENT: Color32 = Color32::from_rgb(0x4c, 0x8d, 0xff);
pub const OK: Color32 = Color32::from_rgb(0x4c, 0xc3, 0x7a);
pub const ERR: Color32 = Color32::from_rgb(0xef, 0x5b, 0x5b);
pub const WARN: Color32 = Color32::from_rgb(0xe8, 0xb3, 0x4a);

/// Surface colours the app paints itself (sidebar, console).
pub struct Surfaces {
    pub sidebar: Color32,
    pub main: Color32,
    pub console: Color32,
    pub border: Color32,
}

pub fn surfaces(dark: bool) -> Surfaces {
    if dark {
        Surfaces {
            sidebar: Color32::from_rgb(0x12, 0x12, 0x13),
            main: Color32::from_rgb(0x17, 0x17, 0x18),
            console: Color32::from_rgb(0x13, 0x13, 0x14),
            border: Color32::from_rgb(0x26, 0x26, 0x28),
        }
    } else {
        Surfaces {
            sidebar: Color32::from_rgb(0xee, 0xee, 0xf0),
            main: Color32::from_rgb(0xfa, 0xfa, 0xfa),
            console: Color32::from_rgb(0xf3, 0xf3, 0xf5),
            border: Color32::from_rgb(0xdc, 0xdc, 0xe0),
        }
    }
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_style_of(Theme::Dark, style(dark_visuals()));
    ctx.set_style_of(Theme::Light, style(light_visuals()));
}

fn style(visuals: Visuals) -> Style {
    let mut s = Style { visuals, ..Style::default() };
    s.animation_time = 0.0;
    s.scroll_animation = ScrollAnimation::none();
    s.spacing.item_spacing = egui::vec2(8.0, 6.0);
    s.spacing.button_padding = egui::vec2(10.0, 4.0);
    s.spacing.interact_size.y = 24.0;
    s.interaction.tooltip_delay = 0.25;
    s.url_in_tooltip = true;
    s
}

fn tune(v: &mut Visuals, s: &Surfaces, input: Color32) {
    let r = CornerRadius::same(4);
    v.panel_fill = s.main;
    v.window_fill = s.main;
    v.window_stroke = Stroke::new(1.0, s.border);
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.window_corner_radius = r;
    v.menu_corner_radius = r;
    v.extreme_bg_color = input;
    v.text_edit_bg_color = Some(input);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.error_fg_color = ERR;
    v.warn_fg_color = WARN;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, s.border);
}

fn dark_visuals() -> Visuals {
    let s = surfaces(true);
    let mut v = Visuals::dark();
    tune(&mut v, &s, Color32::from_rgb(0x0f, 0x0f, 0x10));
    v.faint_bg_color = Color32::from_rgb(0x1c, 0x1c, 0x1e);
    v.code_bg_color = Color32::from_rgb(0x1f, 0x1f, 0x21);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x26, 0x26, 0x29);
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x26, 0x26, 0x29);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x30, 0x30, 0x34);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x30, 0x30, 0x34);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x38, 0x38, 0x3d);
    v.widgets.active.bg_fill = Color32::from_rgb(0x38, 0x38, 0x3d);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x44, 0x44, 0x49));
    v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(0xc8, 0xc8, 0xcc);
    v.widgets.inactive.fg_stroke.color = Color32::from_rgb(0xdc, 0xdc, 0xe0);
    v
}

fn light_visuals() -> Visuals {
    let s = surfaces(false);
    let mut v = Visuals::light();
    tune(&mut v, &s, Color32::WHITE);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xe6, 0xe6, 0xea);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xdc, 0xdc, 0xe2);
    v
}
