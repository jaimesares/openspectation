//! Locale detection and activation.
//!
//! The catalogues themselves live in `locales/*.yml` and are compiled into the
//! binary by the `i18n!` macro invoked in `lib.rs`. This module only decides
//! *which* catalogue is active.
//!
//! Resolution order, highest priority first:
//!
//! 1. the `--lang` flag,
//! 2. the `OPENSPECTATION_LANG` environment variable,
//! 3. the language stored in the user's preferences,
//! 4. `LC_ALL`, `LC_MESSAGES`, `LANG`, `LANGUAGE` (in that order),
//! 5. the `en` fallback.

/// Locale used when nothing else matches.
pub const FALLBACK: &str = "en";

/// Environment variable that overrides the detected system locale.
pub const LANG_ENV: &str = "OPENSPECTATION_LANG";

/// System variables consulted for a locale, highest priority first.
const SYSTEM_LANG_ENV: [&str; 4] = ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"];

/// Every locale compiled into the binary, sorted alphabetically.
pub fn available() -> Vec<String> {
    let mut locales = rust_i18n::available_locales!()
        .into_iter()
        .map(|locale| locale.into_owned())
        .collect::<Vec<_>>();
    locales.sort_unstable();
    locales
}

/// Currently active locale.
pub fn current() -> String {
    rust_i18n::locale().to_string()
}

/// Resolves and installs the effective locale, returning what was applied.
pub fn activate(explicit: Option<&str>) -> String {
    let locale = resolve(explicit);
    rust_i18n::set_locale(&locale);
    locale
}

/// Selects the next locale in the compiled catalogue list and installs it.
///
/// Used by the TUI to demonstrate live language switching.
pub fn cycle() -> String {
    let locales = available();
    let Some(next) = next_locale(&locales, &current()) else {
        return FALLBACK.to_string();
    };
    rust_i18n::set_locale(&next);
    next
}

/// The catalogue after `active`, wrapping around at the end of the list.
///
/// `None` only for a build with no catalogue at all, which the caller answers
/// with the fallback rather than a panic.
fn next_locale(locales: &[String], active: &str) -> Option<String> {
    if locales.is_empty() {
        return None;
    }
    let index = locales
        .iter()
        .position(|item| item.as_str() == active)
        .unwrap_or(0);
    locales.get((index + 1) % locales.len()).cloned()
}

/// Resolves the locale without installing it.
///
/// The stored preference sits between the environment and the system locale: a
/// user who picked a language in the settings panel means it for every session,
/// but `LANG` on a single invocation is still an explicit instruction.
pub fn resolve(explicit: Option<&str>) -> String {
    resolve_from(
        explicit,
        std::env::var(LANG_ENV).ok().as_deref(),
        stored_language,
        system_candidates(),
    )
}

/// The precedence behind [`resolve`], with every source handed in.
///
/// Split out because those sources are process wide state, and `set_var` races
/// with every other thread reading the environment: the ordering is checked by
/// calling this, never by mutating the process. `stored` stays a closure
/// because reading it opens a file, and a `--lang` flag must not pay for a
/// preference it overrules.
fn resolve_from(
    explicit: Option<&str>,
    environment: Option<&str>,
    stored: impl FnOnce() -> Option<String>,
    system: impl IntoIterator<Item = String>,
) -> String {
    explicit
        .and_then(match_supported)
        .or_else(|| environment.and_then(match_supported))
        .or_else(|| stored().as_deref().and_then(match_supported))
        .or_else(|| {
            system
                .into_iter()
                // `LANGUAGE` may hold a colon separated priority list.
                .find_map(|raw| raw.split(':').find_map(match_supported))
        })
        .unwrap_or_else(|| FALLBACK.to_string())
}

/// Raw locale strings the system offers, highest priority first.
fn system_candidates() -> Vec<String> {
    SYSTEM_LANG_ENV
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .collect()
}

fn stored_language() -> Option<String> {
    crate::domain::settings::Settings::load().language
}

/// Reads the raw process arguments looking for `--lang`.
///
/// clap cannot be used here because the localized help text must already exist
/// when the parser is constructed.
pub fn prescan_locale() -> Option<String> {
    match lang_argument(std::env::args().skip(1)) {
        Some(requested) => requested,
        None => std::env::var(LANG_ENV)
            .ok()
            .as_deref()
            .and_then(match_supported),
    }
}

/// The locale named by `--lang`, if the flag is there at all.
///
/// The nesting carries the distinction the environment fallback hangs on: the
/// outer `Option` says whether the flag appeared, the inner one whether its
/// value named a compiled catalogue. A `--lang` nobody has translated into is
/// still an instruction, and must not quietly become `LANG` instead.
fn lang_argument(arguments: impl IntoIterator<Item = String>) -> Option<Option<String>> {
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        if let Some(value) = argument.strip_prefix("--lang=") {
            return Some(match_supported(value));
        }
        if argument == "--lang" {
            return Some(arguments.next().as_deref().and_then(match_supported));
        }
    }

    None
}

/// Maps a raw locale string such as `es_ES.UTF-8@euro` onto a compiled locale.
///
/// An exact match wins; otherwise the primary language subtag is used, so
/// `pt-BR` resolves to `pt` when only a generic Portuguese catalogue exists.
pub fn match_supported(raw: &str) -> Option<String> {
    let cleaned = raw
        .split(['.', '@'])
        .next()
        .unwrap_or(raw)
        .trim()
        .replace('_', "-");

    if cleaned.is_empty()
        || cleaned.eq_ignore_ascii_case("C")
        || cleaned.eq_ignore_ascii_case("POSIX")
    {
        return None;
    }

    let locales = available();

    if let Some(found) = locales
        .iter()
        .find(|item| item.eq_ignore_ascii_case(&cleaned))
    {
        return Some(found.clone());
    }

    let primary = cleaned.split('-').next().unwrap_or(&cleaned);
    locales
        .iter()
        .find(|item| item.eq_ignore_ascii_case(primary))
        .cloned()
}

/// Human readable label for a locale code, used in the TUI header.
pub fn display_name(code: &str) -> &'static str {
    match code {
        "es" => "Español",
        "en" => "English",
        _ => "—",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn normalises_system_locale_strings() {
        assert_eq!(match_supported("es_ES.UTF-8@euro").as_deref(), Some("es"));
        assert_eq!(match_supported("en-GB").as_deref(), Some("en"));
        assert_eq!(match_supported("EN").as_deref(), Some("en"));
        assert_eq!(match_supported("C"), None);
        assert_eq!(match_supported("POSIX"), None);
        assert_eq!(match_supported(""), None);
        assert_eq!(match_supported("de_DE.UTF-8"), None);
    }

    #[test]
    fn every_locale_is_compiled_in() {
        let locales = available();
        assert!(locales.iter().any(|locale| locale == FALLBACK));
        assert!(locales.iter().all(|locale| display_name(locale) != "—"));
    }

    #[test]
    fn a_locale_with_no_catalogue_has_no_label() {
        assert_eq!(display_name("es"), "Español");
        assert_eq!(display_name("en"), "English");
        assert_eq!(display_name("de"), "—");
        assert_eq!(display_name(""), "—");
    }

    #[test]
    fn an_explicit_locale_outranks_every_other_source() {
        assert_eq!(
            resolve_from(
                Some("es"),
                Some("en"),
                || panic!("the stored preference must not be read"),
                vec!["en_GB.UTF-8".to_string()],
            ),
            "es"
        );
    }

    #[test]
    fn the_environment_outranks_the_stored_preference() {
        assert_eq!(
            resolve_from(
                None,
                Some("es"),
                || panic!("the stored preference must not be read"),
                vec!["en_GB.UTF-8".to_string()],
            ),
            "es"
        );
    }

    #[test]
    fn the_stored_preference_outranks_the_system_locale() {
        assert_eq!(
            resolve_from(
                None,
                None,
                || Some("es".to_string()),
                vec!["en_US.UTF-8".to_string()],
            ),
            "es"
        );
    }

    #[test]
    fn a_source_naming_no_catalogue_defers_to_the_next_one() {
        // `--lang de` asks for a language nothing was translated into.
        assert_eq!(
            resolve_from(Some("de"), Some("es"), || None, Vec::new()),
            "es"
        );
        assert_eq!(
            resolve_from(
                None,
                Some("C"),
                || Some("klingon".to_string()),
                vec!["es_ES.UTF-8".to_string()],
            ),
            "es"
        );
    }

    #[test]
    fn a_system_priority_list_is_read_left_to_right() {
        assert_eq!(
            resolve_from(None, None, || None, vec!["de_DE:es_ES:en_GB".to_string()]),
            "es"
        );
        // Across variables the first one that names a catalogue wins.
        assert_eq!(
            resolve_from(
                None,
                None,
                || None,
                vec!["de_DE.UTF-8".to_string(), "en_GB.UTF-8".to_string()],
            ),
            "en"
        );
    }

    #[test]
    fn nothing_configured_falls_back_to_english() {
        assert_eq!(resolve_from(None, None, || None, Vec::new()), FALLBACK);
        assert_eq!(
            resolve_from(
                None,
                None,
                || None,
                vec!["C".to_string(), "POSIX".to_string()],
            ),
            FALLBACK
        );
    }

    #[test]
    fn the_stored_preference_comes_from_the_settings_file() {
        let (_guard, path) = crate::domain::settings::redirect_for_test("i18n-stored");

        std::fs::write(&path, "language: es\n").unwrap();
        assert_eq!(stored_language().as_deref(), Some("es"));

        std::fs::write(&path, "theme: nord\n").unwrap();
        assert_eq!(stored_language(), None);

        // The public entry point reads the same sources; only the flag makes
        // its answer independent of the machine running the test.
        assert_eq!(resolve(Some("es")), "es");
        assert!(available().contains(&resolve(None)));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_lang_flag_is_read_in_either_spelling() {
        let matched = Some(Some("es".to_string()));
        assert_eq!(lang_argument(arguments(&["--lang=es"])), matched);
        assert_eq!(lang_argument(arguments(&["--lang", "es"])), matched);
        assert_eq!(
            lang_argument(arguments(&["tui", "--path", "/tmp", "--lang", "ES_es"])),
            matched
        );
    }

    #[test]
    fn a_lang_flag_naming_no_catalogue_does_not_defer_to_the_environment() {
        assert_eq!(lang_argument(arguments(&["--lang=de"])), Some(None));
        assert_eq!(lang_argument(arguments(&["--lang"])), Some(None));
    }

    #[test]
    fn a_lang_flag_after_the_separator_belongs_to_someone_else() {
        assert_eq!(lang_argument(arguments(&["--", "--lang=es"])), None);
        assert_eq!(lang_argument(arguments(&["list", "--all"])), None);
        assert_eq!(lang_argument(arguments(&[])), None);
    }

    /// Whatever the pre-scan finds is handed straight to `set_locale`, so a
    /// locale with no catalogue behind it must never leave here. The assertion
    /// holds for any argument list and any environment, including this test
    /// binary's own.
    #[test]
    fn the_prescan_never_yields_a_locale_that_is_not_compiled_in() {
        if let Some(locale) = prescan_locale() {
            assert!(available().contains(&locale), "{locale} has no catalogue");
        }
    }

    #[test]
    fn cycling_wraps_around_the_compiled_catalogues() {
        let locales = vec!["en".to_string(), "es".to_string()];

        assert_eq!(next_locale(&locales, "en").as_deref(), Some("es"));
        assert_eq!(next_locale(&locales, "es").as_deref(), Some("en"));
        // An active locale outside the list restarts it rather than panicking.
        assert_eq!(next_locale(&locales, "de").as_deref(), Some("es"));
        assert_eq!(next_locale(&[], "en"), None);
    }

    /// Reinstalling the active locale is a no-op by construction, but the
    /// active locale is process wide and the TUI tests switch it. Those hold
    /// the settings guard while they do, so this takes it too rather than
    /// racing them for a global.
    #[test]
    fn activating_the_active_locale_changes_nothing() {
        let (_guard, path) = crate::domain::settings::redirect_for_test("i18n-activate");

        let before = current();
        assert_eq!(activate(Some(&before)), before);
        assert_eq!(current(), before);

        let _ = std::fs::remove_file(&path);
    }

    fn arguments(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|value| (*value).to_string()).collect()
    }

    /// Guards against a key being added to one catalogue but not the others,
    /// which `rust-i18n` would silently paper over with the fallback locale.
    #[test]
    fn catalogues_declare_the_same_keys() {
        let reference = keys_of(FALLBACK);
        assert!(!reference.is_empty());

        for locale in available() {
            if locale == FALLBACK {
                continue;
            }
            let keys = keys_of(&locale);
            let missing: Vec<_> = reference.difference(&keys).collect();
            let extra: Vec<_> = keys.difference(&reference).collect();
            assert!(missing.is_empty(), "{locale} is missing {missing:?}");
            assert!(extra.is_empty(), "{locale} has unexpected {extra:?}");
        }
    }

    fn keys_of(locale: &str) -> BTreeSet<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("locales")
            .join(format!("{locale}.yml"));
        let raw = std::fs::read_to_string(path).expect("locale file");
        let root: serde_norway::Value = serde_norway::from_str(&raw).expect("valid YAML");

        let mut keys = BTreeSet::new();
        collect(&root, String::new(), &mut keys);
        keys.remove("_version");
        keys
    }

    fn collect(value: &serde_norway::Value, prefix: String, out: &mut BTreeSet<String>) {
        match value {
            serde_norway::Value::Mapping(mapping) => {
                for (key, child) in mapping {
                    let Some(key) = key.as_str() else { continue };
                    let path = if prefix.is_empty() {
                        key.to_string()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    collect(child, path, out);
                }
            }
            _ => {
                out.insert(prefix);
            }
        }
    }
}
