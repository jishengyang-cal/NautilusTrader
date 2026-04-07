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

use std::fmt::{Debug, Display};

use aws_lc_rs::{
    rand as lc_rand,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair},
};
use base64::prelude::*;
use nautilus_core::env::get_or_env_var;
use serde_json::json;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
    common::consts::{JWT_EXPIRY_SECS, JWT_ISSUER},
    http::error::{Error, Result},
};

pub const COINBASE_API_KEY_ENV: &str = "COINBASE_API_KEY";
pub const COINBASE_API_SECRET_ENV: &str = "COINBASE_API_SECRET";

fn base64url_encode(data: &[u8]) -> String {
    BASE64_URL_SAFE_NO_PAD.encode(data)
}

/// CDP API key pair with zeroization on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct CoinbaseCredential {
    api_key: String,
    api_secret: String,
}

impl CoinbaseCredential {
    /// Creates a new [`CoinbaseCredential`] instance.
    pub fn new(api_key: String, api_secret: String) -> Self {
        Self {
            api_key,
            api_secret,
        }
    }

    /// Resolves credentials from provided values or environment variables.
    pub fn resolve(api_key: Option<&str>, api_secret: Option<&str>) -> Result<Self> {
        let key = get_or_env_var(
            api_key.filter(|s| !s.trim().is_empty()).map(String::from),
            COINBASE_API_KEY_ENV,
        )
        .map_err(|_| {
            Error::auth(format!(
                "{COINBASE_API_KEY_ENV} environment variable is not set"
            ))
        })?;

        let secret = get_or_env_var(
            api_secret
                .filter(|s| !s.trim().is_empty())
                .map(String::from),
            COINBASE_API_SECRET_ENV,
        )
        .map_err(|_| {
            Error::auth(format!(
                "{COINBASE_API_SECRET_ENV} environment variable is not set"
            ))
        })?;

        Ok(Self::new(key, secret))
    }

    /// Loads credentials from environment variables.
    pub fn from_env() -> Result<Self> {
        Self::resolve(None, None)
    }

    /// Returns the API key name.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Returns the PEM-encoded API secret.
    pub fn api_secret(&self) -> &str {
        &self.api_secret
    }

    /// Generates a JWT for REST API authentication.
    ///
    /// The `uri` format is `"{METHOD} {host}{path}"`, e.g.
    /// `"GET api.coinbase.com/api/v3/brokerage/accounts"`.
    pub fn build_rest_jwt(&self, uri: &str) -> Result<String> {
        self.build_jwt(Some(uri))
    }

    /// Generates a JWT for WebSocket authentication (no URI claim).
    pub fn build_ws_jwt(&self) -> Result<String> {
        self.build_jwt(None)
    }

    /// Generates an ES256 JWT signed with the PEM EC private key.
    fn build_jwt(&self, uri: Option<&str>) -> Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Error::auth(format!("Failed to get system time: {e}")))?
            .as_secs();

        let nonce = {
            let mut buf = [0u8; 16];
            lc_rand::fill(&mut buf)
                .map_err(|e| Error::auth(format!("Failed to generate nonce: {e}")))?;
            nautilus_core::hex::encode(buf)
        };

        let header = json!({
            "alg": "ES256",
            "typ": "JWT",
            "kid": self.api_key,
            "nonce": nonce,
        });

        let mut payload = json!({
            "sub": self.api_key,
            "iss": JWT_ISSUER,
            "nbf": now,
            "exp": now + JWT_EXPIRY_SECS,
        });

        if let Some(uri) = uri {
            payload["uri"] = serde_json::Value::String(uri.to_string());
        }

        let header_b64 = base64url_encode(header.to_string().as_bytes());
        let payload_b64 = base64url_encode(payload.to_string().as_bytes());
        let signing_input = format!("{header_b64}.{payload_b64}");

        let pem_obj = pem::parse(self.api_secret.trim())
            .map_err(|e| Error::auth(format!("Failed to parse PEM key: {e}")))?;

        // Coinbase issues SEC1 (EC PRIVATE KEY) PEMs; from_private_key_der
        // handles both SEC1 and PKCS#8 formats
        let key_pair = EcdsaKeyPair::from_private_key_der(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            pem_obj.contents(),
        )
        .map_err(|e| Error::auth(format!("Failed to load EC private key: {e}")))?;

        let rng = lc_rand::SystemRandom::new();
        let sig = key_pair
            .sign(&rng, signing_input.as_bytes())
            .map_err(|e| Error::auth(format!("Failed to sign JWT: {e}")))?;

        let sig_b64 = base64url_encode(sig.as_ref());

        Ok(format!("{signing_input}.{sig_b64}"))
    }
}

impl Debug for CoinbaseCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CoinbaseCredential))
            .field(
                "api_key",
                &format!("{}...", &self.api_key[..8.min(self.api_key.len())]),
            )
            .field("api_secret", &"***redacted***")
            .finish()
    }
}

impl Display for CoinbaseCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CoinbaseCredential({}...)",
            &self.api_key[..8.min(self.api_key.len())]
        )
    }
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::encoding::AsDer;
    use rstest::rstest;

    use super::*;

    const TEST_API_KEY: &str = "organizations/test-org/apiKeys/test-key-id";

    /// Generates a SEC1 (RFC 5915) PEM key matching Coinbase's production format.
    fn test_sec1_pem_key() -> String {
        let rng = lc_rand::SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let key_pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref()).unwrap();
        let sec1_der = key_pair.private_key().as_der().unwrap();
        let pem_obj = pem::Pem::new("EC PRIVATE KEY", sec1_der.as_ref().to_vec());
        pem::encode(&pem_obj)
    }

    /// Generates a PKCS#8 PEM key.
    fn test_pkcs8_pem_key() -> String {
        let rng = lc_rand::SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pem_obj = pem::Pem::new("PRIVATE KEY", pkcs8.as_ref().to_vec());
        pem::encode(&pem_obj)
    }

    #[rstest]
    fn test_credential_debug_redacts_secret() {
        let cred = CoinbaseCredential::new(TEST_API_KEY.to_string(), "my_secret_pem".to_string());
        let debug = format!("{cred:?}");
        assert!(debug.contains("redacted"));
        assert!(!debug.contains("my_secret_pem"));
    }

    #[rstest]
    fn test_credential_display_truncates_key() {
        let cred = CoinbaseCredential::new(TEST_API_KEY.to_string(), "my_secret_pem".to_string());
        let display = format!("{cred}");
        assert!(display.contains("organiza..."));
        assert!(!display.contains("my_secret_pem"));
    }

    #[rstest]
    fn test_build_rest_jwt() {
        let pem_key = test_sec1_pem_key();
        let cred = CoinbaseCredential::new(TEST_API_KEY.to_string(), pem_key);
        let jwt = cred.build_rest_jwt("GET api.coinbase.com/api/v3/brokerage/accounts");
        assert!(jwt.is_ok());

        let token = jwt.unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT must have 3 parts");

        // Decode and verify header
        let header_bytes = BASE64_URL_SAFE_NO_PAD.decode(parts[0]).unwrap();
        let header: serde_json::Value = serde_json::from_slice(&header_bytes).unwrap();
        assert_eq!(header["alg"], "ES256");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(header["kid"], TEST_API_KEY);
        assert!(header["nonce"].is_string());

        // Decode and verify payload
        let payload_bytes = BASE64_URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).unwrap();
        assert_eq!(payload["sub"], TEST_API_KEY);
        assert_eq!(payload["iss"], "cdp");
        assert!(payload["nbf"].is_number());
        assert!(payload["exp"].is_number());
        assert!(payload["uri"].is_string());
    }

    #[rstest]
    fn test_build_ws_jwt_has_no_uri() {
        let pem_key = test_sec1_pem_key();
        let cred = CoinbaseCredential::new(TEST_API_KEY.to_string(), pem_key);
        let jwt = cred.build_ws_jwt();
        assert!(jwt.is_ok());

        let token = jwt.unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        let payload_bytes = BASE64_URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).unwrap();
        assert!(payload.get("uri").is_none());
    }

    #[rstest]
    fn test_build_jwt_with_pkcs8_pem() {
        let pem_key = test_pkcs8_pem_key();
        let cred = CoinbaseCredential::new(TEST_API_KEY.to_string(), pem_key);
        let jwt = cred.build_rest_jwt("GET api.coinbase.com/api/v3/brokerage/accounts");
        assert!(jwt.is_ok());
    }

    #[rstest]
    fn test_build_jwt_invalid_pem_fails() {
        let cred = CoinbaseCredential::new(TEST_API_KEY.to_string(), "not-a-pem-key".to_string());
        let result = cred.build_rest_jwt("GET api.coinbase.com/test");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_auth_error());
    }

    #[rstest]
    fn test_base64url_encode() {
        let encoded = base64url_encode(b"hello world");
        assert!(!encoded.contains('='));
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }
}
