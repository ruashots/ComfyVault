//! Path containment: the engine's security boundary.
//!
//! Every path that arrives from the interface passes through this module before
//! the engine writes, links, or deletes anything. The rule is one sentence: a
//! mutating operation must land inside a boundary the engine already trusts,
//! which is a registered install's model folders or the vault root.
//!
//! Two resolvers, and the difference matters:
//!
//! * [`resolve_within`] resolves the whole path, including a final symbolic
//!   link. Use it to read a file or to name a move target.
//! * [`resolve_new_path_within`] resolves only the parent and keeps the last
//!   component literal. Use it to create or remove a link, where following the
//!   link would act on the wrong file.
//!
//! Both normalize `..` textually first and then canonicalize the part that
//! exists, so neither a `..` sequence nor a symbolic link can escape.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::error::{ErrorCode, Result, VaultError};

/// Renders a path for a person or for storage.
///
/// On Windows, [`std::fs::canonicalize`] returns a verbatim path that starts
/// with `\\?\`. That prefix confuses people and breaks some tools, so it is
/// stripped everywhere a path leaves the engine.
pub fn display_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return rest.to_string();
    }
    s.into_owned()
}

/// Removes the `\\?\` prefix Windows canonicalization adds.
///
/// [`std::fs::canonicalize`] on Windows returns a verbatim path, and that path
/// then becomes an install's root, a scan entry's location, a link's address
/// and the vault's own path. All of those reach the interface, so without this
/// the person is shown `\\?\C:\Users\...` everywhere a path appears.
///
/// It also breaks comparisons: `\\?\C:\x` does not start with `C:\x`, so any
/// check that mixes a canonicalized path with a plain one silently fails.
/// Cleaning at the point of canonicalization keeps one form everywhere.
///
/// Paths longer than 260 characters rely on that prefix, but Rust's standard
/// library puts it back for the calls that need it, so storing the plain form
/// is safe and is what the person recognizes.
pub fn clean(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();
    if !s.starts_with(r"\\?\") {
        // Never round-trips through a String on a system that cannot have the
        // prefix, so a file name that is not valid text is left untouched.
        return path.to_path_buf();
    }
    PathBuf::from(display_path(path))
}

/// Canonicalizes a path and removes the Windows verbatim prefix.
///
/// Every place the engine resolves a real location goes through this, so one
/// form of every path is stored, compared and shown.
pub fn canonicalize_clean(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(|p| clean(&p))
}

/// Resolves `.` and `..` without touching the disk.
///
/// A leading `..` that would climb above the prefix or the root is dropped,
/// because there is nothing above a root to climb to.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    let mut normals: Vec<OsString> = Vec::new();
    let mut has_root = false;

    for comp in path.components() {
        match comp {
            Component::Prefix(p) => {
                out.push(p.as_os_str());
            }
            Component::RootDir => {
                has_root = true;
                normals.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if normals.pop().is_none() && !has_root {
                    // A relative path may legitimately start with `..`.
                    normals.push(OsString::from(".."));
                }
            }
            Component::Normal(c) => normals.push(c.to_os_string()),
        }
    }

    if has_root {
        out.push(std::path::MAIN_SEPARATOR.to_string());
    }
    for n in normals {
        out.push(n);
    }
    out
}

/// Splits a path into the deepest ancestor that exists on disk and the
/// components that do not exist yet.
fn split_at_existing(path: &Path) -> (PathBuf, Vec<OsString>) {
    let mut remainder: Vec<OsString> = Vec::new();
    let mut cursor = path.to_path_buf();

    loop {
        // `symlink_metadata` answers "is there an entry here", including a
        // dangling link, which `exists` would call absent.
        if std::fs::symlink_metadata(&cursor).is_ok() {
            remainder.reverse();
            return (cursor, remainder);
        }
        match cursor.file_name() {
            Some(name) => {
                remainder.push(name.to_os_string());
                let parent = cursor.parent().map(Path::to_path_buf);
                match parent {
                    Some(p) if !p.as_os_str().is_empty() => cursor = p,
                    _ => {
                        remainder.reverse();
                        return (PathBuf::from("."), remainder);
                    }
                }
            }
            None => {
                remainder.reverse();
                return (cursor, remainder);
            }
        }
    }
}

/// Canonicalizes the part of `path` that exists and appends the rest verbatim.
pub fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    let (existing, remainder) = split_at_existing(path);
    let mut real = crate::paths::canonicalize_clean(&existing)
        .map_err(|e| VaultError::from_io(&e, &existing, "checking where the folder really is"))?;
    for part in remainder {
        real.push(part);
    }
    Ok(real)
}

/// The canonical form of `root`, which every containment check compares against.
fn canonical_root(root: &Path) -> Result<PathBuf> {
    canonicalize_clean(root).map_err(|e| VaultError::from_io(&e, root, "opening the folder"))
}

/// Resolves `candidate` and proves it lands inside `root`.
///
/// `candidate` may be absolute, or relative to `root`. A final symbolic link is
/// followed, so the answer is the real file. Use
/// [`resolve_new_path_within`] when the final component is a link you intend to
/// create or delete.
pub fn resolve_within(root: &Path, candidate: &Path) -> Result<PathBuf> {
    let joined = if candidate.is_absolute() { candidate.to_path_buf() } else { root.join(candidate) };
    let normalized = lexical_normalize(&joined);
    let real = canonicalize_existing_prefix(&normalized)?;
    let real_root = canonical_root(root)?;
    if !real.starts_with(&real_root) {
        return Err(VaultError::outside_boundary(&real, &real_root));
    }
    Ok(real)
}

/// Resolves a path whose last component must stay literal.
///
/// The parent is canonicalized, so a symbolic link in the parent chain cannot
/// escape. The last component is kept as written, so creating or removing a
/// link acts on the link and never on what it points at.
pub fn resolve_new_path_within(root: &Path, candidate: &Path) -> Result<PathBuf> {
    let joined = if candidate.is_absolute() { candidate.to_path_buf() } else { root.join(candidate) };
    let normalized = lexical_normalize(&joined);

    let file_name = normalized
        .file_name()
        .ok_or_else(|| VaultError::invalid("That path has no file name."))?
        .to_os_string();
    let parent = normalized
        .parent()
        .ok_or_else(|| VaultError::invalid("That path has no parent folder."))?;

    let real_parent = canonicalize_existing_prefix(parent)?;
    let real_root = canonical_root(root)?;
    if !real_parent.starts_with(&real_root) {
        return Err(VaultError::outside_boundary(&real_parent, &real_root));
    }
    Ok(real_parent.join(file_name))
}

/// Is `path` inside `root`, comparing real locations?
///
/// Answers `false` when either side cannot be resolved, because an unanswerable
/// containment question must never read as "yes".
pub fn is_within(root: &Path, path: &Path) -> bool {
    let (Ok(r), Ok(p)) = (canonical_root(root), canonicalize_existing_prefix(&lexical_normalize(path)))
    else {
        return false;
    };
    p.starts_with(&r)
}

/// Compares two paths textually, after normalizing separators and case where the
/// platform ignores case. Use it only for grouping, never for a security check.
pub fn same_path_lexically(a: &Path, b: &Path) -> bool {
    let (a, b) = (lexical_normalize(a), lexical_normalize(b));
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Rejects a file name that is empty, that contains a separator, or that names a
/// directory entry the engine must never write.
pub fn validate_file_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(VaultError::invalid("The name cannot be empty."));
    }
    if name == "." || name == ".." {
        return Err(VaultError::invalid("That name is not a file name."));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(VaultError::invalid("A file name cannot contain a folder separator."));
    }
    // Windows forbids these outright in a file name, and so does every
    // sensible reading of one. A control character reaches here from a YAML
    // escape such as `\b`, and `:` reaches here from a pasted drive letter.
    // Allowing them means a name that looks fine on Linux and fails on the
    // machine this product runs on.
    if let Some(bad) = name
        .chars()
        .find(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err(VaultError::invalid(format!(
            "That name contains {}, which Windows cannot store in a file name.",
            if bad.is_control() {
                "a control character".to_string()
            } else {
                format!("\"{bad}\"")
            }
        )));
    }
    // Windows refuses these device names in any folder, with or without an
    // extension. Creating one silently fails or opens a device.
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = name.split('.').next().unwrap_or(name).to_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(VaultError::new(
            ErrorCode::InvalidArgument,
            format!("Windows reserves the name \"{stem}\" and cannot store a file called that."),
        ));
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err(VaultError::invalid(
            "A file name cannot end with a space or a dot, because Windows drops it.",
        ));
    }
    Ok(())
}

/// A relative folder path the interface supplied, checked component by component.
pub fn validate_relative_dir(rel: &str) -> Result<PathBuf> {
    if rel.is_empty() {
        return Ok(PathBuf::new());
    }
    let p = PathBuf::from(rel);
    if p.is_absolute() {
        return Err(VaultError::invalid("That folder must be given relative to the install."));
    }
    for comp in p.components() {
        match comp {
            Component::Normal(c) => {
                let s = c.to_string_lossy();
                if s.contains('\0') {
                    return Err(VaultError::invalid("That folder name contains a character the disk cannot store."));
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(VaultError::invalid("That folder path cannot contain \"..\"."));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(VaultError::invalid("That folder must be given relative to the install."));
            }
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[cfg(unix)]
    fn link(target: &Path, at: &Path) {
        std::os::unix::fs::symlink(target, at).expect("symlink");
    }
    #[cfg(windows)]
    fn link(target: &Path, at: &Path) {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, at).expect("symlink");
        } else {
            std::os::windows::fs::symlink_file(target, at).expect("symlink");
        }
    }

    #[test]
    fn lexical_normalize_resolves_dot_and_parent() {
        assert_eq!(lexical_normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(lexical_normalize(Path::new("/a/b/../../c")), PathBuf::from("/c"));
    }

    #[test]
    fn lexical_normalize_never_climbs_above_an_absolute_root() {
        // `/..` has nowhere to go. It must stay at the root, not become empty.
        assert_eq!(lexical_normalize(Path::new("/../../etc")), PathBuf::from("/etc"));
    }

    #[test]
    fn resolve_within_accepts_a_path_inside_the_root() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("models/loras")).unwrap();
        let got = resolve_within(root, Path::new("models/loras")).unwrap();
        assert!(got.ends_with("loras"));
        assert!(is_within(root, &got));
    }

    #[test]
    fn resolve_within_rejects_a_parent_escape() {
        let d = tmp();
        let root = d.path().join("root");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(d.path().join("outside")).unwrap();

        let err = resolve_within(&root, Path::new("../outside")).unwrap_err();
        assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
    }

    #[test]
    fn resolve_within_rejects_an_absolute_path_outside_the_root() {
        let d = tmp();
        let root = d.path().join("root");
        let other = d.path().join("other");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&other).unwrap();

        let err = resolve_within(&root, &other).unwrap_err();
        assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
    }

    #[test]
    fn resolve_within_rejects_an_escape_through_a_symbolic_link() {
        // The attack this module exists to stop: a link inside the boundary that
        // points outside it. Lexical checks alone pass this; canonicalizing the
        // existing prefix catches it.
        let d = tmp();
        let root = d.path().join("root");
        let secret = d.path().join("secret");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&secret).unwrap();
        fs::write(secret.join("keys.txt"), b"x").unwrap();
        link(&secret, &root.join("escape"));

        let err = resolve_within(&root, Path::new("escape/keys.txt")).unwrap_err();
        assert_eq!(err.code, ErrorCode::PathOutsideBoundary, "symlink escape was allowed");
    }

    #[test]
    fn resolve_within_allows_a_link_that_stays_inside_the_root() {
        let d = tmp();
        let root = d.path().join("root");
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("real/w.safetensors"), b"x").unwrap();
        link(&root.join("real"), &root.join("alias"));

        let got = resolve_within(&root, Path::new("alias/w.safetensors")).unwrap();
        assert!(got.ends_with("real/w.safetensors") || got.ends_with(r"real\w.safetensors"));
    }

    #[test]
    fn resolve_new_path_within_does_not_follow_the_final_link() {
        // Removing a link must act on the link. If the final component were
        // resolved, the engine would delete the vault file instead.
        let d = tmp();
        let root = d.path().join("root");
        let vault = d.path().join("vault");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&vault).unwrap();
        fs::write(vault.join("real.safetensors"), b"weights").unwrap();
        link(&vault.join("real.safetensors"), &root.join("linked.safetensors"));

        let got = resolve_new_path_within(&root, Path::new("linked.safetensors")).unwrap();
        assert!(got.ends_with("linked.safetensors"));
        assert!(
            std::fs::symlink_metadata(&got).unwrap().file_type().is_symlink(),
            "the resolver followed the link it was told to keep literal"
        );
        // And the file it points at still exists, untouched.
        assert!(vault.join("real.safetensors").exists());
    }

    #[test]
    fn resolve_new_path_within_rejects_a_parent_that_escapes() {
        let d = tmp();
        let root = d.path().join("root");
        let outside = d.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        link(&outside, &root.join("escape"));

        let err = resolve_new_path_within(&root, Path::new("escape/new.safetensors")).unwrap_err();
        assert_eq!(err.code, ErrorCode::PathOutsideBoundary);
    }

    #[test]
    fn resolve_within_works_for_a_path_that_does_not_exist_yet() {
        let d = tmp();
        let root = d.path();
        fs::create_dir_all(root.join("models")).unwrap();
        let got = resolve_within(root, Path::new("models/new/deeper/file.safetensors")).unwrap();
        assert!(got.ends_with("file.safetensors"));
        assert!(!got.exists());
    }

    #[test]
    fn is_within_answers_false_when_it_cannot_resolve() {
        assert!(!is_within(Path::new("/definitely/not/here"), Path::new("/definitely/not/here/x")));
    }

    #[test]
    fn validate_relative_dir_rejects_traversal_and_absolutes() {
        assert!(validate_relative_dir("models/loras").is_ok());
        assert!(validate_relative_dir("").is_ok());
        assert!(validate_relative_dir("../escape").is_err());
        assert!(validate_relative_dir("models/../../escape").is_err());
        assert!(validate_relative_dir("/absolute").is_err());
    }

    #[test]
    fn validate_file_name_rejects_windows_traps() {
        assert!(validate_file_name("lora1.safetensors").is_ok());
        assert!(validate_file_name("").is_err());
        assert!(validate_file_name("..").is_err());
        assert!(validate_file_name("a/b").is_err());
        assert!(validate_file_name("a\\b").is_err());
        // Reserved device names, with and without an extension.
        assert!(validate_file_name("CON").is_err());
        assert!(validate_file_name("con.safetensors").is_err());
        assert!(validate_file_name("LPT1.ckpt").is_err());
        // Windows silently strips a trailing dot or space, so the file the
        // engine creates would not be the file it recorded.
        assert!(validate_file_name("model.safetensors ").is_err());
        assert!(validate_file_name("model.").is_err());
    }

    #[test]
    fn display_path_strips_the_windows_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\C:\ComfyVault\loras")), r"C:\ComfyVault\loras");
        assert_eq!(display_path(Path::new(r"\\?\UNC\server\share\x")), r"\\server\share\x");
        assert_eq!(display_path(Path::new("/home/x")), "/home/x");
    }
}

/// One path, reduced to something two paths can be compared by.
///
/// Windows tells `/` and `\` apart in text but not on disk, and it ignores
/// case. Comparing two paths as written said a link and the file it replaced
/// were different places, and the same content got counted twice.
pub fn compare_key(p: &Path) -> String {
    let s = display_path(p);
    if cfg!(windows) {
        s.replace('/', "\\").to_lowercase()
    } else {
        s
    }
}
