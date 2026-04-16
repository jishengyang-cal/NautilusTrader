# Coinbase

Founded in 2012, Coinbase is one of the largest cryptocurrency exchanges,
offering trading across spot, futures, and perpetual markets. This integration
supports live market data ingest via the
[Advanced Trade API](https://docs.cdp.coinbase.com/coinbase-app/docs/advanced-trade-apis).

## Overview

This adapter is implemented in Rust with Python bindings. It provides direct
integration with Coinbase's REST and WebSocket APIs without requiring external
client libraries.

:::info
This adapter is under active development. The Rust data layer (HTTP, WebSocket,
instrument provider, data client) is available. The execution client and Python
integration are not yet implemented.
:::

The WebSocket layer can deserialize `user`, `status`, and
`futures_balance_summary` payloads. The current data client ignores those
channels until execution, account-state, and venue-status wiring lands.

The following components are available:

- `CoinbaseHttpClient`: Low-level HTTP API connectivity.
- `CoinbaseWebSocketClient`: Low-level WebSocket API connectivity.
- `CoinbaseInstrumentProvider`: Instrument parsing and loading functionality.
- `CoinbaseDataClient`: A market data feed manager.

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to work with these lower level components directly.
:::

## Coinbase documentation

Coinbase provides documentation for the Advanced Trade API:

- [REST API reference](https://docs.cdp.coinbase.com/advanced-trade/reference)
- [WebSocket channels](https://docs.cdp.coinbase.com/advanced-trade/docs/ws-channels)
- [API key setup](https://docs.cdp.coinbase.com/coinbase-app/docs/api-key-authentication)

## Products

| Product Type        | Supported | Notes                             |
|---------------------|-----------|-----------------------------------|
| Spot                | ✓         | Direct crypto trading.            |
| Perpetual contracts | ✓         | USD-margined perpetual swaps.     |
| Futures contracts   | ✓         | Dated delivery futures.           |

## Authentication

Coinbase Advanced Trade uses ES256 JWT authentication. Each request generates
a short-lived JWT signed with your EC private key. The adapter resolves
credentials from environment variables or from the config fields.

### Creating an API key

Coinbase has several key types. The adapter requires a **Coinbase App Secret
API key** with the **ECDSA** signature algorithm (not Ed25519).

<Steps>
<Step>
Go to the CDP portal API keys page:
[portal.cdp.coinbase.com/projects/api-keys](https://portal.cdp.coinbase.com/projects/api-keys).
</Step>
<Step>
Select the **Secret API Keys** tab and click **Create API key**.
</Step>
<Step>
Enter a nickname (e.g. `nautilus-trading`).
</Step>
<Step>
Expand **API restrictions** and set permissions to **View** and **Trade**.
</Step>
<Step>
Expand **Advanced Settings** and change the signature algorithm from Ed25519
to **ECDSA**. This step is required: Ed25519 keys do not work with the
Advanced Trade API.
</Step>
<Step>
Click **Create API key**. Save the key name and private key from the modal.
The key name looks like `organizations/{org_id}/apiKeys/{key_id}`. The
private key is a PEM-encoded EC key (SEC1 format).
</Step>
</Steps>

:::warning
Coinbase no longer auto-downloads the key file. Copy the values from the
creation modal or click the download button before closing it. You cannot
retrieve the private key afterward.
:::

:::info
Do not use legacy API keys from coinbase.com/settings/api (UUID format with
HMAC-SHA256 signing). Those use a different auth scheme (`CB-ACCESS-*`
headers) that the adapter does not support.
:::

For full details see the Coinbase
[API key authentication guide](https://docs.cdp.coinbase.com/coinbase-app/authentication-authorization/api-key-authentication).

### Environment variables

| Variable              | Description                                               |
|-----------------------|-----------------------------------------------------------|
| `COINBASE_API_KEY`    | Key name (`organizations/{org_id}/apiKeys/{key_id}`).     |
| `COINBASE_API_SECRET` | PEM‑encoded EC private key (the full multi‑line string).  |

Example:

```bash
export COINBASE_API_KEY="organizations/abc-123/apiKeys/def-456"
export COINBASE_API_SECRET="$(cat ~/path/to/cdp_api_key.pem)"
```

### Environments

The adapter supports both production and sandbox environments via the
`CoinbaseEnvironment` enum:

| Environment | REST base URL                      |
|-------------|------------------------------------|
| `Live`      | `https://api.coinbase.com`         |
| `Sandbox`   | `https://api-sandbox.coinbase.com` |

## Configuration

### Data client

| Option                             | Default | Description                                   |
|------------------------------------|---------|-----------------------------------------------|
| `api_key`                          | `None`  | Falls back to `COINBASE_API_KEY` env var.     |
| `api_secret`                       | `None`  | Falls back to `COINBASE_API_SECRET` env var.  |
| `base_url_rest`                    | `None`  | Override for the REST base URL.               |
| `base_url_ws`                      | `None`  | Override for the WebSocket market data URL.   |
| `environment`                      | `Live`  | `Live` or `Sandbox`.                          |
| `http_timeout_secs`                | `10`    | HTTP request timeout in seconds.              |
| `ws_timeout_secs`                  | `30`    | WebSocket timeout in seconds.                 |
| `update_instruments_interval_mins` | `60`    | Interval for refreshing instruments.          |

## Contributing

For additional features or to contribute to the Coinbase adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
