use std::collections::{HashMap, HashSet};

use faa_ingest::Aircraft;

use crate::normalize::normalize_name;
use crate::{GoldFile, Mapping};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct EvalReport {
    pub gold_companies: usize,
    pub gold_tails: usize,
    pub gold_unpublished_tails: usize,
    pub faa_gold_name_hits: usize,
    pub name_true_positives: usize,
    pub name_false_positives: usize,
    pub tail_true_positives: usize,
    pub tail_false_negatives: usize,
    pub tail_skipped_missing_master: usize,
    pub tail_false_positives: usize,
    pub unpublished_in_master: usize,
    pub unpublished_skipped_missing_master: usize,
    pub published_rows: usize,
    pub unresolved_trusts: usize,
    pub precision_name: f64,
    pub recall_name: f64,
    pub recall_tails: f64,
}

pub fn evaluate_gold(
    gold: &GoldFile,
    published: &[Mapping],
    aircraft: &[Aircraft],
    unresolved_trusts: usize,
) -> EvalReport {
    let by_n: HashMap<&str, &Mapping> =
        published.iter().map(|m| (m.n_number.as_str(), m)).collect();
    let master: HashSet<&str> = aircraft.iter().map(|a| a.n_number.as_str()).collect();

    let mut expected_norm: HashMap<String, String> = HashMap::new();
    for c in &gold.companies {
        for name in &c.registrant_names {
            expected_norm.insert(normalize_name(name), c.ticker.to_uppercase());
        }
        if !c.company_name.is_empty() {
            expected_norm.insert(normalize_name(&c.company_name), c.ticker.to_uppercase());
        }
    }

    let mut faa_gold_name_hits = 0usize;
    let mut name_tp = 0usize;
    let mut name_fp = 0usize;
    for ac in aircraft {
        let Some(exp) = expected_norm.get(&normalize_name(&ac.registrant_name)) else {
            continue;
        };
        faa_gold_name_hits += 1;
        match by_n.get(ac.n_number.as_str()) {
            Some(m) if m.ticker == *exp => name_tp += 1,
            Some(_) => name_fp += 1,
            None => {}
        }
    }

    let mut tail_tp = 0usize;
    let mut tail_fn = 0usize;
    let mut tail_skip = 0usize;
    for t in &gold.tails {
        let n = faa_ingest::canonical_n_number(&t.n_number);
        if !master.contains(n.as_str()) {
            tail_skip += 1;
            continue;
        }
        match by_n.get(n.as_str()) {
            Some(m) if m.ticker == t.ticker.to_uppercase() => tail_tp += 1,
            _ => tail_fn += 1,
        }
    }

    let mut tail_fp = 0usize;
    let mut unpublished_in_master = 0usize;
    let mut unpublished_skip = 0usize;
    for t in &gold.unpublished_tails {
        let n = faa_ingest::canonical_n_number(&t.n_number);
        if !master.contains(n.as_str()) {
            unpublished_skip += 1;
            continue;
        }
        unpublished_in_master += 1;
        let forbidden = t.must_not_ticker.to_uppercase();
        if let Some(m) = by_n.get(n.as_str()) {
            if m.ticker == forbidden {
                tail_fp += 1;
            }
        }
    }

    let name_denom_p = name_tp + name_fp;
    let name_denom_r = faa_gold_name_hits;
    let tail_denom = tail_tp + tail_fn;

    EvalReport {
        gold_companies: gold.companies.len(),
        gold_tails: gold.tails.len(),
        gold_unpublished_tails: gold.unpublished_tails.len(),
        faa_gold_name_hits,
        name_true_positives: name_tp,
        name_false_positives: name_fp,
        tail_true_positives: tail_tp,
        tail_false_negatives: tail_fn,
        tail_skipped_missing_master: tail_skip,
        tail_false_positives: tail_fp,
        unpublished_in_master,
        unpublished_skipped_missing_master: unpublished_skip,
        published_rows: published.len(),
        unresolved_trusts,
        precision_name: if name_denom_p == 0 {
            1.0
        } else {
            name_tp as f64 / name_denom_p as f64
        },
        recall_name: if name_denom_r == 0 {
            0.0
        } else {
            name_tp as f64 / name_denom_r as f64
        },
        recall_tails: if tail_denom == 0 {
            1.0
        } else {
            tail_tp as f64 / tail_denom as f64
        },
    }
}

impl std::fmt::Display for EvalReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "gold companies: {}", self.gold_companies)?;
        writeln!(f, "gold tails: {}", self.gold_tails)?;
        writeln!(f, "unpublished tails: {}", self.gold_unpublished_tails)?;
        writeln!(
            f,
            "FAA rows matching gold registrant names: {}",
            self.faa_gold_name_hits
        )?;
        writeln!(
            f,
            "name precision: {:.3}  ({}/{})",
            self.precision_name,
            self.name_true_positives,
            self.name_true_positives + self.name_false_positives
        )?;
        writeln!(
            f,
            "name recall: {:.3}  ({}/{})",
            self.recall_name, self.name_true_positives, self.faa_gold_name_hits
        )?;
        writeln!(
            f,
            "tail recall (present in MASTER): {:.3}  ({}/{}; skipped missing {})",
            self.recall_tails,
            self.tail_true_positives,
            self.tail_true_positives + self.tail_false_negatives,
            self.tail_skipped_missing_master
        )?;
        writeln!(
            f,
            "tail false positives (published as must_not_ticker): {}  ({}/{} present in MASTER; skipped missing {})",
            self.tail_false_positives,
            self.unpublished_in_master,
            self.gold_unpublished_tails,
            self.unpublished_skipped_missing_master
        )?;
        writeln!(f, "published rows: {}", self.published_rows)?;
        writeln!(f, "unresolved trusts: {}", self.unresolved_trusts)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GoldCompany, OverrideEntry, UnpublishedTail};
    use faa_ingest::Aircraft;

    fn jet(n: &str, name: &str) -> Aircraft {
        Aircraft {
            n_number: n.into(),
            serial: String::new(),
            type_registrant: "3".into(),
            registrant_name: name.into(),
            street: String::new(),
            city: String::new(),
            state: String::new(),
            type_aircraft: "5".into(),
            type_engine: "5".into(),
            status_code: "V".into(),
            fractional_owner: false,
            icao24: String::new(),
            make: "GULFSTREAM".into(),
            model: "G650".into(),
        }
    }

    fn mapping(n: &str, ticker: &str, name: &str) -> Mapping {
        Mapping {
            n_number: n.into(),
            icao24: String::new(),
            serial: String::new(),
            make: String::new(),
            model: String::new(),
            ticker: ticker.into(),
            cik: String::new(),
            company_name: String::new(),
            registrant_name: name.into(),
            match_method: "exact_legal_name".into(),
            as_of_date: String::new(),
            source_url: String::new(),
            fleet_size: 1,
            aviation_issuer: false,
        }
    }

    fn gold_with_unpublished() -> GoldFile {
        GoldFile {
            companies: vec![GoldCompany {
                ticker: "WMT".into(),
                cik: "0000104169".into(),
                company_name: "Walmart Inc.".into(),
                registrant_names: vec!["WALMART INC".into()],
            }],
            tails: vec![OverrideEntry {
                n_number: "N1WM".into(),
                ticker: "WMT".into(),
                cik: String::new(),
                company_name: String::new(),
                citation: String::new(),
            }],
            unpublished_tails: vec![UnpublishedTail {
                n_number: "N327ME".into(),
                must_not_ticker: "GCI".into(),
                citation: "REACH namesake".into(),
            }],
        }
    }

    #[test]
    fn reports_perfect_name_match() {
        let gold = gold_with_unpublished();
        let published = vec![mapping("N1WM", "WMT", "WALMART INC")];
        let r = evaluate_gold(&gold, &published, &[jet("N1WM", "WALMART INC")], 0);
        assert_eq!(r.name_true_positives, 1);
        assert_eq!(r.precision_name, 1.0);
        assert_eq!(r.recall_tails, 1.0);
        assert_eq!(r.tail_false_positives, 0);
    }

    #[test]
    fn unpublished_published_as_forbidden_is_tail_fp() {
        let gold = gold_with_unpublished();
        let published = vec![mapping("N327ME", "GCI", "REACH CO LLC")];
        let r = evaluate_gold(
            &gold,
            &published,
            &[jet("N1WM", "WALMART INC"), jet("N327ME", "REACH CO LLC")],
            0,
        );
        assert_eq!(r.tail_false_positives, 1);
        assert_eq!(r.unpublished_in_master, 1);
    }

    #[test]
    fn unpublished_on_review_or_unmatched_is_ok() {
        let gold = gold_with_unpublished();
        let r = evaluate_gold(&gold, &[], &[jet("N327ME", "REACH CO LLC")], 0);
        assert_eq!(r.tail_false_positives, 0);
        assert_eq!(r.unpublished_in_master, 1);
    }

    #[test]
    fn unpublished_other_ticker_is_not_this_fp() {
        let gold = gold_with_unpublished();
        let published = vec![mapping("N327ME", "OTHER", "REACH CO LLC")];
        let r = evaluate_gold(&gold, &published, &[jet("N327ME", "REACH CO LLC")], 0);
        assert_eq!(r.tail_false_positives, 0);
    }

    #[test]
    fn unpublished_missing_master_is_skipped() {
        let gold = gold_with_unpublished();
        let r = evaluate_gold(&gold, &[mapping("N327ME", "GCI", "REACH")], &[], 0);
        assert_eq!(r.unpublished_skipped_missing_master, 1);
        assert_eq!(r.tail_false_positives, 0);
    }
}
