// pub mod collections;
pub mod login;
// pub mod logs;
// pub mod overview;
// pub mod settings;
pub mod setup;

use nsh_core::prelude::*;

use crate::nav::Route;

// The page a route shows in the dashboard's content area.
// pub fn page(route: Route) -> impl IntoElement {
//     match route {
//         Route::Collections => collections::page().into_element(),
//         Route::Logs => logs::page().into_element(),
//         Route::Settings => settings::page().into_element(),
//         _ => overview::page().into_element(),
//     }
// }
