//! The app's Rust UI: its root component, which `nsh run`, `nsh package`
//! and `cargo run` mount.
pub mod api;
pub mod components;
// pub mod data;
pub mod layouts;
pub mod nav;
pub mod pages;

use nsh_base::theme;
use nsh_core::prelude::*;

use layouts::auth;
use nav::Route;
use pages::{login, setup};

#[native_module("studio/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// The application's root component.
#[component]
pub fn App() -> impl IntoElement {
    let route = nav::current_route();
    nav::healthcheck();

    // Resume a stored session: refresh it if the API no longer takes the token.
    spawn(api::resume());

    view()
        .flex_1()
        .bg(move || theme().background)
        .text_color(move || theme().foreground)
        .child(when(
            move || matches!(route.get(), Route::Login | Route::Setup),
            move || {
                auth::layout(dynamic(move || {
                    if route.get() == Route::Setup {
                        setup::page().into_element()
                    } else {
                        login::page().into_element()
                    }
                }))
            },
        ))
        // Feedback floats above whichever screen is showing.
        .child(components::toast::toaster())
}
