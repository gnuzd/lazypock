//! Toasts: brief feedback in the corner for results that are not on screen.
//!
//! Anywhere in the app calls [`success`] or [`failure`]; the stack itself is
//! mounted once, by [`toaster`] in the root component.

use std::time::Duration;

use nsh_base::{
    theme, toast as toast_root, toast_stack, Anchor, ToastManager, ToastMotion, ToastOptions,
    ToastStackState, ToastTransitionStatus,
};
use nsh_core::prelude::*;
use nsh_core::time::now;

use crate::components::badge::{tone_color, Tone};

/// How long a success stays up; failures leave more time to be read.
const SUCCESS: Duration = Duration::from_secs(4);
const FAILURE: Duration = Duration::from_secs(8);
/// How often the stack's lifecycle is advanced.
const TICK: Duration = Duration::from_millis(50);

#[derive(Clone, PartialEq)]
struct Notice {
    title: SharedString,
    description: Option<SharedString>,
    tone: Tone,
}

type Shown = Vec<(u64, Notice, ToastTransitionStatus)>;

/// The surface's toasts: the lifecycle, and what is on screen.
#[derive(Clone)]
struct Toaster {
    manager: ToastManager<u64, Notice>,
    shown: Signal<Shown>,
    next: u64,
}

impl Default for Toaster {
    fn default() -> Self {
        Self {
            manager: ToastManager::new(ToastMotion::sonner()),
            shown: signal(Vec::new()),
            next: 1,
        }
    }
}

impl Toaster {
    fn sync(&mut self) {
        let shown = self
            .manager
            .visible(3)
            .map(|(id, notice, status)| (*id, notice.clone(), status))
            .collect();
        self.shown.set(shown);
    }
}

/// Says something worked.
pub fn success(title: impl Into<SharedString>) {
    push(title.into(), None, Tone::Success, SUCCESS);
}

/// Says something failed: what happened, and what to do about it.
pub fn failure(title: impl Into<SharedString>, description: impl Into<SharedString>) {
    push(
        title.into(),
        Some(description.into()),
        Tone::Danger,
        FAILURE,
    );
}

fn push(title: SharedString, description: Option<SharedString>, tone: Tone, timeout: Duration) {
    let toaster = global::<Toaster>();
    toaster.update(|t| {
        let id = t.next;
        t.next += 1;
        let notice = Notice {
            title,
            description,
            tone,
        };
        t.manager
            .push(id, notice, ToastOptions::timeout(Some(timeout)), now());
        t.sync();
    });
}

fn dismiss(id: u64) {
    let toaster = global::<Toaster>();
    toaster.update(|t| {
        t.manager.dismiss(&id, now());
        t.sync();
    });
}

/// The stack, bottom right, above the app. Put it in the root component.
pub fn toaster() -> impl IntoElement {
    let state = ToastStackState::new();
    let toaster = global::<Toaster>();
    // A toast stays mounted from entry to exit, so its clock has to keep
    // running while one is on screen (paused while the stack is expanded).
    set_interval(TICK, move || {
        let mut changed = false;
        toaster.update(|t| changed = t.manager.advance(now(), state.is_expanded()).changed);
        if changed {
            toaster.update(Toaster::sync);
        }
    });
    let shown = toaster.with(|t| t.shown);

    overlay()
        .absolute()
        .inset_0()
        .pointer_events(PointerEvents::BoxNone)
        .justify_end()
        .items_end()
        .p_5()
        .child(
            toast_stack()
                .placement(Anchor::BottomRight)
                .state(state)
                .w(px(360.0))
                .child(each(
                    shown,
                    |(id, _, _): &(u64, Notice, ToastTransitionStatus)| *id,
                    move |item: ReadSignal<(u64, Notice, ToastTransitionStatus)>, _| {
                        let (id, notice, _) = item.get_untracked();
                        let accent = tone_color(notice.tone);
                        let mut card = toast_root()
                            .transition_status(move || item.get().2)
                            .accessibility_label({
                                let notice = notice.clone();
                                move || {
                                    Some(match &notice.description {
                                        Some(detail) => {
                                            format!("{}. {}", notice.title, detail).into()
                                        }
                                        None => notice.title.clone(),
                                    })
                                }
                            })
                            .on_dismiss_request(move || dismiss(id))
                            .flex_col()
                            .gap_1()
                            .rounded_xl()
                            .border_1()
                            .border_color(move || theme().border)
                            .px_4()
                            .py_3()
                            .bg(move || theme().card)
                            .shadow_lg()
                            .transition_all()
                            .duration(200.0)
                            .starting_class("opacity-0 translate-y-4")
                            .ending_class("opacity-0 scale-95")
                            .child(
                                text(notice.title.clone())
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(accent),
                            );
                        if let Some(detail) = notice.description.clone() {
                            card = card.child(
                                text(detail)
                                    .text_xs()
                                    .text_color(move || theme().muted_foreground),
                            );
                        }
                        card
                    },
                )),
        )
}
