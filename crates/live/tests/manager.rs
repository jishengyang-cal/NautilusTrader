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

//! Integration tests for ExecutionManager.
//!
//! These tests focus on observable behavior through the public API.
//! Internal state tests are in the in-module tests in manager.rs.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, clock::TestClock};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::ExecutionEngine;
use nautilus_live::manager::{ExecutionManager, ExecutionManagerConfig, ExecutionReport};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType,
        PositionSideSpecified, TimeInForce,
    },
    events::{OrderEventAny, OrderFilled, account::state::AccountState},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        TraderId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal_macros::dec;

struct TestContext {
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    manager: ExecutionManager,
    exec_engine: Rc<RefCell<ExecutionEngine>>,
}

impl TestContext {
    fn new() -> Self {
        Self::with_config(ExecutionManagerConfig::default())
    }

    fn with_config(config: ExecutionManagerConfig) -> Self {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        // Add test account to cache (required for position creation in ExecutionEngine)
        let account_state = AccountState::new(
            test_account_id(),
            AccountType::Margin,
            vec![AccountBalance::new(
                Money::from("1000000 USDT"),
                Money::from("0 USDT"),
                Money::from("1000000 USDT"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(Currency::USDT()),
        );
        let account = AccountAny::Margin(MarginAccount::new(account_state, true));
        cache.borrow_mut().add_account(account).unwrap();

        let manager = ExecutionManager::new(clock.clone(), cache.clone(), config);
        let mut engine = ExecutionEngine::new(clock.clone(), cache.clone(), None);

        // Register hedging mode for EXTERNAL strategy (used by external/reconciliation orders)
        engine.register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Hedging);

        let exec_engine = Rc::new(RefCell::new(engine));
        Self {
            clock,
            cache,
            manager,
            exec_engine,
        }
    }

    fn advance_time(&self, delta_nanos: u64) {
        let current = self.clock.borrow().get_time_ns();
        self.clock
            .borrow_mut()
            .advance_time(UnixNanos::from(current.as_u64() + delta_nanos), true);
    }

    fn add_instrument(&self, instrument: InstrumentAny) {
        self.cache.borrow_mut().add_instrument(instrument).unwrap();
    }

    fn add_order(&self, order: OrderAny) {
        self.cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
    }

    fn add_position(&self, position: Position) {
        self.cache
            .borrow_mut()
            .add_position(position, OmsType::Hedging)
            .unwrap();
    }

    fn get_order(&self, client_order_id: &ClientOrderId) -> Option<OrderAny> {
        self.cache.borrow().order(client_order_id).cloned()
    }
}

fn test_instrument() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt())
}

fn test_instrument_id() -> InstrumentId {
    crypto_perpetual_ethusdt().id()
}

fn test_account_id() -> AccountId {
    AccountId::from("BINANCE-001")
}

fn test_venue() -> Venue {
    Venue::from("BINANCE")
}

fn test_client_id() -> ClientId {
    ClientId::from("BINANCE")
}

fn create_limit_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from(client_order_id))
        .instrument_id(instrument_id)
        .side(side)
        .quantity(Quantity::from(quantity))
        .price(Price::from(price))
        .build()
}

/// Creates an order that has been submitted (has account_id set)
fn create_submitted_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
) -> OrderAny {
    let mut order = create_limit_order(client_order_id, instrument_id, side, quantity, price);
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    order
}

fn create_order_status_report(
    client_order_id: Option<ClientOrderId>,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
    status: OrderStatus,
    quantity: Quantity,
    filled_qty: Quantity,
) -> OrderStatusReport {
    OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        client_order_id,
        venue_order_id,
        OrderSide::Buy,
        OrderType::Limit,
        TimeInForce::Gtc,
        status,
        quantity,
        filled_qty,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_price(Price::from("3000.00"))
}

#[rstest]
fn test_fill_deduplication_new_fill_not_processed() {
    let ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    assert!(!ctx.manager.is_fill_recently_processed(&trade_id));
}

#[rstest]
fn test_fill_deduplication_tracks_processed_fill() {
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    ctx.manager.mark_fill_processed(trade_id);

    assert!(ctx.manager.is_fill_recently_processed(&trade_id));
}

#[rstest]
fn test_fill_deduplication_prune_removes_expired() {
    let mut ctx = TestContext::new();
    let old_trade = TradeId::from("T-OLD");
    let new_trade = TradeId::from("T-NEW");

    ctx.manager.mark_fill_processed(old_trade);
    ctx.advance_time(120_000_000_000); // 120 seconds
    ctx.manager.mark_fill_processed(new_trade);

    ctx.manager.prune_recent_fills_cache(60.0); // 60 second TTL

    assert!(!ctx.manager.is_fill_recently_processed(&old_trade));
    assert!(ctx.manager.is_fill_recently_processed(&new_trade));
}

#[rstest]
fn test_reconcile_report_returns_empty_when_order_not_in_cache() {
    let mut ctx = TestContext::new();
    let client_order_id = ClientOrderId::from("O-MISSING");

    let report = ExecutionReport {
        client_order_id,
        venue_order_id: Some(VenueOrderId::from("V-001")),
        status: OrderStatus::Accepted,
        filled_qty: Quantity::from(0),
        avg_px: None,
        ts_event: UnixNanos::from(1_000_000),
    };

    let events = ctx.manager.reconcile_report(report).unwrap();

    assert!(events.is_empty());
}

#[rstest]
fn test_reconcile_report_handles_missing_venue_order_id() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    let report = ExecutionReport {
        client_order_id: ClientOrderId::from("O-001"),
        venue_order_id: None, // Missing venue order ID
        status: OrderStatus::Accepted,
        filled_qty: Quantity::from(0),
        avg_px: None,
        ts_event: UnixNanos::from(1_000_000),
    };

    let events = ctx.manager.reconcile_report(report).unwrap();

    // Should return empty since venue_order_id is required
    assert!(events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_with_empty_reports() {
    let mut ctx = TestContext::new();
    let mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_accepted() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None, // No client_order_id = external order
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    // Verify order was added to cache
    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id);
    assert!(order.is_some());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_canceled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-002"),
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));
}

#[tokio::test]
async fn test_external_order_canceled_with_partial_fill() {
    // Test that external orders with Canceled status and partial fills
    // have both the fill and canceled events generated (matching Python behavior)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-PARTIAL");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order was partially filled (0.5 of 1.0) then canceled
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.5"),
    )
    .with_avg_px(3000.00)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    // Add fill report for the partial fill
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3000.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        None, // No client_order_id for external order
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have: Accepted, Filled, Canceled (in ts_event order)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("0.5"));
        assert_eq!(filled.trade_id, TradeId::from("T-PARTIAL-001"));
    }

    // Verify order state in cache
    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from("0.5"));
}

#[tokio::test]
async fn test_cached_order_canceled_with_fills() {
    // Test that a cached order transitioning to Canceled has fills applied
    // BEFORE the Canceled event (matching Python behavior)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-CANCEL-FILL");
    let venue_order_id = VenueOrderId::from("V-CANCEL-FILL");

    ctx.add_instrument(test_instrument());

    // Create and cache an accepted order
    let mut order = create_submitted_order(
        "O-CANCEL-FILL",
        instrument_id,
        OrderSide::Buy,
        "2.0",
        "3000.00",
    );
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports order was partially filled then canceled
    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("2.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(3000.00)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    // Add fill report
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-CACHED-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have: Filled, Canceled (order already accepted)
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));

    // Verify order state
    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Canceled);
    assert_eq!(cached_order.filled_qty(), Quantity::from("1.0"));
}

#[tokio::test]
async fn test_triggered_event_generated_before_canceled() {
    // Test that Triggered event is generated before Canceled when ts_triggered is set
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-TRIG-CANCEL");
    let venue_order_id = VenueOrderId::from("V-TRIG-CANCEL");

    ctx.add_instrument(test_instrument());

    // Create and cache an accepted order
    let mut order = create_submitted_order(
        "O-TRIG-CANCEL",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports order was triggered then canceled
    let mut report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    report.ts_triggered = Some(UnixNanos::from(500_000));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have: Triggered, Canceled
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Triggered(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-003"),
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(3000.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("1.0"));
        assert!(filled.reconciliation);
    }
}

#[tokio::test]
async fn test_external_order_filled_uses_real_fills() {
    // Test that external orders with Filled status use real fill reports
    // instead of inferred fills, preserving trade-level details
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-FILLED");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order is fully filled (2.0 of 2.0)
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(3000.00)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    // Add two separate fill reports (multi-fill execution)
    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-FILL-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("2999.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-FILL-002"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3001.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill1, fill2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have: Accepted, Fill1, Fill2 (real fills, not inferred)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    // Verify we got the real trade IDs, not inferred
    if let OrderEventAny::Filled(filled1) = &result.events[1] {
        assert_eq!(filled1.trade_id, TradeId::from("T-FILL-001"));
        assert_eq!(filled1.last_qty, Quantity::from("1.0"));
        assert_eq!(filled1.last_px, Price::from("2999.00"));
    }
    if let OrderEventAny::Filled(filled2) = &result.events[2] {
        assert_eq!(filled2.trade_id, TradeId::from("T-FILL-002"));
        assert_eq!(filled2.last_qty, Quantity::from("1.0"));
        assert_eq!(filled2.last_px, Price::from("3001.00"));
    }

    // Verify order state in cache
    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("2.0"));
}

#[tokio::test]
async fn test_external_order_filled_with_partial_fills_generates_inferred() {
    // Test that external filled orders with incomplete fill reports
    // still get an inferred fill for the remaining quantity
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-PARTIAL-INFER");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order is fully filled (3.0 of 3.0) according to report
    // Use precision 3 to match instrument size precision
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("3.000"),
        Quantity::from("3.000"),
    )
    .with_avg_px(3000.00)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    // But we only have fill reports for 2.0 (missing 1.0)
    // Use precision 3 to match instrument size precision
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("2999.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have: Accepted, RealFill (2.0), InferredFill (1.0)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    // First fill is real
    if let OrderEventAny::Filled(filled1) = &result.events[1] {
        assert_eq!(filled1.trade_id, TradeId::from("T-PARTIAL-001"));
        assert_eq!(filled1.last_qty, Quantity::from("2.000"));
    }

    // Second fill is inferred (has UUID format trade_id, not the known fill report trade ID)
    if let OrderEventAny::Filled(filled2) = &result.events[2] {
        assert_ne!(
            filled2.trade_id.as_str(),
            "T-PARTIAL-001",
            "Expected inferred trade ID (UUID), got known fill report trade ID"
        );
        assert_eq!(filled2.trade_id.as_str().len(), 36);
        assert_eq!(filled2.last_qty, Quantity::from("1.000"));
    }

    // Verify order is fully filled
    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.filled_qty(), Quantity::from("3.000"));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_external_when_filtered() {
    let config = ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_uses_claimed_strategy() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("MY-STRATEGY");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);

    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.strategy_id(), strategy_id);
}

#[tokio::test]
async fn test_reconcile_mass_status_processes_fills_for_cached_order() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_fills() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add same fill twice
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill.clone(), fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Only one fill should be processed
    assert_eq!(result.events.len(), 1);
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_order_without_instrument() {
    let mut ctx = TestContext::new();
    // Don't add instrument to cache

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None,
        VenueOrderId::from("V-EXT-001"),
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_sorts_events_chronologically() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add fills in reverse chronological order
    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-002"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3001.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(2_000_000), // Later
        UnixNanos::from(2_000_000),
        None,
    );
    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3000.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000), // Earlier
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill2, fill1]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 2);

    // Verify chronological ordering
    assert!(result.events[0].ts_event() < result.events[1].ts_event());
}

#[rstest]
fn test_inflight_order_generates_rejection_after_max_retries() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());

    // Order must be submitted (have account_id) to generate rejection
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);
    ctx.advance_time(200_000_000); // 200ms, past threshold

    let events = ctx.manager.check_inflight_orders();

    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], OrderEventAny::Rejected(_)));

    if let OrderEventAny::Rejected(rejected) = &events[0] {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(rejected.reason.as_str(), "INFLIGHT_TIMEOUT");
    }
}

#[rstest]
fn test_inflight_check_skips_filtered_order_ids() {
    let filtered_id = ClientOrderId::from("O-FILTERED");
    let mut config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };
    config.filtered_client_order_ids.insert(filtered_id);
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    // Use submitted order (has account_id) to verify filtering, not missing account_id
    let order = create_submitted_order(
        "O-FILTERED",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(filtered_id);
    ctx.advance_time(200_000_000);

    let events = ctx.manager.check_inflight_orders();

    // Filtered order should not generate rejection
    assert!(events.is_empty());
}

#[rstest]
fn test_config_default_values() {
    let config = ExecutionManagerConfig::default();

    assert!(config.reconciliation);
    assert_eq!(config.reconciliation_startup_delay_secs, 10.0);
    assert_eq!(config.lookback_mins, Some(60));
    assert!(!config.filter_unclaimed_external);
    assert!(!config.filter_position_reports);
    assert!(config.generate_missing_orders);
    assert_eq!(config.inflight_check_interval_ms, 2_000);
    assert_eq!(config.inflight_threshold_ms, 5_000);
    assert_eq!(config.inflight_max_retries, 5);
}

#[rstest]
fn test_config_with_trader_id() {
    let trader_id = TraderId::from("TRADER-001");
    let config = ExecutionManagerConfig::default().with_trader_id(trader_id);

    assert_eq!(config.trader_id, trader_id);
}

#[rstest]
fn test_purge_operations_do_nothing_when_disabled() {
    let config = ExecutionManagerConfig {
        purge_closed_orders_buffer_mins: None,
        purge_closed_positions_buffer_mins: None,
        purge_account_events_lookback_mins: None,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);

    ctx.manager.purge_closed_orders();
    ctx.manager.purge_closed_positions();
    ctx.manager.purge_account_events();
}

#[tokio::test]
async fn test_reconcile_mass_status_accepted_order_canceled_at_venue() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());

    // Create and accept order locally
    let mut order =
        create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");

    // Apply accepted event to put order in ACCEPTED state
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports order was canceled
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    )
    .with_cancel_reason("USER_REQUEST".to_string());
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert!(canceled.reconciliation != 0); // Verify reconciliation flag is set
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_accepted_order_expired_at_venue() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-002");
    let venue_order_id = VenueOrderId::from("V-002");

    ctx.add_instrument(test_instrument());

    // Create and accept order locally
    let mut order =
        create_submitted_order("O-002", instrument_id, OrderSide::Sell, "2.0", "3100.00");

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports order expired
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Expired,
        Quantity::from("2.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Expired(_)));
}

#[rstest]
fn test_inflight_increments_retry_count_before_max() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id);

    // First check - past threshold, retry count becomes 1
    ctx.advance_time(200_000_000);
    let events1 = ctx.manager.check_inflight_orders();
    assert!(events1.is_empty()); // Not at max yet

    // Second check - retry count becomes 2
    ctx.advance_time(200_000_000);
    let events2 = ctx.manager.check_inflight_orders();
    assert!(events2.is_empty()); // Still not at max

    // Third check - retry count becomes 3, equals max, generates rejection
    ctx.advance_time(200_000_000);
    let events3 = ctx.manager.check_inflight_orders();
    assert_eq!(events3.len(), 1);
    assert!(matches!(events3[0], OrderEventAny::Rejected(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_external_order_partially_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        None, // External order
        VenueOrderId::from("V-EXT-PARTIAL"),
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(3000.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // External orders get: Accepted + Filled (for the partial fill)
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        assert!(filled.reconciliation);
    }

    // Verify order was created in cache (status is Initialized since events haven't been applied)
    let client_order_id = ClientOrderId::from("V-EXT-PARTIAL");
    let order = ctx.get_order(&client_order_id);
    assert!(order.is_some());
}

#[tokio::test]
async fn test_reconcile_mass_status_order_already_in_sync() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SYNC");
    let venue_order_id = VenueOrderId::from("V-SYNC");

    ctx.add_instrument(test_instrument());

    // Create accepted order locally
    let mut order =
        create_submitted_order("O-SYNC", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports exact same state
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[rstest]
fn test_clear_recon_tracking_removes_inflight() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let client_order_id = ClientOrderId::from("O-001");

    ctx.manager.register_inflight(client_order_id);

    // Simulate order being resolved externally (e.g., accepted by venue)
    ctx.manager.clear_recon_tracking(&client_order_id, true);

    // Advance time past threshold
    ctx.advance_time(200_000_000);

    // Check should not generate events since order was cleared
    let events = ctx.manager.check_inflight_orders();
    assert!(events.is_empty());
}

/// Creates an accepted order with venue_order_id set
fn create_accepted_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
    venue_order_id: VenueOrderId,
) -> OrderAny {
    let mut order = create_submitted_order(client_order_id, instrument_id, side, quantity, price);
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    order
}

#[tokio::test]
async fn test_inferred_fill_generated_when_venue_reports_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-FILL-001");
    let venue_order_id = VenueOrderId::from("V-FILL-001");

    ctx.add_instrument(test_instrument());

    // Create accepted order with no fills yet
    let order = create_accepted_order(
        "O-FILL-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    // Venue reports order as partially filled (no FillReport, just status)
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"), // 5 filled
    )
    .with_avg_px(3001.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should generate an inferred fill
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
        assert!(filled.reconciliation);
        assert_eq!(filled.trade_id.as_str().len(), 36);
    }
}

#[tokio::test]
async fn test_inferred_fill_uses_avg_px_for_first_fill() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-AVG-001");
    let venue_order_id = VenueOrderId::from("V-AVG-001");

    ctx.add_instrument(test_instrument());

    let order = create_accepted_order(
        "O-AVG-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(2999.75)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    if let OrderEventAny::Filled(filled) = &result.events[0] {
        // First fill should use avg_px directly
        assert_eq!(filled.last_px.as_f64(), 2999.75);
    }
}

#[tokio::test]
async fn test_no_inferred_fill_when_already_in_sync() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SYNC-001");
    let venue_order_id = VenueOrderId::from("V-SYNC-001");

    ctx.add_instrument(test_instrument());

    // Create an order that is already partially filled
    let mut order = create_accepted_order(
        "O-SYNC-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );

    // Apply a fill to the order
    let fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,                        // trade_id
        None,                        // position_id
        None,                        // last_px
        Some(Quantity::from("5.0")), // last_qty
        None,                        // liquidity_side
        None,                        // commission
        None,                        // ts_filled_ns
        None,                        // account_id
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports same fill state
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"), // Same as local
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_fill_qty_mismatch_venue_less_logs_error() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MISMATCH");
    let venue_order_id = VenueOrderId::from("V-MISMATCH");

    ctx.add_instrument(test_instrument());

    // Create an order that is already partially filled with 5
    let mut order = create_accepted_order(
        "O-MISMATCH",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,                        // trade_id
        None,                        // position_id
        None,                        // last_px
        Some(Quantity::from("5.0")), // last_qty
        None,                        // liquidity_side
        None,                        // commission
        None,                        // ts_filled_ns
        None,                        // account_id
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports LESS filled than we have (anomaly)
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"), // Less than our 5
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should not generate events (error condition)
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_market_order_inferred_fill_is_taker() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MKT-001");
    let venue_order_id = VenueOrderId::from("V-MKT-001");

    ctx.add_instrument(test_instrument());

    // Create a market order (submitted and accepted)
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .client_order_id(ClientOrderId::from("O-MKT-001"))
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy,
        OrderType::Market,
        TimeInForce::Ioc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(UUID4::new()),
    )
    .with_avg_px(3005.00)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
    }
}

#[tokio::test]
async fn test_pending_cancel_status_no_event() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PEND-001");
    let venue_order_id = VenueOrderId::from("V-PEND-001");

    ctx.add_instrument(test_instrument());

    let order = create_accepted_order(
        "O-PEND-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PendingCancel,
        Quantity::from("10.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Pending states don't generate events
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_incremental_fill_calculates_weighted_price() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-INCR-001");
    let venue_order_id = VenueOrderId::from("V-INCR-001");

    ctx.add_instrument(test_instrument());

    // Create an order that already has 5 filled at 3000.00
    let mut order = create_accepted_order(
        "O-INCR-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,                         // trade_id
        None,                         // position_id
        Some(Price::from("3000.00")), // last_px
        Some(Quantity::from("5.0")),  // last_qty
        None,                         // liquidity_side
        None,                         // commission
        None,                         // ts_filled_ns
        None,                         // account_id
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports 8 filled total at avg_px 3002.50
    // Original: 5 @ 3000.00 = 15000
    // New avg: 8 @ 3002.50 = 24020
    // Incremental: 3 @ (24020 - 15000) / 3 = 3006.67
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("8.0"),
    )
    .with_avg_px(3002.50)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        // (8 * 3002.50 - 5 * 3000.00) / 3 ≈ 3006.67
        let expected_px = (8.0 * 3002.50 - 5.0 * 3000.00) / 3.0;
        assert!((filled.last_px.as_f64() - expected_px).abs() < 0.01);
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_skips_exact_duplicate_orders() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    )
    .with_price(Price::from("100.0"));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(result.events.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_mass_status_deduplicates_within_batch() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report1 = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    let report2 = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report1, report2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
}

#[rstest]
#[tokio::test]
async fn test_mass_status_reconciles_when_status_differs() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));
}

#[rstest]
#[tokio::test]
async fn test_mass_status_reconciles_when_filled_qty_differs() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(100.0)
    .unwrap();
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event");
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_matches_order_by_venue_order_id() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));
    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_matches_order_by_venue_order_id_with_mismatched_client_id() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    // Report has wrong client_order_id but correct venue_order_id
    let wrong_client_order_id = ClientOrderId::from("O-WRONG");
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    let report = create_order_status_report(
        Some(wrong_client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));
    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_accepted_orders() {
    // Test that venue_order_id is properly indexed during reconciliation for orders
    // that are already in ACCEPTED state and don't generate new OrderAccepted events.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let client_order_id = ClientOrderId::from("O-TEST");
    let venue_order_id = VenueOrderId::from("V-123");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order.clone());

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(
        ctx.cache.borrow().client_order_id(&venue_order_id),
        Some(&client_order_id),
        "venue_order_id should be indexed after reconciliation"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_external_orders() {
    // Test that venue_order_id is properly indexed for external orders discovered
    // during reconciliation.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let venue_order_id = VenueOrderId::from("V-EXT-001");

    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(
        !result.events.is_empty(),
        "Should generate events for external order"
    );

    let cache_borrow = ctx.cache.borrow();
    let indexed_client_id = cache_borrow.client_order_id(&venue_order_id);
    assert!(
        indexed_client_id.is_some(),
        "venue_order_id should be indexed for external order"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_filled_orders() {
    // Test that venue_order_id is properly indexed for orders that are already
    // FILLED and don't generate new OrderAccepted events.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let client_order_id = ClientOrderId::from("O-FILLED");
    let venue_order_id = VenueOrderId::from("V-456");

    // Create order and process to FILLED state
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();

    let filled = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-1")),
        None,                          // position_id
        Some(Price::from("1.0")),      // last_px
        Some(Quantity::from("1.0")),   // last_qty
        Some(LiquiditySide::Taker),    // liquidity_side
        Some(Money::from("0.01 USD")), // commission
        None,                          // ts_filled_ns
        Some(test_account_id()),
    );
    order.apply(filled).unwrap();
    ctx.add_order(order.clone());

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    let cache_borrow = ctx.cache.borrow();
    assert_eq!(
        cache_borrow.client_order_id(&venue_order_id),
        Some(&client_order_id),
        "venue_order_id should be indexed for filled order"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_orders_without_loaded_instruments() {
    // Test that reconciliation properly skips orders for instruments that aren't loaded,
    // and these skipped orders don't cause validation warnings.
    let mut ctx = TestContext::new();
    let loaded_instrument_id = test_instrument_id();
    let loaded_instrument = test_instrument();
    ctx.add_instrument(loaded_instrument.clone());

    let unloaded_instrument_id = InstrumentId::from("BTCUSDT.SIM");

    let loaded_venue_order_id = VenueOrderId::from("V-LOADED");
    let unloaded_venue_order_id = VenueOrderId::from("V-UNLOADED");

    let loaded_report = create_order_status_report(
        Some(ClientOrderId::from("O-LOADED")),
        loaded_venue_order_id,
        loaded_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let unloaded_report = create_order_status_report(
        Some(ClientOrderId::from("O-UNLOADED")),
        unloaded_venue_order_id,
        unloaded_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );
    mass_status.add_order_reports(vec![loaded_report, unloaded_report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    let cache_borrow = ctx.cache.borrow();
    let loaded_client_id = cache_borrow.client_order_id(&loaded_venue_order_id);
    assert!(
        loaded_client_id.is_some(),
        "Loaded instrument order should be indexed"
    );

    let unloaded_client_id = cache_borrow.client_order_id(&unloaded_venue_order_id);
    assert!(
        unloaded_client_id.is_none(),
        "Unloaded instrument order should not be indexed (skipped during reconciliation)"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_position_from_position_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a position report with no corresponding order reports
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should generate Accepted + Filled events to create the position
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
        assert_eq!(filled.last_px.as_f64(), 3000.50);
        assert!(filled.reconciliation);
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_flat_position_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a flat position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events should be generated for flat position
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_position_report_when_filtered() {
    let config = ExecutionManagerConfig {
        filter_position_reports: true,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Position reports should be filtered
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_short_position_from_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a short position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(2950.25)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        assert_eq!(filled.order_side, OrderSide::Sell);
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_position_report_when_fills_exist() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report for 5.0 qty
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add a position report for the same instrument (would duplicate if not skipped)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should only have 1 fill event from the fill report, not additional events
    // from the position report (which would double-count)
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

/// Helper to create a position from a fill for testing
fn create_test_position(
    instrument: &InstrumentAny,
    position_id: PositionId,
    side: OrderSide,
    qty: &str,
    price: &str,
) -> Position {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(side)
        .quantity(Quantity::from(qty))
        .build();

    let fill = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::new("T-001")),
        Some(position_id),
        Some(Price::from(price)),
        Some(Quantity::from(qty)),
        None,
        None,
        None,
        Some(test_account_id()),
    );

    let order_filled: OrderFilled = fill.into();
    Position::new(instrument, order_filled)
}

#[tokio::test]
async fn test_reconcile_mass_status_iterates_all_position_reports() {
    // Tests that we iterate ALL position reports, not just the first one
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add two position reports for the same instrument (hedge mode scenario)
    let position_report_long = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-LONG-001")),
        Some(dec!(3000.50)),
    );

    let position_report_short = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-SHORT-001")),
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![position_report_long, position_report_short]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have events for both positions (2 initialized + 2 filled = 4 events minimum)
    // Both position reports should be processed, not just the first
    assert!(
        result.events.len() >= 4,
        "Expected at least 4 events for two positions, got {}",
        result.events.len()
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_routes_to_hedging_with_venue_position_id() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report WITH venue_position_id = hedge mode
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-HEDGE-001")),
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should create position since position doesn't exist in cache
    assert!(!result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_routes_to_netting_without_venue_position_id() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report WITHOUT venue_position_id = netting mode
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // No venue_position_id
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should create position since no position exists for instrument
    assert!(!result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_hedge_position_when_fills_in_batch() {
    // Tests that hedge position reconciliation is skipped when fills for the same
    // venue_position_id exist in the batch (prevents duplicate synthetic orders)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report WITH venue_position_id
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add a hedge position report for the same venue_position_id
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should only have fill event, no synthetic order from position report
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_hedge_position_when_filled_order_in_batch() {
    // Tests that hedge position reconciliation is skipped when a filled order report
    // with the same venue_position_id exists (even without explicit fill reports)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a filled order report WITH venue_position_id (no explicit fill report)
    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Buy,
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_avg_px(3000.0)
    .unwrap()
    .with_venue_position_id(venue_position_id);

    mass_status.add_order_reports(vec![order_report]);

    // Add a hedge position report for the same venue_position_id
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have events from the order report (initialized + filled), but no
    // additional synthetic order from position report
    let filled_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();
    assert_eq!(filled_count, 1, "Expected exactly 1 fill event");
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_hedge_position_when_fills_lack_position_id() {
    // Tests that hedge position reconciliation is skipped when fills exist for the
    // instrument but lack venue_position_id (common when venues only include IDs on
    // position reports)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report WITHOUT venue_position_id
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None, // No venue_position_id on fill
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add a hedge position report WITH venue_position_id
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-HEDGE-001")), // Has venue_position_id
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should only have fill event, position report skipped due to instrument-level fill
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_hedge_does_not_skip_unrelated_positions() {
    // Tests that when fills have venue_position_id, only that specific position is skipped,
    // not other hedge positions on the same instrument
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let position_id_1 = PositionId::from("P-HEDGE-001");
    let position_id_2 = PositionId::from("P-HEDGE-002");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report WITH venue_position_id for P-HEDGE-001
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(position_id_1), // Fill attributed to P-HEDGE-001
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add position reports for BOTH positions
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id_1),
        Some(dec!(3000.00)),
    );
    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id_2), // Different position
        Some(dec!(3100.00)),
    );
    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have:
    // - 1 fill event for P-HEDGE-001 (from fill report)
    // - Events for P-HEDGE-002 (from position report, should NOT be skipped)
    let filled_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();

    // At least 2 fills: one from fill report, one from position report for P-HEDGE-002
    assert!(
        filled_count >= 2,
        "Expected at least 2 fill events, got {filled_count}"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_matching_quantities() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position to cache with 5.0 qty
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report matches cached position exactly
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events needed since positions match
    assert!(
        result.events.is_empty(),
        "Expected no events when positions match, got {}",
        result.events.len()
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_discrepancy_generates_order() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position to cache with 5.0 qty
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows 8.0 qty (discrepancy of 3.0)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("8.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should generate reconciliation order to fix the discrepancy
    assert!(
        !result.events.is_empty(),
        "Expected events for position discrepancy reconciliation"
    );
}

#[tokio::test]
async fn test_reconcile_missing_hedge_position_generates_order() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report for position that doesn't exist in cache
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-MISSING-001")),
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should generate order to create the missing position
    assert!(
        !result.events.is_empty(),
        "Expected events for missing position creation"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_discrepancy_disabled() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false, // Disabled
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position with different qty than venue reports
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows 8.0 qty (discrepancy)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("8.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events since generate_missing_orders is disabled
    assert!(
        result.events.is_empty(),
        "Expected no events when generate_missing_orders is disabled"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_both_flat() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report with zero quantity (flat)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-FLAT-001")),
        None,
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // No events needed - position doesn't exist in cache and report is flat
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_hedge_short_position() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Short position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-SHORT-001")),
        Some(dec!(3100.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should create short position
    assert!(!result.events.is_empty());

    // Verify one of the events is a sell order (for short position)
    let has_sell = result.events.iter().any(|e| {
        if let OrderEventAny::Filled(filled) = e {
            filled.order_side == OrderSide::Sell
        } else {
            false
        }
    });
    assert!(has_sell, "Expected a sell order for short position");
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_netting_reports_same_instrument() {
    // Tests that multiple netting reports (no venue_position_id) for the same instrument
    // only create one position, not duplicates
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add two netting reports for the same instrument (both without venue_position_id)
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // No venue_position_id = netting mode
        Some(dec!(3000.50)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
        None, // No venue_position_id = netting mode
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should only create ONE position (2 events: initialized + filled), not two
    // If deduplication fails, we'd see 4 events (two positions)
    assert_eq!(
        result.events.len(),
        2,
        "Expected 2 events (1 position), got {} (duplicate netting reports should be skipped)",
        result.events.len()
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_hedge_reports_same_position_id() {
    // Tests that multiple hedge reports for the same venue_position_id only create
    // one position, not duplicates
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_position_id = PositionId::from("P-HEDGE-001");
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add two hedge reports for the same venue_position_id (duplicate snapshots)
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.50)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
        Some(venue_position_id), // Same venue_position_id
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should only create ONE position (2 events: initialized + filled), not two
    assert_eq!(
        result.events.len(),
        2,
        "Expected 2 events (1 position), got {} (duplicate hedge reports should be skipped)",
        result.events.len()
    );
}

#[tokio::test]
async fn test_adjust_fills_creates_synthetic_for_partial_window() {
    // Test that adjust_fills_for_partial_window creates synthetic fills when
    // historical fills don't fully explain the current position (partial window scenario).
    // This happens when lookback window started mid-position.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows LONG 5.0 with avg_px 3000.00
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(3000.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    // Only have fills for 2.0 (partial window - missing opening fills)
    let venue_order_id = VenueOrderId::from("V-PARTIAL-001");
    let order_report = create_order_status_report(
        Some(ClientOrderId::from("O-PARTIAL-001")),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(3100.00)
    .unwrap();
    mass_status.add_order_reports(vec![order_report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("3100.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(500_000),
        UnixNanos::from(500_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // The adjustment should create a synthetic fill for the missing 3.0
    // (position=5.0, fills=2.0, so synthetic opening of 3.0 is needed)
    // Events: Synthetic order (Accepted + Filled) + Original order (Accepted + Filled) + Position events
    // The exact number depends on implementation but we should have more than just the original fill

    // Verify we have fills that sum to match the position
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    // Should have at least 2 fills (synthetic opening + original)
    assert!(
        fill_events.len() >= 2,
        "Expected at least 2 fills (synthetic + original), got {}",
        fill_events.len()
    );

    // Total filled quantity should match position quantity (5.0)
    let total_qty: f64 = fill_events.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (total_qty - 5.0).abs() < 0.001,
        "Total filled qty should be ~5.0 to match position, got {total_qty}"
    );
}

#[tokio::test]
async fn test_external_order_has_venue_tag() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-EXT-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // External order with no client_order_id
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(!result.events.is_empty());

    // Get the order created and verify it has the VENUE tag
    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    let tags = order.tags().expect("Order should have tags");
    assert!(
        tags.contains(&ustr::Ustr::from("VENUE")),
        "External order should have VENUE tag"
    );
}

#[tokio::test]
async fn test_position_reconciliation_order_has_reconciliation_tag() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report with no corresponding orders - this triggers position reconciliation
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    assert!(!result.events.is_empty());

    // Get the synthetic order created and verify it has the RECONCILIATION tag
    if let OrderEventAny::Accepted(accepted) = &result.events[0] {
        let order = ctx
            .get_order(&accepted.client_order_id)
            .expect("Order should exist");
        let tags = order.tags().expect("Order should have tags");
        assert!(
            tags.contains(&ustr::Ustr::from("RECONCILIATION")),
            "Position reconciliation order should have RECONCILIATION tag, got {tags:?}",
        );
    } else {
        panic!("Expected Accepted event, got {:?}", result.events[0]);
    }
}

#[tokio::test]
async fn test_closed_reconciliation_orders_skipped_on_restart() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-RECON-001");
    let venue_order_id = VenueOrderId::from("V-RECON-001");

    // Create a closed reconciliation order from a previous session
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("5.0"))
        .client_order_id(client_order_id)
        .tags(vec![ustr::Ustr::from("RECONCILIATION")])
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    let filled = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    order.apply(filled).unwrap();

    assert!(order.is_closed());
    ctx.add_order(order);

    // Simulate restart with a mass status that contains this order
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should skip the closed reconciliation order - no new events generated
    assert!(
        result.events.is_empty(),
        "Should skip closed RECONCILIATION order, but got {} events",
        result.events.len()
    );
}

#[tokio::test]
async fn test_netting_position_cross_zero_long_to_short() {
    // Test: Cached position is long +5.0, venue reports short -3.0
    // Should generate: close fill (sell 5.0) + open fill (sell 3.0) = 2 fills
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached LONG position of 5.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports SHORT position of -3.0
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(3100.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have 2 fills: close (sell 5.0) + open (sell 3.0)
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        2,
        "Cross-zero should generate 2 fills (close + open), got {}",
        fill_events.len()
    );

    // First fill should be SELL 5.0 (close long)
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));

    // Second fill should be SELL 3.0 (open short)
    assert_eq!(fill_events[1].order_side, OrderSide::Sell);
    assert_eq!(fill_events[1].last_qty, Quantity::from("3.0"));
}

#[tokio::test]
async fn test_netting_position_cross_zero_short_to_long() {
    // Test: Cached position is short -4.0, venue reports long +2.0
    // Should generate: close fill (buy 4.0) + open fill (buy 2.0) = 2 fills
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached SHORT position of 4.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Sell,
        "4.0",
        "3000.00",
    );
    ctx.add_position(position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports LONG position of +2.0
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("2.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(2900.00)),
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have 2 fills: close (buy 4.0) + open (buy 2.0)
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        2,
        "Cross-zero should generate 2 fills (close + open), got {}",
        fill_events.len()
    );

    // First fill should be BUY 4.0 (close short)
    assert_eq!(fill_events[0].order_side, OrderSide::Buy);
    assert_eq!(fill_events[0].last_qty, Quantity::from("4.0"));

    // Second fill should be BUY 2.0 (open long)
    assert_eq!(fill_events[1].order_side, OrderSide::Buy);
    assert_eq!(fill_events[1].last_qty, Quantity::from("2.0"));
}

#[tokio::test]
async fn test_netting_position_flat_report_closes_cached_position() {
    // Test: Cached position is long +5.0, venue reports flat (0.0)
    // Should generate: close fill (sell 5.0)
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached LONG position of 5.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports FLAT position (0.0)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Flat,
        Quantity::from("0.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        None, // No avg_px for flat
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have 1 fill: close (sell 5.0)
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        1,
        "Flat report should generate 1 closing fill, got {}",
        fill_events.len()
    );

    // Fill should be SELL 5.0 (close long position)
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));
}

#[tokio::test]
async fn test_expired_order_applies_fills_before_terminal_event() {
    // Expired orders should apply fills before the expired event (same as canceled)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let client_order_id = ClientOrderId::from("O-EXPIRE-TEST");
    let venue_order_id = VenueOrderId::from("V-EXPIRE-001");

    // Create and submit an order
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order.clone());
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    // Report shows order EXPIRED with partial fills
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_status_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Expired,
        Quantity::from("10.0"),
        Quantity::from("3.0"), // Partially filled
    );
    mass_status.add_order_reports(vec![report]);

    // Add fill report for the partial fill
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("3.0"),
        Price::from("100.0"),
        Money::from("0.10 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // Should have Fill event BEFORE Expired event
    let fill_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();
    let expired_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Expired(_)))
        .count();

    assert_eq!(fill_count, 1, "Should have 1 fill event");
    assert_eq!(expired_count, 1, "Should have 1 expired event");

    // Verify fill comes before expired in the event list
    let fill_idx = result
        .events
        .iter()
        .position(|e| matches!(e, OrderEventAny::Filled(_)))
        .unwrap();
    let expired_idx = result
        .events
        .iter()
        .position(|e| matches!(e, OrderEventAny::Expired(_)))
        .unwrap();

    assert!(
        fill_idx < expired_idx,
        "Fill event should come before Expired event"
    );
}

#[tokio::test]
async fn test_partial_window_adjustment_skips_hedge_mode_instruments() {
    // Partial-window fill adjustment should skip hedge mode instruments
    // (those with venue_position_id set) to avoid corrupting fills
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let venue_order_id = VenueOrderId::from("V-HEDGE-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a filled order report with fills
    let report = create_order_status_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-HEDGE-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("100.0"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );
    mass_status.add_fill_reports(vec![fill]);

    // Add hedge mode position report (has venue_position_id)
    let hedge_position_id = PositionId::new("HEDGE-POS-001");
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSideSpecified::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,                    // report_id
        Some(hedge_position_id), // Hedge mode - has venue_position_id
        Some(dec!(100.0)),       // avg_px_open
    );
    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, ctx.exec_engine.clone())
        .await;

    // The fill should be preserved (not modified by partial-window adjustment)
    // and external order should be created
    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert!(
        !fill_events.is_empty(),
        "Fill events should be preserved for hedge mode instruments"
    );

    // Verify the fill quantity matches original (wasn't modified)
    assert_eq!(
        fill_events[0].last_qty,
        Quantity::from("5.0"),
        "Fill quantity should match original fill report"
    );
}
