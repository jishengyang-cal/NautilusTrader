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

//! Parsing functions for converting Coinbase WebSocket messages to Nautilus domain types.

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType, BookOrder, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, TradeId},
    instruments::{Instrument, InstrumentAny},
    types::Quantity,
};

use crate::{
    http::parse::{
        coinbase_side_to_aggressor, parse_epoch_secs_timestamp, parse_price, parse_quantity,
        parse_rfc3339_timestamp,
    },
    websocket::messages::{WsBookSide, WsCandle, WsL2DataEvent, WsL2Update, WsTicker, WsTrade},
};

/// Parses a WebSocket trade into a [`TradeTick`].
pub fn parse_ws_trade(
    trade: &WsTrade,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<TradeTick> {
    let price = parse_price(&trade.price, instrument.price_precision())?;
    let size = parse_quantity(&trade.size, instrument.size_precision())?;
    let aggressor_side = coinbase_side_to_aggressor(&trade.side);
    let trade_id = TradeId::new(&trade.trade_id);
    let ts_event = parse_rfc3339_timestamp(&trade.time)?;

    TradeTick::new_checked(
        instrument.id(),
        price,
        size,
        aggressor_side,
        trade_id,
        ts_event,
        ts_init,
    )
}

/// Parses a WebSocket ticker into a [`QuoteTick`].
pub fn parse_ws_ticker(
    ticker: &WsTicker,
    instrument: &InstrumentAny,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<QuoteTick> {
    let bid_price = parse_price(&ticker.best_bid, instrument.price_precision())?;
    let ask_price = parse_price(&ticker.best_ask, instrument.price_precision())?;
    let bid_size = parse_quantity(&ticker.best_bid_quantity, instrument.size_precision())?;
    let ask_size = parse_quantity(&ticker.best_ask_quantity, instrument.size_precision())?;

    QuoteTick::new_checked(
        instrument.id(),
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_event,
        ts_init,
    )
}

/// Parses a WebSocket candle into a [`Bar`].
pub fn parse_ws_candle(
    candle: &WsCandle,
    bar_type: BarType,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<Bar> {
    let open = parse_price(&candle.open, instrument.price_precision())?;
    let high = parse_price(&candle.high, instrument.price_precision())?;
    let low = parse_price(&candle.low, instrument.price_precision())?;
    let close = parse_price(&candle.close, instrument.price_precision())?;
    let volume = parse_quantity(&candle.volume, instrument.size_precision())?;
    let ts_event = parse_epoch_secs_timestamp(&candle.start)?;

    Bar::new_checked(bar_type, open, high, low, close, volume, ts_event, ts_init)
}

/// Parses a WebSocket L2 snapshot event into [`OrderBookDeltas`].
pub fn parse_ws_l2_snapshot(
    event: &WsL2DataEvent,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let ts_event = event
        .updates
        .first()
        .map(|u| parse_rfc3339_timestamp(&u.event_time))
        .transpose()?
        .unwrap_or(ts_init);

    let total = event.updates.len();
    let mut deltas = Vec::with_capacity(total + 1);

    let mut clear = OrderBookDelta::clear(instrument_id, 0, ts_event, ts_init);

    if total == 0 {
        clear.flags |= RecordFlag::F_LAST as u8;
    }
    deltas.push(clear);

    for (i, update) in event.updates.iter().enumerate() {
        let is_last = i == total - 1;
        let delta = parse_l2_delta(
            update,
            instrument_id,
            instrument.price_precision(),
            instrument.size_precision(),
            is_last,
            ts_event,
            ts_init,
        )?;
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses a WebSocket L2 update event into [`OrderBookDeltas`].
pub fn parse_ws_l2_update(
    event: &WsL2DataEvent,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDeltas> {
    let instrument_id = instrument.id();
    let total = event.updates.len();
    let mut deltas = Vec::with_capacity(total);

    for (i, update) in event.updates.iter().enumerate() {
        let is_last = i == total - 1;
        let ts_event = parse_rfc3339_timestamp(&update.event_time)?;
        let price = parse_price(&update.price_level, instrument.price_precision())?;
        let size = parse_quantity(&update.new_quantity, instrument.size_precision())?;
        let side = ws_book_side_to_order_side(update.side);

        let action = if size == Quantity::zero(instrument.size_precision()) {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let mut flags = RecordFlag::F_MBP as u8;

        if is_last {
            flags |= RecordFlag::F_LAST as u8;
        }

        let order = BookOrder::new(side, price, size, 0);
        let delta =
            OrderBookDelta::new_checked(instrument_id, action, order, flags, 0, ts_event, ts_init)?;
        deltas.push(delta);
    }

    OrderBookDeltas::new_checked(instrument_id, deltas)
}

/// Parses a single L2 snapshot level into an [`OrderBookDelta`].
fn parse_l2_delta(
    update: &WsL2Update,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    is_last: bool,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderBookDelta> {
    let price = parse_price(&update.price_level, price_precision)?;
    let size = parse_quantity(&update.new_quantity, size_precision)?;
    let side = ws_book_side_to_order_side(update.side);

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

/// Converts a Coinbase WebSocket book side to a Nautilus order side.
fn ws_book_side_to_order_side(side: WsBookSide) -> OrderSide {
    match side {
        WsBookSide::Bid => OrderSide::Buy,
        WsBookSide::Offer => OrderSide::Sell,
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::bar::BarSpecification,
        enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
        identifiers::{Symbol, Venue},
        instruments::CurrencyPair,
        types::{Currency, Price},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::testing::load_test_fixture,
        websocket::messages::{CoinbaseWsMessage, WsEventType},
    };

    fn test_instrument() -> InstrumentAny {
        let instrument_id =
            InstrumentId::new(Symbol::new("BTC-USD"), Venue::new(Ustr::from("COINBASE")));
        let raw_symbol = Symbol::new("BTC-USD");
        let base_currency = Currency::get_or_create_crypto("BTC");
        let quote_currency = Currency::get_or_create_crypto("USD");

        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            2,
            8,
            Price::from("0.01"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            Some(Quantity::from("0.00000001")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    fn test_parse_ws_trade() {
        let json = load_test_fixture("ws_market_trades.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::MarketTrades { events, .. } => {
                let trade_data = &events[0].trades[0];
                let tick = parse_ws_trade(trade_data, &instrument, ts_init).unwrap();

                assert_eq!(tick.instrument_id, instrument.id());
                assert_eq!(tick.price, Price::from("68900.50"));
                assert_eq!(tick.size, Quantity::from("0.00150000"));
                assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
                assert_eq!(tick.trade_id.as_str(), "995098700");
                assert!(tick.ts_event.as_u64() > 0);
            }
            _ => panic!("Expected MarketTrades"),
        }
    }

    #[rstest]
    fn test_parse_ws_trade_sell_side() {
        let json = load_test_fixture("ws_market_trades.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::MarketTrades { events, .. } => {
                let trade_data = &events[0].trades[1];
                let tick = parse_ws_trade(trade_data, &instrument, ts_init).unwrap();

                assert_eq!(tick.aggressor_side, AggressorSide::Seller);
                assert_eq!(tick.price, Price::from("68900.00"));
                assert_eq!(tick.size, Quantity::from("0.05000000"));
            }
            _ => panic!("Expected MarketTrades"),
        }
    }

    #[rstest]
    fn test_parse_ws_ticker() {
        let json = load_test_fixture("ws_ticker.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::Ticker {
                timestamp, events, ..
            } => {
                let ticker_data = &events[0].tickers[0];
                let ts_event = parse_rfc3339_timestamp(&timestamp).unwrap();
                let quote = parse_ws_ticker(ticker_data, &instrument, ts_event, ts_init).unwrap();

                assert_eq!(quote.instrument_id, instrument.id());
                assert_eq!(quote.bid_price, Price::from("68900.00"));
                assert_eq!(quote.ask_price, Price::from("68901.00"));
                assert_eq!(quote.bid_size, Quantity::from("1.50000000"));
                assert_eq!(quote.ask_size, Quantity::from("0.50000000"));
            }
            _ => panic!("Expected Ticker"),
        }
    }

    #[rstest]
    fn test_parse_ws_candle() {
        let json = load_test_fixture("ws_candles.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        let bar_spec = BarSpecification::new(5, BarAggregation::Minute, PriceType::Last);
        let bar_type = BarType::new(instrument.id(), bar_spec, AggregationSource::External);

        match msg {
            CoinbaseWsMessage::Candles { events, .. } => {
                let candle_data = &events[0].candles[0];
                let bar = parse_ws_candle(candle_data, bar_type, &instrument, ts_init).unwrap();

                assert_eq!(bar.bar_type, bar_type);
                assert_eq!(bar.open, Price::from("68900.00"));
                assert_eq!(bar.high, Price::from("68950.00"));
                assert_eq!(bar.low, Price::from("68850.00"));
                assert_eq!(bar.close, Price::from("68920.50"));
                assert_eq!(bar.volume, Quantity::from("42.15000000"));
                assert_eq!(bar.ts_event.as_u64(), 1_775_521_800_000_000_000);
            }
            _ => panic!("Expected Candles"),
        }
    }

    #[rstest]
    fn test_parse_ws_l2_snapshot() {
        let json = load_test_fixture("ws_l2_data_snapshot.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::L2Data { events, .. } => {
                let event = &events[0];
                assert_eq!(event.event_type, WsEventType::Snapshot);

                let deltas = parse_ws_l2_snapshot(event, &instrument, ts_init).unwrap();
                assert_eq!(deltas.instrument_id, instrument.id());

                // 6 levels + 1 clear = 7 deltas
                assert_eq!(deltas.deltas.len(), 7);

                // First delta is clear
                assert_eq!(deltas.deltas[0].action, BookAction::Clear);

                // Bids
                assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
                assert_eq!(deltas.deltas[1].order.price, Price::from("68900.00"));
                assert_eq!(deltas.deltas[1].order.size, Quantity::from("1.50000000"));

                // Asks
                assert_eq!(deltas.deltas[4].order.side, OrderSide::Sell);
                assert_eq!(deltas.deltas[4].order.price, Price::from("68901.00"));

                // Last delta has F_LAST flag
                let last = deltas.deltas.last().unwrap();
                assert_ne!(last.flags & RecordFlag::F_LAST as u8, 0);
            }
            _ => panic!("Expected L2Data"),
        }
    }

    #[rstest]
    fn test_parse_ws_l2_update() {
        let json = load_test_fixture("ws_l2_data_update.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::L2Data { events, .. } => {
                let event = &events[0];
                assert_eq!(event.event_type, WsEventType::Update);

                let deltas = parse_ws_l2_update(event, &instrument, ts_init).unwrap();
                assert_eq!(deltas.deltas.len(), 2);

                // First update: bid at 68900.00, qty 2.0 -> Update action
                assert_eq!(deltas.deltas[0].order.side, OrderSide::Buy);
                assert_eq!(deltas.deltas[0].order.price, Price::from("68900.00"));
                assert_eq!(deltas.deltas[0].order.size, Quantity::from("2.00000000"));
                assert_eq!(deltas.deltas[0].action, BookAction::Update);

                // Second update: offer at 68901.00, qty 0.0 -> Delete action
                assert_eq!(deltas.deltas[1].order.side, OrderSide::Sell);
                assert_eq!(deltas.deltas[1].action, BookAction::Delete);
                assert_eq!(deltas.deltas[1].order.size, Quantity::from("0.00000000"));

                // Last delta has F_LAST flag
                assert_ne!(deltas.deltas[1].flags & RecordFlag::F_LAST as u8, 0);
            }
            _ => panic!("Expected L2Data"),
        }
    }

    #[rstest]
    fn test_parse_ws_l2_update_zero_quantity_is_delete() {
        let json = load_test_fixture("ws_l2_data_update.json");
        let msg: CoinbaseWsMessage = serde_json::from_str(&json).unwrap();
        let instrument = test_instrument();
        let ts_init = UnixNanos::default();

        match msg {
            CoinbaseWsMessage::L2Data { events, .. } => {
                let event = &events[0];
                let deltas = parse_ws_l2_update(event, &instrument, ts_init).unwrap();

                // The offer with new_quantity "0.00000000" should be a Delete
                let delete_delta = deltas
                    .deltas
                    .iter()
                    .find(|d| d.action == BookAction::Delete)
                    .expect("should have a delete action for zero quantity");
                assert_eq!(delete_delta.order.side, OrderSide::Sell);
            }
            _ => panic!("Expected L2Data"),
        }
    }

    #[rstest]
    fn test_ws_book_side_conversion() {
        assert_eq!(ws_book_side_to_order_side(WsBookSide::Bid), OrderSide::Buy);
        assert_eq!(
            ws_book_side_to_order_side(WsBookSide::Offer),
            OrderSide::Sell
        );
    }
}
