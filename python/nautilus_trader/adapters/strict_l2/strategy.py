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

"""Execution-bound strategy for audited strict-L2 candidate replay."""

from __future__ import annotations

from decimal import Decimal
from typing import TYPE_CHECKING
from typing import Any

from nautilus_trader.adapters.strict_l2.candidate import CandidateSignal
from nautilus_trader.adapters.strict_l2.candidate import load_candidate_signals
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
    from nautilus_trader.common import TimeEvent


MIN_DIRECTION_PROBABILITY = 0.5


class CandidateReplayConfig(StrategyConfig):
    """Configuration for one-symbol candidate replay."""

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
        require_screening_effective: bool = True,
        **_kwargs: object,
    ) -> None:
        """Initialize the candidate replay configuration."""
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
        self.require_screening_effective = require_screening_effective


class CandidateReplayStrategy(Strategy):
    """Replay audited signals causally and hold each filled position for its horizon."""

    def __init__(self, config: CandidateReplayConfig) -> None:
        """Initialize causal signal and round-trip execution state."""
        if not config.research_symbol:
            raise ValueError("research_symbol must not be empty")
        if Decimal(config.trade_size) <= 0:
            raise ValueError("trade_size must be positive")
        if config.min_abs_delta_ticks < 0:
            raise ValueError("min_abs_delta_ticks must be non-negative")
        if not MIN_DIRECTION_PROBABILITY <= config.min_direction_probability <= 1:
            raise ValueError("min_direction_probability must be in [0.5, 1]")
        if config.cooldown_ms < 0 or config.max_signal_lag_ms < 0:
            raise ValueError("replay timing limits must be non-negative")
        if (
            config.replay_start_ns is not None
            and config.replay_end_ns is not None
            and config.replay_start_ns >= config.replay_end_ns
        ):
            raise ValueError("replay time bounds must define a positive interval")
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
        self._require_screening_effective = config.require_screening_effective
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

    @property
    def feedback_bindings(self) -> dict[str, dict[str, Any]]:
        """Return detached execution bindings for identifier-free feedback export."""
        return {key: dict(value) for key, value in self._bindings.items()}

    @property
    def failures(self) -> tuple[str, ...]:
        """Return terminal execution failures observed during replay."""
        return tuple(self._failures)

    @property
    def consumed_signals(self) -> int:
        """Return the number of signals released by the replay clock."""
        return self._cursor

    def on_start(self) -> None:
        """Load sealed signals and subscribe to the managed L2 MBP book."""
        self._instrument = self.cache.instrument(self._instrument_id)
        if self._instrument is None:
            self._fail("instrument is absent from the replay catalog")
            return
        self._signals = load_candidate_signals(
            self._audit_receipt_path,
            horizon_ms=self._horizon_ms,
            instruments={self._research_symbol},
            start_ns=self._replay_start_ns,
            end_ns=self._replay_end_ns,
            require_screening_effective=self._require_screening_effective,
        )
        self.subscribe_book_deltas(self._instrument_id, BookType.L2_MBP, managed=True)
        self._schedule_next_signal()

    def on_book_deltas(self, _deltas: OrderBookDeltas) -> None:
        """Maintain a safety fallback for a due exit after a clock discontinuity."""
        now_ns = int(self.clock.timestamp_ns())
        if self._exit_due_ns is not None and now_ns >= self._exit_due_ns:
            self._submit_exit()

    def on_time_event(self, event: TimeEvent) -> None:
        """Release model decisions and exits on their exact observable clock."""
        now_ns = int(event.ts_event)
        if event.name == self._signal_timer_name:
            self._process_due_signal(now_ns)
            self._schedule_next_signal()
        elif event.name == self._exit_timer_name:
            self._submit_exit()

    def _process_due_signal(self, now_ns: int) -> None:
        """Evaluate the newest prediction observable at ``now_ns``."""
        latest = self._latest_due_signal(now_ns)
        book = self.cache.order_book(self._instrument_id)
        if latest is None or book is None or not book.spread():
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
        """Schedule one alert, keeping timer count bounded for long sessions."""
        if self._cursor >= len(self._signals):
            return
        self.clock.set_time_alert_ns(
            self._signal_timer_name,
            self._signals[self._cursor].ts_recv_ns,
            allow_past=False,
        )

    def on_order_filled(self, event: OrderFilled) -> None:
        """Track partial fills and make fees available to daily feedback."""
        client_order_id = str(event.client_order_id)
        binding = self._bindings.get(client_order_id)
        if binding is None:
            self._fail("received a fill without a prediction binding")
            return
        if event.commission is not None:
            binding["fees"] += event.commission.as_double()
        filled = event.last_qty.as_decimal()
        if event.client_order_id == self._entry_order_id:
            self._entry_filled += filled
        elif event.client_order_id == self._exit_order_id:
            self._exit_filled += filled
            if self._exit_filled >= self._entry_filled:
                self._clear_trade()
        else:
            self._fail("received a fill for an unknown replay order")

    def on_order_rejected(self, event: object) -> None:
        """Fail closed when the simulation rejects an entry or exit."""
        self._terminal_order_failure(event, "rejected")

    def on_order_denied(self, event: object) -> None:
        """Fail closed when risk denies an entry or exit."""
        self._terminal_order_failure(event, "denied")

    def on_order_canceled(self, event: object) -> None:
        """Treat an unfilled FOK as an observed execution outcome."""
        self._handle_fok_no_fill(event, "canceled")

    def on_order_expired(self, event: object) -> None:
        """Treat an unfilled FOK as an observed execution outcome."""
        self._handle_fok_no_fill(event, "expired")

    def on_stop(self) -> None:
        """Cancel resting orders and flag any unclosed position."""
        self._cancel_timer_if_active(self._signal_timer_name)
        self._cancel_timer_if_active(self._exit_timer_name)
        self.cancel_all_orders(self._instrument_id)
        if self._active_signal is not None or not self.portfolio.is_net_flat(self._instrument_id):
            self._failures.append("replay ended with an unclosed candidate position")

    def on_reset(self) -> None:
        """Reset all mutable replay state."""
        self._cancel_timer_if_active(self._signal_timer_name)
        self._cancel_timer_if_active(self._exit_timer_name)
        self._instrument = None
        self._signals = ()
        self._cursor = 0
        self._bindings.clear()
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
            signal.prediction_id
            if action_role == "ENTRY"
            else f"{signal.prediction_id}-exit"
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
            "fees": 0.0,
        }

    def _marketable_price(self, side: OrderSide) -> Price | None:
        book = self.cache.order_book(self._instrument_id)
        if book is None or not book.spread():
            return None
        return book.best_ask_price() if side == OrderSide.BUY else book.best_bid_price()

    def _terminal_order_failure(self, event: object, status: str) -> None:
        client_order_id = getattr(event, "client_order_id", None)
        if client_order_id in {self._entry_order_id, self._exit_order_id}:
            self._fail(f"candidate order {status}")

    def _handle_fok_no_fill(self, event: object, status: str) -> None:
        """Continue after an entry miss, but keep retrying a due position exit."""
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
            # The position remains open. Re-arm the clock one nanosecond later
            # so order-state processing completes before the next attempt.
            self._exit_order_id = None
            self._cancel_timer_if_active(self._exit_timer_name)
            self.clock.set_time_alert_ns(
                self._exit_timer_name,
                int(self.clock.timestamp_ns()) + 1,
                allow_past=False,
            )

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
