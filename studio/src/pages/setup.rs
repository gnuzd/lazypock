//! Setup: the first superuser, created before the studio is reachable.

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::api;
use crate::components::{FieldKind, Variant, action_button, field, text_link, toast};
use crate::nav::{self, Route};

pub fn page() -> View {
    let email = signal(String::new());
    let password = signal(String::new());
    let confirm = signal(String::new());
    let checking = signal(true);
    let complete = signal(false);
    let pending = signal(false);
    let mismatch = computed(move || {
        confirm.with(|confirm| !confirm.is_empty()) && confirm.get() != password.get()
    });
    let can_submit = computed(move || {
        !checking.get()
            && !pending.get()
            && email.with(|email| !email.trim().is_empty())
            && !password.get().is_empty()
            && !confirm.get().is_empty()
    });

    // One check per visit: the endpoint closes itself once a superuser exists.
    spawn(async move {
        if !api::setup_required().await {
            complete.set(true);
        }
        checking.set(false);
    });

    let submit = move || {
        if pending.get() {
            return;
        }
        if mismatch.get() {
            toast::failure(
                "Passwords don't match",
                "Type the same password in both fields.",
            );
            return;
        }
        pending.set(true);
        let address = email.get();
        let secret = password.get();
        spawn(async move {
            // Failures are reported by the toast the call raises.
            if api::create_superuser(&address, &secret).await {
                nav::navigate(Route::Overview);
            } else {
                pending.set(false);
            }
        });
    };

    view()
        .flex_col()
        .gap_6()
        .child(
            view()
                .flex_col()
                .gap_2()
                .child(
                    text("Create a superuser")
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    text("The first account owns this instance. You can add more accounts later.")
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                ),
        )
        .child(when(
            move || complete.get(),
            || {
                text("Setup is already complete. Sign in with the existing superuser.")
                    .text_sm()
                    .text_color(move || theme().muted_foreground)
            },
        ))
        .child(when(
            move || !complete.get(),
            move || {
                view()
                    .flex_col()
                    .gap_4()
                    .child(field("Email", "admin@example.com", email, FieldKind::Email))
                    .child(field("Password", "Password", password, FieldKind::Password))
                    .child(field(
                        "Confirm password",
                        "Repeat the password",
                        confirm,
                        FieldKind::Password,
                    ))
                    .child(action_button(
                        "Create superuser",
                        Variant::Primary,
                        move || !can_submit.get(),
                        submit,
                    ))
            },
        ))
        .child(
            view()
                .items_center()
                .child(text_link("Back to sign in", || nav::navigate(Route::Login))),
        )
}
