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

//! A common in-memory `MessageBus` supporting multiple messaging patterns:
//!
//! - Point-to-Point
//! - Pub/Sub
//! - Request/Response

pub mod core;
pub mod database;
pub mod handler;
pub mod matching;
pub mod message;
pub mod mstr;
pub mod stubs;
pub mod switchboard;
pub mod typed_endpoints;
pub mod typed_handler;
pub mod typed_router;

pub use core::{MessageBus, Subscription};
use std::{
    any::Any,
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use handler::ShareableMessageHandler;
use matching::is_matching_backtracking;
pub use mstr::{Endpoint, MStr, Pattern, Topic};
use nautilus_core::UUID4;
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, GreeksData, IndexPriceUpdate, InstrumentClose, MarkPriceUpdate,
        OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
    },
    events::{AccountState, OrderEventAny, PositionEvent},
    instruments::InstrumentAny,
    orderbook::OrderBook,
    orders::OrderAny,
    position::Position,
};
pub use typed_endpoints::EndpointMap;
pub use typed_handler::{CallbackHandler, Handler, TypedHandler};
pub use typed_router::{TopicRouter, TypedSubscription};

use crate::messages::data::DataResponse;
pub use crate::msgbus::message::BusMessage;

// MessageBus is designed for single-threaded use within each async runtime.
// Thread-local storage ensures each thread gets its own instance, eliminating
// the need for unsafe Send/Sync implementations that were previously required
// for global static storage.
thread_local! {
    static MESSAGE_BUS: OnceCell<Rc<RefCell<MessageBus>>> = const { OnceCell::new() };
}

/// Sets the thread-local message bus.
///
/// # Panics
///
/// Panics if a message bus has already been set for this thread.
pub fn set_message_bus(msgbus: Rc<RefCell<MessageBus>>) {
    MESSAGE_BUS.with(|bus| {
        assert!(
            bus.set(msgbus).is_ok(),
            "Failed to set MessageBus: already initialized for this thread"
        );
    });
}

/// Gets the thread-local message bus.
///
/// If no message bus has been set for this thread, a default one is created and initialized.
pub fn get_message_bus() -> Rc<RefCell<MessageBus>> {
    MESSAGE_BUS.with(|bus| {
        bus.get_or_init(|| {
            let msgbus = MessageBus::default();
            Rc::new(RefCell::new(msgbus))
        })
        .clone()
    })
}

/// Registers a handler for an endpoint using runtime type dispatch (Any).
pub fn register_any(endpoint: MStr<Endpoint>, handler: ShareableMessageHandler) {
    log::debug!(
        "Registering endpoint '{endpoint}' with handler ID {}",
        handler.0.id(),
    );
    get_message_bus()
        .borrow_mut()
        .endpoints
        .insert(endpoint, handler);
}

pub fn register_response_handler(correlation_id: &UUID4, handler: ShareableMessageHandler) {
    if let Err(e) = get_message_bus()
        .borrow_mut()
        .register_response_handler(correlation_id, handler)
    {
        log::error!("Failed to register request handler: {e}");
    }
}

pub fn register_quote_endpoint(endpoint: MStr<Endpoint>, handler: TypedHandler<QuoteTick>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_quotes
        .register(endpoint, handler);
}

pub fn register_trade_endpoint(endpoint: MStr<Endpoint>, handler: TypedHandler<TradeTick>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_trades
        .register(endpoint, handler);
}

pub fn register_bar_endpoint(endpoint: MStr<Endpoint>, handler: TypedHandler<Bar>) {
    get_message_bus()
        .borrow_mut()
        .endpoints_bars
        .register(endpoint, handler);
}

pub fn register_order_event_endpoint(
    endpoint: MStr<Endpoint>,
    handler: TypedHandler<OrderEventAny>,
) {
    get_message_bus()
        .borrow_mut()
        .endpoints_order_events
        .register(endpoint, handler);
}

/// Deregisters the handler for an endpoint (Any-based).
pub fn deregister_any(endpoint: MStr<Endpoint>) {
    log::debug!("Deregistering endpoint '{endpoint}'");
    get_message_bus()
        .borrow_mut()
        .endpoints
        .shift_remove(&endpoint);
}

/// Subscribes a handler to a pattern using runtime type dispatch (Any).
///
/// # Warnings
///
/// Assigning priority handling is an advanced feature which *shouldn't
/// normally be needed by most users*. **Only assign a higher priority to the
/// subscription if you are certain of what you're doing**. If an inappropriate
/// priority is assigned then the handler may receive messages before core
/// system components have been able to process necessary calculations and
/// produce potential side effects for logically sound behavior.
pub fn subscribe_any(
    pattern: MStr<Pattern>,
    handler: ShareableMessageHandler,
    priority: Option<u8>,
) {
    let msgbus = get_message_bus();
    let mut msgbus_ref_mut = msgbus.borrow_mut();
    let sub = Subscription::new(pattern, handler, priority);

    log::debug!(
        "Subscribing {:?} for pattern '{}'",
        sub.handler,
        sub.pattern
    );

    if msgbus_ref_mut.subscriptions.contains(&sub) {
        log::warn!("{sub:?} already exists");
        return;
    }

    for (topic, subs) in &mut msgbus_ref_mut.topics {
        if is_matching_backtracking(*topic, sub.pattern) {
            subs.push(sub.clone());
            subs.sort();
            log::debug!("Added subscription for '{topic}'");
        }
    }

    msgbus_ref_mut.subscriptions.insert(sub);
}

pub fn subscribe_instruments(
    pattern: MStr<Pattern>,
    handler: TypedHandler<InstrumentAny>,
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_instruments.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

pub fn subscribe_instrument_close(
    pattern: MStr<Pattern>,
    handler: TypedHandler<InstrumentClose>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_instrument_close
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_book_snapshots(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderBook>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_book_snapshots
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_quotes(
    pattern: MStr<Pattern>,
    handler: TypedHandler<QuoteTick>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_quotes
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_trades(
    pattern: MStr<Pattern>,
    handler: TypedHandler<TradeTick>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_trades
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_mark_prices(
    pattern: MStr<Pattern>,
    handler: TypedHandler<MarkPriceUpdate>,
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_mark_prices.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

pub fn subscribe_index_prices(
    pattern: MStr<Pattern>,
    handler: TypedHandler<IndexPriceUpdate>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_index_prices
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_funding_rates(
    pattern: MStr<Pattern>,
    handler: TypedHandler<FundingRateUpdate>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_funding_rates
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_bars(pattern: MStr<Pattern>, handler: TypedHandler<Bar>, priority: Option<u8>) {
    get_message_bus()
        .borrow_mut()
        .router_bars
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_deltas(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderBookDeltas>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_deltas
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_depth10(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderBookDepth10>,
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_depth10.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

pub fn subscribe_order_events(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderEventAny>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_order_events
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_position_events(
    pattern: MStr<Pattern>,
    handler: TypedHandler<PositionEvent>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_position_events
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_account_state(
    pattern: MStr<Pattern>,
    handler: TypedHandler<AccountState>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_account_state
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_orders(
    pattern: MStr<Pattern>,
    handler: TypedHandler<OrderAny>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_orders
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

pub fn subscribe_positions(
    pattern: MStr<Pattern>,
    handler: TypedHandler<Position>,
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_positions.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

pub fn subscribe_greeks(
    pattern: MStr<Pattern>,
    handler: TypedHandler<GreeksData>,
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_greeks
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

#[cfg(feature = "defi")]
pub fn subscribe_defi_blocks(
    pattern: MStr<Pattern>,
    handler: TypedHandler<nautilus_model::defi::Block>, // nautilus-import-ok
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_defi_blocks.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

#[cfg(feature = "defi")]
pub fn subscribe_defi_pools(
    pattern: MStr<Pattern>,
    handler: TypedHandler<nautilus_model::defi::Pool>, // nautilus-import-ok
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_defi_pools.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

#[cfg(feature = "defi")]
pub fn subscribe_defi_swaps(
    pattern: MStr<Pattern>,
    handler: TypedHandler<nautilus_model::defi::PoolSwap>, // nautilus-import-ok
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_defi_swaps.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

#[cfg(feature = "defi")]
pub fn subscribe_defi_liquidity(
    pattern: MStr<Pattern>,
    handler: TypedHandler<nautilus_model::defi::PoolLiquidityUpdate>, // nautilus-import-ok
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_liquidity
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

#[cfg(feature = "defi")]
pub fn subscribe_defi_collects(
    pattern: MStr<Pattern>,
    handler: TypedHandler<nautilus_model::defi::PoolFeeCollect>, // nautilus-import-ok
    priority: Option<u8>,
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_collects
        .subscribe(pattern, handler, priority.unwrap_or(0));
}

#[cfg(feature = "defi")]
pub fn subscribe_defi_flash(
    pattern: MStr<Pattern>,
    handler: TypedHandler<nautilus_model::defi::PoolFlash>, // nautilus-import-ok
    priority: Option<u8>,
) {
    get_message_bus().borrow_mut().router_defi_flash.subscribe(
        pattern,
        handler,
        priority.unwrap_or(0),
    );
}

pub fn unsubscribe_instruments(pattern: MStr<Pattern>, handler: &TypedHandler<InstrumentAny>) {
    get_message_bus()
        .borrow_mut()
        .router_instruments
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_deltas(pattern: MStr<Pattern>, handler: &TypedHandler<OrderBookDeltas>) {
    get_message_bus()
        .borrow_mut()
        .router_deltas
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_depth10(pattern: MStr<Pattern>, handler: &TypedHandler<OrderBookDepth10>) {
    get_message_bus()
        .borrow_mut()
        .router_depth10
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_quotes(pattern: MStr<Pattern>, handler: &TypedHandler<QuoteTick>) {
    get_message_bus()
        .borrow_mut()
        .router_quotes
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_trades(pattern: MStr<Pattern>, handler: &TypedHandler<TradeTick>) {
    get_message_bus()
        .borrow_mut()
        .router_trades
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_mark_prices(pattern: MStr<Pattern>, handler: &TypedHandler<MarkPriceUpdate>) {
    get_message_bus()
        .borrow_mut()
        .router_mark_prices
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_index_prices(pattern: MStr<Pattern>, handler: &TypedHandler<IndexPriceUpdate>) {
    get_message_bus()
        .borrow_mut()
        .router_index_prices
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_bars(pattern: MStr<Pattern>, handler: &TypedHandler<Bar>) {
    get_message_bus()
        .borrow_mut()
        .router_bars
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_order_events(pattern: MStr<Pattern>, handler: &TypedHandler<OrderEventAny>) {
    get_message_bus()
        .borrow_mut()
        .router_order_events
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_position_events(pattern: MStr<Pattern>, handler: &TypedHandler<PositionEvent>) {
    get_message_bus()
        .borrow_mut()
        .router_position_events
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_account_state(pattern: MStr<Pattern>, handler: &TypedHandler<AccountState>) {
    get_message_bus()
        .borrow_mut()
        .router_account_state
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_funding_rates(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<FundingRateUpdate>,
) {
    get_message_bus()
        .borrow_mut()
        .router_funding_rates
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_instrument_close(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<InstrumentClose>,
) {
    get_message_bus()
        .borrow_mut()
        .router_instrument_close
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_book_snapshots(pattern: MStr<Pattern>, handler: &TypedHandler<OrderBook>) {
    get_message_bus()
        .borrow_mut()
        .router_book_snapshots
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_orders(pattern: MStr<Pattern>, handler: &TypedHandler<OrderAny>) {
    get_message_bus()
        .borrow_mut()
        .router_orders
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_positions(pattern: MStr<Pattern>, handler: &TypedHandler<Position>) {
    get_message_bus()
        .borrow_mut()
        .router_positions
        .unsubscribe(pattern, handler);
}

pub fn unsubscribe_greeks(pattern: MStr<Pattern>, handler: &TypedHandler<GreeksData>) {
    get_message_bus()
        .borrow_mut()
        .router_greeks
        .unsubscribe(pattern, handler);
}

#[cfg(feature = "defi")]
pub fn unsubscribe_defi_blocks(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<nautilus_model::defi::Block>, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_blocks
        .unsubscribe(pattern, handler);
}

#[cfg(feature = "defi")]
pub fn unsubscribe_defi_pools(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<nautilus_model::defi::Pool>, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_pools
        .unsubscribe(pattern, handler);
}

#[cfg(feature = "defi")]
pub fn unsubscribe_defi_swaps(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<nautilus_model::defi::PoolSwap>, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_swaps
        .unsubscribe(pattern, handler);
}

#[cfg(feature = "defi")]
pub fn unsubscribe_defi_liquidity(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<nautilus_model::defi::PoolLiquidityUpdate>, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_liquidity
        .unsubscribe(pattern, handler);
}

#[cfg(feature = "defi")]
pub fn unsubscribe_defi_collects(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<nautilus_model::defi::PoolFeeCollect>, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_collects
        .unsubscribe(pattern, handler);
}

#[cfg(feature = "defi")]
pub fn unsubscribe_defi_flash(
    pattern: MStr<Pattern>,
    handler: &TypedHandler<nautilus_model::defi::PoolFlash>, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_flash
        .unsubscribe(pattern, handler);
}

/// Unsubscribes a handler from a pattern (Any-based).
pub fn unsubscribe_any(pattern: MStr<Pattern>, handler: ShareableMessageHandler) {
    log::debug!("Unsubscribing {handler:?} from pattern '{pattern}'");

    let sub = core::Subscription::new(pattern, handler, None);

    get_message_bus()
        .borrow_mut()
        .topics
        .values_mut()
        .for_each(|subs| {
            if let Ok(index) = subs.binary_search(&sub) {
                subs.remove(index);
            }
        });

    let removed = get_message_bus().borrow_mut().subscriptions.remove(&sub);

    if removed {
        log::debug!("Handler for pattern '{pattern}' was removed");
    } else {
        log::debug!("No matching handler for pattern '{pattern}' was found");
    }
}

/// Checks if a handler is subscribed to a pattern (Any-based).
pub fn is_subscribed_any<T: AsRef<str>>(pattern: T, handler: ShareableMessageHandler) -> bool {
    let pattern = MStr::from(pattern.as_ref());
    let sub = Subscription::new(pattern, handler, None);
    get_message_bus().borrow().subscriptions.contains(&sub)
}

pub fn subscriptions_count_any<S: AsRef<str>>(topic: S) -> usize {
    get_message_bus().borrow().subscriptions_count(topic)
}

pub fn subscriber_count_deltas(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_deltas
        .subscriber_count(topic)
}

pub fn subscriber_count_depth10(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_depth10
        .subscriber_count(topic)
}

pub fn subscriber_count_book_snapshots(topic: MStr<Topic>) -> usize {
    get_message_bus()
        .borrow()
        .router_book_snapshots
        .subscriber_count(topic)
}

/// Publishes a message to the topic using runtime type dispatch (Any).
pub fn publish_any(topic: MStr<Topic>, message: &dyn Any) {
    let matching_subs = get_message_bus()
        .borrow_mut()
        .inner_matching_subscriptions(topic);

    for sub in matching_subs {
        sub.handler.0.handle(message);
    }
}

pub fn publish_quote(topic: MStr<Topic>, quote: &QuoteTick) {
    get_message_bus()
        .borrow_mut()
        .router_quotes
        .publish(topic, quote);
}

pub fn publish_trade(topic: MStr<Topic>, trade: &TradeTick) {
    get_message_bus()
        .borrow_mut()
        .router_trades
        .publish(topic, trade);
}

pub fn publish_bar(topic: MStr<Topic>, bar: &Bar) {
    get_message_bus()
        .borrow_mut()
        .router_bars
        .publish(topic, bar);
}

pub fn publish_deltas(topic: MStr<Topic>, deltas: &OrderBookDeltas) {
    get_message_bus()
        .borrow_mut()
        .router_deltas
        .publish(topic, deltas);
}

pub fn publish_depth10(topic: MStr<Topic>, depth: &OrderBookDepth10) {
    get_message_bus()
        .borrow_mut()
        .router_depth10
        .publish(topic, depth);
}

pub fn publish_order_event(topic: MStr<Topic>, event: &OrderEventAny) {
    get_message_bus()
        .borrow_mut()
        .router_order_events
        .publish(topic, event);
}

pub fn publish_position_event(topic: MStr<Topic>, event: &PositionEvent) {
    get_message_bus()
        .borrow_mut()
        .router_position_events
        .publish(topic, event);
}

pub fn publish_account_state(topic: MStr<Topic>, state: &AccountState) {
    get_message_bus()
        .borrow_mut()
        .router_account_state
        .publish(topic, state);
}

pub fn publish_instrument(topic: MStr<Topic>, instrument: &InstrumentAny) {
    get_message_bus()
        .borrow_mut()
        .router_instruments
        .publish(topic, instrument);
}

pub fn publish_mark_price(topic: MStr<Topic>, mark_price: &MarkPriceUpdate) {
    get_message_bus()
        .borrow_mut()
        .router_mark_prices
        .publish(topic, mark_price);
}

pub fn publish_index_price(topic: MStr<Topic>, index_price: &IndexPriceUpdate) {
    get_message_bus()
        .borrow_mut()
        .router_index_prices
        .publish(topic, index_price);
}

pub fn publish_funding_rate(topic: MStr<Topic>, funding_rate: &FundingRateUpdate) {
    get_message_bus()
        .borrow_mut()
        .router_funding_rates
        .publish(topic, funding_rate);
}

pub fn publish_instrument_close(topic: MStr<Topic>, close: &InstrumentClose) {
    get_message_bus()
        .borrow_mut()
        .router_instrument_close
        .publish(topic, close);
}

pub fn publish_book_snapshot(topic: MStr<Topic>, book: &OrderBook) {
    get_message_bus()
        .borrow_mut()
        .router_book_snapshots
        .publish(topic, book);
}

pub fn publish_order(topic: MStr<Topic>, order: &OrderAny) {
    get_message_bus()
        .borrow_mut()
        .router_orders
        .publish(topic, order);
}

pub fn publish_position(topic: MStr<Topic>, position: &Position) {
    get_message_bus()
        .borrow_mut()
        .router_positions
        .publish(topic, position);
}

pub fn publish_greeks(topic: MStr<Topic>, greeks: &GreeksData) {
    get_message_bus()
        .borrow_mut()
        .router_greeks
        .publish(topic, greeks);
}

#[cfg(feature = "defi")]
pub fn publish_defi_block(
    topic: MStr<Topic>,
    block: &nautilus_model::defi::Block, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_blocks
        .publish(topic, block);
}

#[cfg(feature = "defi")]
pub fn publish_defi_pool(
    topic: MStr<Topic>,
    pool: &nautilus_model::defi::Pool, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_pools
        .publish(topic, pool);
}

#[cfg(feature = "defi")]
pub fn publish_defi_swap(
    topic: MStr<Topic>,
    swap: &nautilus_model::defi::PoolSwap, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_swaps
        .publish(topic, swap);
}

#[cfg(feature = "defi")]
pub fn publish_defi_liquidity(
    topic: MStr<Topic>,
    update: &nautilus_model::defi::PoolLiquidityUpdate, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_liquidity
        .publish(topic, update);
}

#[cfg(feature = "defi")]
pub fn publish_defi_collect(
    topic: MStr<Topic>,
    collect: &nautilus_model::defi::PoolFeeCollect, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_collects
        .publish(topic, collect);
}

#[cfg(feature = "defi")]
pub fn publish_defi_flash(
    topic: MStr<Topic>,
    flash: &nautilus_model::defi::PoolFlash, // nautilus-import-ok
) {
    get_message_bus()
        .borrow_mut()
        .router_defi_flash
        .publish(topic, flash);
}

/// Sends a message to an endpoint using runtime type dispatch (Any).
pub fn send_any(endpoint: MStr<Endpoint>, message: &dyn Any) {
    let handler = get_message_bus().borrow().get_endpoint(endpoint).cloned();
    if let Some(handler) = handler {
        handler.0.handle(message);
    } else {
        log::error!("send_any: no registered endpoint '{endpoint}'");
    }
}

/// Sends a message to an endpoint, converting to Any (convenience wrapper).
pub fn send_any_value<T: 'static>(endpoint: MStr<Endpoint>, message: T) {
    let handler = get_message_bus().borrow().get_endpoint(endpoint).cloned();
    if let Some(handler) = handler {
        handler.0.handle(&message);
    } else {
        log::error!("send_any_value: no registered endpoint '{endpoint}'");
    }
}

/// Sends the [`DataResponse`] to the registered correlation ID handler.
pub fn send_response(correlation_id: &UUID4, message: &DataResponse) {
    let handler = get_message_bus()
        .borrow()
        .get_response_handler(correlation_id)
        .cloned();

    if let Some(handler) = handler {
        match message {
            DataResponse::Data(resp) => handler.0.handle(resp),
            DataResponse::Instrument(resp) => handler.0.handle(resp.as_ref()),
            DataResponse::Instruments(resp) => handler.0.handle(resp),
            DataResponse::Book(resp) => handler.0.handle(resp),
            DataResponse::Quotes(resp) => handler.0.handle(resp),
            DataResponse::Trades(resp) => handler.0.handle(resp),
            DataResponse::Bars(resp) => handler.0.handle(resp),
        }
    } else {
        log::error!("send_response: handler not found for correlation_id '{correlation_id}'");
    }
}

pub fn send_quote(endpoint: MStr<Endpoint>, quote: &QuoteTick) {
    get_message_bus()
        .borrow()
        .endpoints_quotes
        .send(endpoint, quote);
}

pub fn send_trade(endpoint: MStr<Endpoint>, trade: &TradeTick) {
    get_message_bus()
        .borrow()
        .endpoints_trades
        .send(endpoint, trade);
}

pub fn send_bar(endpoint: MStr<Endpoint>, bar: &Bar) {
    get_message_bus()
        .borrow()
        .endpoints_bars
        .send(endpoint, bar);
}

pub fn send_order_event(endpoint: MStr<Endpoint>, event: &OrderEventAny) {
    get_message_bus()
        .borrow()
        .endpoints_order_events
        .send(endpoint, event);
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_model::{
        data::{Bar, OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick},
        identifiers::InstrumentId,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_typed_quote_publish_subscribe_integration() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |quote: &QuoteTick| {
            received_clone.borrow_mut().push(*quote);
        });

        subscribe_quotes("data.quotes.*".into(), handler, None);

        let quote = QuoteTick::default();
        publish_quote("data.quotes.TEST".into(), &quote);
        publish_quote("data.quotes.TEST".into(), &quote);

        assert_eq!(received.borrow().len(), 2);
    }

    #[rstest]
    fn test_typed_trade_publish_subscribe_integration() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |trade: &TradeTick| {
            received_clone.borrow_mut().push(*trade);
        });

        subscribe_trades("data.trades.*".into(), handler, None);

        let trade = TradeTick::default();
        publish_trade("data.trades.TEST".into(), &trade);

        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_bar_publish_subscribe_integration() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |bar: &Bar| {
            received_clone.borrow_mut().push(*bar);
        });

        subscribe_bars("data.bars.*".into(), handler, None);

        let bar = Bar::default();
        publish_bar("data.bars.TEST".into(), &bar);

        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_deltas_publish_subscribe_integration() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |deltas: &OrderBookDeltas| {
            received_clone.borrow_mut().push(deltas.clone());
        });

        subscribe_deltas("data.book.deltas.*".into(), handler, None);

        let instrument_id = InstrumentId::from("TEST.VENUE");
        let delta = OrderBookDelta::clear(instrument_id, 0, 1.into(), 2.into());
        let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);
        publish_deltas("data.book.deltas.TEST".into(), &deltas);

        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_unsubscribe_stops_delivery() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from_with_id("unsub-test", move |quote: &QuoteTick| {
            received_clone.borrow_mut().push(*quote);
        });

        subscribe_quotes("data.quotes.UNSUB".into(), handler.clone(), None);

        let quote = QuoteTick::default();
        publish_quote("data.quotes.UNSUB".into(), &quote);
        assert_eq!(received.borrow().len(), 1);

        unsubscribe_quotes("data.quotes.UNSUB".into(), &handler);

        publish_quote("data.quotes.UNSUB".into(), &quote);
        assert_eq!(received.borrow().len(), 1);
    }

    #[rstest]
    fn test_typed_wildcard_pattern_matching() {
        let _msgbus = get_message_bus();
        let received = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();

        let handler = TypedHandler::from(move |quote: &QuoteTick| {
            received_clone.borrow_mut().push(*quote);
        });

        subscribe_quotes("data.quotes.WILD.*".into(), handler, None);

        let quote = QuoteTick::default();
        publish_quote("data.quotes.WILD.AAPL".into(), &quote);
        publish_quote("data.quotes.WILD.MSFT".into(), &quote);
        publish_quote("data.quotes.OTHER.AAPL".into(), &quote);

        assert_eq!(received.borrow().len(), 2);
    }

    #[rstest]
    fn test_typed_priority_ordering() {
        let _msgbus = get_message_bus();
        let order = Rc::new(RefCell::new(Vec::new()));

        let order1 = order.clone();
        let handler_low = TypedHandler::from_with_id("low-priority", move |_: &QuoteTick| {
            order1.borrow_mut().push("low");
        });

        let order2 = order.clone();
        let handler_high = TypedHandler::from_with_id("high-priority", move |_: &QuoteTick| {
            order2.borrow_mut().push("high");
        });

        subscribe_quotes("data.quotes.PRIO".into(), handler_low, Some(5));
        subscribe_quotes("data.quotes.PRIO".into(), handler_high, Some(10));

        let quote = QuoteTick::default();
        publish_quote("data.quotes.PRIO".into(), &quote);

        assert_eq!(*order.borrow(), vec!["high", "low"]);
    }

    #[rstest]
    fn test_typed_routing_isolation() {
        let _msgbus = get_message_bus();
        let quote_received = Rc::new(RefCell::new(false));
        let trade_received = Rc::new(RefCell::new(false));

        let qr = quote_received.clone();
        let quote_handler = TypedHandler::from(move |_: &QuoteTick| {
            *qr.borrow_mut() = true;
        });

        let tr = trade_received.clone();
        let trade_handler = TypedHandler::from(move |_: &TradeTick| {
            *tr.borrow_mut() = true;
        });

        subscribe_quotes("data.iso.*".into(), quote_handler, None);
        subscribe_trades("data.iso.*".into(), trade_handler, None);

        let quote = QuoteTick::default();
        publish_quote("data.iso.TEST".into(), &quote);

        assert!(*quote_received.borrow());
        assert!(!*trade_received.borrow());
    }
}
