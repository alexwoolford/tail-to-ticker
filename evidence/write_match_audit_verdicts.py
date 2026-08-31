#!/usr/bin/env python3
"""Merge seed-42 sample with citation-backed audit verdicts."""

from __future__ import annotations

import csv
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SAMPLE = ROOT / "evidence/match_audit_sample.csv"
OUT = ROOT / "evidence/match_audit_verdicts.csv"

# n_number -> (verdict, reason, citation)
VERDICTS: dict[str, tuple[str, str, str]] = {
    # --- exact_legal_name ---
    "N100A": (
        "true_positive",
        "FAA registrant EXXON MOBIL CORP at Houston matches Exxon Mobil legal name and HQ metro.",
        "FAA MASTER Houston TX; SEC title Exxon Mobil Corp",
    ),
    "N10M": (
        "false_positive_namesake",
        "ALSET HOLDING CO LLC in Charlotte is not listed among Alset Inc. (AEI) subsidiaries; AEI is a Texas/Singapore holding company.",
        "https://www.sec.gov/Archives/edgar/data/1750106/000149315226023104/R22.htm ; FlightAware N10M Charlotte NC",
    ),
    "N16LJ": (
        "false_positive_namesake",
        "LEAR HOLDING CORP at a Wilmington DE registered-agent address owns Learjets; Lear Corp (LEA) is an auto-parts issuer in Michigan.",
        "https://aircraftdata.org/N16LJ/ ; Lear Corp HQ Southfield MI",
    ),
    "N191MM": (
        "true_positive",
        "WORKDAY INC at Pleasanton CA is the listed issuer’s HQ.",
        "FAA MASTER Pleasanton CA; Workday IR Pleasanton",
    ),
    "N1922G": (
        "true_positive",
        "GRANITE CONSTRUCTION INC at Watsonville CA matches issuer HQ.",
        "FAA MASTER Watsonville CA; Granite Construction IR",
    ),
    "N1972": (
        "true_positive",
        "NIKE INC at Hillsboro OR matches Nike’s Oregon campus.",
        "FAA MASTER Hillsboro OR; Nike IR Beaverton/Hillsboro",
    ),
    "N256RC": (
        "true_positive",
        "VULCAN MATERIALS COMPANY at Vestavia/Birmingham AL matches issuer HQ.",
        "FAA MASTER Vestavia AL; Vulcan Materials IR Birmingham",
    ),
    "N285EM": (
        "true_positive",
        "Same as N100A: EXXON MOBIL CORP Houston.",
        "FAA MASTER Houston TX",
    ),
    "N302KC": (
        "true_positive",
        "KROGER CO at Cincinnati OH matches issuer HQ.",
        "FAA MASTER Cincinnati OH; Kroger IR",
    ),
    "N339LS": (
        "true_positive",
        "LAS VEGAS SANDS CORP at Las Vegas NV matches issuer HQ.",
        "FAA MASTER Las Vegas NV",
    ),
    "N369FG": (
        "true_positive",
        "FULGENT GENETICS INC at Temple City CA matches issuer HQ area.",
        "FAA MASTER Temple City CA; Fulgent IR El Monte/Temple City",
    ),
    "N396BB": (
        "true_positive",
        "BROWN & BROWN INC at Daytona Beach FL matches issuer HQ.",
        "FAA MASTER Daytona Beach FL",
    ),
    "N39FB": (
        "true_positive_operator",
        "BUTLER NATIONAL INC at New Century KS is the listed OTC issuer; the firm’s business includes aircraft modification (match is right, aviation operator).",
        "FAA MASTER New Century KS; Butler National Corp IR",
    ),
    "N412DL": (
        "true_positive",
        "PNC FINANCIAL SERVICES GROUP INC at Pittsburgh PA matches issuer HQ.",
        "FAA MASTER Pittsburgh PA",
    ),
    "N45GX": (
        "true_positive",
        "TEXAS INSTRUMENTS INC in McKinney TX matches TI’s North Texas operations.",
        "FAA MASTER McKinney TX; TI IR Dallas",
    ),
    "N543GL": (
        "true_positive",
        "SKECHERS USA INC at Manhattan Beach CA matches issuer HQ.",
        "FAA MASTER Manhattan Beach CA",
    ),
    "N5VG": (
        "false_positive_namesake",
        "Same Lear Holding Corp Wilmington DE Learjet SPV as N16LJ, not Lear Corp (LEA).",
        "https://aircraftdata.org/reg-name/lear-holding-corp/",
    ),
    "N663TW": (
        "true_positive_operator",
        "Same Butler National New Century KS fleet as N39FB.",
        "FAA MASTER New Century KS",
    ),
    "N680FD": (
        "true_positive",
        "FEDERATED HERMES INC at Pittsburgh PA matches issuer HQ.",
        "FAA MASTER Pittsburgh PA",
    ),
    "N68CB": (
        "true_positive",
        "CRACKER BARREL OLD COUNTRY STORE INC at Lebanon TN matches issuer HQ.",
        "FAA MASTER Lebanon TN",
    ),
    "N780JH": (
        "true_positive",
        "JACK HENRY & ASSOCIATES INC at Monett MO matches issuer HQ.",
        "FAA MASTER Monett MO",
    ),
    "N78TC": (
        "true_positive",
        "TIMKEN CO at North Canton OH matches issuer HQ.",
        "FAA MASTER North Canton OH",
    ),
    "N78UT": (
        "true_positive",
        "UNITED THERAPEUTICS CORP at Silver Spring MD matches issuer HQ.",
        "FAA MASTER Silver Spring MD",
    ),
    "N797CP": (
        "true_positive",
        "CONOCOPHILLIPS CO at Houston TX matches issuer HQ / flight department.",
        "FAA MASTER Houston TX",
    ),
    "N820CA": (
        "true_positive",
        "CONAGRA BRANDS INC at Omaha NE matches issuer HQ.",
        "FAA MASTER Omaha NE",
    ),
    "N824HH": (
        "true_positive",
        "HILLTOP HOLDINGS INC at Dallas TX matches issuer HQ.",
        "FAA MASTER Dallas TX",
    ),
    "N842A": (
        "true_positive",
        "ASTEC INDUSTRIES INC at Chattanooga TN matches issuer HQ.",
        "FAA MASTER Chattanooga TN",
    ),
    "N870SB": (
        "true_positive",
        "SIMMONS FIRST NATIONAL CORP at Little Rock AR matches issuer operations/HQ area.",
        "FAA MASTER Little Rock AR",
    ),
    "N92WL": (
        "true_positive_operator",
        "WILLIS LEASE FINANCE CORP at Larkspur CA is the listed issuer; it is an aircraft lessor (match right, aviation business).",
        "FAA MASTER Larkspur CA; Willis Lease Finance IR",
    ),
    "N967BY": (
        "true_positive",
        "BERRY GLOBAL INC at Evansville IN matches issuer HQ.",
        "FAA MASTER Evansville IN",
    ),
    # --- ex21 non-operator ---
    "N23GW": (
        "true_positive",
        "DUKE ENERGY BUSINESS SERVICES LLC at Charlotte NC First Flight Dr matches Duke Energy HQ / flight department.",
        "FAA MASTER Charlotte NC; Duke Energy IR",
    ),
    "N288DX": (
        "true_positive",
        "QUEST DIAGNOSTICS CLINICAL LABORATORIES INC is a known Quest operating sub (unique brand).",
        "FAA MASTER Reading PA; Quest Diagnostics legal name",
    ),
    "N300TP": (
        "true_positive",
        "OTTER TAIL POWER CO at Fergus Falls MN is Otter Tail Corp’s utility subsidiary and HQ city.",
        "FAA MASTER Fergus Falls MN",
    ),
    "N327ME": (
        "false_positive_namesake",
        "REACH CO LLC at a Hughson CA farm address is not Gannett (McLean VA); REACH is a generic token after suffix strip.",
        "https://aircraftdata.org/N327ME/",
    ),
    "N331AJ": (
        "false_positive_namesake",
        "ACORN LEASING CO LLC at Savannah GA (FBO/business-and-finance attn) has no public tie to EIDP/DuPont.",
        "https://aircraftdata.org/N331AJ/",
    ),
    "N340FL": (
        "true_positive",
        "HINES HILL AVIATION LLC at 51 E Hines Hill Rd, Boston Heights OH is Arhaus’s campus street.",
        "FAA MASTER Boston Heights OH; Arhaus HQ Boston Heights",
    ),
    "N372BW": (
        "false_positive_namesake",
        "BIRDSEYE HOLDING LLC in Phoenix is a charter/Premier I registrant, not Dominion Energy (Richmond VA).",
        "https://acejet.com/aircraft/N372BW/",
    ),
    "N388AM": (
        "true_positive",
        "FIRST-CITIZENS BANK & TRUST CO is First Citizens BancShares’ bank subsidiary (unique name).",
        "FAA MASTER Morristown NJ; First Citizens IR",
    ),
    "N39RE": (
        "true_positive",
        "AIRUSH INC at New Braunfels TX matches Rush Enterprises HQ; AiRush is the company’s aviation unit.",
        "FAA MASTER New Braunfels TX; Rush Enterprises IR",
    ),
    "N40D": (
        "true_positive",
        "WARBLER I LLC at MBS/Freeland MI sits next to Dow’s Midland HQ; same ticker as N890D DOW CHEMICAL CO.",
        "FAA MASTER Freeland MI; Dow Midland MI",
    ),
    "N425AM": (
        "true_positive",
        "Same First-Citizens Bank fleet as N388AM.",
        "FAA MASTER Morristown NJ",
    ),
    "N512RJ": (
        "true_positive",
        "REGIONS COMMERCIAL EQUIPMENT FINANCE LLC at Birmingham AL matches Regions Financial HQ.",
        "FAA MASTER Birmingham AL",
    ),
    "N517WB": (
        "true_positive",
        "DISCOVERY COMMUNICATIONS LLC is Warner Bros. Discovery’s predecessor operating name.",
        "FAA MASTER New York NY; WBD IR",
    ),
    "N547JR": (
        "false_positive_namesake",
        "TIMBERLAND AVIATION LLC at 2926 Timberland Pl NE, Iowa City is a street-named LLC; VF Corp’s Timberland brand is NH/Denver, not Iowa City.",
        "FAA MASTER Iowa City IA; VF 2013 EX-21 listed a Delaware Timberland Aviation LLC (different geography)",
    ),
    "N54DE": (
        "true_positive",
        "Same Duke Energy Business Services Charlotte fleet as N23GW.",
        "FAA MASTER Charlotte NC",
    ),
    "N570D": (
        "true_positive",
        "Same Dow Warbler I LLC Freeland MI fleet as N40D.",
        "FAA MASTER Freeland MI",
    ),
    "N595LL": (
        "false_positive_namesake",
        "O&M HOLDINGS LLC in Ruidoso NM collides with Babcock & Wilcox’s EX-21 ‘O&M Holding Company’ (Delaware); B&W HQ is Akron OH.",
        "https://aircraftdata.org/N595LL/ ; https://www.sec.gov/Archives/edgar/data/1630805/000163080521000004/a12312020-exhibit211.htm",
    ),
    "N609WB": (
        "true_positive",
        "Same Discovery Communications / WBD fleet as N517WB.",
        "FAA MASTER New York NY",
    ),
    "N660S": (
        "false_positive_namesake",
        "ALPINE II LLC at a Boulder CO residential court is not Aimco (AIV); Aimco EX-21 uses AIMCO-prefixed property LLCs.",
        "https://aircraftdata.org/N660S/",
    ),
    "N672HG": (
        "true_positive",
        "KIMBERLY-CLARK INTEGRATED SERVICES CORP at Irving TX matches KMB HQ.",
        "FAA MASTER Irving TX",
    ),
    "N672WV": (
        "true_positive",
        "WRBC TRANSPORTATION INC at Greenwich CT matches W. R. Berkley HQ (WRBC ticker family).",
        "FAA MASTER Greenwich CT",
    ),
    "N673WM": (
        "true_positive",
        "WM CORPORATE SERVICES INC at Houston TX matches Waste Management HQ.",
        "FAA MASTER Houston TX",
    ),
    "N679W": (
        "false_positive_namesake",
        "DANFORTH LLC at Columbus OH (Love Field-adjacent) has no public tie to Ventas (Chicago REIT).",
        "https://registry.faa.gov/AircraftInquiry/Search/NNumberResult?nNumberTxt=679W",
    ),
    "N68HC": (
        "true_positive",
        "HCA SQUARED LLC at One Park Plaza Nashville is HCA Healthcare HQ.",
        "FAA MASTER Nashville TN",
    ),
    "N70CH": (
        "true_positive",
        "CMH HOMES INC at Maryville TN is Clayton Homes, a Berkshire Hathaway subsidiary.",
        "FAA MASTER Maryville TN; Berkshire/Clayton Homes",
    ),
    "N712AB": (
        "true_positive",
        "ARCBEST II INC at Fort Smith AR matches ArcBest HQ.",
        "FAA MASTER Fort Smith AR",
    ),
    "N717RS": (
        "false_positive_namesake",
        "CCG VENTURES LLC at Dallas Love Field (Lemmon Ave) is a generic initials LLC, not Paramount Global (NY).",
        "https://aircraftdata.org/N717RS/",
    ),
    "N71949": (
        "true_positive",
        "WILSHIRE RENTAL CORP is a Tenet Healthcare subsidiary that holds Tenet’s Falcon; Dallas Pkwy matches Tenet offices.",
        "https://www.sec.gov/Archives/edgar/data/70318/000110465912013589/a11-31386_1ex21.htm ; FAA MASTER Dallas TX",
    ),
    "N75KX": (
        "true_positive",
        "CARMAX ENTERPRISE SERVICES LLC at Richmond VA matches CarMax HQ.",
        "FAA MASTER Richmond VA",
    ),
    "N761PC": (
        "true_positive",
        "IDAHO POWER CO at Boise ID is IDACORP’s utility subsidiary and HQ.",
        "FAA MASTER Boise ID",
    ),
    "N793FB": (
        "true_positive",
        "Chipotle EX-21 lists N793WF Lease, LLC; FAA registrant N793WF LEASE LLC is that SPV (Orange County / Chipotle HQ area).",
        "https://www.sec.gov/Archives/edgar/data/1058090/000105809026000009/cmg-20251231xex211.htm",
    ),
    "N812GB": (
        "false_positive_namesake",
        "TRINITY LOGISTICS HOLDINGS LLC in Saint Johns FL is a trucking namesake; Trinity Industries EX-21 lists Trinity Logistics Group, Inc. (Texas rail).",
        "https://aircraftdata.org/N812GB/ ; https://www.sec.gov/Archives/edgar/data/99780/000009978024000017/exh21listofsubsidiaries123.htm",
    ),
    "N853CR": (
        "false_positive_namesake",
        "VEHICLE HOLDINGS INC at 166 N Roadrunner Pkwy Las Cruces shares Virgin Galactic’s campus; Herc Holdings is a Florida equipment lessor.",
        "https://aircraftdata.org/reg-name/vehicle-holdings-inc/ ; Virgin Galactic same street",
    ),
    "N887RZ": (
        "true_positive_operator",
        "KRATOS UNMANNED AERIAL SYSTEMS / MQM-178 is Kratos’s own target-drone product (match right, aerospace OEM).",
        "FAA MASTER Oklahoma City OK; Kratos IR",
    ),
    "N890D": (
        "true_positive",
        "DOW CHEMICAL CO at 2211 HH Dow Way Midland MI is Dow’s HQ.",
        "FAA MASTER Midland MI",
    ),
    "N899BH": (
        "true_positive",
        "NISOURCE CORPORATE SERVICES CO is NiSource’s services sub; Gary IN is in NiSource’s utility footprint.",
        "FAA MASTER Gary IN",
    ),
    "N929MZ": (
        "false_positive_namesake",
        "M&M AVIATION LLC at Noblesville IN is a generic aviation LLC, not Truist (Charlotte).",
        "https://aircraftdata.org/N929MZ/",
    ),
    "N957AP": (
        "true_positive",
        "O'REILLY AUTOMOTIVE STORES INC at Springfield MO matches O’Reilly HQ.",
        "FAA MASTER Springfield MO",
    ),
    "N97UT": (
        "true_positive",
        "MIDAMERICAN ENERGY CO at Des Moines IA is a Berkshire Hathaway energy subsidiary.",
        "FAA MASTER Des Moines IA",
    ),
    "N988F": (
        "true_positive",
        "FRANKLIN TEMPLETON TRAVEL INC at 1 Franklin Pkwy San Mateo CA is Franklin Resources HQ.",
        "FAA MASTER San Mateo CA",
    ),
    # --- ex21 stress ---
    "N124GD": (
        "false_positive_namesake",
        "DINO CORPORATION LLC at 1209 Orange St Wilmington (Corporation Trust) has no public tie to Dynavax (Berkeley CA); ‘DINO’ is a 4-letter core.",
        "https://aircraftdata.org/N124GD/",
    ),
    "N1875K": (
        "true_positive",
        "HCRH LLC at Churchill Downs’ Louisville HQ address is a documented CHDN acquisition subsidiary.",
        "https://www.sec.gov/Archives/edgar/data/20212/000002021212000031/a100512.htm ; FAA MASTER Louisville KY",
    ),
    "N300XL": (
        "false_positive_namesake",
        "1911 PARTNERS LLC in Rockledge FL (local managers) is not Morgan Stanley; ticker MSTLW is an OTC warrant, not common stock.",
        "https://tailnumberlookup.com/aircraft/300XL",
    ),
    "N313WL": (
        "false_positive_namesake",
        "PEAK AVIATION LLC at a Royal Oak MI house address is a generic aviation LLC, not Jefferies (NY).",
        "https://aircraftdata.org/N313WL/",
    ),
    "N365LV": (
        "true_positive",
        "AULT AVIATION LLC at 11411 Southern Highlands Pkwy Las Vegas is a disclosed Hyperscale Data / Ault Alliance aircraft subsidiary.",
        "https://www.sec.gov/Archives/edgar/data/896493/000121465923012191/0001214659-23-012191.txt",
    ),
    "N43CB": (
        "true_positive",
        "PNFP AVIATION LLC at Nashville matches Pinnacle Financial Partners HQ and ticker.",
        "FAA MASTER Nashville TN",
    ),
    "N444QG": (
        "true_positive",
        "QUAD/AIR LLC at Sussex WI matches Quad/Graphics HQ.",
        "FAA MASTER Sussex WI",
    ),
    "N47KH": (
        "false_positive_namesake",
        "HAWK PARTNERS HOLDINGS LLC in Fort Worth (Donny Lassetter) has no public tie to Carlisle Companies.",
        "https://aircraftdata.org/N47KH/",
    ),
    "N55PC": (
        "true_positive",
        "CTEH LEASING LLC in North Little Rock is CTEH (toxicology), a Montrose Environmental (MEG) acquisition.",
        "FAA MASTER N Little Rock AR; Montrose/CTEH",
    ),
    "N650LC": (
        "true_positive",
        "AGCO CORP at Duluth GA is the listed issuer’s HQ (EX-21 hit on the parent name itself).",
        "FAA MASTER Duluth GA",
    ),
}


def main() -> None:
    rows = list(csv.DictReader(SAMPLE.open()))
    missing = [r["n_number"] for r in rows if r["n_number"] not in VERDICTS]
    extra = sorted(set(VERDICTS) - {r["n_number"] for r in rows})
    if missing or extra:
        raise SystemExit(f"missing={missing} extra={extra}")
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
    from collections import Counter

    print(f"wrote {OUT} n={len(out_rows)}")
    print("overall", Counter(r["verdict"] for r in out_rows))
    for s in sorted({r["stratum"] for r in out_rows}):
        sub = [r for r in out_rows if r["stratum"] == s]
        print(s, len(sub), Counter(r["verdict"] for r in sub))


if __name__ == "__main__":
    main()
