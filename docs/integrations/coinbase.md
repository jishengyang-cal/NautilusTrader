# Coinbase

[Coinbase](https://coinbase.com) is one of the largest cryptocurrency exchanges,
offering trading across spot, futures, and perpetual markets via the
[Advanced Trade API](https://docs.cdp.coinbase.com/coinbase-app/docs/advanced-trade-apis).

## Overview

The adapter connects to Coinbase through both REST and WebSocket APIs,
providing market data streaming and order execution.

- `CoinbaseHttpClient`: Low-level HTTP API connectivity.
- `CoinbaseWebSocketClient`: Low-level WebSocket API connectivity.
- `CoinbaseInstrumentProvider`: Instrument parsing and loading.
- `CoinbaseDataClient`: Market data feed manager.
- `CoinbaseExecutionClient`: Trade execution gateway.
- `CoinbaseLiveDataClientFactory`: Factory for Coinbase data clients.
- `CoinbaseLiveExecClientFactory`: Factory for Coinbase execution clients.

:::note
Most users define a configuration for a live trading node rather than
working directly with these lower-level components.
:::

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

You can create API keys at [Coinbase Developer Platform](https://portal.cdp.coinbase.com/).

### Environment variables

| Variable            | Description                  | Required |
| :------------------ | :--------------------------- | :------: |
| `COINBASE_API_KEY`  | CDP API key name             | Yes      |
| `COINBASE_API_SECRET` | CDP API secret (PEM format) | Yes      |

## Data

### Supported data types

| Data type           | Supported |
| :------------------ | :-------: |
| `OrderBookDelta`    | ✓         |
| `OrderBookDepth10`  | ✓         |
| `QuoteTick`         | ✓         |
| `TradeTick`         | ✓         |
| `Bar`               | ✓         |

### WebSocket channels

| Channel               | Description              |
| :-------------------- | :----------------------- |
| `level2`              | Order book updates       |
| `market_trades`       | Trade executions         |
| `ticker`              | Price ticker             |
| `ticker_batch`        | Batched ticker updates   |
| `candles`             | OHLC bar data            |
| `heartbeats`          | Connection keepalive     |

## Execution

### Supported order types

| Order type       | Supported |
| :--------------- | :-------: |
| `MARKET`         | ✓         |
| `LIMIT`          | ✓         |
| `STOP_LIMIT`     | ✓         |

### Supported time in force

| Time in force | Supported |
| :------------ | :-------: |
| `GTC`         | ✓         |
| `GTD`         | ✓         |
| `IOC`         | ✓         |
| `FOK`         | ✓         |

## Configuration

:::info
Configuration details will be added as the adapter matures.
:::

## Contributing

:::info
For additional features or to contribute to the Coinbase adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
