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

use chrono::{DateTime, Utc};
use nautilus_core::{
    AtomicMap, UnixNanos,
    consts::NAUTILUS_USER_AGENT,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
};
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpResponse, Method, USER_AGENT},
    ratelimiter::quota::Quota,
};
use serde_json::Value;
use url::form_urlencoded;
use ustr::Ustr;

use crate::{
    common::{
        consts::REST_API_PATH,
        credential::CoinbaseCredential,
        enums::{CoinbaseEnvironment, CoinbaseProductType},
        urls,
    },
    http::{
        error::{Error, Result},
        models::{
            AccountsResponse, Fill, FillsResponse, Order, OrderResponse, OrdersListResponse,
            ProductsResponse,
        },
        parse::{
            parse_account_state, parse_fill_report, parse_instrument, parse_order_status_report,
        },
    },
};

// Coinbase Advanced Trade rate limit: 30 requests per second
fn default_quota() -> Option<Quota> {
    Quota::per_second(NonZeroU32::new(30).unwrap()) // Infallible: 30 is non-zero
}

// Query parameters for `request_order_status_reports_internal`.
struct OrderStatusQuery {
    account_id: AccountId,
    product_id: Option<String>,
    client_order_id_filter: Option<String>,
    open_only: bool,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<u32>,
}

// Builds a query string from `(key, value)` pairs, percent-encoding both
// halves. Coinbase cursors and RFC 3339 timestamps (`+00:00`) contain
// reserved characters that must be encoded to avoid the server reading
// them as a different query.
fn encode_query(params: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        serializer.append_pair(k, v);
    }
    serializer.finish()
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
    ///
    /// The JWT URI claim covers only `{METHOD} {host}{path}` without the
    /// query string, matching the Coinbase SDK convention. Query parameters
    /// are appended to the URL but excluded from the signing input.
    pub async fn get_with_query(&self, path: &str, query: &str) -> Result<Value> {
        let full_url_path = if query.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{query}")
        };
        let url = self.build_url(&full_url_path);

        // Sign with the bare path only (no query string).
        let headers = self.auth_headers("GET", path)?;
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

    /// Gets all available products via the public `/market/products` endpoint.
    pub async fn get_products(&self) -> Result<Value> {
        self.get_public("/market/products").await
    }

    /// Gets a specific product by ID via the public endpoint.
    pub async fn get_product(&self, product_id: &str) -> Result<Value> {
        self.get_public(&format!("/market/products/{product_id}"))
            .await
    }

    /// Gets candles for a product via the public endpoint.
    pub async fn get_candles(
        &self,
        product_id: &str,
        start: &str,
        end: &str,
        granularity: &str,
    ) -> Result<Value> {
        let query = format!("start={start}&end={end}&granularity={granularity}");
        self.get_public_with_query(&format!("/market/products/{product_id}/candles"), &query)
            .await
    }

    /// Gets market trades for a product via the public endpoint.
    pub async fn get_market_trades(&self, product_id: &str, limit: u32) -> Result<Value> {
        let query = format!("limit={limit}");
        self.get_public_with_query(&format!("/market/products/{product_id}/ticker"), &query)
            .await
    }

    /// Gets best bid/ask for one or more products.
    ///
    /// No public `/market/` equivalent exists for this endpoint; requires
    /// authentication.
    pub async fn get_best_bid_ask(&self, product_ids: &[&str]) -> Result<Value> {
        let query = product_ids
            .iter()
            .map(|id| format!("product_ids={id}"))
            .collect::<Vec<_>>()
            .join("&");
        self.get_with_query("/best_bid_ask", &query).await
    }

    /// Gets the product order book via the public endpoint.
    pub async fn get_product_book(&self, product_id: &str, limit: Option<u32>) -> Result<Value> {
        let mut query = format!("product_id={product_id}");

        if let Some(limit) = limit {
            query.push_str(&format!("&limit={limit}"));
        }
        self.get_public_with_query("/market/product_book", &query)
            .await
    }

    /// Gets all accounts.
    pub async fn get_accounts(&self) -> Result<Value> {
        self.get("/accounts").await
    }

    /// Gets accounts with a query string (for pagination via `cursor` / `limit`).
    pub async fn get_accounts_with_query(&self, query: &str) -> Result<Value> {
        if query.is_empty() {
            self.get("/accounts").await
        } else {
            self.get_with_query("/accounts", query).await
        }
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

    /// Requests all instruments from Coinbase, optionally filtered by product type.
    ///
    /// Parses each supported product into a Nautilus [`InstrumentAny`] and caches
    /// the results in the shared instrument map. Unsupported products (non-crypto
    /// futures, `UNKNOWN` product types) are skipped with a debug log.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn request_instruments(
        &self,
        product_type: Option<CoinbaseProductType>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let json = self
            .inner
            .get_products()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch products: {e}"))?;
        let response: ProductsResponse =
            serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.ts_now();
        let mut instruments = Vec::with_capacity(response.products.len());

        for product in &response.products {
            if let Some(filter) = product_type
                && product.product_type != filter
            {
                continue;
            }

            match parse_instrument(product, ts_init) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => {
                    log::debug!(
                        "Skipping product '{}' during parse: {e}",
                        product.product_id
                    );
                }
            }
        }

        self.cache_instruments(&instruments);
        Ok(instruments)
    }

    /// Requests a single instrument by product ID.
    ///
    /// Caches the result on success.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails, deserialization fails,
    /// or the product cannot be parsed into a supported instrument.
    pub async fn request_instrument(&self, product_id: &str) -> anyhow::Result<InstrumentAny> {
        let json = self
            .inner
            .get_product(product_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch product '{product_id}': {e}"))?;
        let product: crate::http::models::Product =
            serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;
        let ts_init = self.ts_now();
        let instrument = parse_instrument(&product, ts_init)?;
        self.cache_instrument(&instrument);
        Ok(instrument)
    }

    /// Requests the current account state.
    ///
    /// Builds a cash-type [`AccountState`] from `/accounts` with one balance
    /// per currency. Follows Coinbase's cursor pagination so multi-wallet
    /// accounts are reported in full. `reported` is set to `true` since the
    /// values come from the venue.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot
    /// be parsed.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let accounts = self.fetch_all_accounts().await?;
        let ts_event = self.ts_now();
        parse_account_state(&accounts, account_id, true, ts_event, ts_event)
    }

    async fn fetch_all_accounts(&self) -> anyhow::Result<Vec<crate::http::models::Account>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut pairs: Vec<(&str, &str)> = vec![("limit", "250")];
            if let Some(c) = cursor.as_deref().filter(|s| !s.is_empty()) {
                pairs.push(("cursor", c));
            }
            let query_str = encode_query(&pairs);

            let json = self
                .inner
                .get_accounts_with_query(&query_str)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch accounts: {e}"))?;
            let response: AccountsResponse =
                serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;

            all.extend(response.accounts);

            if !response.has_next || response.cursor.is_empty() {
                break;
            }
            cursor = Some(response.cursor);
        }

        Ok(all)
    }

    /// Requests a single order status report by venue or client order ID.
    ///
    /// Resolves venue order IDs first via `/orders/historical/{id}`. When only a
    /// `client_order_id` is provided, paginates the order history filtered to
    /// that client ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails, the order cannot be found,
    /// or the response cannot be parsed.
    pub async fn request_order_status_report(
        &self,
        account_id: AccountId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        let venue_order_id = match (venue_order_id, client_order_id) {
            (Some(vid), _) => vid,
            (None, Some(cid)) => {
                // Fall back to batched query when only the client order ID is known
                let reports = self
                    .request_order_status_reports_internal(OrderStatusQuery {
                        account_id,
                        product_id: None,
                        client_order_id_filter: Some(cid.as_str().to_string()),
                        open_only: false,
                        start: None,
                        end: None,
                        limit: None,
                    })
                    .await?;
                return reports
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No order found for client_order_id={cid}"));
            }
            (None, None) => {
                anyhow::bail!("Either client_order_id or venue_order_id is required")
            }
        };

        let json = self
            .inner
            .get_order(venue_order_id.as_str())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch order: {e}"))?;
        let response: OrderResponse =
            serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;
        let instrument = self
            .get_or_fetch_instrument(response.order.product_id)
            .await?;
        let ts_init = self.ts_now();
        parse_order_status_report(&response.order, &instrument, account_id, ts_init)
    }

    /// Requests order status reports, optionally filtered by instrument, open
    /// status, and time window.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or when any response cannot
    /// be deserialized.
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let product_id = instrument_id.map(|id| id.symbol.as_str().to_string());
        self.request_order_status_reports_internal(OrderStatusQuery {
            account_id,
            product_id,
            client_order_id_filter: None,
            open_only,
            start,
            end,
            limit,
        })
        .await
    }

    async fn request_order_status_reports_internal(
        &self,
        query_params: OrderStatusQuery,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let OrderStatusQuery {
            account_id,
            product_id,
            client_order_id_filter,
            open_only,
            start,
            end,
            limit,
        } = query_params;

        let orders = self
            .fetch_all_orders(
                product_id.as_deref(),
                open_only,
                start,
                end,
                limit,
                client_order_id_filter.as_deref(),
            )
            .await?;

        let ts_init = self.ts_now();
        let mut reports = Vec::with_capacity(orders.len());

        for order in &orders {
            let instrument = match self.get_or_fetch_instrument(order.product_id).await {
                Ok(inst) => inst,
                Err(e) => {
                    log::debug!("Skipping order {}: {e}", order.order_id);
                    continue;
                }
            };

            match parse_order_status_report(order, &instrument, account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => log::warn!("Failed to parse order {}: {e}", order.order_id),
            }
        }

        Ok(reports)
    }

    async fn fetch_all_orders(
        &self,
        product_id: Option<&str>,
        open_only: bool,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        client_order_id_filter: Option<&str>,
    ) -> anyhow::Result<Vec<Order>> {
        let mut collected: Vec<Order> = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let start_str = start.map(|s| s.to_rfc3339());
            let end_str = end.map(|e| e.to_rfc3339());
            let limit_str = limit.map(|l| l.to_string());

            let mut pairs: Vec<(&str, &str)> = Vec::new();

            // Coinbase accepts `product_ids` as a repeated array parameter on
            // `/orders/historical/batch`; the singular form is silently ignored.
            if let Some(pid) = product_id {
                pairs.push(("product_ids", pid));
            }

            if open_only {
                pairs.push(("order_status", "OPEN"));
            }

            if let Some(s) = start_str.as_deref() {
                pairs.push(("start_date", s));
            }

            if let Some(e) = end_str.as_deref() {
                pairs.push(("end_date", e));
            }

            if let Some(l) = limit_str.as_deref() {
                pairs.push(("limit", l));
            }

            if let Some(c) = cursor.as_deref().filter(|s| !s.is_empty()) {
                pairs.push(("cursor", c));
            }

            let query_str = encode_query(&pairs);

            let json = self
                .inner
                .get_orders(&query_str)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch orders: {e}"))?;
            let response: OrdersListResponse =
                serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;

            for order in response.orders {
                if let Some(cid) = client_order_id_filter
                    && order.client_order_id != cid
                {
                    continue;
                }
                collected.push(order);
            }

            // Stop when the caller wants a hard cap and we've reached it.
            if let Some(limit) = limit
                && collected.len() >= limit as usize
            {
                collected.truncate(limit as usize);
                break;
            }

            if !response.has_next || response.cursor.is_empty() {
                break;
            }
            cursor = Some(response.cursor);
        }

        Ok(collected)
    }

    /// Requests fill reports, optionally filtered by instrument, venue order ID,
    /// and time window.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<VenueOrderId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let fills = self
            .fetch_all_fills(
                instrument_id.map(|id| id.symbol.as_str().to_string()),
                venue_order_id.map(|id| id.as_str().to_string()),
                start,
                end,
                limit,
            )
            .await?;

        let ts_init = self.ts_now();
        let mut reports = Vec::with_capacity(fills.len());

        for fill in &fills {
            let instrument = match self.get_or_fetch_instrument(fill.product_id).await {
                Ok(inst) => inst,
                Err(e) => {
                    log::debug!("Skipping fill {}: {e}", fill.trade_id);
                    continue;
                }
            };

            match parse_fill_report(fill, &instrument, account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => log::warn!("Failed to parse fill {}: {e}", fill.trade_id),
            }
        }

        Ok(reports)
    }

    async fn fetch_all_fills(
        &self,
        product_id: Option<String>,
        venue_order_id: Option<String>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Fill>> {
        let mut collected: Vec<Fill> = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let start_str = start.map(|s| s.to_rfc3339());
            let end_str = end.map(|e| e.to_rfc3339());
            let limit_str = limit.map(|l| l.to_string());

            let mut pairs: Vec<(&str, &str)> = Vec::new();

            // `/orders/historical/fills` takes repeated array filters for
            // product and order IDs. Singular keys are accepted by the server
            // but silently ignored, which would scan the full fill history.
            if let Some(pid) = product_id.as_deref() {
                pairs.push(("product_ids", pid));
            }

            if let Some(vid) = venue_order_id.as_deref() {
                pairs.push(("order_ids", vid));
            }

            if let Some(s) = start_str.as_deref() {
                pairs.push(("start_sequence_timestamp", s));
            }

            if let Some(e) = end_str.as_deref() {
                pairs.push(("end_sequence_timestamp", e));
            }

            if let Some(l) = limit_str.as_deref() {
                pairs.push(("limit", l));
            }

            if let Some(c) = cursor.as_deref().filter(|s| !s.is_empty()) {
                pairs.push(("cursor", c));
            }

            let query_str = encode_query(&pairs);

            let json = self
                .inner
                .get_fills(&query_str)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fetch fills: {e}"))?;
            let response: FillsResponse =
                serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;

            collected.extend(response.fills);

            if let Some(limit) = limit
                && collected.len() >= limit as usize
            {
                collected.truncate(limit as usize);
                break;
            }

            if response.cursor.is_empty() {
                break;
            }
            cursor = Some(response.cursor);
        }

        Ok(collected)
    }

    /// Caches an instrument in the shared instrument map.
    pub fn cache_instrument(&self, instrument: &InstrumentAny) {
        self.instruments.rcu(|m| {
            m.insert(instrument.id(), instrument.clone());
        });
    }

    /// Caches a batch of instruments in the shared instrument map.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments.rcu(|m| {
            for instrument in instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });
    }

    // Returns the cached instrument for a product ID, fetching it on miss.
    // Order and fill reconciliation calls parse hundreds of historical
    // records and each one needs precision metadata. Rather than forcing
    // callers to bootstrap the full instrument universe first, this lazy
    // path fetches any missing product via `/products/{id}` and caches it.
    async fn get_or_fetch_instrument(&self, product_id: Ustr) -> anyhow::Result<InstrumentAny> {
        let instrument_id = InstrumentId::new(
            Symbol::new(product_id),
            *crate::common::consts::COINBASE_VENUE,
        );

        if let Some(instrument) = self.instruments.get_cloned(&instrument_id) {
            return Ok(instrument);
        }
        // Cache miss — fetch and cache the single product. Any parse error
        // (unsupported product type, missing fields) surfaces to the caller so
        // the offending record can be skipped with a log.
        self.request_instrument(product_id.as_str()).await
    }

    /// Wraps a submitted order: generates a fresh client order ID if none is
    /// provided and returns the venue order ID upon success. Phase 1 exposes a
    /// minimal raw path; Phase 4 will layer typed Nautilus Order conversion.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the venue rejects the
    /// order.
    pub async fn submit_order_raw(
        &self,
        request: &crate::http::models::CreateOrderRequest,
    ) -> anyhow::Result<crate::http::models::CreateOrderResponse> {
        let body = serde_json::to_value(request).map_err(|e| anyhow::anyhow!(e))?;
        let json = self
            .inner
            .create_order(&body)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to submit order: {e}"))?;
        serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))
    }

    /// Cancels a batch of orders by venue order ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot be
    /// parsed.
    pub async fn cancel_orders_raw(
        &self,
        venue_order_ids: &[VenueOrderId],
    ) -> anyhow::Result<crate::http::models::CancelOrdersResponse> {
        let ids: Vec<String> = venue_order_ids
            .iter()
            .map(|id| id.as_str().to_string())
            .collect();
        let json = self
            .inner
            .cancel_orders(&ids)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to cancel orders: {e}"))?;
        serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))
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

    #[rstest]
    fn test_encode_query_escapes_rfc3339_timestamps() {
        let query = encode_query(&[("start_date", "2024-01-15T10:00:00+00:00")]);
        // `+` must be escaped so the server does not read it as a space.
        assert_eq!(query, "start_date=2024-01-15T10%3A00%3A00%2B00%3A00");
    }

    #[rstest]
    fn test_encode_query_escapes_opaque_cursor() {
        let query = encode_query(&[("cursor", "a/b+c=?&x")]);
        // Reserved characters in an opaque cursor must not leak into the query structure.
        assert!(!query.contains("a/b+c=?&x"));
        assert!(query.starts_with("cursor="));
    }

    #[rstest]
    fn test_encode_query_joins_pairs_with_ampersand() {
        let query = encode_query(&[("product_id", "BTC-USD"), ("limit", "50")]);
        assert_eq!(query, "product_id=BTC-USD&limit=50");
    }
}
