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

use std::{sync::LazyLock, time::Duration};

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const COINBASE: &str = "COINBASE";

pub static COINBASE_VENUE: LazyLock<Venue> = LazyLock::new(|| Venue::new(Ustr::from(COINBASE)));

pub const REST_URL: &str = "https://api.coinbase.com";
pub const REST_API_PATH: &str = "/api/v3/brokerage";
pub const WS_URL: &str = "wss://advanced-trade-ws.coinbase.com";
pub const WS_USER_URL: &str = "wss://advanced-trade-ws-user.coinbase.com";

pub const REST_URL_SANDBOX: &str = "https://api-sandbox.coinbase.com";
pub const WS_URL_SANDBOX: &str = "wss://advanced-trade-ws-sandbox.coinbase.com";
pub const WS_USER_URL_SANDBOX: &str = "wss://advanced-trade-ws-user-sandbox.coinbase.com";

pub const JWT_ISSUER: &str = "cdp";

/// Coinbase requires JWT regeneration within 2 minutes
pub const JWT_EXPIRY_SECS: u64 = 120;

pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const RECONNECT_BASE_BACKOFF: Duration = Duration::from_millis(250);
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Coinbase disconnects if no subscription within 5 seconds
pub const WS_SUBSCRIBE_DEADLINE: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_venue_constant() {
        assert_eq!(COINBASE_VENUE.as_str(), COINBASE);
    }

    #[rstest]
    fn test_url_constants() {
        assert!(REST_URL.starts_with("https://"));
        assert!(WS_URL.starts_with("wss://"));
        assert!(WS_USER_URL.starts_with("wss://"));
    }

    #[rstest]
    fn test_timeout_constants() {
        assert_eq!(HTTP_TIMEOUT, Duration::from_secs(10));
        assert_eq!(HEARTBEAT_INTERVAL, Duration::from_secs(30));
        assert_eq!(WS_SUBSCRIBE_DEADLINE, Duration::from_secs(5));
    }
}
