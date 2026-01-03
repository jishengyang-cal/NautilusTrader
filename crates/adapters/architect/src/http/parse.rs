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

//! Parsing functions to convert Architect HTTP responses to Nautilus domain types.

use std::str::FromStr;

use anyhow::Context;
use nautilus_core::{UUID4, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType},
    enums::{
        AccountType, AggregationSource, BarAggregation, LiquiditySide, OrderSide, OrderType,
        PositionSideSpecified, PriceType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, TradeId, VenueOrderId},
    instruments::{CryptoPerpetual, Instrument, any::InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rust_decimal::Decimal;

use super::models::{
    ArchitectBalancesResponse, ArchitectCandle, ArchitectFill, ArchitectInstrument,
    ArchitectOpenOrder, ArchitectPosition,
};
use crate::common::{consts::ARCHITECT_VENUE, enums::ArchitectCandleWidth, parse::parse_decimal};

/// Parses a Price from a string field.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed or converted to Price.
fn parse_price(value: &str, field_name: &str) -> anyhow::Result<Price> {
    let decimal = parse_decimal(value)
        .with_context(|| format!("Failed to parse price from field '{field_name}'"))?;
    Price::from_decimal(decimal).context("Failed to convert decimal to Price")
}

/// Parses a Quantity from a string field.
///
/// # Errors
///
/// Returns an error if the string cannot be parsed or converted to Quantity.
fn parse_quantity(value: &str, field_name: &str) -> anyhow::Result<Quantity> {
    let decimal = parse_decimal(value)
        .with_context(|| format!("Failed to parse quantity from field '{field_name}'"))?;
    Quantity::from_decimal(decimal).context("Failed to convert decimal to Quantity")
}

/// Parses a Price with specific precision.
fn parse_price_with_precision(value: &str, precision: u8, field: &str) -> anyhow::Result<Price> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Failed to parse {field}='{value}' as f64"))?;
    Price::new_checked(parsed, precision).with_context(|| {
        format!("Failed to construct Price for {field} with precision {precision}")
    })
}

/// Gets or creates a Currency from a currency code string.
#[must_use]
fn get_currency(code: &str) -> Currency {
    Currency::from(code)
}

/// Converts an Architect candle width to a Nautilus bar specification.
#[must_use]
pub fn candle_width_to_bar_spec(width: ArchitectCandleWidth) -> BarSpecification {
    match width {
        ArchitectCandleWidth::Seconds1 => {
            BarSpecification::new(1, BarAggregation::Second, PriceType::Last)
        }
        ArchitectCandleWidth::Seconds5 => {
            BarSpecification::new(5, BarAggregation::Second, PriceType::Last)
        }
        ArchitectCandleWidth::Minutes1 => {
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last)
        }
        ArchitectCandleWidth::Minutes5 => {
            BarSpecification::new(5, BarAggregation::Minute, PriceType::Last)
        }
        ArchitectCandleWidth::Minutes15 => {
            BarSpecification::new(15, BarAggregation::Minute, PriceType::Last)
        }
        ArchitectCandleWidth::Hours1 => {
            BarSpecification::new(1, BarAggregation::Hour, PriceType::Last)
        }
        ArchitectCandleWidth::Days1 => {
            BarSpecification::new(1, BarAggregation::Day, PriceType::Last)
        }
    }
}

/// Parses an Architect candle into a Nautilus Bar.
///
/// # Errors
///
/// Returns an error if any OHLCV field cannot be parsed.
pub fn parse_bar(
    candle: &ArchitectCandle,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    let open = parse_price_with_precision(&candle.open, price_precision, "candle.open")?;
    let high = parse_price_with_precision(&candle.high, price_precision, "candle.high")?;
    let low = parse_price_with_precision(&candle.low, price_precision, "candle.low")?;
    let close = parse_price_with_precision(&candle.close, price_precision, "candle.close")?;

    // Architect provides volume as i64 contracts
    let volume = Quantity::new(candle.volume as f64, size_precision);

    let ts_event = UnixNanos::from(candle.tn.timestamp_nanos_opt().unwrap_or(0) as u64);

    let bar_spec = candle_width_to_bar_spec(candle.width);
    let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
        .context("Failed to construct Bar from Architect candle")
}

/// Parses an Architect perpetual futures instrument into a Nautilus CryptoPerpetual.
///
/// # Errors
///
/// Returns an error if any required field cannot be parsed or is invalid.
pub fn parse_perp_instrument(
    definition: &ArchitectInstrument,
    maker_fee: Decimal,
    taker_fee: Decimal,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<InstrumentAny> {
    // Architect perpetuals use format: {BASE}-PERP, quoted in USD
    let raw_symbol_str = definition.symbol.as_str();
    let raw_symbol = Symbol::new(raw_symbol_str);
    let instrument_id = InstrumentId::new(raw_symbol, *ARCHITECT_VENUE);

    let base_code = raw_symbol_str
        .split('-')
        .next()
        .context("Failed to extract base currency from symbol")?;
    let base_currency = get_currency(base_code);

    let quote_currency = get_currency(&definition.quote_currency);
    let settlement_currency = quote_currency;

    let price_increment = parse_price(&definition.tick_size, "tick_size")?;
    let size_increment = parse_quantity(&definition.minimum_order_size, "minimum_order_size")?;

    let lot_size = Some(size_increment);
    let min_quantity = Some(size_increment);

    let margin_init = Decimal::from_str(&definition.initial_margin_pct)
        .context("Failed to parse initial_margin_pct")?;
    let margin_maint = Decimal::from_str(&definition.maintenance_margin_pct)
        .context("Failed to parse maintenance_margin_pct")?;

    let instrument = CryptoPerpetual::new(
        instrument_id,
        raw_symbol,
        base_currency,
        quote_currency,
        settlement_currency,
        false, // Architect perps are linear/USDT-margined
        price_increment.precision,
        size_increment.precision,
        price_increment,
        size_increment,
        None,
        lot_size,
        None,
        min_quantity,
        None,
        None,
        None,
        None,
        Some(margin_init),
        Some(margin_maint),
        Some(maker_fee),
        Some(taker_fee),
        ts_event,
        ts_init,
    );

    Ok(InstrumentAny::CryptoPerpetual(instrument))
}

/// Parses an Architect balances response into a Nautilus [`AccountState`].
///
/// Architect provides a simple balance structure with symbol and amount.
/// The amount is treated as both total and free balance (no locked funds tracking).
///
/// # Errors
///
/// Returns an error if balance amount parsing fails.
pub fn parse_account_state(
    response: &ArchitectBalancesResponse,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<AccountState> {
    let mut balances = Vec::new();

    for balance in &response.balances {
        let symbol_str = balance.symbol.as_str().trim();
        if symbol_str.is_empty() {
            tracing::debug!("Skipping balance with empty symbol");
            continue;
        }

        let currency = Currency::from(symbol_str);

        let amount = balance
            .amount
            .parse::<f64>()
            .with_context(|| format!("Failed to parse balance amount for {symbol_str}"))?;

        let total = Money::new(amount, currency);
        let locked = Money::new(0.0, currency);
        let free = total;

        balances.push(AccountBalance::new(total, locked, free));
    }

    if balances.is_empty() {
        let zero_currency = Currency::USD();
        let zero_money = Money::new(0.0, zero_currency);
        balances.push(AccountBalance::new(zero_money, zero_money, zero_money));
    }

    Ok(AccountState::new(
        account_id,
        AccountType::Margin,
        balances,
        vec![],
        true,
        UUID4::new(),
        ts_event,
        ts_init,
        None,
    ))
}

/// Parses an Architect open order into a Nautilus [`OrderStatusReport`].
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity fields cannot be parsed.
/// - Timestamp conversion fails.
pub fn parse_order_status_report(
    order: &ArchitectOpenOrder,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::new(&order.oid);
    let order_side = order.d.into();
    let order_status = order.o.into();
    let time_in_force = order.tif.into();

    // Architect only supports limit orders currently
    let order_type = OrderType::Limit;

    // Parse quantity (Architect uses i64 contracts)
    let quantity = Quantity::new(order.q as f64, instrument.size_precision());
    let filled_qty = Quantity::new(order.xq as f64, instrument.size_precision());

    // Parse price
    let price = parse_price_with_precision(&order.p, instrument.price_precision(), "order.p")?;

    // Architect timestamps are in Unix epoch seconds
    let ts_event = UnixNanos::from((order.ts as u64) * 1_000_000_000);

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        venue_order_id,
        order_side,
        order_type,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    );

    // Add client order ID if tag is present
    if let Some(ref tag) = order.tag
        && !tag.is_empty()
    {
        report = report.with_client_order_id(ClientOrderId::new(tag.as_str()));
    }

    report = report.with_price(price);

    // Calculate average price if there are fills
    if order.xq > 0 {
        let avg_px = price.as_f64();
        report = report.with_avg_px(avg_px)?;
    }

    Ok(report)
}

/// Parses an Architect fill into a Nautilus [`FillReport`].
///
/// Note: Architect fills don't include order ID, side, or liquidity information
/// in the fills endpoint response, so we use default values where necessary.
///
/// # Errors
///
/// Returns an error if:
/// - Price or quantity fields cannot be parsed.
/// - Fee parsing fails.
pub fn parse_fill_report(
    fill: &ArchitectFill,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<FillReport> {
    let instrument_id = instrument.id();

    // Architect fills use execution_id as the unique identifier
    let venue_order_id = VenueOrderId::new(&fill.execution_id);
    let trade_id = TradeId::new_checked(&fill.execution_id)
        .context("Invalid execution_id in Architect fill")?;

    // Architect doesn't provide order side in fills, infer from quantity sign
    let order_side = if fill.quantity >= 0 {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    let last_px =
        parse_price_with_precision(&fill.price, instrument.price_precision(), "fill.price")?;
    let last_qty = Quantity::new(
        fill.quantity.unsigned_abs() as f64,
        instrument.size_precision(),
    );

    // Parse fee (Architect returns positive fee, Nautilus uses negative for costs)
    let fee_f64 = fill
        .fee
        .parse::<f64>()
        .with_context(|| format!("Failed to parse fee='{}'", fill.fee))?;
    let currency = Currency::USD();
    let commission = Money::new(-fee_f64, currency);

    let liquidity_side = if fill.is_taker {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let ts_event = UnixNanos::from(
        fill.timestamp
            .timestamp_nanos_opt()
            .unwrap_or(0)
            .unsigned_abs(),
    );

    Ok(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        None,
        None,
        ts_event,
        ts_init,
        None,
    ))
}

/// Parses an Architect position into a Nautilus [`PositionStatusReport`].
///
/// # Errors
///
/// Returns an error if:
/// - Position quantity parsing fails.
/// - Timestamp conversion fails.
pub fn parse_position_status_report(
    position: &ArchitectPosition,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<PositionStatusReport> {
    let instrument_id = instrument.id();

    // Determine position side and quantity from open_quantity sign
    let (position_side, quantity) = if position.open_quantity > 0 {
        (
            PositionSideSpecified::Long,
            Quantity::new(position.open_quantity as f64, instrument.size_precision()),
        )
    } else if position.open_quantity < 0 {
        (
            PositionSideSpecified::Short,
            Quantity::new(
                position.open_quantity.unsigned_abs() as f64,
                instrument.size_precision(),
            ),
        )
    } else {
        (
            PositionSideSpecified::Flat,
            Quantity::new(0.0, instrument.size_precision()),
        )
    };

    // Calculate average entry price from notional / quantity
    let avg_px_open = if position.open_quantity != 0 {
        let notional =
            Decimal::from_str(&position.open_notional).context("Failed to parse open_notional")?;
        let qty_dec = Decimal::from(position.open_quantity.abs());
        Some(notional / qty_dec)
    } else {
        None
    };

    let ts_last = UnixNanos::from(
        position
            .timestamp
            .timestamp_nanos_opt()
            .unwrap_or(0)
            .unsigned_abs(),
    );

    Ok(PositionStatusReport::new(
        account_id,
        instrument_id,
        position_side,
        quantity,
        ts_last,
        ts_init,
        None,
        None,
        avg_px_open,
    ))
}

#[cfg(test)]
mod tests {
    use nautilus_core::nanos::UnixNanos;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::ArchitectInstrumentState, http::models::ArchitectInstrumentsResponse,
    };

    fn create_test_instrument() -> ArchitectInstrument {
        ArchitectInstrument {
            symbol: Ustr::from("BTC-PERP"),
            state: ArchitectInstrumentState::Open,
            multiplier: "1.0".to_string(),
            minimum_order_size: "0.001".to_string(),
            tick_size: "0.5".to_string(),
            quote_currency: Ustr::from("USD"),
            finding_settlement_currency: Ustr::from("USD"),
            maintenance_margin_pct: "0.005".to_string(),
            initial_margin_pct: "0.01".to_string(),
            contract_mark_price: Some("45000.50".to_string()),
            contract_size: Some("1.0".to_string()),
            description: Some("Bitcoin Perpetual Futures".to_string()),
            funding_calendar_schedule: Some("0,8,16".to_string()),
            funding_frequency: Some("8h".to_string()),
            funding_rate_cap_lower_pct: Some("-0.0075".to_string()),
            funding_rate_cap_upper_pct: Some("0.0075".to_string()),
            price_band_lower_deviation_pct: Some("0.05".to_string()),
            price_band_upper_deviation_pct: Some("0.05".to_string()),
            price_bands: Some("dynamic".to_string()),
            price_quotation: Some("USD".to_string()),
            underlying_benchmark_price: Some("45000.00".to_string()),
        }
    }

    #[rstest]
    fn test_parse_price() {
        let price = parse_price("100.50", "test_field").unwrap();
        assert_eq!(price.as_f64(), 100.50);
    }

    #[rstest]
    fn test_parse_quantity() {
        let qty = parse_quantity("1.5", "test_field").unwrap();
        assert_eq!(qty.as_f64(), 1.5);
    }

    #[rstest]
    fn test_get_currency() {
        let currency = get_currency("USD");
        assert_eq!(currency.code, Ustr::from("USD"));
    }

    #[rstest]
    fn test_parse_perp_instrument() {
        let definition = create_test_instrument();
        let maker_fee = Decimal::new(2, 4);
        let taker_fee = Decimal::new(5, 4);
        let ts_now = UnixNanos::default();

        let result = parse_perp_instrument(&definition, maker_fee, taker_fee, ts_now, ts_now);
        assert!(result.is_ok());

        let instrument = result.unwrap();
        match instrument {
            InstrumentAny::CryptoPerpetual(perp) => {
                assert_eq!(perp.id.symbol.as_str(), "BTC-PERP");
                assert_eq!(perp.id.venue, *ARCHITECT_VENUE);
                assert_eq!(perp.base_currency.code.as_str(), "BTC");
                assert_eq!(perp.quote_currency.code.as_str(), "USD");
                assert!(!perp.is_inverse);
            }
            _ => panic!("Expected CryptoPerpetual instrument"),
        }
    }

    #[rstest]
    fn test_deserialize_instruments_from_test_data() {
        let test_data = include_str!("../../test_data/http_get_instruments.json");
        let response: ArchitectInstrumentsResponse =
            serde_json::from_str(test_data).expect("Failed to deserialize test data");

        assert_eq!(response.instruments.len(), 3);

        let btc = &response.instruments[0];
        assert_eq!(btc.symbol.as_str(), "BTC-PERP");
        assert_eq!(btc.state, ArchitectInstrumentState::Open);
        assert_eq!(btc.tick_size, "0.5");
        assert_eq!(btc.minimum_order_size, "0.001");
        assert!(btc.contract_mark_price.is_some());

        let eth = &response.instruments[1];
        assert_eq!(eth.symbol.as_str(), "ETH-PERP");
        assert_eq!(eth.state, ArchitectInstrumentState::Open);

        // SOL-PERP is suspended with null optional fields
        let sol = &response.instruments[2];
        assert_eq!(sol.symbol.as_str(), "SOL-PERP");
        assert_eq!(sol.state, ArchitectInstrumentState::Suspended);
        assert!(sol.contract_mark_price.is_none());
        assert!(sol.funding_frequency.is_none());
    }

    #[rstest]
    fn test_parse_all_instruments_from_test_data() {
        let test_data = include_str!("../../test_data/http_get_instruments.json");
        let response: ArchitectInstrumentsResponse =
            serde_json::from_str(test_data).expect("Failed to deserialize test data");

        let maker_fee = Decimal::new(2, 4);
        let taker_fee = Decimal::new(5, 4);
        let ts_now = UnixNanos::default();

        let open_instruments: Vec<_> = response
            .instruments
            .iter()
            .filter(|i| i.state == ArchitectInstrumentState::Open)
            .collect();

        assert_eq!(open_instruments.len(), 2);

        for instrument in open_instruments {
            let result = parse_perp_instrument(instrument, maker_fee, taker_fee, ts_now, ts_now);
            assert!(
                result.is_ok(),
                "Failed to parse {}: {:?}",
                instrument.symbol,
                result.err()
            );
        }
    }
}
