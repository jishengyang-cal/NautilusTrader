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
Publish verified strict-L2 manifests as immutable Nautilus catalogs.
"""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path
from pathlib import PurePosixPath
from typing import Any

from nautilus_trader.adapters.strict_l2.data import AVAILABILITY_TIE_BREAK_NS
from nautilus_trader.adapters.strict_l2.data import iter_manifest_deltas
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.persistence import ParquetDataCatalog


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(8 * 1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _catalog_files(root: Path) -> list[dict[str, Any]]:
    files = [
        {
            "path": path.relative_to(root).as_posix(),
            "size_bytes": path.stat().st_size,
            "sha256": _sha256(path),
        }
        for path in sorted(item for item in root.rglob("*") if item.is_file())
    ]

    if not files:
        raise ValueError("strict-L2 catalog publication produced no files")
    return files


def _validate_instrument_metadata(
    manifest: Path,
    *,
    symbol: str,
    instrument: object,
) -> str:
    value = json.loads(manifest.read_text(encoding="utf-8"))
    entries = [
        item
        for item in value.get("files", [])
        if isinstance(item, dict) and item.get("role") == "symbol_metadata"
    ]

    if len(entries) != 1:
        raise ValueError("strict-L2 manifest must bind exactly one symbol metadata file")
    entry = entries[0]
    pure = PurePosixPath(str(entry.get("path")))
    if pure.is_absolute() or not pure.parts or any(part in {"", ".", ".."} for part in pure.parts):
        raise ValueError("symbol metadata path must be normalized and relative")
    path = (manifest.parent / Path(*pure.parts)).resolve(strict=True)
    path.relative_to(manifest.parent)
    digest = _sha256(path)
    if path.stat().st_size != entry.get("size_bytes") or digest != entry.get("sha256"):
        raise ValueError("symbol metadata failed manifest digest verification")
    metadata = json.loads(path.read_text(encoding="utf-8"))
    symbol_metadata = metadata.get("symbols", {}).get(symbol)
    instrument_id = getattr(instrument, "id", None)

    if (
        not isinstance(symbol_metadata, dict)
        or str(instrument_id.symbol) != symbol
        or str(instrument_id.venue) != metadata.get("venue")
        or str(getattr(instrument, "quote_currency", "")) != symbol_metadata.get("currency")
        or getattr(instrument, "price_precision", None) != symbol_metadata.get("price_precision")
    ):
        raise ValueError("Nautilus instrument differs from strict-L2 symbol metadata")
    tick_rules = metadata.get("tick_rule")
    if not isinstance(tick_rules, list) or not tick_rules:
        raise ValueError("strict-L2 symbol metadata has no tick rules")
    allowed_ticks = {
        item.get("tick_size_x1e9") / 1_000_000_000
        for item in tick_rules
        if isinstance(item, dict) and isinstance(item.get("tick_size_x1e9"), int)
    }
    increment = getattr(instrument, "price_increment", None)
    if increment is None or not any(
        math.isclose(increment.as_double(), tick, rel_tol=0.0, abs_tol=1e-12)
        for tick in allowed_ticks
    ):
        raise ValueError("Nautilus price increment is absent from strict-L2 tick rules")
    return digest


def write_manifest_deltas_to_catalog(  # noqa: PLR0913
    manifest_path: str | Path,
    *,
    symbol: str,
    instrument: object,
    catalog_path: str | Path,
    read_batch_size: int = 65_536,
    write_batch_size: int = 65_536,
) -> dict[str, Any]:
    """
    Create one catalog without splitting equal-availability timestamps across writes.
    """
    if write_batch_size < 1:
        raise ValueError("write_batch_size must be positive")
    instrument_id = getattr(instrument, "id", None)
    if not isinstance(instrument_id, InstrumentId):
        raise TypeError("instrument must expose a Nautilus InstrumentId")
    manifest = Path(manifest_path).expanduser().resolve(strict=True)
    symbol_metadata_sha256 = _validate_instrument_metadata(
        manifest,
        symbol=symbol,
        instrument=instrument,
    )
    target = Path(catalog_path).expanduser().resolve()
    staging = target.with_name(f".{target.name}.incomplete")
    if target.exists() or staging.exists():
        raise FileExistsError("strict-L2 catalog publication never overwrites output")
    deltas = iter_manifest_deltas(
        manifest,
        symbol=symbol,
        instrument_id=instrument_id,
        batch_size=read_batch_size,
        price_precision=instrument.price_precision,
    )
    try:
        first = next(deltas)
    except StopIteration as e:
        raise ValueError("strict-L2 manifest partition contains no deltas") from e

    pending: list[OrderBookDelta] = [first]
    current_ts_init = int(first.ts_init)
    first_ts_init = current_ts_init
    last_ts_init = current_ts_init
    count = 1
    target.parent.mkdir(parents=True, exist_ok=True)
    staging.mkdir()
    catalog = ParquetDataCatalog(str(staging))
    catalog.write_instruments([instrument])

    for delta in deltas:
        ts_init = int(delta.ts_init)
        if len(pending) >= write_batch_size and ts_init != current_ts_init:
            catalog.write_order_book_deltas(pending)
            pending.clear()
        pending.append(delta)
        current_ts_init = ts_init
        last_ts_init = ts_init
        count += 1
    catalog.write_order_book_deltas(pending)
    receipt = {
        "schema_version": "strict-l2-nautilus-catalog/v2",
        "catalog_path": str(target),
        "instrument_id": str(instrument_id),
        "symbol": symbol,
        "records": count,
        "first_ts_init_ns": first_ts_init,
        "last_ts_init_ns": last_ts_init,
        "availability_tie_break_ns": AVAILABILITY_TIE_BREAK_NS,
        "source_manifest_sha256": _sha256(manifest),
        "symbol_metadata_sha256": symbol_metadata_sha256,
        "price_precision": instrument.price_precision,
        "price_increment": str(instrument.price_increment),
        "currency": str(instrument.quote_currency),
        "files": _catalog_files(staging),
    }
    (staging / "strict-l2-catalog-receipt.json").write_text(
        json.dumps(receipt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    staging.replace(target)
    return receipt
