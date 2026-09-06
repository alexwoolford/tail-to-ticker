//! FAA MASTER.txt / ACFTREF.txt parsers.
//!
//! Current nightly files are comma-delimited with a padded header row. Older
//! dumps omit the header. We accept both. N-numbers are stored without a
//! leading N in MASTER; we always emit a canonical `N…` value.

use std::collections::HashMap;
use std::io::{Cursor, Read};

use csv::{ReaderBuilder, StringRecord, Trim};
use zip::ZipArchive;

use crate::{canonical_icao24, canonical_n_number, Aircraft, Result};

pub fn parse_registry_zip(bytes: &[u8]) -> Result<(Vec<Aircraft>, usize)> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let master = read_zip_member(&mut archive, "MASTER")?;
    let acftref = read_zip_member(&mut archive, "ACFTREF")?;
    let refs = parse_acftref(&acftref)?;
    let aircraft = parse_master(&master, &refs)?;
    Ok((aircraft, refs.len()))
}

fn read_zip_member(archive: &mut ZipArchive<Cursor<&[u8]>>, needle: &str) -> Result<String> {
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_ascii_uppercase();
        if name.contains(needle) && name.ends_with(".TXT") {
            let mut buf = String::new();
            file.read_to_string(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(crate::Error::Msg(format!(
        "{needle}.txt not found in FAA zip"
    )))
}

pub fn parse_acftref(text: &str) -> Result<HashMap<String, (String, String)>> {
    let mut rdr = faa_reader(text);
    let headers = rdr.headers()?.clone();
    let headered = looks_like_acftref_header(&headers);
    let mut map = HashMap::new();
    let idx = if headered {
        Cols::from_headers(&headers)
    } else {
        // CODE, MFR, MODEL, TYPE-ACFT, TYPE-ENG, AC-CAT, BUILD-CERT-IND, NO-ENG, NO-SEATS, ...
        Cols {
            code: Some(0),
            make: Some(1),
            model: Some(2),
            ..Cols::default()
        }
    };
    for rec in rdr.records() {
        let rec = rec?;
        if !headered && rec_looks_like_acftref_header(&rec) {
            continue;
        }
        let code = idx.get(&rec, idx.code).to_uppercase();
        if code.is_empty() || code == "CODE" {
            continue;
        }
        map.insert(code, (idx.get(&rec, idx.make), idx.get(&rec, idx.model)));
    }
    Ok(map)
}

pub fn parse_master(
    text: &str,
    refs: &HashMap<String, (String, String)>,
) -> Result<Vec<Aircraft>> {
    let mut rdr = faa_reader(text);
    let headers = rdr.headers()?.clone();
    let headered = looks_like_master_header(&headers);
    let idx = if headered {
        Cols::from_headers(&headers)
    } else {
        Cols::master_positional()
    };
    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec?;
        if rec.len() < 7 {
            continue;
        }
        if !headered && rec_looks_like_master_header(&rec) {
            continue;
        }
        let n_raw = idx.get(&rec, idx.n_number);
        if n_raw.is_empty() || n_raw.eq_ignore_ascii_case("N-NUMBER") {
            continue;
        }
        let n_number = canonical_n_number(&n_raw);
        let code = idx.get(&rec, idx.mfr_mdl).to_uppercase();
        let (make, model) = refs
            .get(&code)
            .cloned()
            .unwrap_or_else(|| (String::new(), String::new()));
        let fract = idx.get(&rec, idx.fract_owner);
        out.push(Aircraft {
            n_number,
            serial: idx.get(&rec, idx.serial),
            type_registrant: idx.get(&rec, idx.type_registrant),
            registrant_name: idx.get(&rec, idx.name),
            street: idx.get(&rec, idx.street),
            city: idx.get(&rec, idx.city),
            state: idx.get(&rec, idx.state),
            type_aircraft: idx.get(&rec, idx.type_aircraft),
            type_engine: idx.get(&rec, idx.type_engine),
            status_code: idx.get(&rec, idx.status),
            fractional_owner: fract.eq_ignore_ascii_case("Y"),
            icao24: canonical_icao24(&idx.get(&rec, idx.mode_s_hex)),
            make,
            model,
        });
    }
    Ok(out)
}

fn faa_reader(text: &str) -> csv::Reader<&[u8]> {
    ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(Trim::All)
        .from_reader(text.as_bytes())
}

fn looks_like_master_header(h: &StringRecord) -> bool {
    h.iter().any(|c| {
        c.replace(' ', "").eq_ignore_ascii_case("N-NUMBER") || c.eq_ignore_ascii_case("N-NUMBER")
    })
}

fn rec_looks_like_master_header(r: &StringRecord) -> bool {
    r.get(0)
        .map(|c| c.replace(' ', "").eq_ignore_ascii_case("N-NUMBER"))
        .unwrap_or(false)
}

fn looks_like_acftref_header(h: &StringRecord) -> bool {
    h.iter()
        .any(|c| c.trim().eq_ignore_ascii_case("CODE") || c.trim().eq_ignore_ascii_case("MFR"))
}

fn rec_looks_like_acftref_header(r: &StringRecord) -> bool {
    r.get(0)
        .map(|c| c.trim().eq_ignore_ascii_case("CODE"))
        .unwrap_or(false)
}

#[derive(Default)]
struct Cols {
    n_number: Option<usize>,
    serial: Option<usize>,
    mfr_mdl: Option<usize>,
    type_registrant: Option<usize>,
    name: Option<usize>,
    street: Option<usize>,
    city: Option<usize>,
    state: Option<usize>,
    type_aircraft: Option<usize>,
    type_engine: Option<usize>,
    status: Option<usize>,
    fract_owner: Option<usize>,
    mode_s_hex: Option<usize>,
    code: Option<usize>,
    make: Option<usize>,
    model: Option<usize>,
}

impl Cols {
    fn master_positional() -> Self {
        Self {
            n_number: Some(0),
            serial: Some(1),
            mfr_mdl: Some(2),
            type_registrant: Some(5),
            name: Some(6),
            street: Some(7),
            city: Some(9),
            state: Some(10),
            type_aircraft: Some(18),
            type_engine: Some(19),
            status: Some(20),
            fract_owner: Some(22),
            mode_s_hex: Some(29),
            ..Self::default()
        }
    }

    fn from_headers(h: &StringRecord) -> Self {
        let mut c = Self::default();
        for (i, name) in h.iter().enumerate() {
            let key = norm_header(name);
            match key.as_str() {
                "N-NUMBER" | "NNUMBER" => c.n_number = Some(i),
                "SERIALNUMBER" | "SERIAL" => c.serial = Some(i),
                "MFRMDLCODE" | "CODE" => {
                    if c.mfr_mdl.is_none() {
                        c.mfr_mdl = Some(i);
                    }
                    if c.code.is_none() {
                        c.code = Some(i);
                    }
                }
                "TYPEREGISTRANT" | "TYPE-REGISTRANT" => c.type_registrant = Some(i),
                "NAME" => c.name = Some(i),
                "STREET" => c.street = Some(i),
                "CITY" => c.city = Some(i),
                "STATE" => c.state = Some(i),
                "TYPEAIRCRAFT" | "TYPE-ACFT" => c.type_aircraft = Some(i),
                "TYPEENGINE" | "TYPE-ENG" => c.type_engine = Some(i),
                "STATUSCODE" => c.status = Some(i),
                "FRACTOWNER" => c.fract_owner = Some(i),
                "MODESCODEHEX" => c.mode_s_hex = Some(i),
                "MFR" => c.make = Some(i),
                "MODEL" => c.model = Some(i),
                _ => {}
            }
        }
        c
    }

    fn get(&self, rec: &StringRecord, col: Option<usize>) -> String {
        col.and_then(|i| rec.get(i))
            .unwrap_or("")
            .trim()
            .trim_end_matches(',')
            .to_string()
    }
}

fn norm_header(s: &str) -> String {
    s.trim()
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>()
        .replace('-', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = "\
N-NUMBER,SERIAL NUMBER,MFR MDL CODE,ENG MFR MDL,YEAR MFR,TYPE REGISTRANT,NAME,STREET,STREET2,CITY,STATE,ZIP CODE,REGION,COUNTY,COUNTRY,LAST ACTION DATE,CERT ISSUE DATE,CERTIFICATION,TYPE AIRCRAFT,TYPE ENGINE,STATUS CODE,MODE S CODE,FRACT OWNER,AIR WORTH DATE,OTHER NAMES(1),OTHER NAMES(2),OTHER NAMES(3),OTHER NAMES(4),OTHER NAMES(5),EXPIRATION DATE,UNIQUE ID,KIT MFR,KIT MODEL,MODE S CODE HEX
1WM,12345,GULG650,12345,2018,3,WALMART INC,702 SW 8TH ST,,BENTONVILLE,AR,72716,C,007,US,20240101,20180101,1N,5,5,V,12345678,N,20180101,,,,,,20280101,1,,,A00B1C
123AA,737,BOE737,1,2010,3,AMERICAN AIRLINES INC,1 SKYVIEW,,FORT WORTH,TX,76155,2,439,US,20240101,20100101,1T,5,5,V,1,N,20100101,,,,,,20280101,2,,,A11111
50UT,650,GULG650,1,2019,3,BANK OF UTAH TRUSTEE,200 E SOUTH TEMPLE,,SALT LAKE CITY,UT,84111,4,000,US,20240101,20190101,1N,5,5,V,1,N,20190101,,,,,,20280101,3,,,A22222
";

    const ACFTREF: &str = "\
CODE,MFR,MODEL,TYPE-ACFT,TYPE-ENG,AC-CAT,BUILD-CERT-IND,NO-ENG,NO-SEATS,AC-WEIGHT,SPEED
GULG650,GULFSTREAM AEROSPACE,GVI G650,5,5,1,0,2,  19,CLASS 3,0
BOE737,BOEING,737-800,5,5,1,0,2,189,CLASS 3,0
";

    #[test]
    fn parses_headered_master() {
        let refs = parse_acftref(ACFTREF).unwrap();
        let rows = parse_master(MASTER, &refs).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].n_number, "N1WM");
        assert_eq!(rows[0].registrant_name, "WALMART INC");
        assert_eq!(rows[0].make, "GULFSTREAM AEROSPACE");
        assert_eq!(rows[0].model, "GVI G650");
        assert_eq!(rows[0].icao24, "a00b1c");
        assert_eq!(rows[1].n_number, "N123AA");
        assert!(!rows[0].fractional_owner);
    }

    #[test]
    fn positional_master_without_header_uses_fixed_columns() {
        // csv crate still consumes first line as headers when has_headers(true).
        // Prefix a dummy header-shaped first record that is NOT N-NUMBER so
        // positional mapping applies to subsequent rows.
        let text = "\
x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x,x
N1HD,99,GULG650,e,2017,3,HOME DEPOT INC,st,,ATLANTA,GA,30339,r,c,US,20240101,20170101,cert,5,5,V,ms,N,20170101,o1,o2,o3,o4,o5,A00FFF
";
        let refs = HashMap::new();
        let rows = parse_master(text, &refs).unwrap();
        assert_eq!(rows[0].n_number, "N1HD");
        assert_eq!(rows[0].registrant_name, "HOME DEPOT INC");
        assert_eq!(rows[0].icao24, "a00fff");
    }

    #[test]
    fn parse_registry_zip_roundtrip() {
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zw = zip::ZipWriter::new(&mut buf);
            let opts = SimpleFileOptions::default();
            zw.start_file("MASTER.txt", opts).unwrap();
            zw.write_all(MASTER.as_bytes()).unwrap();
            zw.start_file("ACFTREF.txt", opts).unwrap();
            zw.write_all(ACFTREF.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let bytes = buf.into_inner();
        let (rows, nref) = parse_registry_zip(&bytes).unwrap();
        assert_eq!(rows.len(), 3);
        assert!(nref >= 2);
        assert_eq!(rows[0].make, "GULFSTREAM AEROSPACE");
    }
}
