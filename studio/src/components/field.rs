//! Form fields: a label over a themed input.

use nsh_base::theme;
use nsh_core::element::input;
use nsh_core::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Email,
    Password,
}

/// `label` over an input bound to `value` (two-way: typing sets it).
pub fn field(
    label: &'static str,
    placeholder: &'static str,
    value: Signal<String>,
    kind: FieldKind,
) -> View {
    let mut control = input()
        .value(move || value.get())
        .on_change(move |text: &str| value.set(text.to_owned()))
        .placeholder(placeholder)
        .placeholder_color(|| color(theme().muted_foreground))
        .caret_color(|| color(theme().primary))
        .label(label)
        .h(px(38.0))
        .px_3()
        .text_sm()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().input)
        .bg(move || theme().background)
        .focus(move |s| s.border_color(theme().ring));

    control = match kind {
        FieldKind::Text => control,
        FieldKind::Email => control
            .keyboard_type(KeyboardType::Email)
            .auto_capitalize(AutoCapitalize::None),
        FieldKind::Password => control.masked(true).return_key_type(ReturnKeyType::Go),
    };

    view()
        .gap_2()
        .child(
            text(label)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || theme().foreground),
        )
        .child(control)
}

/// A compact search box with its icon, bound to `value`.
pub fn search_input(value: Signal<String>, placeholder: &'static str) -> View {
    view()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(32.0))
        .px_3()
        .w_full()
        .max_w(px(260.0))
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().input)
        .bg(move || theme().background)
        .child(
            super::icon::icon(super::icon::Icon::Search, 14.0)
                .text_color(move || theme().muted_foreground),
        )
        .child(
            input()
                .value(move || value.get())
                .on_change(move |text: &str| value.set(text.to_owned()))
                .placeholder(placeholder)
                .placeholder_color(|| color(theme().muted_foreground))
                .caret_color(|| color(theme().primary))
                .label("Search")
                .flex_1()
                .h_full()
                .text_sm()
                .bg(move || theme().background),
        )
}
