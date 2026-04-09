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

Coinbase Advanced Trade uses CDP (Coinbase Developer Platform) API keys with
ES256 JWT authentication. Each request generates a short-lived JWT signed with
your EC private key.

You can create API keys at
[Coinbase Developer Platform](https://portal.cdp.coinbase.com/).

### Environment variables

- `COINBASE_API_KEY`: CDP API key name
- `COINBASE_API_SECRET`: CDP API secret (PEM format)

### Environments

The adapter supports both production and sandbox environments via the
`CoinbaseEnvironment` enum:

| Environment | REST base URL                      |
| :---------- | :--------------------------------- |
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
