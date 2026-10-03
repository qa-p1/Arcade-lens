//! Physical quantities and local unit conversion.

use std::sync::LazyLock;

use lens_core::value::QuantityValue;
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Length,
    Area,
    Volume,
    Mass,
    Temperature,
    Speed,
    Data,
}

impl Dimension {
    fn name(&self) -> &'static str {
        match self {
            Dimension::Length => "length",
            Dimension::Area => "area",
            Dimension::Volume => "volume",
            Dimension::Mass => "mass",
            Dimension::Temperature => "temperature",
            Dimension::Speed => "speed",
            Dimension::Data => "data size",
        }
    }
}

pub struct Unit {
    pub symbol: &'static str,
    pub dimension: Dimension,
    /// Value in the dimension's base unit = amount × factor (+ offset for temperature).
    pub factor: f64,
    pub offset: f64,
    /// Accepted spellings. Matched case-sensitively unless `fold` is set.
    pub aliases: &'static [&'static str],
    pub fold: bool,
    /// Shown as a conversion target.
    pub target: bool,
}

const fn u(symbol: &'static str, dimension: Dimension, factor: f64, aliases: &'static [&'static str], fold: bool, target: bool) -> Unit {
    Unit { symbol, dimension, factor, offset: 0.0, aliases, fold, target }
}

use Dimension::*;

/// Base units: metre, square metre, litre, kilogram, kelvin, metre/second, byte.
pub static UNITS: &[Unit] = &[
    u("mm", Length, 0.001, &["mm", "millimeter", "millimeters", "millimetre", "millimetres"], true, true),
    u("cm", Length, 0.01, &["cm", "centimeter", "centimeters", "centimetre", "centimetres"], true, true),
    u("m", Length, 1.0, &["m", "meter", "meters", "metre", "metres"], false, true),
    u("km", Length, 1000.0, &["km", "kilometer", "kilometers", "kilometre", "kilometres"], true, true),
    u("in", Length, 0.0254, &["in", "inch", "inches", "\"", "″"], true, true),
    u("ft", Length, 0.3048, &["ft", "foot", "feet", "′"], true, true),
    u("yd", Length, 0.9144, &["yd", "yard", "yards"], true, false),
    u("mi", Length, 1609.344, &["mi", "mile", "miles"], true, true),
    u("m²", Area, 1.0, &["m²", "m2", "sq m", "sqm", "square meters", "square metres"], true, true),
    u("km²", Area, 1e6, &["km²", "km2", "sq km", "square kilometers", "square kilometres"], true, true),
    u("ft²", Area, 0.09290304, &["ft²", "ft2", "sq ft", "sq. ft.", "sqft", "square feet"], true, true),
    u("acre", Area, 4046.8564224, &["acre", "acres", "ac"], true, true),
    u("ha", Area, 10_000.0, &["ha", "hectare", "hectares"], true, true),
    u("ml", Volume, 0.001, &["ml", "mL", "milliliter", "milliliters", "millilitre", "millilitres"], true, true),
    u("L", Volume, 1.0, &["l", "L", "liter", "liters", "litre", "litres"], false, true),
    u("gal", Volume, 3.785411784, &["gal", "gallon", "gallons"], true, true),
    u("fl oz", Volume, 0.0295735295625, &["fl oz", "fl. oz.", "fl. oz", "fluid ounces"], true, true),
    u("cup", Volume, 0.2365882365, &["cup", "cups"], true, true),
    u("tbsp", Volume, 0.01478676478125, &["tbsp", "tablespoon", "tablespoons"], true, false),
    u("tsp", Volume, 0.00492892159375, &["tsp", "teaspoon", "teaspoons"], true, false),
    u("mg", Mass, 1e-6, &["mg", "milligram", "milligrams"], true, false),
    u("g", Mass, 0.001, &["g", "gram", "grams", "gm"], false, true),
    u("kg", Mass, 1.0, &["kg", "kgs", "kilogram", "kilograms", "kilo", "kilos"], true, true),
    u("lb", Mass, 0.45359237, &["lb", "lbs", "pound", "pounds"], true, true),
    u("oz", Mass, 0.028349523125, &["oz", "ounce", "ounces"], true, true),
    u("st", Mass, 6.35029318, &["stone"], true, false),
    Unit {
        symbol: "°C",
        dimension: Temperature,
        factor: 1.0,
        offset: 273.15,
        aliases: &["°C", "ºC", "° C", "degC", "celsius", "Celsius", "degrees celsius", "degrees Celsius"],
        fold: false,
        target: true,
    },
    Unit {
        symbol: "°F",
        dimension: Temperature,
        factor: 5.0 / 9.0,
        offset: 255.372_222_222_222_2,
        aliases: &["°F", "ºF", "° F", "degF", "fahrenheit", "Fahrenheit", "degrees fahrenheit", "degrees Fahrenheit"],
        fold: false,
        target: true,
    },
    Unit { symbol: "K", dimension: Temperature, factor: 1.0, offset: 0.0, aliases: &["kelvin", "Kelvin"], fold: false, target: true },
    u("km/h", Speed, 1.0 / 3.6, &["km/h", "kmh", "kph", "kmph", "km/hr"], true, true),
    u("mph", Speed, 0.44704, &["mph", "mi/h", "miles per hour"], true, true),
    u("m/s", Speed, 1.0, &["m/s", "meters per second", "metres per second"], true, true),
    u("kn", Speed, 0.514444, &["knots", "knot", "kn", "kt"], true, true),
    // Data: SI prefixes are decimal, IEC prefixes are binary. Case matters (Mb ≠ MB).
    u("B", Data, 1.0, &["B", "bytes", "byte"], false, false),
    u("KB", Data, 1e3, &["KB", "kB"], false, true),
    u("MB", Data, 1e6, &["MB"], false, true),
    u("GB", Data, 1e9, &["GB"], false, true),
    u("TB", Data, 1e12, &["TB"], false, true),
    u("KiB", Data, 1024.0, &["KiB"], false, true),
    u("MiB", Data, 1_048_576.0, &["MiB"], false, true),
    u("GiB", Data, 1_073_741_824.0, &["GiB"], false, true),
    u("TiB", Data, 1_099_511_627_776.0, &["TiB"], false, true),
    u("Mb", Data, 125_000.0, &["Mb", "Mbit", "megabits"], false, false),
    u("Gb", Data, 125_000_000.0, &["Gb", "Gbit", "gigabits"], false, false),
];

static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-+−]?\d+(?:[.,]\d+)?").unwrap());

/// Aliases sorted longest first so `fl oz` wins over `oz` and `km/h` over `km`.
static ALIASES: LazyLock<Vec<(&'static str, &'static Unit)>> = LazyLock::new(|| {
    let mut v: Vec<_> = UNITS.iter().flat_map(|u| u.aliases.iter().map(move |a| (*a, u))).collect();
    v.sort_by_key(|(a, _)| std::cmp::Reverse(a.len()));
    v
});

fn to_base(u: &Unit, x: f64) -> f64 {
    x * u.factor + u.offset
}

fn from_base(u: &Unit, x: f64) -> f64 {
    (x - u.offset) / u.factor
}

/// Formats with ~4 significant digits and no float noise.
pub fn format_number(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let magnitude = x.abs().log10().floor() as i32;
    let decimals = (3 - magnitude).clamp(0, 6) as usize;
    let s = format!("{x:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

/// Rough measurement system, used to prefer cross-system conversions
/// (metric → imperial and back; decimal ↔ binary data units).
fn system(symbol: &str) -> u8 {
    const IMPERIAL: &[&str] =
        &["in", "ft", "yd", "mi", "ft²", "acre", "gal", "fl oz", "cup", "tbsp", "tsp", "lb", "oz", "st", "°F", "mph", "KiB", "MiB", "GiB", "TiB"];
    match symbol {
        "K" | "kn" => 2,
        s if IMPERIAL.contains(&s) => 1,
        _ => 0,
    }
}

/// Up to six conversions, preferring readable magnitudes and the other
/// measurement system.
pub fn conversions(amount: f64, unit: &Unit) -> Vec<(String, String)> {
    let base = to_base(unit, amount);
    let mut targets: Vec<(usize, f64, &Unit)> = UNITS
        .iter()
        .enumerate()
        .filter(|(_, t)| t.dimension == unit.dimension && t.target && t.symbol != unit.symbol)
        .map(|(i, t)| (i, from_base(t, base), t))
        .filter(|(_, v, t)| t.dimension == Temperature || (v.abs() >= 0.01 && v.abs() < 1e7))
        .collect();
    let readable = |v: f64| (0.1..10_000.0).contains(&v.abs()) || v == 0.0;
    // Unreadable magnitudes sort by how far they are from a comfortable range.
    let badness = |v: f64, t: &Unit| if readable(v) || t.dimension == Temperature { 0 } else { (v.abs().log10() - 2.0).abs().round() as i64 };
    targets.sort_by_key(|(i, v, t)| (badness(*v, t), system(t.symbol) == system(unit.symbol), *i));
    targets.into_iter().take(6).map(|(_, v, t)| (format_number(v), t.symbol.to_string())).collect()
}

pub fn find_unit(symbol: &str) -> Option<&'static Unit> {
    UNITS.iter().find(|u| u.symbol == symbol)
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let text = input.text;
    let mut out = Vec::new();
    for m in NUMBER.find_iter(text) {
        // Skip digits glued to identifiers, versions, or currency.
        if text[..m.start()].ends_with(|c: char| c.is_alphanumeric() || "._$€£₹#".contains(c)) {
            continue;
        }
        let rest = &text[m.end()..];
        let gap = rest.len() - rest.trim_start_matches([' ', '\u{a0}']).len();
        let rest_trim = &rest[gap..];
        let found = ALIASES.iter().find(|(alias, unit)| {
            let hit = if unit.fold { rest_trim.get(..alias.len()).is_some_and(|p| p.eq_ignore_ascii_case(alias)) } else { rest_trim.starts_with(alias) };
            // Quote/prime units must be glued to the number: 14.7" not `14.7 "quoted"`.
            let glued_only = matches!(*alias, "\"" | "″" | "′");
            // Short aliases that are also words ("3 in production", "5 m daily")
            // only count when glued to the number or followed by punctuation.
            let wordlike = matches!(*alias, "in" | "m" | "g" | "l" | "L" | "ac" | "kn" | "kt" | "B" | "st" | "gm" | "cup" | "cups");
            let next_word = rest_trim.get(alias.len()..).unwrap_or_default().trim_start().starts_with(|c: char| c.is_alphanumeric());
            hit && (!glued_only || gap == 0)
                && (!wordlike || gap == 0 || !next_word)
                && !rest_trim[alias.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '/' && !alias.contains('/'))
        });
        let Some((alias, unit)) = found else { continue };
        let Ok(amount) = m.as_str().replace('−', "-").replace(',', ".").parse::<f64>() else { continue };
        let end = m.end() + gap + alias.len();
        let r = m.start()..end;
        let conv = conversions(amount, unit);
        let mut d = Detection::new(
            caps::QUANTITY,
            Value::Quantity(QuantityValue {
                raw: text[r.clone()].to_string(),
                amount,
                unit: unit.symbol.to_string(),
                dimension: unit.dimension.name().to_string(),
                conversions: conv.clone(),
            }),
        )
        .span(r)
        // Single-letter units in prose ("5 m", "3 g") are weaker evidence.
        .confidence(if alias.len() <= 1 { 0.6 } else { 0.9 });
        for (v, s) in conv.iter().take(3) {
            d = d.detail("=", format!("{v} {s}"));
        }
        out.push(d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;

    fn q(text: &str) -> Vec<(f64, String, Vec<String>)> {
        detect(&TextInput { text, layout: None }, &cx())
            .into_iter()
            .map(|d| match d.value {
                Value::Quantity(v) => (v.amount, v.unit, v.conversions.iter().map(|(a, b)| format!("{a} {b}")).collect()),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn lengths_and_screens() {
        let r = q("A 14.7\" display");
        assert_eq!(r[0].0, 14.7);
        assert_eq!(r[0].1, "in");
        assert!(r[0].2.contains(&"37.34 cm".to_string()), "{:?}", r[0].2);
        assert_eq!(q("14.7 inches")[0].1, "in");
        assert_eq!(q("runs 5 km daily")[0].2[..3], ["3.107 mi".to_string(), "5000 m".into(), "16404 ft".into()][..]);
        assert!(q("runs 5 m daily").is_empty());
        assert_eq!(q("a 5m cable")[0].1, "m");
    }

    #[test]
    fn temperatures() {
        let r = q("It was 30 °C");
        assert_eq!(r[0].2, vec!["86 °F", "303.1 K"]);
        let r = q("Bake at 350°F");
        assert_eq!(r[0].2[0], "176.7 °C");
    }

    #[test]
    fn data_is_case_sensitive() {
        assert_eq!(q("a 500 MB file")[0].1, "MB");
        assert_eq!(q("100 Mb link")[0].1, "Mb");
        assert!(q("1 GiB")[0].2.contains(&"1074 MB".to_string()));
    }

    #[test]
    fn longest_alias_wins() {
        assert_eq!(q("2 fl oz")[0].1, "fl oz");
        assert_eq!(q("100 km/h")[0].1, "km/h");
        assert_eq!(q("1200 sq ft")[0].1, "ft²");
    }

    #[test]
    fn non_quantities() {
        for t in ["v2 mobile", "Python 3 in production", "said \"hi\" 3 \"times\"", "page 5 of 10", "x86_64"] {
            assert!(q(t).is_empty(), "{t}: {:?}", q(t));
        }
    }

    #[test]
    fn number_formatting() {
        assert_eq!(format_number(37.338), "37.34");
        assert_eq!(format_number(0.000123), "0.000123");
        assert_eq!(format_number(16404.2), "16404");
    }
}
