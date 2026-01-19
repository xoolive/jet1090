use std::path::PathBuf;

/// Expand `~` in a path to the user's home directory
pub fn expanduser(path: PathBuf) -> PathBuf {
    // Check if the path starts with "~"
    if let Some(path_str) = path.to_str() {
        if let Some(stripped) = path_str.strip_prefix("~") {
            if let Some(home_dir) = dirs::home_dir() {
                // Join the home directory with the rest of the path
                return home_dir.join(stripped.trim_start_matches('/'));
            }
        }
    }
    path
}
