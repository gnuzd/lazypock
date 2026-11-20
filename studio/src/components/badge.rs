//! Small status pills: one size, one shape, tone decides the color.

use nsh_base::theme;
use nsh_core::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Info,
    Accent,
    Success,
    Warning,
    Danger,
}

pub(crate) fn tone_color(tone: Tone) -> AdaptiveColor {
    let t = theme();
    match tone {
        Tone::Neutral => t.muted_foreground,
        Tone::Info => t.color("info").unwrap_or(t.chart_2),
        Tone::Accent => t.primary,
        // Custom tokens from theme.json, with a chart color as fallback.
        Tone::Success => t.color("success").unwrap_or(t.chart_2),
        Tone::Warning => t.color("warning").unwrap_or(t.chart_3),
        Tone::Danger => t.destructive,
    }
}

pub fn badge(label: impl IntoReactive<SharedString>, tone: Tone) -> View {
    let fg = tone_color(tone);
    view()
        .flex_row()
        .items_center()
        .justify_center()
        .h(px(20.0))
        .px_2()
        .rounded_full()
        .bg(fg.opacity(0.15))
        .child(
            text(label)
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .number_of_lines(1)
                .text_center()
                .text_color(fg),
        )
}

/// The pill for an HTTP method: one color per verb.
pub fn method_badge(method: &'static str) -> View {
    let tone = match method {
        "GET" => Tone::Info,
        "POST" => Tone::Success,
        "PATCH" | "PUT" => Tone::Warning,
        "DELETE" => Tone::Danger,
        _ => Tone::Neutral,
    };
    badge(method, tone)
}

/// The pill for an HTTP status code.
pub fn status_badge(status: u16) -> View {
    let tone = match status {
        200..=299 => Tone::Success,
        300..=399 => Tone::Accent,
        400..=499 => Tone::Warning,
        _ => Tone::Danger,
    };
    badge(status.to_string(), tone)
}
