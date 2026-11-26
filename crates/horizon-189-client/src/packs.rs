//! The resource packs folder and the selected stack (`ResourcePackRepository`).
use std::path::PathBuf;

/// The packs folder, beside the screenshots.
pub fn directory() -> Option<PathBuf> {
    crate::updater::support_dir().map(|dir| dir.join("resourcepacks"))
}

/// A pack name the settings may keep: one entry in the packs folder.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.contains(['/', '\\', '\0'])
        && name != "."
        && name != ".."
}

/// The selected packs' paths, highest priority first; missing ones drop out.
pub fn selected(names: &[String]) -> Vec<PathBuf> {
    let Some(folder) = directory() else {
        return Vec::new();
    };
    names
        .iter()
        .filter(|name| safe_name(name))
        .map(|name| folder.join(name))
        .filter(|path| path.exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_names_stay_inside_the_folder() {
        assert!(safe_name("Faithful 32x.zip") && safe_name("My Pack"));
        assert!(!safe_name("../x") && !safe_name("a/b") && !safe_name("..") && !safe_name(""));
    }
}
