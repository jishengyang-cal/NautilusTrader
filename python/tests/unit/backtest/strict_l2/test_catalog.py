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
Tests for immutable strict-L2 Nautilus catalog publication.
"""

import hashlib
import json
from pathlib import Path

import pandas as pd
import pytest

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.backtest.strict_l2.catalog import write_manifest_deltas_to_catalog
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import Equity
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.persistence import ParquetDataCatalog


def _manifest(tmp_path: Path) -> Path:
    rows = [
        {
            "symbol": "TEST",
            "ts_event": 1,
            "ts_recv": 2,
            "sequence": 0,
            "event_index": 0,
            "side": "N",
            "price": 0,
            "size": 0,
            "delta": 0,
            "action": "CLEAR",
            "last": True,
        },
        {
            "symbol": "TEST",
            "ts_event": 2,
            "ts_recv": 3,
            "sequence": 1,
            "event_index": 1,
            "side": "B",
            "price": 100_000_000_000,
            "size": 10,
            "delta": 10,
            "action": "SET",
            "last": True,
        },
        {
            "symbol": "TEST",
            "ts_event": 2,
            "ts_recv": 3,
            "sequence": 2,
            "event_index": 2,
            "side": "A",
            "price": 101_000_000_000,
            "size": 7,
            "delta": 7,
            "action": "SET",
            "last": True,
        },
    ]
    partition = tmp_path / "TEST.parquet"
    pd.DataFrame(rows).to_parquet(partition, index=False)
    metadata = tmp_path / "symbol_metadata.json"
    metadata.write_text(
        json.dumps(
            {
                "venue": "XNAS",
                "symbols": {"TEST": {"currency": "USD", "price_precision": 9}},
                "tick_rule": [{"price_gte_x1e9": 1_000_000_000, "tick_size_x1e9": 10_000_000}],
            },
        ),
    )
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
                    {
                        "role": "symbol_metadata",
                        "path": metadata.name,
                        "size_bytes": metadata.stat().st_size,
                        "sha256": hashlib.sha256(metadata.read_bytes()).hexdigest(),
                    },
                ],
            },
        ),
    )
    return manifest


def test_manifest_publishes_queryable_l2_catalog_without_identity(tmp_path: Path) -> None:
    """
    A verified partition and instrument become a queryable L2 catalog.
    """
    manifest = _manifest(tmp_path)
    instrument = Equity(
        instrument_id=InstrumentId(Symbol("TEST"), Venue("XNAS")),
        raw_symbol=Symbol("TEST"),
        currency=Currency.from_str("USD"),
        price_precision=9,
        price_increment=Price.from_str("0.010000000"),
        lot_size=Quantity.from_int(1),
        min_quantity=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    target = tmp_path / "catalog"
    receipt = write_manifest_deltas_to_catalog(
        manifest,
        symbol="TEST",
        instrument=instrument,
        catalog_path=target,
        read_batch_size=1,
        write_batch_size=1,
    )
    catalog = ParquetDataCatalog(str(target))
    loaded = catalog.query_order_book_deltas([str(instrument.id)])
    replayed = catalog.query_order_book_deltas([str(instrument.id)])
    assert receipt["records"] == 3
    assert receipt["first_ts_init_ns"] == 3
    assert receipt["last_ts_init_ns"] == 4
    assert receipt["availability_tie_break_ns"] == 1
    assert [delta.action for delta in loaded] == [
        BookAction.CLEAR,
        BookAction.ADD,
        BookAction.ADD,
    ]
    assert [delta.sequence for delta in loaded] == [0, 1, 2]
    assert replayed == loaded
    assert loaded[0].flags == RecordFlag.F_SNAPSHOT.value | RecordFlag.F_LAST.value
    assert all(delta.order.order_id == 0 for delta in loaded[1:])
    assert not target.with_name(".catalog.incomplete").exists()

    with pytest.raises(FileExistsError, match="never overwrites"):
        write_manifest_deltas_to_catalog(
            manifest,
            symbol="TEST",
            instrument=instrument,
            catalog_path=target,
        )

    config = BacktestRunConfig(
        venues=[
            BacktestVenueConfig(
                name="XNAS",
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                starting_balances=["1_000_000 USD"],
                book_type=BookType.L2_MBP,
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type="OrderBookDelta",
                catalog_path=str(target),
                instrument_id=instrument.id,
            ),
        ],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
    )
    node = BacktestNode([config])
    try:
        assert len(node.run()) == 1
    finally:
        node.dispose()


def test_catalog_failure_removes_staging_for_corrected_retry(tmp_path: Path) -> None:
    """
    A failed streamed publication leaves its catalog path retryable.
    """
    manifest = _manifest(tmp_path)
    partition = tmp_path / "TEST.parquet"
    rows = pd.read_parquet(partition)
    rows.loc[2, "event_index"] = 1
    rows.to_parquet(partition, index=False)
    payload = json.loads(manifest.read_text())
    partition_entry = next(item for item in payload["files"] if item["role"] == "l2_deltas")
    partition_entry["size_bytes"] = partition.stat().st_size
    partition_entry["sha256"] = hashlib.sha256(partition.read_bytes()).hexdigest()
    manifest.write_text(json.dumps(payload))
    instrument = Equity(
        instrument_id=InstrumentId(Symbol("TEST"), Venue("XNAS")),
        raw_symbol=Symbol("TEST"),
        currency=Currency.from_str("USD"),
        price_precision=9,
        price_increment=Price.from_str("0.010000000"),
        lot_size=Quantity.from_int(1),
        min_quantity=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    target = tmp_path / "catalog"

    with pytest.raises(ValueError, match="event_index must be contiguous"):
        write_manifest_deltas_to_catalog(
            manifest,
            symbol="TEST",
            instrument=instrument,
            catalog_path=target,
            read_batch_size=1,
            write_batch_size=1,
        )
    assert not target.with_name(".catalog.incomplete").exists()

    rows.loc[2, "event_index"] = 2
    rows.to_parquet(partition, index=False)
    partition_entry["size_bytes"] = partition.stat().st_size
    partition_entry["sha256"] = hashlib.sha256(partition.read_bytes()).hexdigest()
    manifest.write_text(json.dumps(payload))

    receipt = write_manifest_deltas_to_catalog(
        manifest,
        symbol="TEST",
        instrument=instrument,
        catalog_path=target,
        read_batch_size=1,
        write_batch_size=1,
    )

    assert receipt["records"] == 3
    assert target.exists()
