//! Settings view: 7-page static rendering via embedded-graphics.
//!
//! Pages (matching `SettingsPage`):
//! 0 DeviceName  — name text + Random button
//! 1 Volume      — slider 0-100 + value
//! 2 Mute        — toggle switch + state
//! 3 Brightness  — slider 0-100 + value
//! 4 ScreenOffTime — option list (5/15/30/60/Always) + highlight
//! 5 About       — fw ver / reset reason / abnormal boot cnt / safe boot flag
//! 6 FactoryReset — two-step confirmation flow
//!
//! All text is ASCII (FONT_6X9). CJK font subset arrives in Phase 4.

use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle};
use embedded_graphics::text::Text;

use crate::apps::view::status_bar::STATUS_BAR_H;
use crate::apps::RenderCtx;
use crate::apps::settings::{FactoryResetState, SettingsPage};
use crate::services::display_buf::Rgb565Buf;

// Layout constants.
const TITLE_Y: i32 = 28;
const CONTENT_Y: i32 = 50;
const PAGE_INDICATOR_Y: i32 = 224;
const SCREEN_W: i32 = 240;
const SCREEN_H: i32 = 240;

// Colors.
const BG: Rgb565 = Rgb565::new(0x04, 0x06, 0x0C);
const PANEL: Rgb565 = Rgb565::new(0x18, 0x22, 0x30);
const ACCENT: Rgb565 = Rgb565::new(0x40, 0x80, 0xC0);
const TEXT: Rgb565 = Rgb565::WHITE;
const DIM: Rgb565 = Rgb565::new(0x80, 0x88, 0x90);
const GREEN: Rgb565 = Rgb565::new(0x20, 0xC0, 0x20);
const RED: Rgb565 = Rgb565::new(0xC0, 0x20, 0x20);
const YELLOW: Rgb565 = Rgb565::new(0xFF, 0xE0, 0x00);

/// Draw the Settings app for the given page. Caller has already drawn the
/// status bar. `ctx` provides settings/diag snapshot; `page` + `factory_state`
/// come from the SettingsApp model.
pub fn draw_settings(
    fb: &mut Rgb565Buf,
    ctx: &RenderCtx,
    page: SettingsPage,
    factory_state: FactoryResetState,
) {
    // Clear content area.
    let content = Rectangle::new(
        Point::new(0, STATUS_BAR_H as i32),
        Size::new(SCREEN_W as u32, (SCREEN_H - STATUS_BAR_H as i32) as u32),
    );
    let _ = content.into_styled(PrimitiveStyle::with_fill(BG)).draw(fb);

    // Title bar.
    let title = page_title(page);
    draw_title_bar(fb, title);

    // Page content.
    match page {
        SettingsPage::DeviceName => draw_device_name_page(fb, ctx),
        SettingsPage::Volume => draw_volume_page(fb, ctx),
        SettingsPage::Mute => draw_mute_page(fb, ctx),
        SettingsPage::Brightness => draw_brightness_page(fb, ctx),
        SettingsPage::ScreenOffTime => draw_screen_off_page(fb, ctx),
        SettingsPage::About => draw_about_page(fb, ctx),
        SettingsPage::FactoryReset => draw_factory_reset_page(fb, factory_state),
    }

    // Page indicator (e.g., "1/7").
    draw_page_indicator(fb, page);
}

fn page_title(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::DeviceName => "Device Name",
        SettingsPage::Volume => "Volume",
        SettingsPage::Mute => "Mute",
        SettingsPage::Brightness => "Brightness",
        SettingsPage::ScreenOffTime => "Screen Off",
        SettingsPage::About => "About",
        SettingsPage::FactoryReset => "Factory Reset",
    }
}

fn draw_title_bar(fb: &mut Rgb565Buf, title: &str) {
    // Title background strip.
    let bar = Rectangle::new(
        Point::new(0, STATUS_BAR_H as i32),
        Size::new(SCREEN_W as u32, 22),
    );
    let _ = bar
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x10, 0x18, 0x28)))
        .draw(fb);
    // Bottom border line.
    let _ = Line::new(
        Point::new(0, STATUS_BAR_H as i32 + 22),
        Point::new(SCREEN_W, STATUS_BAR_H as i32 + 22),
    )
    .into_styled(PrimitiveStyle::with_stroke(ACCENT, 1))
    .draw(fb);
    // Title text, centered.
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let tw = (title.len() as i32) * 6;
    let tx = (SCREEN_W - tw) / 2;
    let _ = Text::new(title, Point::new(tx, TITLE_Y), style).draw(fb);
}

fn draw_page_indicator(fb: &mut Rgb565Buf, page: SettingsPage) {
    let style = MonoTextStyle::new(&FONT_6X9, DIM);
    let idx = page as usize + 1;
    let text = format!("{}/7", idx);
    let tw = (text.len() as i32) * 6;
    let tx = (SCREEN_W - tw) / 2;
    let _ = Text::new(&text, Point::new(tx, PAGE_INDICATOR_Y), style).draw(fb);
}

// ---- Page 0: Device Name -------------------------------------------------

fn draw_device_name_page(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    // Label.
    let _ = Text::new("Name:", Point::new(16, CONTENT_Y + 10), dim_style).draw(fb);

    // Name value in a box.
    let name = &ctx.settings.device_name;
    let box_rect = Rectangle::new(
        Point::new(16, CONTENT_Y + 20),
        Size::new(208, 24),
    );
    let _ = box_rect
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(ACCENT)
                .stroke_width(1)
                .fill_color(PANEL)
                .build(),
        )
        .draw(fb);
    let _ = Text::new(name, Point::new(22, CONTENT_Y + 36), style).draw(fb);

    // Random button.
    let btn_rect = Rectangle::new(
        Point::new(60, CONTENT_Y + 80),
        Size::new(120, 28),
    );
    let _ = btn_rect
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_color(ACCENT)
                .stroke_width(2)
                .fill_color(Rgb565::new(0x20, 0x30, 0x48))
                .build(),
        )
        .draw(fb);
    let btn_label = "Random";
    let bw = (btn_label.len() as i32) * 6;
    let bx = 60 + (120 - bw) / 2;
    let _ = Text::new(btn_label, Point::new(bx, CONTENT_Y + 97), style).draw(fb);

    // Hint text.
    let _ = Text::new("Tap Random to generate", Point::new(48, CONTENT_Y + 130), dim_style).draw(fb);
}

// ---- Page 1: Volume ------------------------------------------------------

fn draw_volume_page(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    // Value text.
    let val_str = format!("{}%", ctx.settings.volume);
    let vw = (val_str.len() as i32) * 6;
    let vx = (SCREEN_W - vw) / 2;
    let _ = Text::new(&val_str, Point::new(vx, CONTENT_Y + 10), style).draw(fb);

    // Slider track.
    let track_y = CONTENT_Y + 50;
    let track = Rectangle::new(
        Point::new(20, track_y),
        Size::new(200, 8),
    );
    let _ = track.into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x30, 0x30, 0x38))).draw(fb);

    // Filled portion.
    let fill_w = ((ctx.settings.volume as u32) * 200 / 100) as u32;
    if fill_w > 0 {
        let fill = Rectangle::new(
            Point::new(20, track_y),
            Size::new(fill_w, 8),
        );
        let _ = fill.into_styled(PrimitiveStyle::with_fill(ACCENT)).draw(fb);
    }

    // Knob.
    let knob_x = 20 + (ctx.settings.volume as i32 * 200 / 100);
    let knob = Rectangle::new(
        Point::new(knob_x - 3, track_y - 4),
        Size::new(8, 16),
    );
    let _ = knob.into_styled(PrimitiveStyle::with_fill(TEXT)).draw(fb);

    // Min/Max labels.
    let _ = Text::new("0", Point::new(16, track_y + 16), dim_style).draw(fb);
    let _ = Text::new("100", Point::new(200, track_y + 16), dim_style).draw(fb);
}

// ---- Page 2: Mute --------------------------------------------------------

fn draw_mute_page(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    // Label.
    let _ = Text::new("Global Mute", Point::new(60, CONTENT_Y + 10), dim_style).draw(fb);

    // Toggle switch.
    let toggle_x = 90;
    let toggle_y = CONTENT_Y + 40;
    let toggle_w = 60;
    let toggle_h = 24;
    let (bg_color, knob_x) = if ctx.settings.muted {
        (RED, toggle_x + toggle_w as i32 - 22)
    } else {
        (GREEN, toggle_x + 4)
    };
    let toggle = Rectangle::new(
        Point::new(toggle_x, toggle_y),
        Size::new(toggle_w, toggle_h as u32),
    );
    let _ = toggle
        .into_styled(PrimitiveStyleBuilder::new().stroke_color(TEXT).stroke_width(1).fill_color(bg_color).build())
        .draw(fb);
    // Knob.
    let knob = Rectangle::new(
        Point::new(knob_x, toggle_y + 2),
        Size::new(18, (toggle_h - 4) as u32),
    );
    let _ = knob.into_styled(PrimitiveStyle::with_fill(TEXT)).draw(fb);

    // State text.
    let state_str = if ctx.settings.muted { "MUTED" } else { "ON" };
    let sw = (state_str.len() as i32) * 6;
    let sx = (SCREEN_W - sw) / 2;
    let _ = Text::new(state_str, Point::new(sx, CONTENT_Y + 90), style).draw(fb);
}

// ---- Page 3: Brightness --------------------------------------------------

fn draw_brightness_page(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let val_str = format!("{}%", ctx.settings.brightness);
    let vw = (val_str.len() as i32) * 6;
    let vx = (SCREEN_W - vw) / 2;
    let _ = Text::new(&val_str, Point::new(vx, CONTENT_Y + 10), style).draw(fb);

    // Slider track.
    let track_y = CONTENT_Y + 50;
    let track = Rectangle::new(Point::new(20, track_y), Size::new(200, 8));
    let _ = track.into_styled(PrimitiveStyle::with_fill(Rgb565::new(0x30, 0x30, 0x38))).draw(fb);

    let fill_w = ((ctx.settings.brightness as u32) * 200 / 100) as u32;
    if fill_w > 0 {
        let fill = Rectangle::new(Point::new(20, track_y), Size::new(fill_w, 8));
        let _ = fill.into_styled(PrimitiveStyle::with_fill(YELLOW)).draw(fb);
    }

    // Knob.
    let knob_x = 20 + (ctx.settings.brightness as i32 * 200 / 100);
    let knob = Rectangle::new(Point::new(knob_x - 3, track_y - 4), Size::new(8, 16));
    let _ = knob.into_styled(PrimitiveStyle::with_fill(TEXT)).draw(fb);

    let _ = Text::new("0", Point::new(16, track_y + 16), dim_style).draw(fb);
    let _ = Text::new("100", Point::new(200, track_y + 16), dim_style).draw(fb);
}

// ---- Page 4: Screen Off Time ---------------------------------------------

fn draw_screen_off_page(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let _ = Text::new("Auto Screen Off", Point::new(52, CONTENT_Y + 4), dim_style).draw(fb);

    // Option list: 5s / 15s / 30s / 60s / Always.
    let options: &[(u32, &str)] = &[
        (5, "5 sec"),
        (15, "15 sec"),
        (30, "30 sec"),
        (60, "60 sec"),
        (u32::MAX, "Always on"),
    ];
    let current = ctx.settings.screen_off_sec;
    let mut y = CONTENT_Y + 24;
    for &(sec, label) in options {
        let selected = sec == current;
        let row_bg = if selected { Rgb565::new(0x20, 0x38, 0x50) } else { BG };
        let row = Rectangle::new(Point::new(16, y), Size::new(208, 22));
        let _ = row.into_styled(PrimitiveStyle::with_fill(row_bg)).draw(fb);
        if selected {
            let _ = Line::new(Point::new(16, y), Point::new(16, y + 22))
                .into_styled(PrimitiveStyle::with_stroke(ACCENT, 3))
                .draw(fb);
        }
        let color = if selected { TEXT } else { DIM };
        let _ = Text::new(label, Point::new(28, y + 15), MonoTextStyle::new(&FONT_6X9, color)).draw(fb);
        if selected {
            let _ = Text::new("*", Point::new(200, y + 15), style).draw(fb);
        }
        y += 24;
    }
}

// ---- Page 5: About -------------------------------------------------------

fn draw_about_page(fb: &mut Rgb565Buf, ctx: &RenderCtx) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);

    let mut y = CONTENT_Y + 4;
    let row_h = 30;

    // Firmware version.
    let _ = Text::new("Firmware:", Point::new(16, y + 12), dim_style).draw(fb);
    let _ = Text::new(ctx.fw_version, Point::new(120, y + 12), style).draw(fb);
    y += row_h;

    // Reset reason (use English mapping since FONT_6X9 is ASCII-only).
    let _ = Text::new("Reset:", Point::new(16, y + 12), dim_style).draw(fb);
    let rr = reset_reason_en(ctx.reset_reason);
    let _ = Text::new(rr, Point::new(120, y + 12), style).draw(fb);
    y += row_h;

    // Abnormal boot count.
    let _ = Text::new("Abn. boots:", Point::new(16, y + 12), dim_style).draw(fb);
    let cnt = format!("{}", ctx.abnormal_boot_count);
    let _ = Text::new(&cnt, Point::new(120, y + 12), style).draw(fb);
    y += row_h;

    // Safe boot flag.
    let _ = Text::new("Safe boot:", Point::new(16, y + 12), dim_style).draw(fb);
    let flag = if ctx.safe_mode { "YES" } else { "NO" };
    let color = if ctx.safe_mode { YELLOW } else { DIM };
    let _ = Text::new(flag, Point::new(120, y + 12), MonoTextStyle::new(&FONT_6X9, color)).draw(fb);

    // Build time at bottom.
    y += row_h + 8;
    let _ = Text::new("Build:", Point::new(16, y + 12), dim_style).draw(fb);
    let bt = format!("{}", ctx.build_time);
    let _ = Text::new(&bt, Point::new(120, y + 12), style).draw(fb);
}

fn reset_reason_en(r: crate::services::power::ResetReason) -> &'static str {
    match r {
        crate::services::power::ResetReason::PowerOn => "Power On",
        crate::services::power::ResetReason::Brownout => "Brownout",
        crate::services::power::ResetReason::Wdt => "Watchdog",
        crate::services::power::ResetReason::Panic => "Panic",
        crate::services::power::ResetReason::Unknown => "Unknown",
    }
}

// ---- Page 6: Factory Reset -----------------------------------------------

fn draw_factory_reset_page(fb: &mut Rgb565Buf, state: FactoryResetState) {
    let style = MonoTextStyle::new(&FONT_6X9, TEXT);
    let dim_style = MonoTextStyle::new(&FONT_6X9, DIM);
    let warn_style = MonoTextStyle::new(&FONT_6X9, RED);

    match state {
        FactoryResetState::Idle => {
            // Warning text.
            let _ = Text::new("WARNING", Point::new(90, CONTENT_Y + 4), warn_style).draw(fb);
            let lines = [
                "This will erase all",
                "settings and group",
                "data. This cannot",
                "be undone.",
            ];
            let mut y = CONTENT_Y + 24;
            for line in &lines {
                let _ = Text::new(line, Point::new(40, y), dim_style).draw(fb);
                y += 14;
            }
            // Confirm button.
            let btn = Rectangle::new(Point::new(60, CONTENT_Y + 100), Size::new(120, 28));
            let _ = btn
                .into_styled(
                    PrimitiveStyleBuilder::new()
                        .stroke_color(RED)
                        .stroke_width(2)
                        .fill_color(Rgb565::new(0x30, 0x10, 0x10))
                        .build(),
                )
                .draw(fb);
            let label = "Confirm";
            let bw = (label.len() as i32) * 6;
            let bx = 60 + (120 - bw) / 2;
            let _ = Text::new(label, Point::new(bx, CONTENT_Y + 117), style).draw(fb);
        }
        FactoryResetState::FirstConfirm => {
            // Second confirmation screen.
            let _ = Text::new("Are you sure?", Point::new(60, CONTENT_Y + 4), warn_style).draw(fb);
            let _ = Text::new("All data will be lost.", Point::new(40, CONTENT_Y + 24), dim_style).draw(fb);

            // Cancel button.
            let cancel_btn = Rectangle::new(Point::new(20, CONTENT_Y + 60), Size::new(90, 28));
            let _ = cancel_btn
                .into_styled(
                    PrimitiveStyleBuilder::new()
                        .stroke_color(DIM)
                        .stroke_width(2)
                        .fill_color(PANEL)
                        .build(),
                )
                .draw(fb);
            let _ = Text::new("Cancel", Point::new(38, CONTENT_Y + 77), style).draw(fb);

            // Reset button.
            let reset_btn = Rectangle::new(Point::new(130, CONTENT_Y + 60), Size::new(90, 28));
            let _ = reset_btn
                .into_styled(
                    PrimitiveStyleBuilder::new()
                        .stroke_color(RED)
                        .stroke_width(2)
                        .fill_color(Rgb565::new(0x30, 0x10, 0x10))
                        .build(),
                )
                .draw(fb);
            let _ = Text::new("RESET", Point::new(150, CONTENT_Y + 77), warn_style).draw(fb);
        }
        FactoryResetState::SecondConfirmArmed => {
            // Settings have been reset — show success.
            let _ = Text::new("Reset Done", Point::new(78, CONTENT_Y + 40), MonoTextStyle::new(&FONT_6X9, GREEN)).draw(fb);
            let _ = Text::new("Rebooting...", Point::new(72, CONTENT_Y + 60), dim_style).draw(fb);
        }
    }
}

/// Settings touch field codes (carried in `HitTarget::SettingsControl { field }`).
pub mod field {
    pub const DEVICE_NAME: u8 = 0;
    pub const DEVICE_NAME_RANDOM: u8 = 1;
    pub const VOLUME: u8 = 2;
    pub const MUTE: u8 = 3;
    pub const BRIGHTNESS: u8 = 4;
    pub const SCREEN_OFF_5: u8 = 5;
    pub const SCREEN_OFF_15: u8 = 6;
    pub const SCREEN_OFF_30: u8 = 7;
    pub const SCREEN_OFF_60: u8 = 8;
    pub const SCREEN_OFF_ALWAYS: u8 = 9;
    pub const FACTORY_RESET_ARM: u8 = 10;
    pub const FACTORY_RESET_CANCEL: u8 = 11;
    pub const FACTORY_RESET_CONFIRM: u8 = 12;
}

/// Procedural hit-test for Settings pages. Returns the touched control as
/// `SettingsControl { field }` based on current page + layout, or `None`.
pub fn hit_test(
    x: i32,
    y: i32,
    page: SettingsPage,
    factory_state: FactoryResetState,
) -> Option<crate::apps::HitTarget> {
    use crate::apps::HitTarget;
    // All pages share the title bar strip (STATUS_BAR_H..STATUS_BAR_H+22);
    // taps there are not settings controls.
    let title_bottom = STATUS_BAR_H as i32 + 22;
    if y < title_bottom {
        return None;
    }
    match page {
        SettingsPage::DeviceName => {
            // Name box: x[16..224], y[70..94].
            if (16..224).contains(&x) && (70..94).contains(&y) {
                return Some(HitTarget::SettingsControl { field: field::DEVICE_NAME });
            }
            // Random button: x[60..180], y[130..158].
            if (60..180).contains(&x) && (130..158).contains(&y) {
                return Some(HitTarget::SettingsControl { field: field::DEVICE_NAME_RANDOM });
            }
            None
        }
        SettingsPage::Volume => {
            // Slider track tap zone: x[20..220], y[96..104] (with knob slack).
            if (20..220).contains(&x) && (96..104).contains(&y) {
                return Some(HitTarget::SettingsControl { field: field::VOLUME });
            }
            None
        }
        SettingsPage::Mute => {
            // Toggle: x[90..150], y[90..114].
            if (90..150).contains(&x) && (90..114).contains(&y) {
                return Some(HitTarget::SettingsControl { field: field::MUTE });
            }
            None
        }
        SettingsPage::Brightness => {
            if (20..220).contains(&x) && (96..104).contains(&y) {
                return Some(HitTarget::SettingsControl { field: field::BRIGHTNESS });
            }
            None
        }
        SettingsPage::ScreenOffTime => {
            // 5 rows starting y=74, each 24px tall. x[16..224].
            let row_h = 24;
            let row0_y = CONTENT_Y + 24; // 74
            if !(16..224).contains(&x) {
                return None;
            }
            let idx = (y - row0_y) / row_h;
            let f = match idx {
                0 => field::SCREEN_OFF_5,
                1 => field::SCREEN_OFF_15,
                2 => field::SCREEN_OFF_30,
                3 => field::SCREEN_OFF_60,
                4 => field::SCREEN_OFF_ALWAYS,
                _ => return None,
            };
            // Validate y is within the row's vertical span.
            if y >= row0_y && y < row0_y + 5 * row_h {
                Some(HitTarget::SettingsControl { field: f })
            } else {
                None
            }
        }
        SettingsPage::About => None, // read-only page
        SettingsPage::FactoryReset => match factory_state {
            FactoryResetState::Idle => {
                // Confirm button: x[60..180], y[150..178].
                if (60..180).contains(&x) && (150..178).contains(&y) {
                    Some(HitTarget::SettingsControl { field: field::FACTORY_RESET_ARM })
                } else {
                    None
                }
            }
            FactoryResetState::FirstConfirm => {
                // Cancel: x[20..110], y[110..138].
                if (20..110).contains(&x) && (110..138).contains(&y) {
                    return Some(HitTarget::SettingsControl { field: field::FACTORY_RESET_CANCEL });
                }
                // Reset: x[130..220], y[110..138].
                if (130..220).contains(&x) && (110..138).contains(&y) {
                    return Some(HitTarget::SettingsControl { field: field::FACTORY_RESET_CONFIRM });
                }
                None
            }
            FactoryResetState::SecondConfirmArmed => None, // post-reset, no targets
        },
    }
}
