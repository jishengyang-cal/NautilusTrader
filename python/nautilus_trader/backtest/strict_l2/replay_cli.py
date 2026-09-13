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
Run one audited strict-L2 candidate replay request.
"""

from __future__ import annotations

import argparse
import json
import sys

from nautilus_trader.backtest.strict_l2.replay import run_candidate_replay


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", required=True, help="Immutable replay request JSON")
    parser.add_argument("--output-root", required=True, help="Immutable replay publication root")
    parser.add_argument(
        "--record-performance",
        action="store_true",
        help="Write bounded host callback timings separately from execution feedback",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """
    Run a candidate replay and print its publication receipt.
    """
    args = _parse_args(argv)
    result = run_candidate_replay(
        args.request,
        args.output_root,
        record_performance=args.record_performance,
    )
    sys.stdout.write(json.dumps(result, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
