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

//! Provides the HTTP client for the Coinbase Advanced Trade REST API.
//!
//! Two-layer architecture:
//! - [`CoinbaseRawHttpClient`]: low-level endpoint methods, JWT auth, rate limiting.
//! - [`CoinbaseHttpClient`]: domain wrapper with instrument caching and Nautilus type conversions.

use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

use nautilus_core::{
    AtomicMap, UnixNanos,
    consts::NAUTILUS_USER_AGENT,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{identifiers::InstrumentId, instruments::InstrumentAny};
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpResponse, Method, USER_AGENT},
    ratelimiter::quota::Quota,
};
use serde_json::Value;

use crate::{
    common::{
        consts::REST_API_PATH, credential::CoinbaseCredential, enums::CoinbaseEnvironment, urls,
    },
    http::error::{Error, Result},
};

// Coinbase Advanced Trade rate limit: 30 requests per second
fn default_quota() -> Option<Quota> {
    Quota::per_second(NonZeroU32::new(30).unwrap()) // Infallible: 30 is non-zero
}

/// Provides a raw HTTP client for low-level Coinbase Advanced Trade REST API operations.
///
/// Handles JWT authentication, request construction, and response parsing.
/// Each request generates a fresh ES256 JWT for authentication.
#[derive(Debug, Clone)]
pub struct CoinbaseRawHttpClient {
    client: HttpClient,
    credential: Option<CoinbaseCredential>,
    base_url: String,
    environment: CoinbaseEnvironment,
}

impl CoinbaseRawHttpClient {
    /// Creates a new [`CoinbaseRawHttpClient`] for public endpoints only.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                default_quota(),
                Some(timeout_secs),
                proxy_url,
            )?,
            credential: None,
            base_url: urls::rest_url(environment).to_string(),
            environment,
        })
    }

    /// Creates a new [`CoinbaseRawHttpClient`] with credentials for authenticated requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_credentials(
        credential: CoinbaseCredential,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                default_quota(),
                Some(timeout_secs),
                proxy_url,
            )?,
            credential: Some(credential),
            base_url: urls::rest_url(environment).to_string(),
            environment,
        })
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables are not set.
    pub fn from_env(environment: CoinbaseEnvironment) -> Result<Self> {
        let credential = CoinbaseCredential::from_env()
            .map_err(|e| Error::auth(format!("Missing credentials in environment: {e}")))?;
        Self::with_credentials(credential, environment, 10, None)
            .map_err(|e| Error::auth(format!("Failed to create HTTP client: {e}")))
    }

    /// Creates a new [`CoinbaseRawHttpClient`] with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if credentials are invalid.
    pub fn from_credentials(
        api_key: &str,
        api_secret: &str,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> Result<Self> {
        let credential = CoinbaseCredential::new(api_key.to_string(), api_secret.to_string());
        Self::with_credentials(credential, environment, timeout_secs, proxy_url)
            .map_err(|e| Error::auth(format!("Failed to create HTTP client: {e}")))
    }

    /// Overrides the base REST URL (for testing with mock servers).
    pub fn set_base_url(&mut self, url: String) {
        self.base_url = url;
    }

    /// Returns the configured environment.
    #[must_use]
    pub fn environment(&self) -> CoinbaseEnvironment {
        self.environment
    }

    /// Returns true if this client has credentials for authenticated requests.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.credential.is_some()
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ])
    }

    fn build_url(&self, path: &str) -> String {
        format!("{}{REST_API_PATH}{path}", self.base_url)
    }

    // JWT uri claim must match the actual request host
    fn build_jwt_uri(&self, method: &str, path: &str) -> String {
        let host = self
            .base_url
            .strip_prefix("https://")
            .or_else(|| self.base_url.strip_prefix("http://"))
            .unwrap_or(&self.base_url);
        format!("{method} {host}{REST_API_PATH}{path}")
    }

    fn auth_headers(&self, method: &str, path: &str) -> Result<HashMap<String, String>> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("No credentials configured"))?;

        let uri = self.build_jwt_uri(method, path);
        let jwt = credential.build_rest_jwt(&uri)?;

        Ok(HashMap::from([(
            "Authorization".to_string(),
            format!("Bearer {jwt}"),
        )]))
    }

    fn parse_response(&self, response: &HttpResponse) -> Result<Value> {
        if !response.status.is_success() {
            return Err(Error::from_http_status(
                response.status.as_u16(),
                &response.body,
            ));
        }

        if response.body.is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_slice(&response.body).map_err(Error::Serde)
    }

    /// Sends a GET request to a public endpoint (no auth required).
    pub async fn get_public(&self, path: &str) -> Result<Value> {
        let url = self.build_url(path);
        let response = self
            .client
            .request(Method::GET, url, None, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        self.parse_response(&response)
    }

    /// Sends a GET request with query parameters to a public endpoint.
    pub async fn get_public_with_query(&self, path: &str, query: &str) -> Result<Value> {
        let full_path = if query.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{query}")
        };
        let url = self.build_url(&full_path);
        let response = self
            .client
            .request(Method::GET, url, None, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        self.parse_response(&response)
    }

    /// Sends an authenticated GET request.
    pub async fn get(&self, path: &str) -> Result<Value> {
        let url = self.build_url(path);
        let headers = self.auth_headers("GET", path)?;
        let response = self
            .client
            .request(Method::GET, url, None, Some(headers), None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        self.parse_response(&response)
    }

    /// Sends an authenticated GET request with query parameters appended to the path.
    pub async fn get_with_query(&self, path: &str, query: &str) -> Result<Value> {
        let full_path = if query.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{query}")
        };
        let url = self.build_url(&full_path);
        let headers = self.auth_headers("GET", &full_path)?;
        let response = self
            .client
            .request(Method::GET, url, None, Some(headers), None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        self.parse_response(&response)
    }

    /// Sends an authenticated POST request with a JSON body.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = self.build_url(path);
        let headers = self.auth_headers("POST", path)?;
        let body_bytes = serde_json::to_vec(body).map_err(Error::Serde)?;
        let response = self
            .client
            .request(
                Method::POST,
                url,
                None,
                Some(headers),
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;

        self.parse_response(&response)
    }

    /// Sends an authenticated DELETE request.
    pub async fn delete(&self, path: &str) -> Result<Value> {
        let url = self.build_url(path);
        let headers = self.auth_headers("DELETE", path)?;
        let response = self
            .client
            .request(Method::DELETE, url, None, Some(headers), None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        self.parse_response(&response)
    }

    /// Gets all available products.
    pub async fn get_products(&self) -> Result<Value> {
        self.get_public("/products").await
    }

    /// Gets a specific product by ID.
    pub async fn get_product(&self, product_id: &str) -> Result<Value> {
        self.get_public(&format!("/products/{product_id}")).await
    }

    /// Gets candles for a product.
    pub async fn get_candles(
        &self,
        product_id: &str,
        start: &str,
        end: &str,
        granularity: &str,
    ) -> Result<Value> {
        let query = format!("start={start}&end={end}&granularity={granularity}");
        self.get_public_with_query(&format!("/products/{product_id}/candles"), &query)
            .await
    }

    /// Gets market trades for a product.
    pub async fn get_market_trades(&self, product_id: &str, limit: u32) -> Result<Value> {
        let query = format!("limit={limit}");
        self.get_public_with_query(&format!("/products/{product_id}/ticker"), &query)
            .await
    }

    /// Gets best bid/ask for one or more products.
    pub async fn get_best_bid_ask(&self, product_ids: &[&str]) -> Result<Value> {
        let query = product_ids
            .iter()
            .map(|id| format!("product_ids={id}"))
            .collect::<Vec<_>>()
            .join("&");
        self.get_public_with_query("/best_bid_ask", &query).await
    }

    /// Gets the product order book.
    pub async fn get_product_book(&self, product_id: &str, limit: Option<u32>) -> Result<Value> {
        let mut query = format!("product_id={product_id}");

        if let Some(limit) = limit {
            query.push_str(&format!("&limit={limit}"));
        }
        self.get_public_with_query("/product_book", &query).await
    }

    /// Gets all accounts.
    pub async fn get_accounts(&self) -> Result<Value> {
        self.get("/accounts").await
    }

    /// Gets a specific account by UUID.
    pub async fn get_account(&self, account_id: &str) -> Result<Value> {
        self.get(&format!("/accounts/{account_id}")).await
    }

    /// Creates a new order.
    pub async fn create_order(&self, order: &Value) -> Result<Value> {
        self.post("/orders", order).await
    }

    /// Cancels orders by order IDs.
    pub async fn cancel_orders(&self, order_ids: &[String]) -> Result<Value> {
        let body = serde_json::json!({ "order_ids": order_ids });
        self.post("/orders/batch_cancel", &body).await
    }

    /// Gets historical orders.
    pub async fn get_orders(&self, query: &str) -> Result<Value> {
        self.get_with_query("/orders/historical/batch", query).await
    }

    /// Gets a specific order by ID.
    pub async fn get_order(&self, order_id: &str) -> Result<Value> {
        self.get(&format!("/orders/historical/{order_id}")).await
    }

    /// Gets fills (trade executions).
    pub async fn get_fills(&self, query: &str) -> Result<Value> {
        self.get_with_query("/orders/historical/fills", query).await
    }

    /// Gets fee transaction summary.
    pub async fn get_transaction_summary(&self) -> Result<Value> {
        self.get("/transaction_summary").await
    }
}

/// Provides a domain-level HTTP client for the Coinbase Advanced Trade API.
///
/// Wraps [`CoinbaseRawHttpClient`] in an `Arc` and adds instrument caching
/// and Nautilus type conversions. This is the primary HTTP interface for the
/// data and execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseHttpClient {
    pub(crate) inner: Arc<CoinbaseRawHttpClient>,
    clock: &'static AtomicTime,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
}

impl Default for CoinbaseHttpClient {
    fn default() -> Self {
        Self::new(CoinbaseEnvironment::Live, 10, None)
            .expect("Failed to create default Coinbase HTTP client")
    }
}

impl CoinbaseHttpClient {
    /// Creates a new [`CoinbaseHttpClient`] for public endpoints only.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        let raw = CoinbaseRawHttpClient::new(environment, timeout_secs, proxy_url)?;
        Ok(Self::from_raw(raw))
    }

    /// Creates a new [`CoinbaseHttpClient`] with credentials for authenticated requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_credentials(
        credential: CoinbaseCredential,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        let raw = CoinbaseRawHttpClient::with_credentials(
            credential,
            environment,
            timeout_secs,
            proxy_url,
        )?;
        Ok(Self::from_raw(raw))
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables are not set.
    pub fn from_env(environment: CoinbaseEnvironment) -> Result<Self> {
        let raw = CoinbaseRawHttpClient::from_env(environment)?;
        Ok(Self::from_raw(raw))
    }

    /// Creates a new [`CoinbaseHttpClient`] with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if credentials are invalid.
    pub fn from_credentials(
        api_key: &str,
        api_secret: &str,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> Result<Self> {
        let raw = CoinbaseRawHttpClient::from_credentials(
            api_key,
            api_secret,
            environment,
            timeout_secs,
            proxy_url,
        )?;
        Ok(Self::from_raw(raw))
    }

    fn from_raw(raw: CoinbaseRawHttpClient) -> Self {
        Self {
            inner: Arc::new(raw),
            clock: get_atomic_clock_realtime(),
            instruments: Arc::new(AtomicMap::new()),
        }
    }

    /// Overrides the base REST URL (for testing with mock servers).
    ///
    /// # Panics
    ///
    /// Panics if the inner `Arc` has multiple references.
    pub fn set_base_url(&mut self, url: String) {
        Arc::get_mut(&mut self.inner)
            .expect("cannot override URL: Arc has multiple references")
            .set_base_url(url);
    }

    /// Returns the configured environment.
    #[must_use]
    pub fn environment(&self) -> CoinbaseEnvironment {
        self.inner.environment()
    }

    /// Returns true if this client has credentials for authenticated requests.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.inner.is_authenticated()
    }

    /// Returns a reference to the instrument cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        &self.instruments
    }

    /// Returns the current timestamp from the atomic clock.
    #[must_use]
    pub fn ts_now(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Gets all available products.
    pub async fn get_products(&self) -> Result<Value> {
        self.inner.get_products().await
    }

    /// Gets a specific product by ID.
    pub async fn get_product(&self, product_id: &str) -> Result<Value> {
        self.inner.get_product(product_id).await
    }

    /// Gets candles for a product.
    pub async fn get_candles(
        &self,
        product_id: &str,
        start: &str,
        end: &str,
        granularity: &str,
    ) -> Result<Value> {
        self.inner
            .get_candles(product_id, start, end, granularity)
            .await
    }

    /// Gets market trades for a product.
    pub async fn get_market_trades(&self, product_id: &str, limit: u32) -> Result<Value> {
        self.inner.get_market_trades(product_id, limit).await
    }

    /// Gets best bid/ask for one or more products.
    pub async fn get_best_bid_ask(&self, product_ids: &[&str]) -> Result<Value> {
        self.inner.get_best_bid_ask(product_ids).await
    }

    /// Gets the product order book.
    pub async fn get_product_book(&self, product_id: &str, limit: Option<u32>) -> Result<Value> {
        self.inner.get_product_book(product_id, limit).await
    }

    /// Gets all accounts.
    pub async fn get_accounts(&self) -> Result<Value> {
        self.inner.get_accounts().await
    }

    /// Gets a specific account by UUID.
    pub async fn get_account(&self, account_id: &str) -> Result<Value> {
        self.inner.get_account(account_id).await
    }

    /// Creates a new order.
    pub async fn create_order(&self, order: &Value) -> Result<Value> {
        self.inner.create_order(order).await
    }

    /// Cancels orders by order IDs.
    pub async fn cancel_orders(&self, order_ids: &[String]) -> Result<Value> {
        self.inner.cancel_orders(order_ids).await
    }

    /// Gets historical orders.
    pub async fn get_orders(&self, query: &str) -> Result<Value> {
        self.inner.get_orders(query).await
    }

    /// Gets a specific order by ID.
    pub async fn get_order(&self, order_id: &str) -> Result<Value> {
        self.inner.get_order(order_id).await
    }

    /// Gets fills (trade executions).
    pub async fn get_fills(&self, query: &str) -> Result<Value> {
        self.inner.get_fills(query).await
    }

    /// Gets fee transaction summary.
    pub async fn get_transaction_summary(&self) -> Result<Value> {
        self.inner.get_transaction_summary().await
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_raw_client_construction_live() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Live);
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_raw_client_construction_sandbox() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Sandbox, 10, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Sandbox);
    }

    #[rstest]
    fn test_raw_build_url() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        let url = client.build_url("/products");
        assert_eq!(url, "https://api.coinbase.com/api/v3/brokerage/products");
    }

    #[rstest]
    fn test_raw_build_jwt_uri_live() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        let uri = client.build_jwt_uri("GET", "/accounts");
        assert_eq!(uri, "GET api.coinbase.com/api/v3/brokerage/accounts");
    }

    #[rstest]
    fn test_raw_build_jwt_uri_sandbox() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Sandbox, 10, None).unwrap();
        let uri = client.build_jwt_uri("GET", "/accounts");
        assert_eq!(
            uri,
            "GET api-sandbox.coinbase.com/api/v3/brokerage/accounts"
        );
    }

    #[rstest]
    fn test_raw_build_jwt_uri_custom_base_url() {
        let mut client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        client.set_base_url("http://localhost:8080".to_string());
        let uri = client.build_jwt_uri("POST", "/orders");
        assert_eq!(uri, "POST localhost:8080/api/v3/brokerage/orders");
    }

    #[rstest]
    fn test_raw_auth_headers_without_credentials() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        let result = client.auth_headers("GET", "/accounts");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_auth_error());
    }

    #[rstest]
    fn test_domain_client_construction() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Live);
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_domain_client_default() {
        let client = CoinbaseHttpClient::default();
        assert_eq!(client.environment(), CoinbaseEnvironment::Live);
    }

    #[rstest]
    fn test_domain_client_instruments_cache_empty() {
        let client = CoinbaseHttpClient::default();
        assert!(client.instruments().is_empty());
    }

    #[rstest]
    fn test_domain_client_set_base_url() {
        let mut client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        client.set_base_url("http://localhost:9090".to_string());
        // Verify via raw client's build_url
        let url = client.inner.build_url("/test");
        assert!(url.starts_with("http://localhost:9090"));
    }
}
