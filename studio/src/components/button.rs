//! The app's buttons: one look per variant, on top of `nsh_base::button`.

use nsh_base::{button, theme};
use nsh_core::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// The one action a screen is for.
    Primary,
    /// Everything else that acts.
    Secondary,
    /// Inline actions in lists and headers.
    Ghost,
}

/// `label`, pressed runs `on_press`; while `disabled()` dims it and it hears
/// nothing.
pub fn action_button(
    label: &'static str,
    variant: Variant,
    disabled: impl Fn() -> bool + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let (bg, fg, border) = match variant {
        Variant::Primary => (theme().primary, theme().primary_foreground, theme().primary),
        Variant::Secondary => (theme().background, theme().foreground, theme().border),
        Variant::Ghost => (
            AdaptiveColor::TRANSPARENT,
            theme().foreground,
            AdaptiveColor::TRANSPARENT,
        ),
    };
    let hover_bg = match variant {
        Variant::Primary => theme().primary.opacity(0.88),
        Variant::Secondary | Variant::Ghost => theme().accent,
    };

    button()
        .h(px(34.0))
        .px_3()
        .gap_2()
        .items_center()
        .justify_center()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .hover(move |s| s.bg(hover_bg))
        .focus_visible(move |s| s.ring(px(2.0)).ring_color(theme().ring))
        .disabled_style(|s| {
            s.bg(theme().muted)
                .border_color(theme().border)
                .text_color(theme().muted_foreground)
        })
        .disabled(disabled)
        .on_press(on_press)
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
}

/// An inline text link: an action that reads as text, unboxed.
pub fn text_link(label: &'static str, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .h(px(20.0))
        .px_0()
        .items_center()
        .justify_center()
        .text_color(move || theme().primary)
        .hover(move |s| s.text_color(theme().primary.opacity(0.75)))
        .focus_visible(move |s| s.ring(px(2.0)).ring_color(theme().ring))
        .on_press(on_press)
        .child(text(label).text_sm().font_weight(FontWeight::MEDIUM))
}
