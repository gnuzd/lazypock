//! Where the app is: the route, the header search, and what the sub
//! navigation has picked.

use nsh_core::prelude::*;

use crate::api::{self, Collection, Records};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Login,
    Setup,
    Overview,
    Collections,
    Logs,
    Settings,
}

impl Route {
    pub fn title(self) -> &'static str {
        match self {
            Route::Login => "Sign in",
            Route::Setup => "Setup",
            Route::Overview => "Overview",
            Route::Collections => "Collections",
            Route::Logs => "Logs",
            Route::Settings => "Settings",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Route::Login => "",
            Route::Setup => "Create the first superuser",
            Route::Overview => "Instance health and recent activity",
            Route::Collections => "Records in this collection",
            Route::Logs => "Requests served by this instance",
            Route::Settings => "Instance configuration",
        }
    }

    /// Whether the section has a sub navigation beside the content.
    pub fn has_sub_nav(self) -> bool {
        matches!(self, Route::Collections | Route::Settings)
    }
}

/// The settings sub navigation's sections.
pub const SETTINGS_SECTIONS: [&str; 2] = ["General", "About"];

/// Per app instance (not process-wide), so a closed app leaves nothing behind.
struct NavState {
    version: Signal<String>,
    route: Signal<Route>,
    search: Signal<String>,
    collections: Signal<Vec<Collection>>,
    selected: Signal<String>,
    records: Signal<Records>,
    creating: Signal<bool>,
    section: Signal<usize>,
}

impl Default for NavState {
    fn default() -> Self {
        Self {
            version: signal(String::new()),
            route: signal(if crate::api::signed_in() {
                Route::Overview
            } else {
                Route::Login
            }),
            search: signal(String::new()),
            collections: signal(Vec::new()),
            selected: signal(String::new()),
            records: signal(Records::default()),
            creating: signal(false),
            section: signal(0),
        }
    }
}

pub fn current_route() -> Signal<Route> {
    global::<NavState>().with(|nav| nav.route)
}

pub fn navigate(route: Route) {
    current_route().set(route);
    // A form opened for another section starts over.
    if route != Route::Collections {
        creating_collection().set(false);
    }
}

/// The header's search box: pages filter their lists by it.
pub fn search_query() -> Signal<String> {
    global::<NavState>().with(|nav| nav.search)
}

pub fn version() -> Signal<String> {
    global::<NavState>().with(|nav| nav.version)
}

/// Every collection of the instance.
pub fn collections() -> Signal<Vec<Collection>> {
    global::<NavState>().with(|nav| nav.collections)
}

/// The records of the selected collection.
pub fn records() -> Signal<Records> {
    global::<NavState>().with(|nav| nav.records)
}

/// The id of the collection whose records the content shows; empty for none.
pub fn selected_collection() -> Signal<String> {
    global::<NavState>().with(|nav| nav.selected)
}

pub fn select_collection(id: String) {
    if selected_collection().with(|current| *current == id) {
        return;
    }
    selected_collection().set(id);
    // The header search filters records, so it starts over.
    search_query().set(String::new());
    load_records();
}

/// Whether the New collection form is showing.
pub fn creating_collection() -> Signal<bool> {
    global::<NavState>().with(|nav| nav.creating)
}

pub fn start_creating() {
    creating_collection().set(true);
}

pub fn cancel_creating() {
    creating_collection().set(false);
}

pub fn healthcheck() {
    spawn(async move {
        if let Some(health) = api::healthcheck().await {
            if let Some(v) = health["data"]["version"].as_str() {
                version().set(v.to_string());
            }
        }
    });
}

/// Fetches every collection and the selected one's records; the dashboard calls it
/// once when it opens, and a failure is reported by the toast the call raises.
pub fn load_collections() {
    spawn(async move {
        let Some(list) = api::fetch_collections().await else {
            return;
        };
        collections().set(list);
        // Keep the pick while it still exists, else take the first collection.
        let id = selected_collection().get();
        let kept = !id.is_empty() && collections().with(|list| list.iter().any(|c| c.id == id));
        if !kept {
            let first =
                collections().with(|list| list.first().map(|c| c.id.clone()).unwrap_or_default());
            selected_collection().set(first);
        }
        load_records();
    });
}

/// Fetches the selected collection's records; a reply for a pick that changed
/// meanwhile is dropped.
pub fn load_records() {
    let id = selected_collection().get();
    let name = collections().with(|list| list.iter().find(|c| c.id == id).map(|c| c.name.clone()));
    let Some(name) = name else {
        records().set(Records::default());
        return;
    };
    spawn(async move {
        let Some(page) = api::fetch_records(&name).await else {
            return;
        };
        if collections().with(|list| list.iter().any(|c| c.id == id && c.name == name)) {
            records().set(page);
        }
    });
}

/// Shows the collection the backend has just created, and picks it.
pub fn created_collection(collection: Collection) {
    let id = collection.id.clone();
    collections().update(|list| {
        list.retain(|c| c.id != collection.id);
        list.push(collection);
        list.sort_by(|a, b| a.name.cmp(&b.name));
    });
    creating_collection().set(false);
    selected_collection().set(id);
    search_query().set(String::new());
    load_records();
}

/// Which settings section the content shows.
pub fn settings_section() -> Signal<usize> {
    global::<NavState>().with(|nav| nav.section)
}

pub fn select_settings_section(index: usize) {
    settings_section().set(index);
}
