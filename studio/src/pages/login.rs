//! Login: sign in to the instance with a superuser account.

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::api;
use crate::components::{FieldKind, Variant, action_button, field, text_link};
use crate::nav::{self, Route};

pub fn page() -> View {
    let email = signal(String::new());
    let password = signal(String::new());
    let pending = signal(false);
    let can_submit = computed(move || {
        !pending.get()
            && email.with(|email| !email.trim().is_empty())
            && password.with(|password| !password.is_empty())
    });

    let submit = move || {
        if pending.get() {
            return;
        }
        pending.set(true);
        let identity = email.get();
        let secret = password.get();
        spawn(async move {
            // Failures are reported by the toast the call raises.
            if api::sign_in(&identity, &secret).await {
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
                    text("Welcome back")
                        .text_2xl()
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(
                    text("Sign in to manage your lazypock instance.")
                        .text_sm()
                        .text_color(move || theme().muted_foreground),
                ),
        )
        .child(
            view()
                .flex_col()
                .gap_4()
                .child(field("Email", "admin@example.com", email, FieldKind::Email))
                .child(field("Password", "Password", password, FieldKind::Password))
                .child(action_button(
                    "Sign in",
                    Variant::Primary,
                    move || !can_submit.get(),
                    submit,
                )),
        )
        .child(
            view()
                .items_center()
                .child(text_link("First time here? Create a superuser", || {
                    nav::navigate(Route::Setup)
                })),
        )
}
