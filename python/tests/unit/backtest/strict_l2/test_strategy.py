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
Integration tests for causal candidate execution on an L2 MBP book.
"""

import hashlib
import json
from decimal import Decimal
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pandas as pd
import pytest

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.backtest.strict_l2.data import rows_to_deltas
from nautilus_trader.backtest.strict_l2.feedback import feedback_records_from_orders_report
from nautilus_trader.backtest.strict_l2.replay import _PerShareFeeModel
from nautilus_trader.backtest.strict_l2.replay import run_candidate_replay
from nautilus_trader.backtest.strict_l2.strategy import CandidateReplayConfig
from nautilus_trader.backtest.strict_l2.strategy import CandidateReplayStrategy
from nautilus_trader.config import StrategyConfig
from nautilus_trader.execution import StaticLatencyModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import Equity
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.persistence import ParquetDataCatalog


BASE_TS_NS = int(pd.Timestamp("2026-05-11 09:30:00", tz="America/New_York").tz_convert("UTC").value)


def test_candidate_replay_config_rejects_unknown_setting() -> None:
    """
    An unsupported strategy setting fails at construction.
    """
    with pytest.raises(TypeError, match="unexpected keyword argument"):
        CandidateReplayConfig(
            instrument_id="TEST.XNAS",
            research_symbol="TEST",
            audit_receipt_path="candidate-audit.json",
            cooldown_m=0,
        )


def test_candidate_replay_config_preserves_strategy_defaults() -> None:
    """
    Inherited strategy settings retain their defaults and accept overrides.
    """
    base_config = StrategyConfig()
    config = CandidateReplayConfig(
        instrument_id="TEST.XNAS",
        research_symbol="TEST",
        audit_receipt_path="candidate-audit.json",
        log_events=False,
    )

    for field in (
        "strategy_id",
        "order_id_tag",
        "oms_type",
        "external_order_instrument_ids",
        "manage_contingent_orders",
        "manage_gtd_expiry",
        "manage_stop",
        "market_exit_interval_ms",
        "market_exit_max_attempts",
        "market_exit_time_in_force",
        "market_exit_reduce_only",
        "use_uuid_client_order_ids",
        "use_hyphens_in_client_order_ids",
        "log_commands",
        "log_rejected_due_post_only_as_warning",
    ):
        assert getattr(config, field) == getattr(base_config, field)
    assert config.log_events is False


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _audit_receipt(
    tmp_path: Path,
    signal_times: list[int] | None = None,
) -> Path:
    signal_times = signal_times or [BASE_TS_NS]
    run_id = "0123456789abcdefabcd"
    candidate = tmp_path / run_id
    candidate.mkdir()
    model = candidate / "model.pt"
    model.write_bytes(b"sealed model")
    predictions = candidate / "predictions.parquet"
    pd.DataFrame(
        {
            "ts_recv": signal_times,
            "instrument": ["TEST"] * len(signal_times),
            "delta_mid_ticks_1000ms": [1.0] * len(signal_times),
            "p_down_1000ms": [0.1] * len(signal_times),
            "p_flat_1000ms": [0.1] * len(signal_times),
            "p_up_1000ms": [0.8] * len(signal_times),
            "history_retained_fraction": [1.0] * len(signal_times),
            "history_off_lattice_fraction": [0.0] * len(signal_times),
            "history_out_of_radius_fraction": [0.0] * len(signal_times),
        },
    ).set_index(["ts_recv", "instrument"]).to_parquet(predictions)
    bundle = {
        "schema_version": "lob-prediction-bundle/v2",
        "run_id": run_id,
        "evaluation_segment": "development_test",
        "spec_sha256": "a" * 64,
        "readiness_sha256": "b" * 64,
        "implementation_sha256": "c" * 64,
        "rows": len(signal_times),
        "prediction_file": predictions.name,
        "prediction_sha256": _sha256(predictions),
    }
    bundle_path = candidate / "prediction-bundle.json"
    bundle_path.write_text(json.dumps(bundle), encoding="utf-8")
    receipt = {
        "schema_version": "lob-candidate-audit/v1",
        "run_id": run_id,
        "candidate_path": str(candidate),
        "artifact_valid": True,
        "strict_l2_only": True,
        "evaluation_segment": "development_test",
        "source_spec_sha256": bundle["spec_sha256"],
        "readiness_sha256": bundle["readiness_sha256"],
        "implementation_sha256": bundle["implementation_sha256"],
        "model_sha256": _sha256(model),
        "prediction_bundle_sha256": _sha256(bundle_path),
        "predictions_sha256": _sha256(predictions),
        "symbols": ["TEST"],
        "baseline_screen": {
            "screening_effective": True,
            "required_symbols": 1,
            "overall": {"horizons": {"1000ms": {"joint_baseline_win": True}}},
            "symbols": {
                "TEST": {"horizons": {"1000ms": {"joint_baseline_win": True}}},
            },
        },
    }
    receipt_path = tmp_path / "candidate-audit.json"
    receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
    return receipt_path


def _row(
    event_index: int,
    sequence: int,
    ts_recv: int,
    side: str,
    price: int,
    size: int,
    delta: int,
    action: str,
    last: bool,
) -> dict[str, object]:
    return {
        "symbol": "TEST",
        "ts_event": ts_recv,
        "ts_recv": ts_recv,
        "sequence": sequence,
        "event_index": event_index,
        "side": side,
        "price": price,
        "size": size,
        "delta": delta,
        "action": action,
        "last": last,
    }


def _catalog(
    tmp_path: Path,
    *,
    exact_same_time_update: bool = False,
    exit_same_time_update: bool = False,
    shallow_bid: bool = False,
    sparse_updates: bool = False,
    unchanged_exit_update: bool = False,
) -> tuple[Path, Equity, Path]:
    instrument_id = InstrumentId(Symbol("TEST"), Venue("SIM"))
    instrument = Equity(
        instrument_id=instrument_id,
        raw_symbol=Symbol("TEST"),
        currency=Currency.from_str("USD"),
        price_precision=9,
        price_increment=Price.from_str("0.000000001"),
        lot_size=Quantity.from_int(1),
        min_quantity=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    snapshot_ts = BASE_TS_NS - 1_000_000_000
    rows = [
        _row(0, 0, snapshot_ts, "N", 0, 0, 0, "CLEAR", False),
        _row(1, 0, snapshot_ts, "B", 100_000_000_000, 10, 10, "SET", False),
        _row(2, 0, snapshot_ts, "A", 101_000_000_000, 10, 10, "SET", True),
    ]

    if exact_same_time_update:
        rows.extend(
            [
                _row(3, 1, BASE_TS_NS, "A", 101_000_000_000, 0, -10, "SET", False),
                _row(4, 1, BASE_TS_NS, "A", 102_000_000_000, 10, 10, "SET", True),
                _row(5, 2, BASE_TS_NS + 1_100_000_000, "A", 102_000_000_000, 10, 0, "SET", True),
                _row(6, 3, BASE_TS_NS + 1_200_000_000, "B", 100_000_000_000, 10, 0, "SET", True),
                _row(7, 4, BASE_TS_NS + 1_300_000_000, "A", 102_000_000_000, 10, 0, "SET", True),
                _row(8, 5, BASE_TS_NS + 2_100_000_000, "B", 100_000_000_000, 10, 0, "SET", True),
                _row(9, 6, BASE_TS_NS + 3_200_000_000, "A", 102_000_000_000, 10, 0, "SET", True),
            ],
        )
    else:
        rows.extend(
            [
                # No market-data event occurs exactly on the 100 ms model grid. The
                # strategy must release the prediction from Nautilus's clock timer,
                # using only the already completed snapshot.
                _row(3, 1, BASE_TS_NS + 100_000, "B", 100_000_000_000, 10, 0, "SET", True),
                _row(4, 2, BASE_TS_NS + 1_100_000_000, "A", 101_000_000_000, 10, 0, "SET", True),
                _row(5, 3, BASE_TS_NS + 1_200_000_000, "B", 100_000_000_000, 10, 0, "SET", True),
                _row(6, 4, BASE_TS_NS + 1_300_000_000, "A", 101_000_000_000, 10, 0, "SET", True),
                _row(7, 5, BASE_TS_NS + 2_100_000_000, "B", 100_000_000_000, 10, 0, "SET", True),
                _row(8, 6, BASE_TS_NS + 3_200_000_000, "A", 101_000_000_000, 10, 0, "SET", True),
            ],
        )
    if exit_same_time_update:
        if not exact_same_time_update:
            raise ValueError("exit_same_time_update requires exact_same_time_update")
        rows.pop()
        rows.extend(
            [
                _row(9, 6, BASE_TS_NS + 3_000_000_000, "B", 100_000_000_000, 0, -10, "SET", False),
                _row(10, 6, BASE_TS_NS + 3_000_000_000, "B", 99_000_000_000, 10, 10, "SET", True),
                _row(11, 7, BASE_TS_NS + 3_200_000_000, "A", 102_000_000_000, 10, 0, "SET", True),
                _row(12, 8, BASE_TS_NS + 3_400_000_000, "B", 99_000_000_000, 10, 0, "SET", True),
                # The retry submitted from the 3.4 s book update has 1 ms insertion
                # latency. A later event is required for the engine to process its fill.
                _row(13, 9, BASE_TS_NS + 3_600_000_000, "A", 102_000_000_000, 10, 0, "SET", True),
            ],
        )
    if sparse_updates:
        rows = rows[:3]
        if unchanged_exit_update:
            rows.append(
                _row(3, 1, BASE_TS_NS + 1_002_000_000, "A", 101_000_000_000, 10, 0, "SET", True),
            )
        index = len(rows)
        sequence = index - 2
        rows.extend(
            [
                _row(
                    index,
                    sequence,
                    BASE_TS_NS + 2_000_000_000,
                    "B",
                    100_000_000_000,
                    0,
                    -10,
                    "SET",
                    False,
                ),
                _row(
                    index + 1,
                    sequence,
                    BASE_TS_NS + 2_000_000_000,
                    "B",
                    99_000_000_000,
                    10,
                    10,
                    "SET",
                    True,
                ),
                _row(
                    index + 2,
                    sequence + 1,
                    BASE_TS_NS + 3_200_000_000,
                    "A",
                    101_000_000_000,
                    10,
                    0,
                    "SET",
                    True,
                ),
                _row(
                    index + 3,
                    sequence + 2,
                    BASE_TS_NS + 3_400_000_000,
                    "A",
                    101_000_000_000,
                    10,
                    0,
                    "SET",
                    True,
                ),
            ],
        )
    if shallow_bid:
        for row in rows:
            if row["side"] == "B" and row["ts_recv"] < BASE_TS_NS + 1_200_000_000:
                row["size"] = 1
                row["delta"] = 1 if row["sequence"] == 0 else 0
            elif row["side"] == "B" and row["ts_recv"] == BASE_TS_NS + 1_200_000_000:
                row["delta"] = 9
    target = tmp_path / "catalog"
    target.mkdir()
    catalog = ParquetDataCatalog(str(target))
    catalog.write_instruments([instrument])
    catalog.write_order_book_deltas(
        list(
            rows_to_deltas(
                rows,
                instrument.id,
                expected_symbol="TEST",
                price_precision=instrument.price_precision,
            ),
        ),
    )
    symbol_metadata = tmp_path / "symbol_metadata.json"
    symbol_metadata.write_text(
        json.dumps(
            {
                "venue": "SIM",
                "symbols": {"TEST": {"currency": "USD", "price_precision": 9}},
                "tick_rule": [{"price_gte_x1e9": 1_000_000_000, "tick_size_x1e9": 1}],
            },
        ),
        encoding="utf-8",
    )
    source_manifest = tmp_path / "published-dataset-manifest.json"
    source_manifest.write_text(
        json.dumps(
            {
                "schema_version": "research/published-dataset-manifest-v1",
                "dataset_kind": "strict-l2-mbp",
                "point_in_time": {"effective_at": "2026-05-11"},
                "contracts": {"l2": "strict-l2-v1"},
                "files": [
                    {"role": "l2_deltas", "symbol": "TEST"},
                    {
                        "role": "symbol_metadata",
                        "path": symbol_metadata.name,
                        "size_bytes": symbol_metadata.stat().st_size,
                        "sha256": _sha256(symbol_metadata),
                    },
                ],
            },
        ),
        encoding="utf-8",
    )
    files = [
        {
            "path": path.relative_to(target).as_posix(),
            "size_bytes": path.stat().st_size,
            "sha256": _sha256(path),
        }
        for path in sorted(item for item in target.rglob("*") if item.is_file())
    ]
    receipt = {
        "schema_version": "strict-l2-nautilus-catalog/v2",
        "catalog_path": str(target),
        "instrument_id": str(instrument.id),
        "symbol": "TEST",
        "records": len(rows),
        "first_ts_init_ns": snapshot_ts + 1,
        "last_ts_init_ns": rows[-1]["ts_recv"] + 1,
        "availability_tie_break_ns": 1,
        "source_manifest_sha256": _sha256(source_manifest),
        "symbol_metadata_sha256": _sha256(symbol_metadata),
        "price_precision": instrument.price_precision,
        "price_increment": str(instrument.price_increment),
        "currency": str(instrument.quote_currency),
        "files": files,
    }
    (target / "strict-l2-catalog-receipt.json").write_text(
        json.dumps(receipt),
        encoding="utf-8",
    )
    return target, instrument, source_manifest


def _replay_request(
    receipt: Path,
    catalog: Path,
    instrument: Equity,
    source_manifest: Path,
) -> dict[str, object]:
    signal_policy = {
        "horizon_ms": 1_000,
        "trade_size": "1",
        "min_abs_delta_ticks": 0.5,
        "min_direction_probability": 0.5,
        "cooldown_ms": 1_000,
        "max_signal_lag_ms": 0,
    }
    return {
        "schema_version": "strict-l2-candidate-replay-request/v2",
        "audit_receipt_path": str(receipt),
        "audit_receipt_sha256": _sha256(receipt),
        "trading_date": "2026-05-11",
        "source_manifest_path": str(source_manifest),
        "catalogs": [
            {
                "symbol": "TEST",
                "catalog_path": str(catalog),
                "instrument_id": str(instrument.id),
                "catalog_receipt_sha256": _sha256(
                    catalog / "strict-l2-catalog-receipt.json",
                ),
            },
        ],
        **signal_policy,
        "starting_balances": ["1_000_000 USD"],
        "fee_per_share_usd": "0.01",
        "order_insert_latency_ns": 1_000_000,
    }


def test_candidate_strategy_executes_and_closes_after_prediction_horizon(tmp_path: Path) -> None:
    """
    A released signal produces one closed aggressive L2 round trip.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, _ = _catalog(tmp_path)
    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name=str(instrument.id.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(catalog),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    strategy = CandidateReplayStrategy(
        CandidateReplayConfig(
            instrument_id=str(instrument.id),
            research_symbol="TEST",
            audit_receipt_path=str(receipt),
            horizon_ms=1_000,
            trade_size="1",
            max_signal_lag_ms=0,
        ),
    )
    node = BacktestNode([config])
    try:
        node.build()
        node.add_strategy(config.id, strategy)
        results = node.run()
        orders = node.generate_orders_report(config.id)

        assert len(results) == 1
        assert len(orders) == 2
        assert strategy.failures == ()
        assert strategy.consumed_signals == 1
        assert len(strategy.feedback_bindings) == 2
        assert node.get_engine_portfolio(config.id).is_net_flat(instrument.id)
        records = feedback_records_from_orders_report(
            orders.reset_index().to_dict("records"),
            strategy.feedback_bindings,
        )
        assert len(records) == 2
        assert all(record["filled_qty"] == 1 for record in records)
        assert all("client_order_id" not in record for record in records)
        assert {record["action_role"] for record in records} == {"ENTRY", "EXIT"}
        assert all(record["signal_ts_recv_ns"] == BASE_TS_NS for record in records)
        assert all(record["horizon_ms"] == 1_000 for record in records)
        entry = next(record for record in records if record["action_role"] == "ENTRY")
        exit_record = next(record for record in records if record["action_role"] == "EXIT")
        assert entry["decision_ts_ns"] == BASE_TS_NS
        assert exit_record["decision_ts_ns"] == BASE_TS_NS + 1_000_000_000
    finally:
        node.dispose()


def test_run_candidate_replay_publishes_sanitized_daily_feedback(tmp_path: Path) -> None:
    """
    The replay runner publishes an immutable L2 result and identifier-free feedback.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request_path = tmp_path / "replay-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")
    output_root = tmp_path / "replays"

    result = run_candidate_replay(request_path, output_root)

    output = Path(result["output"])
    feedback = json.loads((output / "execution-feedback.json").read_text())
    replay = json.loads((output / "replay-result.json").read_text())
    assert result["status"] == "complete"
    assert result["records"] == 2
    assert feedback["trading_date"] == "2026-05-11"
    assert replay["book_type"] == "L2_MBP"
    assert replay["request"]["audit_receipt_sha256"] == request["audit_receipt_sha256"]
    assert replay["request"]["source_manifest_sha256"] == _sha256(source_manifest)
    assert "audit_receipt_path" not in replay["request"]
    assert "source_manifest_path" not in replay["request"]
    assert "catalog_path" not in replay["request"]["catalogs"][0]
    assert str(tmp_path) not in json.dumps(replay)
    assert replay["fee_scenario"] == {
        "model": "per_share",
        "currency": "USD",
        "fee_per_share": "0.01",
    }
    assert sum(record["fees"] for record in feedback["records"]) == pytest.approx(0.02)
    assert replay["latency_scenario"] == {
        "model": "static_order_latency",
        "unit": "nanosecond",
        "order_insert_latency_ns": 1_000_000,
    }
    assert feedback["metrics"]["execution_assumptions"] == {
        "execution_mode": replay["execution_mode"],
        "market_data_availability": replay["market_data_availability"],
        "fee_scenario": replay["fee_scenario"],
        "latency_scenario": replay["latency_scenario"],
    }
    assert len(replay["catalog_receipts"]) == 1
    assert replay["catalog_receipts"][0]["sha256"] == _sha256(
        catalog / "strict-l2-catalog-receipt.json",
    )
    assert replay["catalog_receipts"][0]["file"] == "strict-l2-catalog-receipt.json"
    assert all("client_order_id" not in record for record in feedback["records"])


def test_run_candidate_replay_consumes_verified_byte_snapshots(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Replacements after verification cannot change the data consumed by the engine.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request_path = tmp_path / "replay-request.json"
    request_path.write_text(
        json.dumps(_replay_request(receipt, catalog, instrument, source_manifest)),
        encoding="utf-8",
    )
    catalog_receipt = json.loads((catalog / "strict-l2-catalog-receipt.json").read_text())
    artifact = catalog / catalog_receipt["files"][0]["path"]
    original_run = BacktestNode.run

    def run(node):
        receipt.write_text("{}", encoding="utf-8")
        artifact.write_bytes(b"replaced after verification")
        return original_run(node)

    monkeypatch.setattr(BacktestNode, "run", run)

    result = run_candidate_replay(request_path, tmp_path / "replays")

    assert result["status"] == "complete"
    assert result["records"] == 2


@pytest.mark.parametrize("latency_ns", [0, 1_000_000])
@pytest.mark.parametrize("unchanged_exit_update", [False, True])
def test_delayed_entry_exit_submission_and_settlement(
    tmp_path: Path,
    latency_ns: int,
    unchanged_exit_update: bool,
) -> None:
    """
    Overdue exits submit at the horizon and settle against the later available book.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, _ = _catalog(
        tmp_path,
        sparse_updates=True,
        unchanged_exit_update=unchanged_exit_update,
    )
    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name=str(instrument.id.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
                latency_model=StaticLatencyModel(insert_latency_nanos=latency_ns),
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(catalog),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    strategy = CandidateReplayStrategy(
        CandidateReplayConfig(
            instrument_id=str(instrument.id),
            research_symbol="TEST",
            audit_receipt_path=str(receipt),
            horizon_ms=1_000,
            trade_size="1",
            max_signal_lag_ms=0,
            order_insert_latency_ns=latency_ns,
        ),
    )
    node = BacktestNode([config])
    try:
        node.build()
        node.add_strategy(config.id, strategy)
        node.run()
        orders = node.generate_orders_report(config.id)
        records = feedback_records_from_orders_report(
            orders.reset_index().to_dict("records"),
            strategy.feedback_bindings,
        )
        assert strategy.failures == ()
        assert node.get_engine_portfolio(config.id).is_net_flat(instrument.id)
        entry = next(record for record in records if record["action_role"] == "ENTRY")
        exit_record = next(record for record in records if record["action_role"] == "EXIT")
        assert entry["filled_qty"] == exit_record["filled_qty"] == 1
        assert exit_record["decision_ts_ns"] == BASE_TS_NS + 1_000_000_000
        assert exit_record["submit_ts_ns"] == BASE_TS_NS + 1_000_000_000

        if latency_ns and not unchanged_exit_update:
            assert sorted(orders["status"].astype(str)) == ["CANCELED", "FILLED", "FILLED"]
            assert exit_record["status"] == "MIXED"
            assert exit_record["average_fill_price"] == 99
            assert exit_record["last_fill_ts_ns"] == BASE_TS_NS + 3_200_000_001
        else:
            assert sorted(orders["status"].astype(str)) == ["FILLED", "FILLED"]
            assert exit_record["status"] == "FILLED"
            assert exit_record["average_fill_price"] == 100
            fill_offset_ns = 1_002_000_001 if latency_ns else 1_000_000_000
            assert exit_record["last_fill_ts_ns"] == BASE_TS_NS + fill_offset_ns
    finally:
        node.dispose()


def test_run_candidate_replay_publishes_zero_trade_outcome(tmp_path: Path) -> None:
    """
    A fixed threshold yielding no orders is a completed, ineffective outcome.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request["min_abs_delta_ticks"] = 2.0
    request_path = tmp_path / "zero-trade-replay-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    result = run_candidate_replay(request_path, tmp_path / "replays")

    output = Path(result["output"])
    feedback = json.loads((output / "execution-feedback.json").read_text())
    assert result["status"] == "complete"
    assert result["records"] == 0
    assert feedback["records"] == []


@pytest.mark.parametrize("threshold", [float("nan"), float("inf"), float("-inf")])
def test_run_candidate_replay_rejects_non_finite_threshold(
    tmp_path: Path,
    threshold: float,
) -> None:
    """
    A non-finite signal threshold fails before replay execution.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request["min_abs_delta_ticks"] = threshold
    request_path = tmp_path / "non-finite-threshold-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    with pytest.raises(ValueError, match="min_abs_delta_ticks must be finite and non-negative"):
        run_candidate_replay(request_path, tmp_path / "replays")


@pytest.mark.parametrize(
    "field",
    [
        "trade_size",
        "horizon_ms",
        "min_abs_delta_ticks",
        "min_direction_probability",
        "cooldown_ms",
        "max_signal_lag_ms",
    ],
)
def test_run_candidate_replay_rejects_boolean_numeric_setting(
    tmp_path: Path,
    field: str,
) -> None:
    """
    A boolean numeric setting fails before replay execution.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request[field] = True
    request_path = tmp_path / "boolean-numeric-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    with pytest.raises(ValueError, match="replay numeric settings must use their declared types"):
        run_candidate_replay(request_path, tmp_path / "replays")


def test_run_candidate_replay_rejects_non_integer_timing(tmp_path: Path) -> None:
    """
    A non-integer timing setting fails before replay execution.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request["cooldown_ms"] = float("nan")
    request_path = tmp_path / "non-integer-timing-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    with pytest.raises(ValueError, match="replay numeric settings must use their declared types"):
        run_candidate_replay(request_path, tmp_path / "replays")


def test_candidate_strategy_rearms_exact_signal_timer(tmp_path: Path) -> None:
    """
    One bounded timer is rearmed for multiple grid-aligned predictions.
    """
    receipt = _audit_receipt(
        tmp_path,
        [BASE_TS_NS, BASE_TS_NS + 2_000_000_000],
    )
    catalog, instrument, _ = _catalog(tmp_path)
    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name=str(instrument.id.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(catalog),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    strategy = CandidateReplayStrategy(
        CandidateReplayConfig(
            instrument_id=str(instrument.id),
            research_symbol="TEST",
            audit_receipt_path=str(receipt),
            horizon_ms=1_000,
            trade_size="1",
            max_signal_lag_ms=0,
        ),
    )
    node = BacktestNode([config])
    try:
        node.build()
        node.add_strategy(config.id, strategy)
        node.run()
        orders = node.generate_orders_report(config.id)
        records = feedback_records_from_orders_report(
            orders.reset_index().to_dict("records"),
            strategy.feedback_bindings,
        )
        entries = [record for record in records if record["action_role"] == "ENTRY"]
        assert strategy.consumed_signals == 2
        assert len(orders) == 4
        assert sorted(record["decision_ts_ns"] for record in entries) == [
            BASE_TS_NS,
            BASE_TS_NS + 2_000_000_000,
        ]
    finally:
        node.dispose()


def test_candidate_timer_precedes_same_timestamp_book_update(tmp_path: Path) -> None:
    """
    The replay preserves the research sampler's left-closed time boundary.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, _ = _catalog(tmp_path, exact_same_time_update=True)
    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name=str(instrument.id.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(catalog),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    strategy = CandidateReplayStrategy(
        CandidateReplayConfig(
            instrument_id=str(instrument.id),
            research_symbol="TEST",
            audit_receipt_path=str(receipt),
            horizon_ms=1_000,
            trade_size="1",
            max_signal_lag_ms=0,
        ),
    )
    node = BacktestNode([config])
    try:
        node.build()
        node.add_strategy(config.id, strategy)
        node.run()
        orders = node.generate_orders_report(config.id)
        records = feedback_records_from_orders_report(
            orders.reset_index().to_dict("records"),
            strategy.feedback_bindings,
        )
        entry = next(record for record in records if record["action_role"] == "ENTRY")
        assert entry["decision_ts_ns"] == BASE_TS_NS
        assert entry["average_fill_price"] == pytest.approx(101.0)
    finally:
        node.dispose()


def test_candidate_replay_records_entry_miss_and_continues(tmp_path: Path) -> None:
    """
    A stale marketable FOK is a fill outcome, not a terminal replay fault.
    """
    receipt = _audit_receipt(
        tmp_path,
        [BASE_TS_NS, BASE_TS_NS + 2_000_000_000],
    )
    catalog, instrument, _ = _catalog(
        tmp_path,
        exact_same_time_update=True,
        exit_same_time_update=True,
    )
    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name=str(instrument.id.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
                latency_model=StaticLatencyModel(insert_latency_nanos=1_000_000),
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(catalog),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    strategy = CandidateReplayStrategy(
        CandidateReplayConfig(
            instrument_id=str(instrument.id),
            research_symbol="TEST",
            audit_receipt_path=str(receipt),
            horizon_ms=1_000,
            trade_size="1",
            max_signal_lag_ms=0,
        ),
    )
    node = BacktestNode([config])
    try:
        node.build()
        node.add_strategy(config.id, strategy)
        node.run()
        orders = node.generate_orders_report(config.id)

        assert strategy.failures == ()
        assert strategy.consumed_signals == 2
        assert len(orders) == 4
        assert node.get_engine_portfolio(config.id).is_net_flat(instrument.id)
        assert sorted(orders["status"].astype(str)) == [
            "CANCELED",
            "CANCELED",
            "FILLED",
            "FILLED",
        ]
        records = feedback_records_from_orders_report(
            orders.reset_index().to_dict("records"),
            strategy.feedback_bindings,
        )
        retried_exit = next(
            record
            for record in records
            if record["action_role"] == "EXIT" and record["status"] == "MIXED"
        )
        assert retried_exit["decision_ts_ns"] == BASE_TS_NS + 3_000_000_000
    finally:
        node.dispose()


def test_run_candidate_replay_rejects_catalog_changed_after_publication(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """
    Catalog mutation after publication fails before the replay engine starts.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    catalog_receipt = json.loads((catalog / "strict-l2-catalog-receipt.json").read_text())
    artifact = catalog / catalog_receipt["files"][0]["path"]
    artifact.write_bytes(artifact.read_bytes() + b"changed")
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request_path = tmp_path / "tampered-replay-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")
    output_root = tmp_path / "replays"
    artifact_copy_started = False
    original_open = Path.open

    def track_artifact_copy(
        path: Path,
        mode: str = "r",
        *args: Any,
        **kwargs: Any,
    ) -> Any:
        nonlocal artifact_copy_started
        if mode == "xb" and output_root in path.parents:
            artifact_copy_started = True
        return original_open(path, mode, *args, **kwargs)

    monkeypatch.setattr(Path, "open", track_artifact_copy)

    with pytest.raises(ValueError, match="catalog artifact digest mismatch"):
        run_candidate_replay(request_path, output_root)

    assert artifact_copy_started is False
    assert list(output_root.iterdir()) == []


def test_run_candidate_replay_rejects_coherently_rewritten_catalog(tmp_path: Path) -> None:
    """
    The request seals the receipt so changing both inventory and bytes fails closed.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request_path = tmp_path / "replay-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")
    catalog_receipt_path = catalog / "strict-l2-catalog-receipt.json"
    catalog_receipt = json.loads(catalog_receipt_path.read_text())
    artifact = catalog / catalog_receipt["files"][0]["path"]
    artifact.write_bytes(artifact.read_bytes() + b"changed")
    catalog_receipt["files"][0]["size_bytes"] = artifact.stat().st_size
    catalog_receipt["files"][0]["sha256"] = _sha256(artifact)
    catalog_receipt_path.write_text(json.dumps(catalog_receipt), encoding="utf-8")

    with pytest.raises(ValueError, match="catalog receipt SHA-256"):
        run_candidate_replay(request_path, tmp_path / "replays")


def test_run_candidate_replay_rejects_changed_audit_receipt(tmp_path: Path) -> None:
    """
    The replay request seals the independently generated audit receipt.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request_path = tmp_path / "replay-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")
    payload = json.loads(receipt.read_text(encoding="utf-8"))
    payload["baseline_screen"]["screening_effective"] = False
    receipt.write_text(json.dumps(payload), encoding="utf-8")

    with pytest.raises(ValueError, match="audit receipt SHA-256"):
        run_candidate_replay(request_path, tmp_path / "replays")


def test_run_candidate_replay_rejects_source_manifest_for_another_day(tmp_path: Path) -> None:
    """
    A catalog cannot be replayed under a different requested trading date.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    manifest = json.loads(source_manifest.read_text())
    manifest["point_in_time"]["effective_at"] = "2026-05-12"
    source_manifest.write_text(json.dumps(manifest), encoding="utf-8")
    request = _replay_request(receipt, catalog, instrument, source_manifest)
    request_path = tmp_path / "wrong-day-replay-request.json"
    request_path.write_text(json.dumps(request), encoding="utf-8")

    with pytest.raises(ValueError, match="requested strict-L2 trading day"):
        run_candidate_replay(request_path, tmp_path / "replays")


@pytest.mark.parametrize(("quantity", "expected"), [(100, "0.35"), (200, "0.70")])
def test_per_share_fee_preserves_subcent_rate(quantity: int, expected: str) -> None:
    """
    Accumulate per-share fees without binary floating-point drift.
    """
    model = _PerShareFeeModel(Decimal("0.0035"))
    commission = model.get_commission(
        None,
        Quantity.from_int(quantity),
        Price.from_str("100"),
        None,
    )
    assert commission.as_decimal() == Decimal(expected)


@pytest.mark.parametrize(
    ("end_offset_ns", "latency_ns", "shallow_bid", "expected_orders"),
    [
        (500_000_000, 0, False, 0),
        (1_000_000_000, 0, False, 0),
        (1_001_000_000, 1_000_000, False, 0),
        (3_000_000_000, 0, True, 4),
    ],
)
def test_candidate_exit_retry_and_session_entry_cutoff(
    tmp_path: Path,
    end_offset_ns: int,
    latency_ns: int,
    shallow_bid: bool,
    expected_orders: int,
) -> None:
    """
    Bound exit retries and reject entries too close to the replay cutoff.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, _ = _catalog(tmp_path, shallow_bid=shallow_bid)
    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name=str(instrument.id.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
                latency_model=StaticLatencyModel(insert_latency_nanos=latency_ns),
                fee_model=_PerShareFeeModel(Decimal("0.0035")),
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(catalog),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
        end=BASE_TS_NS + end_offset_ns,
    )
    strategy = CandidateReplayStrategy(
        CandidateReplayConfig(
            instrument_id=str(instrument.id),
            research_symbol="TEST",
            audit_receipt_path=str(receipt),
            horizon_ms=1_000,
            trade_size="10",
            max_signal_lag_ms=0,
            replay_end_ns=BASE_TS_NS + end_offset_ns,
            order_insert_latency_ns=latency_ns,
        ),
    )
    node = BacktestNode([config])
    try:
        node.build()
        node.add_strategy(config.id, strategy)
        node.run()
        orders = node.generate_orders_report(config.id)
        assert strategy.failures == ()
        assert len(orders) == expected_orders
        assert node.get_engine_portfolio(config.id).is_net_flat(instrument.id)

        if shallow_bid:
            assert sorted(orders["status"].astype(str)) == [
                "CANCELED",
                "CANCELED",
                "FILLED",
                "FILLED",
            ]
            records = feedback_records_from_orders_report(
                orders.reset_index().to_dict("records"),
                strategy.feedback_bindings,
            )
            assert all(record["fees"] == Decimal("0.04") for record in records)
            exit_record = next(record for record in records if record["action_role"] == "EXIT")
            assert exit_record["last_fill_ts_ns"] == BASE_TS_NS + 1_200_000_001
    finally:
        node.dispose()


@pytest.mark.parametrize("trade_size", ["0.1", "1.5", "2.5", "1.000"])
def test_candidate_strategy_requires_exact_instrument_quantity(
    tmp_path: Path,
    trade_size: str,
) -> None:
    """
    Reject trade sizes that equity precision would silently round.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, _ = _catalog(tmp_path)
    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name=str(instrument.id.venue),
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(catalog),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        dispose_on_completion=False,
    )
    strategy = CandidateReplayStrategy(
        CandidateReplayConfig(
            instrument_id=str(instrument.id),
            research_symbol="TEST",
            audit_receipt_path=str(receipt),
            trade_size=trade_size,
        ),
    )
    node = BacktestNode([config])
    try:
        node.build()
        node.add_strategy(config.id, strategy)
        node.run()
        orders = node.generate_orders_report(config.id)
        assert node.get_engine_portfolio(config.id).is_net_flat(instrument.id)

        if trade_size == "1.000":
            assert strategy.failures == ()
            assert len(orders) == 2
            assert all(
                Decimal(str(quantity)) == Decimal(trade_size) for quantity in orders["quantity"]
            )
        else:
            assert strategy.failures == (
                "trade_size is not exactly representable at the instrument size precision",
            )
            assert len(orders) == 0
            assert strategy.feedback_bindings == {}
            assert strategy.consumed_signals == 0
    finally:
        node.dispose()


@pytest.mark.parametrize(
    ("bid", "ask"),
    [
        (100_000_000_000, None),
        (100_000_000_000, 99_000_000_000),
        (100_000_000_000, 100_000_000_000),
        (100_000_000_000, 101_000_000_000),
        (100_000_000_000_000_001, 100_000_000_000_000_000),
        (100_000_000_000_000_000, 100_000_000_000_000_001),
    ],
)
@pytest.mark.parametrize("side", [OrderSide.BUY, OrderSide.SELL])
def test_candidate_book_guards_preserve_locked_and_reject_crossed(
    bid: int,
    ask: int | None,
    side: OrderSide,
) -> None:
    """
    Use native books to verify signal and entry/exit price guards independently.
    """
    instrument_id = InstrumentId.from_str("TEST.SIM")
    book = OrderBook(instrument_id, BookType.L2_MBP)
    levels = [("N", 0, 0, "CLEAR"), ("B", bid, 10, "SET")]
    if ask is not None:
        levels.append(("A", ask, 10, "SET"))
    rows = [
        _row(
            index,
            0,
            BASE_TS_NS,
            direction,
            price,
            size,
            size,
            action,
            index == len(levels) - 1,
        )
        for index, (direction, price, size, action) in enumerate(levels)
    ]

    for delta in rows_to_deltas(rows, instrument_id, expected_symbol="TEST"):
        book.apply_delta(delta)
    submissions = []
    signal = SimpleNamespace(ts_recv_ns=BASE_TS_NS)
    state = SimpleNamespace(
        cache=SimpleNamespace(order_book=lambda _: book),
        portfolio=SimpleNamespace(is_net_flat=lambda _: True),
        _instrument_id=instrument_id,
        _latest_due_signal=lambda _: signal,
        _active_signal=None,
        _last_entry_ns=None,
        _max_signal_lag_ns=0,
        _signal_side=lambda _: side,
        _submit_entry=lambda *args: submissions.append(args),
    )
    price = CandidateReplayStrategy._marketable_price(state, side)
    CandidateReplayStrategy._process_due_signal(state, BASE_TS_NS)
    valid = ask is not None and ask >= bid
    assert len(submissions) == int(valid)
    if valid:
        expected = ask if side == OrderSide.BUY else bid
        assert price.as_decimal() == Decimal(expected) / 10**9
    else:
        assert price is None
