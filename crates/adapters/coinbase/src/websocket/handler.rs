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

//! Feed handler for parsing Coinbase WebSocket messages into Nautilus types.

use std::{fmt::Debug, sync::Arc};

use ahash::AHashMap;
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Bar, BarType, OrderBookDeltas, QuoteTick, TradeTick},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::WebSocketClient;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    common::consts::COINBASE,
    websocket::{
        messages::{CoinbaseWsMessage, CoinbaseWsSubscription, WsEventType},
        parse::{
            parse_ws_candle, parse_ws_l2_snapshot, parse_ws_l2_update, parse_ws_ticker,
            parse_ws_trade,
        },
    },
};

fn instrument_id_from_product(product_id: &ustr::Ustr) -> InstrumentId {
    InstrumentId::from(format!("{product_id}.{COINBASE}").as_str())
}

/// Commands sent from [`super::client::CoinbaseWebSocketClient`] to the feed handler.
pub enum HandlerCommand {
    /// Provides the network-level WebSocket client.
    SetClient(WebSocketClient),
    /// Subscribes to a channel for the given product IDs.
    Subscribe(CoinbaseWsSubscription),
    /// Unsubscribes from a channel.
    Unsubscribe(CoinbaseWsSubscription),
    /// Disconnects the WebSocket.
    Disconnect,
    /// Caches instruments for precision lookups during parsing.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Updates a single instrument in the cache.
    UpdateInstrument(Box<InstrumentAny>),
    /// Registers a bar type for candle parsing.
    AddBarType { key: String, bar_type: BarType },
    /// Removes a bar type registration.
    RemoveBarType { key: String },
}

impl Debug for HandlerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetClient(_) => f.write_str("SetClient"),
            Self::Subscribe(s) => write!(f, "Subscribe({:?})", s.channel),
            Self::Unsubscribe(s) => write!(f, "Unsubscribe({:?})", s.channel),
            Self::Disconnect => f.write_str("Disconnect"),
            Self::InitializeInstruments(v) => write!(f, "InitializeInstruments({})", v.len()),
            Self::UpdateInstrument(i) => write!(f, "UpdateInstrument({})", i.id()),
            Self::AddBarType { key, .. } => write!(f, "AddBarType({key})"),
            Self::RemoveBarType { key } => write!(f, "RemoveBarType({key})"),
        }
    }
}

/// Nautilus-typed messages produced by the feed handler.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Trade tick from market_trades channel.
    Trade(TradeTick),
    /// Quote tick from ticker channel.
    Quote(QuoteTick),
    /// Order book deltas from l2_data channel.
    Deltas(OrderBookDeltas),
    /// Bar from candles channel.
    Bar(Bar),
    /// The connection was re-established after a drop.
    Reconnected,
    /// An error occurred during message processing.
    Error(String),
}

/// Processes raw WebSocket messages into Nautilus domain types.
#[derive(Debug)]
pub struct FeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<std::sync::atomic::AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    instruments: AHashMap<InstrumentId, InstrumentAny>,
    bar_types: AHashMap<String, BarType>,
    buffer: Vec<NautilusWsMessage>,
}

impl FeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub fn new(
        signal: Arc<std::sync::atomic::AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            instruments: AHashMap::new(),
            bar_types: AHashMap::new(),
            buffer: Vec::new(),
        }
    }

    /// Polls for the next output message, processing commands and raw messages.
    ///
    /// Returns `None` when the handler should shut down.
    pub async fn next(&mut self) -> Option<NautilusWsMessage> {
        // Check signal before draining buffer so disconnect takes
        // priority over pending buffered messages
        if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
            self.buffer.clear();
            return None;
        }

        if let Some(msg) = self.buffer.pop() {
            return Some(msg);
        }

        loop {
            if self.signal.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            self.client = Some(client);
                        }
                        HandlerCommand::Subscribe(sub) => {
                            self.send_subscription(&sub).await;
                        }
                        HandlerCommand::Unsubscribe(sub) => {
                            self.send_subscription(&sub).await;
                        }
                        HandlerCommand::Disconnect => {
                            if let Some(client) = self.client.take() {
                                // Transition to CLOSED immediately without waiting
                                // for ACTIVE (avoids blocking during reconnect)
                                client.notify_closed();
                            }
                            return None;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments.insert(inst.id(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments.insert(inst.id(), *inst);
                        }
                        HandlerCommand::AddBarType { key, bar_type } => {
                            self.bar_types.insert(key, bar_type);
                        }
                        HandlerCommand::RemoveBarType { key } => {
                            self.bar_types.remove(&key);
                        }
                    }
                }
                Some(raw) = self.raw_rx.recv() => {
                    match raw {
                        Message::Text(text) => {
                            if let Some(msg) = self.handle_text(&text) {
                                return Some(msg);
                            }
                        }
                        Message::Ping(data) => {
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await
                            {
                                log::error!("Failed to send pong: {e}");
                            }
                        }
                        Message::Close(_) => return None,
                        _ => {}
                    }
                }
                else => return None,
            }
        }
    }

    async fn send_subscription(&self, sub: &CoinbaseWsSubscription) {
        let Some(client) = &self.client else {
            log::warn!("Cannot send subscription, no WebSocket client set");
            return;
        };

        match serde_json::to_string(sub) {
            Ok(json) => {
                if let Err(e) = client.send_text(json, None).await {
                    log::error!("Failed to send subscription: {e}");
                }
            }
            Err(e) => log::error!("Failed to serialize subscription: {e}"),
        }
    }

    fn handle_text(&mut self, text: &str) -> Option<NautilusWsMessage> {
        // Check for reconnection sentinel
        if text == "__RECONNECTED__" {
            return Some(NautilusWsMessage::Reconnected);
        }

        let ts_init = self.clock.get_time_ns();

        let msg: CoinbaseWsMessage = match serde_json::from_str(text) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Failed to parse WS message: {e}");
                return None;
            }
        };

        match msg {
            CoinbaseWsMessage::L2Data { events, .. } => self.handle_l2_events(&events, ts_init),
            CoinbaseWsMessage::MarketTrades { events, .. } => {
                self.handle_market_trades(&events, ts_init)
            }
            CoinbaseWsMessage::Ticker {
                timestamp, events, ..
            }
            | CoinbaseWsMessage::TickerBatch {
                timestamp, events, ..
            } => self.handle_ticker(&events, &timestamp, ts_init),
            CoinbaseWsMessage::Candles { events, .. } => self.handle_candles(&events, ts_init),
            CoinbaseWsMessage::Heartbeats { .. } => None,
            CoinbaseWsMessage::Subscriptions { events, .. } => {
                log::debug!("Subscription confirmed: {events:?}");
                None
            }
            CoinbaseWsMessage::User { events, .. } => {
                log::debug!(
                    "Ignoring {} user events until Coinbase execution support lands",
                    events.len()
                );
                None
            }
            CoinbaseWsMessage::FuturesBalanceSummary { events, .. } => {
                log::debug!(
                    "Ignoring {} futures balance summary events until account-state handling lands",
                    events.len()
                );
                None
            }
            CoinbaseWsMessage::Status { events, .. } => {
                log::debug!(
                    "Ignoring {} status events until venue status handling lands",
                    events.len()
                );
                None
            }
        }
    }

    fn handle_l2_events(
        &mut self,
        events: &[crate::websocket::messages::WsL2DataEvent],
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            let instrument_id = instrument_id_from_product(&event.product_id);

            let instrument = match self.instruments.get(&instrument_id) {
                Some(inst) => inst,
                None => {
                    log::warn!("No instrument cached for {instrument_id}");
                    continue;
                }
            };

            let result = match event.event_type {
                WsEventType::Snapshot => parse_ws_l2_snapshot(event, instrument, ts_init),
                WsEventType::Update => parse_ws_l2_update(event, instrument, ts_init),
            };

            match result {
                Ok(deltas) => {
                    let msg = NautilusWsMessage::Deltas(deltas);

                    if first.is_none() {
                        first = Some(msg);
                    } else {
                        self.buffer.push(msg);
                    }
                }
                Err(e) => log::warn!("Failed to parse L2 event: {e}"),
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }

    fn handle_market_trades(
        &mut self,
        events: &[crate::websocket::messages::WsMarketTradesEvent],
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        for event in events {
            for trade in &event.trades {
                let instrument_id = instrument_id_from_product(&trade.product_id);

                let instrument = match self.instruments.get(&instrument_id) {
                    Some(inst) => inst,
                    None => {
                        log::warn!("No instrument cached for {instrument_id}");
                        continue;
                    }
                };

                match parse_ws_trade(trade, instrument, ts_init) {
                    Ok(tick) => {
                        self.buffer_remaining_trades(events, event, trade, ts_init);
                        // Reverse so pop() drains in exchange order
                        self.buffer.reverse();
                        return Some(NautilusWsMessage::Trade(tick));
                    }
                    Err(e) => log::warn!("Failed to parse trade: {e}"),
                }
            }
        }
        None
    }

    fn buffer_remaining_trades(
        &mut self,
        events: &[crate::websocket::messages::WsMarketTradesEvent],
        current_event: &crate::websocket::messages::WsMarketTradesEvent,
        current_trade: &crate::websocket::messages::WsTrade,
        ts_init: UnixNanos,
    ) {
        let mut found_current = false;

        for event in events {
            let is_current_event = std::ptr::eq(event, current_event);

            for trade in &event.trades {
                if !found_current {
                    if is_current_event && std::ptr::eq(trade, current_trade) {
                        found_current = true;
                    }
                    continue;
                }

                let instrument_id = instrument_id_from_product(&trade.product_id);

                if let Some(instrument) = self.instruments.get(&instrument_id)
                    && let Ok(tick) = parse_ws_trade(trade, instrument, ts_init)
                {
                    self.buffer.push(NautilusWsMessage::Trade(tick));
                }
            }
        }
    }

    fn handle_ticker(
        &mut self,
        events: &[crate::websocket::messages::WsTickerEvent],
        timestamp: &str,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let ts_event = crate::http::parse::parse_rfc3339_timestamp(timestamp).unwrap_or(ts_init);

        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            for ticker in &event.tickers {
                let instrument_id = instrument_id_from_product(&ticker.product_id);

                let instrument = match self.instruments.get(&instrument_id) {
                    Some(inst) => inst,
                    None => {
                        log::warn!("No instrument cached for {instrument_id}");
                        continue;
                    }
                };

                match parse_ws_ticker(ticker, instrument, ts_event, ts_init) {
                    Ok(quote) => {
                        let msg = NautilusWsMessage::Quote(quote);

                        if first.is_none() {
                            first = Some(msg);
                        } else {
                            self.buffer.push(msg);
                        }
                    }
                    Err(e) => log::warn!("Failed to parse ticker: {e}"),
                }
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }

    fn handle_candles(
        &mut self,
        events: &[crate::websocket::messages::WsCandlesEvent],
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut first: Option<NautilusWsMessage> = None;

        for event in events {
            for candle in &event.candles {
                let key = candle.product_id.as_str();

                let bar_type = match self.bar_types.get(key) {
                    Some(bt) => *bt,
                    None => {
                        log::debug!("No bar type registered for {key}");
                        continue;
                    }
                };

                let instrument_id = instrument_id_from_product(&candle.product_id);

                let instrument = match self.instruments.get(&instrument_id) {
                    Some(inst) => inst,
                    None => {
                        log::warn!("No instrument cached for {instrument_id}");
                        continue;
                    }
                };

                match parse_ws_candle(candle, bar_type, instrument, ts_init) {
                    Ok(bar) => {
                        let msg = NautilusWsMessage::Bar(bar);

                        if first.is_none() {
                            first = Some(msg);
                        } else {
                            self.buffer.push(msg);
                        }
                    }
                    Err(e) => log::warn!("Failed to parse candle: {e}"),
                }
            }
        }

        if first.is_some() {
            self.buffer.reverse();
        }
        first
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::AtomicBool};

    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_fixture;

    fn test_handler() -> FeedHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        FeedHandler::new(Arc::new(AtomicBool::new(false)), cmd_rx, raw_rx)
    }

    #[rstest]
    fn test_handle_text_ignores_user_channel_until_execution_support() {
        let json = load_test_fixture("ws_user.json");
        let mut handler = test_handler();

        assert!(handler.handle_text(&json).is_none());
        assert!(handler.buffer.is_empty());
    }

    #[rstest]
    fn test_handle_text_ignores_status_channel() {
        let json = r#"{
          "channel": "status",
          "client_id": "",
          "timestamp": "2023-02-09T20:29:49.753424311Z",
          "sequence_num": 0,
          "events": [
            {
              "type": "snapshot",
              "products": [
                {
                  "product_type": "SPOT",
                  "id": "BTC-USD",
                  "base_currency": "BTC",
                  "quote_currency": "USD",
                  "base_increment": "0.00000001",
                  "quote_increment": "0.01",
                  "display_name": "BTC/USD",
                  "status": "online",
                  "status_message": "",
                  "min_market_funds": "1"
                }
              ]
            }
          ]
        }"#;
        let mut handler = test_handler();

        assert!(handler.handle_text(json).is_none());
        assert!(handler.buffer.is_empty());
    }

    #[rstest]
    fn test_handle_text_ignores_futures_balance_summary_channel() {
        let json = r#"{
          "channel": "futures_balance_summary",
          "client_id": "",
          "timestamp": "2023-02-09T20:33:57.609931463Z",
          "sequence_num": 0,
          "events": [
            {
              "type": "snapshot",
              "fcm_balance_summary": {
                "futures_buying_power": "100.00",
                "total_usd_balance": "200.00",
                "cbi_usd_balance": "300.00",
                "cfm_usd_balance": "400.00",
                "total_open_orders_hold_amount": "500.00",
                "unrealized_pnl": "600.00",
                "daily_realized_pnl": "0",
                "initial_margin": "700.00",
                "available_margin": "800.00",
                "liquidation_threshold": "900.00",
                "liquidation_buffer_amount": "1000.00",
                "liquidation_buffer_percentage": "1000",
                "intraday_margin_window_measure": {
                  "margin_window_type": "FCM_MARGIN_WINDOW_TYPE_INTRADAY",
                  "margin_level": "MARGIN_LEVEL_TYPE_BASE",
                  "initial_margin": "100.00",
                  "maintenance_margin": "200.00",
                  "liquidation_buffer_percentage": "1000",
                  "total_hold": "100.00",
                  "futures_buying_power": "400.00"
                },
                "overnight_margin_window_measure": {
                  "margin_window_type": "FCM_MARGIN_WINDOW_TYPE_OVERNIGHT",
                  "margin_level": "MARGIN_LEVEL_TYPE_BASE",
                  "initial_margin": "300.00",
                  "maintenance_margin": "200.00",
                  "liquidation_buffer_percentage": "1000",
                  "total_hold": "-30.00",
                  "futures_buying_power": "2000.00"
                }
              }
            }
          ]
        }"#;
        let mut handler = test_handler();

        assert!(handler.handle_text(json).is_none());
        assert!(handler.buffer.is_empty());
    }
}
