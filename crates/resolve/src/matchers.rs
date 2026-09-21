use std::collections::{HashMap, HashSet};

use faa_ingest::{canonical_n_number, is_corporate_aviation, Aircraft};
use sec_universe::{Company, Subsidiary};

use crate::classify::{classify_registrant, Class};
use crate::fcc::FccLicensee;
use crate::normalize::{
    ex21_indexable, name_match_corroborated, normalize_address, normalize_name,
};
use crate::{
    Conflict, EdgarHit, Mapping, OverrideEntry, Unresolved, ADDRESS_CLUSTER, EDGAR_NNUMBER,
    EX21_SUBSIDIARY, EXACT_LEGAL_NAME, FCC_LICENSEE_EXACT, MANUAL_OVERRIDE,
};

pub struct ResolveOutput {
    pub published: Vec<Mapping>,
    pub review_queue: Vec<Mapping>,
    pub unresolved: Vec<Unresolved>,
    pub conflicts: Vec<Conflict>,
    pub excluded_count: usize,
}

#[derive(Clone)]
struct NameHit {
    ticker: String,
    cik: String,
    company_name: String,
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_all(
    aircraft: &[Aircraft],
    companies: &[Company],
    subsidiaries: &[Subsidiary],
    edgar_hits: &[EdgarHit],
    overrides: &HashMap<String, OverrideEntry>,
    unpublished: &HashMap<String, String>,
    as_of: &str,
    faa_source: &str,
    publish_address_cluster: bool,
) -> ResolveOutput {
    resolve_all_fcc(
        aircraft,
        companies,
        subsidiaries,
        edgar_hits,
        overrides,
        unpublished,
        as_of,
        faa_source,
        publish_address_cluster,
        &[],
    )
}

/// Same as [`resolve_all`], plus FCC ULS licensee names for trustee pierce.
#[allow(clippy::too_many_arguments)]
pub fn resolve_all_fcc(
    aircraft: &[Aircraft],
    companies: &[Company],
    subsidiaries: &[Subsidiary],
    edgar_hits: &[EdgarHit],
    overrides: &HashMap<String, OverrideEntry>,
    unpublished: &HashMap<String, String>,
    as_of: &str,
    faa_source: &str,
    publish_address_cluster: bool,
    fcc_licensees: &[FccLicensee],
) -> ResolveOutput {
    let primary = sec_universe::primary_listings(companies);
    let companies = primary.as_slice();
    let by_cik: HashMap<String, &Company> = companies.iter().map(|c| (c.cik.clone(), c)).collect();

    let mut name_index: HashMap<String, Vec<NameHit>> = HashMap::new();
    let mut addr_index: HashMap<String, Vec<NameHit>> = HashMap::new();
    for c in companies {
        let hit = NameHit {
            ticker: c.ticker.clone(),
            cik: c.cik.clone(),
            company_name: c.name.clone(),
        };
        push_unique(&mut name_index, normalize_name(&c.name), hit.clone());
        for former in &c.former_names {
            push_unique(&mut name_index, normalize_name(former), hit.clone());
        }
        let addr = normalize_address(&c.street, &c.city, &c.state);
        if !addr.is_empty() {
            push_unique(&mut addr_index, addr, hit);
        }
    }

    let mut sub_index: HashMap<String, Vec<NameHit>> = HashMap::new();
    for s in subsidiaries {
        let Some(parent) = by_cik.get(&s.parent_cik) else {
            continue;
        };
        let hit = NameHit {
            ticker: parent.ticker.clone(),
            cik: parent.cik.clone(),
            company_name: parent.name.clone(),
        };
        let sub_key = normalize_name(&s.subsidiary_name);
        if ex21_indexable(&sub_key) {
            push_unique(&mut sub_index, sub_key, hit.clone());
        }
        // Parent HQ from Exhibit 21 fills gaps when submissions were not ingested.
        let addr = normalize_address(&s.parent_street, &s.parent_city, &s.parent_state);
        if !addr.is_empty() {
            push_unique(&mut addr_index, addr, hit);
        }
    }

    let master_n: HashSet<String> = aircraft.iter().map(|a| a.n_number.clone()).collect();
    let mut edgar_by_n: HashMap<String, Vec<&EdgarHit>> = HashMap::new();
    for h in edgar_hits {
        if !h.keyword_hit {
            continue;
        }
        let n = canonical_n_number(&h.n_number);
        if !master_n.contains(&n) {
            continue;
        }
        edgar_by_n.entry(n).or_default().push(h);
    }

    let mut fcc_by_n: HashMap<String, Vec<&FccLicensee>> = HashMap::new();
    for lic in fcc_licensees {
        if lic.n_number.is_empty() {
            continue;
        }
        fcc_by_n.entry(lic.n_number.clone()).or_default().push(lic);
    }

    let mut published = Vec::new();
    let mut review_queue = Vec::new();
    let mut unresolved = Vec::new();
    let mut conflicts = Vec::new();
    let mut excluded_count = 0usize;

    for ac in aircraft {
        if !is_corporate_aviation(ac) {
            continue;
        }
        let class =
            classify_registrant(&ac.registrant_name, ac.is_individual(), ac.fractional_owner);
        match class {
            Class::Fractional | Class::FaaFractionalFlag | Class::Airline | Class::Individual => {
                excluded_count += 1;
                continue;
            }
            Class::Trustee | Class::Eligible => {}
        }
        let is_trustee = class == Class::Trustee;

        if let Some(ov) = overrides.get(&ac.n_number) {
            let company_name = if ov.company_name.is_empty() {
                by_cik
                    .get(&sec_universe::pad_cik(&ov.cik))
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| ov.company_name.clone())
            } else {
                ov.company_name.clone()
            };
            let m = mapping(
                ac,
                &ov.ticker,
                &sec_universe::pad_cik(&ov.cik),
                &company_name,
                MANUAL_OVERRIDE,
                as_of,
                &ov.citation,
            );
            published.push(m);
            continue;
        }

        if let Some(hits) = edgar_by_n.get(&ac.n_number) {
            match unique_edgar(hits, &by_cik) {
                EdgarPick::One {
                    ticker,
                    cik,
                    company_name,
                    filing_url,
                } => {
                    let src = if filing_url.is_empty() {
                        faa_source
                    } else {
                        filing_url.as_str()
                    };
                    let m = mapping(ac, &ticker, &cik, &company_name, EDGAR_NNUMBER, as_of, src);
                    published.push(m);
                    continue;
                }
                EdgarPick::Many(tickers) => {
                    conflicts.push(Conflict {
                        n_number: ac.n_number.clone(),
                        registrant_name: ac.registrant_name.clone(),
                        tickers,
                        method: EDGAR_NNUMBER.into(),
                    });
                    continue;
                }
                EdgarPick::None => {}
            }
        }

        if is_trustee {
            match try_fcc_licensee(
                ac,
                fcc_by_n.get(&ac.n_number),
                &name_index,
                &sub_index,
                as_of,
            ) {
                FccPick::Published(m) => published.push(m),
                FccPick::Review(m) => review_queue.push(m),
                FccPick::Conflict(c) => conflicts.push(c),
                FccPick::None => unresolved.push(unres(ac, "trustee")),
            }
            continue;
        }

        let norm = normalize_name(&ac.registrant_name);
        if norm.is_empty() {
            unresolved.push(unres(ac, "empty_name"));
            continue;
        }

        match unique_hits(name_index.get(&norm)) {
            Unique::One(hit) => {
                let m = mapping(
                    ac,
                    &hit.ticker,
                    &hit.cik,
                    &hit.company_name,
                    EXACT_LEGAL_NAME,
                    as_of,
                    faa_source,
                );
                // Identity: unique listed name or alias. Corroboration is EX-21 only.
                published.push(m);
                continue;
            }
            Unique::Many(tickers) => {
                conflicts.push(Conflict {
                    n_number: ac.n_number.clone(),
                    registrant_name: ac.registrant_name.clone(),
                    tickers,
                    method: EXACT_LEGAL_NAME.into(),
                });
                continue;
            }
            Unique::None => {}
        }

        match unique_hits(sub_index.get(&norm)) {
            Unique::One(hit) => {
                let m = mapping(
                    ac,
                    &hit.ticker,
                    &hit.cik,
                    &hit.company_name,
                    EX21_SUBSIDIARY,
                    as_of,
                    "pudl:exhibit21",
                );
                hold_or_publish(m, &norm, &hit, &mut published, &mut review_queue);
                continue;
            }
            Unique::Many(tickers) => {
                conflicts.push(Conflict {
                    n_number: ac.n_number.clone(),
                    registrant_name: ac.registrant_name.clone(),
                    tickers,
                    method: EX21_SUBSIDIARY.into(),
                });
                continue;
            }
            Unique::None => {}
        }

        let addr = normalize_address(&ac.street, &ac.city, &ac.state);
        if !addr.is_empty() {
            match unique_hits(addr_index.get(&addr)) {
                Unique::One(hit) => {
                    let m = mapping(
                        ac,
                        &hit.ticker,
                        &hit.cik,
                        &hit.company_name,
                        ADDRESS_CLUSTER,
                        as_of,
                        faa_source,
                    );
                    if publish_address_cluster {
                        published.push(m);
                    } else {
                        review_queue.push(m);
                    }
                    continue;
                }
                Unique::Many(_) => {
                    // Ambiguous HQ cluster is not a publishable conflict; skip.
                }
                Unique::None => {}
            }
        }

        unresolved.push(unres(ac, "unmatched"));
    }

    apply_unpublished_gate(&mut published, &mut review_queue, unpublished);

    published.sort_by(|a, b| a.n_number.cmp(&b.n_number));
    review_queue.sort_by(|a, b| a.n_number.cmp(&b.n_number));
    stamp_fleet_size(&mut published);
    ResolveOutput {
        published,
        review_queue,
        unresolved,
        conflicts,
        excluded_count,
    }
}

enum Unique {
    None,
    One(NameHit),
    Many(Vec<String>),
}

enum FccPick {
    None,
    Published(Mapping),
    Review(Mapping),
    Conflict(Conflict),
}

enum EdgarPick {
    None,
    One {
        ticker: String,
        cik: String,
        company_name: String,
        filing_url: String,
    },
    Many(Vec<String>),
}

fn unique_hits(hits: Option<&Vec<NameHit>>) -> Unique {
    let Some(hits) = hits else {
        return Unique::None;
    };
    let mut tickers: Vec<String> = hits.iter().map(|h| h.ticker.clone()).collect();
    tickers.sort();
    tickers.dedup();
    match tickers.len() {
        0 => Unique::None,
        1 => Unique::One(hits[0].clone()),
        _ => Unique::Many(tickers),
    }
}

fn try_fcc_licensee(
    ac: &Aircraft,
    licenses: Option<&Vec<&FccLicensee>>,
    name_index: &HashMap<String, Vec<NameHit>>,
    sub_index: &HashMap<String, Vec<NameHit>>,
    as_of: &str,
) -> FccPick {
    let Some(licenses) = licenses else {
        return FccPick::None;
    };
    let usable: Vec<&FccLicensee> = licenses
        .iter()
        .copied()
        .filter(|l| classify_registrant(&l.licensee_name, false, false) != Class::Trustee)
        .collect();
    if usable.is_empty() {
        return FccPick::None;
    }

    let mut chosen: Option<(NameHit, bool, String)> = None;
    for lic in &usable {
        let norm = normalize_name(&lic.licensee_name);
        if norm.is_empty() {
            continue;
        }
        let (hit, ex21) = match unique_hits(name_index.get(&norm)) {
            Unique::One(h) => (h, false),
            Unique::Many(tickers) => {
                return FccPick::Conflict(Conflict {
                    n_number: ac.n_number.clone(),
                    registrant_name: ac.registrant_name.clone(),
                    tickers,
                    method: FCC_LICENSEE_EXACT.into(),
                });
            }
            Unique::None => match unique_hits(sub_index.get(&norm)) {
                Unique::One(h) => (h, true),
                Unique::Many(tickers) => {
                    return FccPick::Conflict(Conflict {
                        n_number: ac.n_number.clone(),
                        registrant_name: ac.registrant_name.clone(),
                        tickers,
                        method: FCC_LICENSEE_EXACT.into(),
                    });
                }
                Unique::None => continue,
            },
        };
        if let Some((prev, _, _)) = &chosen {
            if prev.ticker != hit.ticker {
                let mut tickers = vec![prev.ticker.clone(), hit.ticker.clone()];
                tickers.sort();
                tickers.dedup();
                return FccPick::Conflict(Conflict {
                    n_number: ac.n_number.clone(),
                    registrant_name: ac.registrant_name.clone(),
                    tickers,
                    method: FCC_LICENSEE_EXACT.into(),
                });
            }
        } else {
            chosen = Some((hit, ex21, lic.uls_id.clone()));
        }
    }

    let Some((hit, ex21, uls_id)) = chosen else {
        return FccPick::None;
    };
    let src = format!("fcc-uls-aircraft:{uls_id}");
    let m = mapping(
        ac,
        &hit.ticker,
        &hit.cik,
        &hit.company_name,
        FCC_LICENSEE_EXACT,
        as_of,
        &src,
    );
    if !ex21 {
        return FccPick::Published(m);
    }
    let lic_name = usable
        .iter()
        .find(|l| l.uls_id == uls_id)
        .map(|l| l.licensee_name.as_str())
        .unwrap_or("");
    if name_match_corroborated(&normalize_name(lic_name), &hit.company_name, &hit.ticker) {
        FccPick::Published(m)
    } else {
        FccPick::Review(m)
    }
}

fn unique_edgar(hits: &[&EdgarHit], by_cik: &HashMap<String, &Company>) -> EdgarPick {
    let mut resolved = Vec::new();
    for h in hits {
        let cik = sec_universe::pad_cik(&h.cik);
        let company = by_cik.get(&cik);
        let ticker = h
            .ticker
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| company.map(|c| c.ticker.clone()));
        let Some(ticker) = ticker else {
            continue;
        };
        resolved.push((
            ticker.to_uppercase(),
            cik,
            company.map(|c| c.name.clone()).unwrap_or_default(),
            h.filing_url.clone(),
        ));
    }
    let mut tickers: Vec<String> = resolved.iter().map(|r| r.0.clone()).collect();
    tickers.sort();
    tickers.dedup();
    match tickers.len() {
        0 => EdgarPick::None,
        1 => EdgarPick::One {
            ticker: resolved[0].0.clone(),
            cik: resolved[0].1.clone(),
            company_name: resolved[0].2.clone(),
            filing_url: resolved[0].3.clone(),
        },
        _ => EdgarPick::Many(tickers),
    }
}

fn push_unique(map: &mut HashMap<String, Vec<NameHit>>, key: String, hit: NameHit) {
    if key.is_empty() {
        return;
    }
    let e = map.entry(key).or_default();
    if e.iter().any(|h| h.ticker == hit.ticker && h.cik == hit.cik) {
        return;
    }
    e.push(hit);
}

fn mapping(
    ac: &Aircraft,
    ticker: &str,
    cik: &str,
    company_name: &str,
    method: &str,
    as_of: &str,
    source: &str,
) -> Mapping {
    Mapping {
        n_number: ac.n_number.clone(),
        icao24: ac.icao24.clone(),
        serial: ac.serial.clone(),
        make: ac.make.clone(),
        model: ac.model.clone(),
        ticker: ticker.to_uppercase(),
        cik: cik.to_string(),
        company_name: company_name.to_string(),
        registrant_name: ac.registrant_name.clone(),
        match_method: method.to_string(),
        as_of_date: as_of.to_string(),
        source_url: source.to_string(),
        fleet_size: 0,
        aviation_issuer: false,
    }
}

fn apply_unpublished_gate(
    published: &mut Vec<Mapping>,
    review: &mut Vec<Mapping>,
    unpublished: &HashMap<String, String>,
) {
    if unpublished.is_empty() {
        return;
    }
    let mut kept = Vec::with_capacity(published.len());
    for m in published.drain(..) {
        let forbidden = unpublished.get(&m.n_number).map(|t| t.to_ascii_uppercase());
        if forbidden.as_deref() == Some(m.ticker.as_str()) {
            review.push(m);
        } else {
            kept.push(m);
        }
    }
    *published = kept;
}

fn hold_or_publish(
    m: Mapping,
    norm: &str,
    hit: &NameHit,
    published: &mut Vec<Mapping>,
    review: &mut Vec<Mapping>,
) {
    if name_match_corroborated(norm, &hit.company_name, &hit.ticker) {
        published.push(m);
    } else {
        review.push(m);
    }
}

fn stamp_fleet_size(rows: &mut [Mapping]) {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for m in rows.iter() {
        *counts.entry(m.ticker.clone()).or_insert(0) += 1;
    }
    for m in rows.iter_mut() {
        m.fleet_size = counts.get(&m.ticker).copied().unwrap_or(0);
    }
}

fn unres(ac: &Aircraft, reason: &str) -> Unresolved {
    Unresolved {
        n_number: ac.n_number.clone(),
        icao24: ac.icao24.clone(),
        make: ac.make.clone(),
        model: ac.model.clone(),
        registrant_name: ac.registrant_name.clone(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faa_ingest::Aircraft;
    use sec_universe::Company;

    fn jet(n: &str, name: &str) -> Aircraft {
        Aircraft {
            n_number: n.into(),
            serial: "1".into(),
            type_registrant: "3".into(),
            registrant_name: name.into(),
            street: "702 SW 8TH ST".into(),
            city: "BENTONVILLE".into(),
            state: "AR".into(),
            type_aircraft: "5".into(),
            type_engine: "5".into(),
            status_code: "V".into(),
            fractional_owner: false,
            icao24: "abcdef".into(),
            make: "GULFSTREAM AEROSPACE".into(),
            model: "GVI".into(),
        }
    }

    fn wmt() -> Company {
        Company {
            cik: "0000104169".into(),
            ticker: "WMT".into(),
            name: "Walmart Inc.".into(),
            exchange: "NYSE".into(),
            former_names: vec!["WAL-MART STORES, INC.".into()],
            street: "702 SW 8TH STREET".into(),
            city: "Bentonville".into(),
            state: "AR".into(),
        }
    }

    #[test]
    fn exact_name_match() {
        let out = resolve_all(
            &[jet("N1WM", "WALMART INC")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert_eq!(out.published.len(), 1);
        assert_eq!(out.published[0].ticker, "WMT");
        assert_eq!(out.published[0].match_method, EXACT_LEGAL_NAME);
    }

    #[test]
    fn former_name_match() {
        let out = resolve_all(
            &[jet("N1WM", "WAL-MART STORES INC")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert_eq!(out.published[0].ticker, "WMT");
    }

    #[test]
    fn issuer_alias_identity_skips_parent_brand_corroboration() {
        let rtx = Company {
            cik: "0000101829".into(),
            ticker: "RTX".into(),
            name: "RTX Corp".into(),
            exchange: "NYSE".into(),
            former_names: vec!["RAYTHEON CO".into()],
            street: String::new(),
            city: String::new(),
            state: String::new(),
        };
        let out = resolve_all(
            &[jet("N289MT", "RAYTHEON CO")],
            &[rtx],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-09-08",
            "faa:test",
            false,
        );
        assert_eq!(out.published.len(), 1);
        assert_eq!(out.published[0].ticker, "RTX");
        assert_eq!(out.published[0].match_method, EXACT_LEGAL_NAME);
    }

    #[test]
    fn ex21_subsidiary_match() {
        let sub = Subsidiary {
            parent_cik: "0000104169".into(),
            parent_name: "Walmart Inc.".into(),
            subsidiary_name: "WALMART AVIATION LLC".into(),
            parent_street: String::new(),
            parent_city: String::new(),
            parent_state: String::new(),
        };
        let out = resolve_all(
            &[jet("N9WM", "WALMART AVIATION LLC")],
            &[wmt()],
            &[sub],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert_eq!(out.published[0].match_method, EX21_SUBSIDIARY);
        assert_eq!(out.published[0].ticker, "WMT");
        assert_eq!(out.published[0].fleet_size, 1);
    }

    fn azo() -> Company {
        Company {
            cik: "0000866787".into(),
            ticker: "AZO".into(),
            name: "AutoZone, Inc.".into(),
            exchange: "NYSE".into(),
            former_names: Vec::new(),
            street: String::new(),
            city: String::new(),
            state: String::new(),
        }
    }

    #[test]
    fn ex21_generic_tw_aviation_not_published() {
        let sub = Subsidiary {
            parent_cik: "0000866787".into(),
            parent_name: "AutoZone, Inc.".into(),
            subsidiary_name: "TW Aviation, Inc.".into(),
            parent_street: String::new(),
            parent_city: String::new(),
            parent_state: String::new(),
        };
        let out = resolve_all(
            &[jet("N450BT", "TW AVIATION LLC")],
            &[azo()],
            &[sub],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert!(out.published.is_empty());
        assert!(out
            .unresolved
            .iter()
            .any(|u| u.n_number == "N450BT" && u.reason == "unmatched"));
    }

    #[test]
    fn trustee_unresolved() {
        let out = resolve_all(
            &[jet("N50UT", "BANK OF UTAH TRUSTEE")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.unresolved[0].reason, "trustee");
    }

    fn fcc(n: &str, name: &str, uls: &str) -> crate::FccLicensee {
        crate::FccLicensee {
            n_number: n.into(),
            licensee_name: name.into(),
            uls_id: uls.into(),
        }
    }

    #[test]
    fn fcc_licensee_exact_publishes_trustee() {
        let licenses = [fcc("N425MP", "Marathon Petroleum Corporation", "uls-mpc")];
        let mpc = company("MPC", "Marathon Petroleum Corp", "0001510295");
        let out = resolve_all_fcc(
            &[jet("N425MP", "BANK OF UTAH TRUSTEE")],
            &[mpc],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-09-21",
            "faa:test",
            false,
            &licenses,
        );
        assert_eq!(out.published.len(), 1);
        assert_eq!(out.published[0].ticker, "MPC");
        assert_eq!(out.published[0].match_method, FCC_LICENSEE_EXACT);
        assert_eq!(out.published[0].registrant_name, "BANK OF UTAH TRUSTEE");
        assert_eq!(out.published[0].source_url, "fcc-uls-aircraft:uls-mpc");
        assert!(out.unresolved.is_empty());
    }

    #[test]
    fn fcc_trustee_on_both_sides_stays_unresolved() {
        let licenses = [fcc("N50UT", "WELLS FARGO TRUST COMPANY", "uls-wfc")];
        let out = resolve_all_fcc(
            &[jet("N50UT", "BANK OF UTAH TRUSTEE")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-09-21",
            "faa:test",
            false,
            &licenses,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.unresolved[0].reason, "trustee");
    }

    #[test]
    fn fcc_ex21_corroborated_publishes() {
        let licenses = [fcc("N903TF", "Tyson Shared Services, Inc.", "uls-tsn")];
        let tsn = company("TSN", "Tyson Foods, Inc.", "0000100493");
        let sub_row = sub(
            "0000100493",
            "Tyson Foods, Inc.",
            "Tyson Shared Services, Inc.",
        );
        let out = resolve_all_fcc(
            &[jet("N903TF", "TVPX AIRCRAFT SOLUTIONS INC TRUSTEE")],
            &[tsn],
            &[sub_row],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-09-21",
            "faa:test",
            false,
            &licenses,
        );
        assert_eq!(out.published.len(), 1);
        assert_eq!(out.published[0].ticker, "TSN");
        assert_eq!(out.published[0].match_method, FCC_LICENSEE_EXACT);
    }

    #[test]
    fn fcc_uncorroborated_ex21_stays_on_review() {
        let licenses = [fcc("N327ME", "REACH CO LLC", "uls-gci")];
        let gci = company("GCI", "Gannett Co., Inc.", "0001579684");
        let sub_row = sub("0001579684", "Gannett Co., Inc.", "REACH CO LLC");
        let out = resolve_all_fcc(
            &[jet("N327ME", "BANK OF UTAH TRUSTEE")],
            &[gci],
            &[sub_row],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-09-21",
            "faa:test",
            false,
            &licenses,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.review_queue.len(), 1);
        assert_eq!(out.review_queue[0].match_method, FCC_LICENSEE_EXACT);
        assert_eq!(out.review_queue[0].ticker, "GCI");
    }

    #[test]
    fn fcc_unpublished_gold_goes_to_review() {
        let licenses = [fcc("N881RC", "Cooper Companies, Inc.", "uls-coo")];
        let mut unpublished = HashMap::new();
        unpublished.insert("N881RC".into(), "COO".into());
        let out = resolve_all_fcc(
            &[jet("N881RC", "BANK OF UTAH TRUSTEE")],
            &[company("COO", "Cooper Companies, Inc.", "0000711404")],
            &[],
            &[],
            &HashMap::new(),
            &unpublished,
            "2026-09-21",
            "faa:test",
            false,
            &licenses,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.review_queue[0].ticker, "COO");
        assert_eq!(out.review_queue[0].match_method, FCC_LICENSEE_EXACT);
    }

    #[test]
    fn eligible_faa_path_ignores_fcc() {
        let licenses = [fcc("N1WM", "Some Other Inc", "uls-x")];
        let out = resolve_all_fcc(
            &[jet("N1WM", "WALMART INC")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
            &licenses,
        );
        assert_eq!(out.published[0].match_method, EXACT_LEGAL_NAME);
        assert_eq!(out.published[0].ticker, "WMT");
    }

    #[test]
    fn address_cluster_not_published_by_default() {
        let out = resolve_all(
            &[jet("N8XX", "SOME SPV LLC")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.review_queue.len(), 1);
        assert_eq!(out.review_queue[0].match_method, ADDRESS_CLUSTER);
    }

    #[test]
    fn edgar_hit_requires_master_row() {
        let hit = EdgarHit {
            cik: "0000104169".into(),
            ticker: Some("WMT".into()),
            accession: "0001".into(),
            form: "DEF 14A".into(),
            n_number: "N1WM".into(),
            snippet: "corporate aircraft N1WM".into(),
            filing_url: "https://www.sec.gov/x".into(),
            keyword_hit: true,
        };
        let out = resolve_all(
            &[jet("N1WM", "SOME TRUST")],
            &[wmt()],
            &[],
            &[hit],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        // "SOME TRUST" contains TRUSTEE? No - "TRUST" as suffix is not trustee unless TRUSTEE
        // classify: "SOME TRUST" - is_trustee checks TRUSTEE, OWNER TRUST, AIRCRAFT TRUST.
        // "SOME TRUST" does not match. Then edgar wins over name.
        assert_eq!(out.published[0].match_method, EDGAR_NNUMBER);
    }

    #[test]
    fn override_wins() {
        let mut ov = HashMap::new();
        ov.insert(
            "N1WM".into(),
            OverrideEntry {
                n_number: "N1WM".into(),
                ticker: "WMT".into(),
                cik: "0000104169".into(),
                company_name: "Walmart Inc.".into(),
                citation: "https://example.com".into(),
            },
        );
        let out = resolve_all(
            &[jet("N1WM", "RANDOM LLC")],
            &[wmt()],
            &[],
            &[],
            &ov,
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert_eq!(out.published[0].match_method, MANUAL_OVERRIDE);
    }

    #[test]
    fn airline_excluded() {
        let out = resolve_all(
            &[jet("N123AA", "AMERICAN AIRLINES INC")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            false,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.excluded_count, 1);
    }

    fn company(ticker: &str, name: &str, cik: &str) -> Company {
        Company {
            cik: cik.into(),
            ticker: ticker.into(),
            name: name.into(),
            exchange: "NYSE".into(),
            former_names: Vec::new(),
            street: String::new(),
            city: String::new(),
            state: String::new(),
        }
    }

    fn sub(parent_cik: &str, parent_name: &str, subsidiary: &str) -> Subsidiary {
        Subsidiary {
            parent_cik: parent_cik.into(),
            parent_name: parent_name.into(),
            subsidiary_name: subsidiary.into(),
            parent_street: String::new(),
            parent_city: String::new(),
            parent_state: String::new(),
        }
    }

    #[test]
    fn corroboration_gate_audit_cases() {
        struct Case {
            n: &'static str,
            faa: &'static str,
            ticker: &'static str,
            issuer: &'static str,
            cik: &'static str,
            ex21: Option<&'static str>,
            publish: bool,
        }
        let cases = [
            Case {
                n: "N327ME",
                faa: "REACH CO LLC",
                ticker: "GCI",
                issuer: "Gannett Co., Inc.",
                cik: "0001579684",
                ex21: Some("REACH CO LLC"),
                publish: false,
            },
            Case {
                n: "N16LJ",
                faa: "LEAR HOLDING CORP",
                ticker: "LEA",
                issuer: "Lear Corp",
                cik: "0000842162",
                ex21: None,
                publish: false,
            },
            Case {
                n: "N547JR",
                faa: "TIMBERLAND AVIATION LLC",
                ticker: "VFC",
                issuer: "V F Corp",
                cik: "0000103379",
                ex21: Some("TIMBERLAND AVIATION LLC"),
                publish: false,
            },
            Case {
                n: "N313WL",
                faa: "PEAK AVIATION LLC",
                ticker: "JEF",
                issuer: "Jefferies Financial Group Inc.",
                cik: "0000096223",
                ex21: Some("PEAK AVIATION LLC"),
                publish: false,
            },
            Case {
                n: "N9WM",
                faa: "WALMART AVIATION LLC",
                ticker: "WMT",
                issuer: "Walmart Inc.",
                cik: "0000104169",
                ex21: Some("WALMART AVIATION LLC"),
                publish: true,
            },
            Case {
                n: "N1972",
                faa: "NIKE INC",
                ticker: "NKE",
                issuer: "NIKE, Inc.",
                cik: "0000320187",
                ex21: None,
                publish: true,
            },
            Case {
                n: "N793FB",
                faa: "N793WF LEASE LLC",
                ticker: "CMG",
                issuer: "Chipotle Mexican Grill Inc",
                cik: "0001058090",
                ex21: Some("N793WF LEASE LLC"),
                publish: true,
            },
            Case {
                n: "N1AG",
                faa: "AGCO AVIATION LLC",
                ticker: "AGCO",
                issuer: "AGCO Corp",
                cik: "0000880266",
                ex21: Some("AGCO AVIATION LLC"),
                publish: true,
            },
        ];
        for c in cases {
            let issuer = company(c.ticker, c.issuer, c.cik);
            let subs = c
                .ex21
                .map(|s| sub(c.cik, c.issuer, s))
                .into_iter()
                .collect::<Vec<_>>();
            let out = resolve_all(
                &[jet(c.n, c.faa)],
                &[issuer],
                &subs,
                &[],
                &HashMap::new(),
                &HashMap::new(),
                "2026-08-31",
                "faa:test",
                false,
            );
            let published = out.published.iter().any(|m| m.n_number == c.n);
            assert_eq!(
                published, c.publish,
                "{} {} → {} publish={published} want={}",
                c.n, c.faa, c.ticker, c.publish
            );
            if !c.publish {
                assert!(out.published.is_empty(), "{} should not be published", c.n);
            } else {
                assert_eq!(out.published[0].ticker, c.ticker);
                assert!(
                    out.published[0].match_method == EXACT_LEGAL_NAME
                        || out.published[0].match_method == EX21_SUBSIDIARY
                );
            }
        }
    }

    #[test]
    fn uncorroborated_ex21_stays_on_review_queue() {
        let out = resolve_all(
            &[jet("N327ME", "REACH CO LLC")],
            &[company("GCI", "Gannett Co., Inc.", "0001579684")],
            &[sub("0001579684", "Gannett Co., Inc.", "REACH CO LLC")],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-31",
            "faa:test",
            false,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.review_queue.len(), 1);
        assert_eq!(out.review_queue[0].match_method, EX21_SUBSIDIARY);
        assert_eq!(out.review_queue[0].ticker, "GCI");
    }

    #[test]
    fn address_cluster_published_when_flagged() {
        let out = resolve_all(
            &[jet("N8XX", "SOME SPV LLC")],
            &[wmt()],
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-30",
            "faa:test",
            true,
        );
        assert_eq!(out.published.len(), 1);
        assert_eq!(out.published[0].match_method, ADDRESS_CLUSTER);
        assert!(out.review_queue.is_empty());
    }

    #[test]
    fn unpublished_gold_tail_goes_to_review() {
        let mut unpublished = HashMap::new();
        unpublished.insert("N881RC".into(), "COO".into());
        let out = resolve_all(
            &[jet("N881RC", "COOPER COMPANIES INC")],
            &[company("COO", "Cooper Companies, Inc.", "0000711404")],
            &[],
            &[],
            &HashMap::new(),
            &unpublished,
            "2026-08-31",
            "faa:test",
            false,
        );
        assert!(out.published.is_empty());
        assert_eq!(out.review_queue.len(), 1);
        assert_eq!(out.review_queue[0].ticker, "COO");
        assert_eq!(out.review_queue[0].n_number, "N881RC");
    }

    #[test]
    fn cik_with_preferred_emits_common_share() {
        let aub = company("AUB", "Atlantic Union Bankshares Corp", "0000883948");
        let mut aub_pa = aub.clone();
        aub_pa.ticker = "AUB-PA".into();
        let out = resolve_all(
            &[jet("N730AE", "ATLANTIC UNION BANK")],
            &[aub_pa, aub],
            &[sub(
                "0000883948",
                "Atlantic Union Bankshares Corp",
                "ATLANTIC UNION BANK",
            )],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            "2026-08-31",
            "faa:test",
            false,
        );
        assert_eq!(out.published.len(), 1);
        assert_eq!(out.published[0].ticker, "AUB");
        assert_eq!(out.published[0].match_method, EX21_SUBSIDIARY);
    }

    #[test]
    fn fleet_size_is_published_count() {
        let mut rows = vec![
            Mapping {
                n_number: "N1".into(),
                ticker: "WMT".into(),
                ..Mapping::default()
            },
            Mapping {
                n_number: "N2".into(),
                ticker: "WMT".into(),
                ..Mapping::default()
            },
            Mapping {
                n_number: "N3".into(),
                ticker: "NKE".into(),
                ..Mapping::default()
            },
        ];
        stamp_fleet_size(&mut rows);
        assert_eq!(rows[0].fleet_size, 2);
        assert_eq!(rows[1].fleet_size, 2);
        assert_eq!(rows[2].fleet_size, 1);
    }
}
