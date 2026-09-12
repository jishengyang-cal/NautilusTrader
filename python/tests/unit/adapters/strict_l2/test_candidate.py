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

"""Tests for audited strict-L2 candidate signal loading."""

import hashlib
import json
from pathlib import Path

import pandas as pd
import pytest

from nautilus_trader.adapters.strict_l2.candidate import load_candidate_signals


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _candidate(tmp_path: Path, *, extra_column: str | None = None) -> tuple[Path, Path]:
    run_id = "0123456789abcdefabcd"
    candidate = tmp_path / run_id
    candidate.mkdir()
    model = candidate / "model.pt"
    model.write_bytes(b"sealed model")
    frame = pd.DataFrame({
        "ts_recv": [1_000, 2_000, 3_000],
        "instrument": ["NVDA", "TSLA", "NVDA"],
        "delta_mid_ticks_1000ms": [1.5, -2.0, 0.25],
        "p_down_1000ms": [0.1, 0.8, 0.2],
        "p_flat_1000ms": [0.1, 0.1, 0.3],
        "p_up_1000ms": [0.8, 0.1, 0.5],
    })
    if extra_column is not None:
        frame[extra_column] = [1, 2, 3]
    predictions = candidate / "predictions.parquet"
    frame.to_parquet(predictions, index=False)
    bundle = {
        "schema_version": "lob-prediction-bundle/v1",
        "run_id": run_id,
        "rows": len(frame),
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
        "model_sha256": _sha256(model),
        "prediction_bundle_sha256": _sha256(bundle_path),
        "predictions_sha256": _sha256(predictions),
        "symbols": ["NVDA", "TSLA"],
        "baseline_screen": {
            "screening_effective": True,
            "required_symbols": 2,
            "overall": {"horizons": {"1000ms": {"joint_baseline_win": True}}},
            "symbols": {
                symbol: {"horizons": {"1000ms": {"joint_baseline_win": True}}}
                for symbol in ("NVDA", "TSLA")
            },
        },
    }
    receipt_path = tmp_path / "candidate-audit.json"
    receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
    return receipt_path, model


def test_load_candidate_signals_exposes_only_causal_prediction_fields(tmp_path: Path) -> None:
    """An audited bundle yields ordered prediction state without labels."""
    receipt, _ = _candidate(tmp_path)

    signals = load_candidate_signals(receipt, horizon_ms=1_000)

    assert [(signal.ts_recv_ns, signal.instrument) for signal in signals] == [
        (1_000, "NVDA"),
        (2_000, "TSLA"),
        (3_000, "NVDA"),
    ]
    assert signals[0].expected_delta_ticks == 1.5
    assert signals[0].p_up == 0.8
    assert len({signal.prediction_id for signal in signals}) == 3


def test_load_candidate_signals_rejects_artifact_changed_after_audit(tmp_path: Path) -> None:
    """A model mutation after independent audit fails closed."""
    receipt, model = _candidate(tmp_path)
    model.write_bytes(b"changed after audit")

    with pytest.raises(ValueError, match="model digest"):
        load_candidate_signals(receipt, horizon_ms=1_000)


def test_load_candidate_signals_rejects_order_identity_columns(tmp_path: Path) -> None:
    """Execution identity cannot cross into the strict-L2 signal boundary."""
    receipt, _ = _candidate(tmp_path, extra_column="client_order_id")

    with pytest.raises(ValueError, match="forbidden execution identity"):
        load_candidate_signals(receipt, horizon_ms=1_000)


def test_load_candidate_signals_requires_a_win_at_the_requested_horizon(tmp_path: Path) -> None:
    """An unrelated horizon win cannot authorize the precommitted replay horizon."""
    receipt, _ = _candidate(tmp_path)
    payload = json.loads(receipt.read_text())
    payload["baseline_screen"]["overall"]["horizons"]["1000ms"][
        "joint_baseline_win"
    ] = False
    receipt.write_text(json.dumps(payload), encoding="utf-8")

    with pytest.raises(ValueError, match="requested-horizon"):
        load_candidate_signals(receipt, horizon_ms=1_000)
