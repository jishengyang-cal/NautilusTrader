# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautilustrader.io.
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
# -------------------------------------------------------------------------------------------------
"""
Publish one strict-L2 manifest partition as an immutable Nautilus catalog.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from pathlib import PurePosixPath

from nautilus_trader.backtest.strict_l2.catalog import write_manifest_deltas_to_catalog
from nautilus_trader.model import Currency
from nautilus_trader.model import Equity
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--symbol", required=True)
    parser.add_argument("--price-increment", required=True)
    parser.add_argument("--catalog", required=True)
    parser.add_argument("--read-batch-size", type=int, default=65_536)
    parser.add_argument("--write-batch-size", type=int, default=65_536)
    return parser.parse_args(argv)


def _equity(manifest_path: str, symbol: str, price_increment: str) -> Equity:
    manifest = Path(manifest_path).expanduser().resolve(strict=True)
    value = json.loads(manifest.read_text(encoding="utf-8"))
    entries = [
        item
        for item in value.get("files", [])
        if isinstance(item, dict) and item.get("role") == "symbol_metadata"
    ]

    if len(entries) != 1:
        raise ValueError("manifest must bind exactly one symbol metadata file")
    entry = entries[0]
    pure = PurePosixPath(str(entry.get("path")))
    if pure.is_absolute() or not pure.parts or any(part in {"", ".", ".."} for part in pure.parts):
        raise ValueError("symbol metadata path must be normalized and relative")
    metadata_path = (manifest.parent / Path(*pure.parts)).resolve(strict=True)
    metadata_path.relative_to(manifest.parent)
    raw = metadata_path.read_bytes()
    if len(raw) != entry.get("size_bytes") or hashlib.sha256(raw).hexdigest() != entry.get(
        "sha256",
    ):
        raise ValueError("symbol metadata failed manifest digest verification")
    metadata = json.loads(raw)
    details = metadata.get("symbols", {}).get(symbol)
    if details is None:
        raise ValueError("symbol is absent from strict-L2 metadata")
    if not isinstance(details, dict):
        raise TypeError("strict-L2 symbol metadata must be an object")
    venue = metadata.get("venue")
    if not isinstance(venue, str) or not venue:
        raise ValueError("strict-L2 metadata venue is missing")
    return Equity(
        instrument_id=InstrumentId(Symbol(symbol), Venue(venue)),
        raw_symbol=Symbol(symbol),
        currency=Currency.from_str(details["currency"]),
        price_precision=details["price_precision"],
        price_increment=Price.from_str(price_increment),
        lot_size=Quantity.from_int(1),
        min_quantity=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )


def main(argv: list[str] | None = None) -> int:
    """
    Build the requested immutable catalog and print its receipt.
    """
    args = _parse_args(argv)
    receipt = write_manifest_deltas_to_catalog(
        args.manifest,
        symbol=args.symbol,
        instrument=_equity(args.manifest, args.symbol, args.price_increment),
        catalog_path=args.catalog,
        read_batch_size=args.read_batch_size,
        write_batch_size=args.write_batch_size,
    )
    sys.stdout.write(
        json.dumps(
            {
                "status": "complete",
                "catalog_path": receipt["catalog_path"],
                "instrument_id": receipt["instrument_id"],
                "records": receipt["records"],
                "source_manifest_sha256": receipt["source_manifest_sha256"],
            },
            sort_keys=True,
        )
        + "\n",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
