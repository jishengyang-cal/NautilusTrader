// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use pyo3::{prelude::*, types::PyType};

use crate::data::greeks::{
    BlackScholesGreeksResult, GreeksData, OptionGreekValues, PortfolioGreeks, black_scholes_greeks,
    imply_vol, refine_vol_and_greeks,
};

fn check_finite(value: f64, parameter: &str) -> PyResult<()> {
    if !value.is_finite() {
        return Err(to_pyvalue_err(format!(
            "{parameter} must be finite, was {value}"
        )));
    }
    Ok(())
}

fn check_positive_finite(value: f64, parameter: &str) -> PyResult<()> {
    check_finite(value, parameter)?;
    if value <= 0.0 {
        return Err(to_pyvalue_err(format!(
            "{parameter} must be positive, was {value}"
        )));
    }
    Ok(())
}

fn check_option_inputs(s: f64, r: f64, b: f64, k: f64, t: f64) -> PyResult<()> {
    check_positive_finite(s, "s")?;
    check_finite(r, "r")?;
    check_finite(b, "b")?;
    check_positive_finite(k, "k")?;
    check_positive_finite(t, "t")
}

#[expect(clippy::too_many_arguments)]
fn check_option_price(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
    parameter: &str,
) -> PyResult<()> {
    check_positive_finite(price, parameter)?;
    let discounted_spot = s * ((b - r) * t).exp();
    let discounted_strike = k * (-r * t).exp();
    check_positive_finite(discounted_spot, "discounted spot")?;
    check_positive_finite(discounted_strike, "discounted strike")?;
    let (lower_bound, upper_bound) = if is_call {
        (
            (discounted_spot - discounted_strike).max(0.0),
            discounted_spot,
        )
    } else {
        (
            (discounted_strike - discounted_spot).max(0.0),
            discounted_strike,
        )
    };

    if price <= lower_bound || price >= upper_bound {
        return Err(to_pyvalue_err(format!(
            "{parameter} violates no-arbitrage bounds ({lower_bound}, {upper_bound}), was {price}"
        )));
    }
    Ok(())
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OptionGreekValues {
    #[getter]
    fn delta(&self) -> f64 {
        self.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.theta
    }

    #[getter]
    fn rho(&self) -> f64 {
        self.rho
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl GreeksData {
    #[classmethod]
    #[pyo3(name = "from_delta", signature = (instrument_id, delta, multiplier, ts_event=0))]
    fn py_from_delta(
        _cls: &Bound<'_, PyType>,
        instrument_id: crate::identifiers::InstrumentId,
        delta: f64,
        multiplier: f64,
        ts_event: u64,
    ) -> Self {
        Self::from_delta(instrument_id, delta, multiplier, UnixNanos::from(ts_event))
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn instrument_id(&self) -> crate::identifiers::InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn is_call(&self) -> bool {
        self.is_call
    }

    #[getter]
    fn strike(&self) -> f64 {
        self.strike
    }

    #[getter]
    fn expiry(&self) -> i32 {
        self.expiry
    }

    #[getter]
    fn expiry_in_days(&self) -> i32 {
        self.expiry_in_days
    }

    #[getter]
    fn expiry_in_years(&self) -> f64 {
        self.expiry_in_years
    }

    #[getter]
    fn multiplier(&self) -> f64 {
        self.multiplier
    }

    #[getter]
    fn quantity(&self) -> f64 {
        self.quantity
    }

    #[getter]
    fn underlying_price(&self) -> f64 {
        self.underlying_price
    }

    #[getter]
    fn interest_rate(&self) -> f64 {
        self.interest_rate
    }

    #[getter]
    fn cost_of_carry(&self) -> f64 {
        self.cost_of_carry
    }

    #[getter]
    fn vol(&self) -> f64 {
        self.vol
    }

    #[getter]
    fn pnl(&self) -> f64 {
        self.pnl
    }

    #[getter]
    fn price(&self) -> f64 {
        self.price
    }

    #[getter]
    fn delta(&self) -> f64 {
        self.greeks.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.greeks.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.greeks.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.greeks.theta
    }

    #[getter]
    fn rho(&self) -> f64 {
        self.greeks.rho
    }

    #[getter]
    fn itm_prob(&self) -> f64 {
        self.itm_prob
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PortfolioGreeks {
    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn pnl(&self) -> f64 {
        self.pnl
    }

    #[getter]
    fn price(&self) -> f64 {
        self.price
    }

    #[getter]
    fn delta(&self) -> f64 {
        self.greeks.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.greeks.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.greeks.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.greeks.theta
    }

    #[getter]
    fn rho(&self) -> f64 {
        self.greeks.rho
    }
}

#[cfg(feature = "python")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BlackScholesGreeksResult {
    #[getter]
    fn price(&self) -> f64 {
        self.price
    }

    #[getter]
    fn vol(&self) -> f64 {
        self.vol
    }

    #[getter]
    fn delta(&self) -> f64 {
        self.delta
    }

    #[getter]
    fn gamma(&self) -> f64 {
        self.gamma
    }

    #[getter]
    fn vega(&self) -> f64 {
        self.vega
    }

    #[getter]
    fn theta(&self) -> f64 {
        self.theta
    }

    #[getter]
    fn itm_prob(&self) -> f64 {
        self.itm_prob
    }
}

/// Computes Black-Scholes greeks using the fast `compute_greeks` implementation.
///
/// # Errors
///
/// Returns a `PyValueError` if an input is non-finite or outside its positive domain.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "black_scholes_greeks")]
pub fn py_black_scholes_greeks(
    s: f64,
    r: f64,
    b: f64,
    vol: f64,
    is_call: bool,
    k: f64,
    t: f64,
) -> PyResult<BlackScholesGreeksResult> {
    check_option_inputs(s, r, b, k, t)?;
    check_positive_finite(vol, "vol")?;
    Ok(black_scholes_greeks(s, r, b, vol, is_call, k, t))
}

/// Computes the implied volatility for an option given its parameters and market price.
///
/// # Errors
///
/// Returns a `PyValueError` if an input is non-finite, outside its positive domain, or violates the
/// generalized Black-Scholes no-arbitrage price bounds.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "imply_vol")]
pub fn py_imply_vol(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
) -> PyResult<f64> {
    check_option_inputs(s, r, b, k, t)?;
    check_option_price(s, r, b, is_call, k, t, price, "price")?;
    let vol = imply_vol(s, r, b, is_call, k, t, price);
    check_positive_finite(vol, "implied volatility")?;
    Ok(vol)
}

/// Computes implied volatility and greeks using the fast implementations.
///
/// # Errors
///
/// Returns a `PyValueError` if an input is non-finite, outside its positive domain, or violates the
/// generalized Black-Scholes no-arbitrage price bounds.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "imply_vol_and_greeks")]
pub fn py_imply_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    price: f64,
) -> PyResult<BlackScholesGreeksResult> {
    check_option_inputs(s, r, b, k, t)?;
    check_option_price(s, r, b, is_call, k, t, price, "price")?;
    let vol = imply_vol(s, r, b, is_call, k, t, price);
    check_positive_finite(vol, "implied volatility")?;
    Ok(black_scholes_greeks(s, r, b, vol, is_call, k, t))
}

/// Refines implied volatility using an initial guess and computes greeks.
///
/// # Errors
///
/// Returns a `PyValueError` if an input is non-finite, outside its positive domain, or violates the
/// generalized Black-Scholes no-arbitrage price bounds.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[pyo3(name = "refine_vol_and_greeks")]
#[expect(clippy::too_many_arguments)]
pub fn py_refine_vol_and_greeks(
    s: f64,
    r: f64,
    b: f64,
    is_call: bool,
    k: f64,
    t: f64,
    target_price: f64,
    initial_vol: f64,
) -> PyResult<BlackScholesGreeksResult> {
    check_option_inputs(s, r, b, k, t)?;
    check_option_price(s, r, b, is_call, k, t, target_price, "target_price")?;
    check_positive_finite(initial_vol, "initial_vol")?;
    Ok(refine_vol_and_greeks(
        s,
        r,
        b,
        is_call,
        k,
        t,
        target_price,
        initial_vol,
    ))
}
