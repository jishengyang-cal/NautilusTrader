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

//! Manual verification script for Architect orders WebSocket.
//!
//! Tests authenticated WebSocket connection and order event streaming.
//! Defaults to sandbox environment.
//!
//! Requires environment variables:
//! - `ARCHITECT_API_KEY`: Your API key
//! - `ARCHITECT_API_SECRET`: Your API secret
//!
//! For 2FA (if enabled on your account):
//! - `ARCHITECT_TOTP_SECRET`: Base32 TOTP secret for auto-generating codes
//!
//! Usage:
//! ```bash
//! ARCHITECT_API_KEY=your_key \
//!   ARCHITECT_API_SECRET=your_secret \
//!   ARCHITECT_TOTP_SECRET=your_totp_secret \
//!   cargo run --bin architect-ws-orders -p nautilus-architect
//! ```

use std::time::Duration;

use futures_util::StreamExt;
use nautilus_architect::{
    common::enums::ArchitectEnvironment,
    http::{client::ArchitectRawHttpClient, error::ArchitectHttpError},
    websocket::{ArchitectOrdersWsMessage, orders::ArchitectOrdersWebSocketClient},
};
use nautilus_model::identifiers::AccountId;
use totp_rs::{Algorithm, Secret, TOTP};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let api_key = std::env::var("ARCHITECT_API_KEY")
        .expect("ARCHITECT_API_KEY environment variable required");
    let api_secret = std::env::var("ARCHITECT_API_SECRET")
        .expect("ARCHITECT_API_SECRET environment variable required");

    let environment = if std::env::var("ARCHITECT_IS_SANDBOX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true)
    {
        ArchitectEnvironment::Sandbox
    } else {
        ArchitectEnvironment::Production
    };

    tracing::info!("Environment: {environment}");

    let http_client = ArchitectRawHttpClient::new(
        Some(environment.http_url().to_string()),
        Some(environment.orders_url().to_string()),
        Some(30),
        None,
        None,
        None,
        None,
    )?;

    tracing::info!(
        "Authenticating via HTTP to {}/authenticate ...",
        environment.http_url()
    );

    // Generate TOTP code from secret if available
    let totp_code: Option<String> = std::env::var("ARCHITECT_TOTP_SECRET").ok().map(|secret| {
        let secret_bytes = Secret::Encoded(secret)
            .to_bytes()
            .expect("Invalid base32 TOTP secret");
        let totp =
            TOTP::new(Algorithm::SHA1, 6, 1, 30, secret_bytes).expect("Invalid TOTP configuration");
        let code = totp.generate_current().expect("Failed to generate TOTP");
        tracing::info!("Generated TOTP code from secret");
        code
    });

    // First try without TOTP (in case 2FA is disabled)
    let auth_response = match http_client.authenticate(&api_key, &api_secret, 3600).await {
        Ok(resp) => resp,
        Err(e) => {
            // Check if 2FA is required
            if matches!(e, ArchitectHttpError::UnexpectedStatus { status: 400, .. }) {
                let code = match totp_code {
                    Some(code) => code,
                    None => {
                        tracing::error!("2FA required but ARCHITECT_TOTP_SECRET not set");
                        return Err("2FA required but ARCHITECT_TOTP_SECRET not provided".into());
                    }
                };

                tracing::info!("2FA required, using provided code...");
                match http_client
                    .authenticate_with_totp(&api_key, &api_secret, 3600, Some(&code))
                    .await
                {
                    Ok(resp) => resp,
                    Err(e) => {
                        tracing::error!("Authentication with 2FA failed: {e:?}");
                        return Err(format!("Authentication failed: {e:?}").into());
                    }
                }
            } else {
                tracing::error!("Authentication failed: {e:?}");
                return Err(format!("Authentication failed: {e:?}").into());
            }
        }
    };
    tracing::info!("Authenticated successfully");

    let account_id = AccountId::new("ARCHITECT-001");
    tracing::info!("Account ID: {account_id}");

    tracing::info!(
        "Connecting to orders WebSocket: {}",
        environment.ws_orders_url()
    );
    let mut client = ArchitectOrdersWebSocketClient::new(
        environment.ws_orders_url().to_string(),
        account_id,
        Some(30),
    );

    client.connect(&auth_response.token).await?;
    tracing::info!("Connected and authenticated");

    tracing::info!("Requesting open orders...");
    client.get_open_orders().await?;

    tracing::info!("Listening for messages (30 seconds)...");
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();
    let mut message_count = 0;

    {
        let stream = client.stream();
        tokio::pin!(stream);

        while let Some(msg) = stream.next().await {
            message_count += 1;

            match &msg {
                ArchitectOrdersWsMessage::Authenticated => {
                    tracing::info!("WebSocket authenticated");
                }
                ArchitectOrdersWsMessage::OrderAcknowledged(ack) => {
                    tracing::info!("Order acknowledged: {} {}", ack.o.oid, ack.o.s);
                }
                ArchitectOrdersWsMessage::OrderPartiallyFilled(fill) => {
                    tracing::info!(
                        "Order partially filled: {} {} @ {}",
                        fill.o.oid,
                        fill.xs.q,
                        fill.xs.p
                    );
                }
                ArchitectOrdersWsMessage::OrderFilled(fill) => {
                    tracing::info!("Order filled: {} {} @ {}", fill.o.oid, fill.xs.q, fill.xs.p);
                }
                ArchitectOrdersWsMessage::OrderCanceled(cancel) => {
                    tracing::info!("Order canceled: {} reason={}", cancel.o.oid, cancel.xr);
                }
                ArchitectOrdersWsMessage::OrderRejectedRaw(reject) => {
                    tracing::warn!("Order rejected: {} reason={}", reject.o.oid, reject.r);
                }
                ArchitectOrdersWsMessage::OrderExpired(expired) => {
                    tracing::info!("Order expired: {}", expired.o.oid);
                }
                ArchitectOrdersWsMessage::OrderReplaced(replaced) => {
                    tracing::info!("Order replaced: {}", replaced.o.oid);
                }
                ArchitectOrdersWsMessage::OrderDoneForDay(done) => {
                    tracing::info!("Order done for day: {}", done.o.oid);
                }
                ArchitectOrdersWsMessage::CancelRejected(reject) => {
                    tracing::warn!("Cancel rejected: {} reason={}", reject.oid, reject.r);
                }
                ArchitectOrdersWsMessage::PlaceOrderResponse(resp) => {
                    tracing::info!(
                        "Place order response: rid={} oid={}",
                        resp.rid,
                        resp.res.oid
                    );
                }
                ArchitectOrdersWsMessage::CancelOrderResponse(resp) => {
                    tracing::info!(
                        "Cancel order response: rid={} accepted={}",
                        resp.rid,
                        resp.res.cxl_rx
                    );
                }
                ArchitectOrdersWsMessage::OpenOrdersResponse(resp) => {
                    tracing::info!("Open orders: {} orders", resp.res.len());
                    for order in &resp.res {
                        tracing::info!(
                            "  {} {} {:?} {} @ {} ({:?})",
                            order.oid,
                            order.s,
                            order.d,
                            order.q,
                            order.p,
                            order.o
                        );
                    }
                }
                ArchitectOrdersWsMessage::OrderStatusReports(reports) => {
                    tracing::info!("Order status reports: {} items", reports.len());
                }
                ArchitectOrdersWsMessage::FillReports(reports) => {
                    tracing::info!("Fill reports: {} items", reports.len());
                }
                ArchitectOrdersWsMessage::OrderRejected(reject) => {
                    tracing::warn!("Order rejected: {}", reject.client_order_id);
                }
                ArchitectOrdersWsMessage::OrderCancelRejected(reject) => {
                    tracing::warn!("Cancel rejected: {}", reject.client_order_id);
                }
                ArchitectOrdersWsMessage::Error(err) => {
                    tracing::error!("Error: {}", err.message);
                }
                ArchitectOrdersWsMessage::Reconnected => {
                    tracing::warn!("Reconnected");
                }
            }

            if start.elapsed() > timeout {
                tracing::info!("Timeout reached");
                break;
            }
        }
    }

    tracing::info!("Disconnecting...");
    client.disconnect().await;

    tracing::info!("Received {message_count} messages");
    tracing::info!("Done");

    Ok(())
}
