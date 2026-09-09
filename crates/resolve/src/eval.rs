use std::collections::{HashMap, HashSet};

use faa_ingest::Aircraft;

use crate::normalize::normalize_name;
use crate::{GoldFile, Mapping, RubricFile, RubricRow};

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

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RubricStratum {
    pub stratum: String,
    pub n: usize,
    pub skipped_missing_master: usize,
    pub true_positives: usize,
    pub false_negatives: usize,
    pub false_positives: usize,
    pub recall: Option<f64>,
    pub precision: Option<f64>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RubricReport {
    pub holdout_rows: usize,
    pub strata: Vec<RubricStratum>,
}

fn is_holdout_positive(row: &RubricRow) -> bool {
    row.split == "holdout" && row.stratum != "not_our_join" && !row.ticker.is_empty()
}

fn is_holdout_negative(row: &RubricRow) -> bool {
    row.split == "holdout" && row.stratum != "not_our_join" && !row.must_not_ticker.is_empty()
}

pub fn evaluate_rubric(
    rubric: &RubricFile,
    published: &[Mapping],
    aircraft: &[Aircraft],
) -> RubricReport {
    let by_n: HashMap<&str, &Mapping> =
        published.iter().map(|m| (m.n_number.as_str(), m)).collect();
    let master: HashSet<&str> = aircraft.iter().map(|a| a.n_number.as_str()).collect();

    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, (usize, usize, usize, usize, usize)> = HashMap::new();

    let mut holdout_rows = 0usize;
    for row in &rubric.rows {
        if row.split != "holdout" {
            continue;
        }
        holdout_rows += 1;
        if row.stratum == "not_our_join" {
            continue;
        }
        if !buckets.contains_key(&row.stratum) {
            order.push(row.stratum.clone());
            buckets.insert(row.stratum.clone(), (0, 0, 0, 0, 0));
        }
        let slot = buckets.get_mut(&row.stratum).expect("stratum");
        slot.0 += 1;
        if !master.contains(row.n_number.as_str()) {
            slot.1 += 1;
            continue;
        }
        if is_holdout_positive(row) {
            match by_n.get(row.n_number.as_str()) {
                Some(m) if m.ticker == row.ticker => slot.2 += 1,
                _ => slot.3 += 1,
            }
        } else if is_holdout_negative(row) {
            if let Some(m) = by_n.get(row.n_number.as_str()) {
                if m.ticker == row.must_not_ticker {
                    slot.4 += 1;
                }
            }
        }
    }

    let mut strata = Vec::new();
    for s in order {
        let (n, skip, tp, fn_, fp) = buckets[&s];
        let scored_pos = tp + fn_;
        let recall = if scored_pos == 0 {
            None
        } else {
            Some(tp as f64 / scored_pos as f64)
        };
        let prec_den = tp + fp;
        let precision = if prec_den == 0 {
            None
        } else {
            Some(tp as f64 / prec_den as f64)
        };
        strata.push(RubricStratum {
            stratum: s,
            n,
            skipped_missing_master: skip,
            true_positives: tp,
            false_negatives: fn_,
            false_positives: fp,
            recall,
            precision,
        });
    }
    RubricReport {
        holdout_rows,
        strata,
    }
}

impl std::fmt::Display for RubricReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "rubric holdout rows: {}", self.holdout_rows)?;
        if self.strata.is_empty() {
            writeln!(f, "(no scored holdout strata)")?;
            return Ok(());
        }
        for s in &self.strata {
            let rec = s
                .recall
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| "n/a".into());
            let prec = s
                .precision
                .map(|v| format!("{v:.3}"))
                .unwrap_or_else(|| "n/a".into());
            writeln!(
                f,
                "  {:<18} n={}  skip_master={}  tp={}  fn={}  fp={}  recall={}  precision={}",
                s.stratum,
                s.n,
                s.skipped_missing_master,
                s.true_positives,
                s.false_negatives,
                s.false_positives,
                rec,
                prec
            )?;
        }
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

    #[test]
    fn rubric_holdout_scores_positives_and_negatives() {
        use crate::RubricRow;
        let rubric = RubricFile {
            rows: vec![
                RubricRow {
                    n_number: "N3546".into(),
                    ticker: "NKE".into(),
                    must_not_ticker: String::new(),
                    stratum: "identity_easy".into(),
                    split: "holdout".into(),
                    citation: "test".into(),
                },
                RubricRow {
                    n_number: "N139FW".into(),
                    ticker: "CVX".into(),
                    must_not_ticker: String::new(),
                    stratum: "identity_hard".into(),
                    split: "holdout".into(),
                    citation: "test".into(),
                },
                RubricRow {
                    n_number: "N111HR".into(),
                    ticker: String::new(),
                    must_not_ticker: "H".into(),
                    stratum: "subsidiary_hold".into(),
                    split: "holdout".into(),
                    citation: "test".into(),
                },
                RubricRow {
                    n_number: "N100BD".into(),
                    ticker: String::new(),
                    must_not_ticker: String::new(),
                    stratum: "not_our_join".into(),
                    split: "holdout".into(),
                    citation: "test".into(),
                },
            ],
        };
        let published = vec![mapping("N3546", "NKE", "NIKE INC")];
        let aircraft = [
            jet("N3546", "NIKE INC"),
            jet("N139FW", "CHEVRON USA INC"),
            jet("N111HR", "INVERNESS LLC"),
            jet("N100BD", "TVPX TRUSTEE"),
        ];
        let r = evaluate_rubric(&rubric, &published, &aircraft);
        assert_eq!(r.holdout_rows, 4);
        let easy = r
            .strata
            .iter()
            .find(|s| s.stratum == "identity_easy")
            .unwrap();
        assert_eq!(easy.true_positives, 1);
        let hard = r
            .strata
            .iter()
            .find(|s| s.stratum == "identity_hard")
            .unwrap();
        assert_eq!(hard.false_negatives, 1);
        let hold = r
            .strata
            .iter()
            .find(|s| s.stratum == "subsidiary_hold")
            .unwrap();
        assert_eq!(hold.false_positives, 0);
        assert!(r.strata.iter().all(|s| s.stratum != "not_our_join"));
    }
}
