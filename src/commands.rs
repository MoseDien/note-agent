//! Short-command prefix resolution shared by Terminal and Telegram.
//!
//! A slash command may be invoked by its shortest unique prefix: `/d` resolves
//! to `/delete`, `/r` to `/recent`, and so on. When two commands share a prefix,
//! the caller must type enough characters to disambiguate (`/privat` for
//! `/private`, `/privac` for `/privacy`). Commands in the `*_FULL_ONLY` sets are
//! matched by their full name only and are never abbreviated — this keeps
//! `/link` and `/log` from being triggered by a typo.

/// Telegram commands that may be abbreviated by shortest unique prefix.
pub const TELEGRAM_COMMANDS: &[&str] = &[
    "start",
    "help",
    "helo",
    "classify",
    "show",
    "recent",
    "connections",
    "export",
    "delete",
    "privacy",
    "private",
];
/// Telegram commands matched by full name only (never abbreviated).
pub const TELEGRAM_FULL_ONLY: &[&str] = &["link", "log"];

/// Terminal interactive commands that may be abbreviated by shortest unique prefix.
pub const TERMINAL_COMMANDS: &[&str] = &["exit", "quit", "recent", "connections", "delete"];
/// Terminal commands matched by full name only.
pub const TERMINAL_FULL_ONLY: &[&str] = &[];

/// Splits `input` into the first whitespace-delimited token and the remaining
/// arguments (leading whitespace trimmed). Input without whitespace yields
/// `(input, "")`.
fn split_command(input: &str) -> (&str, &str) {
    match input.find(char::is_whitespace) {
        Some(idx) => (&input[..idx], input[idx..].trim_start()),
        None => (input, ""),
    }
}

/// Resolves a command token (with a leading `/`) to its canonical full name.
///
/// An exact full match always wins. Otherwise the token is treated as a prefix
/// and matched against `eligible` only; it resolves only if it is a unique
/// prefix of exactly one command. Returns `None` for non-slash, empty,
/// ambiguous, or unrecognized input.
pub fn resolve<'a>(
    token: &str,
    eligible: &'a [&'a str],
    full_only: &'a [&'a str],
) -> Option<&'a str> {
    let name = token.strip_prefix('/')?;
    if name.is_empty() {
        return None;
    }
    for cmd in eligible.iter().copied().chain(full_only.iter().copied()) {
        if cmd == name {
            return Some(cmd);
        }
    }
    let mut matched: Option<&'a str> = None;
    for cmd in eligible.iter().copied() {
        if cmd.starts_with(name) {
            if matched.is_some() {
                return None;
            }
            matched = Some(cmd);
        }
    }
    matched
}

/// Expands a short command prefix in `input` to its full form, preserving any
/// arguments. Input that is not a recognized command is returned unchanged, so
/// plain text and unknown commands pass through untouched.
pub fn expand(input: &str, eligible: &[&str], full_only: &[&str]) -> String {
    let (token, rest) = split_command(input);
    match resolve(token, eligible, full_only) {
        Some(cmd) if rest.is_empty() => format!("/{cmd}"),
        Some(cmd) => format!("/{cmd} {rest}"),
        None => input.to_string(),
    }
}

/// Expands Telegram short-command prefixes to their full forms.
pub fn expand_telegram(input: &str) -> String {
    let (token, rest) = split_command(input);
    let alias = match token {
        "/h" => Some("helo"),
        "/c" => Some("classify"),
        "/s" => Some("show"),
        "/r" => Some("recent"),
        "/d" => Some("delete"),
        "/e" => Some("export"),
        "/a" => Some("categories"),
        _ => None,
    };
    if let Some(command) = alias {
        return if rest.is_empty() {
            format!("/{command}")
        } else {
            format!("/{command} {rest}")
        };
    }
    expand(input, TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY)
}

/// Expands Terminal short-command prefixes to their full forms.
pub fn expand_terminal(input: &str) -> String {
    expand(input, TERMINAL_COMMANDS, TERMINAL_FULL_ONLY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_unique_single_letter_prefixes() {
        for (short, full) in [
            ("/d", "delete"),
            ("/r", "recent"),
            ("/c", "classify"),
            ("/e", "export"),
            ("/h", "helo"),
            ("/s", "show"),
        ] {
            assert_eq!(expand_telegram(short), format!("/{full}"));
        }
    }

    #[test]
    fn rejects_ambiguous_or_too_short_prefixes() {
        for short in ["/p", "/pri", "/priv", "/l"] {
            assert_eq!(
                resolve(short, TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY),
                None,
                "{short} should be ambiguous"
            );
        }
    }

    #[test]
    fn resolves_overlapping_commands_at_minimal_unique_prefix() {
        assert_eq!(
            resolve("/privat", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY),
            Some("private")
        );
        assert_eq!(
            resolve("/privac", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY),
            Some("privacy")
        );
    }

    #[test]
    fn full_only_commands_are_not_abbreviated() {
        assert_eq!(
            resolve("/link", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY),
            Some("link")
        );
        assert_eq!(resolve("/lin", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY), None);
        assert_eq!(
            resolve("/log", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY),
            Some("log")
        );
        assert_eq!(resolve("/lo", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY), None);
    }

    #[test]
    fn non_slash_and_empty_input_do_not_resolve() {
        assert_eq!(
            resolve("delete", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY),
            None
        );
        assert_eq!(resolve("/", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY), None);
        assert_eq!(resolve("", TELEGRAM_COMMANDS, TELEGRAM_FULL_ONLY), None);
    }

    #[test]
    fn expand_preserves_arguments_and_passes_through_unknown_input() {
        assert_eq!(expand_terminal("/d -2"), "/delete -2");
        assert_eq!(expand_terminal("/r"), "/recent");
        assert_eq!(expand_telegram("/privat my note"), "/private my note");
        assert_eq!(expand_telegram("/delete abc"), "/delete abc");
        // plain text and non-slash input pass through unchanged
        assert_eq!(expand_terminal("plain text"), "plain text");
        assert_eq!(expand_terminal("delete 5"), "delete 5");
        // link/log are full-word only, so prefixes pass through unchanged
        assert_eq!(expand_telegram("/lin x"), "/lin x");
        assert_eq!(expand_telegram("/lo x"), "/lo x");
        // ambiguous prefixes pass through unchanged
        assert_eq!(expand_telegram("/p"), "/p");
    }
}
