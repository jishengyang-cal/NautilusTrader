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
Tests for immutable, identifier-free execution feedback publication.
"""

import json
from decimal import Decimal
from pathlib import Path
from typing import Any

import numpy as np
import pytest
import simplejson

from nautilus_trader.backtest.strict_l2.feedback import feedback_records_from_orders_report
from nautilus_trader.backtest.strict_l2.feedback import publish_execution_feedback
from nautilus_trader.backtest.strict_l2.feedback_cli import export_feedback_from_json


def _publish(tmp_path: Path, records: list[dict[str, Any]]) -> dict[str, Any]:
    return publish_execution_feedback(
        tmp_path / "feedback.json",
        environment="backtest",
        trading_date="2026-09-10",
        run_id="run",
        model_id="model",
        model_artifact_sha256="a" * 64,
        feature_manifest_sha256="b" * 64,
        reconciliation={"status": "complete"},
        records=records,
        metrics={"pnl_after_fees": 1.0},
    )


def test_feedback_is_atomic_sanitized_and_immutable(tmp_path: Path) -> None:
    """
    A complete feedback file is atomic and never overwritten.
    """
    result = _publish(tmp_path, [])
    payload = json.loads((tmp_path / "feedback.json").read_text())
    assert result["records"] == 0
    assert payload["timestamp_unit"] == "nanosecond"
    assert not (tmp_path / ".feedback.json.incomplete").exists()
    with pytest.raises(FileExistsError):
        _publish(tmp_path, [])


def test_feedback_rejects_nested_execution_identity(tmp_path: Path) -> None:
    """
    Nested broker and order identity is excluded from research feedback.
    """
    with pytest.raises(ValueError, match="forbidden"):
        _publish(tmp_path, [{"fills": [{"venueOrderId": "secret"}]}])


def test_orders_report_is_aggregated_without_order_identity() -> None:
    """
    Strategy bindings join native rows then disappear from research output.
    """
    rows = [
        {
            "client_order_id": "ORDER-1",
            "instrument_id": "NVDA.XNAS",
            "side": "BUY",
            "status": "FILLED",
            "filled_qty": "2.0",
            "avg_px": "100.0",
            "ts_init": np.int64(1_000),
            "ts_last": np.int64(2_000),
        },
        {
            "client_order_id": "ORDER-2",
            "instrument_id": "NVDA.XNAS",
            "side": "BUY",
            "status": "FILLED",
            "filled_qty": "1.0",
            "avg_px": "103.0",
            "ts_init": 1_500,
            "ts_last": 2_500,
        },
    ]
    bindings = {
        "ORDER-1": {
            "prediction_id": "prediction-1",
            "instrument_uid": "nvda-canonical",
            "decision_ts_ns": 900,
            "fees": 0.2,
            "last_fill_ts_ns": 2_000,
        },
        "ORDER-2": {
            "prediction_id": "prediction-1",
            "instrument_uid": "nvda-canonical",
            "decision_ts_ns": 900,
            "fees": 0.1,
            "last_fill_ts_ns": 2_500,
        },
    }
    records = feedback_records_from_orders_report(rows, bindings)
    assert len(records) == 1
    assert records[0]["filled_qty"] == 3.0
    assert records[0]["average_fill_price"] == 101.0
    assert records[0]["fees"] == Decimal("0.3")
    assert records[0]["last_fill_ts_ns"] == 2_500
    assert "client_order_id" not in simplejson.dumps(records, use_decimal=True)


def test_feedback_rejects_cross_day_fill_and_incomplete_reconciliation(tmp_path: Path) -> None:
    """
    A daily artifact cannot include another trading day or unreconciled execution.
    """
    record = {
        "prediction_id": "prediction-1",
        "instrument_uid": "nvda-canonical",
        "side": "BUY",
        "status": "FILLED",
        "decision_ts_ns": 1_789_048_800_000_000_000,
        "submit_ts_ns": 1_789_048_801_000_000_000,
        "last_fill_ts_ns": 1_789_135_200_000_000_000,
        "filled_qty": 1.0,
        "average_fill_price": 100.0,
        "fees": 0.01,
    }
    with pytest.raises(ValueError, match="fill is outside"):
        _publish(tmp_path, [record])
    with pytest.raises(ValueError, match="complete reconciliation"):
        publish_execution_feedback(
            tmp_path / "unreconciled.json",
            environment="paper",
            trading_date="2026-09-10",
            run_id="run",
            model_id="model",
            model_artifact_sha256="a" * 64,
            feature_manifest_sha256="b" * 64,
            reconciliation={"status": "pending"},
            records=[],
        )


def test_orders_report_rejects_conflicting_bindings_and_non_finite_fees() -> None:
    """
    Prediction identity conflicts and invalid fee values fail before publication.
    """
    rows = [
        {
            "client_order_id": "ORDER-1",
            "side": "BUY",
            "status": "FILLED",
            "filled_qty": 1,
            "avg_px": 100,
            "ts_init": 1_000,
            "ts_last": 2_000,
        },
        {
            "client_order_id": "ORDER-2",
            "side": "SELL",
            "status": "FILLED",
            "filled_qty": 1,
            "avg_px": 100,
            "ts_init": 1_100,
            "ts_last": 2_100,
        },
    ]
    binding = {
        "prediction_id": "prediction-1",
        "instrument_uid": "nvda-canonical",
        "decision_ts_ns": 900,
        "fees": 0.01,
        "last_fill_ts_ns": 2_000,
    }
    with pytest.raises(ValueError, match="inconsistent bound orders"):
        feedback_records_from_orders_report(rows, {"ORDER-1": binding, "ORDER-2": binding})
    bad_binding = {**binding, "fees": float("nan")}
    with pytest.raises(ValueError, match="finite numeric"):
        feedback_records_from_orders_report(rows[:1], {"ORDER-1": bad_binding})


def test_orders_report_requires_one_to_one_strategy_binding() -> None:
    """
    Duplicate rows and unused bindings cannot pass daily reconciliation.
    """
    row = {
        "client_order_id": "ORDER-1",
        "side": "BUY",
        "status": "FILLED",
        "filled_qty": 1,
        "avg_px": 100,
        "ts_init": 1_000,
        "ts_last": 2_000,
    }
    binding = {
        "prediction_id": "prediction-1",
        "instrument_uid": "nvda-canonical",
        "decision_ts_ns": 900,
        "fees": 0.01,
        "last_fill_ts_ns": 2_000,
    }
    with pytest.raises(ValueError, match="duplicates"):
        feedback_records_from_orders_report([row, row], {"ORDER-1": binding})
    with pytest.raises(ValueError, match="one-to-one"):
        feedback_records_from_orders_report(
            [row],
            {"ORDER-1": binding, "ORDER-2": binding},
        )


@pytest.mark.parametrize(
    ("fees", "quantity", "price"),
    [
        (Decimal("0.01"), Decimal(1), Decimal(100)),
        (Decimal("0.123456789123456789"), Decimal("1.000000001"), Decimal("100.000000001")),
    ],
)
def test_json_export_command_publishes_only_sanitized_records(
    tmp_path: Path,
    fees: Decimal,
    quantity: Decimal,
    price: Decimal,
) -> None:
    """
    Preserve exact JSON numbers while excluding ephemeral IDs from exported records.
    """
    trading_ns = 1_757_512_800_000_000_000
    request = {
        "environment": "paper",
        "trading_date": "2025-09-10",
        "run_id": "run",
        "model_id": "model",
        "model_artifact_sha256": "a" * 64,
        "feature_manifest_sha256": "b" * 64,
        "reconciliation": {"status": "complete"},
        "metrics": {"pnl_after_fees": 1.0},
        "bindings": {
            "ORDER-1": {
                "prediction_id": "prediction-1",
                "instrument_uid": "nvda-canonical",
                "decision_ts_ns": trading_ns,
                "fees": fees,
                "last_fill_ts_ns": trading_ns + 2,
            },
        },
    }
    orders = [
        {
            "client_order_id": "ORDER-1",
            "side": "BUY",
            "status": "FILLED",
            "filled_qty": quantity,
            "avg_px": price,
            "ts_init": trading_ns + 1,
            "ts_last": trading_ns + 2,
        },
    ]
    request_path = tmp_path / "request.json"
    orders_path = tmp_path / "orders.json"
    output_path = tmp_path / "feedback.json"
    request_path.write_text(simplejson.dumps(request, use_decimal=True))
    orders_path.write_text(simplejson.dumps(orders, use_decimal=True))
    receipt = export_feedback_from_json(request_path, orders_path, output_path)
    payload = output_path.read_text()
    assert receipt["records"] == 1
    assert "ORDER-1" not in payload
    assert "client_order_id" not in payload
    record = json.loads(payload, parse_float=Decimal)["records"][0]
    assert record["fees"] == fees
    assert record["filled_qty"] == quantity
    assert record["average_fill_price"] == price


@pytest.mark.parametrize("reverse", [False, True])
def test_fill_time_ignores_cancellation_and_unfilled_sibling(reverse: bool) -> None:
    """
    Use only authenticated fill events when reports also contain cancellations.
    """
    rows = [
        {
            "client_order_id": "filled",
            "side": "BUY",
            "status": "CANCELED",
            "filled_qty": "0.2",
            "avg_px": "100.1",
            "ts_init": 100,
            "ts_last": 500,
        },
        {
            "client_order_id": "empty",
            "side": "BUY",
            "status": "CANCELED",
            "filled_qty": "0",
            "avg_px": None,
            "ts_init": 300,
            "ts_last": 900,
        },
    ]
    binding = {
        "prediction_id": "prediction",
        "instrument_uid": "TEST.SIM",
        "decision_ts_ns": 90,
        "fees": "0",
    }
    bindings = {
        "filled": {**binding, "last_fill_ts_ns": 200},
        "empty": {**binding, "last_fill_ts_ns": None},
    }
    records = feedback_records_from_orders_report(rows[::-1] if reverse else rows, bindings)
    assert records[0]["last_fill_ts_ns"] == 200
    assert records[0]["filled_qty"] == Decimal("0.2")
    assert records[0]["average_fill_price"] == Decimal("100.1")


def test_filled_order_requires_bound_fill_timestamp() -> None:
    """
    Reject a filled native report without its ephemeral fill timestamp binding.
    """
    row = {
        "client_order_id": "filled",
        "side": "BUY",
        "status": "FILLED",
        "filled_qty": "1",
        "avg_px": "100",
        "ts_init": 100,
        "ts_last": 200,
    }
    binding = {
        "prediction_id": "prediction",
        "instrument_uid": "TEST.SIM",
        "decision_ts_ns": 90,
        "fees": "0",
    }
    with pytest.raises(TypeError, match="last_fill_ts_ns"):
        feedback_records_from_orders_report([row], {"filled": binding})


@pytest.mark.parametrize("side", ["buy", "Buy", None])
def test_report_requires_native_side(side: str | None) -> None:
    """
    Reject non-native or missing order-side spellings.
    """
    row = {
        "client_order_id": "empty",
        "side": side,
        "order_side": "BUY",
        "status": "CANCELED",
        "filled_qty": "0",
        "avg_px": None,
        "ts_init": 100,
    }
    binding = {
        "prediction_id": "prediction",
        "instrument_uid": "TEST.SIM",
        "decision_ts_ns": 90,
        "fees": "0",
    }
    with pytest.raises(ValueError, match="unsupported order side"):
        feedback_records_from_orders_report([row], {"empty": binding})


@pytest.mark.parametrize("bad", [Decimal("NaN"), Decimal("Infinity"), Decimal("-Infinity")])
def test_feedback_rejects_nonfinite_decimal_metrics(tmp_path: Path, bad: Decimal) -> None:
    """
    Exact decimal serialization must not admit non-finite JSON numbers.
    """
    with pytest.raises(ValueError, match="non-finite"):
        _publish(tmp_path, [{"metric": bad}])


def test_feedback_preserves_decimal_numbers_on_disk(tmp_path: Path) -> None:
    """
    Preserve quantities above binary precision and sub-cent fees as JSON numbers.
    """
    timestamp = 1_789_048_800_000_000_000
    quantity = Decimal(9007199254740993)
    price = Decimal("100.000000001")
    fees = Decimal("0.123456789123456789")
    row = {
        "client_order_id": "ORDER-1",
        "side": "BUY",
        "status": "FILLED",
        "filled_qty": quantity,
        "avg_px": price,
        "ts_init": timestamp,
    }
    binding = {
        "prediction_id": "prediction-1",
        "instrument_uid": "test-canonical",
        "decision_ts_ns": timestamp,
        "fees": fees,
        "last_fill_ts_ns": timestamp,
    }
    records = feedback_records_from_orders_report([row], {"ORDER-1": binding})
    assert records[0]["filled_qty"] == quantity
    assert records[0]["average_fill_price"] == price
    assert records[0]["fees"] == fees
    _publish(tmp_path, records)
    payload = json.loads((tmp_path / "feedback.json").read_text(), parse_float=Decimal)
    actual = payload["records"][0]
    assert actual["filled_qty"] == quantity
    assert actual["average_fill_price"] == price
    assert actual["fees"] == fees
    assert not isinstance(actual["filled_qty"], str)
