// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Conversion helpers that translate Architect API schemas into Nautilus types.

use std::str::FromStr;

use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer};

/// Deserializes an optional Decimal from a string field.
///
/// Returns `None` if the string is empty or "0", otherwise parses to `Decimal`.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_optional_decimal<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    if s.is_empty() || s == "0" {
        Ok(None)
    } else {
        Decimal::from_str(&s)
            .map(Some)
            .map_err(serde::de::Error::custom)
    }
}

/// Deserializes a Decimal from an optional string field, defaulting to zero.
///
/// Handles edge cases: None, empty string "", or "0" all become Decimal::ZERO.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_optional_decimal_or_zero<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Deserialize::deserialize(deserializer)?;
    match opt {
        None => Ok(Decimal::ZERO),
        Some(s) if s.is_empty() || s == "0" => Ok(Decimal::ZERO),
        Some(s) => Decimal::from_str(&s).map_err(serde::de::Error::custom),
    }
}

/// Deserializes a Decimal from a string field that might be empty.
///
/// Handles edge case where empty string "" becomes Decimal::ZERO.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a valid decimal.
pub fn deserialize_decimal_or_zero<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    if s.is_empty() || s == "0" {
        Ok(Decimal::ZERO)
    } else {
        Decimal::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// Parses a string to Decimal, returning an error if parsing fails.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a Decimal.
pub fn parse_decimal(s: &str) -> anyhow::Result<Decimal> {
    Decimal::from_str(s).map_err(|e| anyhow::anyhow!("Failed to parse decimal from '{s}': {e}"))
}

/// Parses an optional string to Decimal, returning None if the string is None or empty.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed as a Decimal.
pub fn parse_optional_decimal(s: &Option<String>) -> anyhow::Result<Option<Decimal>> {
    match s {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => parse_decimal(s).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json;

    use super::*;

    #[derive(Deserialize)]
    struct TestOptionalDecimal {
        #[serde(deserialize_with = "deserialize_optional_decimal")]
        value: Option<Decimal>,
    }

    #[derive(Deserialize)]
    struct TestDecimalOrZero {
        #[serde(deserialize_with = "deserialize_decimal_or_zero")]
        value: Decimal,
    }

    #[derive(Deserialize)]
    struct TestOptionalDecimalOrZero {
        #[serde(deserialize_with = "deserialize_optional_decimal_or_zero")]
        value: Decimal,
    }

    #[rstest]
    #[case(r#"{"value":"123.45"}"#, Some(Decimal::new(12345, 2)))]
    #[case(r#"{"value":"0"}"#, None)]
    #[case(r#"{"value":""}"#, None)]
    fn test_deserialize_optional_decimal(#[case] json: &str, #[case] expected: Option<Decimal>) {
        let result: TestOptionalDecimal = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value":"123.45"}"#, Decimal::new(12345, 2))]
    #[case(r#"{"value":"0"}"#, Decimal::ZERO)]
    #[case(r#"{"value":""}"#, Decimal::ZERO)]
    fn test_deserialize_decimal_or_zero(#[case] json: &str, #[case] expected: Decimal) {
        let result: TestDecimalOrZero = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case(r#"{"value":"123.45"}"#, Decimal::new(12345, 2))]
    #[case(r#"{"value":"0"}"#, Decimal::ZERO)]
    #[case(r#"{"value":null}"#, Decimal::ZERO)]
    fn test_deserialize_optional_decimal_or_zero(#[case] json: &str, #[case] expected: Decimal) {
        let result: TestOptionalDecimalOrZero = serde_json::from_str(json).unwrap();
        assert_eq!(result.value, expected);
    }

    #[rstest]
    #[case("123.45", Decimal::new(12345, 2))]
    #[case("0", Decimal::ZERO)]
    #[case("0.0", Decimal::ZERO)]
    fn test_parse_decimal(#[case] input: &str, #[case] expected: Decimal) {
        let result = parse_decimal(input).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_parse_decimal_invalid() {
        assert!(parse_decimal("invalid").is_err());
        assert!(parse_decimal("").is_err());
    }

    #[rstest]
    #[case(&Some("123.45".to_string()), Some(Decimal::new(12345, 2)))]
    #[case(&Some("0".to_string()), Some(Decimal::ZERO))]
    #[case(&Some(String::new()), None)]
    #[case(&None, None)]
    fn test_parse_optional_decimal(
        #[case] input: &Option<String>,
        #[case] expected: Option<Decimal>,
    ) {
        let result = parse_optional_decimal(input).unwrap();
        assert_eq!(result, expected);
    }
}
