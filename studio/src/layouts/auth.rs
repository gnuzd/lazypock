//! The signed-out shell: brand panel on the left, the form on the right —
//! two equal columns, and only the form (with the brand above it) on narrow
//! windows, where the panel would leave no room for it.

use std::time::Duration;

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::layouts::main::compact;
use crate::nav;

/// How long each character of [`SCRIPT`] takes, and how long the finished
/// script stays up before it types itself again.
const TYPE_TICK: Duration = Duration::from_millis(22);
const HOLD_TICKS: usize = 180;

/// What a run of the script is, so it can be coloured like in an editor.
#[derive(Clone, Copy)]
enum Kind {
    Plain,
    Prompt,
    Keyword,
    String,
    Comment,
    Number,
    Type,
    Method,
}

/// A run of the script in one colour.
#[derive(Clone, Copy)]
struct Piece {
    text: &'static str,
    kind: Kind,
}

const fn plain(text: &'static str) -> Piece {
    Piece { text, kind: Kind::Plain }
}

const fn prompt(text: &'static str) -> Piece {
    Piece { text, kind: Kind::Prompt }
}

const fn keyword(text: &'static str) -> Piece {
    Piece { text, kind: Kind::Keyword }
}

const fn string(text: &'static str) -> Piece {
    Piece { text, kind: Kind::String }
}

const fn comment(text: &'static str) -> Piece {
    Piece { text, kind: Kind::Comment }
}

const fn number(text: &'static str) -> Piece {
    Piece { text, kind: Kind::Number }
}

const fn ty(text: &'static str) -> Piece {
    Piece { text, kind: Kind::Type }
}

const fn method(text: &'static str) -> Piece {
    Piece { text, kind: Kind::Method }
}

/// What the panel types out: the TypeScript SDK's install and first calls
/// (<https://lazypock.gnuzd.dev/sdk/typescript>), one line per row.
const SCRIPT: &[&[Piece]] = &[
    // Spaces lead each piece: text nodes lose their trailing whitespace.
    &[prompt("$"), plain(" npm install lazypock")],
    &[],
    &[
        keyword("import"),
        plain(" {"),
        ty(" LazypockClient"),
        plain(" }"),
        keyword(" from"),
        string(" 'lazypock'"),
        plain(";"),
    ],
    &[],
    &[
        keyword("const"),
        plain(" client ="),
        keyword(" new"),
        ty(" LazypockClient"),
        plain("({"),
    ],
    &[
        method("  baseUrl"),
        plain(":"),
        string(" 'http://localhost:4000/api'"),
        plain(","),
    ],
    &[plain("});")],
    &[],
    &[comment("// sign in as a superuser")],
    &[
        keyword("await"),
        plain(" client."),
        method("login"),
        plain("("),
        string("'admin@example.com'"),
        plain(","),
        string(" 'password'"),
        plain(");"),
    ],
    &[],
    &[comment("// list the newest posts")],
    &[
        keyword("const"),
        plain(" posts ="),
        keyword(" await"),
        plain(" client"),
    ],
    &[
        plain("  ."),
        method("collection"),
        plain("("),
        string("'posts'"),
        plain(")"),
    ],
    &[
        plain("  ."),
        method("getList"),
        plain("("),
        number("1"),
        plain(","),
        number(" 30"),
        plain(");"),
    ],
    &[],
    &[comment("// create one")],
    &[
        keyword("await"),
        plain(" client."),
        method("collection"),
        plain("("),
        string("'posts'"),
        plain(")."),
        method("create"),
        plain("({"),
    ],
    &[method("  title"), plain(":"), string(" 'Hello'"), plain(",")],
    &[
        method("  published"),
        plain(":"),
        keyword(" true"),
        plain(","),
    ],
    &[plain("});")],
];

pub fn layout(form: impl IntoElement) -> View {
    view()
        .flex_1()
        .flex_row()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(when(|| !compact().get(), brand_panel))
        .child(
            view()
                .flex_1()
                .items_center()
                .justify_center()
                .p_6()
                .child(
                    view()
                        .w_full()
                        .max_w(px(360.0))
                        .child(when(|| compact().get(), narrow_brand))
                        .child(form),
                ),
        )
}

/// Just the mark, above the form, on windows too narrow for the panel.
fn narrow_brand() -> View {
    view()
        .flex_row()
        .items_center()
        .gap_3()
        .pb_6()
        .child(mark(36.0))
        .child(
            view()
                .flex_col()
                .child(text("Studio").text_base().font_weight(FontWeight::SEMIBOLD))
                .child(
                    text("lazypock")
                        .text_xs()
                        .text_color(move || theme().muted_foreground),
                ),
        )
}

fn mark(size: f32) -> View {
    view()
        .size(px(size))
        .items_center()
        .justify_center()
        .rounded_lg()
        .bg(move || theme().primary)
        .child(
            text("S")
                .text_base()
                .font_weight(FontWeight::BOLD)
                .text_color(move || theme().primary_foreground),
        )
}

fn brand_panel() -> View {
    let version = nav::version;

    view()
        .flex_1()
        .h_full()
        .flex_col()
        .justify_between()
        .gap_8()
        .p_8()
        .bg(move || theme().muted)
        .border_r_1()
        .border_r_color(move || theme().border)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_3()
                .child(mark(36.0))
                .child(
                    view()
                        .flex_col()
                        .child(text("Studio").text_base().font_weight(FontWeight::SEMIBOLD))
                        .child(
                            text("lazypock")
                                .text_xs()
                                .text_color(move || theme().muted_foreground),
                        ),
                ),
        )
        .child(
            view()
                .w_full()
                .flex_col()
                .gap_5()
                .child(
                    view()
                        .w_full()
                        .flex_col()
                        .gap_3()
                        .child(
                            text("Everything your backend does, in one studio.")
                                .text_2xl()
                                .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            text("Collections, records, logs and settings — and the same API from your own code.")
                                .text_sm()
                                .text_color(move || theme().muted_foreground),
                        ),
                )
                .child(terminal()),
        )
        .child(
            text(move || format!("lazypock studio · {}", version().get()))
                .text_xs()
                .text_color(move || theme().muted_foreground),
        )
}

/// The SDK sample, typed out and coloured. The written-out script is shown
/// at once when the system asks for less motion.
fn terminal() -> View {
    let total: usize = SCRIPT
        .iter()
        .flat_map(|line| line.iter())
        .map(|piece| piece.text.chars().count())
        .sum();
    let typed = signal(total);
    if !reduce_motion() {
        typed.set(0);
        set_interval(TYPE_TICK, move || {
            typed.update(|n| *n = if *n > total + HOLD_TICKS { 0 } else { *n + 1 });
        });
    }

    let mut body = view().w_full().p_4().flex_col();
    let mut start = 0;
    for line in SCRIPT {
        let line_start = start;
        let line_end = line_start + line.iter().map(|piece| piece.text.chars().count()).sum::<usize>();
        let mut row = view().w_full().flex_row().items_center().h(px(18.0));
        let mut from = line_start;
        for piece in line.iter() {
            let len = piece.text.chars().count();
            let piece_start = from;
            from += len;
            row = row.child(
                mono(
                    move || {
                        let take = typed.get().saturating_sub(piece_start).min(len);
                        piece.text.chars().take(take).collect::<String>()
                    },
                    piece.kind,
                )
                .number_of_lines(1),
            );
        }
        row = row.child(
            mono(
                move || {
                    let at = typed.get();
                    let here = if line_start == line_end {
                        at == line_start
                    } else {
                        at >= line_start && at < line_end
                    };
                    if here && at < total { "▌" } else { "" }.to_string()
                },
                Kind::Plain,
            )
            .number_of_lines(1),
        );
        body = body.child(row);
        start = line_end;
    }

    view()
        .w_full()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(move || theme().border)
        .bg(move || theme().card)
        .shadow_sm()
        .overflow_hidden()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap_2()
                .px_4()
                .py_3()
                .border_b_1()
                .border_b_color(move || theme().border)
                .child(dot())
                .child(dot())
                .child(dot())
                .child(
                    text("lazypock · typescript sdk")
                        .text_xs()
                        .text_color(move || theme().muted_foreground),
                ),
        )
        // Every row has a fixed height, so the panel never grows while typing.
        .child(body)
}

/// One run of the script in the terminal's font, in its colour.
fn mono(value: impl IntoReactive<SharedString> + 'static, kind: Kind) -> Text {
    text(value)
        .text_xs()
        .font_family(move || {
            theme().font_mono.clone().unwrap_or_else(|| {
                // The platform takes one family, the web a CSS list.
                if cfg!(target_arch = "wasm32") {
                    "ui-monospace, SFMono-Regular, Menlo, monospace".into()
                } else {
                    "Menlo".into()
                }
            })
        })
        .text_color(kind_color(kind))
}

fn kind_color(kind: Kind) -> AdaptiveColor {
    let t = theme();
    match kind {
        Kind::Plain => t.card_foreground,
        Kind::Prompt | Kind::Comment => t.muted_foreground,
        Kind::Keyword => t.primary,
        Kind::Type | Kind::Method => t.color("info").unwrap_or(t.chart_2),
        Kind::String => t.color("success").unwrap_or(t.chart_2),
        Kind::Number => t.color("warning").unwrap_or(t.chart_3),
    }
}

fn dot() -> View {
    view().size(px(9.0)).rounded_full().bg(move || theme().border)
}
