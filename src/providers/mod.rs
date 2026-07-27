//! LLM provider backends (server-only).

#[cfg(feature = "ssr")]
pub mod traits;
#[cfg(feature = "ssr")]
pub mod ollama;
#[cfg(feature = "ssr")]
pub mod openai;
#[cfg(feature = "ssr")]
pub mod registry;
