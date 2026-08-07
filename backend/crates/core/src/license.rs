//! Normalize GitHub license payloads into a short display / filter key.
//!
//! GitHub Search/REST often return `{ key: "other", spdx_id: "NOASSERTION" }`
//! for real license files that are multi-license, have SPDX exceptions, or are
//! non-catalogued. Treating those as "no license" blanks the leaderboard even
//! when a LICENSE file exists.

/// Prefer a real SPDX id, then key/name; map bare Other/NOASSERTION to `"Other"`.
///
/// Returns `None` only when GitHub reports no license object / all empty fields.
pub fn license_from_gh(
    spdx_id: Option<String>,
    key: Option<String>,
    name: Option<String>,
) -> Option<String> {
    let mut saw_other = false;
    for cand in [spdx_id, key, name] {
        if let Some(s) = cand {
            let t = s.trim();
            if t.is_empty() {
                continue;
            }
            if is_other_or_none(t) {
                saw_other = true;
                continue;
            }
            return Some(normalize_spdx_or_key(t));
        }
    }
    if saw_other {
        Some("Other".to_string())
    } else {
        None
    }
}

/// Pull a better label from LICENSE file text (e.g. SPDX header in linux COPYING).
///
/// Used when GitHub only classifies the file as Other / NOASSERTION.
pub fn license_from_file_content(content: &str) -> Option<String> {
    for line in content.lines().take(80) {
        let trimmed = line.trim();
        // Strip common comment prefixes before looking for SPDX.
        let body = trimmed
            .trim_start_matches("//")
            .trim_start_matches('#')
            .trim_start_matches('*')
            .trim_start_matches("/*")
            .trim();
        let lower = body.to_ascii_lowercase();
        if lower.starts_with("spdx-license-identifier:") {
            // Map back to original casing from `body` after the colon.
            let colon = body.find(':').unwrap_or(0);
            let id = body[colon + 1..].trim().trim_matches('"').trim();
            if id.is_empty() || is_other_or_none(id) {
                continue;
            }
            // Display primary id; drop WITH exceptions (e.g. Linux-syscall-note).
            let primary = id
                .split_once(" WITH ")
                .or_else(|| id.split_once(" with "))
                .map(|(p, _)| p.trim())
                .unwrap_or(id);
            if !primary.is_empty() && !is_other_or_none(primary) {
                return Some(normalize_spdx_or_key(primary));
            }
        }
    }

    // Lightweight phrase fallbacks for common non-catalogued files.
    let head: String = content.chars().take(800).collect();
    let head_l = head.to_ascii_lowercase();
    if head_l.contains("apache license") && head_l.contains("version 2.0") {
        return Some("Apache-2.0".into());
    }
    if head_l.contains("mit license") {
        return Some("MIT".into());
    }
    if head_l.contains("gnu general public license") {
        if head_l.contains("version 3") || head_l.contains("gpl-3.0") {
            return Some("GPL-3.0".into());
        }
        if head_l.contains("version 2") || head_l.contains("gpl-2.0") {
            return Some("GPL-2.0".into());
        }
    }
    if head_l.contains("creative commons attribution") && head_l.contains("4.0") {
        if head_l.contains("sharealike") || head_l.contains("share-alike") {
            return Some("CC-BY-SA-4.0".into());
        }
        return Some("CC-BY-4.0".into());
    }
    if head_l.contains("bsd 3-clause") || head_l.contains("bsd-3-clause") {
        return Some("BSD-3-Clause".into());
    }
    if head_l.contains("bsd 2-clause") || head_l.contains("bsd-2-clause") {
        return Some("BSD-2-Clause".into());
    }
    if head_l.contains("mozilla public license") && head_l.contains("2.0") {
        return Some("MPL-2.0".into());
    }
    if head_l.contains("unlicense") {
        return Some("Unlicense".into());
    }

    None
}

/// Combine GitHub license object with optional LICENSE body.
pub fn resolve_license(
    spdx_id: Option<String>,
    key: Option<String>,
    name: Option<String>,
    file_content: Option<&str>,
) -> Option<String> {
    let from_meta = license_from_gh(spdx_id, key, name);
    if let Some(content) = file_content {
        if let Some(refined) = license_from_file_content(content) {
            // Prefer content-derived SPDX when meta is missing or only Other.
            if from_meta.as_deref().is_none_or(|m| m == "Other") {
                return Some(refined);
            }
        }
    }
    from_meta
}

fn is_other_or_none(t: &str) -> bool {
    t.eq_ignore_ascii_case("NOASSERTION")
        || t.eq_ignore_ascii_case("other")
        || t.eq_ignore_ascii_case("none")
}

/// GitHub keys are often lowercase (`mit`); keep known SPDX casing when possible.
fn normalize_spdx_or_key(t: &str) -> String {
    // Already looks like SPDX (has digit or dash uppercase mix) — keep as-is.
    if t.contains('-') || t.chars().any(|c| c.is_ascii_uppercase()) {
        return t.to_string();
    }
    match t.to_ascii_lowercase().as_str() {
        "mit" => "MIT".into(),
        "isc" => "ISC".into(),
        "unlicense" => "Unlicense".into(),
        "apache-2.0" => "Apache-2.0".into(),
        "gpl-2.0" => "GPL-2.0".into(),
        "gpl-3.0" => "GPL-3.0".into(),
        "agpl-3.0" => "AGPL-3.0".into(),
        "lgpl-2.1" => "LGPL-2.1".into(),
        "lgpl-3.0" => "LGPL-3.0".into(),
        "bsd-2-clause" => "BSD-2-Clause".into(),
        "bsd-3-clause" => "BSD-3-Clause".into(),
        "mpl-2.0" => "MPL-2.0".into(),
        "cc0-1.0" => "CC0-1.0".into(),
        "cc-by-4.0" => "CC-BY-4.0".into(),
        "cc-by-sa-4.0" => "CC-BY-SA-4.0".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_spdx_over_key() {
        assert_eq!(
            license_from_gh(
                Some("MIT".into()),
                Some("mit".into()),
                Some("MIT License".into())
            ),
            Some("MIT".into())
        );
    }

    #[test]
    fn other_noassertion_becomes_other_not_none() {
        assert_eq!(
            license_from_gh(
                Some("NOASSERTION".into()),
                Some("other".into()),
                Some("Other".into())
            ),
            Some("Other".into())
        );
    }

    #[test]
    fn empty_fields_are_none() {
        assert_eq!(license_from_gh(None, None, None), None);
        assert_eq!(
            license_from_gh(Some("".into()), Some("  ".into()), None),
            None
        );
    }

    #[test]
    fn skips_none_token() {
        assert_eq!(
            license_from_gh(Some("none".into()), None, None),
            Some("Other".into())
        );
    }

    #[test]
    fn content_spdx_header() {
        let text = "The Linux Kernel is provided under:\n\n\tSPDX-License-Identifier: GPL-2.0 WITH Linux-syscall-note\n";
        assert_eq!(
            license_from_file_content(text),
            Some("GPL-2.0".into())
        );
    }

    #[test]
    fn content_cc_by() {
        let text = "Creative Commons Attribution 4.0 International License (CC BY 4.0)\n";
        assert_eq!(
            license_from_file_content(text),
            Some("CC-BY-4.0".into())
        );
    }

    #[test]
    fn resolve_prefers_content_when_meta_is_other() {
        let content = "SPDX-License-Identifier: GPL-2.0 WITH Linux-syscall-note\n";
        assert_eq!(
            resolve_license(
                Some("NOASSERTION".into()),
                Some("other".into()),
                Some("Other".into()),
                Some(content)
            ),
            Some("GPL-2.0".into())
        );
    }

    #[test]
    fn resolve_keeps_real_spdx_even_with_content() {
        let content = "MIT License\n";
        assert_eq!(
            resolve_license(
                Some("Apache-2.0".into()),
                Some("apache-2.0".into()),
                None,
                Some(content)
            ),
            Some("Apache-2.0".into())
        );
    }
}
