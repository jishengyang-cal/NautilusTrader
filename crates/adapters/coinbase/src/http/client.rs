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
//! This module implements [`CoinbaseHttpClient`] for sending authenticated and public requests
//! to Coinbase endpoints. Each request generates a fresh ES256 JWT for authentication.

use std::{collections::HashMap, num::NonZeroU32};

use nautilus_core::consts::NAUTILUS_USER_AGENT;
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
    Quota::per_second(NonZeroU32::new(30).unwrap())
}

/// Provides an HTTP client for the Coinbase Advanced Trade REST API.
///
/// Handles JWT authentication, request construction, and response parsing.
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
    client: HttpClient,
    credential: Option<CoinbaseCredential>,
    base_url: String,
    environment: CoinbaseEnvironment,
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_client_construction_live() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Live);
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_client_construction_sandbox() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Sandbox, 10, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Sandbox);
    }

    #[rstest]
    fn test_build_url() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        let url = client.build_url("/products");
        assert_eq!(url, "https://api.coinbase.com/api/v3/brokerage/products");
    }

    #[rstest]
    fn test_build_jwt_uri_live() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        let uri = client.build_jwt_uri("GET", "/accounts");
        assert_eq!(uri, "GET api.coinbase.com/api/v3/brokerage/accounts");
    }

    #[rstest]
    fn test_build_jwt_uri_sandbox() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Sandbox, 10, None).unwrap();
        let uri = client.build_jwt_uri("GET", "/accounts");
        assert_eq!(
            uri,
            "GET api-sandbox.coinbase.com/api/v3/brokerage/accounts"
        );
    }

    #[rstest]
    fn test_build_jwt_uri_custom_base_url() {
        let mut client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        client.set_base_url("http://localhost:8080".to_string());
        let uri = client.build_jwt_uri("POST", "/orders");
        assert_eq!(uri, "POST localhost:8080/api/v3/brokerage/orders");
    }

    #[rstest]
    fn test_auth_headers_without_credentials() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None).unwrap();
        let result = client.auth_headers("GET", "/accounts");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_auth_error());
    }
}
