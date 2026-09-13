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
Independent normal-CDF and finite-difference checks of native option Greeks.
"""

from math import erf
from math import exp
from math import log
from math import sqrt

import pytest

from nautilus_trader.model import black_scholes_greeks
from nautilus_trader.model import imply_vol_and_greeks


def _reference_price(s, k, t, r, b, vol, is_call):
    # Independent test oracle only, never used by runtime pricing
    sign = 1.0 if is_call else -1.0
    d1 = (log(s / k) + (b + vol * vol / 2) * t) / (vol * sqrt(t))
    d2 = d1 - vol * sqrt(t)

    def cdf(x):
        return (1 + erf(x / sqrt(2))) / 2

    return sign * (s * exp((b - r) * t) * cdf(sign * d1) - k * exp(-r * t) * cdf(sign * d2))


@pytest.mark.parametrize("is_call", [True, False])
@pytest.mark.parametrize(
    ("s", "k", "t", "r", "b", "vol"),
    [
        (100.0, 100.0, 1.0, 0.05, 0.05, 0.2),
        (90.0, 100.0, 0.25, -0.01, -0.03, 0.4),
        (110.0, 100.0, 0.02, 0.03, 0.01, 0.6),
    ],
)
def test_native_greeks_units_and_full_iv_round_trip(s, k, t, r, b, vol, is_call) -> None:
    """
    Check native Greek units and full IV recovery against an independent price oracle.
    """
    actual = black_scholes_greeks(s, r, b, vol, is_call, k, t)
    price = _reference_price(s, k, t, r, b, vol, is_call)
    assert actual.price == pytest.approx(price, abs=3e-5)
    spot_step, vol_step, time_step = 0.01, 0.0001, 0.00001
    up = _reference_price(s + spot_step, k, t, r, b, vol, is_call)
    down = _reference_price(s - spot_step, k, t, r, b, vol, is_call)
    assert actual.delta == pytest.approx((up - down) / (2 * spot_step), abs=3e-6)
    assert actual.gamma == pytest.approx((up - 2 * price + down) / spot_step**2, abs=3e-6)
    vega = (
        _reference_price(s, k, t, r, b, vol + vol_step, is_call)
        - _reference_price(s, k, t, r, b, vol - vol_step, is_call)
    ) / (2 * vol_step)
    theta = (
        _reference_price(s, k, t - time_step, r, b, vol, is_call)
        - _reference_price(s, k, t + time_step, r, b, vol, is_call)
    ) / (2 * time_step)
    assert actual.vega == pytest.approx(vega * 0.01, abs=3e-6)
    assert actual.theta == pytest.approx(theta / 365.25, abs=3e-6)
    recovered = imply_vol_and_greeks(s, r, b, is_call, k, t, price)
    assert recovered.vol == pytest.approx(vol, abs=1e-6)
    assert recovered.price == pytest.approx(price, abs=3e-5)


def test_native_put_call_parity_with_dividend_carry() -> None:
    """
    Check discounted put-call parity with dividend carry.
    """
    call = black_scholes_greeks(100.0, 0.04, 0.01, 0.3, True, 105.0, 0.7)
    put = black_scholes_greeks(100.0, 0.04, 0.01, 0.3, False, 105.0, 0.7)
    assert call.price - put.price == pytest.approx(
        100 * exp(-0.03 * 0.7) - 105 * exp(-0.04 * 0.7),
        abs=3e-5,
    )
