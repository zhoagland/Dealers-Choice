//! Named settings profiles as TOML files (PRD FR-UI-3):
//!
//! ```text
//! <config_root>/            default ./config, or $DC_CONFIG_ROOT, or --config-root
//! ├── sims/<name>.toml      a sim RunSpec
//! └── play/<name>.toml      a play profile
//! ```
//!
//! Files, not database rows: they're readable, diffable, can be committed, and can be edited by
//! hand or with `dc config`. This module only stores and lists; the caller decides the type.

use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

/// The kinds of profile, each in its own directory.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    Sim,
    Play,
}

impl ProfileKind {
    pub fn dir_name(self) -> &'static str {
        match self {
            ProfileKind::Sim => "sims",
            ProfileKind::Play => "play",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProfileStore {
    root: PathBuf,
}

impl ProfileStore {
    /// `explicit` (a CLI flag) wins, then `$DC_CONFIG_ROOT`, then `./config`.
    pub fn resolve(explicit: Option<&Path>) -> ProfileStore {
        let root = explicit
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("DC_CONFIG_ROOT").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("config"));
        ProfileStore { root }
    }

    pub fn path(&self, kind: ProfileKind, name: &str) -> PathBuf {
        self.root.join(kind.dir_name()).join(format!("{name}.toml"))
    }

    /// Profile names of one kind, sorted.
    pub fn list(&self, kind: ProfileKind) -> Vec<String> {
        let dir = self.root.join(kind.dir_name());
        let mut names = Vec::new();
        // A missing directory just means no profiles yet.
        let Ok(entries) = std::fs::read_dir(dir) else {
            return names;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|x| x == "toml")
                && let Some(stem) = path.file_stem()
            {
                names.push(stem.to_string_lossy().into_owned());
            }
        }
        names.sort();
        names
    }

    /// Profile names may only use letters, digits, `-` and `_`, so they're safe as file names.
    pub fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }

    pub fn load_table(&self, kind: ProfileKind, name: &str) -> Result<toml::Table, String> {
        let path = self.path(kind, name);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        text.parse::<toml::Table>()
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load<T: DeserializeOwned>(&self, kind: ProfileKind, name: &str) -> Result<T, String> {
        let table = self.load_table(kind, name)?;
        toml::Value::Table(table)
            .try_into()
            .map_err(|e| format!("profile {name}: {e}"))
    }

    pub fn save<T: Serialize>(
        &self,
        kind: ProfileKind,
        name: &str,
        value: &T,
    ) -> Result<PathBuf, String> {
        if !ProfileStore::valid_name(name) {
            return Err(format!(
                "invalid profile name {name:?}: use letters, digits, - and _"
            ));
        }
        let path = self.path(kind, name);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let text = toml::to_string_pretty(value).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    pub fn delete(&self, kind: ProfileKind, name: &str) -> Result<(), String> {
        let path = self.path(kind, name);
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct P {
        a: u32,
        b: Vec<String>,
    }

    #[test]
    fn save_list_load_delete() {
        let dir = std::env::temp_dir().join(format!("dc-profiles-{}", std::process::id()));
        let store = ProfileStore::resolve(Some(&dir));
        let p = P {
            a: 3,
            b: vec!["x".into()],
        };
        store.save(ProfileKind::Sim, "nightly", &p).unwrap();
        store.save(ProfileKind::Sim, "a-first", &p).unwrap();
        assert_eq!(store.list(ProfileKind::Sim), vec!["a-first", "nightly"]);
        assert!(store.list(ProfileKind::Play).is_empty());
        assert_eq!(store.load::<P>(ProfileKind::Sim, "nightly").unwrap(), p);
        assert!(
            store.save(ProfileKind::Sim, "../escape", &p).is_err(),
            "names can't be paths"
        );
        store.delete(ProfileKind::Sim, "nightly").unwrap();
        assert_eq!(store.list(ProfileKind::Sim), vec!["a-first"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
