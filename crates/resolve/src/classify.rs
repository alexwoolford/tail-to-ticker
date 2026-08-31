//! Registrant denylists: trustees, fractionals, scheduled airlines.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Eligible,
    Trustee,
    Fractional,
    Airline,
    Individual,
    FaaFractionalFlag,
}

pub fn classify_registrant(name: &str, is_individual: bool, faa_fract_owner: bool) -> Class {
    if is_individual {
        return Class::Individual;
    }
    if faa_fract_owner {
        return Class::FaaFractionalFlag;
    }
    let n = name.to_uppercase();
    if is_trustee(&n) {
        return Class::Trustee;
    }
    if is_fractional(&n) {
        return Class::Fractional;
    }
    if is_airline(&n) {
        return Class::Airline;
    }
    Class::Eligible
}

fn is_trustee(n: &str) -> bool {
    if n.contains("TRUSTEE") || n.contains("OWNER TRUST") || n.contains("AIRCRAFT TRUST") {
        return true;
    }
    const NEEDLES: &[&str] = &[
        "BANK OF UTAH",
        "WILMINGTON TRUST",
        "TVPX",
        "WELLS FARGO BANK NORTHWEST",
        "WELLS FARGO TRUST",
        "US BANK TRUST",
        "U.S. BANK TRUST",
        "UMB BANK",
        "BANK OF OKLAHOMA",
        "BOKF",
        "FIRST SECURITY BANK",
    ];
    NEEDLES.iter().any(|k| n.contains(k))
}

fn is_fractional(n: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "NETJETS",
        "FLEXJET",
        "VISTAJET",
        "VISTA JET",
        "WHEELS UP",
        "PLANESENSE",
        "PLANE SENSE",
        "FLIGHT OPTIONS",
        "CITATIONSHARES",
        "CITATION SHARES",
        "DIRECTIONAL AVIATION",
        "NICHOLAS AIR",
        "JET LINX",
        "JETLINX",
        "SOLAIRUS",
        "EXECUTIVE JET",
        "AIRSHARE",
        "NJASPE",
    ];
    NEEDLES.iter().any(|k| n.contains(k))
}

fn is_airline(n: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "AMERICAN AIRLINES",
        "DELTA AIR LINES",
        "DELTA AIRLINES",
        "UNITED AIRLINES",
        "SOUTHWEST AIRLINES",
        "JETBLUE",
        "ALASKA AIRLINES",
        "SPIRIT AIRLINES",
        "FRONTIER AIRLINES",
        "HAWAIIAN AIRLINES",
        "SKYWEST",
        "FEDERAL EXPRESS",
        "FEDEX CORPORATION",
        "FEDEX CORP",
        "UNITED PARCEL",
        "ENVOY AIR",
        "REPUBLIC AIRWAYS",
        "REPUBLIC AIRLINE",
        "MESA AIRLINES",
        "ALLEGIANT AIR",
        "SUN COUNTRY",
        "ATLAS AIR",
        "KALITTA",
        "HORIZON AIR",
        "PSA AIRLINES",
        "ENDEAVOR AIR",
        "AIR WISCONSIN",
        "COMMUTEAIR",
        "GOJET",
        "PIEDMONT AIRLINES",
        "AMERIFLIGHT",
        "ABX AIR",
        "POLAR AIR",
        "BREEZE AVIATION",
        "JETBLUE AIRWAYS",
    ];
    NEEDLES.iter().any(|k| n.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trustee_not_every_bank() {
        assert_eq!(
            classify_registrant("BANK OF UTAH TRUSTEE", false, false),
            Class::Trustee
        );
        assert_eq!(
            classify_registrant("BANK OF AMERICA CORP", false, false),
            Class::Eligible
        );
        assert_eq!(
            classify_registrant("WELLS FARGO BANK NORTHWEST NA TRUSTEE", false, false),
            Class::Trustee
        );
    }

    #[test]
    fn airline_not_unitedhealth() {
        assert_eq!(
            classify_registrant("UNITED AIRLINES INC", false, false),
            Class::Airline
        );
        assert_eq!(
            classify_registrant("UNITEDHEALTH GROUP INC", false, false),
            Class::Eligible
        );
    }

    #[test]
    fn netjets_fractional() {
        assert_eq!(
            classify_registrant("NETJETS SALES INC", false, false),
            Class::Fractional
        );
    }
}
