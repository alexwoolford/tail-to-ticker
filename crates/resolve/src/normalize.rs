//! Name and address normalization for conservative exact matching.

use std::collections::HashSet;

/// Legal suffixes that do not change which entity a name refers to.
const SUFFIXES: &[&str] = &[
    "INCORPORATED",
    "CORPORATION",
    "COMPANY",
    "LIMITED",
    "TRUSTEE",
    "TRUST",
    "LLC",
    "L L C",
    "LLP",
    "LP",
    "PLC",
    "PC",
    "PA",
    "NA",
    "N A",
    "INC",
    "CORP",
    "CO",
    "LTD",
    "THE",
];

/// Uppercase, strip punctuation, collapse legal suffixes, canonicalize AND.
///
/// HoldCo / Group / Partners tokens are kept so Lear Holding Corp does not
/// collapse to the same key as Lear Corp.
pub fn normalize_name(raw: &str) -> String {
    let mut s = String::with_capacity(raw.len());
    let upper = raw.to_uppercase().replace(['&', '+'], " AND ");
    let mut prev_space = false;
    for ch in upper.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch);
            prev_space = false;
        } else if !prev_space {
            s.push(' ');
            prev_space = true;
        }
    }
    let mut s = s.trim().to_string();
    loop {
        let before = s.clone();
        if let Some(rest) = s.strip_prefix("THE ") {
            s = rest.to_string();
        }
        for suf in SUFFIXES {
            let pad = format!(" {suf}");
            if s.ends_with(&pad) {
                s = s[..s.len() - pad.len()].trim_end().to_string();
            }
            if s == *suf {
                s.clear();
            }
        }
        if s == before {
            break;
        }
    }
    s
}

/// Tokens stripped when judging whether an Exhibit 21 name is distinctive enough to index.
const EX21_DROP_TOKENS: &[&str] = &[
    "AVIATION", "AIRCRAFT", "AIRPLANE", "AIR", "AERO", "LEASING", "LEASE", "SERVICES", "SERVICE",
    "SALES",
];

const EX21_GENERIC_WORDS: &[&str] = &["WINGS", "STAR", "CAPITAL", "MANAGEMENT", "INTERNATIONAL"];

/// Identity suffixes kept in match keys; ignored when looking for a shared brand token.
const IDENTITY_SUFFIXES: &[&str] = &["HOLDING", "HOLDINGS", "GROUP", "PARTNERS", "PARTNERSHIP"];

/// Remaining brand-like core after dropping generic aviation tokens from a normalized name.
fn distinctive_core(normalized: &str) -> String {
    normalized
        .split_whitespace()
        .filter(|t| !EX21_DROP_TOKENS.contains(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when an Exhibit 21 subsidiary name is specific enough to use as a match key.
///
/// `TW AVIATION` collapses to `TW` (too short). `WALMART AVIATION` keeps `WALMART`.
pub fn ex21_indexable(normalized: &str) -> bool {
    let core = distinctive_core(normalized);
    let alnum: String = core.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    alnum.len() >= 4
}

fn looks_like_n_number_token(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.first() != Some(&b'N') {
        return false;
    }
    let rest = &token[1..];
    if rest.len() < 2 || rest.len() > 5 {
        return false;
    }
    rest.chars().all(|c| c.is_ascii_alphanumeric()) && rest.chars().any(|c| c.is_ascii_digit())
}

fn overlap_token_ok(token: &str) -> bool {
    token.len() >= 4 && !IDENTITY_SUFFIXES.contains(&token) && !EX21_GENERIC_WORDS.contains(&token)
}

fn alnum_only(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Parent brand spelled as consecutive FAA tokens (WAL + MART = WALMART).
/// Leftover identity suffixes mean HoldCo vs OpCo, not a former-name spelling.
fn consecutive_tokens_match_parent(tokens: &[&str], parent_alnum: &str) -> bool {
    if parent_alnum.len() < 4 {
        return false;
    }
    for i in 0..tokens.len() {
        let mut acc = String::new();
        for j in i..tokens.len() {
            acc.push_str(tokens[j]);
            if acc == parent_alnum {
                let leftover_ok = tokens
                    .iter()
                    .enumerate()
                    .all(|(k, tok)| (k >= i && k <= j) || !IDENTITY_SUFFIXES.contains(tok));
                if leftover_ok {
                    return true;
                }
            }
            if acc.len() > parent_alnum.len() {
                break;
            }
        }
    }
    false
}

fn faa_has_extra_identity_suffix(faa_tokens: &[&str], parent_tokens: &HashSet<&str>) -> bool {
    faa_tokens
        .iter()
        .any(|t| IDENTITY_SUFFIXES.contains(t) && !parent_tokens.contains(t))
}

/// Whether an exact-name or Exhibit 21 hit is strong enough to publish.
///
/// Uncorroborated hits go to the review queue. Publish if the distinctive core
/// equals the ticker, a token looks like an N-number SPV, or a brand token
/// (length ≥ 4) is shared with the parent name.
pub fn name_match_corroborated(norm_key: &str, parent_name: &str, ticker: &str) -> bool {
    let core = distinctive_core(norm_key);
    if core.is_empty() {
        return false;
    }
    let ticker = ticker.trim().to_uppercase();
    let ticker_alnum = alnum_only(&ticker);
    let core_alnum = alnum_only(&core);
    if !ticker.is_empty()
        && (core == ticker || (!ticker_alnum.is_empty() && core_alnum == ticker_alnum))
    {
        return true;
    }
    let tokens: Vec<&str> = core.split_whitespace().collect();
    if tokens
        .iter()
        .any(|t| (!ticker.is_empty() && *t == ticker.as_str()) || looks_like_n_number_token(t))
    {
        return true;
    }
    let parent_core = distinctive_core(&normalize_name(parent_name));
    let parent_alnum = alnum_only(&parent_core);
    let parent_token_set: HashSet<&str> = parent_core.split_whitespace().collect();
    if faa_has_extra_identity_suffix(&tokens, &parent_token_set) {
        return false;
    }
    if consecutive_tokens_match_parent(&tokens, &parent_alnum) {
        return true;
    }
    if core_alnum.len() >= 4 && parent_alnum.contains(&core_alnum) {
        return true;
    }
    let parent_overlap: HashSet<&str> = parent_core
        .split_whitespace()
        .filter(|t| overlap_token_ok(t))
        .collect();
    tokens
        .iter()
        .any(|t| overlap_token_ok(t) && parent_overlap.contains(t))
}

/// Street + city + state, digits kept, street suffixes lightly collapsed.
pub fn normalize_address(street: &str, city: &str, state: &str) -> String {
    let street = normalize_street(street);
    let city = normalize_name(city);
    let state = state.trim().to_uppercase();
    if street.is_empty() || city.is_empty() {
        return String::new();
    }
    format!("{street}|{city}|{state}")
}

fn normalize_street(raw: &str) -> String {
    let mut s = normalize_name(raw);
    for (from, to) in [
        (" STREET", " ST"),
        (" AVENUE", " AVE"),
        (" BOULEVARD", " BLVD"),
        (" DRIVE", " DR"),
        (" ROAD", " RD"),
        (" LANE", " LN"),
        (" PARKWAY", " PKWY"),
        (" SUITE", " "),
        (" STE", " "),
        (" FLOOR", " "),
        (" FL", " "),
    ] {
        s = s.replace(from, to);
    }
    collapse_spaces(&s)
}

fn collapse_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_suffixes_collapse() {
        assert_eq!(normalize_name("Walmart Inc."), "WALMART");
        assert_eq!(normalize_name("WAL-MART STORES, INC."), "WAL MART STORES");
        assert_eq!(normalize_name("The Home Depot, Inc."), "HOME DEPOT");
        assert_eq!(normalize_name("Johnson & Johnson"), "JOHNSON AND JOHNSON");
        assert_eq!(normalize_name("NIKE INC"), "NIKE");
    }

    #[test]
    fn identity_suffixes_are_kept() {
        assert_eq!(normalize_name("LEAR HOLDING CORP"), "LEAR HOLDING");
        assert_eq!(normalize_name("Lear Corp"), "LEAR");
        assert_eq!(normalize_name("ALSET HOLDING CO LLC"), "ALSET HOLDING");
        assert_eq!(normalize_name("Alset Inc."), "ALSET");
        assert_eq!(normalize_name("VEHICLE HOLDINGS INC"), "VEHICLE HOLDINGS");
        assert_eq!(normalize_name("Herc Holdings Inc"), "HERC HOLDINGS");
    }

    #[test]
    fn address_collapses_street_suffix() {
        assert_eq!(
            normalize_address("702 SW 8th Street", "Bentonville", "AR"),
            "702 SW 8TH ST|BENTONVILLE|AR"
        );
    }

    #[test]
    fn ex21_indexable_keeps_brand_aliases() {
        assert!(ex21_indexable(&normalize_name("WALMART AVIATION LLC")));
        assert!(ex21_indexable(&normalize_name("XTO ENERGY INC")));
        assert!(ex21_indexable(&normalize_name("CHEVRON U S A INC")));
        assert!(ex21_indexable(&normalize_name("HOMERLEASE CO INC")));
    }

    #[test]
    fn ex21_indexable_rejects_short_cores() {
        assert!(!ex21_indexable(&normalize_name("TW Aviation, Inc.")));
        assert!(!ex21_indexable(&normalize_name("TW AVIATION LLC")));
        assert!(!ex21_indexable(&normalize_name("KA LLC")));
        assert!(!ex21_indexable(&normalize_name("AVIATION LLC")));
        // WINGS is long enough to index; corroboration holds it unless the parent shares the token.
        assert!(ex21_indexable(&normalize_name("WINGS LLC")));
    }

    #[test]
    fn corroboration_ticker_parent_and_nnumber_spv() {
        assert!(name_match_corroborated("AGCO", "AGCO Corp", "AGCO"));
        assert!(name_match_corroborated(
            "WALMART AVIATION",
            "Walmart Inc.",
            "WMT"
        ));
        assert!(name_match_corroborated("NIKE", "NIKE, Inc.", "NKE"));
        assert!(name_match_corroborated(
            "N793WF LEASE",
            "Chipotle Mexican Grill Inc",
            "CMG"
        ));
        assert!(name_match_corroborated(
            "DUKE ENERGY BUSINESS SERVICES",
            "Duke Energy Corp",
            "DUKH"
        ));
        assert!(name_match_corroborated(
            "WAL MART STORES",
            "Walmart Inc.",
            "WMT"
        ));
    }

    #[test]
    fn corroboration_rejects_audit_namesakes() {
        assert!(!name_match_corroborated(
            "REACH",
            "Gannett Co., Inc.",
            "GCI"
        ));
        assert!(!name_match_corroborated(
            "TIMBERLAND AVIATION",
            "V F Corp",
            "VFC"
        ));
        assert!(!name_match_corroborated(
            "PEAK AVIATION",
            "Jefferies Financial Group Inc.",
            "JEF"
        ));
        assert!(!name_match_corroborated("LEAR HOLDING", "Lear Corp", "LEA"));
        assert!(!name_match_corroborated(
            "VEHICLE HOLDINGS",
            "Herc Holdings Inc",
            "HRI"
        ));
        assert!(!name_match_corroborated("NIKE", "Gannett Co., Inc.", "GCI"));
    }
}
