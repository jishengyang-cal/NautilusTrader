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

//! Enumerations that model Architect string enums across HTTP and WebSocket payloads.

use nautilus_model::enums::{AggressorSide, OrderSide, OrderStatus, PositionSide, TimeInForce};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

use super::consts::{
    ARCHITECT_HTTP_SANDBOX_URL, ARCHITECT_HTTP_URL, ARCHITECT_ORDERS_SANDBOX_URL,
    ARCHITECT_ORDERS_URL, ARCHITECT_WS_PRIVATE_URL, ARCHITECT_WS_PUBLIC_URL,
    ARCHITECT_WS_SANDBOX_PRIVATE_URL, ARCHITECT_WS_SANDBOX_PUBLIC_URL,
};

/// Architect API environment.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectEnvironment {
    /// Sandbox/test environment.
    #[default]
    Sandbox,
    /// Production/live environment.
    Production,
}

impl ArchitectEnvironment {
    /// Returns the HTTP API base URL for this environment.
    #[must_use]
    pub const fn http_url(&self) -> &'static str {
        match self {
            Self::Sandbox => ARCHITECT_HTTP_SANDBOX_URL,
            Self::Production => ARCHITECT_HTTP_URL,
        }
    }

    /// Returns the Orders API base URL for this environment.
    #[must_use]
    pub const fn orders_url(&self) -> &'static str {
        match self {
            Self::Sandbox => ARCHITECT_ORDERS_SANDBOX_URL,
            Self::Production => ARCHITECT_ORDERS_URL,
        }
    }

    /// Returns the market data WebSocket URL for this environment.
    #[must_use]
    pub const fn ws_md_url(&self) -> &'static str {
        match self {
            Self::Sandbox => ARCHITECT_WS_SANDBOX_PUBLIC_URL,
            Self::Production => ARCHITECT_WS_PUBLIC_URL,
        }
    }

    /// Returns the orders WebSocket URL for this environment.
    #[must_use]
    pub const fn ws_orders_url(&self) -> &'static str {
        match self {
            Self::Sandbox => ARCHITECT_WS_SANDBOX_PRIVATE_URL,
            Self::Production => ARCHITECT_WS_PRIVATE_URL,
        }
    }
}

/// Instrument state as returned by the Architect API.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/symbols-instruments/get-instruments>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectInstrumentState {
    /// Instrument is in pre-open state.
    PreOpen,
    /// Instrument is open for trading.
    Open,
    /// Instrument trading is suspended.
    Suspended,
    /// Instrument has been delisted.
    Delisted,
    /// Instrument state is unknown.
    Unknown,
}

/// Order side for trading operations.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectOrderSide {
    /// Buy order.
    #[serde(rename = "B")]
    #[strum(serialize = "B")]
    Buy,
    /// Sell order.
    #[serde(rename = "S")]
    #[strum(serialize = "S")]
    Sell,
}

impl From<ArchitectOrderSide> for AggressorSide {
    fn from(side: ArchitectOrderSide) -> Self {
        match side {
            ArchitectOrderSide::Buy => Self::Buyer,
            ArchitectOrderSide::Sell => Self::Seller,
        }
    }
}

impl From<ArchitectOrderSide> for OrderSide {
    fn from(side: ArchitectOrderSide) -> Self {
        match side {
            ArchitectOrderSide::Buy => Self::Buy,
            ArchitectOrderSide::Sell => Self::Sell,
        }
    }
}

impl From<ArchitectOrderSide> for PositionSide {
    fn from(side: ArchitectOrderSide) -> Self {
        match side {
            ArchitectOrderSide::Buy => Self::Long,
            ArchitectOrderSide::Sell => Self::Short,
        }
    }
}

/// Order status as returned by the Architect API.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/get-open-orders>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectOrderStatus {
    /// Order is pending submission.
    Pending,
    /// Order has been accepted by the exchange.
    Accepted,
    /// Order has been partially filled.
    PartiallyFilled,
    /// Order has been completely filled.
    Filled,
    /// Order has been canceled.
    Canceled,
    /// Order has been rejected.
    Rejected,
    /// Order has expired.
    Expired,
    /// Order has been replaced.
    Replaced,
    /// Order is done for the day.
    DoneForDay,
    /// Order status is unknown.
    Unknown,
}

impl From<ArchitectOrderStatus> for OrderStatus {
    fn from(status: ArchitectOrderStatus) -> Self {
        match status {
            ArchitectOrderStatus::Pending => Self::Submitted,
            ArchitectOrderStatus::Accepted => Self::Accepted,
            ArchitectOrderStatus::PartiallyFilled => Self::PartiallyFilled,
            ArchitectOrderStatus::Filled => Self::Filled,
            ArchitectOrderStatus::Canceled => Self::Canceled,
            ArchitectOrderStatus::Rejected => Self::Rejected,
            ArchitectOrderStatus::Expired => Self::Expired,
            ArchitectOrderStatus::Replaced => Self::Accepted,
            ArchitectOrderStatus::DoneForDay => Self::Canceled,
            ArchitectOrderStatus::Unknown => Self::Initialized,
        }
    }
}

/// Time in force for order validity.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectTimeInForce {
    /// Good-Till-Canceled: order remains active until filled or canceled.
    Gtc,
    /// Immediate-Or-Cancel: fill immediately or cancel unfilled portion.
    Ioc,
    /// Day order: valid until end of trading day.
    Day,
}

impl From<ArchitectTimeInForce> for TimeInForce {
    fn from(tif: ArchitectTimeInForce) -> Self {
        match tif {
            ArchitectTimeInForce::Gtc => Self::Gtc,
            ArchitectTimeInForce::Ioc => Self::Ioc,
            ArchitectTimeInForce::Day => Self::Day,
        }
    }
}

/// Market data subscription level.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectMarketDataLevel {
    /// Level 1: best bid/ask only.
    #[serde(rename = "LEVEL_1")]
    #[strum(serialize = "LEVEL_1")]
    Level1,
    /// Level 2: aggregated price levels.
    #[serde(rename = "LEVEL_2")]
    #[strum(serialize = "LEVEL_2")]
    Level2,
    /// Level 3: individual order quantities.
    #[serde(rename = "LEVEL_3")]
    #[strum(serialize = "LEVEL_3")]
    Level3,
}

/// Candle/bar width for market data subscriptions.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectCandleWidth {
    /// 1-second candles.
    #[serde(rename = "1s")]
    #[strum(serialize = "1s")]
    Seconds1,
    /// 5-second candles.
    #[serde(rename = "5s")]
    #[strum(serialize = "5s")]
    Seconds5,
    /// 1-minute candles.
    #[serde(rename = "1m")]
    #[strum(serialize = "1m")]
    Minutes1,
    /// 5-minute candles.
    #[serde(rename = "5m")]
    #[strum(serialize = "5m")]
    Minutes5,
    /// 15-minute candles.
    #[serde(rename = "15m")]
    #[strum(serialize = "15m")]
    Minutes15,
    /// 1-hour candles.
    #[serde(rename = "1h")]
    #[strum(serialize = "1h")]
    Hours1,
    /// 1-day candles.
    #[serde(rename = "1d")]
    #[strum(serialize = "1d")]
    Days1,
}

/// WebSocket market data message type (server to client).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/md-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectMdWsMessageType {
    /// Heartbeat event.
    #[serde(rename = "h")]
    #[strum(serialize = "h")]
    Heartbeat,
    /// Ticker statistics update.
    #[serde(rename = "s")]
    #[strum(serialize = "s")]
    Ticker,
    /// Trade event.
    #[serde(rename = "t")]
    #[strum(serialize = "t")]
    Trade,
    /// Candle/OHLCV update.
    #[serde(rename = "c")]
    #[strum(serialize = "c")]
    Candle,
    /// Level 1 book update (best bid/ask).
    #[serde(rename = "1")]
    #[strum(serialize = "1")]
    BookLevel1,
    /// Level 2 book update (aggregated levels).
    #[serde(rename = "2")]
    #[strum(serialize = "2")]
    BookLevel2,
    /// Level 3 book update (individual orders).
    #[serde(rename = "3")]
    #[strum(serialize = "3")]
    BookLevel3,
}

/// WebSocket order message type (server to client).
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectOrderWsMessageType {
    /// Heartbeat event.
    #[serde(rename = "h")]
    #[strum(serialize = "h")]
    Heartbeat,
    /// Cancel rejected event.
    #[serde(rename = "e")]
    #[strum(serialize = "e")]
    CancelRejected,
    /// Order acknowledged event.
    #[serde(rename = "n")]
    #[strum(serialize = "n")]
    OrderAcknowledged,
    /// Order canceled event.
    #[serde(rename = "c")]
    #[strum(serialize = "c")]
    OrderCanceled,
    /// Order replaced/amended event.
    #[serde(rename = "r")]
    #[strum(serialize = "r")]
    OrderReplaced,
    /// Order rejected event.
    #[serde(rename = "j")]
    #[strum(serialize = "j")]
    OrderRejected,
    /// Order expired event.
    #[serde(rename = "x")]
    #[strum(serialize = "x")]
    OrderExpired,
    /// Order done for day event.
    #[serde(rename = "d")]
    #[strum(serialize = "d")]
    OrderDoneForDay,
    /// Order partially filled event.
    #[serde(rename = "p")]
    #[strum(serialize = "p")]
    OrderPartiallyFilled,
    /// Order filled event.
    #[serde(rename = "f")]
    #[strum(serialize = "f")]
    OrderFilled,
}

/// Reason for order cancellation.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectCancelReason {
    /// User requested cancellation.
    UserRequested,
}

/// Reason for cancel rejection.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/orders-ws>
#[derive(
    Clone,
    Copy,
    Debug,
    Display,
    Eq,
    PartialEq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.architect")
)]
pub enum ArchitectCancelRejectionReason {
    /// Order not found or already canceled.
    OrderNotFound,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(ArchitectInstrumentState::Open, "\"OPEN\"")]
    #[case(ArchitectInstrumentState::PreOpen, "\"PRE_OPEN\"")]
    #[case(ArchitectInstrumentState::Suspended, "\"SUSPENDED\"")]
    #[case(ArchitectInstrumentState::Delisted, "\"DELISTED\"")]
    fn test_instrument_state_serialization(
        #[case] state: ArchitectInstrumentState,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectInstrumentState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }

    #[rstest]
    #[case(ArchitectOrderSide::Buy, "\"B\"")]
    #[case(ArchitectOrderSide::Sell, "\"S\"")]
    fn test_order_side_serialization(#[case] side: ArchitectOrderSide, #[case] expected: &str) {
        let json = serde_json::to_string(&side).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectOrderSide = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, side);
    }

    #[rstest]
    #[case(ArchitectOrderStatus::Pending, "\"PENDING\"")]
    #[case(ArchitectOrderStatus::Accepted, "\"ACCEPTED\"")]
    #[case(ArchitectOrderStatus::PartiallyFilled, "\"PARTIALLY_FILLED\"")]
    #[case(ArchitectOrderStatus::Filled, "\"FILLED\"")]
    #[case(ArchitectOrderStatus::Canceled, "\"CANCELED\"")]
    fn test_order_status_serialization(
        #[case] status: ArchitectOrderStatus,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectOrderStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, status);
    }

    #[rstest]
    #[case(ArchitectTimeInForce::Gtc, "\"GTC\"")]
    #[case(ArchitectTimeInForce::Ioc, "\"IOC\"")]
    #[case(ArchitectTimeInForce::Day, "\"DAY\"")]
    fn test_time_in_force_serialization(#[case] tif: ArchitectTimeInForce, #[case] expected: &str) {
        let json = serde_json::to_string(&tif).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectTimeInForce = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tif);
    }

    #[rstest]
    #[case(ArchitectMarketDataLevel::Level1, "\"LEVEL_1\"")]
    #[case(ArchitectMarketDataLevel::Level2, "\"LEVEL_2\"")]
    #[case(ArchitectMarketDataLevel::Level3, "\"LEVEL_3\"")]
    fn test_market_data_level_serialization(
        #[case] level: ArchitectMarketDataLevel,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectMarketDataLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, level);
    }

    #[rstest]
    #[case(ArchitectCandleWidth::Seconds1, "\"1s\"")]
    #[case(ArchitectCandleWidth::Minutes1, "\"1m\"")]
    #[case(ArchitectCandleWidth::Minutes5, "\"5m\"")]
    #[case(ArchitectCandleWidth::Hours1, "\"1h\"")]
    #[case(ArchitectCandleWidth::Days1, "\"1d\"")]
    fn test_candle_width_serialization(
        #[case] width: ArchitectCandleWidth,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&width).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectCandleWidth = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, width);
    }

    #[rstest]
    #[case(ArchitectMdWsMessageType::Heartbeat, "\"h\"")]
    #[case(ArchitectMdWsMessageType::Ticker, "\"s\"")]
    #[case(ArchitectMdWsMessageType::Trade, "\"t\"")]
    #[case(ArchitectMdWsMessageType::Candle, "\"c\"")]
    #[case(ArchitectMdWsMessageType::BookLevel1, "\"1\"")]
    #[case(ArchitectMdWsMessageType::BookLevel2, "\"2\"")]
    #[case(ArchitectMdWsMessageType::BookLevel3, "\"3\"")]
    fn test_md_ws_message_type_serialization(
        #[case] msg_type: ArchitectMdWsMessageType,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&msg_type).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectMdWsMessageType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg_type);
    }

    #[rstest]
    #[case(ArchitectOrderWsMessageType::Heartbeat, "\"h\"")]
    #[case(ArchitectOrderWsMessageType::OrderAcknowledged, "\"n\"")]
    #[case(ArchitectOrderWsMessageType::OrderCanceled, "\"c\"")]
    #[case(ArchitectOrderWsMessageType::OrderFilled, "\"f\"")]
    #[case(ArchitectOrderWsMessageType::OrderPartiallyFilled, "\"p\"")]
    fn test_order_ws_message_type_serialization(
        #[case] msg_type: ArchitectOrderWsMessageType,
        #[case] expected: &str,
    ) {
        let json = serde_json::to_string(&msg_type).unwrap();
        assert_eq!(json, expected);

        let parsed: ArchitectOrderWsMessageType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, msg_type);
    }
}
