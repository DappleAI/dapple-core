pub mod embed;
pub mod madlib;
pub mod summarize;
pub mod synthesize;

pub use embed::handle_embed;
pub use madlib::handle_madlib;
pub use summarize::handle_summarize;
pub use synthesize::handle_synthesize;
