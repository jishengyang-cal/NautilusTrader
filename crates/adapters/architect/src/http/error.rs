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

//! Error structures and enumerations for the Architect HTTP integration.

use nautilus_network::http::HttpClientError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Build error for query parameter validation.
#[derive(Debug, Clone, Error)]
pub enum ArchitectBuildError {
    /// Missing required symbol.
    #[error("Missing required symbol")]
    MissingSymbol,
    /// Invalid limit value.
    #[error("Invalid limit: {0}")]
    InvalidLimit(String),
    /// Invalid time range: `start` should be less than `end`.
    #[error("Invalid time range: start ({start}) must be less than end ({end})")]
    InvalidTimeRange { start: i64, end: i64 },
    /// Missing required order identifier.
    #[error("Missing required order identifier")]
    MissingOrderId,
}

/// Represents the JSON structure of an error response returned by the Architect API.
///
/// Note: The exact error response format will be updated as we learn more about
/// the Architect API error structure.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ArchitectErrorResponse {
    /// Error code or type.
    #[serde(default)]
    pub error: Option<String>,
    /// A human-readable explanation of the error condition.
    #[serde(default)]
    pub message: Option<String>,
    /// HTTP status code.
    #[serde(default)]
    pub status: Option<u16>,
}

/// A typed error enumeration for the Architect HTTP client.
#[derive(Debug, Clone, Error)]
pub enum ArchitectHttpError {
    /// Error variant when credentials are missing but the request is authenticated.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Errors returned directly by Architect API.
    #[error("Architect API error: {message}")]
    ApiError { message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Parameter validation error.
    #[error("Parameter validation error: {0}")]
    ValidationError(String),
    /// Build error for query parameters.
    #[error("Build error: {0}")]
    BuildError(#[from] ArchitectBuildError),
    /// Request was canceled, typically due to shutdown or disconnect.
    #[error("Request canceled: {0}")]
    Canceled(String),
    /// Generic network error (for retries, cancellations, etc).
    #[error("Network error: {0}")]
    NetworkError(String),
    /// Any unknown HTTP status or unexpected response from Architect.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: u16, body: String },
}

impl From<HttpClientError> for ArchitectHttpError {
    fn from(error: HttpClientError) -> Self {
        Self::NetworkError(error.to_string())
    }
}

impl From<String> for ArchitectHttpError {
    fn from(error: String) -> Self {
        Self::ValidationError(error)
    }
}

impl From<serde_json::Error> for ArchitectHttpError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}

impl From<ArchitectErrorResponse> for ArchitectHttpError {
    fn from(error: ArchitectErrorResponse) -> Self {
        let message = error
            .message
            .or(error.error)
            .unwrap_or_else(|| "Unknown error".to_string());
        Self::ApiError { message }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_architect_build_error_display() {
        let error = ArchitectBuildError::MissingSymbol;
        assert_eq!(error.to_string(), "Missing required symbol");

        let error = ArchitectBuildError::InvalidLimit("must be positive".to_string());
        assert_eq!(error.to_string(), "Invalid limit: must be positive");

        let error = ArchitectBuildError::InvalidTimeRange {
            start: 100,
            end: 50,
        };
        assert_eq!(
            error.to_string(),
            "Invalid time range: start (100) must be less than end (50)"
        );
    }

    #[rstest]
    fn test_architect_http_error_from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json")
            .expect_err("Should fail to parse");
        let http_err = ArchitectHttpError::from(json_err);

        assert!(matches!(http_err, ArchitectHttpError::JsonError(_)));
    }

    #[rstest]
    fn test_architect_http_error_from_string() {
        let error = ArchitectHttpError::from("Test validation error".to_string());
        assert_eq!(
            error.to_string(),
            "Parameter validation error: Test validation error"
        );
    }

    #[rstest]
    fn test_architect_error_response_to_http_error() {
        let error_response = ArchitectErrorResponse {
            error: Some("INVALID_REQUEST".to_string()),
            message: Some("Invalid parameter".to_string()),
            status: Some(400),
        };

        let http_error = ArchitectHttpError::from(error_response);
        assert_eq!(
            http_error.to_string(),
            "Architect API error: Invalid parameter"
        );
    }
}
