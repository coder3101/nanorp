//! Leptos server functions — the RPC boundary between the WASM client and
//! the server. The `Db` handle comes from Leptos context (provided per
//! request in main.rs).

pub mod character;
pub mod chat;
pub mod provider;
pub mod settings;
