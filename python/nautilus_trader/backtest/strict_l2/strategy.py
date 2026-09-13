# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Execution-bound strategy for audited strict-L2 candidate replay.
"""

from __future__ import annotations

import math
from decimal import Decimal
from functools import wraps
from time import perf_counter_ns
from typing import TYPE_CHECKING
from typing import Any

from nautilus_trader.backtest.strict_l2.candidate import CandidateSignal
from nautilus_trader.backtest.strict_l2.candidate import load_candidate_signals
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LimitOrder
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TimeInForce
from nautilus_trader.trading import Strategy


if TYPE_CHECKING:
    from collections.abc import Callable

    from nautilus_trader.common import TimeEvent


MIN_DIRECTION_PROBABILITY = 0.5
_MAX_PERFORMANCE_SAMPLES = 100_000
_SUBMIT_ENTRY_ARGUMENT_COUNT = 3


class CandidateReplayConfig(StrategyConfig):
    """
    Configuration for one-symbol candidate replay.
    """

    def __init__(  # noqa: PLR0913
        self,
        *,
        instrument_id: str,
        research_symbol: str,
        audit_receipt_path: str,
        horizon_ms: int = 1_000,
        trade_size: str = "1",
        min_abs_delta_ticks: float = 0.5,
        min_direction_probability: float = 0.5,
        cooldown_ms: int = 1_000,
        max_signal_lag_ms: int = 100,
        replay_start_ns: int | None = None,
        replay_end_ns: int | None = None,
        order_insert_latency_ns: int = 0,
        record_performance: bool = False,
        **_kwargs: object,
    ) -> None:
        """
        Initialize the candidate replay configuration.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.research_symbol = research_symbol
        self.audit_receipt_path = audit_receipt_path
        self.horizon_ms = horizon_ms
        self.trade_size = trade_size
        self.min_abs_delta_ticks = min_abs_delta_ticks
        self.min_direction_probability = min_direction_probability
        self.cooldown_ms = cooldown_ms
        self.max_signal_lag_ms = max_signal_lag_ms
        self.replay_start_ns = replay_start_ns
        self.replay_end_ns = replay_end_ns
        self.order_insert_latency_ns = order_insert_latency_ns
        self.record_performance = record_performance


def _validate_trade_size(value: str) -> None:
    try:
        trade_size = Decimal(value)
    except ArithmeticError as e:
        raise ValueError("trade_size must be a finite positive decimal string") from e
    if not trade_size.is_finite() or trade_size <= 0:
        raise ValueError("trade_size must be a finite positive decimal string")


def _validate_strategy_config(config: CandidateReplayConfig) -> None:
    declared_types = (
        (config.trade_size, (str,)),
        (config.horizon_ms, (int,)),
        (config.min_abs_delta_ticks, (float,)),
        (config.min_direction_probability, (float,)),
        (config.cooldown_ms, (int,)),
        (config.max_signal_lag_ms, (int,)),
        (config.replay_start_ns, (int, type(None))),
        (config.replay_end_ns, (int, type(None))),
        (config.order_insert_latency_ns, (int,)),
    )

    if any(type(value) not in allowed for value, allowed in declared_types):
        raise ValueError("replay numeric settings must use their declared types")
    if not config.research_symbol:
        raise ValueError("research_symbol must not be empty")
    _validate_trade_size(config.trade_size)
    if not math.isfinite(config.min_abs_delta_ticks) or config.min_abs_delta_ticks < 0:
        raise ValueError("min_abs_delta_ticks must be finite and non-negative")
    if (
        not math.isfinite(config.min_direction_probability)
        or not MIN_DIRECTION_PROBABILITY <= config.min_direction_probability <= 1
    ):
        raise ValueError("min_direction_probability must be finite and in [0.5, 1]")
    if config.cooldown_ms < 0 or config.max_signal_lag_ms < 0:
        raise ValueError("replay timing limits must be non-negative")
    if (
        config.replay_start_ns is not None
        and config.replay_end_ns is not None
        and config.replay_start_ns >= config.replay_end_ns
    ):
        raise ValueError("replay time bounds must define a positive interval")
    if config.order_insert_latency_ns < 0:
        raise ValueError("order_insert_latency_ns must be non-negative")
    if type(config.record_performance) is not bool:
        raise ValueError("record_performance must be boolean")


class CandidateReplayStrategy(Strategy):
    """
    Replay audited signals with an exit deadline measured from entry submission.

    A completed entry fill past that deadline reevaluates the due exit immediately.
    Submission does not guarantee a fill: insertion latency and subsequent book
    updates can cause a marketable FOK to cancel. Unfilled exits retry on book updates.
    Entries whose horizon plus twice the insertion latency reaches the replay end
    are skipped; any position still open at the end prevents feedback publication.

    """

    def __init__(self, config: CandidateReplayConfig) -> None:
        """
        Initialize causal signal and round-trip execution state.
        """
        _validate_strategy_config(config)
        super().__init__(config)
        self._instrument_id = InstrumentId.from_str(config.instrument_id)
        self._research_symbol = config.research_symbol
        self._audit_receipt_path = config.audit_receipt_path
        self._horizon_ms = config.horizon_ms
        self._trade_size = Decimal(config.trade_size)
        self._min_abs_delta_ticks = float(config.min_abs_delta_ticks)
        self._min_direction_probability = float(config.min_direction_probability)
        self._cooldown_ns = config.cooldown_ms * 1_000_000
        self._max_signal_lag_ns = config.max_signal_lag_ms * 1_000_000
        self._replay_start_ns = config.replay_start_ns
        self._replay_end_ns = config.replay_end_ns
        self._order_insert_latency_ns = config.order_insert_latency_ns
        self._instrument = None
        self._signals: tuple[CandidateSignal, ...] = ()
        self._cursor = 0
        self._active_signal: CandidateSignal | None = None
        self._entry_order_id = None
        self._exit_order_id = None
        self._entry_side: OrderSide | None = None
        self._entry_filled = Decimal(0)
        self._exit_filled = Decimal(0)
        self._exit_due_ns: int | None = None
        self._last_entry_ns: int | None = None
        self._bindings: dict[str, dict[str, Any]] = {}
        self._failures: list[str] = []
        timer_suffix = str(self._instrument_id).replace(".", "-")
        self._signal_timer_name = f"strict-l2-signal-{timer_suffix}"
        self._exit_timer_name = f"strict-l2-exit-{timer_suffix}"
        self._performance_samples: dict[str, list[int]] = {}
        self._performance_counts: dict[str, int] = {}
        self._performance_times: dict[str, list[int | None]] = {}
        self._record_performance = config.record_performance
        if self._record_performance:
            for name in (
                "_process_due_signal",
                "_submit_entry",
                "_submit_exit",
                "on_order_filled",
                "on_book_deltas",
            ):
                setattr(self, name, self._timed_callback(name, getattr(self, name)))

    def _timed_callback[**P, R](self, name: str, callback: Callable[P, R]) -> Callable[P, R]:
        @wraps(callback)
        def measured(*args: P.args, **kwargs: P.kwargs) -> R:
            started = perf_counter_ns()
            try:
                return callback(*args, **kwargs)
            finally:
                elapsed = perf_counter_ns() - started
                self._performance_counts[name] = self._performance_counts.get(name, 0) + 1
                samples = self._performance_samples.setdefault(name, [])
                if len(samples) < _MAX_PERFORMANCE_SAMPLES:
                    samples.append(elapsed)
                    market_time = None

                    if (
                        name in {"on_order_filled", "on_book_deltas"}
                        and args
                        and isinstance(args[0], (OrderFilled, OrderBookDeltas))
                    ):
                        market_time = args[0].ts_event
                    elif name == "_process_due_signal" and args and isinstance(args[0], int):
                        market_time = args[0]
                    elif (
                        name == "_submit_entry"
                        and len(args) == _SUBMIT_ENTRY_ARGUMENT_COUNT
                        and isinstance(args[2], int)
                    ):
                        market_time = args[2]
                    self._performance_times.setdefault(name, []).append(market_time)

        return measured

    @property
    def performance_report(self) -> dict[str, Any] | None:
        """
        Return inclusive Python callback durations measured with ``perf_counter_ns``.

        Profiling is disabled by default. Each stage retains its first 100,000 samples
        per symbol in memory, including warmup callbacks, so samples may favor premarket
        activity. Nested stages overlap; their times are not additive. These diagnostics
        exclude managed book updates, model inference, and broker network latency and do
        not establish full-day performance acceptance.

        """
        if not self._record_performance:
            return None
        return {
            "schema_version": "strict-l2-host-performance/v1",
            "scope": "inclusive Python callback host duration; stages overlap",
            "excludes": ["managed book update", "model inference", "broker network latency"],
            "unit": "nanoseconds",
            "max_samples_per_stage": _MAX_PERFORMANCE_SAMPLES,
            "sampling": "first samples per stage; counts include later callbacks",
            "stages": {
                name: {
                    "count": count,
                    "samples": list(self._performance_samples[name]),
                    "market_time_ns": list(self._performance_times[name]),
                    "truncated": count > len(self._performance_samples[name]),
                }
                for name, count in self._performance_counts.items()
            },
        }

    @property
    def feedback_bindings(self) -> dict[str, dict[str, Any]]:
        """
        Return detached execution bindings for identifier-free feedback export.
        """
        return {key: dict(value) for key, value in self._bindings.items()}

    @property
    def failures(self) -> tuple[str, ...]:
        """
        Return terminal execution failures observed during replay.
        """
        return tuple(self._failures)

    @property
    def consumed_signals(self) -> int:
        """
        Return the number of signals released by the replay clock.
        """
        return self._cursor

    def on_start(self) -> None:
        """
        Load audited development signals and subscribe to the managed L2 MBP book.
        """
        self._instrument = self.cache.instrument(self._instrument_id)
        if self._instrument is None:
            self._fail("instrument is absent from the replay catalog")
            return
        quantity = Quantity.from_decimal_dp(self._trade_size, self._instrument.size_precision)
        if quantity.as_decimal() != self._trade_size:
            self._fail("trade_size is not exactly representable at the instrument size precision")
            return
        self._signals = load_candidate_signals(
            self._audit_receipt_path,
            horizon_ms=self._horizon_ms,
            instruments={self._research_symbol},
            start_ns=self._replay_start_ns,
            end_ns=self._replay_end_ns,
        )
        self.subscribe_book_deltas(self._instrument_id, BookType.L2_MBP, managed=True)
        self._schedule_next_signal()

    def on_book_deltas(self, _deltas: OrderBookDeltas) -> None:
        """
        Retry due exits when the managed book updates.
        """
        now_ns = int(self.clock.timestamp_ns())
        if self._exit_due_ns is not None and now_ns >= self._exit_due_ns:
            self._submit_exit()

    def on_time_event(self, event: TimeEvent) -> None:
        """
        Release model decisions and exits on their exact observable clock.
        """
        now_ns = int(event.ts_event)
        if event.name == self._signal_timer_name:
            self._process_due_signal(now_ns)
            self._schedule_next_signal()
        elif event.name == self._exit_timer_name:
            self._submit_exit()

    def _process_due_signal(self, now_ns: int) -> None:
        """
        Evaluate the newest prediction observable at ``now_ns``.
        """
        latest = self._latest_due_signal(now_ns)
        book = self.cache.order_book(self._instrument_id)

        if (
            latest is None
            or book is None
            or (bid := book.best_bid_price()) is None
            or (ask := book.best_ask_price()) is None
            or ask < bid
        ):
            return
        if self._active_signal is not None:
            return
        if self._last_entry_ns is not None and now_ns - self._last_entry_ns < self._cooldown_ns:
            return
        if now_ns - latest.ts_recv_ns > self._max_signal_lag_ns:
            return
        if not self.portfolio.is_net_flat(self._instrument_id):
            self._fail("strategy was unexpectedly non-flat before a new prediction")
            return
        side = self._signal_side(latest)
        if side is None:
            return
        self._submit_entry(latest, side, now_ns)

    def _schedule_next_signal(self) -> None:
        """
        Schedule one alert, keeping timer count bounded for long sessions.
        """
        if self._cursor >= len(self._signals):
            return
        self.clock.set_time_alert_ns(
            self._signal_timer_name,
            self._signals[self._cursor].ts_recv_ns,
            allow_past=False,
        )

    def on_order_filled(self, event: OrderFilled) -> None:
        """
        Track partial fills and make fees available to daily feedback.
        """
        client_order_id = str(event.client_order_id)
        binding = self._bindings.get(client_order_id)
        if binding is None:
            self._fail("received a fill without a prediction binding")
            return
        if event.commission is not None:
            binding["fees"] += event.commission.as_decimal()
        binding["last_fill_ts_ns"] = max(
            binding["last_fill_ts_ns"] or 0,
            int(event.ts_event),
        )
        filled = event.last_qty.as_decimal()
        if event.client_order_id == self._entry_order_id:
            self._entry_filled += filled
            if (
                self._entry_filled >= self._trade_size
                and self._exit_due_ns is not None
                and int(self.clock.timestamp_ns()) >= self._exit_due_ns
            ):
                self._submit_exit()
        elif event.client_order_id == self._exit_order_id:
            self._exit_filled += filled
            if self._exit_filled >= self._entry_filled:
                self._clear_trade()
        else:
            self._fail("received a fill for an unknown replay order")

    def on_order_rejected(self, event: object) -> None:
        """
        Fail closed when the simulation rejects an entry or exit.
        """
        self._terminal_order_failure(event, "rejected")

    def on_order_denied(self, event: object) -> None:
        """
        Fail closed when risk denies an entry or exit.
        """
        self._terminal_order_failure(event, "denied")

    def on_order_canceled(self, event: object) -> None:
        """
        Treat an unfilled FOK as an observed execution outcome.
        """
        self._handle_fok_no_fill(event, "canceled")

    def on_order_expired(self, event: object) -> None:
        """
        Treat an unfilled FOK as an observed execution outcome.
        """
        self._handle_fok_no_fill(event, "expired")

    def on_stop(self) -> None:
        """
        Cancel resting orders and flag any unclosed position.
        """
        self._cancel_timer_if_active(self._signal_timer_name)
        self._cancel_timer_if_active(self._exit_timer_name)
        self.cancel_all_orders(self._instrument_id)
        if self._active_signal is not None or not self.portfolio.is_net_flat(self._instrument_id):
            self._failures.append("replay ended with an unclosed candidate position")

    def on_reset(self) -> None:
        """
        Reset all mutable replay state.
        """
        self._cancel_timer_if_active(self._signal_timer_name)
        self._cancel_timer_if_active(self._exit_timer_name)
        self._instrument = None
        self._signals = ()
        self._cursor = 0
        self._bindings.clear()
        self._performance_samples.clear()
        self._performance_counts.clear()
        self._performance_times.clear()
        self._failures.clear()
        self._last_entry_ns = None
        self._clear_trade()

    def _latest_due_signal(self, now_ns: int) -> CandidateSignal | None:
        latest = None

        while self._cursor < len(self._signals):
            candidate = self._signals[self._cursor]
            if candidate.ts_recv_ns > now_ns:
                break
            latest = candidate
            self._cursor += 1
        return latest

    def _signal_side(self, signal: CandidateSignal) -> OrderSide | None:
        if (
            signal.expected_delta_ticks >= self._min_abs_delta_ticks
            and signal.p_up >= self._min_direction_probability
            and signal.p_up > signal.p_down
        ):
            return OrderSide.BUY
        if (
            signal.expected_delta_ticks <= -self._min_abs_delta_ticks
            and signal.p_down >= self._min_direction_probability
            and signal.p_down > signal.p_up
        ):
            return OrderSide.SELL
        return None

    def _submit_entry(self, signal: CandidateSignal, side: OrderSide, now_ns: int) -> None:
        if (
            self._replay_end_ns is not None
            and now_ns + signal.horizon_ms * 1_000_000 + 2 * self._order_insert_latency_ns
            >= self._replay_end_ns
        ):
            return
        if self._instrument is None:
            self._fail("instrument disappeared before entry")
            return
        price = self._marketable_price(side)
        if price is None:
            return
        order = self.order_factory.limit(
            instrument_id=self._instrument_id,
            order_side=side,
            quantity=Quantity.from_decimal_dp(self._trade_size, self._instrument.size_precision),
            price=price,
            time_in_force=TimeInForce.FOK,
            post_only=False,
        )
        self._active_signal = signal
        self._entry_order_id = order.client_order_id
        self._entry_side = side
        self._exit_due_ns = now_ns + signal.horizon_ms * 1_000_000
        self.clock.set_time_alert_ns(
            self._exit_timer_name,
            self._exit_due_ns,
            allow_past=False,
        )
        self._last_entry_ns = now_ns
        self._bind_order(
            order,
            signal=signal,
            action_role="ENTRY",
            decision_ts_ns=now_ns,
        )
        self.submit_order(order)

    def _submit_exit(self) -> None:
        if self._exit_order_id is not None or self._entry_filled <= 0:
            return
        if self.cache.orders_inflight(strategy_id=self.strategy_id):
            return
        if (
            self._instrument is None
            or self._active_signal is None
            or self._entry_side is None
            or self._exit_due_ns is None
        ):
            self._fail("candidate exit state is incomplete")
            return
        side = OrderSide.SELL if self._entry_side == OrderSide.BUY else OrderSide.BUY
        price = self._marketable_price(side)
        if price is None:
            return
        order = self.order_factory.limit(
            instrument_id=self._instrument_id,
            order_side=side,
            quantity=Quantity.from_decimal_dp(
                self._entry_filled,
                self._instrument.size_precision,
            ),
            price=price,
            time_in_force=TimeInForce.FOK,
            reduce_only=True,
            post_only=False,
        )
        self._exit_order_id = order.client_order_id
        self._bind_order(
            order,
            signal=self._active_signal,
            action_role="EXIT",
            decision_ts_ns=self._exit_due_ns,
        )
        self.submit_order(order)

    def _bind_order(
        self,
        order: LimitOrder,
        signal: CandidateSignal,
        action_role: str,
        decision_ts_ns: int,
    ) -> None:
        prediction_id = (
            signal.prediction_id if action_role == "ENTRY" else f"{signal.prediction_id}-exit"
        )
        self._bindings[str(order.client_order_id)] = {
            "prediction_id": prediction_id,
            "parent_prediction_id": signal.prediction_id,
            "signal_ts_recv_ns": signal.ts_recv_ns,
            "horizon_ms": signal.horizon_ms,
            "action_role": action_role,
            "research_symbol": self._research_symbol,
            "instrument_uid": str(self._instrument_id),
            "decision_ts_ns": decision_ts_ns,
            "side": order.side.name,
            "fees": Decimal(0),
            "last_fill_ts_ns": None,
        }

    def _marketable_price(self, side: OrderSide) -> Price | None:
        book = self.cache.order_book(self._instrument_id)
        if book is None:
            return None
        bid = book.best_bid_price()
        ask = book.best_ask_price()
        if bid is None or ask is None or ask < bid:
            return None
        return ask if side == OrderSide.BUY else bid

    def _terminal_order_failure(self, event: object, status: str) -> None:
        client_order_id = getattr(event, "client_order_id", None)
        if client_order_id in {self._entry_order_id, self._exit_order_id}:
            self._fail(f"candidate order {status}")

    def _handle_fok_no_fill(self, event: object, status: str) -> None:
        """
        Continue after an entry miss, but keep retrying a due position exit.
        """
        client_order_id = getattr(event, "client_order_id", None)
        if client_order_id == self._entry_order_id:
            if self._entry_filled:
                self._fail(f"partially filled FOK entry was {status}")
                return
            self._cancel_timer_if_active(self._exit_timer_name)
            self._clear_trade()
        elif client_order_id == self._exit_order_id:
            if self._exit_filled:
                self._fail(f"partially filled FOK exit was {status}")
                return
            self._exit_order_id = None
            self._cancel_timer_if_active(self._exit_timer_name)

    def _clear_trade(self) -> None:
        self._active_signal = None
        self._entry_order_id = None
        self._exit_order_id = None
        self._entry_side = None
        self._entry_filled = Decimal(0)
        self._exit_filled = Decimal(0)
        self._exit_due_ns = None

    def _cancel_timer_if_active(self, name: str) -> None:
        if name in self.clock.timer_names():
            self.clock.cancel_timer(name)

    def _fail(self, message: str) -> None:
        self._failures.append(message)
        self.log.error(message)
        self.stop()
