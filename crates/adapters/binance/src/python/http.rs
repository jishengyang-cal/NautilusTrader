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

//! Python bindings for the Binance Spot HTTP client.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{identifiers::InstrumentId, python::instruments::instrument_any_to_pyobject};
use pyo3::{IntoPyObjectExt, prelude::*, types::PyList};

use crate::{common::enums::BinanceEnvironment, spot::http::client::BinanceSpotHttpClient};

#[pymethods]
impl BinanceSpotHttpClient {
    /// Creates a new Binance Spot HTTP client.
    #[new]
    #[pyo3(signature = (environment=BinanceEnvironment::Mainnet, api_key=None, api_secret=None, base_url=None, recv_window=None, timeout_secs=None, proxy_url=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            environment,
            api_key,
            api_secret,
            base_url,
            recv_window,
            timeout_secs,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    /// Returns the SBE schema ID.
    #[getter]
    #[pyo3(name = "schema_id")]
    #[must_use]
    pub fn py_schema_id(&self) -> u16 {
        Self::schema_id()
    }

    /// Returns the SBE schema version.
    #[getter]
    #[pyo3(name = "schema_version")]
    #[must_use]
    pub fn py_schema_version(&self) -> u16 {
        Self::schema_version()
    }

    /// Tests connectivity to the API.
    #[pyo3(name = "ping")]
    fn py_ping<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.ping().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

    /// Returns the server time in microseconds since epoch.
    #[pyo3(name = "server_time")]
    fn py_server_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let timestamp = client.server_time().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(timestamp.into_pyobject(py)?.into_any().unbind()))
        })
    }

    /// Requests Nautilus instruments for all trading symbols.
    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client.request_instruments().await.map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_instruments: PyResult<Vec<_>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                let pylist = PyList::new(py, py_instruments?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests recent trades for an instrument.
    #[pyo3(name = "request_trades", signature = (instrument_id, limit=None))]
    fn py_request_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trades(instrument_id, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_trades: PyResult<Vec<_>> = trades
                    .into_iter()
                    .map(|tick| tick.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_trades?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }
}
