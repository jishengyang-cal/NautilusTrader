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
Load audited strict-L2 research predictions for deterministic replay.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
from dataclasses import dataclass
from pathlib import Path
from pathlib import PurePosixPath

import pyarrow as pa
import pyarrow.parquet as pq


AUDIT_SCHEMA = "lob-candidate-audit/v1"
PREDICTION_SCHEMA = "lob-prediction-bundle/v2"
SUPPORTED_HORIZONS_MS = frozenset({250, 1_000, 5_000, 15_000, 60_000})
SHA256_HEX_LENGTH = 64
FORBIDDEN_FIELDS = frozenset(
    {
        "accountid",
        "brokerorderid",
        "clientorderid",
        "orderid",
        "venueorderid",
        "mpid",
    },
)


@dataclass(frozen=True, slots=True)
class CandidateSignal:
    """
    One prediction released to a replay strategy at its receive timestamp.
    """

    prediction_id: str
    run_id: str
    instrument: str
    ts_recv_ns: int
    horizon_ms: int
    expected_delta_ticks: float
    p_down: float
    p_flat: float
    p_up: float


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _validate_digest(value: object, field: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != SHA256_HEX_LENGTH
        or any(char not in "0123456789abcdef" for char in value)
    ):
        raise ValueError(f"{field} must be a lowercase SHA-256")
    return value


def _load_json(value: bytes, name: str, expected_type: type) -> object:
    loaded = json.loads(value)
    if not isinstance(loaded, expected_type):
        raise TypeError(f"{name} has an invalid top-level type")
    return loaded


def _bound_file(root: Path, name: object) -> Path:
    pure = PurePosixPath(str(name))
    if pure.is_absolute() or len(pure.parts) != 1 or pure.parts[0] in {"", ".", ".."}:
        raise ValueError("prediction file must be a direct relative candidate artifact")
    path = (root / pure.name).resolve(strict=True)
    path.relative_to(root)
    if not path.is_file():
        raise ValueError("prediction artifact is not a regular file")
    return path


def _prediction_id(run_id: str, instrument: str, ts_recv_ns: int, horizon_ms: int) -> str:
    raw = f"{run_id}|{instrument}|{ts_recv_ns}|{horizon_ms}".encode()
    return hashlib.sha256(raw).hexdigest()[:24]


def _require_horizon_baseline_win(screening: object, horizon_ms: int) -> None:
    if not isinstance(screening, dict):
        raise TypeError("candidate audit has no baseline screen")
    horizon = f"{horizon_ms}ms"
    overall = screening.get("overall")
    symbols = screening.get("symbols")
    required = screening.get("required_symbols")
    if (
        screening.get("screening_effective") is not True
        or not isinstance(overall, dict)
        or overall.get("horizons", {}).get(horizon, {}).get("joint_baseline_win") is not True
        or not isinstance(symbols, dict)
        or isinstance(required, bool)
        or not isinstance(required, int)
        or required < 1
    ):
        raise ValueError("candidate did not pass the requested-horizon predictive baseline screen")
    wins = sum(
        isinstance(block, dict)
        and block.get("horizons", {}).get(horizon, {}).get("joint_baseline_win") is True
        for block in symbols.values()
    )

    if wins < required:
        raise ValueError("candidate did not generalize at the requested horizon")


def _load_candidate_signals(  # noqa: C901, PLR0912, PLR0913, PLR0915
    receipt_path: Path,
    receipt_bytes: bytes,
    *,
    horizon_ms: int,
    instruments: set[str] | frozenset[str] | None = None,
    start_ns: int | None = None,
    end_ns: int | None = None,
    batch_size: int = 65_536,
) -> tuple[CandidateSignal, ...]:
    """
    Load audited v2 offline-development signals, never sealed-final evaluation results.
    """
    if horizon_ms not in SUPPORTED_HORIZONS_MS:
        raise ValueError("unsupported prediction horizon")
    if batch_size < 1:
        raise ValueError("batch_size must be positive")
    if any(
        value is not None and (isinstance(value, bool) or not isinstance(value, int) or value < 0)
        for value in (start_ns, end_ns)
    ):
        raise ValueError("signal time bounds must be non-negative Unix nanoseconds")
    if start_ns is not None and end_ns is not None and start_ns >= end_ns:
        raise ValueError("signal time bounds must define a positive interval")
    receipt = _load_json(receipt_bytes, receipt_path.name, dict)
    if receipt.get("schema_version") != AUDIT_SCHEMA or receipt.get("artifact_valid") is not True:
        raise ValueError("a valid strict-L2 candidate audit receipt is required")
    if receipt.get("strict_l2_only") is not True:
        raise ValueError("candidate audit does not enforce strict L2")
    if receipt.get("evaluation_segment") not in {"validation", "development_test"}:
        raise ValueError("candidate replay requires an offline-development evaluation segment")
    audited_symbols = receipt.get("symbols")
    if (
        not isinstance(audited_symbols, list)
        or not audited_symbols
        or len(audited_symbols) != len(set(audited_symbols))
        or not all(isinstance(symbol, str) and symbol for symbol in audited_symbols)
    ):
        raise ValueError("candidate audit has an invalid symbol set")
    screening = receipt.get("baseline_screen")
    screened_symbols = screening.get("symbols") if isinstance(screening, dict) else None
    if (
        not isinstance(screening, dict)
        or not isinstance(screened_symbols, dict)
        or set(screened_symbols) != set(audited_symbols)
    ):
        raise ValueError("candidate baseline screen differs from its audited symbols")
    _require_horizon_baseline_win(screening, horizon_ms)
    candidate_source = Path(str(receipt.get("candidate_path"))).expanduser()
    if candidate_source.is_symlink():
        raise ValueError("candidate path must not be a symbolic link")
    candidate = candidate_source.resolve(strict=True)
    if not candidate.is_dir():
        raise ValueError("candidate path must be a real directory")
    run_id = receipt.get("run_id")
    if not isinstance(run_id, str) or candidate.name != run_id:
        raise ValueError("candidate audit run identity mismatch")
    model = candidate / "model.pt"
    bundle_path = candidate / "prediction-bundle.json"
    if not model.is_file() or not bundle_path.is_file():
        raise ValueError("candidate model or prediction bundle is missing")
    model_bytes = model.read_bytes()
    if _sha256_bytes(model_bytes) != _validate_digest(receipt.get("model_sha256"), "model_sha256"):
        raise ValueError("candidate model digest differs from its audit receipt")
    bundle_bytes = bundle_path.read_bytes()
    if _sha256_bytes(bundle_bytes) != _validate_digest(
        receipt.get("prediction_bundle_sha256"),
        "prediction_bundle_sha256",
    ):
        raise ValueError("prediction bundle digest differs from its audit receipt")
    bundle = _load_json(bundle_bytes, bundle_path.name, dict)
    if bundle.get("schema_version") != PREDICTION_SCHEMA or bundle.get("run_id") != run_id:
        raise ValueError("prediction bundle identity mismatch")
    if bundle.get("evaluation_segment") != receipt["evaluation_segment"]:
        raise ValueError("prediction bundle evaluation segment differs from its audit")
    for bundle_field, audit_field in (
        ("spec_sha256", "source_spec_sha256"),
        ("readiness_sha256", "readiness_sha256"),
        ("implementation_sha256", "implementation_sha256"),
    ):
        digest = _validate_digest(receipt.get(audit_field), audit_field)
        if bundle.get(bundle_field) != digest:
            raise ValueError(f"prediction bundle {bundle_field} differs from its audit")
    prediction_path = _bound_file(candidate, bundle.get("prediction_file"))
    prediction_sha256 = _validate_digest(
        receipt.get("predictions_sha256"),
        "predictions_sha256",
    )

    prediction_bytes = prediction_path.read_bytes()

    if (
        bundle.get("prediction_sha256") != prediction_sha256
        or _sha256_bytes(
            prediction_bytes,
        )
        != prediction_sha256
    ):
        raise ValueError("prediction artifact digest mismatch")

    expected = {
        "ts_recv",
        "instrument",
        f"delta_mid_ticks_{horizon_ms}ms",
        f"p_down_{horizon_ms}ms",
        f"p_flat_{horizon_ms}ms",
        f"p_up_{horizon_ms}ms",
    }
    parquet = pq.ParquetFile(pa.BufferReader(prediction_bytes))
    names = set(parquet.schema_arrow.names)
    if not expected <= names:
        raise ValueError("prediction artifact is missing replay signal columns")
    coverage_columns = {
        "history_retained_fraction",
        "history_off_lattice_fraction",
        "history_out_of_radius_fraction",
    }

    if not coverage_columns <= names:
        raise ValueError("v2 prediction artifact is missing history coverage columns")
    normalized_names = {re.sub(r"[^a-z0-9]", "", name.lower()) for name in names}
    if normalized_names & FORBIDDEN_FIELDS:
        raise ValueError("prediction artifact exposes forbidden execution identity")
    requested = set(instruments) if instruments is not None else None
    if requested is not None and (
        not requested or not all(isinstance(x, str) and x for x in requested)
    ):
        raise ValueError("instruments must contain non-empty symbol strings")
    if requested is not None and not requested <= set(audited_symbols):
        raise ValueError("requested instruments are outside the audited candidate")
    columns = sorted(expected | coverage_columns)
    signals = []
    seen: set[tuple[str, int]] = set()
    artifact_symbols = set()

    for batch in parquet.iter_batches(batch_size=batch_size, columns=columns):
        values = batch.to_pydict()
        for index in range(batch.num_rows):
            coverage = [values[name][index] for name in coverage_columns]

            if any(
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(value)
                or not 0 <= value <= 1
                for value in coverage
            ) or not (
                math.isclose(sum(coverage), 0.0, abs_tol=1e-8)
                or math.isclose(sum(coverage), 1.0, rel_tol=1e-5, abs_tol=1e-8)
            ):
                raise ValueError("v2 prediction history coverage violates its partition contract")
            instrument = values["instrument"][index]
            ts_recv_ns = values["ts_recv"][index]

            if not isinstance(instrument, str) or not instrument:
                raise ValueError("prediction instrument must be a non-empty string")
            artifact_symbols.add(instrument)
            if requested is not None and instrument not in requested:
                continue
            if isinstance(ts_recv_ns, bool) or not isinstance(ts_recv_ns, int) or ts_recv_ns < 0:
                raise ValueError("prediction timestamp must be Unix nanoseconds")
            if start_ns is not None and ts_recv_ns < start_ns:
                continue
            if end_ns is not None and ts_recv_ns >= end_ns:
                continue
            identity = (instrument, ts_recv_ns)
            if identity in seen:
                raise ValueError("prediction artifact contains duplicate symbol timestamps")
            seen.add(identity)
            numeric = [
                values[f"delta_mid_ticks_{horizon_ms}ms"][index],
                values[f"p_down_{horizon_ms}ms"][index],
                values[f"p_flat_{horizon_ms}ms"][index],
                values[f"p_up_{horizon_ms}ms"][index],
            ]

            if any(
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(value)
                for value in numeric
            ):
                raise ValueError("prediction signal contains non-finite numeric data")
            expected_delta, p_down, p_flat, p_up = map(float, numeric)
            if any(value < 0 or value > 1 for value in (p_down, p_flat, p_up)) or not math.isclose(
                p_down + p_flat + p_up,
                1.0,
                abs_tol=1e-5,
            ):
                raise ValueError("prediction probabilities are invalid")
            signals.append(
                CandidateSignal(
                    prediction_id=_prediction_id(run_id, instrument, ts_recv_ns, horizon_ms),
                    run_id=run_id,
                    instrument=instrument,
                    ts_recv_ns=ts_recv_ns,
                    horizon_ms=horizon_ms,
                    expected_delta_ticks=expected_delta,
                    p_down=p_down,
                    p_flat=p_flat,
                    p_up=p_up,
                ),
            )
    if artifact_symbols != set(audited_symbols):
        raise ValueError("prediction artifact symbols differ from the candidate audit")
    if not signals:
        raise ValueError("candidate contains no signals for the requested instruments")
    signals.sort(key=lambda signal: (signal.ts_recv_ns, signal.instrument))
    if (
        len(signals) != bundle.get("rows")
        and requested is None
        and start_ns is None
        and end_ns is None
    ):
        raise ValueError("prediction row count differs from its bundle")
    return tuple(signals)


def load_candidate_signals(  # noqa: PLR0913
    audit_receipt_path: str | Path,
    *,
    horizon_ms: int,
    instruments: set[str] | frozenset[str] | None = None,
    start_ns: int | None = None,
    end_ns: int | None = None,
    batch_size: int = 65_536,
) -> tuple[CandidateSignal, ...]:
    """
    Load audited signals from one receipt and artifact byte snapshot.
    """
    receipt_path = Path(audit_receipt_path).expanduser().resolve(strict=True)
    return load_candidate_signals_snapshot(
        receipt_path,
        receipt_path.read_bytes(),
        horizon_ms=horizon_ms,
        instruments=instruments,
        start_ns=start_ns,
        end_ns=end_ns,
        batch_size=batch_size,
    )


def load_candidate_signals_snapshot(  # noqa: PLR0913
    receipt_path: Path,
    receipt_bytes: bytes,
    *,
    horizon_ms: int,
    instruments: set[str] | frozenset[str] | None = None,
    start_ns: int | None = None,
    end_ns: int | None = None,
    batch_size: int = 65_536,
) -> tuple[CandidateSignal, ...]:
    """
    Load audited signals from caller-bound receipt bytes.
    """
    return _load_candidate_signals(
        receipt_path,
        receipt_bytes,
        horizon_ms=horizon_ms,
        instruments=instruments,
        start_ns=start_ns,
        end_ns=end_ns,
        batch_size=batch_size,
    )
