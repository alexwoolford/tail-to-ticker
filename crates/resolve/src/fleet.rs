//! Post-resolve annotations: fleet size per ticker.

use std::collections::HashMap;

use crate::Mapping;

/// Stamp `fleet_size` on each published row (count of published tails for that ticker).
pub fn annotate_fleet(rows: &mut [Mapping]) {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for m in rows.iter() {
        *counts.entry(m.ticker.clone()).or_insert(0) += 1;
    }
    for m in rows.iter_mut() {
        m.fleet_size = counts.get(&m.ticker).copied().unwrap_or(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(n: &str, ticker: &str) -> Mapping {
        Mapping {
            n_number: n.into(),
            ticker: ticker.into(),
            ..Mapping::default()
        }
    }

    #[test]
    fn fleet_size_is_published_count() {
        let mut rows = vec![row("N1", "WMT"), row("N2", "WMT"), row("N3", "NKE")];
        annotate_fleet(&mut rows);
        assert_eq!(rows[0].fleet_size, 2);
        assert_eq!(rows[1].fleet_size, 2);
        assert_eq!(rows[2].fleet_size, 1);
    }
}
