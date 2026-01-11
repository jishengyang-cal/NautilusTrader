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

//! Binance Futures HTTP query parameter builders.

use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::common::enums::{
    BinanceFuturesOrderType, BinanceIncomeType, BinanceMarginType, BinancePositionSide,
    BinanceSide, BinanceTimeInForce, BinanceWorkingType,
};

/// Query parameters for `GET /fapi/v1/depth` or `GET /dapi/v1/depth`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceDepthParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Depth limit (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for `GET /fapi/v1/ticker/24hr` or `GET /dapi/v1/ticker/24hr`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceTicker24hrParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Query parameters for `GET /fapi/v1/ticker/bookTicker` or `GET /dapi/v1/ticker/bookTicker`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceBookTickerParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Query parameters for `GET /fapi/v1/premiumIndex` or `GET /dapi/v1/premiumIndex`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceMarkPriceParams {
    /// Filter by single symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Query parameters for `GET /fapi/v1/fundingRate` or `GET /dapi/v1/fundingRate`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceFundingRateParams {
    /// Trading symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Number of results (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for `GET /fapi/v1/openInterest` or `GET /dapi/v1/openInterest`.
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into))]
pub struct BinanceOpenInterestParams {
    /// Trading symbol (required).
    pub symbol: String,
}

/// Query parameters for `GET /fapi/v2/balance` or `GET /dapi/v1/balance`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceFuturesBalanceParams {
    /// Filter by asset (e.g., "USDT").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v2/positionRisk` or `GET /dapi/v1/positionRisk`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinancePositionRiskParams {
    /// Filter by symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/income` or `GET /dapi/v1/income`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceIncomeHistoryParams {
    /// Filter by symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Income type filter (e.g., FUNDING_FEE).
    #[serde(rename = "incomeType", skip_serializing_if = "Option::is_none")]
    pub income_type: Option<BinanceIncomeType>,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Maximum number of rows (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/userTrades` or `GET /dapi/v1/userTrades`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceUserTradesParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Trade ID to fetch from (inclusive).
    #[serde(rename = "fromId", skip_serializing_if = "Option::is_none")]
    pub from_id: Option<i64>,
    /// Number of trades to return (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/openOrders` or `GET /dapi/v1/openOrders`.
#[derive(Clone, Debug, Deserialize, Serialize, Default, Builder)]
#[builder(default)]
#[builder(setter(into, strip_option))]
pub struct BinanceOpenOrdersParams {
    /// Filter by symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/order` or `GET /dapi/v1/order`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceOrderQueryParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order ID.
    #[serde(rename = "orderId", skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    /// Orig client order ID.
    #[serde(rename = "origClientOrderId", skip_serializing_if = "Option::is_none")]
    pub orig_client_order_id: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `POST /fapi/v1/order` (new order).
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct BinanceNewOrderParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order side (required).
    pub side: BinanceSide,
    /// Order type (required).
    #[serde(rename = "type")]
    pub order_type: BinanceFuturesOrderType,
    /// Position side (required for hedge mode).
    #[serde(rename = "positionSide", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub position_side: Option<BinancePositionSide>,
    /// Time in force.
    #[serde(rename = "timeInForce", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub time_in_force: Option<BinanceTimeInForce>,
    /// Order quantity.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub quantity: Option<String>,
    /// Reduce only flag.
    #[serde(rename = "reduceOnly", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub reduce_only: Option<bool>,
    /// Limit price.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub price: Option<String>,
    /// Client order ID.
    #[serde(rename = "newClientOrderId", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub new_client_order_id: Option<String>,
    /// Stop price.
    #[serde(rename = "stopPrice", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub stop_price: Option<String>,
    /// Close position flag.
    #[serde(rename = "closePosition", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub close_position: Option<bool>,
    /// Activation price for trailing stop.
    #[serde(rename = "activationPrice", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub activation_price: Option<String>,
    /// Callback rate for trailing stop.
    #[serde(rename = "callbackRate", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub callback_rate: Option<String>,
    /// Working type (MARK_PRICE or CONTRACT_PRICE).
    #[serde(rename = "workingType", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub working_type: Option<BinanceWorkingType>,
    /// Price protect flag.
    #[serde(rename = "priceProtect", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub price_protect: Option<bool>,
    /// Response type (ACK, RESULT, FULL).
    #[serde(rename = "newOrderRespType", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub new_order_resp_type: Option<String>,
    /// Good till date (for GTD orders).
    #[serde(rename = "goodTillDate", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub good_till_date: Option<i64>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub recv_window: Option<u64>,
}

/// Query parameters for `DELETE /fapi/v1/order` (cancel order).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceCancelOrderParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order ID.
    #[serde(rename = "orderId", skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    /// Orig client order ID.
    #[serde(rename = "origClientOrderId", skip_serializing_if = "Option::is_none")]
    pub orig_client_order_id: Option<String>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `DELETE /fapi/v1/allOpenOrders` (cancel all open orders).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceCancelAllOrdersParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `PUT /fapi/v1/order` (modify order).
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option))]
pub struct BinanceModifyOrderParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order ID.
    #[serde(rename = "orderId", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub order_id: Option<i64>,
    /// Orig client order ID.
    #[serde(rename = "origClientOrderId", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub orig_client_order_id: Option<String>,
    /// Order side (required).
    pub side: BinanceSide,
    /// Order quantity (required).
    pub quantity: String,
    /// Limit price (required).
    pub price: String,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub recv_window: Option<u64>,
}

/// Query parameters for `GET /fapi/v1/allOrders` (all orders history).
#[derive(Clone, Debug, Default, Deserialize, Serialize, Builder)]
#[builder(setter(into, strip_option), default)]
pub struct BinanceAllOrdersParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Order ID to start from.
    #[serde(rename = "orderId", skip_serializing_if = "Option::is_none")]
    pub order_id: Option<i64>,
    /// Start time in milliseconds.
    #[serde(rename = "startTime", skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds.
    #[serde(rename = "endTime", skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Number of results (default 500, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    pub recv_window: Option<u64>,
}

/// Query parameters for `POST /fapi/v1/leverage` (set leverage).
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into))]
pub struct BinanceSetLeverageParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Target leverage (required).
    pub leverage: u32,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub recv_window: Option<u64>,
}

/// Query parameters for `POST /fapi/v1/marginType` (set margin type).
#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[builder(setter(into))]
pub struct BinanceSetMarginTypeParams {
    /// Trading symbol (required).
    pub symbol: String,
    /// Margin type (required).
    #[serde(rename = "marginType")]
    pub margin_type: BinanceMarginType,
    /// Recv window override (ms).
    #[serde(rename = "recvWindow", skip_serializing_if = "Option::is_none")]
    #[builder(default)]
    pub recv_window: Option<u64>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_depth_params_builder() {
        let params = BinanceDepthParamsBuilder::default()
            .symbol("BTCUSDT")
            .limit(100u32)
            .build()
            .unwrap();

        assert_eq!(params.symbol, "BTCUSDT");
        assert_eq!(params.limit, Some(100));
    }

    #[rstest]
    fn test_ticker_params_serialization() {
        let params = BinanceTicker24hrParams {
            symbol: Some("BTCUSDT".to_string()),
        };

        let serialized = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(serialized, "symbol=BTCUSDT");
    }

    #[rstest]
    fn test_order_query_params_builder() {
        let params = BinanceOrderQueryParamsBuilder::default()
            .symbol("BTCUSDT")
            .order_id(12345_i64)
            .recv_window(5_000_u64)
            .build()
            .unwrap();

        assert_eq!(params.symbol, "BTCUSDT");
        assert_eq!(params.order_id, Some(12345));
        assert_eq!(params.recv_window, Some(5_000));
    }

    #[rstest]
    fn test_income_history_params_serialization() {
        let params = BinanceIncomeHistoryParamsBuilder::default()
            .symbol("ETHUSDT")
            .income_type(BinanceIncomeType::FundingFee)
            .limit(50_u32)
            .build()
            .unwrap();

        let serialized = serde_urlencoded::to_string(&params).unwrap();
        assert_eq!(serialized, "symbol=ETHUSDT&incomeType=FUNDING_FEE&limit=50");
    }

    #[rstest]
    fn test_open_orders_params_builder() {
        let params = BinanceOpenOrdersParamsBuilder::default()
            .symbol("BNBUSDT")
            .build()
            .unwrap();

        assert_eq!(params.symbol.as_deref(), Some("BNBUSDT"));
        assert!(params.recv_window.is_none());
    }
}
