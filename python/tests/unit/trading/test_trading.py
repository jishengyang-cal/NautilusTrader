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

import datetime as dt
import inspect
from decimal import Decimal

import pytest

from nautilus_trader.common import ComponentState
from nautilus_trader.common import CustomData
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import DataType
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.trading import ForexSession
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from nautilus_trader.trading import fx_local_from_utc
from nautilus_trader.trading import fx_next_end
from nautilus_trader.trading import fx_next_start
from nautilus_trader.trading import fx_prev_end
from nautilus_trader.trading import fx_prev_start
from tests.providers import TestInstrumentProvider


def test_strategy_default_construction():
    strategy = Strategy()

    assert strategy.trader_id is None
    assert strategy.strategy_id is not None
    assert strategy.state() == ComponentState.PRE_INITIALIZED
    assert strategy.is_ready() is False
    assert strategy.is_running() is False
    assert strategy.is_stopped() is False
    assert strategy.is_disposed() is False
    assert strategy.is_degraded() is False
    assert strategy.is_faulted() is False


def test_strategy_construction_with_config():
    config = StrategyConfig(
        StrategyId("S-001"),
        "001",
        None,
        None,
        False,
        False,
        False,
        0,
        3,
        TimeInForce.GTC,
        False,
        True,
        True,
        True,
        True,
        False,
    )
    strategy = Strategy(config)

    assert strategy.strategy_id == StrategyId("S-001")


def test_strategy_clock_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.clock


def test_strategy_cache_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.cache


LIFECYCLE_METHODS = ["start", "stop", "resume", "reset", "dispose", "degrade", "fault"]


@pytest.mark.parametrize("method_name", LIFECYCLE_METHODS)
def test_strategy_lifecycle_methods_reject_pre_initialized(method_name):
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="Invalid state trigger PRE_INITIALIZED"):
        getattr(strategy, method_name)()


def test_strategy_submit_order_signature():
    sig = inspect.signature(Strategy.submit_order)
    params = tuple(sig.parameters)

    assert "order" in params
    assert "position_id" in params
    assert "client_id" in params


def test_strategy_config_defaults():
    config = StrategyConfig(
        None,
        None,
        None,
        None,
        False,
        False,
        False,
        0,
        3,
        TimeInForce.GTC,
        False,
        True,
        True,
        True,
        True,
        False,
    )

    assert config.strategy_id is None
    assert config.order_id_tag is None
    assert config.oms_type is None
    assert config.manage_contingent_orders is False
    assert config.manage_gtd_expiry is False
    assert config.use_uuid_client_order_ids is True
    assert config.use_hyphens_in_client_order_ids is True
    assert config.log_events is True
    assert config.log_commands is True
    assert config.log_rejected_due_post_only_as_warning is False


def test_strategy_config_with_explicit_values():
    config = StrategyConfig(
        StrategyId("S-002"),
        "002",
        None,
        None,
        True,
        True,
        False,
        500,
        5,
        TimeInForce.IOC,
        True,
        False,
        False,
        False,
        False,
        True,
    )

    assert config.strategy_id == StrategyId("S-002")
    assert config.order_id_tag == "002"
    assert config.manage_contingent_orders is True
    assert config.manage_gtd_expiry is True
    assert config.use_uuid_client_order_ids is False
    assert config.use_hyphens_in_client_order_ids is False
    assert config.log_events is False
    assert config.log_commands is False
    assert config.log_rejected_due_post_only_as_warning is True


def test_forex_session_variants():
    variants = list(ForexSession.variants())

    assert len(variants) == 4
    assert ForexSession.from_str("SYDNEY") == ForexSession.SYDNEY
    assert ForexSession.from_str("TOKYO") == ForexSession.TOKYO
    assert ForexSession.from_str("LONDON") == ForexSession.LONDON
    assert ForexSession.from_str("NEW_YORK") == ForexSession.NEW_YORK


NOW_UTC = dt.datetime(2024, 6, 15, 12, 0, 0, tzinfo=dt.UTC)


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_next_start_returns_future_datetime(session):
    result = fx_next_start(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result > NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_next_end_returns_future_datetime(session):
    result = fx_next_end(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result > NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_prev_start_returns_past_datetime(session):
    result = fx_prev_start(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result < NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_prev_end_returns_past_datetime(session):
    result = fx_prev_end(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result < NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_local_from_utc_returns_string(session):
    result = fx_local_from_utc(session, NOW_UTC)

    assert isinstance(result, str)
    assert "2024" in result


@pytest.fixture
def strategy():
    return Strategy()


@pytest.fixture
def strategy_sample_objects():
    instrument = TestInstrumentProvider.audusd_sim()
    quote = QuoteTick(
        instrument.id,
        Price.from_str("1.00000"),
        Price.from_str("1.00001"),
        Quantity.from_int(1),
        Quantity.from_int(2),
        1,
        2,
    )
    trade = TradeTick(
        instrument.id,
        Price.from_str("1.00000"),
        Quantity.from_int(10),
        AggressorSide.BUYER,
        TradeId("T-001"),
        1,
        2,
    )
    bar_type = BarType.from_str(f"{instrument.id}-1-MINUTE-LAST-EXTERNAL")
    bar = Bar(
        bar_type,
        Price.from_str("1.00000"),
        Price.from_str("1.10000"),
        Price.from_str("0.90000"),
        Price.from_str("1.05000"),
        Quantity.from_int(100),
        1,
        2,
    )
    custom_data = CustomData(DataType("X"), [1, 2], 3, 4)
    mark_price = MarkPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    index_price = IndexPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    funding_rate = FundingRateUpdate(instrument.id, Decimal("0.0001"), 1, 2, interval=480)

    return {
        "historical_data": custom_data,
        "historical_quotes": [quote],
        "historical_trades": [trade],
        "historical_funding_rates": [funding_rate],
        "historical_bars": [bar],
        "historical_mark_prices": [mark_price],
        "historical_index_prices": [index_price],
    }


STRATEGY_HISTORICAL_CALLBACKS = [
    ("on_historical_data", "historical_data"),
    ("on_historical_quotes", "historical_quotes"),
    ("on_historical_trades", "historical_trades"),
    ("on_historical_funding_rates", "historical_funding_rates"),
    ("on_historical_bars", "historical_bars"),
    ("on_historical_mark_prices", "historical_mark_prices"),
    ("on_historical_index_prices", "historical_index_prices"),
]


@pytest.mark.parametrize(("method_name", "sample_name"), STRATEGY_HISTORICAL_CALLBACKS)
def test_strategy_historical_callbacks_accept_runtime_objects(
    strategy,
    strategy_sample_objects,
    method_name,
    sample_name,
):
    assert getattr(strategy, method_name)(strategy_sample_objects[sample_name]) is None
