//! The parts of Python's `os.path` that ComfyUI's configuration parser uses.
//!
//! # Why this exists
//!
//! ComfyUI resolves the folders in `extra_model_paths.yaml` with `os.path.join`,
//! `os.path.normpath`, `os.path.expanduser` and `os.path.expandvars`. Those
//! functions have behavior Rust's [`std::path`] does not copy, and the
//! differences are not cosmetic:
//!
//! * `os.path.join(r"C:\base", "C:x")` returns `C:\base\x`, because a
//!   drive-relative tail on the same drive is joined. Rust's `Path::join`
//!   returns `C:x`, because it sees a prefix and replaces everything.
//! * `os.path.join(r"\\srv\share", "models")` returns `\\srv\share\models`.
//!   The separator after a bare UNC share is a special case in Python.
//! * `os.path.normpath` resolves `..` textually. Rust's standard library has no
//!   equivalent at all.
//! * `expanduser` and `expandvars` do not exist in Rust's standard library, and
//!   the exact rule for an undefined variable decides what folder gets scanned.
//!
//! If the engine resolved a folder differently from ComfyUI, it would scan a
//! folder ComfyUI never reads, or miss one it does. Either way the person's
//! duplicates would not be found. So these rules are reimplemented exactly,
//! following CPython's `ntpath` and `posixpath`.
//!
//! One rule is version dependent. Python 3.12 and earlier call a drive-rooted
//! path such as `\models` absolute; Python 3.13 does not. This module follows
//! 3.12, which is what the ComfyUI portable build ships. The difference only
//! shows for a category path that starts with a separator and has no
//! `base_path`, which is rare.
//!
//! # Why the style is a parameter
//!
//! [`PathStyle`] is chosen by the caller instead of by `cfg!(windows)`, so the
//! Windows rules are unit tested from Linux. The engine passes
//! [`PathStyle::host`] at run time.

use std::collections::HashMap;

/// Which platform's `os.path` rules to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    Windows,
    Posix,
}

impl PathStyle {
    /// The rules of the system this build runs on.
    pub fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }

    fn sep(self) -> char {
        match self {
            Self::Windows => '\\',
            Self::Posix => '/',
        }
    }

    fn is_sep(self, c: char) -> bool {
        match self {
            Self::Windows => c == '\\' || c == '/',
            Self::Posix => c == '/',
        }
    }
}

/// Splits a Windows path into its drive (or UNC share) and the rest.
///
/// Mirrors `ntpath.splitdrive`.
fn split_drive(style: PathStyle, p: &str) -> (&str, &str) {
    if style == PathStyle::Posix {
        return ("", p);
    }
    let b: Vec<char> = p.chars().collect();
    if b.len() >= 2 {
        // A UNC path: \\server\share
        if style.is_sep(b[0]) && style.is_sep(b[1]) && (b.len() < 3 || !style.is_sep(b[2])) {
            // Find the separator that ends the server, then the one that ends
            // the share. The drive is everything up to the second.
            let after = &p[2..];
            let mut it = after.char_indices().filter(|(_, c)| style.is_sep(*c));
            if let Some((srv_end, _)) = it.next() {
                if let Some((shr_end, _)) = it.next() {
                    let cut = 2 + shr_end;
                    return (&p[..cut], &p[cut..]);
                }
                // `\\server\share` with nothing after it is all drive.
                let _ = srv_end;
                return (p, "");
            }
            return ("", p);
        }
        if b[1] == ':' {
            return (&p[..2], &p[2..]);
        }
    }
    ("", p)
}

/// Splits a path into its drive, its root separator, and the rest.
///
/// Mirrors `ntpath.splitroot` and `posixpath.splitroot`. [`join`] is built on
/// it, exactly as CPython builds its own.
fn split_root(style: PathStyle, p: &str) -> (&str, &str, &str) {
    match style {
        PathStyle::Posix => {
            if !p.starts_with('/') {
                ("", "", p)
            } else if !p[1..].starts_with('/') || p[2..].starts_with('/') {
                ("", &p[..1], &p[1..])
            } else {
                // `//foo` is implementation defined and kept whole.
                ("", &p[..2], &p[2..])
            }
        }
        PathStyle::Windows => {
            let norm = p.replace('/', "\\");
            if norm.starts_with('\\') {
                if norm[1..].starts_with('\\') {
                    // A UNC share or a device path: the drive is
                    // `\\server\share`, and the root is what follows it.
                    let start = if norm.len() >= 8 && norm[..8].eq_ignore_ascii_case(r"\\?\UNC\") {
                        8
                    } else {
                        2
                    };
                    let Some(i) = norm[start..].find('\\').map(|i| i + start) else {
                        return (p, "", "");
                    };
                    let Some(j) = norm[i + 1..].find('\\').map(|k| k + i + 1) else {
                        return (p, "", "");
                    };
                    (&p[..j], &p[j..j + 1], &p[j + 1..])
                } else {
                    // Rooted but with no drive, for example `\models`.
                    ("", &p[..1], &p[1..])
                }
            } else if norm[1..].starts_with(':') {
                if norm[2..].starts_with('\\') {
                    (&p[..2], &p[2..3], &p[3..])
                } else {
                    // Drive relative, for example `C:models`.
                    (&p[..2], "", &p[2..])
                }
            } else {
                ("", "", p)
            }
        }
    }
}

/// True when Python would call the path absolute.
///
/// Follows Python 3.12: the path is absolute when what follows the drive starts
/// with a separator. So `\models` is absolute and `C:models` is not. Python 3.13
/// changed the first of those. See the module note.
pub fn isabs(style: PathStyle, p: &str) -> bool {
    match style {
        PathStyle::Posix => p.starts_with('/'),
        PathStyle::Windows => {
            let (_, rest) = split_drive(style, p);
            rest.starts_with('\\') || rest.starts_with('/')
        }
    }
}

/// Mirrors `os.path.join` for two components, following CPython's algorithm.
pub fn join(style: PathStyle, base: &str, tail: &str) -> String {
    if style == PathStyle::Posix {
        return if tail.starts_with('/') {
            tail.to_string()
        } else if base.is_empty() || base.ends_with('/') {
            format!("{base}{tail}")
        } else {
            format!("{base}/{tail}")
        };
    }

    let (base_drive, base_root, base_path) = split_root(style, base);
    let (tail_drive, tail_root, tail_path) = split_root(style, tail);

    let mut drive = base_drive.to_string();
    let mut root = base_root.to_string();
    let mut path = base_path.to_string();

    if !tail_root.is_empty() {
        // The tail is rooted, so it replaces the path. It keeps its own drive
        // when it has one, otherwise it lands on the base's drive.
        if !tail_drive.is_empty() || drive.is_empty() {
            drive = tail_drive.to_string();
        }
        root = tail_root.to_string();
        path = tail_path.to_string();
    } else {
        if !tail_drive.is_empty() && !tail_drive.eq_ignore_ascii_case(&drive) {
            // A different drive discards the base entirely.
            return tail.to_string();
        }
        if !tail_drive.is_empty() {
            drive = tail_drive.to_string();
        }
        if !path.is_empty() && !path.ends_with(['\\', '/']) {
            path.push('\\');
        }
        path.push_str(tail_path);
    }

    // A bare UNC share carries no root, so Python inserts the separator here.
    // Without this, `\\srv\share` joined with `models` loses the separator.
    if !path.is_empty()
        && root.is_empty()
        && !drive.is_empty()
        && !drive.ends_with([':', '\\', '/'])
    {
        return format!("{drive}\\{path}");
    }
    format!("{drive}{root}{path}")
}

/// Mirrors `os.path.normpath`: collapses separators, `.` and `..`, without
/// touching the disk.
pub fn normpath(style: PathStyle, p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let sep = style.sep();
    let (drive, rest) = split_drive(style, p);
    let rest_norm: String = if style == PathStyle::Windows {
        rest.replace('/', "\\")
    } else {
        rest.to_string()
    };

    let rooted = rest_norm.starts_with(sep);
    let mut out: Vec<&str> = Vec::new();
    for part in rest_norm.split(sep) {
        match part {
            "" | "." => {}
            ".." => {
                match out.last() {
                    Some(&"..") => out.push(".."),
                    Some(_) => {
                        out.pop();
                    }
                    None => {
                        // Above a root there is nothing, so Python drops it.
                        if !rooted {
                            out.push("..");
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }

    let mut s = String::new();
    s.push_str(drive);
    if rooted {
        s.push(sep);
    }
    s.push_str(&out.join(&sep.to_string()));
    if s.is_empty() {
        ".".to_string()
    } else {
        s
    }
}

/// Mirrors `os.path.abspath` with an explicit working directory.
pub fn abspath(style: PathStyle, cwd: &str, p: &str) -> String {
    if isabs(style, p) {
        normpath(style, p)
    } else {
        normpath(style, &join(style, cwd, p))
    }
}

/// Mirrors `os.path.expanduser`.
///
/// Only a leading `~` or `~/` is expanded. A `~` anywhere else is a literal
/// character, and ComfyUI's parser relies on that.
pub fn expanduser(style: PathStyle, p: &str, home: Option<&str>) -> String {
    if !p.starts_with('~') {
        return p.to_string();
    }
    let rest: String = p.chars().skip(1).collect();
    // `~user` is not expanded here: ComfyUI would need the user database, and a
    // wrong guess would point at the wrong person's folder.
    if !rest.is_empty() && !style.is_sep(rest.chars().next().unwrap()) {
        return p.to_string();
    }
    match home {
        Some(h) => {
            let h = h.trim_end_matches(|c| style.is_sep(c));
            format!("{h}{rest}")
        }
        None => p.to_string(),
    }
}

/// Mirrors `os.path.expandvars`.
///
/// Windows expands `%NAME%`, `$NAME` and `${NAME}`. Posix expands `$NAME` and
/// `${NAME}`. An undefined name is left exactly as written, which is what
/// Python does and what produces the surprising literal paths people report.
pub fn expandvars(style: PathStyle, p: &str, env: &HashMap<String, String>) -> String {
    if !p.contains('$') && !(style == PathStyle::Windows && p.contains('%')) {
        return p.to_string();
    }
    let chars: Vec<char> = p.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if style == PathStyle::Windows && c == '%' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == '%') {
                let name: String = chars[i + 1..i + 1 + close].iter().collect();
                match env.get(&name) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push('%');
                        out.push_str(&name);
                        out.push('%');
                    }
                }
                i += close + 2;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == '$' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                if let Some(close) = chars[i + 2..].iter().position(|&c| c == '}') {
                    let name: String = chars[i + 2..i + 2 + close].iter().collect();
                    match env.get(&name) {
                        Some(v) => out.push_str(v),
                        None => out.push_str(&format!("${{{name}}}")),
                    }
                    i += close + 3;
                    continue;
                }
                out.push(c);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if j == i + 1 {
                out.push(c);
                i += 1;
                continue;
            }
            let name: String = chars[i + 1..j].iter().collect();
            match env.get(&name) {
                Some(v) => out.push_str(v),
                None => {
                    out.push('$');
                    out.push_str(&name);
                }
            }
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::PathStyle::{Posix, Windows};
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    // --- join -------------------------------------------------------------
    // Each expected value below is what CPython's os.path.join returns.

    #[test]
    fn windows_join_appends_a_relative_tail() {
        assert_eq!(join(Windows, r"C:\base", "models"), r"C:\base\models");
        assert_eq!(join(Windows, r"C:\base\", "models"), r"C:\base\models");
    }

    #[test]
    fn windows_join_lets_an_absolute_tail_win() {
        assert_eq!(join(Windows, r"C:\base", r"D:\other"), r"D:\other");
    }

    #[test]
    fn windows_join_keeps_the_drive_when_the_tail_is_rooted() {
        // The rule Rust's Path::join gets wrong. A category path written as
        // `\models\loras` lands on the base's drive, not under the base folder.
        assert_eq!(join(Windows, r"C:\base", r"\models"), r"C:\models");
        assert_eq!(join(Windows, r"C:\base", "/models"), r"C:/models");
    }

    #[test]
    fn windows_join_handles_a_same_drive_tail() {
        assert_eq!(join(Windows, r"C:\base", r"C:\x"), r"C:\x");
        assert_eq!(join(Windows, r"C:\base", "C:x"), r"C:\base\x");
    }

    #[test]
    fn windows_join_handles_a_unc_base() {
        assert_eq!(join(Windows, r"\\srv\share", "models"), r"\\srv\share\models");
        assert_eq!(join(Windows, r"\\srv\share\a", r"\b"), r"\\srv\share\b");
    }

    #[test]
    fn posix_join_is_the_simple_rule() {
        assert_eq!(join(Posix, "/base", "models"), "/base/models");
        assert_eq!(join(Posix, "/base/", "models"), "/base/models");
        assert_eq!(join(Posix, "/base", "/abs"), "/abs");
    }

    // --- isabs ------------------------------------------------------------

    #[test]
    fn windows_absoluteness_follows_python_3_12() {
        // A drive-rooted path counts as absolute here. Python 3.13 changed
        // this; the module note explains why 3.12 is the rule followed.
        assert!(isabs(Windows, r"\models"));
        assert!(isabs(Windows, r"C:\models"));
        assert!(!isabs(Windows, "C:models"), "drive relative is not absolute");
        assert!(isabs(Windows, r"\\srv\share\x"));
        assert!(!isabs(Windows, r"models\loras"));
    }

    #[test]
    fn posix_absolute_is_a_leading_slash() {
        assert!(isabs(Posix, "/a"));
        assert!(!isabs(Posix, "a"));
        assert!(!isabs(Posix, "./a"));
    }

    // --- normpath ---------------------------------------------------------

    #[test]
    fn normpath_collapses_dots_and_separators() {
        assert_eq!(normpath(Windows, r"C:\a\.\b\..\c"), r"C:\a\c");
        assert_eq!(normpath(Windows, r"C:\a\\b"), r"C:\a\b");
        assert_eq!(normpath(Posix, "/a/./b/../c"), "/a/c");
        assert_eq!(normpath(Posix, "/a//b"), "/a/b");
    }

    #[test]
    fn normpath_turns_forward_slashes_into_backslashes_on_windows() {
        assert_eq!(normpath(Windows, "C:/a/b"), r"C:\a\b");
    }

    #[test]
    fn normpath_keeps_leading_parent_parts_on_a_relative_path() {
        assert_eq!(normpath(Posix, "../../a"), "../../a");
        assert_eq!(normpath(Windows, r"..\..\a"), r"..\..\a");
    }

    #[test]
    fn normpath_drops_a_parent_that_climbs_above_a_root() {
        assert_eq!(normpath(Posix, "/../a"), "/a");
        assert_eq!(normpath(Windows, r"C:\..\a"), r"C:\a");
    }

    #[test]
    fn normpath_of_empty_is_a_dot() {
        assert_eq!(normpath(Posix, ""), ".");
        assert_eq!(normpath(Windows, ""), ".");
    }

    // --- expanduser -------------------------------------------------------

    #[test]
    fn expanduser_replaces_a_leading_tilde_only() {
        assert_eq!(expanduser(Posix, "~/models", Some("/home/sam")), "/home/sam/models");
        assert_eq!(
            expanduser(Windows, r"~\models", Some(r"C:\Users\sam")),
            r"C:\Users\sam\models"
        );
        assert_eq!(expanduser(Posix, "~", Some("/home/sam")), "/home/sam");
    }

    #[test]
    fn expanduser_leaves_a_tilde_in_the_middle_alone() {
        assert_eq!(expanduser(Posix, "/a/~/b", Some("/home/sam")), "/a/~/b");
    }

    #[test]
    fn expanduser_leaves_a_named_user_alone() {
        // Guessing another person's home folder would point the scan at the
        // wrong place, so it is left exactly as written.
        assert_eq!(expanduser(Posix, "~other/models", Some("/home/sam")), "~other/models");
    }

    // --- expandvars -------------------------------------------------------

    #[test]
    fn expandvars_handles_percent_form_on_windows() {
        let e = env(&[("USERPROFILE", r"C:\Users\sam")]);
        assert_eq!(
            expandvars(Windows, r"%USERPROFILE%\models", &e),
            r"C:\Users\sam\models"
        );
    }

    #[test]
    fn expandvars_handles_dollar_forms() {
        let e = env(&[("HOME", "/home/sam")]);
        assert_eq!(expandvars(Posix, "$HOME/models", &e), "/home/sam/models");
        assert_eq!(expandvars(Posix, "${HOME}/models", &e), "/home/sam/models");
    }

    #[test]
    fn expandvars_leaves_an_undefined_name_exactly_as_written() {
        // Python does this, and it is how a folder literally named `%NOPE%`
        // ends up in a scan. The engine has to reproduce it to agree.
        let e = env(&[]);
        assert_eq!(expandvars(Windows, r"%NOPE%\models", &e), r"%NOPE%\models");
        assert_eq!(expandvars(Posix, "$NOPE/models", &e), "$NOPE/models");
        assert_eq!(expandvars(Posix, "${NOPE}/models", &e), "${NOPE}/models");
    }

    #[test]
    fn expandvars_leaves_a_lone_marker_alone() {
        let e = env(&[("A", "x")]);
        assert_eq!(expandvars(Posix, "100$", &e), "100$");
        assert_eq!(expandvars(Windows, "50%", &e), "50%");
        assert_eq!(expandvars(Posix, "${unclosed", &e), "${unclosed");
    }

    // --- abspath ----------------------------------------------------------

    #[test]
    fn abspath_resolves_against_the_given_working_directory() {
        assert_eq!(abspath(Posix, "/yaml/dir", "models/loras"), "/yaml/dir/models/loras");
        assert_eq!(abspath(Windows, r"C:\yaml\dir", r"models\loras"), r"C:\yaml\dir\models\loras");
        assert_eq!(abspath(Posix, "/yaml/dir", "/already/absolute"), "/already/absolute");
    }

    #[test]
    fn abspath_normalizes_a_parent_step_in_the_relative_part() {
        assert_eq!(abspath(Posix, "/yaml/dir", "../models"), "/yaml/models");
    }
}
