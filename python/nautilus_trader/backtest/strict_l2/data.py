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
Convert immutable strict-L2 price-level publications to Nautilus deltas.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from pathlib import PurePosixPath
from typing import TYPE_CHECKING

import pyarrow.parquet as pq

from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag


if TYPE_CHECKING:
    from collections.abc import Iterable
    from collections.abc import Iterator
    from collections.abc import Mapping

STRICT_L2_FIELDS = frozenset(
    {
        "symbol",
        "ts_event",
        "ts_recv",
        "sequence",
        "event_index",
        "side",
        "price",
        "size",
        "delta",
        "action",
        "last",
    },
)
NANO_PRICE_PRECISION = 9
AVAILABILITY_TIE_BREAK_NS = 1


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _resolve_file(root: Path, entry: Mapping) -> Path:
    declared = entry.get("path")
    pure = PurePosixPath(str(declared))
    if pure.is_absolute() or not pure.parts or any(part in {"", ".", ".."} for part in pure.parts):
        raise ValueError("manifest file path must be normalized and relative")
    path = (root / Path(*pure.parts)).resolve(strict=True)
    path.relative_to(root)
    if path.stat().st_size != entry.get("size_bytes") or _sha256(path) != entry.get("sha256"):
        raise ValueError(f"published strict-L2 artifact failed digest verification: {declared}")
    return path


def rows_to_deltas(  # noqa: C901, PLR0912, PLR0915
    rows: Iterable[Mapping],
    instrument_id: InstrumentId,
    *,
    expected_symbol: str | None = None,
    price_precision: int = 9,
) -> Iterator[OrderBookDelta]:
    """
    Map absolute price-level rows and preserve the research left-closed boundary.

    ``ts_recv`` is the source availability time. Nautilus market data uses
    ``ts_init = ts_recv + 1 ns`` so a model timer at ``t`` observes exactly the
    completed messages with ``ts_recv < t``; venue time remains unchanged.

    """
    if isinstance(price_precision, bool) or not isinstance(price_precision, int):
        raise TypeError("price_precision must be an integer")
    if not 0 <= price_precision <= NANO_PRICE_PRECISION:
        raise ValueError("price_precision must be in 0..9 for nanosecond prices")
    price_scale = 10 ** (NANO_PRICE_PRECISION - price_precision)
    levels: dict[tuple[str, int], int] = {}
    message_open = False
    message_identity: tuple[int, int] | None = None
    message_last_ts_event = -1
    message_is_snapshot = False
    previous_sequence: int | None = None
    previous_ts_recv = -1

    for expected_index, row in enumerate(rows):
        if frozenset(row) != STRICT_L2_FIELDS:
            raise ValueError("strict-L2 row fields do not match the sanitized contract")
        if expected_symbol is not None and row["symbol"] != expected_symbol:
            raise ValueError("strict-L2 symbol does not match the requested manifest partition")
        event_index = row["event_index"]
        if (
            isinstance(event_index, bool)
            or not isinstance(event_index, int)
            or event_index != expected_index
        ):
            raise ValueError("strict-L2 event_index must be contiguous from zero")
        ts_event = row["ts_event"]
        ts_recv = row["ts_recv"]

        if (
            isinstance(ts_event, bool)
            or isinstance(ts_recv, bool)
            or not isinstance(ts_event, int)
            or not isinstance(ts_recv, int)
            or ts_event < 0
            or ts_event > ts_recv
            or ts_recv < previous_ts_recv
        ):
            raise ValueError("strict-L2 timestamps violate event/availability ordering")
        previous_ts_recv = ts_recv
        sequence = row["sequence"]
        if isinstance(sequence, bool) or not isinstance(sequence, int) or sequence < 0:
            raise ValueError("strict-L2 sequence must be a non-negative integer")
        identity = (sequence, ts_recv)
        if message_open:
            if identity != message_identity:
                raise ValueError(
                    "logical L2 message changed sequence or availability time before F_LAST",
                )
            if ts_event < message_last_ts_event:
                raise ValueError("logical L2 message venue time moved backwards before F_LAST")
        else:
            if previous_sequence is not None and sequence != previous_sequence + 1:
                raise ValueError("strict-L2 sequence must be contiguous between logical messages")
            message_identity = identity
            previous_sequence = sequence
        message_last_ts_event = ts_event
        is_last = row["last"]
        if not isinstance(is_last, bool):
            raise TypeError("strict-L2 last marker must be boolean")
        action = row["action"]
        if action == "CLEAR":
            if message_open:
                raise ValueError("CLEAR must start a logical L2 message")
            if any((row["side"] != "N", row["price"] != 0, row["size"] != 0, row["delta"] != 0)):
                raise ValueError("CLEAR cannot carry a price-level identity")
            message_is_snapshot = True
            levels.clear()
            flags = RecordFlag.F_SNAPSHOT.value
            if is_last:
                flags |= RecordFlag.F_LAST.value
            delta = OrderBookDelta(
                instrument_id,
                BookAction.CLEAR,
                BookOrder(
                    OrderSide.NO_ORDER_SIDE,
                    Price.zero(price_precision),
                    Quantity.zero(0),
                    0,
                ),
                flags,
                sequence,
                ts_event,
                ts_recv + AVAILABILITY_TIE_BREAK_NS,
            )
        elif action == "SET":
            flags = RecordFlag.F_SNAPSHOT.value if message_is_snapshot else 0
            if is_last:
                flags |= RecordFlag.F_LAST.value
            side = row["side"]
            if side not in {"B", "A"}:
                raise ValueError("SET side must be bid or ask")
            price = row["price"]
            size = row["size"]
            change = row["delta"]

            if any(
                isinstance(value, bool) or not isinstance(value, int)
                for value in (price, size, change)
            ):
                raise ValueError("price, size, and delta must be integers")
            if price <= 0 or size < 0:
                raise ValueError("SET requires positive price and non-negative displayed size")
            if price % price_scale:
                raise ValueError("SET price is not aligned to instrument price precision")
            key = (side, price)
            before = levels.get(key, 0)
            if size - before != change:
                raise ValueError("absolute SET is inconsistent with aggregate depth state")
            if size == 0:
                if key not in levels:
                    raise ValueError("cannot delete an absent aggregate price level")
                book_action = BookAction.DELETE
                levels.pop(key)
            else:
                book_action = BookAction.UPDATE if key in levels else BookAction.ADD
                levels[key] = size
            order = BookOrder(
                OrderSide.BUY if side == "B" else OrderSide.SELL,
                Price.from_str(
                    str(price // 1_000_000_000)
                    if price_precision == 0
                    else (
                        f"{price // 1_000_000_000}."
                        f"{(price % 1_000_000_000) // price_scale:0{price_precision}d}"
                    ),
                ),
                Quantity.from_int(size),
                0,
            )
            delta = OrderBookDelta(
                instrument_id,
                book_action,
                order,
                flags,
                sequence,
                ts_event,
                ts_recv + AVAILABILITY_TIE_BREAK_NS,
            )
        else:
            raise ValueError("strict-L2 action must be SET or CLEAR")
        message_open = not is_last
        if not message_open:
            message_identity = None
            message_last_ts_event = -1
            message_is_snapshot = False
        yield delta
    if message_open:
        raise ValueError("strict-L2 stream ended before the logical message boundary")


def iter_manifest_deltas(
    manifest_path: str | Path,
    *,
    symbol: str,
    instrument_id: InstrumentId,
    batch_size: int = 65_536,
    price_precision: int = 9,
) -> Iterator[OrderBookDelta]:
    """
    Verify one immutable publication and stream all L2 updates in source order.
    """
    if batch_size < 1:
        raise ValueError("batch_size must be positive")
    manifest_file = Path(manifest_path).expanduser().resolve(strict=True)
    root = manifest_file.parent
    manifest = json.loads(manifest_file.read_text(encoding="utf-8"))
    if (
        manifest.get("schema_version") != "research/published-dataset-manifest-v1"
        or manifest.get("dataset_kind") != "strict-l2-mbp"
        or manifest.get("contracts", {}).get("l2") != "strict-l2-v1"
    ):
        raise ValueError("strict-L2 published manifest is required")
    matches = [
        entry
        for entry in manifest.get("files", [])
        if entry.get("role") == "l2_deltas" and entry.get("symbol") == symbol
    ]

    if len(matches) != 1:
        raise ValueError("manifest must contain exactly one requested symbol partition")
    path = _resolve_file(root, matches[0])
    parquet = pq.ParquetFile(path)

    def rows() -> Iterator[dict]:
        for batch in parquet.iter_batches(batch_size=batch_size, columns=sorted(STRICT_L2_FIELDS)):
            values = batch.to_pydict()
            for index in range(batch.num_rows):
                yield {name: values[name][index] for name in STRICT_L2_FIELDS}

    yield from rows_to_deltas(
        rows(),
        instrument_id,
        expected_symbol=symbol,
        price_precision=price_precision,
    )
