// OpenAI API Handlers Module
// Handles OpenAI-compatible API endpoints

mod chat;
mod completions;
mod images;
mod models;

// Re-export all public handlers
pub use chat::handle_chat_completions;
pub use completions::handle_completions;
pub use images::{handle_images_edits, handle_images_generations};
pub use models::handle_list_models;
