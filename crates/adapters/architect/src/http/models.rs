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

//! Data transfer objects for deserializing Architect HTTP API payloads.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::enums::{
    ArchitectCandleWidth, ArchitectInstrumentState, ArchitectOrderSide, ArchitectOrderStatus,
    ArchitectTimeInForce,
};

/// Default instrument state when not provided by API.
fn default_instrument_state() -> ArchitectInstrumentState {
    ArchitectInstrumentState::Open
}

/// Response payload returned by `GET /whoami`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/whoami>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectWhoAmI {
    /// User account UUID.
    pub id: String,
    /// Username for the account.
    pub username: String,
    /// Account creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Whether two-factor authentication is enabled.
    pub enabled_2fa: bool,
    /// Whether the user has completed onboarding.
    pub is_onboarded: bool,
    /// Whether the account is frozen.
    pub is_frozen: bool,
    /// Whether the user has admin privileges.
    pub is_admin: bool,
    /// Whether the account is in close-only mode.
    pub is_close_only: bool,
    /// Maker fee rate as string.
    pub maker_fee: String,
    /// Taker fee rate as string.
    pub taker_fee: String,
}

/// Individual instrument definition.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/symbols-instruments/get-instruments>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectInstrument {
    /// Trading symbol for the instrument.
    pub symbol: Ustr,
    /// Current trading state of the instrument (defaults to Open if not provided).
    #[serde(default = "default_instrument_state")]
    pub state: ArchitectInstrumentState,
    /// Contract multiplier.
    pub multiplier: String,
    /// Minimum order size.
    pub minimum_order_size: String,
    /// Price tick size.
    pub tick_size: String,
    /// Quote currency symbol.
    pub quote_currency: Ustr,
    // TODO: Rename to `funding_settlement_currency` once fixed
    /// Funding settlement currency.
    #[serde(alias = "funding_settlement_currency")]
    pub finding_settlement_currency: Ustr,
    /// Maintenance margin percentage.
    pub maintenance_margin_pct: String,
    /// Initial margin percentage.
    pub initial_margin_pct: String,
    /// Current mark price for the contract (optional).
    #[serde(default)]
    pub contract_mark_price: Option<String>,
    /// Contract size (optional).
    #[serde(default)]
    pub contract_size: Option<String>,
    /// Instrument description (optional).
    #[serde(default)]
    pub description: Option<String>,
    /// Funding calendar schedule (optional).
    #[serde(default)]
    pub funding_calendar_schedule: Option<String>,
    /// Funding frequency (optional).
    #[serde(default)]
    pub funding_frequency: Option<String>,
    /// Lower cap for funding rate percentage (optional).
    #[serde(default)]
    pub funding_rate_cap_lower_pct: Option<String>,
    /// Upper cap for funding rate percentage (optional).
    #[serde(default)]
    pub funding_rate_cap_upper_pct: Option<String>,
    /// Lower deviation percentage for price bands (optional).
    #[serde(default)]
    pub price_band_lower_deviation_pct: Option<String>,
    /// Upper deviation percentage for price bands (optional).
    #[serde(default)]
    pub price_band_upper_deviation_pct: Option<String>,
    /// Price bands configuration (optional).
    #[serde(default)]
    pub price_bands: Option<String>,
    /// Price quotation format (optional).
    #[serde(default)]
    pub price_quotation: Option<String>,
    /// Underlying benchmark price (optional).
    #[serde(default)]
    pub underlying_benchmark_price: Option<String>,
}

/// Response payload returned by `GET /instruments`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/symbols-instruments/get-instruments>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectInstrumentsResponse {
    /// List of instruments.
    pub instruments: Vec<ArchitectInstrument>,
}

/// Individual balance entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-balances>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectBalance {
    /// Asset symbol.
    pub symbol: Ustr,
    /// Available balance amount.
    pub amount: String,
}

/// Response payload returned by `GET /balances`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-balances>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectBalancesResponse {
    /// List of balances.
    pub balances: Vec<ArchitectBalance>,
}

/// Individual position entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-positions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectPosition {
    /// User account UUID.
    pub user_id: String,
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Open quantity (positive for long, negative for short).
    pub open_quantity: i64,
    /// Open notional value.
    pub open_notional: String,
    /// Position timestamp.
    pub timestamp: DateTime<Utc>,
    /// Realized profit and loss.
    pub realized_pnl: String,
}

/// Response payload returned by `GET /positions`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-positions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectPositionsResponse {
    /// List of positions.
    pub positions: Vec<ArchitectPosition>,
}

/// Individual ticker entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-ticker>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectTicker {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Best bid price.
    #[serde(default)]
    pub bid: Option<String>,
    /// Best ask price.
    #[serde(default)]
    pub ask: Option<String>,
    /// Last trade price.
    #[serde(default)]
    pub last: Option<String>,
    /// Mark price.
    #[serde(default)]
    pub mark: Option<String>,
    /// Index price.
    #[serde(default)]
    pub index: Option<String>,
    /// 24-hour volume.
    #[serde(default)]
    pub volume_24h: Option<String>,
    /// 24-hour high price.
    #[serde(default)]
    pub high_24h: Option<String>,
    /// 24-hour low price.
    #[serde(default)]
    pub low_24h: Option<String>,
    /// Ticker timestamp.
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
}

/// Response payload returned by `GET /tickers`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-tickers>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectTickersResponse {
    /// List of tickers.
    pub tickers: Vec<ArchitectTicker>,
}

/// Response payload returned by `POST /authenticate`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectAuthenticateResponse {
    /// Session token for authenticated requests.
    pub token: String,
}

/// Response payload returned by `POST /place_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectPlaceOrderResponse {
    /// Order ID of the placed order.
    pub oid: String,
}

/// Response payload returned by `POST /cancel_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/cancel-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectCancelOrderResponse {
    /// Whether the cancel request has been accepted.
    pub cxl_rx: bool,
}

/// Individual open order entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/get-open-orders>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchitectOpenOrder {
    /// Trade number.
    pub tn: i64,
    /// Timestamp (Unix epoch).
    pub ts: i64,
    /// Order side: "B" (buy) or "S" (sell).
    pub d: ArchitectOrderSide,
    /// Order status.
    pub o: ArchitectOrderStatus,
    /// Order ID.
    pub oid: String,
    /// Price.
    pub p: String,
    /// Quantity.
    pub q: i64,
    /// Remaining quantity.
    pub rq: i64,
    /// Symbol.
    pub s: Ustr,
    /// Time in force.
    pub tif: ArchitectTimeInForce,
    /// User ID.
    pub u: String,
    /// Executed quantity.
    pub xq: i64,
    /// Optional order tag.
    #[serde(default)]
    pub tag: Option<String>,
}

/// Response payload returned by `GET /open_orders`.
///
/// Note: The response is a direct array, not wrapped in an object.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/get-open-orders>
pub type ArchitectOpenOrdersResponse = Vec<ArchitectOpenOrder>;

/// Individual fill/trade entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-fills>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectFill {
    /// Execution ID.
    pub execution_id: String,
    /// Fee amount.
    pub fee: String,
    /// Whether this was a taker order.
    pub is_taker: bool,
    /// Execution price.
    pub price: String,
    /// Executed quantity.
    pub quantity: i64,
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Execution timestamp.
    pub timestamp: DateTime<Utc>,
    /// User ID.
    pub user_id: String,
}

/// Response payload returned by `GET /fills`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-fills>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectFillsResponse {
    /// List of fills.
    pub fills: Vec<ArchitectFill>,
}

/// Individual candle/OHLCV entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectCandle {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Candle timestamp.
    pub tn: DateTime<Utc>,
    /// Open price.
    pub open: String,
    /// High price.
    pub high: String,
    /// Low price.
    pub low: String,
    /// Close price.
    pub close: String,
    /// Buy volume.
    pub buy_volume: i64,
    /// Sell volume.
    pub sell_volume: i64,
    /// Total volume.
    pub volume: i64,
    /// Candle width/interval.
    pub width: ArchitectCandleWidth,
}

/// Response payload returned by `GET /candles`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-candles>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectCandlesResponse {
    /// List of candles.
    pub candles: Vec<ArchitectCandle>,
}

/// Response payload returned by `GET /candles/current` and `GET /candles/last`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-current-candle>
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-last-candle>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectCandleResponse {
    /// The candle data.
    pub candle: ArchitectCandle,
}

/// Individual funding rate entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-funding-rates>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectFundingRate {
    /// Instrument symbol.
    pub symbol: Ustr,
    /// Timestamp in nanoseconds.
    pub timestamp_ns: i64,
    /// Funding rate.
    pub funding_rate: String,
    /// Funding amount.
    pub funding_amount: String,
    /// Benchmark price.
    pub benchmark_price: String,
    /// Settlement price.
    pub settlement_price: String,
}

/// Response payload returned by `GET /funding-rates`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/marketdata/get-funding-rates>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectFundingRatesResponse {
    /// List of funding rates.
    pub funding_rates: Vec<ArchitectFundingRate>,
}

/// Per-symbol risk metrics.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectPerSymbolRisk {
    /// Open quantity.
    pub open_quantity: i64,
    /// Open notional value.
    pub open_notional: String,
    /// Average entry price.
    pub average_price: String,
    /// Liquidation price.
    #[serde(default)]
    pub liquidation_price: Option<String>,
    /// Initial margin required.
    #[serde(default)]
    pub initial_margin_required: Option<String>,
    /// Maintenance margin required.
    #[serde(default)]
    pub maintenance_margin_required: Option<String>,
    /// Unrealized P&L.
    #[serde(default)]
    pub unrealized_pnl: Option<String>,
}

/// Risk snapshot data.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectRiskSnapshot {
    /// USD account balance.
    pub balance_usd: String,
    /// Total equity value.
    pub equity: String,
    /// Available initial margin.
    pub initial_margin_available: String,
    /// Margin required for open orders.
    pub initial_margin_required_for_open_orders: String,
    /// Margin required for positions.
    pub initial_margin_required_for_positions: String,
    /// Total initial margin requirement.
    pub initial_margin_required_total: String,
    /// Available maintenance margin.
    pub maintenance_margin_available: String,
    /// Required maintenance margin.
    pub maintenance_margin_required: String,
    /// Unrealized profit/loss.
    pub unrealized_pnl: String,
    /// Snapshot timestamp.
    pub timestamp_ns: DateTime<Utc>,
    /// User identifier.
    pub user_id: String,
    /// Per-symbol risk data.
    #[serde(default)]
    pub per_symbol: HashMap<String, ArchitectPerSymbolRisk>,
}

/// Response payload returned by `GET /risk-snapshot`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-risk-snapshot>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectRiskSnapshotResponse {
    /// The risk snapshot data.
    pub risk_snapshot: ArchitectRiskSnapshot,
}

/// Individual transaction entry.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-transactions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectTransaction {
    /// Transaction amount.
    pub amount: String,
    /// Unique event identifier.
    pub event_id: String,
    /// Asset symbol.
    pub symbol: Ustr,
    /// Transaction timestamp.
    pub timestamp: DateTime<Utc>,
    /// Type of transaction.
    pub transaction_type: Ustr,
    /// User identifier.
    pub user_id: String,
    /// Optional reference identifier.
    #[serde(default)]
    pub reference_id: Option<String>,
}

/// Response payload returned by `GET /transactions`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/portfolio-management/get-transactions>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArchitectTransactionsResponse {
    /// List of transactions.
    pub transactions: Vec<ArchitectTransaction>,
}

/// Request body for `POST /authenticate` using API key and secret.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthenticateApiKeyRequest {
    /// API key.
    pub api_key: String,
    /// API secret.
    pub api_secret: String,
    /// Token expiration in seconds.
    pub expiration_seconds: i32,
    /// Optional 2FA code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totp: Option<String>,
}

impl AuthenticateApiKeyRequest {
    /// Creates a new [`AuthenticateApiKeyRequest`].
    #[must_use]
    pub fn new(
        api_key: impl Into<String>,
        api_secret: impl Into<String>,
        expiration_seconds: i32,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
            expiration_seconds,
            totp: None,
        }
    }

    /// Sets the optional 2FA code.
    #[must_use]
    pub fn with_totp(mut self, totp: impl Into<String>) -> Self {
        self.totp = Some(totp.into());
        self
    }
}

/// Request body for `POST /authenticate` using username and password.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/user-management/get-user-token>
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuthenticateUserRequest {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
    /// Token expiration in seconds.
    pub expiration_seconds: i32,
    /// Optional 2FA code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub totp: Option<String>,
}

impl AuthenticateUserRequest {
    /// Creates a new [`AuthenticateUserRequest`].
    #[must_use]
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
        expiration_seconds: i32,
    ) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
            expiration_seconds,
            totp: None,
        }
    }

    /// Sets the optional 2FA code.
    #[must_use]
    pub fn with_totp(mut self, totp: impl Into<String>) -> Self {
        self.totp = Some(totp.into());
        self
    }
}

/// Request body for `POST /place_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/place-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaceOrderRequest {
    /// Order side: "B" (buy) or "S" (sell).
    pub d: ArchitectOrderSide,
    /// Order price as decimal string.
    pub p: String,
    /// Post-only flag (maker-or-cancel).
    pub po: bool,
    /// Order quantity in contracts.
    pub q: i64,
    /// Order symbol.
    pub s: String,
    /// Time in force.
    pub tif: ArchitectTimeInForce,
    /// Optional order tag (max 10 alphanumeric characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

impl PlaceOrderRequest {
    /// Creates a new [`PlaceOrderRequest`].
    #[must_use]
    pub fn new(
        side: ArchitectOrderSide,
        price: impl Into<String>,
        quantity: i64,
        symbol: impl Into<String>,
        time_in_force: ArchitectTimeInForce,
        post_only: bool,
    ) -> Self {
        Self {
            d: side,
            p: price.into(),
            po: post_only,
            q: quantity,
            s: symbol.into(),
            tif: time_in_force,
            tag: None,
        }
    }

    /// Sets the optional order tag.
    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }
}

/// Request body for `POST /cancel_order`.
///
/// # References
/// - <https://docs.sandbox.x.architect.co/api-reference/order-management/cancel-order>
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    /// Order ID to cancel.
    pub oid: String,
}

impl CancelOrderRequest {
    /// Creates a new [`CancelOrderRequest`].
    #[must_use]
    pub fn new(order_id: impl Into<String>) -> Self {
        Self {
            oid: order_id.into(),
        }
    }
}
