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
Strict price-level L2 ingestion and research feedback boundaries.
"""

from nautilus_trader.backtest.strict_l2.catalog import write_manifest_deltas_to_catalog
from nautilus_trader.backtest.strict_l2.data import iter_manifest_deltas
from nautilus_trader.backtest.strict_l2.data import rows_to_deltas
from nautilus_trader.backtest.strict_l2.feedback import feedback_records_from_orders_report
from nautilus_trader.backtest.strict_l2.feedback import publish_execution_feedback


__all__ = [
    "feedback_records_from_orders_report",
    "iter_manifest_deltas",
    "publish_execution_feedback",
    "rows_to_deltas",
    "write_manifest_deltas_to_catalog",
]
