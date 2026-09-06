//! Corporate-aviation type filter (manufacturer / engine / airframe).
//! Name-based airline, fractional, and trustee denylists live in `resolve`.

use crate::Aircraft;

/// Turbo-prop, turbo-shaft, turbo-jet, turbo-fan.
const TURBINE_ENGINES: &[&str] = &["2", "3", "4", "5"];

/// Fixed-wing multi-engine or rotorcraft.
const CORP_AIRFRAMES: &[&str] = &["5", "6"];

const MFR_ALLOW: &[&str] = &[
    "GULFSTREAM",
    "BOMBARDIER",
    "CANADAIR",
    "LEARJET",
    "DASSAULT",
    "FALCON",
    "EMBRAER",
    "CESSNA",
    "TEXTRON",
    "HAWKER",
    "BEECHCRAFT",
    "BEECH",
    "PILATUS",
    "HONDA",
    "ECLIPSE",
    "CIRRUS",
    "IAI",
    "ISRAEL",
    "SABRELINER",
    "MITSUBISHI",
    "PIAGGIO",
    "SOCATA",
    "DAHER",
    "BRITISH AEROSPACE",
    "BAE",
    "WESTWIND",
    "ASTRA",
    "SIKORSKY",
    "LEONARDO",
    "AGUSTA",
    "AIRBUS HELICOPTER",
    "EUROCOPTER",
    "BELL",
    "BOEING",
    "AIRBUS",
];

const AIRLINER_MODELS: &[&str] = &[
    "737", "747", "757", "767", "777", "787", "A318", "A319", "A320", "A321", "A330", "A340",
    "A350", "A380", "ERJ", "E170", "E175", "E190", "E195", "CRJ", "MD-80", "MD-90", "DC-9", "A220",
];

const CORP_AIRLINER_HINTS: &[&str] = &["BBJ", "ACJ", "BUSINESS", "VIP", "LINEAGE", "PRESTIGE"];

pub fn is_corporate_aviation(ac: &Aircraft) -> bool {
    corporate_reason(ac).is_none()
}

/// Returns `None` if the airframe is in the corporate-aviation universe.
pub fn corporate_reason(ac: &Aircraft) -> Option<&'static str> {
    if !ac.is_valid_registration() {
        return Some("invalid_status");
    }
    if ac.is_individual() {
        return Some("individual");
    }
    let eng = ac.type_engine.trim();
    if !TURBINE_ENGINES.contains(&eng) {
        return Some("not_turbine");
    }
    let air = ac.type_aircraft.trim();
    if !CORP_AIRFRAMES.contains(&air) {
        return Some("not_multi_or_rotor");
    }

    let blob = format!("{} {}", ac.make, ac.model).to_uppercase();
    if is_scheduled_airliner(&blob) {
        return Some("airliner_type");
    }

    let mfr_ok = MFR_ALLOW.iter().any(|m| blob.contains(m));
    let jet = eng == "4" || eng == "5";
    if mfr_ok || (jet && air == "5") {
        return None;
    }
    Some("manufacturer")
}

fn is_scheduled_airliner(blob: &str) -> bool {
    if CORP_AIRLINER_HINTS.iter().any(|h| blob.contains(h)) {
        return false;
    }
    // Embraer Phenom / Praetor / Legacy are corporate even if "EMBRAER" is also
    // used on airliners.
    if blob.contains("PHENOM") || blob.contains("PRAETOR") || blob.contains("LEGACY") {
        return false;
    }
    AIRLINER_MODELS.iter().any(|m| blob.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Aircraft;

    fn ac(
        make: &str,
        model: &str,
        t_ac: &str,
        t_eng: &str,
        status: &str,
        reg_type: &str,
    ) -> Aircraft {
        Aircraft {
            n_number: "N1".into(),
            serial: String::new(),
            type_registrant: reg_type.into(),
            registrant_name: "X INC".into(),
            street: String::new(),
            city: String::new(),
            state: String::new(),
            type_aircraft: t_ac.into(),
            type_engine: t_eng.into(),
            status_code: status.into(),
            fractional_owner: false,
            icao24: String::new(),
            make: make.into(),
            model: model.into(),
        }
    }

    #[test]
    fn gulfstream_passes() {
        assert!(is_corporate_aviation(&ac(
            "GULFSTREAM AEROSPACE",
            "GVI",
            "5",
            "5",
            "V",
            "3"
        )));
    }

    #[test]
    fn piston_trainer_rejected() {
        assert_eq!(
            corporate_reason(&ac("CESSNA", "172", "4", "1", "V", "3")),
            Some("not_turbine")
        );
    }

    #[test]
    fn airline_737_rejected_unless_bbj() {
        assert_eq!(
            corporate_reason(&ac("BOEING", "737-800", "5", "5", "V", "3")),
            Some("airliner_type")
        );
        assert!(is_corporate_aviation(&ac(
            "BOEING", "737 BBJ", "5", "5", "V", "3"
        )));
    }

    #[test]
    fn individual_rejected() {
        assert_eq!(
            corporate_reason(&ac("GULFSTREAM", "G650", "5", "5", "V", "1")),
            Some("individual")
        );
    }
}
