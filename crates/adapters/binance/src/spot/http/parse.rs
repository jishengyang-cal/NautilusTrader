// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! SBE decode functions for Binance Spot HTTP responses.
//!
//! Each function decodes raw SBE bytes into domain types, validating the
//! message header (schema ID, version, template ID) before extracting fields.

use super::{
    error::SbeDecodeError,
    models::{
        BinanceAccountInfo, BinanceAccountTrade, BinanceBalance, BinanceCancelOrderResponse,
        BinanceDepth, BinanceNewOrderResponse, BinanceOrderFill, BinanceOrderResponse,
        BinancePriceLevel, BinanceTrade, BinanceTrades,
    },
};
use crate::common::sbe::spot::{
    SBE_SCHEMA_ID, SBE_SCHEMA_VERSION,
    account_response_codec::SBE_TEMPLATE_ID as ACCOUNT_TEMPLATE_ID,
    account_trades_response_codec::SBE_TEMPLATE_ID as ACCOUNT_TRADES_TEMPLATE_ID,
    account_type::AccountType, bool_enum::BoolEnum,
    cancel_open_orders_response_codec::SBE_TEMPLATE_ID as CANCEL_OPEN_ORDERS_TEMPLATE_ID,
    cancel_order_response_codec::SBE_TEMPLATE_ID as CANCEL_ORDER_TEMPLATE_ID,
    depth_response_codec::SBE_TEMPLATE_ID as DEPTH_TEMPLATE_ID,
    message_header_codec::ENCODED_LENGTH as HEADER_LENGTH,
    new_order_full_response_codec::SBE_TEMPLATE_ID as NEW_ORDER_FULL_TEMPLATE_ID,
    order_response_codec::SBE_TEMPLATE_ID as ORDER_TEMPLATE_ID, order_side::OrderSide,
    order_status::OrderStatus, order_type::OrderType,
    orders_response_codec::SBE_TEMPLATE_ID as ORDERS_TEMPLATE_ID,
    ping_response_codec::SBE_TEMPLATE_ID as PING_TEMPLATE_ID,
    self_trade_prevention_mode::SelfTradePreventionMode,
    server_time_response_codec::SBE_TEMPLATE_ID as SERVER_TIME_TEMPLATE_ID,
    time_in_force::TimeInForce, trades_response_codec::SBE_TEMPLATE_ID as TRADES_TEMPLATE_ID,
};

/// Group size encoding length (u16 block_length + u32 num_in_group).
const GROUP_SIZE_LENGTH: usize = 6;

/// Maximum allowed group size to prevent OOM from malicious payloads.
const MAX_GROUP_SIZE: u32 = 10_000;

/// SBE message header.
#[derive(Debug, Clone, Copy)]
struct MessageHeader {
    #[allow(dead_code)]
    block_length: u16,
    template_id: u16,
    schema_id: u16,
    version: u16,
}

impl MessageHeader {
    /// Decode message header from buffer.
    fn decode(buf: &[u8]) -> Result<Self, SbeDecodeError> {
        if buf.len() < HEADER_LENGTH {
            return Err(SbeDecodeError::BufferTooShort {
                expected: HEADER_LENGTH,
                actual: buf.len(),
            });
        }
        Ok(Self {
            block_length: u16::from_le_bytes([buf[0], buf[1]]),
            template_id: u16::from_le_bytes([buf[2], buf[3]]),
            schema_id: u16::from_le_bytes([buf[4], buf[5]]),
            version: u16::from_le_bytes([buf[6], buf[7]]),
        })
    }

    /// Validate schema ID and version.
    fn validate(&self) -> Result<(), SbeDecodeError> {
        if self.schema_id != SBE_SCHEMA_ID {
            return Err(SbeDecodeError::SchemaMismatch {
                expected: SBE_SCHEMA_ID,
                actual: self.schema_id,
            });
        }
        if self.version != SBE_SCHEMA_VERSION {
            return Err(SbeDecodeError::VersionMismatch {
                expected: SBE_SCHEMA_VERSION,
                actual: self.version,
            });
        }
        Ok(())
    }
}

/// Decode a ping response.
///
/// Ping response has no body (block_length = 0), just validates the header.
///
/// # Errors
///
/// Returns error if buffer is too short or schema mismatch.
pub fn decode_ping(buf: &[u8]) -> Result<(), SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != PING_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    Ok(())
}

/// Decode a server time response.
///
/// Returns the server time as **microseconds** since epoch (SBE provides
/// microsecond precision vs JSON's milliseconds).
///
/// # Errors
///
/// Returns error if buffer is too short or schema mismatch.
///
/// # Panics
///
/// This function will not panic as buffer lengths are validated before slicing.
pub fn decode_server_time(buf: &[u8]) -> Result<i64, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != SERVER_TIME_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let body_start = HEADER_LENGTH;
    let body_end = body_start + 8;

    if buf.len() < body_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: body_end,
            actual: buf.len(),
        });
    }

    // SAFETY: Length validated above
    let server_time = i64::from_le_bytes(buf[body_start..body_end].try_into().expect("slice len"));
    Ok(server_time)
}

/// Decode a depth response.
///
/// Returns the order book depth with bids and asks.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or group size exceeded.
///
/// # Panics
///
/// This function will not panic as buffer lengths are validated before slicing.
pub fn decode_depth(buf: &[u8]) -> Result<BinanceDepth, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != DEPTH_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Depth block: last_update_id (8) + price_exponent (1) + qty_exponent (1) = 10 bytes
    let block_start = HEADER_LENGTH;
    let block_end = block_start + 10;

    if buf.len() < block_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_end,
            actual: buf.len(),
        });
    }

    // SAFETY: Length validated above
    let last_update_id = i64::from_le_bytes(
        buf[block_start..block_start + 8]
            .try_into()
            .expect("slice len"),
    );
    let price_exponent = buf[block_start + 8] as i8;
    let qty_exponent = buf[block_start + 9] as i8;

    let (bids, bids_end) = decode_price_levels(&buf[block_end..])?;
    let (asks, _asks_end) = decode_price_levels(&buf[block_end + bids_end..])?;

    Ok(BinanceDepth {
        last_update_id,
        price_exponent,
        qty_exponent,
        bids,
        asks,
    })
}

/// Decode a trades response.
///
/// Returns the list of trades.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or group size exceeded.
pub fn decode_trades(buf: &[u8]) -> Result<BinanceTrades, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != TRADES_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Trades block: price_exponent (1) + qty_exponent (1) = 2 bytes
    let block_start = HEADER_LENGTH;
    let block_end = block_start + 2;

    if buf.len() < block_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_end,
            actual: buf.len(),
        });
    }

    let price_exponent = buf[block_start] as i8;
    let qty_exponent = buf[block_start + 1] as i8;

    let trades = decode_trades_group(&buf[block_end..])?;

    Ok(BinanceTrades {
        price_exponent,
        qty_exponent,
        trades,
    })
}

/// Decode a group of price levels (bids or asks).
///
/// Returns (levels, bytes_consumed).
fn decode_price_levels(buf: &[u8]) -> Result<(Vec<BinancePriceLevel>, usize), SbeDecodeError> {
    if buf.len() < GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let _block_length = u16::from_le_bytes([buf[0], buf[1]]);
    let count = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);

    if count > MAX_GROUP_SIZE {
        return Err(SbeDecodeError::GroupSizeTooLarge {
            count,
            max: MAX_GROUP_SIZE,
        });
    }

    let level_size = 16; // price (8) + qty (8)
    let total_size = GROUP_SIZE_LENGTH + (count as usize * level_size);

    if buf.len() < total_size {
        return Err(SbeDecodeError::BufferTooShort {
            expected: total_size,
            actual: buf.len(),
        });
    }

    let mut levels = Vec::with_capacity(count as usize);
    let mut offset = GROUP_SIZE_LENGTH;

    for _ in 0..count {
        let price_mantissa = i64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
        let qty_mantissa = i64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());

        levels.push(BinancePriceLevel {
            price_mantissa,
            qty_mantissa,
        });

        offset += level_size;
    }

    Ok((levels, total_size))
}

/// Decode a group of trades.
fn decode_trades_group(buf: &[u8]) -> Result<Vec<BinanceTrade>, SbeDecodeError> {
    if buf.len() < GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let _block_length = u16::from_le_bytes([buf[0], buf[1]]);
    let count = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);

    if count > MAX_GROUP_SIZE {
        return Err(SbeDecodeError::GroupSizeTooLarge {
            count,
            max: MAX_GROUP_SIZE,
        });
    }

    // Trade: id(8) + price(8) + qty(8) + quoteQty(8) + time(8) + isBuyerMaker(1) + isBestMatch(1) = 42
    let trade_size = 42;
    let total_size = GROUP_SIZE_LENGTH + (count as usize * trade_size);

    if buf.len() < total_size {
        return Err(SbeDecodeError::BufferTooShort {
            expected: total_size,
            actual: buf.len(),
        });
    }

    let mut trades = Vec::with_capacity(count as usize);
    let mut offset = GROUP_SIZE_LENGTH;

    for _ in 0..count {
        let id = i64::from_le_bytes(buf[offset..offset + 8].try_into().unwrap());
        let price_mantissa = i64::from_le_bytes(buf[offset + 8..offset + 16].try_into().unwrap());
        let qty_mantissa = i64::from_le_bytes(buf[offset + 16..offset + 24].try_into().unwrap());
        let quote_qty_mantissa =
            i64::from_le_bytes(buf[offset + 24..offset + 32].try_into().unwrap());
        let time = i64::from_le_bytes(buf[offset + 32..offset + 40].try_into().unwrap());
        let is_buyer_maker = BoolEnum::from(buf[offset + 40]) == BoolEnum::True;
        let is_best_match = BoolEnum::from(buf[offset + 41]) == BoolEnum::True;

        trades.push(BinanceTrade {
            id,
            price_mantissa,
            qty_mantissa,
            quote_qty_mantissa,
            time,
            is_buyer_maker,
            is_best_match,
        });

        offset += trade_size;
    }

    Ok(trades)
}

/// Block length for new order full response.
const NEW_ORDER_FULL_BLOCK_LENGTH: usize = 153;

/// Block length for cancel order response.
const CANCEL_ORDER_BLOCK_LENGTH: usize = 137;

/// Block length for order response (query).
const ORDER_BLOCK_LENGTH: usize = 153;

/// Decode a new order full response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
///
/// # Panics
///
/// This function will not panic in practice as buffer bounds are validated
/// before any slice operations.
#[allow(dead_code)]
pub fn decode_new_order_full(buf: &[u8]) -> Result<BinanceNewOrderResponse, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != NEW_ORDER_FULL_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let block_start = HEADER_LENGTH;
    let block_end = block_start + NEW_ORDER_FULL_BLOCK_LENGTH;

    if buf.len() < block_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_end,
            actual: buf.len(),
        });
    }

    let price_exponent = buf[block_start] as i8;
    let qty_exponent = buf[block_start + 1] as i8;
    let order_id = i64::from_le_bytes(buf[block_start + 2..block_start + 10].try_into().unwrap());
    let order_list_id_raw =
        i64::from_le_bytes(buf[block_start + 10..block_start + 18].try_into().unwrap());
    let order_list_id = if order_list_id_raw == i64::MIN {
        None
    } else {
        Some(order_list_id_raw)
    };
    let transact_time =
        i64::from_le_bytes(buf[block_start + 18..block_start + 26].try_into().unwrap());
    let price_mantissa =
        i64::from_le_bytes(buf[block_start + 26..block_start + 34].try_into().unwrap());
    let orig_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 34..block_start + 42].try_into().unwrap());
    let executed_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 42..block_start + 50].try_into().unwrap());
    let cummulative_quote_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 50..block_start + 58].try_into().unwrap());
    let status = buf[block_start + 58].into();
    let time_in_force = buf[block_start + 59].into();
    let order_type = buf[block_start + 60].into();
    let side = buf[block_start + 61].into();
    let stop_price_raw =
        i64::from_le_bytes(buf[block_start + 62..block_start + 70].try_into().unwrap());
    let stop_price_mantissa = if stop_price_raw == i64::MIN {
        None
    } else {
        Some(stop_price_raw)
    };

    // Skip trailing_delta (70-78), trailing_time (78-86)
    let working_time_raw =
        i64::from_le_bytes(buf[block_start + 86..block_start + 94].try_into().unwrap());
    let working_time = if working_time_raw == i64::MIN {
        None
    } else {
        Some(working_time_raw)
    };

    // Skip iceberg_qty (94-102), strategy_id (102-110), strategy_type (110-114),
    // order_capacity (114), working_floor (115), used_sor (116)
    let self_trade_prevention_mode = buf[block_start + 117].into();

    // Skip trade_group_id (118-126), prevented_quantity (126-134)
    // commissionExponent at offset 134
    let commission_exponent = buf[block_start + 134] as i8;

    // Parse fills group (starts after fixed block)
    let mut offset = block_end;
    let (fills, fills_bytes) = decode_order_fills(&buf[offset..], commission_exponent)?;
    offset += fills_bytes;

    // Skip prevented matches group
    let prevented_group_size = decode_group_size(&buf[offset..])?;
    offset += GROUP_SIZE_LENGTH + prevented_group_size.1 as usize * prevented_group_size.0 as usize;

    let (symbol, symbol_len) = decode_var_string(&buf[offset..])?;
    offset += 1 + symbol_len;

    let (client_order_id, _) = decode_var_string(&buf[offset..])?;

    Ok(BinanceNewOrderResponse {
        price_exponent,
        qty_exponent,
        order_id,
        order_list_id,
        transact_time,
        price_mantissa,
        orig_qty_mantissa,
        executed_qty_mantissa,
        cummulative_quote_qty_mantissa,
        status,
        time_in_force,
        order_type,
        side,
        stop_price_mantissa,
        working_time,
        self_trade_prevention_mode,
        client_order_id,
        symbol,
        fills,
    })
}

/// Decode a cancel order response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
///
/// # Panics
///
/// This function will not panic in practice as buffer bounds are validated
/// before any slice operations.
#[allow(dead_code)]
pub fn decode_cancel_order(buf: &[u8]) -> Result<BinanceCancelOrderResponse, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != CANCEL_ORDER_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let block_start = HEADER_LENGTH;
    let block_end = block_start + CANCEL_ORDER_BLOCK_LENGTH;

    if buf.len() < block_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_end,
            actual: buf.len(),
        });
    }

    let price_exponent = buf[block_start] as i8;
    let qty_exponent = buf[block_start + 1] as i8;
    let order_id = i64::from_le_bytes(buf[block_start + 2..block_start + 10].try_into().unwrap());
    let order_list_id_raw =
        i64::from_le_bytes(buf[block_start + 10..block_start + 18].try_into().unwrap());
    let order_list_id = if order_list_id_raw == i64::MIN {
        None
    } else {
        Some(order_list_id_raw)
    };
    let transact_time =
        i64::from_le_bytes(buf[block_start + 18..block_start + 26].try_into().unwrap());
    let price_mantissa =
        i64::from_le_bytes(buf[block_start + 26..block_start + 34].try_into().unwrap());
    let orig_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 34..block_start + 42].try_into().unwrap());
    let executed_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 42..block_start + 50].try_into().unwrap());
    let cummulative_quote_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 50..block_start + 58].try_into().unwrap());
    let status = buf[block_start + 58].into();
    let time_in_force = buf[block_start + 59].into();
    let order_type = buf[block_start + 60].into();
    let side = buf[block_start + 61].into();
    let self_trade_prevention_mode = buf[block_start + 62].into();

    let mut offset = block_end;
    let (symbol, symbol_len) = decode_var_string(&buf[offset..])?;
    offset += 1 + symbol_len;

    let (orig_client_order_id, orig_len) = decode_var_string(&buf[offset..])?;
    offset += 1 + orig_len;

    let (client_order_id, _) = decode_var_string(&buf[offset..])?;

    Ok(BinanceCancelOrderResponse {
        price_exponent,
        qty_exponent,
        order_id,
        order_list_id,
        transact_time,
        price_mantissa,
        orig_qty_mantissa,
        executed_qty_mantissa,
        cummulative_quote_qty_mantissa,
        status,
        time_in_force,
        order_type,
        side,
        self_trade_prevention_mode,
        client_order_id,
        orig_client_order_id,
        symbol,
    })
}

/// Decode an order query response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
///
/// # Panics
///
/// This function will not panic in practice as buffer bounds are validated
/// before any slice operations.
#[allow(dead_code)]
pub fn decode_order(buf: &[u8]) -> Result<BinanceOrderResponse, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != ORDER_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let block_start = HEADER_LENGTH;
    let block_end = block_start + ORDER_BLOCK_LENGTH;

    if buf.len() < block_end {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_end,
            actual: buf.len(),
        });
    }

    let price_exponent = buf[block_start] as i8;
    let qty_exponent = buf[block_start + 1] as i8;
    let order_id = i64::from_le_bytes(buf[block_start + 2..block_start + 10].try_into().unwrap());
    let order_list_id_raw =
        i64::from_le_bytes(buf[block_start + 10..block_start + 18].try_into().unwrap());
    let order_list_id = if order_list_id_raw == i64::MIN {
        None
    } else {
        Some(order_list_id_raw)
    };
    let price_mantissa =
        i64::from_le_bytes(buf[block_start + 18..block_start + 26].try_into().unwrap());
    let orig_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 26..block_start + 34].try_into().unwrap());
    let executed_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 34..block_start + 42].try_into().unwrap());
    let cummulative_quote_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 42..block_start + 50].try_into().unwrap());
    let status = buf[block_start + 50].into();
    let time_in_force = buf[block_start + 51].into();
    let order_type = buf[block_start + 52].into();
    let side = buf[block_start + 53].into();
    let stop_price_raw =
        i64::from_le_bytes(buf[block_start + 54..block_start + 62].try_into().unwrap());
    let stop_price_mantissa = if stop_price_raw == i64::MIN {
        None
    } else {
        Some(stop_price_raw)
    };
    let iceberg_qty_raw =
        i64::from_le_bytes(buf[block_start + 62..block_start + 70].try_into().unwrap());
    let iceberg_qty_mantissa = if iceberg_qty_raw == i64::MIN {
        None
    } else {
        Some(iceberg_qty_raw)
    };
    let time = i64::from_le_bytes(buf[block_start + 70..block_start + 78].try_into().unwrap());
    let update_time =
        i64::from_le_bytes(buf[block_start + 78..block_start + 86].try_into().unwrap());
    let is_working = BoolEnum::from(buf[block_start + 86]) == BoolEnum::True;
    let working_time_raw =
        i64::from_le_bytes(buf[block_start + 87..block_start + 95].try_into().unwrap());
    let working_time = if working_time_raw == i64::MIN {
        None
    } else {
        Some(working_time_raw)
    };
    let orig_quote_order_qty_mantissa =
        i64::from_le_bytes(buf[block_start + 95..block_start + 103].try_into().unwrap());
    let self_trade_prevention_mode = buf[block_start + 103].into();

    let mut offset = block_end;
    let (symbol, symbol_len) = decode_var_string(&buf[offset..])?;
    offset += 1 + symbol_len;

    let (client_order_id, _) = decode_var_string(&buf[offset..])?;

    Ok(BinanceOrderResponse {
        price_exponent,
        qty_exponent,
        order_id,
        order_list_id,
        price_mantissa,
        orig_qty_mantissa,
        executed_qty_mantissa,
        cummulative_quote_qty_mantissa,
        status,
        time_in_force,
        order_type,
        side,
        stop_price_mantissa,
        iceberg_qty_mantissa,
        time,
        update_time,
        is_working,
        working_time,
        orig_quote_order_qty_mantissa,
        self_trade_prevention_mode,
        client_order_id,
        symbol,
    })
}

/// Block length for orders group item.
const ORDERS_GROUP_BLOCK_LENGTH: usize = 162;

/// Decode multiple orders response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
///
/// # Panics
///
/// This function will not panic in practice as buffer bounds are validated
/// before any slice operations.
#[allow(dead_code)]
pub fn decode_orders(buf: &[u8]) -> Result<Vec<BinanceOrderResponse>, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != ORDERS_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Orders response has block_length=0 (no root fields), immediately followed by group
    let group_start = HEADER_LENGTH;
    let (block_length, count) = decode_group_size(&buf[group_start..])?;

    if count == 0 {
        return Ok(Vec::new());
    }

    // Validate block length matches expected
    if block_length as usize != ORDERS_GROUP_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: ORDERS_GROUP_BLOCK_LENGTH as u16,
            actual: block_length,
        });
    }

    let mut orders = Vec::with_capacity(count as usize);
    let mut pos = group_start + GROUP_SIZE_LENGTH;

    for _ in 0..count {
        // Each order has fixed block + var-length strings
        if buf.len() < pos + ORDERS_GROUP_BLOCK_LENGTH {
            return Err(SbeDecodeError::BufferTooShort {
                expected: pos + ORDERS_GROUP_BLOCK_LENGTH,
                actual: buf.len(),
            });
        }

        let price_exponent = buf[pos] as i8;
        let qty_exponent = buf[pos + 1] as i8;
        let order_id = i64::from_le_bytes(buf[pos + 2..pos + 10].try_into().unwrap());
        let order_list_id_raw = i64::from_le_bytes(buf[pos + 10..pos + 18].try_into().unwrap());
        let order_list_id = if order_list_id_raw == i64::MIN {
            None
        } else {
            Some(order_list_id_raw)
        };
        let price_mantissa = i64::from_le_bytes(buf[pos + 18..pos + 26].try_into().unwrap());
        let orig_qty_mantissa = i64::from_le_bytes(buf[pos + 26..pos + 34].try_into().unwrap());
        let executed_qty_mantissa = i64::from_le_bytes(buf[pos + 34..pos + 42].try_into().unwrap());
        let cummulative_quote_qty_mantissa =
            i64::from_le_bytes(buf[pos + 42..pos + 50].try_into().unwrap());
        let status = OrderStatus::from(buf[pos + 50]);
        let time_in_force = TimeInForce::from(buf[pos + 51]);
        let order_type = OrderType::from(buf[pos + 52]);
        let side = OrderSide::from(buf[pos + 53]);
        let stop_price_raw = i64::from_le_bytes(buf[pos + 54..pos + 62].try_into().unwrap());
        let stop_price_mantissa = if stop_price_raw == i64::MIN {
            None
        } else {
            Some(stop_price_raw)
        };
        // Skip trailing_delta (62-70) and trailing_time (70-78)
        let iceberg_qty_raw = i64::from_le_bytes(buf[pos + 78..pos + 86].try_into().unwrap());
        let iceberg_qty_mantissa = if iceberg_qty_raw == i64::MIN {
            None
        } else {
            Some(iceberg_qty_raw)
        };
        let time = i64::from_le_bytes(buf[pos + 86..pos + 94].try_into().unwrap());
        let update_time = i64::from_le_bytes(buf[pos + 94..pos + 102].try_into().unwrap());
        let is_working = BoolEnum::from(buf[pos + 102]) == BoolEnum::True;
        let working_time_raw = i64::from_le_bytes(buf[pos + 103..pos + 111].try_into().unwrap());
        let working_time = if working_time_raw == i64::MIN {
            None
        } else {
            Some(working_time_raw)
        };
        let orig_quote_order_qty_mantissa =
            i64::from_le_bytes(buf[pos + 111..pos + 119].try_into().unwrap());
        // Skip strategy_id (119-127), strategy_type (127-131), order_capacity (131), working_floor (132)
        let self_trade_prevention_mode = SelfTradePreventionMode::from(buf[pos + 133]);
        // Skip prevented_match_id (134-142), prevented_quantity (142-150), used_sor (150), peg fields (151-162)

        // Move past fixed block to read var-length strings
        pos += ORDERS_GROUP_BLOCK_LENGTH;

        // Read symbol (u8 length prefix)
        let (symbol, symbol_len) = decode_var_string(&buf[pos..])?;
        pos += 1 + symbol_len;

        // Read client_order_id (u8 length prefix)
        let (client_order_id, client_order_id_len) = decode_var_string(&buf[pos..])?;
        pos += 1 + client_order_id_len;

        orders.push(BinanceOrderResponse {
            price_exponent,
            qty_exponent,
            order_id,
            order_list_id,
            price_mantissa,
            orig_qty_mantissa,
            executed_qty_mantissa,
            cummulative_quote_qty_mantissa,
            status,
            time_in_force,
            order_type,
            side,
            stop_price_mantissa,
            iceberg_qty_mantissa,
            time,
            update_time,
            is_working,
            working_time,
            orig_quote_order_qty_mantissa,
            self_trade_prevention_mode,
            client_order_id,
            symbol,
        });
    }

    Ok(orders)
}

/// Decode cancel open orders response.
///
/// Each item in the response group contains an embedded cancel_order_response SBE message.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
///
/// # Panics
///
/// This function will not panic in practice as buffer bounds are validated
/// before any slice operations.
#[allow(dead_code)]
pub fn decode_cancel_open_orders(
    buf: &[u8],
) -> Result<Vec<BinanceCancelOrderResponse>, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != CANCEL_OPEN_ORDERS_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Response has block_length=0 (no root fields), immediately followed by group
    let group_start = HEADER_LENGTH;
    let (_block_length, count) = decode_group_size(&buf[group_start..])?;

    if count == 0 {
        return Ok(Vec::new());
    }

    let mut responses = Vec::with_capacity(count as usize);
    let mut pos = group_start + GROUP_SIZE_LENGTH;

    // Each group item has block_length=0, followed by u16 length + embedded SBE message
    for _ in 0..count {
        if buf.len() < pos + 2 {
            return Err(SbeDecodeError::BufferTooShort {
                expected: pos + 2,
                actual: buf.len(),
            });
        }
        let response_len = u16::from_le_bytes(buf[pos..pos + 2].try_into().unwrap()) as usize;
        pos += 2;

        if buf.len() < pos + response_len {
            return Err(SbeDecodeError::BufferTooShort {
                expected: pos + response_len,
                actual: buf.len(),
            });
        }

        let embedded_response = &buf[pos..pos + response_len];
        let cancel_response = decode_cancel_order(embedded_response)?;
        responses.push(cancel_response);

        pos += response_len;
    }

    Ok(responses)
}

/// Account response block length (from SBE codec).
const ACCOUNT_BLOCK_LENGTH: usize = 64;

/// Balance group item block length (from SBE codec).
const BALANCE_BLOCK_LENGTH: u16 = 17;

/// Decode account information response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
///
/// # Panics
///
/// This function will not panic in practice as buffer bounds are validated
/// before any slice operations.
#[allow(dead_code)]
pub fn decode_account(buf: &[u8]) -> Result<BinanceAccountInfo, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != ACCOUNT_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    let block_start = HEADER_LENGTH;

    if buf.len() < block_start + ACCOUNT_BLOCK_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: block_start + ACCOUNT_BLOCK_LENGTH,
            actual: buf.len(),
        });
    }

    // Fixed fields (offsets from SBE codec)
    let commission_exponent = buf[block_start] as i8;
    let maker_commission_mantissa =
        i64::from_le_bytes(buf[block_start + 1..block_start + 9].try_into().unwrap());
    let taker_commission_mantissa =
        i64::from_le_bytes(buf[block_start + 9..block_start + 17].try_into().unwrap());
    let buyer_commission_mantissa =
        i64::from_le_bytes(buf[block_start + 17..block_start + 25].try_into().unwrap());
    let seller_commission_mantissa =
        i64::from_le_bytes(buf[block_start + 25..block_start + 33].try_into().unwrap());
    let can_trade = BoolEnum::from(buf[block_start + 33]) == BoolEnum::True;
    let can_withdraw = BoolEnum::from(buf[block_start + 34]) == BoolEnum::True;
    let can_deposit = BoolEnum::from(buf[block_start + 35]) == BoolEnum::True;
    // offset 36 is 'brokered' - skipped
    let require_self_trade_prevention = BoolEnum::from(buf[block_start + 37]) == BoolEnum::True;
    let prevent_sor = BoolEnum::from(buf[block_start + 38]) == BoolEnum::True;
    let update_time =
        i64::from_le_bytes(buf[block_start + 39..block_start + 47].try_into().unwrap());
    let account_type_enum = AccountType::from(buf[block_start + 47]);
    // offsets 48-55: tradeGroupId, 56-63: uid - skipped

    let account_type = account_type_enum.to_string();

    // Parse balances group (starts after fixed block)
    let mut offset = block_start + ACCOUNT_BLOCK_LENGTH;

    if buf.len() < offset + GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: offset + GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let (block_length, balance_count) = decode_group_size(&buf[offset..])?;
    offset += GROUP_SIZE_LENGTH;

    if block_length != BALANCE_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: BALANCE_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    let mut balances = Vec::with_capacity(balance_count as usize);

    for _ in 0..balance_count {
        if buf.len() < offset + block_length as usize {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + block_length as usize,
                actual: buf.len(),
            });
        }

        let exponent = buf[offset] as i8;
        let free_mantissa = i64::from_le_bytes(buf[offset + 1..offset + 9].try_into().unwrap());
        let locked_mantissa = i64::from_le_bytes(buf[offset + 9..offset + 17].try_into().unwrap());
        offset += block_length as usize;

        // Read asset var-length string
        if buf.len() < offset + 1 {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + 1,
                actual: buf.len(),
            });
        }
        let asset_len = buf[offset] as usize;
        offset += 1;

        if buf.len() < offset + asset_len {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + asset_len,
                actual: buf.len(),
            });
        }
        let asset = std::str::from_utf8(&buf[offset..offset + asset_len])
            .map_err(|_| SbeDecodeError::InvalidUtf8)?;
        offset += asset_len;

        balances.push(BinanceBalance {
            asset: asset.to_string(),
            free_mantissa,
            locked_mantissa,
            exponent,
        });
    }

    // Skip permissions and reduceOnlyAssets groups (we don't need them)

    Ok(BinanceAccountInfo {
        commission_exponent,
        maker_commission_mantissa,
        taker_commission_mantissa,
        buyer_commission_mantissa,
        seller_commission_mantissa,
        can_trade,
        can_withdraw,
        can_deposit,
        require_self_trade_prevention,
        prevent_sor,
        update_time,
        account_type,
        balances,
    })
}

/// Account trade group item block length (from SBE codec).
const ACCOUNT_TRADE_BLOCK_LENGTH: u16 = 70;

/// Decode account trades response.
///
/// # Errors
///
/// Returns error if buffer is too short, schema mismatch, or decode error.
///
/// # Panics
///
/// Panics if internal slice operations fail after bounds checking (unreachable).
#[allow(dead_code)]
pub fn decode_account_trades(buf: &[u8]) -> Result<Vec<BinanceAccountTrade>, SbeDecodeError> {
    let header = MessageHeader::decode(buf)?;
    header.validate()?;

    if header.template_id != ACCOUNT_TRADES_TEMPLATE_ID {
        return Err(SbeDecodeError::UnknownTemplateId(header.template_id));
    }

    // Account trades response has block_length=0, immediately followed by trades group
    let group_start = HEADER_LENGTH;

    if buf.len() < group_start + GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: group_start + GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let (block_length, trade_count) = decode_group_size(&buf[group_start..])?;
    let mut offset = group_start + GROUP_SIZE_LENGTH;

    if block_length != ACCOUNT_TRADE_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: ACCOUNT_TRADE_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    let mut trades = Vec::with_capacity(trade_count as usize);

    for _ in 0..trade_count {
        if buf.len() < offset + block_length as usize {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + block_length as usize,
                actual: buf.len(),
            });
        }

        let price_exponent = buf[offset] as i8;
        let qty_exponent = buf[offset + 1] as i8;
        let commission_exponent = buf[offset + 2] as i8;
        let id = i64::from_le_bytes(buf[offset + 3..offset + 11].try_into().unwrap());
        let order_id = i64::from_le_bytes(buf[offset + 11..offset + 19].try_into().unwrap());
        let order_list_id_raw =
            i64::from_le_bytes(buf[offset + 19..offset + 27].try_into().unwrap());
        let order_list_id = if order_list_id_raw == i64::MIN {
            None
        } else {
            Some(order_list_id_raw)
        };
        let price_mantissa = i64::from_le_bytes(buf[offset + 27..offset + 35].try_into().unwrap());
        let qty_mantissa = i64::from_le_bytes(buf[offset + 35..offset + 43].try_into().unwrap());
        let quote_qty_mantissa =
            i64::from_le_bytes(buf[offset + 43..offset + 51].try_into().unwrap());
        let commission_mantissa =
            i64::from_le_bytes(buf[offset + 51..offset + 59].try_into().unwrap());
        let time = i64::from_le_bytes(buf[offset + 59..offset + 67].try_into().unwrap());
        let is_buyer = BoolEnum::from(buf[offset + 67]) == BoolEnum::True;
        let is_maker = BoolEnum::from(buf[offset + 68]) == BoolEnum::True;
        let is_best_match = BoolEnum::from(buf[offset + 69]) == BoolEnum::True;
        offset += block_length as usize;

        // Read symbol var-length string
        if buf.len() < offset + 1 {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + 1,
                actual: buf.len(),
            });
        }
        let symbol_len = buf[offset] as usize;
        offset += 1;

        if buf.len() < offset + symbol_len {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + symbol_len,
                actual: buf.len(),
            });
        }
        let symbol = std::str::from_utf8(&buf[offset..offset + symbol_len])
            .map_err(|_| SbeDecodeError::InvalidUtf8)?;
        offset += symbol_len;

        // Read commissionAsset var-length string
        if buf.len() < offset + 1 {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + 1,
                actual: buf.len(),
            });
        }
        let commission_asset_len = buf[offset] as usize;
        offset += 1;

        if buf.len() < offset + commission_asset_len {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + commission_asset_len,
                actual: buf.len(),
            });
        }
        let commission_asset = std::str::from_utf8(&buf[offset..offset + commission_asset_len])
            .map_err(|_| SbeDecodeError::InvalidUtf8)?;
        offset += commission_asset_len;

        trades.push(BinanceAccountTrade {
            price_exponent,
            qty_exponent,
            commission_exponent,
            id,
            order_id,
            order_list_id,
            price_mantissa,
            qty_mantissa,
            quote_qty_mantissa,
            commission_mantissa,
            time,
            is_buyer,
            is_maker,
            is_best_match,
            symbol: symbol.to_string(),
            commission_asset: commission_asset.to_string(),
        });
    }

    Ok(trades)
}

/// Fills group item block length (from SBE codec).
const FILLS_BLOCK_LENGTH: u16 = 42;

/// Decode order fills from buffer.
///
/// Returns the fills and total bytes consumed from the buffer.
fn decode_order_fills(
    buf: &[u8],
    _parent_commission_exponent: i8,
) -> Result<(Vec<BinanceOrderFill>, usize), SbeDecodeError> {
    if buf.len() < GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let (block_length, count) = decode_group_size(buf)?;

    if count > MAX_GROUP_SIZE {
        return Err(SbeDecodeError::GroupSizeTooLarge {
            count,
            max: MAX_GROUP_SIZE,
        });
    }

    if block_length != FILLS_BLOCK_LENGTH {
        return Err(SbeDecodeError::InvalidBlockLength {
            expected: FILLS_BLOCK_LENGTH,
            actual: block_length,
        });
    }

    let mut fills = Vec::with_capacity(count as usize);
    let mut offset = GROUP_SIZE_LENGTH;

    for _ in 0..count {
        if buf.len() < offset + block_length as usize {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + block_length as usize,
                actual: buf.len(),
            });
        }

        // Fixed fields (from SBE codec offsets)
        let commission_exponent = buf[offset] as i8;
        // offset 1: matchType (skipped)
        let price_mantissa = i64::from_le_bytes(buf[offset + 2..offset + 10].try_into().unwrap());
        let qty_mantissa = i64::from_le_bytes(buf[offset + 10..offset + 18].try_into().unwrap());
        let commission_mantissa =
            i64::from_le_bytes(buf[offset + 18..offset + 26].try_into().unwrap());
        let trade_id_raw = i64::from_le_bytes(buf[offset + 26..offset + 34].try_into().unwrap());
        let trade_id = if trade_id_raw == i64::MIN {
            None
        } else {
            Some(trade_id_raw)
        };
        // offset 34-42: allocId (skipped)
        offset += block_length as usize;

        // Read commission_asset var-length string
        if buf.len() < offset + 1 {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + 1,
                actual: buf.len(),
            });
        }
        let asset_len = buf[offset] as usize;
        offset += 1;

        if buf.len() < offset + asset_len {
            return Err(SbeDecodeError::BufferTooShort {
                expected: offset + asset_len,
                actual: buf.len(),
            });
        }
        let commission_asset = std::str::from_utf8(&buf[offset..offset + asset_len])
            .map_err(|_| SbeDecodeError::InvalidUtf8)?;
        offset += asset_len;

        fills.push(BinanceOrderFill {
            price_mantissa,
            qty_mantissa,
            commission_mantissa,
            commission_exponent,
            commission_asset: commission_asset.to_string(),
            trade_id,
        });
    }

    Ok((fills, offset))
}

/// Decode group size header (block_length, count).
fn decode_group_size(buf: &[u8]) -> Result<(u16, u32), SbeDecodeError> {
    if buf.len() < GROUP_SIZE_LENGTH {
        return Err(SbeDecodeError::BufferTooShort {
            expected: GROUP_SIZE_LENGTH,
            actual: buf.len(),
        });
    }

    let block_length = u16::from_le_bytes([buf[0], buf[1]]);
    let count = u32::from_le_bytes([buf[2], buf[3], buf[4], buf[5]]);

    Ok((block_length, count))
}

/// Decode a variable-length string (1-byte length prefix).
fn decode_var_string(buf: &[u8]) -> Result<(String, usize), SbeDecodeError> {
    if buf.is_empty() {
        return Err(SbeDecodeError::BufferTooShort {
            expected: 1,
            actual: 0,
        });
    }

    let len = buf[0] as usize;
    if len == 0 {
        return Ok((String::new(), 0));
    }

    if buf.len() < 1 + len {
        return Err(SbeDecodeError::BufferTooShort {
            expected: 1 + len,
            actual: buf.len(),
        });
    }

    let s = std::str::from_utf8(&buf[1..=len])
        .map_err(|_| SbeDecodeError::InvalidUtf8)?
        .to_string();

    Ok((s, len))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn create_header(block_length: u16, template_id: u16, schema_id: u16, version: u16) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&block_length.to_le_bytes());
        buf[2..4].copy_from_slice(&template_id.to_le_bytes());
        buf[4..6].copy_from_slice(&schema_id.to_le_bytes());
        buf[6..8].copy_from_slice(&version.to_le_bytes());
        buf
    }

    #[rstest]
    fn test_decode_ping_valid() {
        // Ping: block_length=0, template_id=101, schema_id=3, version=1
        let buf = create_header(0, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        assert!(decode_ping(&buf).is_ok());
    }

    #[rstest]
    fn test_decode_ping_buffer_too_short() {
        let buf = [0u8; 4];
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_ping_schema_mismatch() {
        let buf = create_header(0, PING_TEMPLATE_ID, 99, SBE_SCHEMA_VERSION);
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::SchemaMismatch { .. }));
    }

    #[rstest]
    fn test_decode_ping_wrong_template() {
        let buf = create_header(0, 999, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        let err = decode_ping(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(999)));
    }

    #[rstest]
    fn test_decode_server_time_valid() {
        // ServerTime: block_length=8, template_id=102, schema_id=3, version=1
        let header = create_header(
            8,
            SERVER_TIME_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );
        let timestamp: i64 = 1734300000000; // Example timestamp

        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&timestamp.to_le_bytes());

        let result = decode_server_time(&buf).unwrap();
        assert_eq!(result, timestamp);
    }

    #[rstest]
    fn test_decode_server_time_buffer_too_short() {
        // Header only, missing body
        let buf = create_header(
            8,
            SERVER_TIME_TEMPLATE_ID,
            SBE_SCHEMA_ID,
            SBE_SCHEMA_VERSION,
        );
        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::BufferTooShort { .. }));
    }

    #[rstest]
    fn test_decode_server_time_wrong_template() {
        let header = create_header(8, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes());

        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    #[rstest]
    fn test_decode_server_time_version_mismatch() {
        let header = create_header(8, SERVER_TIME_TEMPLATE_ID, SBE_SCHEMA_ID, 99);
        let mut buf = Vec::with_capacity(16);
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes());

        let err = decode_server_time(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::VersionMismatch { .. }));
    }

    fn create_group_header(block_length: u16, count: u32) -> [u8; 6] {
        let mut buf = [0u8; 6];
        buf[0..2].copy_from_slice(&block_length.to_le_bytes());
        buf[2..6].copy_from_slice(&count.to_le_bytes());
        buf
    }

    #[rstest]
    fn test_decode_depth_valid() {
        // Depth: block_length=10, template_id=200
        let header = create_header(10, DEPTH_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Block: last_update_id (8) + price_exponent (1) + qty_exponent (1)
        let last_update_id: i64 = 123456789;
        let price_exponent: i8 = -8;
        let qty_exponent: i8 = -8;
        buf.extend_from_slice(&last_update_id.to_le_bytes());
        buf.push(price_exponent as u8);
        buf.push(qty_exponent as u8);

        // Bids group: 2 levels
        buf.extend_from_slice(&create_group_header(16, 2));
        // Bid 1: price=100000000000, qty=50000000
        buf.extend_from_slice(&100_000_000_000i64.to_le_bytes());
        buf.extend_from_slice(&50_000_000i64.to_le_bytes());
        // Bid 2: price=99900000000, qty=30000000
        buf.extend_from_slice(&99_900_000_000i64.to_le_bytes());
        buf.extend_from_slice(&30_000_000i64.to_le_bytes());

        // Asks group: 1 level
        buf.extend_from_slice(&create_group_header(16, 1));
        // Ask 1: price=100100000000, qty=25000000
        buf.extend_from_slice(&100_100_000_000i64.to_le_bytes());
        buf.extend_from_slice(&25_000_000i64.to_le_bytes());

        let depth = decode_depth(&buf).unwrap();

        assert_eq!(depth.last_update_id, 123456789);
        assert_eq!(depth.price_exponent, -8);
        assert_eq!(depth.qty_exponent, -8);
        assert_eq!(depth.bids.len(), 2);
        assert_eq!(depth.asks.len(), 1);
        assert_eq!(depth.bids[0].price_mantissa, 100_000_000_000);
        assert_eq!(depth.bids[0].qty_mantissa, 50_000_000);
        assert_eq!(depth.asks[0].price_mantissa, 100_100_000_000);
    }

    #[rstest]
    fn test_decode_depth_empty_book() {
        let header = create_header(10, DEPTH_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&0i64.to_le_bytes()); // last_update_id
        buf.push(0); // price_exponent
        buf.push(0); // qty_exponent

        // Empty bids
        buf.extend_from_slice(&create_group_header(16, 0));
        // Empty asks
        buf.extend_from_slice(&create_group_header(16, 0));

        let depth = decode_depth(&buf).unwrap();

        assert!(depth.bids.is_empty());
        assert!(depth.asks.is_empty());
    }

    #[rstest]
    fn test_decode_trades_valid() {
        // Trades: block_length=2, template_id=201
        let header = create_header(2, TRADES_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);

        // Block: price_exponent (1) + qty_exponent (1)
        let price_exponent: i8 = -8;
        let qty_exponent: i8 = -8;
        buf.push(price_exponent as u8);
        buf.push(qty_exponent as u8);

        // Trades group: 1 trade (42 bytes each)
        buf.extend_from_slice(&create_group_header(42, 1));

        // Trade: id(8) + price(8) + qty(8) + quoteQty(8) + time(8) + isBuyerMaker(1) + isBestMatch(1)
        let trade_id: i64 = 999;
        let price: i64 = 100_000_000_000;
        let qty: i64 = 10_000_000;
        let quote_qty: i64 = 1_000_000_000_000;
        let time: i64 = 1734300000000;
        let is_buyer_maker: u8 = 1; // true
        let is_best_match: u8 = 1; // true

        buf.extend_from_slice(&trade_id.to_le_bytes());
        buf.extend_from_slice(&price.to_le_bytes());
        buf.extend_from_slice(&qty.to_le_bytes());
        buf.extend_from_slice(&quote_qty.to_le_bytes());
        buf.extend_from_slice(&time.to_le_bytes());
        buf.push(is_buyer_maker);
        buf.push(is_best_match);

        let trades = decode_trades(&buf).unwrap();

        assert_eq!(trades.price_exponent, -8);
        assert_eq!(trades.qty_exponent, -8);
        assert_eq!(trades.trades.len(), 1);
        assert_eq!(trades.trades[0].id, 999);
        assert_eq!(trades.trades[0].price_mantissa, 100_000_000_000);
        assert!(trades.trades[0].is_buyer_maker);
        assert!(trades.trades[0].is_best_match);
    }

    #[rstest]
    fn test_decode_trades_empty() {
        let header = create_header(2, TRADES_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.push(0); // price_exponent
        buf.push(0); // qty_exponent

        // Empty trades group
        buf.extend_from_slice(&create_group_header(42, 0));

        let trades = decode_trades(&buf).unwrap();

        assert!(trades.trades.is_empty());
    }

    #[rstest]
    fn test_decode_depth_wrong_template() {
        let header = create_header(10, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&[0u8; 10]); // dummy block

        let err = decode_depth(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }

    #[rstest]
    fn test_decode_trades_wrong_template() {
        let header = create_header(2, PING_TEMPLATE_ID, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION);

        let mut buf = Vec::new();
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&[0u8; 2]); // dummy block

        let err = decode_trades(&buf).unwrap_err();
        assert!(matches!(err, SbeDecodeError::UnknownTemplateId(101)));
    }
}
