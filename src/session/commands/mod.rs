/// Slash-command handlers split into focused submodules.
/// All `impl Session` blocks work across files since Session fields are `pub`.
mod info;
mod session_mgmt;
mod provider;
mod memory;
mod media;
mod code;
mod index;
mod tasks;
mod skills;
mod git;

// Re-export public standalone functions so callers keep the same paths.
pub use media::paste_clipboard_image;
pub use skills::{skill_list_text, skill_show_text};
pub use code::detect_project_type;
