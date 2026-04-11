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

//! Parsing functions for converting Coinbase API responses to Nautilus domain types.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, TradeTick},
    enums::{AggressorSide, BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, Symbol, TradeId},
    instruments::{CryptoFuture, CryptoPerpetual, CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::COINBASE_VENUE,
        enums::{CoinbaseContractExpiryType, CoinbaseOrderSide, CoinbaseProductType},
    },
    http::models::{BookLevel, Candle, PriceBook, Product, Trade},
};

/// Parses an RFC 3339 timestamp string to `UnixNanos`.
pub fn parse_rfc3339_timestamp(timestamp: &str) -> anyhow::Result<UnixNanos> {
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)
        .context(format!("Failed to parse timestamp '{timestamp}'"))?;
    let nanos = dt
        .timestamp_nanos_opt()
        .context(format!("Timestamp out of range: '{timestamp}'"))?;
    anyhow::ensure!(nanos >= 0, "Negative timestamp: '{timestamp}'");
    Ok(UnixNanos::from(nanos as u64))
}

/// Parses a Unix epoch seconds string to `UnixNanos`.
pub fn parse_epoch_secs_timestamp(epoch_secs: &str) -> anyhow::Result<UnixNanos> {
    let secs: u64 = epoch_secs
        .parse()
        .context(format!("Failed to parse epoch seconds '{epoch_secs}'"))?;
    Ok(UnixNanos::from(secs * 1_000_000_000))
}

/// Parses a price string with the given precision.
pub fn parse_price(value: &str, precision: u8) -> anyhow::Result<Price> {
    let decimal = Decimal::from_str(value).context(format!("Failed to parse price '{value}'"))?;
    Price::from_decimal_dp(decimal, precision).context(format!(
        "Failed to create Price from '{value}' with precision {precision}"
    ))
}

/// Parses a quantity string with the given precision.
pub fn parse_quantity(value: &str, precision: u8) -> anyhow::Result<Quantity> {
    let decimal =
        Decimal::from_str(value).context(format!("Failed to parse quantity '{value}'"))?;
    Quantity::from_decimal_dp(decimal, precision).context(format!(
        "Failed to create Quantity from '{value}' with precision {precision}"
    ))
}

/// Derives precision (number of decimal places) from an increment string.
///
/// For example, `"0.01"` returns 2, `"0.00000001"` returns 8, `"1"` returns 0.
pub fn precision_from_increment(increment: &str) -> u8 {
    match increment.find('.') {
        Some(pos) => {
            let decimals = &increment[pos + 1..];
            let trimmed_len = decimals.trim_end_matches('0').len();
            let min = usize::from(!decimals.chars().all(|c| c == '0'));
            trimmed_len.max(min) as u8
        }
        None => 0,
    }
}

/// Converts a Coinbase order side to a Nautilus aggressor side.
pub fn coinbase_side_to_aggressor(side: &CoinbaseOrderSide) -> AggressorSide {
    match side {
        CoinbaseOrderSide::Buy => AggressorSide::Buyer,
        CoinbaseOrderSide::Sell => AggressorSide::Seller,
        CoinbaseOrderSide::Unknown => AggressorSide::NoAggressor,
    }
}

/// Parses an optional quantity from a string, returning `None` for empty or zero.
fn parse_optional_quantity(value: &str) -> Option<Quantity> {
    if value.is_empty() || value == "0" {
        None
    } else {
        Some(Quantity::from(value))
    }
}

/// Derives the base currency from the product, falling back to the first word
/// in `display_name` when `base_currency_id` is empty (Coinbase futures).
fn derive_base_currency(product: &Product) -> Currency {
    if product.base_currency_id.is_empty() {
        let base_str = product
            .display_name
            .split_whitespace()
            .next()
            .unwrap_or("UNKNOWN");
        Currency::get_or_create_crypto(base_str)
    } else {
        Currency::get_or_create_crypto(product.base_currency_id)
    }
}

/// Extracts the contract size as a multiplier from future product details.
fn contract_size_multiplier(product: &Product) -> Option<Quantity> {
    product.future_product_details.as_ref().and_then(|d| {
        if d.contract_size.is_empty() || d.contract_size == "0" {
            None
        } else {
            Some(Quantity::from(d.contract_size.as_str()))
        }
    })
}

/// Parses a Coinbase spot product into a `CurrencyPair`.
pub fn parse_spot_instrument(
    product: &Product,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(product.product_id), *COINBASE_VENUE);
    let raw_symbol = Symbol::new(product.product_id);

    let base_currency = Currency::get_or_create_crypto(product.base_currency_id);
    let quote_currency = Currency::get_or_create_crypto(product.quote_currency_id);

    let price_precision = precision_from_increment(&product.price_increment);
    let size_precision = precision_from_increment(&product.base_increment);

    let price_increment = Price::from(product.price_increment.as_str());
    let size_increment = Quantity::from(product.base_increment.as_str());

    let min_quantity = parse_optional_quantity(&product.base_min_size);
    let max_quantity = parse_optional_quantity(&product.base_max_size);

    let instrument = CurrencyPair::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        None, // multiplier
        None, // lot_size
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee (loaded separately via transaction_summary)
        None, // taker_fee
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CurrencyPair(instrument))
}

/// Parses a Coinbase perpetual futures product into a `CryptoPerpetual`.
pub fn parse_perpetual_instrument(
    product: &Product,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(product.product_id), *COINBASE_VENUE);
    let raw_symbol = Symbol::new(product.product_id);

    let base_currency = derive_base_currency(product);
    let quote_currency = Currency::get_or_create_crypto(product.quote_currency_id);
    let settlement_currency = quote_currency;

    let price_precision = precision_from_increment(&product.price_increment);
    let size_precision = precision_from_increment(&product.base_increment);

    let price_increment = Price::from(product.price_increment.as_str());
    let size_increment = Quantity::from(product.base_increment.as_str());

    let min_quantity = parse_optional_quantity(&product.base_min_size);
    let max_quantity = parse_optional_quantity(&product.base_max_size);

    let multiplier = contract_size_multiplier(product);

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false, // is_inverse
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        multiplier,
        None, // lot_size
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses a Coinbase dated future into a `CryptoFuture`.
pub fn parse_future_instrument(
    product: &Product,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(product.product_id), *COINBASE_VENUE);
    let raw_symbol = Symbol::new(product.product_id);

    let underlying = derive_base_currency(product);
    let quote_currency = Currency::get_or_create_crypto(product.quote_currency_id);
    let settlement_currency = quote_currency;

    let price_precision = precision_from_increment(&product.price_increment);
    let size_precision = precision_from_increment(&product.base_increment);

    let price_increment = Price::from(product.price_increment.as_str());
    let size_increment = Quantity::from(product.base_increment.as_str());

    let min_quantity = parse_optional_quantity(&product.base_min_size);
    let max_quantity = parse_optional_quantity(&product.base_max_size);

    let expiry_str = product
        .future_product_details
        .as_ref()
        .map_or("", |d| d.contract_expiry.as_str());

    anyhow::ensure!(
        !expiry_str.is_empty(),
        "Missing contract_expiry for dated future '{}'",
        product.product_id
    );

    let expiration_ns = parse_rfc3339_timestamp(expiry_str).context(format!(
        "Failed to parse contract_expiry for '{}'",
        product.product_id
    ))?;

    let multiplier = contract_size_multiplier(product);

    let instrument = CryptoFuture::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote_currency,
        settlement_currency,
        false, // is_inverse
        ts_init,
        expiration_ns,
        price_precision,
        size_precision,
        price_increment,
        size_increment,
        multiplier,
        None, // lot_size
        max_quantity,
        min_quantity,
        None, // max_notional
        None, // min_notional
        None, // max_price
        None, // min_price
        None, // margin_init
        None, // margin_maint
        None, // maker_fee
        None, // taker_fee
        None, // info
        ts_init,
        ts_init,
    );

    Ok(InstrumentAny::CryptoFuture(instrument))
}

/// Parses a Coinbase product into the appropriate Nautilus instrument type.
pub fn parse_instrument(product: &Product, ts_init: UnixNanos) -> anyhow::Result<InstrumentAny> {
    match product.product_type {
        CoinbaseProductType::Spot => parse_spot_instrument(product, ts_init),
        CoinbaseProductType::Future => {
            if is_perpetual_product(product) {
                parse_perpetual_instrument(product, ts_init)
            } else {
                parse_future_instrument(product, ts_init)
            }
        }
        CoinbaseProductType::Unknown => {
            anyhow::bail!("Unknown product type for '{}'", product.product_id)
        }
    }
}

/// Determines whether a futures product is a perpetual contract.
///
/// Coinbase returns `contract_expiry_type: "EXPIRING"` for both perpetuals
/// and dated futures, so the `CoinbaseContractExpiryType::Perpetual` variant
/// alone is not sufficient. We check three signals in order:
///
/// 1. `contract_expiry_type == Perpetual` (forward compat if Coinbase fixes the API)
/// 2. Non-empty `funding_rate` in `future_product_details` (structural signal:
///    only perpetuals have ongoing funding)
/// 3. `display_name` contains "PERP" or "Perpetual" (heuristic fallback)
pub(crate) fn is_perpetual_product(product: &Product) -> bool {
    if let Some(details) = &product.future_product_details {
        if details.contract_expiry_type == CoinbaseContractExpiryType::Perpetual {
            return true;
        }

        if !details.funding_rate.is_empty() {
            return true;
        }
    }
    product.display_name.contains("PERP") || product.display_name.contains("Perpetual")
}

/// Parses a Coinbase trade into a `TradeTick`.
pub fn parse_trade_tick(
    trade: &Trade,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, price_precision)?;
    let size = parse_quantity(&trade.size, size_precision)?;
    let aggressor_side = coinbase_side_to_aggressor(&trade.side);
    let trade_id = TradeId::new(&trade.trade_id);
    let ts_event = parse_rfc3339_timestamp(&trade.time)?;

    TradeTick::new_checked(
        instrument_id,
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Parses a Coinbase candle into a `Bar`.
pub fn parse_bar(
    candle: &Candle,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = parse_price(&candle.open, price_precision)?;
    let high = parse_price(&candle.high, price_precision)?;
    let low = parse_price(&candle.low, price_precision)?;
    let close = parse_price(&candle.close, price_precision)?;
    let volume = parse_quantity(&candle.volume, size_precision)?;

    // Coinbase candle "start" is epoch seconds for the candle open time
    let ts_event = parse_epoch_secs_timestamp(&candle.start)?;

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parses a Coinbase order book snapshot into `OrderBookDeltas`.
pub fn parse_product_book_snapshot(
    book: &PriceBook,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let ts_event = parse_rfc3339_timestamp(&book.time)?;
    let total_levels = book.bids.len() + book.asks.len();
    let mut deltas = Vec::with_capacity(total_levels + 1);

    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);

    if total_levels == 0 {
        clear.flags |= RecordFlag::F_LAST as u8;
    }
    deltas.push(clear);

    let mut processed = 0usize;

    for level in &book.bids {
        processed += 1;
        let delta = parse_book_delta(
            level,
            OrderSide::Buy,
            instrument_id,
            price_precision,
            size_precision,
            processed == total_levels,
            ts_event,
            ts_init,
        )?;
        deltas.push(delta);
    }

    for level in &book.asks {
        processed += 1;
        let delta = parse_book_delta(
            level,
            OrderSide::Sell,
            instrument_id,
            price_precision,
            size_precision,
            processed == total_levels,
            ts_event,
            ts_init,
        )?;
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

#[expect(clippy::too_many_arguments)]
fn parse_book_delta(
    level: &BookLevel,
    side: OrderSide,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    is_last: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price = parse_price(&level.price, price_precision)?;
    let size = parse_quantity(&level.size, size_precision)?;

    let mut flags = RecordFlag::F_MBP as u8;

    if is_last {
        flags |= RecordFlag::F_LAST as u8;
    }

    let order = BookOrder::new(side, price, size, 0);
    OrderBookDelta::new_checked(
        instrument_id,
        BookAction::Add,
        order,
        flags,
        0,
        ts_event,
        ts_init,
    )
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::bar::{BarSpecification, BarType},
        enums::{AggregationSource, BarAggregation, PriceType},
        identifiers::Venue,
        instruments::Instrument,
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::common::testing::load_test_fixture;

    fn coinbase_venue() -> Venue {
        Venue::new(Ustr::from("COINBASE"))
    }

    #[rstest]
    #[case("0.01", 2)]
    #[case("0.00000001", 8)]
    #[case("1", 0)]
    #[case("5", 0)]
    #[case("0.1", 1)]
    #[case("0.001", 3)]
    fn test_precision_from_increment(#[case] increment: &str, #[case] expected: u8) {
        assert_eq!(precision_from_increment(increment), expected);
    }

    #[rstest]
    fn test_parse_rfc3339_timestamp() {
        let ts = parse_rfc3339_timestamp("2026-04-07T00:28:32.643779Z").unwrap();
        assert_eq!(ts.as_u64(), 1_775_521_712_643_779_000);
    }

    #[rstest]
    #[case("")]
    #[case("not-a-date")]
    #[case("2026-13-01T00:00:00Z")]
    fn test_parse_rfc3339_timestamp_rejects_invalid(#[case] input: &str) {
        assert!(parse_rfc3339_timestamp(input).is_err());
    }

    #[rstest]
    fn test_parse_epoch_secs_timestamp() {
        let ts = parse_epoch_secs_timestamp("1712192400").unwrap();
        assert_eq!(ts.as_u64(), 1_712_192_400_000_000_000);
    }

    #[rstest]
    #[case("")]
    #[case("abc")]
    fn test_parse_epoch_secs_timestamp_rejects_invalid(#[case] input: &str) {
        assert!(parse_epoch_secs_timestamp(input).is_err());
    }

    #[rstest]
    fn test_parse_price_valid() {
        let price = parse_price("68913.87", 2).unwrap();
        assert_eq!(price, Price::from("68913.87"));
    }

    #[rstest]
    #[case("")]
    #[case("abc")]
    fn test_parse_price_rejects_invalid(#[case] input: &str) {
        assert!(parse_price(input, 2).is_err());
    }

    #[rstest]
    fn test_parse_quantity_valid() {
        let qty = parse_quantity("0.00014004", 8).unwrap();
        assert_eq!(qty, Quantity::from("0.00014004"));
    }

    #[rstest]
    #[case("")]
    #[case("abc")]
    fn test_parse_quantity_rejects_invalid(#[case] input: &str) {
        assert!(parse_quantity(input, 8).is_err());
    }

    #[rstest]
    fn test_parse_spot_instrument() {
        let json = load_test_fixture("http_product.json");
        let product: crate::http::models::Product = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let instrument = parse_spot_instrument(&product, ts).unwrap();
        let pair = match &instrument {
            InstrumentAny::CurrencyPair(p) => p,
            other => panic!("Expected CurrencyPair, was{other:?}"),
        };

        assert_eq!(pair.id().symbol.as_str(), "BTC-USD");
        assert_eq!(pair.id().venue, coinbase_venue());
        assert_eq!(pair.base_currency().unwrap().code.as_str(), "BTC");
        assert_eq!(pair.quote_currency().code.as_str(), "USD");
        assert_eq!(pair.price_precision(), 2);
        assert_eq!(pair.size_precision(), 8);
        assert_eq!(pair.price_increment(), Price::from("0.01"));
        assert_eq!(pair.size_increment(), Quantity::from("0.00000001"));
        assert_eq!(pair.min_quantity(), Some(Quantity::from("0.00000001")));
        assert_eq!(pair.max_quantity(), Some(Quantity::from("3400")));
    }

    #[rstest]
    fn test_parse_spot_instruments_from_list() {
        let json = load_test_fixture("http_products.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let instruments: Vec<InstrumentAny> = response
            .products
            .iter()
            .map(|p| parse_instrument(p, ts).unwrap())
            .collect();

        assert_eq!(instruments.len(), 2);
        for inst in &instruments {
            assert!(matches!(inst, InstrumentAny::CurrencyPair(_)));
        }
    }

    #[rstest]
    fn test_parse_future_instruments_distinguishes_perp_and_dated() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let instruments: Vec<InstrumentAny> = response
            .products
            .iter()
            .map(|p| parse_instrument(p, ts).unwrap())
            .collect();

        assert_eq!(instruments.len(), 2);

        // First product is "BTC PERP" -> CryptoPerpetual
        assert!(
            matches!(&instruments[0], InstrumentAny::CryptoPerpetual(_)),
            "Expected CryptoPerpetual for BTC PERP, was{:?}",
            &instruments[0]
        );

        // Second product is "BTC 24 APR 26" -> CryptoFuture
        assert!(
            matches!(&instruments[1], InstrumentAny::CryptoFuture(_)),
            "Expected CryptoFuture for dated future, was{:?}",
            &instruments[1]
        );
    }

    #[rstest]
    fn test_parse_perpetual_instrument_derives_base_from_display_name() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        // The first future product has empty base_currency_id and display_name "BTC PERP"
        let perp_product = response
            .products
            .iter()
            .find(|p| p.display_name.contains("PERP"))
            .expect("should have a PERP product");

        let instrument = parse_perpetual_instrument(perp_product, ts).unwrap();
        let perp = match &instrument {
            InstrumentAny::CryptoPerpetual(p) => p,
            other => panic!("Expected CryptoPerpetual, was{other:?}"),
        };

        assert_eq!(perp.base_currency().unwrap().code.as_str(), "BTC");
        assert_eq!(perp.quote_currency().code.as_str(), "USD");
    }

    #[rstest]
    fn test_parse_perpetual_instrument_has_contract_size_multiplier() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let perp_product = response
            .products
            .iter()
            .find(|p| p.display_name.contains("PERP"))
            .expect("should have a PERP product");

        let instrument = parse_perpetual_instrument(perp_product, ts).unwrap();
        let perp = match &instrument {
            InstrumentAny::CryptoPerpetual(p) => p,
            other => panic!("Expected CryptoPerpetual, was {other:?}"),
        };

        assert_eq!(perp.multiplier, Quantity::from("0.01"));
    }

    #[rstest]
    fn test_parse_future_instrument_has_expiry_and_multiplier() {
        let json = load_test_fixture("http_products_future.json");
        let response: crate::http::models::ProductsResponse = serde_json::from_str(&json).unwrap();
        let ts = UnixNanos::default();

        let future_product = response
            .products
            .iter()
            .find(|p| !p.display_name.contains("PERP") && !p.display_name.contains("Perpetual"))
            .expect("should have a dated future product");

        let instrument = parse_future_instrument(future_product, ts).unwrap();
        let future = match &instrument {
            InstrumentAny::CryptoFuture(f) => f,
            other => panic!("Expected CryptoFuture, was {other:?}"),
        };

        // Verify contract_expiry "2026-04-24T15:00:00Z" parsed correctly
        let expected_expiry = parse_rfc3339_timestamp("2026-04-24T15:00:00Z").unwrap();
        assert_eq!(future.expiration_ns, expected_expiry);
        assert_eq!(future.multiplier, Quantity::from("0.01"));
        assert_eq!(future.base_currency().unwrap().code.as_str(), "BTC");
        assert_eq!(future.quote_currency().code.as_str(), "USD");
    }

    #[rstest]
    fn test_parse_trade_tick() {
        let json = load_test_fixture("http_ticker.json");
        let response: crate::http::models::TickerResponse = serde_json::from_str(&json).unwrap();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let ts_init = UnixNanos::default();

        let trades: Vec<TradeTick> = response
            .trades
            .iter()
            .map(|t| parse_trade_tick(t, instrument_id, 2, 8, ts_init).unwrap())
            .collect();

        assert_eq!(trades.len(), 3);

        // Verify exact values from first fixture trade
        assert_eq!(trades[0].instrument_id, instrument_id);
        assert_eq!(trades[0].price, Price::from("68923.67"));
        assert_eq!(trades[0].size, Quantity::from("0.00064000"));
        assert_eq!(trades[0].trade_id.as_str(), "995098663");
        assert!(trades[0].ts_event.as_u64() > 0);
    }

    #[rstest]
    fn test_parse_trade_tick_aggressor_side() {
        let json = load_test_fixture("http_ticker.json");
        let response: crate::http::models::TickerResponse = serde_json::from_str(&json).unwrap();
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let ts_init = UnixNanos::default();

        for trade_data in &response.trades {
            let trade = parse_trade_tick(trade_data, instrument_id, 2, 8, ts_init).unwrap();
            match trade_data.side {
                CoinbaseOrderSide::Buy => {
                    assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
                }
                CoinbaseOrderSide::Sell => {
                    assert_eq!(trade.aggressor_side, AggressorSide::Seller);
                }
                _ => {}
            }
        }
    }

    #[rstest]
    fn test_parse_bar() {
        let json = load_test_fixture("http_candles.json");
        let response: crate::http::models::CandlesResponse = serde_json::from_str(&json).unwrap();

        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let bar_spec = BarSpecification::new(1, BarAggregation::Hour, PriceType::Last);
        let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
        let ts_init = UnixNanos::default();

        let bars: Vec<Bar> = response
            .candles
            .iter()
            .map(|c| parse_bar(c, bar_type, 2, 8, ts_init).unwrap())
            .collect();

        assert_eq!(bars.len(), 2);

        // Verify exact OHLCV from first fixture candle (start=1712192400)
        let bar = &bars[0];
        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.open, Price::from("66312.40"));
        assert_eq!(bar.high, Price::from("66331.99"));
        assert_eq!(bar.low, Price::from("66055.14"));
        assert_eq!(bar.close, Price::from("66181.60"));
        assert_eq!(bar.volume, Quantity::from("355.82896243"));
        assert_eq!(bar.ts_event.as_u64(), 1_712_192_400_000_000_000);
    }

    #[rstest]
    fn test_parse_product_book_snapshot() {
        let json = load_test_fixture("http_product_book.json");
        let response: crate::http::models::ProductBookResponse =
            serde_json::from_str(&json).unwrap();

        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), coinbase_venue());
        let ts_init = UnixNanos::default();

        let deltas =
            parse_product_book_snapshot(&response.pricebook, instrument_id, 2, 8, ts_init).unwrap();

        assert_eq!(deltas.instrument_id, instrument_id);
        let total_levels = response.pricebook.bids.len() + response.pricebook.asks.len();
        assert_eq!(deltas.deltas.len(), total_levels + 1);

        // First delta is a clear
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);

        // Verify first bid side and price
        let first_bid = &deltas.deltas[1];
        assert_eq!(first_bid.order.side, OrderSide::Buy);
        assert_eq!(first_bid.action, BookAction::Add);
        assert!(first_bid.order.price.as_f64() > 0.0);

        // Verify first ask comes after bids
        let first_ask_idx = response.pricebook.bids.len() + 1;
        let first_ask = &deltas.deltas[first_ask_idx];
        assert_eq!(first_ask.order.side, OrderSide::Sell);
        assert_eq!(first_ask.action, BookAction::Add);

        // Last delta has F_LAST flag
        let last = deltas.deltas.last().unwrap();
        assert_ne!(last.flags & RecordFlag::F_LAST as u8, 0);
    }
}
