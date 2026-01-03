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

//! Manual verification script for Architect HTTP public endpoints.
//!
//! Tests the instruments endpoint to verify connectivity and response parsing.
//! Defaults to sandbox environment.
//!
//! Usage:
//! ```bash
//! cargo run --bin architect-http-public -p nautilus-architect
//! ```

use nautilus_architect::{
    common::consts::{
        ARCHITECT_HTTP_SANDBOX_URL, ARCHITECT_HTTP_URL, ARCHITECT_ORDERS_SANDBOX_URL,
        ARCHITECT_ORDERS_URL,
    },
    http::client::ArchitectRawHttpClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let is_sandbox = std::env::var("ARCHITECT_IS_SANDBOX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(true);

    let (base_url, orders_base_url) = if is_sandbox {
        (ARCHITECT_HTTP_SANDBOX_URL, ARCHITECT_ORDERS_SANDBOX_URL)
    } else {
        (ARCHITECT_HTTP_URL, ARCHITECT_ORDERS_URL)
    };

    tracing::info!("Connecting to Architect HTTP API: {base_url}");
    tracing::info!(
        "Environment: {}",
        if is_sandbox { "SANDBOX" } else { "PRODUCTION" }
    );

    let client = ArchitectRawHttpClient::new(
        Some(base_url.to_string()),
        Some(orders_base_url.to_string()),
        Some(30),
        None,
        None,
        None,
        None,
    )?;

    tracing::info!("Fetching all instruments...");
    let start = std::time::Instant::now();
    let instruments_response = client.get_instruments().await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "Fetched {} instruments in {:.2}s",
        instruments_response.instruments.len(),
        elapsed.as_secs_f64()
    );

    for inst in instruments_response.instruments.iter().take(5) {
        tracing::info!(
            "  {} ({:?}) tick={} min_size={}",
            inst.symbol,
            inst.state,
            inst.tick_size,
            inst.minimum_order_size
        );
    }
    if instruments_response.instruments.len() > 5 {
        tracing::info!(
            "  ... and {} more",
            instruments_response.instruments.len() - 5
        );
    }

    let test_symbol = instruments_response
        .instruments
        .first()
        .map_or("EURUSD-PERP", |i| i.symbol.as_str());

    tracing::info!("Fetching single instrument: {test_symbol}");
    let start = std::time::Instant::now();
    let instrument = client.get_instrument(test_symbol).await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "Fetched {} in {:.2}s",
        instrument.symbol,
        elapsed.as_secs_f64()
    );
    tracing::info!("  State: {:?}", instrument.state);
    tracing::info!("  Tick size: {}", instrument.tick_size);
    tracing::info!("  Min order size: {}", instrument.minimum_order_size);
    tracing::info!("  Quote currency: {}", instrument.quote_currency);
    tracing::info!("  Multiplier: {}", instrument.multiplier);

    tracing::info!("Done");

    Ok(())
}
