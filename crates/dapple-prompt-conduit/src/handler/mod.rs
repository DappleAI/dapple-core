pub mod embed;
pub mod image;
pub mod madlib;
pub mod prompt;
pub mod render_document;
pub mod search;
pub mod summarize;
pub mod tts;
pub mod video;

pub use embed::handle_embed;
pub use image::{handle_image_compose, handle_image_composite};
pub use madlib::handle_madlib;
pub use prompt::handle_prompt;
pub use render_document::handle_render_document;
pub use search::handle_search;
pub use summarize::handle_summarize;
