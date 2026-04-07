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

import pytest

from nautilus_trader.common import ComponentState
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.trading import ForexSession
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from nautilus_trader.trading import fx_local_from_utc
from nautilus_trader.trading import fx_next_end
from nautilus_trader.trading import fx_next_start
from nautilus_trader.trading import fx_prev_end
from nautilus_trader.trading import fx_prev_start


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
