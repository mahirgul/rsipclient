//! Plugin System Subsystem
//!
//! Provides extensible plugin capabilities supporting:
//! - **Rhai Script Engine** (`.rhai` files)
//! - **Lua 5.4 Script Engine** (`.lua` files)
//! - **Asynchronous HTTP Webhooks**
//!
//! ### Module Organization:
//! - [`types`]: Data models, event payloads, and plugin action results
//! - [`manager`]: Plugin state coordinator and file I/O
//! - [`rhai_runner`]: Rhai scripting engine runner
//! - [`lua_runner`]: Lua scripting engine runner
//! - [`webhook_runner`]: Async HTTP Webhook POST sender

pub mod lua_runner;
pub mod manager;
pub mod rhai_runner;
pub mod types;
pub mod webhook_runner;

pub use manager::PluginManager;
#[allow(unused_imports)]
pub use types::{PluginActionResult, PluginEvent, PluginSystemConfig, WebhookPluginConfig};
