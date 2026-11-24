//! Explicit, byte-only import of compatible legacy preferences.
//!
//! A digest is recorded only when at least one key maps successfully (including
//! `searchMode = exact`). Unsupported-only, invalid-only, and protected-only
//! imports are not marked as imported. A partially valid list appears in both
//! imported_keys and invalid_keys; invalid_entries counts rejected list entries,
//! not malformed scalar fields. Reports contain key names, never source values.

use crate::models::SUPPORTED_TYPES;
use crate::settings::Settings;
use crate::{CoreError, Result};
use plist::Value;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportResult {
    pub settings: Settings,
    pub report: ImportReport,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    pub imported_keys: Vec<String>,
    pub skipped_keys: Vec<String>,
    pub invalid_keys: Vec<String>,
    pub invalid_entries: usize,
    pub already_imported: bool,
}

/// Build one validated settings value; the caller owns confirmation and the
/// atomic Store::update_settings call. This function cannot open an old store.
pub fn import_settings(bytes: &[u8], current: &Settings, overwrite: bool) -> Result<ImportResult> {
    if bytes.len() > MAX_BYTES {
        return Err(CoreError::InvalidInput);
    }
    let xml = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let xml = xml.trim_ascii_start();
    if !bytes.starts_with(b"bplist00") && !xml.starts_with(b"<") {
        return Err(CoreError::InvalidInput);
    }
    let Value::Dictionary(values) =
        Value::from_reader(Cursor::new(bytes)).map_err(|_| CoreError::InvalidInput)?
    else {
        return Err(CoreError::InvalidInput);
    };
    if values.keys().any(|key| !valid_string(key, 255)) {
        return Err(CoreError::InvalidInput);
    }
    current.validate()?;
    let digest = blake3::hash(bytes).to_hex().to_string();
    let mut settings = current.clone();
    let mut report = ImportReport::default();
    if !overwrite && current.import_digest.as_ref() == Some(&digest) {
        report.already_imported = true;
        report.skipped_keys.extend(values.keys().cloned());
        report.skipped_keys.sort();
        return Ok(ImportResult { settings, report });
    }
    let defaults = Settings::default();
    let mut apps_imported = false;
    let apps_protected = !overwrite && current.ignored_apps != defaults.ignored_apps;

    // Apps are resolved before allowlist mode, independent of plist key order.
    if let Some(value) = values.get("ignoredApps") {
        if apps_protected {
            report.skipped_keys.push("ignoredApps".into());
        } else if let Some(apps) = import_list(
            "ignoredApps",
            value,
            256,
            |s| {
                valid_string(s, 255)
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    && s.split('.').all(|part| !part.is_empty())
            },
            &mut report,
        ) {
            settings.ignored_apps = apps;
            apps_imported = true;
        }
    }

    for (key, value) in &values {
        let protected = !overwrite
            && match key.as_str() {
                "enabledPasteboardTypes" => current.enabled_types != defaults.enabled_types,
                "ignoreAllAppsExceptListed" => current.allowlist_apps != defaults.allowlist_apps,
                "ignoreRegexp" => current.ignored_patterns != defaults.ignored_patterns,
                "ignoredPasteboardTypes" => current.ignored_types != defaults.ignored_types,
                "clipboardCheckInterval" => current.poll_interval_ms != defaults.poll_interval_ms,
                _ => false,
            };
        if protected {
            report.skipped_keys.push(key.clone());
            continue;
        }
        match key.as_str() {
            "ignoredApps" => {}
            "enabledPasteboardTypes" => {
                if let Some(types) = import_list(
                    key,
                    value,
                    SUPPORTED_TYPES.len(),
                    |s| valid_string(s, 255) && SUPPORTED_TYPES.contains(&s),
                    &mut report,
                ) {
                    settings.enabled_types = types;
                }
            }
            "ignoredPasteboardTypes" => {
                if let Some(types) =
                    import_list(key, value, 128, |s| valid_string(s, 255), &mut report)
                {
                    // Mandatory markers remain enforced by Settings::ignores.
                    settings.ignored_types = types;
                }
            }
            "ignoreRegexp" => {
                if let Some(patterns) = import_list(
                    key,
                    value,
                    32,
                    |s| valid_string(s, 512) && Settings::compile_pattern(s).is_ok(),
                    &mut report,
                ) {
                    settings.ignored_patterns = patterns;
                }
            }
            "ignoreAllAppsExceptListed" => match value.as_boolean() {
                Some(true) if values.contains_key("ignoredApps") && apps_protected => {
                    report.skipped_keys.push(key.clone());
                }
                Some(true)
                    if settings.ignored_apps.is_empty()
                        || (values.contains_key("ignoredApps") && !apps_imported) =>
                {
                    report.invalid_keys.push(key.clone());
                }
                Some(mode) => {
                    settings.allowlist_apps = mode;
                    report.imported_keys.push(key.clone());
                }
                None => report.invalid_keys.push(key.clone()),
            },
            "clipboardCheckInterval" => {
                let seconds = match value {
                    Value::Real(number) => Some(*number),
                    Value::Integer(number) => number
                        .as_signed()
                        .map(|v| v as f64)
                        .or_else(|| number.as_unsigned().map(|v| v as f64)),
                    _ => None,
                };
                if let Some(seconds) = seconds.filter(|v| v.is_finite() && *v >= 0.0) {
                    settings.poll_interval_ms = (seconds.clamp(0.1, 5.0) * 1000.0).round() as u64;
                    report.imported_keys.push(key.clone());
                } else {
                    report.invalid_keys.push(key.clone());
                }
            }
            "searchMode" => match value.as_string() {
                Some("exact") => report.imported_keys.push(key.clone()),
                Some(mode) if valid_string(mode, 255) => report.skipped_keys.push(key.clone()),
                _ => report.invalid_keys.push(key.clone()),
            },
            _ => report.skipped_keys.push(key.clone()),
        }
    }
    if !report.imported_keys.is_empty() {
        settings.import_digest = Some(digest);
    }
    settings.validate()?;
    report.imported_keys.sort();
    report.skipped_keys.sort();
    report.invalid_keys.sort();
    Ok(ImportResult { settings, report })
}

fn valid_string(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control)
}

fn import_list(
    key: &str,
    value: &Value,
    limit: usize,
    valid: impl Fn(&str) -> bool,
    report: &mut ImportReport,
) -> Option<Vec<String>> {
    let Some(entries) = value.as_array() else {
        report.invalid_keys.push(key.into());
        return None;
    };
    let mut accepted = Vec::new();
    let mut invalid = 0;
    for (index, entry) in entries.iter().enumerate() {
        if let Some(text) = entry.as_string().filter(|s| index < limit && valid(s)) {
            if !accepted.iter().any(|s| s == text) {
                accepted.push(text.to_owned());
            }
        } else {
            invalid += 1;
        }
    }
    if invalid != 0 {
        report.invalid_keys.push(key.into());
        report.invalid_entries += invalid;
    }
    // An invalid nonempty list must not silently clear existing exclusions.
    if !entries.is_empty() && accepted.is_empty() {
        return None;
    }
    report.imported_keys.push(key.into());
    Some(accepted)
}
