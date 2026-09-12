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
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied. See the License for the
#  specific language governing permissions and limitations under the License.
# -------------------------------------------------------------------------------------------------
"""Validate Interactive Brokers client configuration without connecting or placing orders."""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from typing import Any

from nautilus_trader.adapters import interactive_brokers
from nautilus_trader.model import InstrumentId


@dataclass(frozen=True)
class InteractiveBrokersConnectionSpec:
    """Define the inputs for an offline data and execution client configuration check."""

    host: str
    port: int
    data_client_id: int
    execution_client_id: int
    load_ids: tuple[str, ...]
    account_id: str | None = None
    build_options_chain: bool = False
    min_expiry_days: int = 7
    max_expiry_days: int = 90

    def __post_init__(self) -> None:
        if not self.host.strip():
            raise ValueError("host must be non-empty")
        if not 1 <= self.port <= 65535:
            raise ValueError("port must be between 1 and 65535")
        if self.data_client_id == self.execution_client_id:
            raise ValueError("data and execution client IDs must be distinct")
        if self.execution_client_id % 1000 == 0:
            raise ValueError("execution client ID must not be a multiple of 1000")
        if not self.load_ids:
            raise ValueError("at least one instrument ID is required")
        if self.min_expiry_days < 0 or self.max_expiry_days < self.min_expiry_days:
            raise ValueError("option expiry range is invalid")


def build_configs(spec: InteractiveBrokersConnectionSpec) -> dict[str, Any]:
    """Build the public provider, data and execution client configurations."""
    provider = interactive_brokers.InteractiveBrokersInstrumentProviderConfig(
        symbology_method=interactive_brokers.SymbologyMethod.SIMPLIFIED,
        load_ids={InstrumentId.from_str(value) for value in spec.load_ids},
        build_options_chain=spec.build_options_chain,
        min_expiry_days=spec.min_expiry_days,
        max_expiry_days=spec.max_expiry_days,
        cache_validity_days=1,
    )
    data = interactive_brokers.InteractiveBrokersDataClientConfig(
        host=spec.host,
        port=spec.port,
        client_id=spec.data_client_id,
        connection_timeout=15,
        request_timeout=60,
        market_data_type=interactive_brokers.MarketDataType.REALTIME,
        instrument_provider=provider,
    )
    execution = interactive_brokers.InteractiveBrokersExecutionClientConfig(
        host=spec.host,
        port=spec.port,
        client_id=spec.execution_client_id,
        account_id=spec.account_id,
        connection_timeout=15,
        request_timeout=60,
        fetch_all_open_orders=False,
        track_option_exercise_from_position_update=True,
        instrument_provider=provider,
    )
    return {"provider": provider, "data": data, "execution": execution}


def config_summary(spec: InteractiveBrokersConnectionSpec) -> dict[str, Any]:
    """Return a secret-free configuration summary."""
    return {
        "host": spec.host,
        "port": spec.port,
        "data_client_id": spec.data_client_id,
        "execution_client_id": spec.execution_client_id,
        "account_id_configured": bool(spec.account_id),
        "load_ids": list(spec.load_ids),
        "build_options_chain": spec.build_options_chain,
        "min_expiry_days": spec.min_expiry_days,
        "max_expiry_days": spec.max_expiry_days,
        "execution_enabled_in_config": True,
    }


def main() -> int:
    """Validate command-line inputs and construct configs without opening a connection."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--data-client-id", type=int, required=True)
    parser.add_argument("--execution-client-id", type=int, required=True)
    parser.add_argument("--account-id")
    parser.add_argument("--load-id", action="append", required=True)
    parser.add_argument("--options", action="store_true")
    args = parser.parse_args()
    spec = InteractiveBrokersConnectionSpec(
        host=args.host,
        port=args.port,
        data_client_id=args.data_client_id,
        execution_client_id=args.execution_client_id,
        account_id=args.account_id,
        load_ids=tuple(dict.fromkeys(args.load_id)),
        build_options_chain=args.options,
    )
    configs = build_configs(spec)
    print(json.dumps({"valid": bool(configs), **config_summary(spec)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
