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
Tests for strict price-level L2 conversion and publication verification.
"""

import hashlib
import json
from pathlib import Path
from typing import Any

import pandas as pd
import pytest

from nautilus_trader.adapters.strict_l2.data import iter_manifest_deltas
from nautilus_trader.adapters.strict_l2.data import rows_to_deltas
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import RecordFlag


def _row(
    index: int,
    *,
    side: str = "B",
    price: int = 100_000_000_000,
    size: int = 10,
    delta: int = 10,
    action: str = "SET",
    last: bool = True,
) -> dict[str, Any]:
    return {
        "symbol": "TEST",
        "ts_event": 1 + index,
        "ts_recv": 2 + index,
        "sequence": index,
        "event_index": index,
        "side": side,
        "price": price,
        "size": size,
        "delta": delta,
        "action": action,
        "last": last,
    }


def test_absolute_l2_rows_replay_to_matching_mbp_state() -> None:
    """
    Absolute SET rows should reproduce the expected L2_MBP state.
    """
    clear = _row(0, side="N", price=0, size=0, delta=0, action="CLEAR")
    rows = [
        clear,
        _row(1, size=10, delta=10),
        _row(2, size=15, delta=5),
        _row(3, side="A", price=101_000_000_000, size=7, delta=7),
    ]
    instrument = InstrumentId.from_str("TEST.XNAS")
    deltas = list(rows_to_deltas(rows, instrument))
    assert [delta.action for delta in deltas] == [
        BookAction.CLEAR,
        BookAction.ADD,
        BookAction.UPDATE,
        BookAction.ADD,
    ]
    assert all(delta.order.order_id == 0 for delta in deltas[1:])
    assert all(delta.ts_init == row["ts_recv"] + 1 for delta, row in zip(deltas, rows, strict=True))
    book = OrderBook(instrument, BookType.L2_MBP)
    for delta in deltas:
        book.apply_delta(delta)
    assert str(book.best_bid_price()) == "100.000000000"
    assert str(book.best_bid_size()) == "15"
    assert str(book.best_ask_price()) == "101.000000000"


def test_strict_l2_rejects_order_identity_and_incomplete_message() -> None:
    """
    Order identity and truncated logical messages must fail closed.
    """
    instrument = InstrumentId.from_str("TEST.XNAS")
    row = _row(0)
    row["order_id"] = 12
    with pytest.raises(ValueError, match="fields"):
        list(rows_to_deltas([row], instrument))
    row = _row(0, last=False)
    with pytest.raises(ValueError, match="message boundary"):
        list(rows_to_deltas([row], instrument))


def test_strict_l2_rejects_broken_logical_message_identity() -> None:
    """
    Rows before F_LAST must share sequence and availability timestamps.
    """
    instrument = InstrumentId.from_str("TEST.XNAS")
    first = _row(0, last=False)
    second = _row(1, side="A", price=101_000_000_000, size=7, delta=7)
    with pytest.raises(ValueError, match="logical L2 message"):
        list(rows_to_deltas([first, second], instrument))


def test_logical_message_allows_increasing_venue_times_at_one_receive_time() -> None:
    """
    One received packet may aggregate venue events with distinct ordered timestamps.
    """
    instrument = InstrumentId.from_str("TEST.XNAS")
    first = _row(0, last=False)
    second = _row(1, side="A", price=101_000_000_000, size=7, delta=7)
    second["sequence"] = first["sequence"]
    second["ts_recv"] = first["ts_recv"]
    second["ts_event"] = first["ts_event"] + 1
    assert len(list(rows_to_deltas([first, second], instrument))) == 2

    second["ts_event"] = first["ts_event"] - 1
    with pytest.raises(ValueError, match="venue time moved backwards"):
        list(rows_to_deltas([first, second], instrument))


def test_snapshot_flags_preserve_buffered_event_boundaries() -> None:
    """
    CLEAR snapshots must carry snapshot flags and end with F_LAST.
    """
    instrument = InstrumentId.from_str("TEST.XNAS")
    clear = _row(0, side="N", price=0, size=0, delta=0, action="CLEAR", last=False)
    level = _row(1, last=True)
    level["sequence"] = clear["sequence"]
    level["ts_event"] = clear["ts_event"]
    level["ts_recv"] = clear["ts_recv"]
    deltas = list(rows_to_deltas([clear, level], instrument))
    assert deltas[0].flags == RecordFlag.F_SNAPSHOT.value
    assert deltas[1].flags == RecordFlag.F_SNAPSHOT.value | RecordFlag.F_LAST.value

    empty = _row(0, side="N", price=0, size=0, delta=0, action="CLEAR")
    empty_delta = next(rows_to_deltas([empty], instrument))
    assert empty_delta.flags == RecordFlag.F_SNAPSHOT.value | RecordFlag.F_LAST.value


@pytest.mark.parametrize(
    ("mutate", "message"),
    [
        (lambda rows: rows[1].update(event_index=0), "event_index"),
        (lambda rows: rows[1].update(ts_recv=1), "timestamps"),
        (lambda rows: rows[1].update(delta=9), "aggregate depth"),
    ],
)
def test_strict_l2_rejects_ordering_and_state_faults(mutate: Any, message: str) -> None:
    """
    Duplicate indices, reversed clocks and inconsistent state fail closed.
    """
    instrument = InstrumentId.from_str("TEST.XNAS")
    rows = [_row(0), _row(1, size=15, delta=5)]
    mutate(rows)
    with pytest.raises(ValueError, match=message):
        list(rows_to_deltas(rows, instrument))


def test_strict_l2_rejects_clear_inside_open_message_and_absent_delete() -> None:
    """
    A reset cannot split a message and an unknown price level cannot be deleted.
    """
    instrument = InstrumentId.from_str("TEST.XNAS")
    first = _row(0, last=False)
    clear = _row(1, side="N", price=0, size=0, delta=0, action="CLEAR")
    clear.update(sequence=first["sequence"], ts_event=first["ts_event"], ts_recv=first["ts_recv"])
    with pytest.raises(ValueError, match="CLEAR must start"):
        list(rows_to_deltas([first, clear], instrument))

    delete = _row(0, size=0, delta=0)
    with pytest.raises(ValueError, match="absent"):
        list(rows_to_deltas([delete], instrument))


def test_strict_l2_uses_instrument_price_precision_and_rejects_misalignment() -> None:
    """
    Nanosecond integer prices must map exactly to the instrument precision.
    """
    instrument = InstrumentId.from_str("TEST.XNAS")
    delta = next(rows_to_deltas([_row(0)], instrument, price_precision=2))
    assert str(delta.order.price) == "100.00"
    misaligned = _row(0, price=100_000_000_001)
    with pytest.raises(ValueError, match="price precision"):
        list(rows_to_deltas([misaligned], instrument, price_precision=2))


def test_manifest_stream_verifies_digest_and_preserves_availability_clock(tmp_path: Path) -> None:
    """
    Manifest ingestion verifies bytes and maps receive time to ts_init.
    """
    rows = [
        _row(0, side="N", price=0, size=0, delta=0, action="CLEAR"),
        _row(1),
    ]
    partition = tmp_path / "TEST.parquet"
    pd.DataFrame(rows).to_parquet(partition, index=False)
    manifest = tmp_path / "published-dataset-manifest.json"
    manifest.write_text(
        json.dumps(
            {
                "schema_version": "research/published-dataset-manifest-v1",
                "dataset_kind": "strict-l2-mbp",
                "contracts": {"l2": "strict-l2-v1"},
                "files": [
                    {
                        "role": "l2_deltas",
                        "symbol": "TEST",
                        "path": partition.name,
                        "size_bytes": partition.stat().st_size,
                        "sha256": hashlib.sha256(partition.read_bytes()).hexdigest(),
                    },
                ],
            },
        ),
    )
    instrument = InstrumentId.from_str("TEST.XNAS")
    deltas = list(iter_manifest_deltas(manifest, symbol="TEST", instrument_id=instrument))
    assert [delta.ts_init for delta in deltas] == [3, 4]
    value = json.loads(manifest.read_text())
    value["files"][0]["sha256"] = "0" * 64
    manifest.write_text(json.dumps(value))
    with pytest.raises(ValueError, match="digest"):
        list(iter_manifest_deltas(manifest, symbol="TEST", instrument_id=instrument))
