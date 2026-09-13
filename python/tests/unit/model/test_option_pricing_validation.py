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
"""Independent normal-CDF and finite-difference checks of native option Greeks."""

from math import erf, exp, log, sqrt

import pytest

from nautilus_trader.model import black_scholes_greeks
from nautilus_trader.model import imply_vol_and_greeks


def _reference_price(s, k, t, r, b, vol, is_call):
    # Independent test oracle only, never used by runtime pricing
    sign = 1.0 if is_call else -1.0
    d1 = (log(s / k) + (b + vol * vol / 2) * t) / (vol * sqrt(t))
    d2 = d1 - vol * sqrt(t)
    cdf = lambda x: (1 + erf(x / sqrt(2))) / 2
    return sign * (s * exp((b - r) * t) * cdf(sign * d1)
                   - k * exp(-r * t) * cdf(sign * d2))


@pytest.mark.parametrize("is_call", [True, False])
@pytest.mark.parametrize("s,k,t,r,b,vol", [
    (100., 100., 1., .05, .05, .2),
    (90., 100., .25, -.01, -.03, .4),
    (110., 100., .02, .03, .01, .6),
])
def test_native_greeks_units_and_full_iv_round_trip(s, k, t, r, b, vol, is_call):
    actual = black_scholes_greeks(s, r, b, vol, is_call, k, t)
    price = _reference_price(s, k, t, r, b, vol, is_call)
    assert actual.price == pytest.approx(price, abs=3e-5)
    spot_step, vol_step, time_step = .01, .0001, .00001
    up = _reference_price(s + spot_step, k, t, r, b, vol, is_call)
    down = _reference_price(s - spot_step, k, t, r, b, vol, is_call)
    assert actual.delta == pytest.approx((up - down) / (2 * spot_step), abs=3e-6)
    assert actual.gamma == pytest.approx((up - 2 * price + down) / spot_step**2, abs=3e-6)
    vega = (_reference_price(s, k, t, r, b, vol + vol_step, is_call)
            - _reference_price(s, k, t, r, b, vol - vol_step, is_call)) / (2 * vol_step)
    theta = (_reference_price(s, k, t - time_step, r, b, vol, is_call)
             - _reference_price(s, k, t + time_step, r, b, vol, is_call)) / (2 * time_step)
    assert actual.vega == pytest.approx(vega * .01, abs=3e-6)
    assert actual.theta == pytest.approx(theta / 365.25, abs=3e-6)
    recovered = imply_vol_and_greeks(s, r, b, is_call, k, t, price)
    assert recovered.vol == pytest.approx(vol, abs=1e-6)
    assert recovered.price == pytest.approx(price, abs=3e-5)


def test_native_put_call_parity_with_dividend_carry():
    call = black_scholes_greeks(100., .04, .01, .3, True, 105., .7)
    put = black_scholes_greeks(100., .04, .01, .3, False, 105., .7)
    assert call.price - put.price == pytest.approx(
        100 * exp(-.03 * .7) - 105 * exp(-.04 * .7), abs=3e-5,
    )
