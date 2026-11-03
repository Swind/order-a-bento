//! JSON amounts remain TWD dollars; Rust/SQLite amounts are integer cents.
//! Parse the JSON number's decimal spelling, never through floating-point arithmetic.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use serde_json::{Number, value::RawValue};

pub fn parse(raw: &str) -> Result<i64, &'static str> {
    let (coefficient, exponent) = match raw.split_once(['e', 'E']) {
        Some((c, e)) => (c, e.parse::<i32>().map_err(|_| "金額超出範圍")?),
        None => (raw, 0),
    };
    let negative = coefficient.starts_with('-');
    let coefficient = coefficient.strip_prefix('-').unwrap_or(coefficient);
    let (whole, fraction) = coefficient.split_once('.').unwrap_or((coefficient, ""));
    if whole.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return Err("金額格式不正確");
    }
    let shift = 2_i32
        .checked_add(exponent)
        .and_then(|v| v.checked_sub(fraction.len().try_into().ok()?))
        .ok_or("金額超出範圍")?;
    if shift < 0 {
        return Err("金額最多小數兩位");
    }
    if shift > 18 {
        return Err("金額超出範圍");
    }
    let digits = format!("{whole}{fraction}");
    let magnitude = digits.parse::<i64>().map_err(|_| "金額超出範圍")?;
    let cents = magnitude
        .checked_mul(10_i64.checked_pow(shift as u32).ok_or("金額超出範圍")?)
        .ok_or("金額超出範圍")?;
    if negative {
        cents.checked_neg().ok_or("金額超出範圍")
    } else {
        Ok(cents)
    }
}

pub fn text(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let n = cents.unsigned_abs();
    let fraction = n % 100;
    if fraction == 0 {
        format!("{sign}{}", n / 100)
    } else if fraction.is_multiple_of(10) {
        format!("{sign}{}.{}", n / 100, fraction / 10)
    } else {
        format!("{sign}{}.{fraction:02}", n / 100)
    }
}

pub fn serialize<S: Serializer>(cents: &i64, serializer: S) -> Result<S::Ok, S::Error> {
    RawValue::from_string(text(*cents))
        .map_err(serde::ser::Error::custom)?
        .serialize(serializer)
}
pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    parse(&Number::deserialize(d)?.to_string()).map_err(D::Error::custom)
}
pub mod optional {
    use super::*;
    pub fn serialize<S: Serializer>(cents: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
        cents
            .map(|v| RawValue::from_string(text(v)))
            .transpose()
            .map_err(serde::ser::Error::custom)?
            .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
        Option::<Number>::deserialize(d)?
            .map(|n| parse(&n.to_string()).map_err(D::Error::custom))
            .transpose()
    }
}

/// Validate original numeric spellings before serde's tagged-enum buffering normalizes them.
pub fn validate_prices(json: &str) -> Result<(), String> {
    validate_at(json, 0)
}
fn validate_at(json: &str, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("JSON nesting too deep".into());
    }
    if json.trim_start().starts_with('{') {
        let fields: std::collections::BTreeMap<String, Box<RawValue>> =
            serde_json::from_str(json).map_err(|e| e.to_string())?;
        for (key, value) in fields {
            if key == "price" && value.get() != "null" {
                parse(value.get()).map_err(str::to_string)?;
            }
            validate_at(value.get(), depth + 1)?;
        }
    } else if json.trim_start().starts_with('[') {
        let values: Vec<Box<RawValue>> = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for value in values {
            validate_at(value.get(), depth + 1)?;
        }
    }
    Ok(())
}

pub mod raw_optional {
    use super::*;
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
        Option::<Box<RawValue>>::deserialize(d)?
            .map(|n| parse(n.get()).map_err(D::Error::custom))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimal_boundary_and_format() {
        for (s, cents) in [
            ("100", 10000),
            ("7.5", 750),
            ("7.50", 750),
            ("0.01", 1),
            ("-22.5", -2250),
            ("1e2", 10000),
        ] {
            assert_eq!(parse(s), Ok(cents));
            assert_eq!(parse(&text(cents)), Ok(cents));
        }
        for s in [
            "NaN",
            "inf",
            "7.501",
            "1.23000000000000000001",
            "92233720368547758.08",
            "1e999",
            "1e-3",
        ] {
            assert!(parse(s).is_err(), "{s}");
        }
        assert_eq!(text(2250), "22.5");
        assert_eq!(text(751), "7.51");
    }
    #[test]
    fn serde_keeps_legacy_dollar_contract_and_validates_raw_precision() {
        #[derive(Serialize, Deserialize)]
        struct Price {
            #[serde(with = "super::super::amount")]
            price: i64,
        }
        let legacy: Price = serde_json::from_str(r#"{"price":100}"#).unwrap();
        assert_eq!(legacy.price, 10000);
        assert_eq!(serde_json::to_string(&legacy).unwrap(), r#"{"price":100}"#);
        let decimal: Price = serde_json::from_str(r#"{"price":7.5}"#).unwrap();
        assert_eq!(decimal.price, 750);
        assert_eq!(serde_json::to_string(&decimal).unwrap(), r#"{"price":7.5}"#);
        assert!(
            validate_prices(r#"{"accepted":[{"menu":[{"price":7.5000000000000001}]}]}"#).is_err()
        );
        assert!(validate_prices(r#"{"price":7.50}"#).is_ok());
    }
}
