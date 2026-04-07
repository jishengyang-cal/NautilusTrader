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

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Coinbase environment type.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.coinbase",
        eq,
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.coinbase")
)]
pub enum CoinbaseEnvironment {
    /// Production environment.
    #[default]
    Live,
    /// Sandbox/testing environment.
    Sandbox,
}

impl CoinbaseEnvironment {
    /// Returns true if this is the sandbox environment.
    #[must_use]
    pub const fn is_sandbox(self) -> bool {
        matches!(self, Self::Sandbox)
    }
}

/// Coinbase product type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, EnumIter,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseProductType {
    Spot,
    Future,
}

/// Coinbase order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseOrderSide {
    Buy,
    Sell,
    #[serde(rename = "UNKNOWN_ORDER_SIDE")]
    #[strum(serialize = "UNKNOWN_ORDER_SIDE")]
    Unknown,
}

/// Coinbase order type used in create order requests.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseOrderType {
    #[serde(rename = "UNKNOWN_ORDER_TYPE")]
    #[strum(serialize = "UNKNOWN_ORDER_TYPE")]
    Unknown,
    Market,
    Limit,
    Stop,
    StopLimit,
    Bracket,
}

/// Coinbase order status.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseOrderStatus {
    Pending,
    Open,
    Filled,
    Cancelled,
    Expired,
    Failed,
    #[serde(rename = "UNKNOWN_ORDER_STATUS")]
    #[strum(serialize = "UNKNOWN_ORDER_STATUS")]
    Unknown,
    Queued,
    CancelQueued,
}

/// Coinbase time in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseTimeInForce {
    #[serde(rename = "UNKNOWN_TIME_IN_FORCE")]
    #[strum(serialize = "UNKNOWN_TIME_IN_FORCE")]
    Unknown,
    GoodUntilDate,
    GoodUntilCancelled,
    ImmediateOrCancel,
    FillOrKill,
}

/// Coinbase product status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CoinbaseProductStatus {
    Online,
    Offline,
    /// Futures products return an empty status string
    #[serde(rename = "")]
    #[strum(serialize = "")]
    Unset,
}

/// Coinbase product venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseProductVenue {
    /// Coinbase Exchange (spot).
    Cbe,
    /// Futures Commission Merchant (futures/perpetuals).
    Fcm,
}

/// Coinbase contract expiry type for futures products.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseContractExpiryType {
    Expiring,
    /// Non-expiring (perpetual)
    #[serde(rename = "PERPETUAL")]
    #[strum(serialize = "PERPETUAL")]
    Perpetual,
}

/// Coinbase futures asset type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
pub enum CoinbaseFuturesAssetType {
    #[serde(rename = "FUTURES_ASSET_TYPE_CRYPTO")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_CRYPTO")]
    Crypto,
    #[serde(rename = "FUTURES_ASSET_TYPE_ENERGY")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_ENERGY")]
    Energy,
    #[serde(rename = "FUTURES_ASSET_TYPE_METALS")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_METALS")]
    Metals,
    #[serde(rename = "FUTURES_ASSET_TYPE_STOCKS")]
    #[strum(serialize = "FUTURES_ASSET_TYPE_STOCKS")]
    Stocks,
}

/// Coinbase fill liquidity indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseLiquidityIndicator {
    Maker,
    Taker,
    Unknown,
}

/// Coinbase candle granularity for historical data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum CoinbaseGranularity {
    OneMinute,
    FiveMinute,
    FifteenMinute,
    ThirtyMinute,
    OneHour,
    TwoHour,
    SixHour,
    OneDay,
}

/// Coinbase WebSocket channel type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString, AsRefStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CoinbaseWsChannel {
    Level2,
    MarketTrades,
    Ticker,
    TickerBatch,
    Candles,
    User,
    Heartbeats,
    FuturesBalanceSummary,
    Status,
}

impl CoinbaseWsChannel {
    /// Returns true if this channel requires authentication.
    pub fn requires_auth(&self) -> bool {
        matches!(self, Self::User | Self::FuturesBalanceSummary)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(CoinbaseProductType::Spot, "SPOT")]
    #[case(CoinbaseProductType::Future, "FUTURE")]
    fn test_product_type_display(#[case] variant: CoinbaseProductType, #[case] expected: &str) {
        assert_eq!(variant.to_string(), expected);
    }

    #[rstest]
    #[case("BUY", CoinbaseOrderSide::Buy)]
    #[case("SELL", CoinbaseOrderSide::Sell)]
    fn test_order_side_from_str(#[case] input: &str, #[case] expected: CoinbaseOrderSide) {
        assert_eq!(CoinbaseOrderSide::from_str(input).unwrap(), expected);
    }

    #[rstest]
    fn test_ws_channel_requires_auth() {
        assert!(CoinbaseWsChannel::User.requires_auth());
        assert!(CoinbaseWsChannel::FuturesBalanceSummary.requires_auth());
        assert!(!CoinbaseWsChannel::Level2.requires_auth());
        assert!(!CoinbaseWsChannel::MarketTrades.requires_auth());
        assert!(!CoinbaseWsChannel::Ticker.requires_auth());
    }

    #[rstest]
    fn test_order_status_serialization() {
        let status = CoinbaseOrderStatus::Open;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"OPEN\"");

        let deserialized: CoinbaseOrderStatus = serde_json::from_str("\"CANCELLED\"").unwrap();
        assert_eq!(deserialized, CoinbaseOrderStatus::Cancelled);
    }

    #[rstest]
    fn test_screaming_snake_case_multi_word() {
        let json = serde_json::to_string(&CoinbaseOrderType::StopLimit).unwrap();
        assert_eq!(json, "\"STOP_LIMIT\"");

        let json = serde_json::to_string(&CoinbaseOrderStatus::CancelQueued).unwrap();
        assert_eq!(json, "\"CANCEL_QUEUED\"");

        let json = serde_json::to_string(&CoinbaseTimeInForce::GoodUntilDate).unwrap();
        assert_eq!(json, "\"GOOD_UNTIL_DATE\"");

        let json = serde_json::to_string(&CoinbaseGranularity::FifteenMinute).unwrap();
        assert_eq!(json, "\"FIFTEEN_MINUTE\"");
    }

    #[rstest]
    fn test_ws_channel_snake_case() {
        let json = serde_json::to_string(&CoinbaseWsChannel::Level2).unwrap();
        assert_eq!(json, "\"level2\"");

        let json = serde_json::to_string(&CoinbaseWsChannel::MarketTrades).unwrap();
        assert_eq!(json, "\"market_trades\"");

        let json = serde_json::to_string(&CoinbaseWsChannel::FuturesBalanceSummary).unwrap();
        assert_eq!(json, "\"futures_balance_summary\"");
    }

    #[rstest]
    fn test_unknown_variants_have_qualified_names() {
        let json = serde_json::to_string(&CoinbaseOrderSide::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_ORDER_SIDE\"");

        let json = serde_json::to_string(&CoinbaseOrderType::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_ORDER_TYPE\"");

        let json = serde_json::to_string(&CoinbaseOrderStatus::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_ORDER_STATUS\"");

        let json = serde_json::to_string(&CoinbaseTimeInForce::Unknown).unwrap();
        assert_eq!(json, "\"UNKNOWN_TIME_IN_FORCE\"");
    }
}
