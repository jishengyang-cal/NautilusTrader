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
Publish identifier-free daily research feedback from Nautilus order reports.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Mapping
from collections.abc import Sequence
from decimal import Decimal
from pathlib import Path
from typing import Any

from nautilus_trader.backtest.strict_l2.feedback import feedback_records_from_orders_report
from nautilus_trader.backtest.strict_l2.feedback import publish_execution_feedback


def _load_json(path: str | Path) -> object:
    source = Path(path).expanduser().resolve(strict=True)
    return json.loads(source.read_text(encoding="utf-8"), parse_float=Decimal)


def export_feedback_from_json(
    request_path: str | Path,
    orders_report_path: str | Path,
    output_path: str | Path,
) -> dict[str, Any]:
    """
    Load a daily export request and publish sanitized research feedback.

    Decimal JSON numbers in both inputs are parsed as ``Decimal`` so fees and
    order amounts reach aggregation without binary floating-point conversion.
    See ``feedback_records_from_orders_report`` for the native row and binding contract.

    """
    request = _load_json(request_path)
    rows = _load_json(orders_report_path)

    if not isinstance(request, Mapping):
        raise TypeError("feedback request must be a JSON object")
    if not isinstance(rows, Sequence) or isinstance(rows, (str, bytes, bytearray)):
        raise TypeError("orders report must be a JSON array")
    if not all(isinstance(row, Mapping) for row in rows):
        raise TypeError("every orders report row must be a JSON object")
    bindings = request.get("bindings")
    reconciliation = request.get("reconciliation")
    metrics = request.get("metrics", {})
    if not isinstance(bindings, Mapping):
        raise TypeError("feedback request bindings must be a JSON object")
    if not isinstance(reconciliation, Mapping):
        raise TypeError("feedback request reconciliation must be a JSON object")
    if not isinstance(metrics, Mapping):
        raise TypeError("feedback request metrics must be a JSON object")
    required = (
        "environment",
        "trading_date",
        "run_id",
        "model_id",
        "model_artifact_sha256",
        "feature_manifest_sha256",
    )
    missing = [field for field in required if not isinstance(request.get(field), str)]
    if missing:
        raise ValueError(f"feedback request is missing string fields: {', '.join(missing)}")
    records = feedback_records_from_orders_report(rows, bindings)
    return publish_execution_feedback(
        output_path,
        environment=request["environment"],
        trading_date=request["trading_date"],
        run_id=request["run_id"],
        model_id=request["model_id"],
        model_artifact_sha256=request["model_artifact_sha256"],
        feature_manifest_sha256=request["feature_manifest_sha256"],
        reconciliation=reconciliation,
        records=records,
        metrics=metrics,
    )


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", required=True, help="Daily feedback request JSON")
    parser.add_argument("--orders-report", required=True, help="Nautilus orders report JSON")
    parser.add_argument("--output", required=True, help="Immutable feedback output JSON")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """
    Run the daily feedback export command.
    """
    args = _parse_args(argv)
    receipt = export_feedback_from_json(args.request, args.orders_report, args.output)
    sys.stdout.write(json.dumps(receipt, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
