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
Source binding tests use synthetic catalogs, never live execution evidence.
"""

import json
from pathlib import Path

import pytest
from tests.unit.backtest.strict_l2.test_strategy import _audit_receipt
from tests.unit.backtest.strict_l2.test_strategy import _catalog
from tests.unit.backtest.strict_l2.test_strategy import _replay_request

from nautilus_trader.backtest.strict_l2 import replay


@pytest.mark.parametrize("source_changes", [False, True])
def test_profile_sources_are_bound_before_and_after_run(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    source_changes: bool,
) -> None:
    """
    Verify source drift rejects publication while preserving existing output.
    """
    receipt = _audit_receipt(tmp_path)
    catalog, instrument, source_manifest = _catalog(tmp_path)
    request = tmp_path / "request.json"
    request.write_text(json.dumps(_replay_request(receipt, catalog, instrument, source_manifest)))
    output = tmp_path / "output"
    output.mkdir()
    existing = output / "existing-publication"
    existing.write_bytes(b"preserve")
    original_sha256 = replay._sha256
    original_run = replay.BacktestNode.run
    runs = 0
    source_checks: list[int] = []

    def run(node):
        nonlocal runs
        runs += 1
        return original_run(node)

    def sha256(path):
        if path == Path(replay.__file__).with_name("strategy.py"):
            source_checks.append(runs)
            if source_changes and runs:
                return "0" * 64
        return original_sha256(path)

    monkeypatch.setattr(replay.BacktestNode, "run", run)
    monkeypatch.setattr(replay, "_sha256", sha256)

    if source_changes:
        with pytest.raises(ValueError, match=r"profiling source.*changed"):
            replay.run_candidate_replay(request, output, record_performance=True)
        assert list(output.iterdir()) == [existing]
    else:
        result = replay.run_candidate_replay(request, output, record_performance=True)
        report = json.loads((Path(result["output"]) / "performance.json").read_text())
        assert report["source_sha256"]["strategy.py"] == original_sha256(
            Path(replay.__file__).with_name("strategy.py"),
        )
    assert source_checks == [0, 1]
    assert runs == 1
    assert existing.read_bytes() == b"preserve"
