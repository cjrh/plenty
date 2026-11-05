//! `std.units.catalogue` and the named units of `std.units`, checked against
//! the conversion factors NIST SP 811 (2008) prints in table B.8, and the
//! catalogue's prefixed SI units checked for completeness.
use std::process::Command;

/// Prints `expr` for each item, one per line, and parses the results. The
/// prints are split across functions to stay under the frontend's limit of
/// 256 local slots per function.
fn values<T>(items: &[T], expr: impl Fn(&T) -> String) -> Vec<f64> {
    let mut source_text =
        String::from("import std.units as u\nimport std.units.catalogue as uc\n\n");
    let chunks: Vec<_> = items.chunks(64).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        source_text.push_str(&format!("def part{index}() -> Result[(), IoError]:\n"));
        for item in *chunk {
            source_text.push_str(&format!("    print({})?\n", expr(item)));
        }
        source_text.push_str("    Ok(())\n\n");
    }
    source_text.push_str("def main() -> Result[(), IoError]:\n");
    for index in 0..chunks.len() {
        source_text.push_str(&format!("    part{index}()?\n"));
    }
    source_text.push_str("    Ok(())\n");
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("main.plenty");
    std::fs::write(&source, source_text).unwrap();
    let program = dir.path().join("program");
    plenty::compile_file_to_executable(&source, &program, None).unwrap();
    let output = Command::new(program).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| line.parse().unwrap())
        .collect()
}

/// Half a unit in the last printed digit of a rounded factor such as
/// `1.333224e+03`.
fn rounding_tolerance(factor: &str) -> f64 {
    let (mantissa, exponent) = factor.split_once('e').unwrap();
    let decimals = mantissa.split_once('.').map_or(0, |(_, d)| d.len());
    let exponent: i32 = exponent.parse().unwrap();
    0.5 * 10f64.powi(exponent - decimals as i32)
}

// Generated from NIST SP 811 (2008) table B.8: (constant, factor, exact).
const SP811_FACTORS: &[(&str, &str, bool)] = &[
    ("u.angstrom", "1.0e-10", true),                    // ångström (Å)
    ("u.km_per_hour", "2.777778e-01", false),           // kilometer per hour (km/h)
    ("u.mph", "4.4704e-01", true),                      // mile per hour (mi/h)
    ("u.year", "3.1536e+07", true),                     // year (365 days)
    ("u.g_n", "9.80665e+00", true),                     // acceleration of free fall, standard (g_n)
    ("u.atm", "1.01325e+05", true),                     // atmosphere, standard (atm)
    ("u.bar", "1.0e+05", true),                         // bar (bar)
    ("u.mbar", "1.0e+02", true),                        // millibar (mbar)
    ("u.hectare", "1.0e+04", true),                     // hectare (ha)
    ("u.litre", "1.0e-03", true),                       // liter (L)
    ("u.tonne", "1.0e+03", true),                       // tonne (called "metric ton" in U.S.) (t)
    ("u.minute", "6.0e+01", true),                      // minute (min)
    ("u.hour", "3.6e+03", true),                        // hour (h)
    ("u.day", "8.64e+04", true),                        // day (d)
    ("u.degree", "1.745329e-02", false),                // degree (angle) (°)
    ("u.arcminute", "2.908882e-04", false),             // minute (angle) (′)
    ("u.arcsecond", "4.848137e-06", false),             // second (angle) (″)
    ("u.au", "1.495979e+11", false),                    // astronomical unit (au)
    ("u.eV", "1.602177e-19", false),                    // electronvolt (eV)
    ("u.rankine", "5.555556e-01", false), // degree Rankine (temperature interval) (°R)
    ("u.inch", "2.54e-02", true),         // inch (in)
    ("u.foot", "3.048e-01", true),        // foot (ft)
    ("u.yard", "9.144e-01", true),        // yard (yd)
    ("u.mile", "1.609344e+03", true),     // mile (mi)
    ("u.nautical_mile", "1.852e+03", true), // mile, nautical
    ("u.light_year", "9.46073e+15", false), // light year (l.y.)
    ("u.parsec", "3.085678e+16", false),  // parsec (pc)
    ("u.gallon", "3.785412e-03", false),  // gallon (U.S.) (gal)
    ("u.pound", "4.535924e-01", false),   // pound (avoirdupois) (lb)
    ("u.ounce", "2.834952e-02", false),   // ounce (avoirdupois) (oz)
    ("u.pound_force", "4.448222e+00", false), // pound-force (lbf)
    ("u.knot", "5.144444e-01", false),    // knot (nautical mile per hour)
    ("u.psi", "6.894757e+03", false),     // pound-force per square inch (psi) (lbf/in^2)
    ("u.torr", "1.333224e+02", false),    // torr (Torr)
    ("u.mmHg", "1.333224e+02", false),    // millimeter of mercury, conventional (mmHg)
    ("u.calorie", "4.184e+00", true),     // calorie_th (cal_th)
    ("u.kilocalorie", "4.184e+03", true), // kilocalorie_th (kcal_th)
    ("u.btu", "1.055056e+03", false),     // British thermal unit_IT(Btu_IT)
    ("u.horsepower", "7.456999e+02", false), // horsepower (550 ft · lbf/s) (hp)
    ("uc.fermi", "1.0e-15", true),        // fermi
    ("uc.micron", "1.0e-06", true),       // micron (μ)
    ("uc.mil", "2.54e-05", true),         // mil (0.001 in)
    ("uc.microinch", "2.54e-08", true),   // microinch
    ("uc.point", "3.527778e-04", false),  // point (computer) (1/72 in)
    ("uc.pica", "4.233333e-03", false),   // pica (computer) (1/6 in)
    ("uc.printers_point", "3.514598e-04", false), // point (printer's)
    ("uc.printers_pica", "4.217518e-03", false), // pica (printer's)
    ("uc.survey_foot", "3.048006e-01", false), // foot (U.S. survey) (ft)
    ("uc.survey_mile", "1.609347e+03", false), // mile (based on U.S. survey foot) (mi)
    ("uc.chain", "2.011684e+01", false),  // chain (based on U.S. survey foot) (ch)
    ("uc.rod", "5.029210e+00", false),    // rod (based on U.S. survey foot) (rd)
    ("uc.fathom", "1.828804e+00", false), // fathom (based on U.S. survey foot)
    ("uc.are", "1.0e+02", true),          // are (a)
    ("uc.barn", "1.0e-28", true),         // barn (b)
    ("uc.circular_mil", "5.067075e-10", false), // circular mil
    ("uc.survey_acre", "4.046873e+03", false), // acre (based on U.S. survey foot)
    ("uc.darcy", "9.869233e-13", false),  // darcy
    ("uc.stere", "1.0e+00", true),        // stere (st)
    ("uc.acre_foot", "1.233489e+03", false), // acre foot (based on U.S. survey foot)
    ("uc.barrel", "1.589873e-01", false), // barrel [for petroleum, 42 gallons (U.S.)] (bbl)
    ("uc.quart", "9.463529e-04", false),  // quart (U.S. liquid) (liq qt)
    ("uc.pint", "4.731765e-04", false),   // pint (U.S. liquid) (liq pt)
    ("uc.cup", "2.365882e-04", false),    // cup (U.S.)
    ("uc.gill", "1.182941e-04", false),   // gill (U.S.) (gi)
    ("uc.fluid_ounce", "2.957353e-05", false), // ounce (U.S. fluid) (fl oz)
    ("uc.tablespoon", "1.478676e-05", false), // tablespoon
    ("uc.teaspoon", "4.928922e-06", false), // teaspoon
    ("uc.bushel", "3.523907e-02", false), // bushel (U.S.) (bu)
    ("uc.peck", "8.809768e-03", false),   // peck (U.S.) (pk)
    ("uc.dry_quart", "1.101221e-03", false), // quart (U.S. dry) (dry qt)
    ("uc.dry_pint", "5.506105e-04", false), // pint (U.S. dry) (dry pt)
    ("uc.imperial_gallon", "4.54609e-03", true), // gallon [Canadian and U.K. (Imperial)] (gal)
    ("uc.imperial_gill", "1.420653e-04", false), // gill [Canadian and U.K. (Imperial)] (gi)
    ("uc.imperial_fluid_ounce", "2.841306e-05", false), // ounce [Canadian and U.K. fluid (Imperial)] (fl oz)
    ("uc.cord", "3.624556e+00", false),                 // cord (128 ft^3)
    ("uc.register_ton", "2.831685e+00", false),         // ton, register
    ("uc.carat", "2.0e-04", true),                      // carat, metric
    ("uc.grain", "6.479891e-05", true),                 // grain (gr)
    ("uc.pennyweight", "1.555174e-03", false),          // pennyweight (dwt)
    ("uc.troy_ounce", "3.110348e-02", false),           // ounce (troy or apothecary) (oz)
    ("uc.troy_pound", "3.732417e-01", false),           // pound (troy or apothecary) (lb)
    ("uc.short_hundredweight", "4.535924e+01", false),  // hundredweight (short, 100 lb)
    ("uc.long_hundredweight", "5.080235e+01", false),   // hundredweight (long, 112 lb)
    ("uc.short_ton", "9.071847e+02", false),            // ton, short (2000 lb)
    ("uc.long_ton", "1.016047e+03", false),             // ton, long (2240 lb)
    ("uc.assay_ton", "2.916667e-02", false),            // ton, assay (AT)
    ("uc.slug", "1.459390e+01", false),                 // slug (slug)
    ("uc.shake", "1.0e-08", true),                      // shake
    ("uc.sidereal_day", "8.616409e+04", false),         // day (sidereal)
    ("uc.sidereal_hour", "3.590170e+03", false),        // hour (sidereal)
    ("uc.sidereal_minute", "5.983617e+01", false),      // minute (sidereal)
    ("uc.sidereal_second", "9.972696e-01", false),      // second (sidereal)
    ("uc.sidereal_year", "3.155815e+07", false),        // year (sidereal)
    ("uc.tropical_year", "3.155693e+07", false),        // year (tropical)
    ("uc.dyne", "1.0e-05", true),                       // dyne (dyn)
    ("uc.kilogram_force", "9.80665e+00", true),         // kilogram-force (kgf)
    ("uc.ounce_force", "2.780139e-01", false),          // ounce (avoirdupois)-force (ozf)
    ("uc.poundal", "1.382550e-01", false),              // poundal
    ("uc.kip", "4.448222e+03", false),                  // kip (1 kip= 1000 lbf)
    ("uc.ton_force", "8.896443e+03", false),            // ton-force (2000 lbf)
    ("uc.technical_atmosphere", "9.80665e+04", true),   // atmosphere, technical (at)
    ("uc.ksi", "6.894757e+06", false),                  // kip per square inch (ksi) (kip/in^2)
    ("uc.cmHg", "1.333224e+03", false), // centimeter of mercury, conventional (cmHg)
    ("uc.inHg", "3.386389e+03", false), // inch of mercury, conventional (inHg)
    ("uc.ftHg", "4.063666e+04", false), // foot of mercury, conventional (ftHg)
    ("uc.mmH2O", "9.80665e+00", true),  // millimeter of water, conventional (mm H_2O)
    ("uc.cmH2O", "9.80665e+01", true),  // centimeter of water, conventional (cmH_2O)
    ("uc.inH2O", "2.490889e+02", false), // inch of water, conventional (inH_2O)
    ("uc.ftH2O", "2.989067e+03", false), // foot of water, conventional (ftH_2O)
    ("uc.erg", "1.0e-07", true),        // erg (erg)
    ("uc.calorie_IT", "4.1868e+00", true), // calorie_IT (cal_IT)
    ("uc.kilocalorie_IT", "4.1868e+03", true), // kilocalorie_IT (kcal_IT)
    ("uc.btu_th", "1.054350e+03", false), // British thermal unit_th(Btu_th)
    ("uc.therm_EC", "1.05506e+08", true), // therm (EC)
    ("uc.therm_US", "1.054804e+08", true), // therm (U.S.)
    ("uc.quad", "1.055056e+18", false), // quad (10^15 Btu_IT)
    ("uc.ton_of_tnt", "4.184e+09", true), // ton of TNT (energy equivalent)
    ("uc.horsepower_electric", "7.46e+02", true), // horsepower (electric)
    ("uc.horsepower_metric", "7.354988e+02", false), // horsepower (metric)
    ("uc.ton_of_refrigeration", "3.516853e+03", false), // ton of refrigeration (12 000 Btu_IT/h)
    ("uc.poise", "1.0e-01", true),      // poise (P)
    ("uc.centipoise", "1.0e-03", true), // centipoise (cP)
    ("uc.rhe", "1.0e+01", true),        // rhe
    ("uc.stokes", "1.0e-04", true),     // stokes (St)
    ("uc.centistokes", "1.0e-06", true), // centistokes (cSt)
    ("uc.ampere_hour", "3.6e+03", true), // ampere hour (A · h)
    ("uc.faraday", "9.648531e+04", false), // faraday (based on carbon 12)
    ("uc.mho", "1.0e+00", true),        // mho
    ("uc.debye", "3.335641e-30", false), // debye (D)
    ("uc.gauss", "1.0e-04", true),      // gauss (Gs, G)
    ("uc.gamma", "1.0e-09", true),      // gamma (γ)
    ("uc.maxwell", "1.0e-08", true),    // maxwell (Mx)
    ("uc.oersted", "7.957747e+01", false), // oersted (Oe)
    ("uc.curie", "3.7e+10", true),      // curie (Ci)
    ("uc.rad_dose", "1.0e-02", true),   // rad (absorbed dose) (rad)
    ("uc.rem", "1.0e-02", true),        // rem (rem)
    ("uc.roentgen", "2.58e-04", true),  // roentgen (R)
    ("uc.footcandle", "1.076391e+01", false), // footcandle
    ("uc.phot", "1.0e+04", true),       // phot (ph)
    ("uc.stilb", "1.0e+04", true),      // stilb (sb)
    ("uc.lambert", "3.183099e+03", false), // lambert
    ("uc.footlambert", "3.426259e+00", false), // footlambert
    ("uc.revolution", "6.283185e+00", false), // revolution (r)
    ("uc.gon", "1.570796e-02", false),  // gon (also called grade) (gon)
    ("uc.angular_mil", "9.817477e-04", false), // mil (angle)
    ("uc.gal", "1.0e-02", true),        // gal (Gal)
    ("uc.kayser", "1.0e+02", true),     // kayser (K)
    ("uc.tex", "1.0e-06", true),        // tex
    ("uc.denier", "1.111111e-07", false), // denier
];

/// SP 811 (2008) took the faraday from CODATA 2006; the 2019 SI fixes it as
/// e·N_A, which differs in the seventh digit.
const CODATA_2006: &[(&str, f64)] = &[("uc.faraday", 3e-6)];

#[test]
fn named_units_match_sp811_factors() {
    let got = values(SP811_FACTORS, |(name, _, _)| format!("{name}.get()"));
    let mut failures = Vec::new();
    for ((name, factor, exact), value) in SP811_FACTORS.iter().zip(got) {
        let expected: f64 = factor.parse().unwrap();
        let tolerance = if *exact {
            // Definitions built from several exact decimals can round.
            4.0 * f64::EPSILON * expected.abs()
        } else if let Some((_, relative)) = CODATA_2006.iter().find(|(n, _)| n == name) {
            relative * expected.abs()
        } else {
            rounding_tolerance(factor)
        };
        if (value - expected).abs() > tolerance {
            failures.push(format!("{name} = {value:e}, SP 811 gives {factor}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const PREFIXES: &[(&str, i32)] = &[
    ("q", -30),
    ("r", -27),
    ("y", -24),
    ("z", -21),
    ("a", -18),
    ("f", -15),
    ("p", -12),
    ("n", -9),
    ("u", -6),
    ("m", -3),
    ("c", -2),
    ("d", -1),
    ("da", 1),
    ("h", 2),
    ("k", 3),
    ("M", 6),
    ("G", 9),
    ("T", 12),
    ("P", 15),
    ("E", 18),
    ("Z", 21),
    ("Y", 24),
    ("R", 27),
    ("Q", 30),
];

/// The SI base units (the gram standing in for the kilogram) and the coherent
/// derived units with special names, except the steradian and degree Celsius.
const SI_UNITS: &[&str] = &[
    "g", "m", "s", "A", "K", "mol", "cd", "rad", "Hz", "N", "Pa", "J", "W", "C", "V", "F", "ohm",
    "S", "Wb", "T", "H", "lm", "lx", "Bq", "Gy", "Sv", "kat",
];

#[test]
fn catalogue_has_every_prefix_on_every_si_unit() {
    let mut cases = Vec::new();
    for unit in SI_UNITS {
        for (prefix, power) in PREFIXES {
            let name = match format!("{prefix}{unit}").as_str() {
                "as" => "attosecond".to_string(),
                "mohm" => "milliohm".to_string(),
                other => other.to_string(),
            };
            // `.to` requires the unit's own dimension.
            let (coherent, power) = if *unit == "g" {
                ("kg", power - 3)
            } else {
                (*unit, *power)
            };
            cases.push((name, coherent, power));
        }
    }
    let got = values(&cases, |(name, coherent, _)| {
        format!("uc.{name}.to(u.{coherent})")
    });
    for ((name, _, power), value) in cases.iter().zip(got) {
        let expected = 10f64.powi(*power);
        assert!(
            (value - expected).abs() <= f64::EPSILON * expected,
            "uc.{name} = {value:e}, expected 1e{power}"
        );
    }
}
