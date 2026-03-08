// Gemini mapper 模块
// 负责 v1internal 包装/解包

pub mod collector;
pub mod models;
pub mod wrapper; // [NEW]

// No public exports needed here if unused
pub use wrapper::*;
