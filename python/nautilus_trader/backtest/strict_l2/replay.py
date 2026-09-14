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
Run one immutable trading day of audited candidate signals against L2 MBP catalogs.
"""

from __future__ import annotations

import hashlib
import json
import math
import shutil
from collections.abc import Mapping
from collections.abc import Sequence
from decimal import Decimal
from pathlib import Path
from pathlib import PurePosixPath
from typing import Any

import pandas as pd

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.backtest.strict_l2.candidate import CandidateSignal
from nautilus_trader.backtest.strict_l2.candidate import load_candidate_signals_snapshot
from nautilus_trader.backtest.strict_l2.feedback import feedback_records_from_orders_report
from nautilus_trader.backtest.strict_l2.feedback import publish_execution_feedback
from nautilus_trader.backtest.strict_l2.strategy import CandidateReplayConfig
from nautilus_trader.backtest.strict_l2.strategy import CandidateReplayStrategy
from nautilus_trader.execution import FeeModel
from nautilus_trader.execution import StaticLatencyModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.persistence import ParquetDataCatalog


class _PerShareFeeModel(FeeModel):
    def __init__(self, rate: Decimal) -> None:
        super().__init__()
        self._rate = rate

    def get_commission(
        self,
        _order: object,
        fill_quantity: Quantity,
        _fill_px: Price,
        _instrument: object,
    ) -> Money:
        """
        Calculate the USD fee before rounding to currency precision.
        """
        return Money.from_decimal(
            self._rate * fill_quantity.as_decimal(),
            Currency.from_str("USD"),
        )


REQUEST_SCHEMA = "strict-l2-candidate-replay-request/v2"
RESULT_SCHEMA = "strict-l2-candidate-replay-result/v2"
SHA256_HEX_LENGTH = 64
REQUEST_FIELDS = {
    "schema_version",
    "audit_receipt_path",
    "audit_receipt_sha256",
    "trading_date",
    "source_manifest_path",
    "catalogs",
    "horizon_ms",
    "trade_size",
    "min_abs_delta_ticks",
    "min_direction_probability",
    "cooldown_ms",
    "max_signal_lag_ms",
    "starting_balances",
    "fee_per_share_usd",
    "order_insert_latency_ns",
}


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _canonical_sha256(value: object) -> str:
    rendered = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(rendered).hexdigest()


def _json_value(value: object) -> object:
    if isinstance(value, Mapping):
        return {str(key): _json_value(child) for key, child in value.items()}
    if isinstance(value, Sequence) and not isinstance(value, (str, bytes, bytearray)):
        return [_json_value(child) for child in value]
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, (str, int, bool)) or value is None:
        return value
    return str(value)


def _load_request(path: str | Path) -> tuple[dict[str, Any], str]:
    source = Path(path).expanduser().resolve(strict=True)
    source_bytes = source.read_bytes()
    value = json.loads(source_bytes)
    if not isinstance(value, dict) or set(value) != REQUEST_FIELDS:
        raise ValueError("candidate replay request fields do not match the versioned contract")
    if value["schema_version"] != REQUEST_SCHEMA:
        raise ValueError("candidate replay request schema mismatch")
    catalogs = value["catalogs"]
    if (
        not isinstance(catalogs, list)
        or not catalogs
        or any(not isinstance(item, dict) for item in catalogs)
    ):
        raise TypeError("candidate replay catalogs must be a non-empty object list")
    if any(
        set(item) != {"symbol", "catalog_path", "instrument_id", "catalog_receipt_sha256"}
        for item in catalogs
    ):
        raise ValueError("candidate replay catalog binding fields are invalid")
    balances = value["starting_balances"]
    if (
        not isinstance(balances, list)
        or not balances
        or not all(isinstance(item, str) and item for item in balances)
    ):
        raise ValueError("candidate replay requires starting balance strings")
    fee = value["fee_per_share_usd"]
    if not isinstance(fee, str):
        raise TypeError("candidate replay fee_per_share_usd must be a decimal string")
    try:
        fee_decimal = Decimal(fee)
    except ArithmeticError as e:
        raise ValueError("candidate replay fee_per_share_usd is invalid") from e
    if not fee_decimal.is_finite() or fee_decimal < 0:
        raise ValueError("candidate replay fee_per_share_usd must be finite and non-negative")
    latency = value["order_insert_latency_ns"]
    if isinstance(latency, bool) or not isinstance(latency, int) or latency < 0:
        raise ValueError("candidate replay order_insert_latency_ns must be non-negative integer ns")
    return value, _sha256_bytes(source_bytes)


def _validate_source_manifest(
    path: Path,
    *,
    trading_date: str,
    symbols: set[str],
) -> tuple[str, str]:
    manifest_bytes = path.read_bytes()
    value = json.loads(manifest_bytes)
    if (
        not isinstance(value, dict)
        or value.get("schema_version") != "research/published-dataset-manifest-v1"
        or value.get("dataset_kind") != "strict-l2-mbp"
        or value.get("contracts", {}).get("l2") != "strict-l2-v1"
        or value.get("point_in_time", {}).get("effective_at") != trading_date
    ):
        raise ValueError("replay source manifest is not the requested strict-L2 trading day")
    files = value.get("files")
    if not isinstance(files, list):
        raise TypeError("replay source manifest has no artifact inventory")
    manifest_symbols = {
        item.get("symbol")
        for item in files
        if isinstance(item, dict) and item.get("role") == "l2_deltas"
    }

    if not symbols <= manifest_symbols:
        raise ValueError("replay catalogs are not present in the source manifest")
    metadata_entries = [
        item for item in files if isinstance(item, dict) and item.get("role") == "symbol_metadata"
    ]

    if len(metadata_entries) != 1:
        raise ValueError("replay source manifest must bind one symbol metadata artifact")
    entry = metadata_entries[0]
    pure = PurePosixPath(str(entry.get("path")))
    if pure.is_absolute() or not pure.parts or any(part in {"", ".", ".."} for part in pure.parts):
        raise ValueError("source symbol metadata path must be normalized and relative")
    metadata_path = (path.parent / Path(*pure.parts)).resolve(strict=True)
    metadata_path.relative_to(path.parent)
    metadata_bytes = metadata_path.read_bytes()
    metadata_sha256 = _sha256_bytes(metadata_bytes)
    if len(metadata_bytes) != entry.get("size_bytes") or metadata_sha256 != entry.get(
        "sha256",
    ):
        raise ValueError("source symbol metadata failed digest verification")
    return _sha256_bytes(manifest_bytes), metadata_sha256


def _load_catalog_bindings(
    values: list[dict[str, Any]],
    source_manifest_sha256: str,
    symbol_metadata_sha256: str,
    snapshot_root: Path,
) -> tuple[
    list[BacktestDataConfig],
    list[tuple[str, InstrumentId]],
    str,
    int,
    list[dict[str, Any]],
]:
    data_configs = []
    bindings = []
    venues = set()
    symbols = set()
    instrument_ids = set()
    first_timestamps = []
    receipt_bindings = []

    for index, value in enumerate(values):
        symbol = value["symbol"]
        if not isinstance(symbol, str) or not symbol or symbol in symbols:
            raise ValueError("catalog symbols must be non-empty and unique")
        symbols.add(symbol)
        instrument_id = InstrumentId.from_str(value["instrument_id"])
        if instrument_id in instrument_ids:
            raise ValueError("catalog instrument IDs must be unique")
        instrument_ids.add(instrument_id)
        venues.add(str(instrument_id.venue))
        catalog_source = Path(value["catalog_path"]).expanduser()
        if catalog_source.is_symlink():
            raise ValueError("catalog path must not be a symbolic link")
        catalog_path = catalog_source.resolve(strict=True)
        if not catalog_path.is_dir():
            raise ValueError("catalog path must be a real directory")
        snapshot_catalog_path = snapshot_root / str(index)
        snapshot_catalog_path.mkdir()
        receipt, receipt_sha256 = _verify_catalog_receipt(
            catalog_path,
            snapshot_path=snapshot_catalog_path,
            symbol=symbol,
            instrument_id=instrument_id,
            expected_receipt_sha256=value["catalog_receipt_sha256"],
            source_manifest_sha256=source_manifest_sha256,
            symbol_metadata_sha256=symbol_metadata_sha256,
        )
        receipt_bindings.append(
            {
                "symbol": symbol,
                "instrument_id": str(instrument_id),
                "file": "strict-l2-catalog-receipt.json",
                "sha256": receipt_sha256,
            },
        )
        first_timestamps.append(receipt["first_ts_init_ns"])
        catalog = ParquetDataCatalog(str(snapshot_catalog_path))
        instruments = [item for item in catalog.instruments() if item.id == instrument_id]
        if len(instruments) != 1:
            raise ValueError("catalog does not contain exactly one bound instrument")
        instrument = instruments[0]
        if (
            instrument.price_precision != receipt["price_precision"]
            or str(instrument.price_increment) != receipt["price_increment"]
            or str(instrument.quote_currency) != receipt["currency"]
        ):
            raise ValueError("catalog instrument differs from its strict-L2 receipt")
        data_configs.append(
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(snapshot_catalog_path),
                instrument_id=instrument_id,
            ),
        )
        bindings.append((symbol, instrument_id))
    if len(venues) != 1:
        raise ValueError("one replay request must use exactly one venue")
    return data_configs, bindings, venues.pop(), min(first_timestamps), receipt_bindings


def _verify_catalog_receipt(  # noqa: C901, PLR0913
    catalog_path: Path,
    *,
    snapshot_path: Path,
    symbol: str,
    instrument_id: InstrumentId,
    expected_receipt_sha256: object,
    source_manifest_sha256: str,
    symbol_metadata_sha256: str,
) -> tuple[dict[str, Any], str]:
    receipt_path = catalog_path / "strict-l2-catalog-receipt.json"
    if not receipt_path.is_file():
        raise ValueError("strict-L2 catalog receipt is missing")
    receipt_bytes = receipt_path.read_bytes()
    receipt_sha256 = _sha256_bytes(receipt_bytes)
    if (
        not isinstance(expected_receipt_sha256, str)
        or len(expected_receipt_sha256) != SHA256_HEX_LENGTH
        or any(character not in "0123456789abcdef" for character in expected_receipt_sha256)
        or receipt_sha256 != expected_receipt_sha256
    ):
        raise ValueError("strict-L2 catalog receipt SHA-256 binding is invalid")
    receipt = json.loads(receipt_bytes)
    required = {
        "schema_version",
        "catalog_path",
        "instrument_id",
        "symbol",
        "records",
        "first_ts_init_ns",
        "last_ts_init_ns",
        "availability_tie_break_ns",
        "source_manifest_sha256",
        "symbol_metadata_sha256",
        "price_precision",
        "price_increment",
        "currency",
        "files",
    }

    if not isinstance(receipt, dict) or set(receipt) != required:
        raise ValueError("strict-L2 catalog receipt fields are invalid")
    if (
        receipt["schema_version"] != "strict-l2-nautilus-catalog/v2"
        or Path(receipt["catalog_path"]).expanduser().resolve() != catalog_path
        or receipt["instrument_id"] != str(instrument_id)
        or receipt["symbol"] != symbol
        or receipt["source_manifest_sha256"] != source_manifest_sha256
        or receipt["symbol_metadata_sha256"] != symbol_metadata_sha256
        or receipt["availability_tie_break_ns"] != 1
    ):
        raise ValueError("strict-L2 catalog receipt binding mismatch")
    if (
        isinstance(receipt["records"], bool)
        or not isinstance(receipt["records"], int)
        or receipt["records"] < 1
        or receipt["first_ts_init_ns"] > receipt["last_ts_init_ns"]
    ):
        raise ValueError("strict-L2 catalog receipt range is invalid")
    files = receipt["files"]
    if not isinstance(files, list) or not files:
        raise ValueError("strict-L2 catalog receipt has no artifact inventory")
    declared = set()
    for entry in files:
        if not isinstance(entry, dict) or set(entry) != {"path", "size_bytes", "sha256"}:
            raise ValueError("strict-L2 catalog file entry is invalid")
        pure = PurePosixPath(str(entry["path"]))
        if (
            pure.is_absolute()
            or not pure.parts
            or any(part in {"", ".", ".."} for part in pure.parts)
        ):
            raise ValueError("strict-L2 catalog file path is not normalized")
        path = (catalog_path / Path(*pure.parts)).resolve(strict=True)
        path.relative_to(catalog_path)
        artifact_bytes = path.read_bytes()
        if (
            not path.is_file()
            or len(artifact_bytes) != entry["size_bytes"]
            or _sha256_bytes(
                artifact_bytes,
            )
            != entry["sha256"]
        ):
            raise ValueError("strict-L2 catalog artifact digest mismatch")
        snapshot_artifact = snapshot_path / Path(*pure.parts)
        snapshot_artifact.parent.mkdir(parents=True, exist_ok=True)
        snapshot_artifact.write_bytes(artifact_bytes)
        declared.add(pure.as_posix())
    actual = {
        path.relative_to(catalog_path).as_posix()
        for path in catalog_path.rglob("*")
        if path.is_file() and path != receipt_path
    }

    if declared != actual:
        raise ValueError("strict-L2 catalog artifact inventory mismatch")
    return receipt, receipt_sha256


def _session_bounds(trading_date: str) -> tuple[int, int]:
    day = pd.Timestamp(trading_date, tz="America/New_York")
    start = day + pd.Timedelta(hours=9, minutes=30)
    end = day + pd.Timedelta(hours=16)
    return int(start.tz_convert("UTC").value), int(end.tz_convert("UTC").value)


def _validated_orders(
    node: BacktestNode,
    config_id: str,
    bindings: list[tuple[str, InstrumentId]],
    strategies: list[CandidateReplayStrategy],
    results: list[object],
) -> pd.DataFrame:
    if len(results) != 1:
        raise RuntimeError("candidate replay did not produce exactly one engine result")
    failures = [message for strategy in strategies for message in strategy.failures]
    if failures:
        raise RuntimeError("candidate replay failed closed: " + "; ".join(failures))
    portfolio = node.get_engine_portfolio(config_id)
    if any(not portfolio.is_net_flat(instrument_id) for _, instrument_id in bindings):
        raise RuntimeError("candidate replay finished with a non-flat portfolio")
    return node.generate_orders_report(config_id)


def _load_bound_audit(request: dict[str, Any]) -> tuple[Path, bytes, dict[str, Any], str]:
    audit_path = Path(request["audit_receipt_path"]).expanduser().resolve(strict=True)
    audit_bytes = audit_path.read_bytes()
    digest = request["audit_receipt_sha256"]
    if (
        not isinstance(digest, str)
        or len(digest) != SHA256_HEX_LENGTH
        or any(character not in "0123456789abcdef" for character in digest)
        or _sha256_bytes(audit_bytes) != digest
    ):
        raise ValueError("candidate audit receipt SHA-256 binding is invalid")
    audit = json.loads(audit_bytes)
    if not isinstance(audit, dict) or audit.get("artifact_valid") is not True:
        raise ValueError("candidate replay requires a valid audit receipt")
    return audit_path, audit_bytes, audit, digest


def _public_request(
    request: dict[str, Any],
    *,
    source_manifest_sha256: str,
) -> dict[str, Any]:
    return {
        key: value
        for key, value in request.items()
        if key not in {"audit_receipt_path", "source_manifest_path", "catalogs"}
    } | {
        "source_manifest_sha256": source_manifest_sha256,
        "catalogs": [
            {key: value for key, value in catalog.items() if key != "catalog_path"}
            for catalog in request["catalogs"]
        ],
    }


def run_candidate_replay(
    request_path: str | Path,
    output_root: str | Path,
) -> dict[str, Any]:
    """
    Execute and atomically publish one strict-L2 candidate replay day.

    The request binds the candidate audit, source manifest, catalogs, and execution
    settings; ``REQUEST_FIELDS`` defines its exact fields. Candidate eligibility is
    enforced by ``load_candidate_signals`` without a separate replay policy artifact.
    The session is 09:30-16:00 America/New_York, with earlier catalog data used for
    book warmup. Feedback has ``environment=backtest`` and provides no live evidence.

    """
    request, request_sha256 = _load_request(request_path)
    source_manifest = Path(request["source_manifest_path"]).expanduser().resolve(strict=True)
    requested_symbols = {item["symbol"] for item in request["catalogs"]}
    source_manifest_sha256, symbol_metadata_sha256 = _validate_source_manifest(
        source_manifest,
        trading_date=request["trading_date"],
        symbols=requested_symbols,
    )
    start_ns, end_ns = _session_bounds(request["trading_date"])
    audit_path, audit_bytes, audit, audit_receipt_sha256 = _load_bound_audit(request)
    candidate_signals = load_candidate_signals_snapshot(
        audit_path,
        audit_bytes,
        horizon_ms=request["horizon_ms"],
        instruments=requested_symbols,
        start_ns=start_ns,
        end_ns=end_ns,
    )
    signals_by_symbol: dict[str, tuple[CandidateSignal, ...]] = {
        symbol: tuple(signal for signal in candidate_signals if signal.instrument == symbol)
        for symbol in requested_symbols
    }
    if any(not signals for signals in signals_by_symbol.values()):
        raise ValueError("candidate contains no signals for a requested instrument")
    identity = {
        "request_sha256": request_sha256,
        "audit_receipt_sha256": audit_receipt_sha256,
        "candidate_run_id": audit.get("run_id"),
        "trading_date": request["trading_date"],
    }
    replay_id = _canonical_sha256(identity)[:20]
    root = Path(output_root).expanduser().resolve()
    final = root / replay_id
    staging = root / f".incomplete-{replay_id}"
    if final.exists() or staging.exists():
        raise FileExistsError("candidate replay publication never overwrites output")
    root.mkdir(parents=True, exist_ok=True)
    staging.mkdir()
    snapshot_root = staging / ".catalog-snapshots"
    snapshot_root.mkdir()
    strategies: list[CandidateReplayStrategy] = []
    node = None
    try:
        data_configs, bindings, venue, catalog_start_ns, catalog_receipts = _load_catalog_bindings(
            request["catalogs"],
            source_manifest_sha256,
            symbol_metadata_sha256,
            snapshot_root,
        )
        book_warmup_start_ns = min(catalog_start_ns, start_ns)
        config = BacktestRunConfig(
            id=replay_id,
            venues=[
                BacktestVenueConfig(
                    name=venue,
                    oms_type=OmsType.NETTING,
                    account_type=AccountType.MARGIN,
                    starting_balances=request["starting_balances"],
                    book_type=BookType.L2_MBP,
                    use_reduce_only=True,
                    trade_execution=True,
                    liquidity_consumption=True,
                    fee_model=_PerShareFeeModel(Decimal(request["fee_per_share_usd"])),
                    latency_model=StaticLatencyModel(
                        base_latency_nanos=0,
                        insert_latency_nanos=request["order_insert_latency_ns"],
                        update_latency_nanos=request["order_insert_latency_ns"],
                        cancel_latency_nanos=request["order_insert_latency_ns"],
                    ),
                ),
            ],
            data=data_configs,
            engine=BacktestEngineConfig(bypass_logging=True, run_analysis=True),
            dispose_on_completion=False,
            start=book_warmup_start_ns,
            end=end_ns,
        )
        node = BacktestNode([config])
        node.build()

        for symbol, instrument_id in bindings:
            strategy = CandidateReplayStrategy(
                CandidateReplayConfig(
                    instrument_id=str(instrument_id),
                    research_symbol=symbol,
                    audit_receipt_path=str(audit_path),
                    horizon_ms=request["horizon_ms"],
                    trade_size=request["trade_size"],
                    min_abs_delta_ticks=request["min_abs_delta_ticks"],
                    min_direction_probability=request["min_direction_probability"],
                    cooldown_ms=request["cooldown_ms"],
                    max_signal_lag_ms=request["max_signal_lag_ms"],
                    replay_start_ns=start_ns,
                    replay_end_ns=end_ns,
                    order_insert_latency_ns=request["order_insert_latency_ns"],
                ),
                signals=signals_by_symbol[symbol],
            )
            strategies.append(strategy)
            node.add_strategy(config.id, strategy)
        results = node.run()
        orders = _validated_orders(node, config.id, bindings, strategies, results)
        feedback_bindings = {
            key: value
            for strategy in strategies
            for key, value in strategy.feedback_bindings.items()
        }
        records = feedback_records_from_orders_report(
            orders.reset_index().to_dict("records"),
            feedback_bindings,
        )
        result = results[0]
        execution_assumptions = {
            "execution_mode": "aggressive_marketable_fok",
            "market_data_availability": {
                "research_boundary": "left_closed_ts_recv",
                "catalog_ts_init_offset_ns": 1,
            },
            "fee_scenario": {
                "model": "per_share",
                "currency": "USD",
                "fee_per_share": request["fee_per_share_usd"],
            },
            "latency_scenario": {
                "model": "static_order_latency",
                "unit": "nanosecond",
                "order_insert_latency_ns": request["order_insert_latency_ns"],
            },
        }
        metrics = _json_value(
            {
                "summary": result.summary,
                "stats_pnls": result.stats_pnls,
                "stats_returns": result.stats_returns,
                "stats_general": result.stats_general,
                "iterations": result.iterations,
                "total_events": result.total_events,
                "total_orders": result.total_orders,
                "total_positions": result.total_positions,
                "consumed_signals": {
                    symbol: strategy.consumed_signals
                    for (symbol, _), strategy in zip(bindings, strategies, strict=True)
                },
                "execution_assumptions": execution_assumptions,
            },
        )
        feedback_path = staging / "execution-feedback.json"
        feedback_receipt = publish_execution_feedback(
            feedback_path,
            environment="backtest",
            trading_date=request["trading_date"],
            run_id=replay_id,
            model_id=audit["run_id"],
            model_artifact_sha256=audit["model_sha256"],
            feature_manifest_sha256=source_manifest_sha256,
            reconciliation={
                "status": "complete",
                "orders": len(orders),
                "bindings": len(feedback_bindings),
                "records": len(records),
            },
            records=records,
            metrics=metrics,
        )
        payload = {
            "schema_version": RESULT_SCHEMA,
            "replay_id": replay_id,
            **identity,
            "request": _public_request(
                request,
                source_manifest_sha256=source_manifest_sha256,
            ),
            "catalog_receipts": catalog_receipts,
            "book_type": "L2_MBP",
            **execution_assumptions,
            "trade_execution": True,
            "liquidity_consumption": True,
            "book_warmup_start_ns": book_warmup_start_ns,
            "feedback_file": "execution-feedback.json",
            "feedback_sha256": feedback_receipt["sha256"],
            "metrics": metrics,
        }
        rendered = json.dumps(payload, indent=2, sort_keys=True) + "\n"
        (staging / "replay-result.json").write_text(rendered, encoding="utf-8")
        node.dispose()
        node = None
        shutil.rmtree(snapshot_root)
        staging.replace(final)
        return {
            "status": "complete",
            "replay_id": replay_id,
            "output": str(final),
            "feedback_sha256": feedback_receipt["sha256"],
            "records": len(records),
        }
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    finally:
        if node is not None:
            node.dispose()
