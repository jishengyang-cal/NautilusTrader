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
Test interactive brokers factories behavior.
"""

from pathlib import Path
from types import SimpleNamespace

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import capture_exec_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.databento import DatabentoDataClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersDataClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecutionClientConfig
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersExecutionClientFactory
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers import MarketDataType
from nautilus_trader.adapters.interactive_brokers import SymbologyMethod
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId


IB = "IB"
ib_data_tester = load_example_module("interactive_brokers", "data_tester")
ib_exec_tester = load_example_module("interactive_brokers", "exec_tester")
ib_order_strategies = load_example_module("interactive_brokers", "ib_v2_order_strategies")
ib_with_databento = load_example_module("interactive_brokers", "with_databento_client")


def test_interactive_brokers_factories_expose_python_names() -> None:
    """
    Test interactive brokers factories expose python names.
    """
    assert InteractiveBrokersDataClientFactory().name() == IB
    assert InteractiveBrokersExecutionClientFactory().name() == IB


def test_interactive_brokers_instrument_provider_config_and_empty_surface() -> None:
    """
    Test interactive brokers instrument provider config and empty surface.
    """
    instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    contract = {"secType": "STK", "symbol": "MSFT", "exchange": "SMART"}
    config = InteractiveBrokersInstrumentProviderConfig(
        symbology_method=SymbologyMethod.RAW,
        load_ids={instrument_id},
        load_contracts=[contract],
        min_expiry_days=2,
        max_expiry_days=30,
        build_options_chain=True,
        build_futures_chain=False,
        cache_validity_days=7,
        convert_exchange_to_mic_venue=True,
        symbol_to_mic_venue={"AAPL": "XNAS"},
        filter_sec_types={"OPT", "STK"},
        filter_callable="package.module:filter_instrument",
        cache_path="cache/instruments.json",
    )
    provider = InteractiveBrokersInstrumentProvider(config)

    assert config.symbology_method == SymbologyMethod.RAW
    assert config.load_ids == {instrument_id}
    assert config.load_contracts == [contract]
    assert config.min_expiry_days == 2
    assert config.max_expiry_days == 30
    assert config.build_options_chain is True
    assert config.build_futures_chain is False
    assert config.cache_validity_days == 7
    assert config.convert_exchange_to_mic_venue is True
    assert config.symbol_to_mic_venue == {"AAPL": "XNAS"}
    assert set(config.filter_sec_types) == {"OPT", "STK"}
    assert config.filter_callable == "package.module:filter_instrument"
    assert config.cache_path == "cache/instruments.json"
    assert provider.count() == 0
    assert provider.get_all() == []
    assert provider.find(instrument_id) is None


def test_live_node_builder_accepts_interactive_brokers_data_factory() -> None:
    """
    Test live node builder accepts interactive brokers data factory.
    """
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("IB-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            InteractiveBrokersDataClientFactory(),
            InteractiveBrokersDataClientConfig(
                client_id=101,
                market_data_type=MarketDataType.DELAYED,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_interactive_brokers_exec_factory() -> None:
    """
    Test live node builder accepts interactive brokers exec factory.
    """
    trader_id = TraderId.from_str("TESTER-001")
    node = (
        LiveNode.builder("IB-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            InteractiveBrokersDataClientFactory(),
            InteractiveBrokersDataClientConfig(
                client_id=101,
                market_data_type=MarketDataType.DELAYED,
            ),
        )
        .add_exec_client(
            None,
            InteractiveBrokersExecutionClientFactory(),
            InteractiveBrokersExecutionClientConfig(client_id=101, account_id="U1234567"),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_interactive_brokers_data_tester_runs(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers data tester runs.
    """
    captured = capture_data_tester_main(monkeypatch, ib_data_tester)
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["request_instruments"] is True
    assert captured["run_called"] is True


def test_interactive_brokers_exec_tester_requires_account(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers exec tester requires account.
    """
    monkeypatch.delenv("TWS_ACCOUNT", raising=False)

    with pytest.raises(SystemExit, match="TWS_ACCOUNT must be set"):
        ib_exec_tester.main()


def test_interactive_brokers_exec_tester_runs_live_orders(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test interactive brokers exec tester runs live orders.
    """
    monkeypatch.setenv("TWS_ACCOUNT", "U1234567")
    captured = capture_exec_tester_main(monkeypatch, ib_exec_tester)
    kwargs = captured["exec_tester_kwargs"]
    _, _, exec_config = captured["exec_client_args"]

    assert isinstance(kwargs, dict)
    assert isinstance(exec_config, InteractiveBrokersExecutionClientConfig)
    assert exec_config.account_id == "U1234567"
    assert kwargs["dry_run"] is False
    assert kwargs["enable_limit_buys"] is True
    assert kwargs["enable_limit_sells"] is True
    assert captured["run_called"] is True


def test_databento_market_order_strategy_loads_instrument_offline(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test Databento market order strategy loads its instrument offline.
    """

    class ProbeStrategy(ib_order_strategies.DatabentoMarketOrderStrategy):
        requested_client_id: object | None = None
        submitted = False

        def request_instrument(
            self,
            instrument_id: object,
            *,
            client_id: object | None = None,
            **_: object,
        ) -> str:
            self.requested_client_id = client_id
            self.on_instrument(SimpleNamespace(id=instrument_id))
            return "offline-request"

        def submit_example_orders(self) -> None:
            self.submitted = True

    monkeypatch.setenv("IB_V2_ENABLE_ORDER_SUBMISSION", "1")
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    strategy = ProbeStrategy()

    try:
        engine.add_strategy(strategy)
        engine.run()

        assert strategy.requested_client_id == ib_order_strategies.databento_client_id()
        assert strategy.instrument is not None
        assert strategy.submitted is True
    finally:
        engine.dispose()


@pytest.mark.parametrize(
    ("subscribe_quotes", "expected_market_data"),
    [
        (True, "quotes"),
        (False, "trades"),
    ],
)
def test_databento_subscription_strategy_dispatches_offline(
    monkeypatch: pytest.MonkeyPatch,
    subscribe_quotes: bool,
    expected_market_data: str,
) -> None:
    """
    Test Databento subscription strategy dispatches offline.
    """

    class ProbeStrategy(ib_order_strategies.DatabentoSubscriptionStrategy):
        operations: list[tuple[object, ...]] = []

        def subscribe_quotes(
            self,
            instrument_id: object,
            *,
            client_id: object | None = None,
            **_: object,
        ) -> None:
            type(self).operations.append(("quotes", instrument_id, client_id))

        def subscribe_trades(
            self,
            instrument_id: object,
            *,
            client_id: object | None = None,
            **_: object,
        ) -> None:
            type(self).operations.append(("trades", instrument_id, client_id))

        def subscribe_book_deltas(
            self,
            instrument_id: object,
            book_type: object,
            *,
            client_id: object | None = None,
            **_: object,
        ) -> None:
            type(self).operations.append(
                ("book_deltas", instrument_id, book_type, client_id),
            )

        def subscribe_instrument_status(
            self,
            instrument_id: object,
            *,
            client_id: object | None = None,
            **_: object,
        ) -> None:
            type(self).operations.append(("status", instrument_id, client_id))

    monkeypatch.setenv("IB_V2_DATABENTO_SUBSCRIBE_QUOTES", str(int(subscribe_quotes)))
    monkeypatch.setenv("IB_V2_DATABENTO_SUBSCRIBE_TRADES", "1")
    monkeypatch.setenv("IB_V2_DATABENTO_SUBSCRIBE_BARS", "0")
    monkeypatch.setenv("IB_V2_DATABENTO_SUBSCRIBE_MBO", "1")
    monkeypatch.setenv("IB_V2_DATABENTO_SUBSCRIBE_STATUS", "1")
    ProbeStrategy.operations = []
    strategy = ProbeStrategy()

    strategy.on_start()

    client_id = ib_order_strategies.databento_client_id()
    assert ProbeStrategy.operations == [
        (expected_market_data, strategy.instrument_id, client_id),
        (
            "book_deltas",
            strategy.instrument_id,
            ib_order_strategies.BookType.L3_MBO,
            client_id,
        ),
        ("status", strategy.instrument_id, client_id),
    ]


def test_databento_and_ib_configuration_are_isolated(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Test Databento and IB configuration are isolated.
    """
    captured: dict[str, object] = {}

    class CaptureNode:
        trader_id = TraderId.from_str("IB-V2-DATABENTO-001")

        def add_strategy_from_config(self, _config: object) -> None:
            pass

    class CaptureBuilder:
        def with_timeout_connection(self, _timeout_secs: int) -> "CaptureBuilder":
            return self

        def with_reconciliation(self, _reconciliation: bool) -> "CaptureBuilder":
            return self

        def add_data_client(self, *args: object) -> "CaptureBuilder":
            captured["data_client_args"] = args
            return self

        def add_exec_client(self, *args: object) -> "CaptureBuilder":
            captured["exec_client_args"] = args
            return self

        def build(self) -> CaptureNode:
            return CaptureNode()

    class CaptureLiveNode:
        @staticmethod
        def builder(
            _name: str,
            _trader_id: object,
            _environment: object,
        ) -> CaptureBuilder:
            return CaptureBuilder()

    publishers_path = (
        Path(__file__).resolve().parents[5] / "crates/adapters/databento/publishers.json"
    )
    monkeypatch.setenv("DATABENTO_API_KEY", "d" * 32)
    monkeypatch.setenv("DATABENTO_PUBLISHERS_FILE", str(publishers_path))
    monkeypatch.setenv("IB_V2_ENABLE_EXECUTION", "1")
    monkeypatch.setenv("TWS_ACCOUNT", "DU1234567")
    monkeypatch.setenv("IB_V2_HOST", "ib-paper.test")
    monkeypatch.setenv("IB_V2_EXEC_CLIENT_ID", "1312")
    monkeypatch.setenv("IB_V2_CONNECTION_TIMEOUT", "11")
    monkeypatch.setenv("IB_V2_REQUEST_TIMEOUT", "22")
    monkeypatch.delenv("IB_V2_RUN_NODE", raising=False)
    monkeypatch.setattr(ib_with_databento, "LiveNode", CaptureLiveNode)

    ib_with_databento.main()

    data_client_args = captured["data_client_args"]
    exec_client_args = captured["exec_client_args"]
    assert isinstance(data_client_args, tuple)
    assert isinstance(exec_client_args, tuple)
    _, _, data_config = data_client_args
    _, _, exec_config = exec_client_args
    assert isinstance(data_config, DatabentoDataClientConfig)
    assert data_config.publishers_filepath == publishers_path
    assert data_config.use_exchange_as_venue is True
    assert not hasattr(data_config, "account_id")
    assert isinstance(exec_config, InteractiveBrokersExecutionClientConfig)
    assert exec_config.host == "ib-paper.test"
    assert exec_config.port == 4002
    assert exec_config.client_id == 1312
    assert exec_config.account_id == "DU1234567"
    assert exec_config.connection_timeout == 11
    assert exec_config.request_timeout == 22
    assert not hasattr(exec_config, "publishers_filepath")
