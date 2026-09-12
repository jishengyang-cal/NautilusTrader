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
Tests for audited strict-L2 candidate signal loading.
"""

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
    frame = pd.DataFrame(
        {
            "ts_recv": [1_000, 2_000, 3_000],
            "instrument": ["NVDA", "TSLA", "NVDA"],
            "delta_mid_ticks_1000ms": [1.5, -2.0, 0.25],
            "p_down_1000ms": [0.1, 0.8, 0.2],
            "p_flat_1000ms": [0.1, 0.1, 0.3],
            "p_up_1000ms": [0.8, 0.1, 0.5],
        },
    )

    if extra_column is not None:
        frame[extra_column] = [1, 2, 3]
    # Match the producer's indexed v2 output, including withheld outcome columns
    targets = [f"delta_mid_ticks_{h}ms" for h in (250, 1_000, 5_000, 15_000, 60_000)]
    targets += [
        f"{name}_{h}ms"
        for h in (1_000, 5_000, 15_000, 60_000)
        for name in ("mfe_long_ticks", "mae_long_ticks")
    ]

    for target in targets:
        if target not in frame:
            frame[target] = 1.0
        frame[f"actual_{target}"] = [99.0, -99.0, 99.0]
    for horizon in (250, 5_000, 15_000, 60_000):
        for direction, probability in (("down", 0.1), ("flat", 0.1), ("up", 0.8)):
            frame[f"p_{direction}_{horizon}ms"] = probability
    for context in (
        "session_progress",
        "rolling_volatility",
        "liquidity_distance_ticks",
        "max_displayed_depth",
    ):
        frame[context] = 0.5
    frame["history_retained_fraction"] = 0.5
    frame["history_off_lattice_fraction"] = 0.25
    frame["history_out_of_radius_fraction"] = 0.25
    frame = frame.set_index(["ts_recv", "instrument"])
    predictions = candidate / "predictions.parquet"
    frame.to_parquet(predictions)
    bundle = {
        "schema_version": "lob-prediction-bundle/v2",
        "run_id": run_id,
        "evaluation_segment": "development_test",
        "spec_sha256": "a" * 64,
        "readiness_sha256": "b" * 64,
        "implementation_sha256": "c" * 64,
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
        "evaluation_segment": "development_test",
        "source_spec_sha256": bundle["spec_sha256"],
        "readiness_sha256": bundle["readiness_sha256"],
        "implementation_sha256": bundle["implementation_sha256"],
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
    """
    An audited bundle yields ordered prediction state without labels.
    """
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
    assert not hasattr(signals[0], "actual_delta_mid_ticks_1000ms")
    assert not hasattr(signals[0], "history_retained_fraction")


def test_load_candidate_signals_rejects_artifact_changed_after_audit(tmp_path: Path) -> None:
    """
    A model mutation after independent audit fails closed.
    """
    receipt, model = _candidate(tmp_path)
    model.write_bytes(b"changed after audit")

    with pytest.raises(ValueError, match="model digest"):
        load_candidate_signals(receipt, horizon_ms=1_000)


def test_load_candidate_signals_rejects_order_identity_columns(tmp_path: Path) -> None:
    """
    Execution identity cannot cross into the strict-L2 signal boundary.
    """
    receipt, _ = _candidate(tmp_path, extra_column="client_order_id")

    with pytest.raises(ValueError, match="forbidden execution identity"):
        load_candidate_signals(receipt, horizon_ms=1_000)


def test_load_candidate_signals_requires_a_win_at_the_requested_horizon(tmp_path: Path) -> None:
    """
    An unrelated horizon win cannot authorize the precommitted replay horizon.
    """
    receipt, _ = _candidate(tmp_path)
    payload = json.loads(receipt.read_text())
    payload["baseline_screen"]["overall"]["horizons"]["1000ms"]["joint_baseline_win"] = False
    receipt.write_text(json.dumps(payload), encoding="utf-8")

    with pytest.raises(ValueError, match="requested-horizon"):
        load_candidate_signals(receipt, horizon_ms=1_000)


def _update_bundle(receipt_path: Path, changes: dict[str, object]) -> None:
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    bundle_path = Path(receipt["candidate_path"]) / "prediction-bundle.json"
    bundle = json.loads(bundle_path.read_text(encoding="utf-8"))
    bundle.update(changes)
    bundle_path.write_text(json.dumps(bundle), encoding="utf-8")
    receipt["prediction_bundle_sha256"] = _sha256(bundle_path)
    receipt_path.write_text(json.dumps(receipt), encoding="utf-8")


@pytest.mark.parametrize("version", ["lob-prediction-bundle/v1", "lob-prediction-bundle/v3"])
def test_load_candidate_signals_rejects_legacy_and_unknown_versions(
    tmp_path: Path,
    version: str,
) -> None:
    """
    Reject legacy and future schemas even when their file hashes match.
    """
    receipt, _ = _candidate(tmp_path)
    _update_bundle(receipt, {"schema_version": version})
    with pytest.raises(ValueError, match="identity mismatch"):
        load_candidate_signals(receipt, horizon_ms=1_000)


@pytest.mark.parametrize("segment", ["sealed_final", "train", "valid", None])
@pytest.mark.parametrize("location", ["audit", "bundle", "both"])
def test_load_candidate_signals_rejects_non_development_segments(
    tmp_path: Path,
    segment: str | None,
    location: str,
) -> None:
    """
    Prevent sealed or unspecified evaluation results from entering daily replay.
    """
    receipt, _ = _candidate(tmp_path)
    if location in {"bundle", "both"}:
        _update_bundle(receipt, {"evaluation_segment": segment})
    if location in {"audit", "both"}:
        payload = json.loads(receipt.read_text())
        payload["evaluation_segment"] = segment
        receipt.write_text(json.dumps(payload))
    with pytest.raises(ValueError, match="evaluation segment"):
        load_candidate_signals(receipt, horizon_ms=1_000)


@pytest.mark.parametrize("field", ["spec_sha256", "readiness_sha256", "implementation_sha256"])
def test_load_candidate_signals_rejects_mismatched_provenance(
    tmp_path: Path,
    field: str,
) -> None:
    """
    Bind the source spec, readiness and implementation to the complete audit.
    """
    receipt, _ = _candidate(tmp_path)
    _update_bundle(receipt, {field: "d" * 64})
    with pytest.raises(ValueError, match=f"{field} differs"):
        load_candidate_signals(receipt, horizon_ms=1_000)


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("artifact_valid", False),
        ("schema_version", "lob-candidate-audit/v2"),
        ("strict_l2_only", False),
        ("source_spec_sha256", "invalid"),
    ],
)
def test_load_candidate_signals_rejects_invalid_audit(
    tmp_path: Path,
    field: str,
    value: object,
) -> None:
    """
    Require a valid strict-L2 audit with a well-formed source reference.
    """
    receipt, _ = _candidate(tmp_path)
    payload = json.loads(receipt.read_text())
    payload[field] = value
    receipt.write_text(json.dumps(payload))
    with pytest.raises(ValueError, match=r"audit|strict L2|SHA-256"):
        load_candidate_signals(receipt, horizon_ms=1_000)


@pytest.mark.parametrize("coverage", [float("nan"), float("inf"), -0.1, 1.1, 0.1])
def test_load_candidate_signals_rejects_invalid_v2_coverage(
    tmp_path: Path,
    coverage: float,
) -> None:
    """
    Reject non-finite, out-of-range and non-partitioning v2 history coverage.
    """
    receipt, _ = _candidate(tmp_path)
    payload = json.loads(receipt.read_text())
    predictions = Path(payload["candidate_path"]) / "predictions.parquet"
    frame = pd.read_parquet(predictions)
    frame["history_retained_fraction"] = coverage
    frame.to_parquet(predictions)
    payload["predictions_sha256"] = _sha256(predictions)
    receipt.write_text(json.dumps(payload))
    _update_bundle(receipt, {"prediction_sha256": payload["predictions_sha256"]})
    with pytest.raises(ValueError, match="coverage"):
        load_candidate_signals(receipt, horizon_ms=1_000)


def test_load_candidate_signals_excludes_future_predictions(tmp_path: Path) -> None:
    """
    Respect the exclusive end of the available prediction interval.
    """
    receipt, _ = _candidate(tmp_path)
    signals = load_candidate_signals(receipt, horizon_ms=1_000, start_ns=1_000, end_ns=2_000)
    assert [signal.ts_recv_ns for signal in signals] == [1_000]


@pytest.mark.parametrize("missing_column", [False, True])
def test_load_candidate_signals_distinguishes_empty_and_missing_history_coverage(
    tmp_path: Path,
    missing_column: bool,
) -> None:
    """
    Permit absent historical activity only when v2 explicitly records zero coverage.
    """
    receipt, _ = _candidate(tmp_path)
    payload = json.loads(receipt.read_text())
    predictions = Path(payload["candidate_path"]) / "predictions.parquet"
    frame = pd.read_parquet(predictions)
    coverage = [
        "history_retained_fraction",
        "history_off_lattice_fraction",
        "history_out_of_radius_fraction",
    ]
    frame[coverage] = 0.0
    if missing_column:
        frame = frame.drop(columns=coverage[0])
    frame.to_parquet(predictions)
    payload["predictions_sha256"] = _sha256(predictions)
    receipt.write_text(json.dumps(payload))
    _update_bundle(receipt, {"prediction_sha256": payload["predictions_sha256"]})

    if missing_column:
        with pytest.raises(ValueError, match="missing history coverage"):
            load_candidate_signals(receipt, horizon_ms=1_000)
    else:
        assert len(load_candidate_signals(receipt, horizon_ms=1_000)) == 3


def test_load_candidate_signals_accepts_consistently_audited_validation(tmp_path: Path) -> None:
    """
    Permit validation replay with matching audit and bundle segment declarations.
    """
    receipt, _ = _candidate(tmp_path)
    _update_bundle(receipt, {"evaluation_segment": "validation"})
    payload = json.loads(receipt.read_text())
    payload["evaluation_segment"] = "validation"
    receipt.write_text(json.dumps(payload))
    assert len(load_candidate_signals(receipt, horizon_ms=1_000)) == 3


def test_load_candidate_signals_rejects_mixed_development_segments(tmp_path: Path) -> None:
    """
    Reject a bundle whose development segment differs from the complete audit.
    """
    receipt, _ = _candidate(tmp_path)
    _update_bundle(receipt, {"evaluation_segment": "validation"})
    with pytest.raises(ValueError, match="evaluation segment differs"):
        load_candidate_signals(receipt, horizon_ms=1_000)


@pytest.mark.parametrize("artifact", ["prediction-bundle.json", "predictions.parquet"])
def test_load_candidate_signals_rejects_changed_referenced_artifact(
    tmp_path: Path,
    artifact: str,
) -> None:
    """
    Detect mutation of each immutable prediction reference after its audit.
    """
    receipt, _ = _candidate(tmp_path)
    payload = json.loads(receipt.read_text())
    path = Path(payload["candidate_path"]) / artifact
    path.write_bytes(path.read_bytes() + b" ")
    with pytest.raises(ValueError, match="digest"):
        load_candidate_signals(receipt, horizon_ms=1_000)
