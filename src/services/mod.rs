//! Business-logic services between the server functions and the database.

#[cfg(feature = "ssr")]
pub mod character_service;
#[cfg(feature = "ssr")]
pub mod chat_service;
#[cfg(feature = "ssr")]
pub mod generation;
#[cfg(feature = "ssr")]
pub mod provider_service;
#[cfg(feature = "ssr")]
pub mod settings_service;
