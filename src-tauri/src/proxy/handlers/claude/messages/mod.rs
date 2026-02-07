//! Claude Messages Handler module.
//!
//! POST /v1/messages - Main message processing endpoint.
//!
//! # Module Structure
//!
//! - `handler` - Main request handler
//! - `compression` - 3-layer progressive compression
//! - `retry` - Error handling and retry logic
//! - `response` - Response building helpers

mod compression;
mod handler;
mod response;
mod retry;

pub use handler::handle_messages;
