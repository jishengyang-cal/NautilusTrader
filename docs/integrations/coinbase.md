# Coinbase

[Coinbase](https://coinbase.com) is one of the largest cryptocurrency exchanges,
offering trading across spot, futures, and perpetual markets via the
[Advanced Trade API](https://docs.cdp.coinbase.com/coinbase-app/docs/advanced-trade-apis).

## Overview

This adapter is implemented in Rust with Python bindings. It provides direct
integration with Coinbase's REST API without requiring external client libraries.

:::info
This adapter is under active development. The Rust HTTP client and parsing layer
are available. WebSocket streaming, data client, execution client, and Python
integration are not yet implemented.
:::

The following components are available:

- `CoinbaseHttpClient`: HTTP API connectivity with instrument caching.
- `CoinbaseRawHttpClient`: Low-level HTTP endpoint methods and JWT authentication.

## Instrument types

| Coinbase product type | Nautilus instrument type |
| :-------------------- | :---------------------- |
| Spot                  | `CurrencyPair`          |
| Future                | `CryptoFuture`          |
| Perpetual             | `CryptoPerpetual`       |

## Authentication

Coinbase Advanced Trade uses CDP (Coinbase Developer Platform) API keys with
ES256 JWT authentication. Each request generates a short-lived JWT signed with
your EC private key.

You can create API keys at
[Coinbase Developer Platform](https://portal.cdp.coinbase.com/).

### Environment variables

| Variable              | Description                   | Required |
| :-------------------- | :---------------------------- | :------: |
| `COINBASE_API_KEY`    | CDP API key name              | Yes      |
| `COINBASE_API_SECRET` | CDP API secret (PEM format)   | Yes      |

### Environments

The adapter supports both production and sandbox environments via the
`CoinbaseEnvironment` enum:

| Environment | REST base URL                        |
| :---------- | :----------------------------------- |
| `Live`      | `https://api.coinbase.com`           |
| `Sandbox`   | `https://api-sandbox.coinbase.com`   |

## REST API coverage

### Public endpoints

| Endpoint          | Method                         |
| :---------------- | :----------------------------- |
| List products     | `get_products()`               |
| Get product       | `get_product(product_id)`      |
| Get candles       | `get_candles(product_id, ...)` |
| Get market trades | `get_market_trades(product_id, limit)` |
| Best bid/ask      | `get_best_bid_ask(product_ids)` |
| Product book      | `get_product_book(product_id, limit)` |

### Authenticated endpoints

| Endpoint              | Method                       |
| :-------------------- | :--------------------------- |
| List accounts         | `get_accounts()`             |
| Get account           | `get_account(account_id)`    |
| Create order          | `create_order(order)`        |
| Cancel orders         | `cancel_orders(order_ids)`   |
| List orders           | `get_orders(query)`          |
| Get order             | `get_order(order_id)`        |
| List fills            | `get_fills(query)`           |
| Transaction summary   | `get_transaction_summary()`  |

## Parsing

The adapter parses Coinbase API responses into Nautilus types:

| Coinbase data    | Nautilus type       |
| :--------------- | :------------------ |
| Product          | `InstrumentAny`     |
| Trade            | `TradeTick`         |
| Candle           | `Bar`               |
| Product book     | `OrderBookDeltas`   |

## Contributing

For additional features or to contribute to the Coinbase adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
