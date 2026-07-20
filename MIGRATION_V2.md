# Migrate from v1 to v2

NautilusTrader v2 is the Rust core and PyO3 Python package under `python/`. It becomes the primary
Python path when `develop` switches to that package. Until that cutover, the main distribution and
general documentation still describe the legacy v1 Cython package. Use this guide to test or prepare
a migration without mixing the two environments.

After cutover, v1 moves to the `develop_v1` branch for approximately three months of critical
security backports. It does not receive new feature or parity work.

The v1 and v2 packages both install and import as `nautilus_trader`. Use separate virtual
environments while migrating. Do not install both versions into one environment.

## Install v2

Install a release candidate from PyPI in a fresh environment:

```bash
uv venv --python 3.14
source .venv/bin/activate
uv pip install --pre nautilus_trader
```

Run this block outside a NautilusTrader source checkout. The repository's `exclude-newer` uv policy
can filter out newly published release-candidate wheels. Inside a source checkout, use the source
build below.

Inside a source checkout, build the package from `python/` into its dedicated `python/.venv`:

```bash
make build-debug-v2
cd python
.venv/bin/python -c 'import nautilus_trader; print(nautilus_trader.__version__)'
```

The root `.venv` and root `pyproject.toml` belong to the legacy v1 build during the transition.
See [Installation](docs/getting_started/installation.md) for platform support and package-index options.

## Port Python code

The strategy lifecycle and common data, order, risk, portfolio, backtest, and live workflows remain
available. Update imports and configuration to the new module paths:

| v1 path                                                        | v2 path                                                   |
|----------------------------------------------------------------|-----------------------------------------------------------|
| `nautilus_trader.backtest.engine.BacktestEngine`               | `nautilus_trader.backtest.BacktestEngine`                 |
| `nautilus_trader.backtest.node.BacktestNode`                   | `nautilus_trader.backtest.BacktestNode`                   |
| `nautilus_trader.live.node.TradingNode`                        | `nautilus_trader.live.LiveNode`                           |
| `nautilus_trader.config.StrategyConfig`                        | `nautilus_trader.trading.StrategyConfig`                  |
| Adapter classes from `nautilus_trader.adapters.<venue>.config` | Rust/PyO3 classes from `nautilus_trader.adapters.<venue>` |

### Common API renames

V2 shortens several high-frequency strategy and cache names. The `QuoteTick` and `TradeTick` model
type names do not change, and the `register_indicator_for_*_ticks` names remain unchanged.

| v1 name                              | v2 name                        |
|--------------------------------------|--------------------------------|
| `on_quote_tick`                      | `on_quote`                     |
| `on_trade_tick`                      | `on_trade`                     |
| `on_order_book`                      | `on_book`                      |
| `on_order_book_deltas`               | `on_book_deltas`               |
| `on_order_book_depth`                | `on_book_depth`                |
| `subscribe_quote_ticks`              | `subscribe_quotes`             |
| `subscribe_trade_ticks`              | `subscribe_trades`             |
| `unsubscribe_quote_ticks`            | `unsubscribe_quotes`           |
| `unsubscribe_trade_ticks`            | `unsubscribe_trades`           |
| `request_quote_ticks`                | `request_quotes`               |
| `request_trade_ticks`                | `request_trades`               |
| `subscribe_order_book_deltas`        | `subscribe_book_deltas`        |
| `subscribe_order_book_depth`         | `subscribe_book_depth10`       |
| `subscribe_order_book_at_interval`   | `subscribe_book_at_interval`   |
| `unsubscribe_order_book_deltas`      | `unsubscribe_book_deltas`      |
| `unsubscribe_order_book_depth`       | `unsubscribe_book_depth10`     |
| `unsubscribe_order_book_at_interval` | `unsubscribe_book_at_interval` |
| `request_order_book_snapshot`        | `request_book_snapshot`        |
| `request_order_book_deltas`          | `request_book_deltas`          |
| `request_order_book_depth`           | `request_book_depth`           |
| `cache.quote_tick`                   | `cache.quote`                  |
| `cache.trade_tick`                   | `cache.trade`                  |
| `cache.quote_ticks`                  | `cache.quotes`                 |
| `cache.trade_ticks`                  | `cache.trades`                 |
| `cache.quote_tick_count`             | `cache.quote_count`            |
| `cache.trade_tick_count`             | `cache.trade_count`            |

### Inspection and state renames

V2 exposes one read-only inspection contract across its economic instrument types. Fields such as
`asset_class`, `instrument_class`, currencies, fees, margins, quantity and price limits,
`multiplier`, and `tick_scheme` can be read consistently even when their value is `None` or a
documented default. `SyntheticInstrument` is formula-derived and does not carry that economic
state; inspect its `id`, `components`, `formula`, price precision and increment, and timestamps.

Several v1 inspection names have direct v2 replacements:

| v1 name                                              | v2 name                               |
|------------------------------------------------------|---------------------------------------|
| `instrument.symbol`                                  | `instrument.id.symbol`                |
| `instrument.venue`                                   | `instrument.id.venue`                 |
| `instrument.activation_utc`                          | `instrument.activation_ns`            |
| `instrument.expiration_utc`                          | `instrument.expiration_ns`            |
| `instrument.tick_scheme_name`                        | `instrument.tick_scheme`              |
| `AdaptiveMovingAverage.period` or `.period_er`       | `.period_efficiency_ratio`            |
| `AdaptiveMovingAverage.period_alpha_fast`            | `.period_fast`                        |
| `AdaptiveMovingAverage.period_alpha_slow`            | `.period_slow`                        |
| `LinearRegression.R2`                                | `LinearRegression.r2`                 |
| `DirectionalMovement.value`                          | `.pos` and `.neg`                     |
| `CustomData.data`                                    | `CustomData.value`                    |
| `DataType.type`                                      | `DataType.type_name`                  |
| `OrderBookDelta.is_add/is_clear/is_delete/is_update` | inspect `OrderBookDelta.action`       |
| `OrderBookDeltas.is_snapshot`                        | inspect `OrderBookDeltas.flags`       |
| `BookLevel.side`                                     | use the containing bid or ask context |
| `Bar.is_revision`                                    | removed                               |

`activation_ns` and `expiration_ns` contain UNIX nanoseconds; convert them to the datetime type
used by the application when calendar-time inspection is needed. V1 `DirectionalMovement.value`
never changed from zero, so v2 exposes the meaningful positive and negative outputs instead.

### Config readback and sensitive values

V2 immutable configs expose their non-secret constructor values as read-only properties. This
includes nested engine configs, backtest venue and run settings, live reconciliation settings, and
the data and execution tester configs. `LiveRiskEngineConfig.max_notional_per_order` returns the
validated string values stored by v2, even when the constructor received Python integers or decimal
values.

Potential credentials and consumed callbacks use bounded inspection properties instead of raw
readback:

| Constructor field                                    | Inspection property                         |
|------------------------------------------------------|---------------------------------------------|
| `BacktestDataConfig.catalog_fs_storage_options`      | `catalog_fs_storage_option_keys`            |
| `BacktestDataConfig.catalog_fs_rust_storage_options` | `catalog_fs_rust_storage_option_keys`       |
| `SocketConfig.handler`                               | `has_handler`                               |
| `WebSocketConfig.headers`                            | `header_names`                              |
| `WebSocketConfig.proxy_url`                          | `has_proxy_url`                             |

The raw fields in this table are intentionally not properties. Keep the original secret or callback
in application-owned state if it must be reused.

In general, v1 types from `nautilus_trader.config` move beside the runtime that owns them. For
example, v2 exposes `BacktestRunConfig` from `nautilus_trader.backtest` and `PortfolioConfig` from
`nautilus_trader.portfolio`.

Use the generated type stubs in `python/nautilus_trader/` as the exact Python contract. The
[Python v2 examples][python-v2-examples] show current live-node builders, adapter factories,
strategies, actors, and data/execution testers.

Python v2 strategies still subclass `Strategy` and override lifecycle or data callbacks:

```python
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class MyStrategyConfig(StrategyConfig):
    pass


class MyStrategy(Strategy):
    def on_start(self) -> None:
        pass
```

v1-style annotated custom fields on a `StrategyConfig` subclass do not carry over. When a v2
subclass adds custom fields, remove their keyword arguments in `__new__` before the PyO3 base
validates them, then assign the fields in `__init__`. See the
[v2 strategy config example][python-v2-strategy-config].

Port one workflow at a time and verify the generated stub before replacing a v1 convenience method.
Do not assume that a v1 adapter config field also exists on its v2 Rust config.

## Accepted contract differences

The cutover accepts these differences from v1:

- Custom data flows as native `CustomData` without the v1 wrapper semantics.
- v2 caches `OptionGreeks` for option fee calculation; this is an extension rather than v1 parity.
- `Bar.is_revision` is not exposed on the v2 Python surface. Do not depend on it during migration.
- A direct `Position.apply` fill that crosses zero resets the open entry price to the flipping fill.
  v1 retains the old side's entry price; the v2 behavior is the go-forward contract.
- `PortfolioConfig.use_mark_prices` defaults to `true`; v1 defaulted to `false`. Set it to `false` to
  skip mark prices.
- v2 `OrderList` stores client order IDs instead of order objects. Replace `order_list.orders` with
  `order_list.client_order_ids()` and resolve each ID through `cache.order(client_order_id)`. Replace
  `order_list.first` with `cache.order(order_list.first_client_order_id)` after checking the ID is not
  `None`.
- Catalog order-event data written before `activation_price` and `OrderFilled.info` were added cannot
  be read by the new schema. Regenerate or migrate that data before upgrading a catalog in place.

## Deferred limits

These gaps do not block the supported cutover workflows, but they can affect a migration:

- Python request callback, join, and pending-request convenience semantics are not complete.
- Direct Python injection of Redis cache databases and external message-bus backing factories into
  `LiveNode` is not exposed. The backing implementations remain available to Rust builders.
- SQL cache position and synthetic loads, actor and strategy state persistence, and heartbeat remain
  incomplete. The audited restart workflow uses the Redis backing through Rust builders; Python
  `LiveNode` configuration cannot select that backing yet.
- Serialized order and position snapshot publishing to external message-bus topics remains deferred.
- The v2 `BacktestNode` does not yet wire the v1 `StreamingConfig` and `DataCatalogConfig` iterator
  workflow.
- Instrument-provider filter dictionaries are not a common v2 adapter contract. Hyperliquid v2
  loads its configured instrument universe and does not accept the v1 `instrument_provider` field.
  Check each adapter's Rust/PyO3 config rather than copying v1 provider examples.
- The published quickstart and backtesting tutorials still use v1 imports and configuration. Use the
  generated v2 stubs, the [v2 backtest acceptance tests][python-v2-backtest-tests] for backtesting,
  and [Python v2 examples][python-v2-examples] for live and adapter workflows while those tutorials
  are ported.

The [v2 roadmap][v2-roadmap] tracks the wider post-cutover surface. Release-specific breaking
changes remain in [RELEASES.md][release-notes].

[python-v2-backtest-tests]: python/tests/acceptance/test_backtest.py
[python-v2-examples]: python/examples/
[python-v2-strategy-config]: python/tests/strategies/ema_cross.py
[release-notes]: RELEASES.md
[v2-roadmap]: https://github.com/nautechsystems/nautilus_trader/issues/4042
