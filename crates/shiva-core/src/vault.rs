//! Jailed filesystem navigator for the sample vault.
//!
//! Powers the terminal-style file browser in the UI (`cd` / `ls` / `cat`). It
//! is deliberately **not** a remote command shell: no OS process is ever
//! spawned. Every path is resolved against the vault root and verified to stay
//! inside it, so `../` traversal cannot escape the downloads directory.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("path escapes the vault")]
    Escape,
    #[error("not found")]
    NotFound,
    #[error("not a directory")]
    NotDir,
    #[error("not a file")]
    NotFile,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Listing {
    /// Vault-relative path, e.g. `/` or `/subdir`.
    pub cwd: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn root_canon(&self) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::canonicalize(&self.root)
    }

    /// Resolve a vault-relative path to an absolute path guaranteed to live
    /// inside the root. `..` is folded away and can never climb above root.
    fn resolve(&self, rel: &str) -> Result<PathBuf, VaultError> {
        let root = self.root_canon()?;
        let mut p = root.clone();
        for comp in Path::new(rel).components() {
            match comp {
                Component::Normal(s) => p.push(s),
                Component::ParentDir => {
                    p.pop();
                    if !p.starts_with(&root) {
                        p = root.clone();
                    }
                }
                Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            }
        }
        let canon = std::fs::canonicalize(&p).unwrap_or(p);
        if !canon.starts_with(&root) {
            return Err(VaultError::Escape);
        }
        Ok(canon)
    }

    fn rel_display(&self, abs: &Path) -> String {
        let root = self.root_canon().unwrap_or_else(|_| self.root.clone());
        let r = abs.strip_prefix(&root).unwrap_or(abs);
        let s = r.to_string_lossy().replace('\\', "/");
        if s.is_empty() {
            "/".into()
        } else {
            format!("/{}", s)
        }
    }

    pub fn list(&self, rel: &str) -> Result<Listing, VaultError> {
        let abs = self.resolve(rel)?;
        let meta = std::fs::metadata(&abs).map_err(|_| VaultError::NotFound)?;
        if !meta.is_dir() {
            return Err(VaultError::NotDir);
        }
        let mut entries = Vec::new();
        for e in std::fs::read_dir(&abs)? {
            let e = e?;
            let m = e.metadata()?;
            entries.push(Entry {
                name: e.file_name().to_string_lossy().to_string(),
                is_dir: m.is_dir(),
                size: m.len(),
            });
        }
        // Directories first, then alphabetical.
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        Ok(Listing {
            cwd: self.rel_display(&abs),
            entries,
        })
    }

    /// Read up to `max` bytes of a file and render as text, escaping
    /// non-printable bytes so binaries do not corrupt the terminal view.
    pub fn head(&self, rel: &str, max: usize) -> Result<String, VaultError> {
        let abs = self.resolve(rel)?;
        let meta = std::fs::metadata(&abs).map_err(|_| VaultError::NotFound)?;
        if !meta.is_file() {
            return Err(VaultError::NotFile);
        }
        let bytes = std::fs::read(&abs)?;
        let slice = &bytes[..bytes.len().min(max)];
        let mut out = String::with_capacity(slice.len());
        for &b in slice {
            match b {
                b'\n' | b'\t' => out.push(b as char),
                0x20..=0x7e => out.push(b as char),
                _ => out.push_str(&format!("\\x{:02x}", b)),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_cannot_escape_root() {
        let tmp = std::env::temp_dir().join(format!("shiva-vault-test-{}", std::process::id()));
        let v = Vault::new(tmp.clone());
        // A pile of `..` must collapse back to root, never above it.
        let resolved = v.resolve("../../../../etc").unwrap();
        let root = std::fs::canonicalize(&tmp).unwrap();
        assert!(resolved.starts_with(&root));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
