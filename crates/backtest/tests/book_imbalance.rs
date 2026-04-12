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

#![cfg(feature = "examples")]

use ahash::AHashMap;
use nautilus_backtest::{config::BacktestEngineConfig, engine::BacktestEngine};
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny};
use nautilus_model::{
    data::{Data, QuoteTick},
    enums::{AccountType, BookType, OmsType},
    identifiers::{InstrumentId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Money, Price, Quantity},
};
use nautilus_trading::examples::actors::{BookImbalanceActor, BookImbalanceActorConfig};
use rstest::*;

fn create_engine() -> BacktestEngine {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();
    engine
        .add_venue(
            Venue::from("BINANCE"),
            OmsType::Netting,
            AccountType::Margin,
            BookType::L1_MBP,
            vec![Money::from("1_000_000 USDT")],
            None,
            None,
            AHashMap::new(),
            None,
            vec![],
            FillModelAny::default(),
            FeeModelAny::default(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    engine
}

fn quote(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> Data {
    Data::Quote(QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        ts.into(),
        ts.into(),
    ))
}

#[rstest]
fn test_from_config_registers_and_runs(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let config = BookImbalanceActorConfig::new(vec![instrument_id]).with_log_interval(0);
    engine
        .add_actor(BookImbalanceActor::from_config(config))
        .unwrap();

    let quotes: Vec<Data> = (0..10u64)
        .map(|i| {
            quote(
                instrument_id,
                "999.95",
                "1000.05",
                1_000_000_000 + i * 1_000_000_000,
            )
        })
        .collect();
    engine.add_data(quotes, None, true, true);

    engine.run(None, None, None, false).unwrap();
    assert_eq!(engine.get_result().iterations, 10);
}
