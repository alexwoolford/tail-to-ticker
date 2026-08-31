#!/usr/bin/env python3
"""Merge seed-43 published sample with citation-backed verdicts."""

from __future__ import annotations

import csv
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SAMPLE = ROOT / "evidence/published_precision_sample.csv"
OUT = ROOT / "evidence/published_precision_verdicts.csv"

# n_number -> (verdict, reason, citation)
VERDICTS: dict[str, tuple[str, str, str]] = {
    "N1027P": (
        "true_positive_operator",
        "TEXTRON AVIATION INC at One Cessna Blvd Wichita is Textron’s Cessna/Beech OEM (match right, aviation manufacturer).",
        "FAA MASTER Wichita KS; Textron Aviation HQ",
    ),
    "N145BH": (
        "true_positive_operator",
        "BELL TEXTRON INC at Bell Flight Blvd Fort Worth is Textron’s helicopter OEM.",
        "FAA MASTER Fort Worth TX; Bell Textron",
    ),
    "N158AB": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N166AE": (
        "true_positive_operator",
        "TEXTRON FINANCIAL CORP is Textron’s captive finance sub; ticker TXT is an aerospace OEM.",
        "FAA MASTER Wichita KS; Textron IR",
    ),
    "N1895T": (
        "true_positive",
        "CHEVRON U S A INC at Sugar Land TX is Chevron’s principal U.S. operating company (Houston-area campus).",
        "FAA MASTER Sugar Land TX; Chevron USA",
    ),
    "N19KB": (
        "true_positive",
        "KONTOOR SERVICES LLC at 400 N Elm St Greensboro is on Kontoor Brands’ EX-21 and is the HQ street.",
        "https://www.sec.gov/Archives/edgar/data/1760965/000176096525000011/kontoor202410-kex21.htm",
    ),
    "N206BH": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N24PL": (
        "true_positive",
        "POLARIS INDUSTRIES INC at 2100 Highway 55 Medina MN is Polaris HQ.",
        "FAA MASTER Medina MN; Polaris IR",
    ),
    "N280GD": (
        "true_positive_operator",
        "GENERAL DYNAMICS ORDNANCE AND TACTICAL SYSTEMS INC at Carillon Pkwy St Petersburg is a GD operating sub; GD is a defense/aerospace OEM (Gulfstream).",
        "FAA MASTER Saint Petersburg FL; GD-OTS",
    ),
    "N288KM": (
        "true_positive_operator",
        "GARMIN INTERNATIONAL INC at 1200 E 151st St Olathe KS is Garmin’s U.S. HQ; Garmin is an avionics OEM.",
        "FAA MASTER Olathe KS; Garmin IR",
    ),
    "N296R": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N3107N": (
        "true_positive",
        "WEYERHAEUSER NR CO at 220 Occidental Ave S Seattle is Weyerhaeuser’s HQ / natural-resources operating co.",
        "FAA MASTER Seattle WA; Weyerhaeuser IR",
    ),
    "N367JX": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N37WH": (
        "true_positive_operator",
        "NORTHROP GRUMMAN SYSTEMS CORP at 1 Hornet Way El Segundo is Northrop’s operating company (defense aerospace).",
        "FAA MASTER El Segundo CA; Northrop IR",
    ),
    "N391BM": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N407GM": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N441WM": (
        "true_positive",
        "WHITE MOUNTAINS CAPITAL LLC at 23 S Main St Hanover NH matches White Mountains Insurance Group HQ.",
        "FAA MASTER Hanover NH; White Mountains IR",
    ),
    "N47CK": (
        "true_positive",
        "DUKE ENERGY BUSINESS SERVICES LLC in Charlotte is Duke Energy’s services sub / flight department.",
        "FAA MASTER Charlotte NC; Duke Energy IR",
    ),
    "N505GR": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N57HE": (
        "true_positive_operator",
        "TEXTRON AVIATION INC at 1 Cessna Blvd Wichita is Textron’s fixed-wing OEM.",
        "FAA MASTER Wichita KS",
    ),
    "N598WC": (
        "true_positive",
        "WASTE CONNECTIONS US INC at 17725 JFK Blvd Houston is Waste Connections’ U.S. operating subsidiary.",
        "https://registry.faa.gov/AircraftInquiry/Search/NNumberResult?nNumberTxt=598WC",
    ),
    "N62774": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N628G": (
        "true_positive_operator",
        "GENERAL DYNAMICS MISSION SYSTEMS INC at 12450 Fair Lakes Cir Fairfax VA is that GD sub’s HQ campus.",
        "FAA MASTER Fairfax VA; GD Mission Systems",
    ),
    "N672HG": (
        "true_positive",
        "KIMBERLY-CLARK INTEGRATED SERVICES CORP at 351 Phelps Dr Irving TX is K-C HQ.",
        "FAA MASTER Irving TX; Kimberly-Clark IR",
    ),
    "N692BG": (
        "true_positive_operator",
        "BRISTOW U S LLC at New Iberia LA is Bristow Group’s U.S. helicopter operating company (offshore/SAR).",
        "FAA MASTER New Iberia LA; Bristow IR",
    ),
    "N6PG": (
        "true_positive",
        "PROCTER & GAMBLE LEASING LLC at One Procter & Gamble Plaza Cincinnati is P&G HQ.",
        "FAA MASTER Cincinnati OH; P&G IR",
    ),
    "N730AE": (
        "true_positive",
        "ATLANTIC UNION BANK at 2475 Northwinds Pkwy Alpharetta is Atlantic Union’s equipment-finance office (same bank as AUB).",
        "https://www.atlanticunionbank.com/commercial/industry-expertise/governmental-leasing",
    ),
    "N741AM": (
        "true_positive_operator",
        "TEXTRON FINANCIAL CORP at Cessna Blvd Wichita is Textron captive finance; TXT is an aerospace OEM.",
        "FAA MASTER Wichita KS",
    ),
    "N74NG": (
        "true_positive_operator",
        "Same Northrop Grumman Systems El Segundo fleet as N37WH.",
        "FAA MASTER El Segundo CA",
    ),
    "N750CX": (
        "true_positive_operator",
        "TEXTRON AVIATION INC at 1 Cessna Blvd Wichita is Textron’s Citation OEM.",
        "FAA MASTER Wichita KS",
    ),
    "N767H": (
        "true_positive",
        "Same Weyerhaeuser NR Co Seattle HQ fleet as N3107N.",
        "FAA MASTER Seattle WA",
    ),
    "N792CP": (
        "true_positive",
        "CONOCOPHILLIPS ALASKA INC at Anchorage airpark is ConocoPhillips’ Alaska operating subsidiary (Dash 8 oil-patch support).",
        "FAA MASTER Anchorage AK; ConocoPhillips Alaska",
    ),
    "N805X": (
        "true_positive_operator",
        "Same Northrop Grumman Systems El Segundo fleet as N37WH.",
        "FAA MASTER El Segundo CA",
    ),
    "N820UT": (
        "true_positive_operator",
        "Same Garmin International Olathe HQ fleet as N288KM.",
        "FAA MASTER Olathe KS",
    ),
    "N846UP": (
        "true_positive",
        "UNION PACIFIC RAILROAD CO at Doolittle Plaza Omaha is Union Pacific’s railroad operating company / HQ area.",
        "FAA MASTER Omaha NE; Union Pacific IR",
    ),
    "N890D": (
        "true_positive",
        "DOW CHEMICAL CO at 2211 HH Dow Way Midland MI is Dow HQ.",
        "FAA MASTER Midland MI; Dow IR",
    ),
    "N907MT": (
        "true_positive",
        "FIRST-CITIZENS BANK & TRUST CO at Mount Kemble Ave Morristown NJ is First Citizens’ bank subsidiary.",
        "FAA MASTER Morristown NJ; First Citizens IR",
    ),
    "N915HB": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N929BT": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N947TR": (
        "true_positive_operator",
        "Same Bell Textron Fort Worth OEM fleet as N145BH.",
        "FAA MASTER Fort Worth TX",
    ),
    "N120GE": (
        "true_positive_operator",
        "GENERAL ELECTRIC CO at Cincinnati is GE Aerospace (ticker GE) / GE Aviation campus (match right, aviation OEM).",
        "FAA MASTER Cincinnati OH; GE Aerospace",
    ),
    "N1922G": (
        "true_positive",
        "GRANITE CONSTRUCTION INC at 585 W Beach St Watsonville CA is Granite HQ.",
        "FAA MASTER Watsonville CA; Granite IR",
    ),
    "N19GR": (
        "true_positive",
        "GORMAN-RUPP CO at Mansfield OH airport road matches Gorman-Rupp HQ.",
        "FAA MASTER Mansfield OH; Gorman-Rupp IR",
    ),
    "N1HP": (
        "true_positive",
        "HELMERICH & PAYNE INC at 1427 S Boulder Ave Tulsa is H&P HQ (ticker HP is the driller, not HP Inc).",
        "FAA MASTER Tulsa OK; Helmerich & Payne IR",
    ),
    "N202AV": (
        "true_positive",
        "AVISTA CORP at 1411 E Mission Ave Spokane is Avista HQ.",
        "FAA MASTER Spokane WA; Avista IR",
    ),
    "N2291R": (
        "true_positive",
        "INGERSOLL RAND INC at 525 Harbour Place Dr Davidson NC is Ingersoll Rand HQ.",
        "FAA MASTER Davidson NC; Ingersoll Rand IR",
    ),
    "N281WC": (
        "true_positive",
        "WILLIAMS COMPANIES INC hangar at Sheridan Rd Tulsa matches Williams HQ city.",
        "FAA MASTER Tulsa OK; Williams IR",
    ),
    "N283CE": (
        "true_positive",
        "CUMMINS INC at 5175 N Warren Dr Columbus IN is Cummins HQ / flight department.",
        "FAA MASTER Columbus IN; Cummins IR",
    ),
    "N285AF": (
        "true_positive",
        "AFLAC INC at 1932 Wynnton Rd Columbus GA is Aflac HQ.",
        "FAA MASTER Columbus GA; Aflac IR",
    ),
    "N336LS": (
        "true_positive",
        "LAS VEGAS SANDS CORP at Haven St Las Vegas matches the issuer’s Las Vegas operations.",
        "FAA MASTER Las Vegas NV; Las Vegas Sands IR",
    ),
    "N352W": (
        "true_positive",
        "WORTHINGTON ENTERPRISES INC at 200 Old Wilson Bridge Rd Columbus OH is Worthington HQ.",
        "FAA MASTER Columbus OH; Worthington IR",
    ),
    "N37TD": (
        "true_positive",
        "TWIN DISC INC at 1328 Racine St Racine WI is Twin Disc HQ.",
        "FAA MASTER Racine WI; Twin Disc IR",
    ),
    "N39FB": (
        "true_positive_operator",
        "BUTLER NATIONAL INC at 1 Aero Plz New Century KS is the OTC issuer; the firm’s business includes aircraft modification.",
        "FAA MASTER New Century KS; Butler National IR",
    ),
    "N421SC": (
        "true_positive",
        "STRYKER CORP at 1941 Stryker Way Portage MI is Stryker HQ.",
        "FAA MASTER Portage MI; Stryker IR",
    ),
    "N45GH": (
        "true_positive",
        "WALMART INC at 702 SW 8th St Bentonville is Walmart HQ.",
        "FAA MASTER Bentonville AR; Walmart IR",
    ),
    "N45GX": (
        "true_positive",
        "TEXAS INSTRUMENTS INC at Wattley Way McKinney TX is a TI campus / flight department.",
        "FAA MASTER McKinney TX; Texas Instruments",
    ),
    "N494EC": (
        "true_positive",
        "EASTMAN CHEMICAL CO at 200 S Wilcox Dr Kingsport TN is Eastman HQ.",
        "FAA MASTER Kingsport TN; Eastman IR",
    ),
    "N507TS": (
        "true_positive_operator",
        "TEXTRON INC at 40 Westminster St Providence is Textron corporate HQ; Textron is an aerospace OEM.",
        "FAA MASTER Providence RI; Textron IR",
    ),
    "N570PK": (
        "true_positive",
        "PRIMORIS SERVICES CORP at 2300 N Field St Dallas is Primoris HQ.",
        "FAA MASTER Dallas TX; Primoris IR",
    ),
    "N680FD": (
        "true_positive",
        "FEDERATED HERMES INC at 1001 Liberty Ave Pittsburgh is Federated Hermes HQ.",
        "FAA MASTER Pittsburgh PA; Federated Hermes IR",
    ),
    "N683UF": (
        "true_positive",
        "UNIFIRST CORP is the listed issuer’s legal name; Manchester NH is a UniFirst facility (HQ is Wilmington MA).",
        "FAA MASTER Manchester NH; UniFirst IR",
    ),
    "N68CB": (
        "true_positive",
        "CRACKER BARREL OLD COUNTRY STORE INC at 305 Hartman Dr Lebanon TN is Cracker Barrel HQ.",
        "FAA MASTER Lebanon TN; Cracker Barrel IR",
    ),
    "N701P": (
        "true_positive",
        "PACCAR INC at 1500 S 184th St SeaTac WA is PACCAR’s Seattle-area operations (issuer HQ Bellevue/Renton).",
        "FAA MASTER SeaTac WA; PACCAR IR",
    ),
    "N831MT": (
        "true_positive",
        "MICRON TECHNOLOGY INC at 8000 S Federal Way Boise is Micron HQ.",
        "FAA MASTER Boise ID; Micron IR",
    ),
    "N842A": (
        "true_positive",
        "ASTEC INDUSTRIES INC at 1725 Shepherd Rd Chattanooga is Astec HQ.",
        "FAA MASTER Chattanooga TN; Astec IR",
    ),
    "N881RC": (
        "false_positive_namesake",
        "COOPER COMPANIES INC at 15881 N 80th St Scottsdale AZ is a local firm, not The Cooper Companies (COO, San Ramon CA medical devices).",
        "https://www.myplane.com/N881RC/ ; COO HQ San Ramon CA",
    ),
    "N894JH": (
        "true_positive",
        "JACK HENRY & ASSOCIATES INC at 663 W Highway 60 Monett MO is Jack Henry HQ.",
        "FAA MASTER Monett MO; Jack Henry IR",
    ),
    "N92WL": (
        "true_positive_operator",
        "WILLIS LEASE FINANCE CORP at Larkspur CA is the listed aircraft lessor (match right, aviation business).",
        "FAA MASTER Larkspur CA; Willis Lease Finance IR",
    ),
    "N980JF": (
        "true_positive",
        "NUCOR CORP at 1915 Rexford Rd Charlotte is Nucor HQ.",
        "FAA MASTER Charlotte NC; Nucor IR",
    ),
    "N986BL": (
        "true_positive",
        "Same Walmart Inc Bentonville HQ fleet as N45GH.",
        "FAA MASTER Bentonville AR",
    ),
}


def main() -> None:
    with SAMPLE.open() as f:
        rows = list(csv.DictReader(f))
    missing = [r["n_number"] for r in rows if r["n_number"] not in VERDICTS]
    extra = sorted(set(VERDICTS) - {r["n_number"] for r in rows})
    if missing or extra:
        raise SystemExit(f"verdict coverage mismatch missing={missing} extra={extra}")
    fields = list(rows[0].keys()) + ["verdict", "reason", "citation"]
    out_rows = []
    for r in rows:
        v, reason, cite = VERDICTS[r["n_number"]]
        r = dict(r)
        r["verdict"] = v
        r["reason"] = reason
        r["citation"] = cite
        out_rows.append(r)
    with OUT.open("w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(out_rows)
    print("wrote", OUT, "n", len(out_rows))
    print("overall", Counter(r["verdict"] for r in out_rows))
    for s in ("exact_legal_name", "ex21_subsidiary"):
        sub = [r for r in out_rows if r["stratum"] == s]
        print(s, len(sub), Counter(r["verdict"] for r in sub))


if __name__ == "__main__":
    main()
