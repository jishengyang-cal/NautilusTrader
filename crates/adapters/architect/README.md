# nautilus-architect

[![build](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml/badge.svg?branch=master)](https://github.com/nautechsystems/nautilus_trader/actions/workflows/build.yml)
[![Documentation](https://img.shields.io/docsrs/nautilus-architect)](https://docs.rs/nautilus-architect/latest/nautilus-architect/)
[![crates.io version](https://img.shields.io/crates/v/nautilus-architect.svg)](https://crates.io/crates/nautilus-architect)
![license](https://img.shields.io/github/license/nautechsystems/nautilus_trader?color=blue)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/NautilusTrader)

[NautilusTrader](http://nautilustrader.io) adapter for [Architect](https://architect.exchange)
perpetual futures exchange and multi-asset brokerage.

## Overview

[Architect](https://architect.exchange) builds modern infrastructure for professional and
institutional trading. This crate provides connectivity to two Architect products:

- **AX Exchange** (`AX`): A regulated perpetual futures exchange for traditional asset classes.
- **Architect Brokerage** (`ARCHITECT`): A US-regulated multi-asset brokerage for equities,
  futures, and options.

## AX Exchange

[AX Exchange](https://architect.exchange) is the world's first centralized and regulated exchange
for perpetual futures on traditional underlying asset classes (FX, rates, metals, energy, stock
indexes). Designed for institutional and professional traders, it combines innovations from digital
asset perpetual exchanges with the safety and risk management of traditional futures exchanges.
Licensed under the Bermuda Monetary Authority.

## Architect Brokerage

[Architect](https://architect.co) operates a US-regulated institutional multi-asset brokerage
offering equities, futures, and options trading with full-featured APIs designed for professional
traders and trading firms. Architect Securities LLC is an SEC-registered broker-dealer
(FINRA/SIPC member), and Architect Financial Derivatives LLC is an NFA-registered introducing
broker.

The brokerage integration is planned for future development. The current implementation focuses on
AX Exchange perpetual futures.

## Platform

[NautilusTrader](http://nautilustrader.io) is an open-source, high-performance, production-grade
algorithmic trading platform, providing quantitative traders with the ability to backtest
portfolios of automated trading strategies on historical data with an event-driven engine,
and also deploy those same strategies live, with no code changes.

NautilusTrader's design, architecture, and implementation philosophy prioritizes software
correctness and safety at the highest level, with the aim of supporting mission-critical trading
system backtesting and live deployment workloads.

## Product support

| Integration | Product Type      | Data Feed | Trading | Notes                                      |
|-------------|-------------------|-----------|---------|--------------------------------------------|
| AX          | Perpetual Futures | ✓         | ✓       | FX, rates, metals, and traditional assets. |
| ARCHITECT   | Futures           | -         | -       | *Not yet supported.* Planned.              |
| ARCHITECT   | Equities          | -         | -       | *Not yet supported.* Planned.              |
| ARCHITECT   | Options           | -         | -       | *Not yet supported.* Planned.              |

## Feature flags

This crate provides feature flags to control source code inclusion during compilation:

- `python`: Enables Python bindings from [PyO3](https://pyo3.rs).
- `extension-module`: Builds as a Python extension module (used with `python`).

## Documentation

- [Crate docs](https://docs.rs/nautilus-architect)
- [API reference](https://docs.sandbox.x.architect.co/api-reference/)
- [AX Exchange](https://architect.exchange/)
- [Architect Brokerage](https://architect.co/)

## Authentication

Architect uses bearer token authentication via HTTP headers:

1. API key and secret (with optional TOTP) obtain a session token via `/authenticate`.
2. The session token is used as a bearer token for subsequent REST and WebSocket requests.

## API endpoints

| Environment | HTTP API                                         | Market Data WebSocket                            | Orders WebSocket                                     |
|-------------|--------------------------------------------------|--------------------------------------------------|------------------------------------------------------|
| Sandbox     | `https://gateway.sandbox.architect.exchange/api` | `wss://gateway.sandbox.architect.exchange/md/ws` | `wss://gateway.sandbox.architect.exchange/orders/ws` |
| Production  | `https://gateway.architect.exchange/api`         | `wss://gateway.architect.exchange/md/ws`         | `wss://gateway.architect.exchange/orders/ws`         |

## Usage

Run example binaries to test the adapter:

```bash
# HTTP client example
cargo run -p nautilus-architect --bin architect-http-public

# WebSocket data client example
cargo run -p nautilus-architect --bin architect-ws-data

# WebSocket orders client example
cargo run -p nautilus-architect --bin architect-ws-orders
```

## License

The source code for NautilusTrader is available on GitHub under the [GNU Lesser General Public License v3.0](https://www.gnu.org/licenses/lgpl-3.0.en.html).
Contributions to the project are welcome and require the completion of a standard [Contributor License Agreement (CLA)](https://github.com/nautechsystems/nautilus_trader/blob/develop/CLA.md).

---

NautilusTrader™ is developed and maintained by Nautech Systems, a technology
company specializing in the development of high-performance trading systems.
For more information, visit <https://nautilustrader.io>.

<img src="https://github.com/nautechsystems/nautilus_trader/raw/develop/assets/nautilus-logo-white.png" alt="logo" width="400" height="auto"/>

© 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
