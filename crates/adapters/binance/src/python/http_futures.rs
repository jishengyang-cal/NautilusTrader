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

//! Python bindings for the Binance Futures HTTP client.

use chrono::{TimeZone, Utc};
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
    python::instruments::instrument_any_to_pyobject,
};
use pyo3::{prelude::*, types::PyList};

use crate::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    futures::http::client::BinanceFuturesHttpClient,
};

#[pymethods]
impl BinanceFuturesHttpClient {
    #[new]
    #[pyo3(signature = (
        product_type,
        environment=BinanceEnvironment::Mainnet,
        api_key=None,
        api_secret=None,
        base_url=None,
        recv_window=None,
        timeout_secs=None,
        proxy_url=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        product_type: BinanceProductType,
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            product_type,
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

    #[getter]
    #[pyo3(name = "product_type")]
    #[must_use]
    pub fn py_product_type(&self) -> BinanceProductType {
        self.product_type()
    }

    #[pyo3(name = "server_time")]
    fn py_server_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let timestamp = client.server_time().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(timestamp.server_time.into_pyobject(py)?.into_any().unbind()))
        })
    }

    #[pyo3(name = "query_hedge_mode")]
    fn py_query_hedge_mode<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.query_hedge_mode().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| {
                Ok(response
                    .dual_side_position
                    .into_pyobject(py)?
                    .to_owned()
                    .into_any()
                    .unbind())
            })
        })
    }

    #[pyo3(name = "create_listen_key")]
    fn py_create_listen_key<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.create_listen_key().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(response.listen_key.into_pyobject(py)?.into_any().unbind()))
        })
    }

    #[pyo3(name = "keepalive_listen_key")]
    fn py_keepalive_listen_key<'py>(
        &self,
        py: Python<'py>,
        listen_key: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .keepalive_listen_key(&listen_key)
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(name = "close_listen_key")]
    fn py_close_listen_key<'py>(
        &self,
        py: Python<'py>,
        listen_key: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .close_listen_key(&listen_key)
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

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

    #[pyo3(name = "request_account_state")]
    fn py_request_account_state<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_state = client
                .request_account_state(account_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(account_state.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, limit=None))]
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
                    .map(|t| Ok(t.into_py_any_unwrap(py)))
                    .collect();
                let pylist = PyList::new(py, py_trades?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        let start_dt = start
            .map(|ts| {
                Utc.timestamp_millis_opt(ts).single().ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err(format!(
                        "Invalid start timestamp: {ts}"
                    ))
                })
            })
            .transpose()?;

        let end_dt = end
            .map(|ts| {
                Utc.timestamp_millis_opt(ts).single().ok_or_else(|| {
                    pyo3::exceptions::PyValueError::new_err(format!("Invalid end timestamp: {ts}"))
                })
            })
            .transpose()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(bar_type, start_dt, end_dt, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_bars: PyResult<Vec<_>> = bars
                    .into_iter()
                    .map(|b| Ok(b.into_py_any_unwrap(py)))
                    .collect();
                let pylist = PyList::new(py, py_bars?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }
}
