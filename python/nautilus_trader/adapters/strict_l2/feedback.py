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
Atomic, identifier-free daily execution feedback publication.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
from collections.abc import Mapping
from collections.abc import Sequence
from datetime import UTC
from datetime import date
from datetime import datetime
from decimal import Decimal
from decimal import InvalidOperation
from numbers import Integral
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo


FORBIDDEN_KEYS = {
    "accountid",
    "brokerorderid",
    "clientorderid",
    "orderid",
    "venueorderid",
    "mpid",
}
TRADING_TIMEZONE = ZoneInfo("America/New_York")
SHA256_HEX_LENGTH = 64
SIGNAL_CONTEXT_FIELDS = (
    "parent_prediction_id",
    "signal_ts_recv_ns",
    "horizon_ms",
    "action_role",
    "research_symbol",
)


def _validate_digest(value: str, field: str) -> None:
    if len(value) != SHA256_HEX_LENGTH or any(char not in "0123456789abcdef" for char in value):
        raise ValueError(f"{field} must be a lowercase SHA-256")


def _validate_payload(value: object, location: str = "payload") -> None:
    if isinstance(value, Mapping):
        for key, child in value.items():
            normalized = re.sub(r"[^a-z0-9]", "", str(key).lower())
            if normalized in FORBIDDEN_KEYS:
                raise ValueError(f"{location} contains a forbidden execution identity")
            _validate_payload(child, f"{location}.{key}")
    elif isinstance(value, Sequence) and not isinstance(value, (str, bytes, bytearray)):
        for index, child in enumerate(value):
            _validate_payload(child, f"{location}[{index}]")
    elif isinstance(value, float) and not math.isfinite(value):
        raise ValueError(f"{location} contains a non-finite value")


def _signal_context(binding: Mapping[str, Any], location: str) -> dict[str, Any]:
    present = [field in binding for field in SIGNAL_CONTEXT_FIELDS]
    if not any(present):
        return {}
    if not all(present):
        raise ValueError(f"{location} has an incomplete signal context")
    parent = binding["parent_prediction_id"]
    if not isinstance(parent, str) or not parent:
        raise ValueError(f"{location}.parent_prediction_id is required")
    signal_ts = _timestamp_ns(binding["signal_ts_recv_ns"], f"{location}.signal_ts_recv_ns")
    horizon = binding["horizon_ms"]
    if isinstance(horizon, bool) or not isinstance(horizon, Integral) or horizon < 1:
        raise ValueError(f"{location}.horizon_ms must be a positive integer")
    action_role = binding["action_role"]
    if action_role not in {"ENTRY", "EXIT"}:
        raise ValueError(f"{location}.action_role must be ENTRY or EXIT")
    research_symbol = binding["research_symbol"]
    if not isinstance(research_symbol, str) or not research_symbol:
        raise ValueError(f"{location}.research_symbol is required")
    return {
        "parent_prediction_id": parent,
        "signal_ts_recv_ns": signal_ts,
        "horizon_ms": int(horizon),
        "action_role": action_role,
        "research_symbol": research_symbol,
    }


def _validate_records(  # noqa: C901, PLR0912
    records: Sequence[Mapping[str, Any]],
    trading_day: date,
) -> None:
    for index, record in enumerate(records):
        for field in ("prediction_id", "instrument_uid", "side", "status"):
            if not isinstance(record.get(field), str) or not record[field]:
                raise ValueError(f"records[{index}].{field} is required")
        if record["side"] not in {"BUY", "SELL"}:
            raise ValueError(f"records[{index}].side must be BUY or SELL")
        timestamps = []

        for field in ("decision_ts_ns", "submit_ts_ns"):
            value = record.get(field)
            if isinstance(value, bool) or not isinstance(value, int) or value < 0:
                raise ValueError(f"records[{index}].{field} must be Unix nanoseconds")
            local_day = (
                datetime.fromtimestamp(
                    value // 1_000_000_000,
                    UTC,
                )
                .astimezone(TRADING_TIMEZONE)
                .date()
            )

            if local_day != trading_day:
                raise ValueError(f"records[{index}].{field} is outside trading_date")
            timestamps.append(value)
        if timestamps[0] > timestamps[1]:
            raise ValueError(f"records[{index}] submits before its decision")
        context = _signal_context(record, f"records[{index}]")
        if context:
            signal_ts = context["signal_ts_recv_ns"]
            local_day = (
                datetime.fromtimestamp(
                    signal_ts // 1_000_000_000,
                    UTC,
                )
                .astimezone(TRADING_TIMEZONE)
                .date()
            )

            if local_day != trading_day or signal_ts > timestamps[0]:
                raise ValueError(f"records[{index}] has an invalid signal availability time")
        fill = record.get("last_fill_ts_ns")
        if fill is not None:
            if isinstance(fill, bool) or not isinstance(fill, int) or fill < timestamps[1]:
                raise ValueError(f"records[{index}] has an invalid fill time")
            local_day = (
                datetime.fromtimestamp(
                    fill // 1_000_000_000,
                    UTC,
                )
                .astimezone(TRADING_TIMEZONE)
                .date()
            )

            if local_day != trading_day:
                raise ValueError(f"records[{index}] fill is outside trading_date")
        for field in ("filled_qty", "average_fill_price", "fees"):
            value = record.get(field)
            if value is not None and (
                isinstance(value, bool)
                or not isinstance(value, (int, float))
                or not math.isfinite(value)
            ):
                raise ValueError(f"records[{index}].{field} must be finite numeric data")
        if record.get("filled_qty", 0) < 0:
            raise ValueError(f"records[{index}].filled_qty cannot be negative")
        if record.get("filled_qty", 0) > 0 and record.get("average_fill_price", 0) <= 0:
            raise ValueError(f"records[{index}] requires a positive average fill price")


def _timestamp_ns(value: object, field: str) -> int:
    if isinstance(value, bool):
        raise TypeError(f"{field} must be a Unix-nanosecond integer or timezone-aware timestamp")
    if isinstance(value, Integral):
        return int(value)
    raw = getattr(value, "value", None)
    timezone = getattr(value, "tzinfo", None)
    if isinstance(raw, Integral) and timezone is not None:
        return int(raw)
    raise TypeError(f"{field} must be a Unix-nanosecond integer or timezone-aware timestamp")


def _finite_decimal(value: object, field: str) -> Decimal:
    if isinstance(value, bool) or value is None:
        raise TypeError(f"{field} must be finite numeric data")
    try:
        parsed = Decimal(str(value))
    except (InvalidOperation, ValueError) as exc:
        raise ValueError(f"{field} must be finite numeric data") from exc
    if not parsed.is_finite():
        raise ValueError(f"{field} must be finite numeric data")
    return parsed


def feedback_records_from_orders_report(  # noqa: C901, PLR0912, PLR0915
    rows: Sequence[Mapping[str, Any]],
    bindings: Mapping[str, Mapping[str, Any]],
) -> list[dict[str, Any]]:
    """
    Aggregate native Nautilus order-report rows by prediction and remove order identity.

    ``bindings`` is strategy-owned ephemeral state keyed by ``client_order_id``.
    It supplies the prediction identity, canonical instrument UID, decision
    timestamp, last fill-event timestamp and reconciled fee for each order. No client
    or venue order ID is copied into the returned research records. Pass native output as
    ``ReportProvider.generate_orders_report(...).reset_index().to_dict("records")``.

    """
    aggregates: dict[str, dict[str, Any]] = {}
    consumed_bindings: set[str] = set()

    for index, row in enumerate(rows):
        client_order_id = row.get("client_order_id")
        if not isinstance(client_order_id, str) or not client_order_id:
            raise ValueError(f"rows[{index}].client_order_id is required at the Nautilus boundary")
        if client_order_id in consumed_bindings:
            raise ValueError(f"rows[{index}] duplicates a Nautilus client order identity")
        consumed_bindings.add(client_order_id)
        binding = bindings.get(client_order_id)
        if not isinstance(binding, Mapping):
            raise TypeError(f"rows[{index}] has no strategy prediction binding")
        prediction_id = binding.get("prediction_id")
        instrument_uid = binding.get("instrument_uid")
        decision_ts = binding.get("decision_ts_ns")
        fees = _finite_decimal(
            binding.get("fees", 0.0),
            f"bindings[{client_order_id!r}].fees",
        )

        if not isinstance(prediction_id, str) or not prediction_id:
            raise ValueError(f"bindings[{client_order_id!r}].prediction_id is required")
        if not isinstance(instrument_uid, str) or not instrument_uid:
            raise ValueError(f"bindings[{client_order_id!r}].instrument_uid is required")
        side = row.get("side")
        if side not in {"BUY", "SELL"}:
            raise ValueError(f"rows[{index}] has an unsupported order side")
        status = str(row.get("status", ""))
        if not status:
            raise ValueError(f"rows[{index}].status is required")
        filled_qty = _finite_decimal(row.get("filled_qty", 0.0), f"rows[{index}].filled_qty")
        average_fill_price = row.get("avg_px")

        if filled_qty < 0:
            raise ValueError(f"rows[{index}].filled_qty must be finite and non-negative")
        if filled_qty > 0:
            average_fill_price = _finite_decimal(
                average_fill_price,
                f"rows[{index}].average_fill_price",
            )

            if average_fill_price <= 0:
                raise ValueError(f"rows[{index}] requires a positive finite average fill price")
        submit_ts = _timestamp_ns(row.get("ts_init"), f"rows[{index}].ts_init")
        fill_ts_raw = binding.get("last_fill_ts_ns")
        fill_ts = (
            None
            if not filled_qty
            else _timestamp_ns(fill_ts_raw, f"bindings[{client_order_id!r}].last_fill_ts_ns")
        )
        decision_ns = _timestamp_ns(
            decision_ts,
            f"bindings[{client_order_id!r}].decision_ts_ns",
        )
        context = _signal_context(binding, f"bindings[{client_order_id!r}]")
        aggregate = aggregates.setdefault(
            prediction_id,
            {
                "prediction_id": prediction_id,
                "instrument_uid": instrument_uid,
                "side": side,
                "statuses": set(),
                "decision_ts_ns": decision_ns,
                "submit_ts_ns": submit_ts,
                "last_fill_ts_ns": fill_ts,
                "filled_qty": Decimal(0),
                "fill_notional": Decimal(0),
                "fees": Decimal(0),
                **context,
            },
        )

        if (
            aggregate["instrument_uid"] != instrument_uid
            or aggregate["side"] != side
            or aggregate["decision_ts_ns"] != decision_ns
            or any(aggregate.get(field) != context.get(field) for field in SIGNAL_CONTEXT_FIELDS)
        ):
            raise ValueError(f"prediction {prediction_id!r} has inconsistent bound orders")
        aggregate["statuses"].add(status)
        aggregate["submit_ts_ns"] = min(aggregate["submit_ts_ns"], submit_ts)
        if fill_ts is not None:
            aggregate["last_fill_ts_ns"] = max(aggregate["last_fill_ts_ns"] or 0, fill_ts)
        aggregate["filled_qty"] += filled_qty
        if filled_qty:
            aggregate["fill_notional"] += filled_qty * average_fill_price
        aggregate["fees"] += fees
    if set(bindings) != consumed_bindings:
        raise ValueError("strategy bindings and Nautilus order rows must reconcile one-to-one")
    records = []

    for prediction_id in sorted(aggregates):
        aggregate = aggregates[prediction_id]
        filled_qty = aggregate.pop("filled_qty")
        fill_notional = aggregate.pop("fill_notional")
        statuses = aggregate.pop("statuses")
        aggregate["status"] = next(iter(statuses)) if len(statuses) == 1 else "MIXED"
        aggregate["filled_qty"] = float(filled_qty)
        aggregate["average_fill_price"] = float(fill_notional / filled_qty) if filled_qty else None
        aggregate["fees"] = float(aggregate["fees"])
        if not filled_qty:
            aggregate["last_fill_ts_ns"] = None
        records.append(aggregate)
    _validate_payload(records, "records")
    return records


def publish_execution_feedback(  # noqa: PLR0913
    output_path: str | Path,
    *,
    environment: str,
    trading_date: str,
    run_id: str,
    model_id: str,
    model_artifact_sha256: str,
    feature_manifest_sha256: str,
    reconciliation: Mapping[str, Any],
    records: Sequence[Mapping[str, Any]],
    metrics: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """
    Publish one reconciled day; the final JSON is never overwritten.
    """
    if environment not in {"backtest", "paper", "live"}:
        raise ValueError("unsupported execution environment")
    trading_day = date.fromisoformat(trading_date)
    _validate_digest(model_artifact_sha256, "model_artifact_sha256")
    _validate_digest(feature_manifest_sha256, "feature_manifest_sha256")

    if reconciliation.get("status") != "complete":
        raise ValueError("daily execution feedback requires complete reconciliation")
    _validate_payload(records, "records")
    _validate_records(records, trading_day)
    payload = {
        "schema_version": "research/execution-feedback-v1",
        "environment": environment,
        "trading_date": trading_date,
        "timestamp_epoch": "Unix",
        "timestamp_unit": "nanosecond",
        "trading_timezone": "America/New_York",
        "created_at": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
        "run_id": run_id,
        "model_id": model_id,
        "model_artifact_sha256": model_artifact_sha256,
        "feature_manifest_sha256": feature_manifest_sha256,
        "reconciliation": dict(reconciliation),
        "records": [dict(record) for record in records],
        "metrics": dict(metrics or {}),
    }
    _validate_payload(payload)
    target = Path(output_path).expanduser().resolve()
    target.parent.mkdir(parents=True, exist_ok=True)
    staging = target.with_name(f".{target.name}.incomplete")
    if target.exists() or staging.exists():
        raise FileExistsError("execution feedback publication never overwrites output")
    rendered = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    staging.write_text(rendered, encoding="utf-8")
    staging.replace(target)
    return {
        "path": str(target),
        "sha256": hashlib.sha256(rendered.encode()).hexdigest(),
        "records": len(records),
    }
