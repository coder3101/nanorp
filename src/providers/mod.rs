//! LLM provider backends (server-only).

#[cfg(feature = "ssr")]
mod buffering;
#[cfg(feature = "ssr")]
mod http;
#[cfg(feature = "ssr")]
pub mod ollama;
#[cfg(feature = "ssr")]
pub mod openai;
#[cfg(feature = "ssr")]
pub mod registry;
#[cfg(feature = "ssr")]
pub mod traits;
