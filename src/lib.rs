//! NanoRP — a lightweight, self-hosted AI roleplay chat app
//! (Leptos + Axum + SQLite).

// Deeply-nested `view!` component trees (especially the chat page under async
// hydration) exceed rustc's default type-recursion limit on wasm; raise it.
#![recursion_limit = "512"]

pub mod app;
pub mod markdown;
pub mod models;
pub mod theme;

// Server-only modules
#[cfg(feature = "ssr")]
pub mod config;
#[cfg(feature = "ssr")]
pub mod crypto;
#[cfg(feature = "ssr")]
pub mod db;
#[cfg(feature = "ssr")]
pub mod providers;
#[cfg(feature = "ssr")]
pub mod services;

// Server functions (shared signatures, server-only implementation)
pub mod server;

// UI components (shared between SSR and hydration)
pub mod components;
pub mod pages;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    use crate::app::*;
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(App);
}
