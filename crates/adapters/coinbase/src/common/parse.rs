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

//! Common parsing utilities for the Coinbase adapter.

use nautilus_model::{
    data::BarType,
    enums::{AggregationSource, BarAggregation},
};

use crate::common::enums::CoinbaseGranularity;

/// Converts a Nautilus [`BarType`] to a [`CoinbaseGranularity`].
///
/// # Errors
///
/// Returns an error if the bar type uses an unsupported aggregation or step value.
pub fn bar_type_to_granularity(bar_type: &BarType) -> anyhow::Result<CoinbaseGranularity> {
    let spec = bar_type.spec();

    anyhow::ensure!(
        bar_type.aggregation_source() == AggregationSource::External,
        "Only EXTERNAL aggregation is supported"
    );

    let step = spec.step.get();

    match spec.aggregation {
        BarAggregation::Minute => match step {
            1 => Ok(CoinbaseGranularity::OneMinute),
            5 => Ok(CoinbaseGranularity::FiveMinute),
            15 => Ok(CoinbaseGranularity::FifteenMinute),
            30 => Ok(CoinbaseGranularity::ThirtyMinute),
            _ => anyhow::bail!("Unsupported minute step: {step}"),
        },
        BarAggregation::Hour => match step {
            1 => Ok(CoinbaseGranularity::OneHour),
            2 => Ok(CoinbaseGranularity::TwoHour),
            6 => Ok(CoinbaseGranularity::SixHour),
            _ => anyhow::bail!("Unsupported hour step: {step}"),
        },
        BarAggregation::Day => match step {
            1 => Ok(CoinbaseGranularity::OneDay),
            _ => anyhow::bail!("Unsupported day step: {step}"),
        },
        other => anyhow::bail!("Unsupported aggregation: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        "BTC-USD.COINBASE-1-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::OneMinute
    )]
    #[case(
        "BTC-USD.COINBASE-5-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::FiveMinute
    )]
    #[case(
        "BTC-USD.COINBASE-15-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::FifteenMinute
    )]
    #[case(
        "BTC-USD.COINBASE-30-MINUTE-LAST-EXTERNAL",
        CoinbaseGranularity::ThirtyMinute
    )]
    #[case("BTC-USD.COINBASE-1-HOUR-LAST-EXTERNAL", CoinbaseGranularity::OneHour)]
    #[case("BTC-USD.COINBASE-2-HOUR-LAST-EXTERNAL", CoinbaseGranularity::TwoHour)]
    #[case("BTC-USD.COINBASE-6-HOUR-LAST-EXTERNAL", CoinbaseGranularity::SixHour)]
    #[case("BTC-USD.COINBASE-1-DAY-LAST-EXTERNAL", CoinbaseGranularity::OneDay)]
    fn test_bar_type_to_granularity(
        #[case] bar_type_str: &str,
        #[case] expected: CoinbaseGranularity,
    ) {
        let bar_type = BarType::from(bar_type_str);
        let result = bar_type_to_granularity(&bar_type).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("BTC-USD.COINBASE-3-MINUTE-LAST-EXTERNAL")]
    #[case("BTC-USD.COINBASE-4-HOUR-LAST-EXTERNAL")]
    #[case("BTC-USD.COINBASE-2-DAY-LAST-EXTERNAL")]
    fn test_bar_type_to_granularity_unsupported(#[case] bar_type_str: &str) {
        let bar_type = BarType::from(bar_type_str);
        assert!(bar_type_to_granularity(&bar_type).is_err());
    }
}
