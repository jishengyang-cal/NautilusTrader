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

use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth10},
    enums::OrderSide,
};

use super::{
    super::{SbeCursor, SbeDecodeError, SbeEncodeError},
    MarketSbeMessage,
    common::{
        BOOK_ORDER_BLOCK_LENGTH, DEPTH10_COUNTS_BLOCK_LENGTH, DEPTH10_LEVEL_BLOCK_LENGTH,
        DEPTH10_LEVEL_COUNT, GROUP_HEADER_16_LENGTH, ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH,
        decode_book_action, decode_instrument_id, decode_order_side, decode_price, decode_quantity,
        decode_unix_nanos, encode_group_header_16, encode_instrument_id, encode_price,
        encode_quantity, encode_unix_nanos, encoded_instrument_id_size,
    },
    template_id,
};

impl MarketSbeMessage for BookOrder {
    const TEMPLATE_ID: u16 = template_id::BOOK_ORDER;
    const BLOCK_LENGTH: u16 = BOOK_ORDER_BLOCK_LENGTH;

    fn encode_body(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError> {
        encode_book_order(buf, self);
        Ok(())
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        decode_book_order(cursor)
    }
}

impl MarketSbeMessage for OrderBookDelta {
    const TEMPLATE_ID: u16 = template_id::ORDER_BOOK_DELTA;
    const BLOCK_LENGTH: u16 = ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH;

    fn encode_body(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError> {
        encode_order_book_delta_fields(buf, self);
        encode_instrument_id(buf, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let action = decode_book_action(cursor)?;
        let order = decode_book_order(cursor)?;
        let flags = cursor.read_u8()?;
        let sequence = cursor.read_u64_le()?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

impl MarketSbeMessage for OrderBookDeltas {
    const TEMPLATE_ID: u16 = template_id::ORDER_BOOK_DELTAS;
    const BLOCK_LENGTH: u16 = 25;

    fn encode_body(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError> {
        buf.push(self.flags);
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        encode_unix_nanos(buf, self.ts_event);
        encode_unix_nanos(buf, self.ts_init);
        encode_instrument_id(buf, &self.instrument_id)?;
        encode_group_header_16(
            buf,
            "OrderBookDeltas.deltas",
            self.deltas.len(),
            ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH,
        )?;

        for delta in &self.deltas {
            encode_order_book_delta_fields(buf, delta);
            encode_instrument_id(buf, &delta.instrument_id)?;
        }
        Ok(())
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let flags = cursor.read_u8()?;
        let sequence = cursor.read_u64_le()?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;
        let (block_length, count) = cursor.read_group_header_16()?;

        if block_length != ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH {
            return Err(SbeDecodeError::InvalidBlockLength {
                expected: ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH,
                actual: block_length,
            });
        }

        let mut deltas = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let action = decode_book_action(cursor)?;
            let order = decode_book_order(cursor)?;
            let delta_flags = cursor.read_u8()?;
            let delta_sequence = cursor.read_u64_le()?;
            let delta_ts_event = decode_unix_nanos(cursor)?;
            let delta_ts_init = decode_unix_nanos(cursor)?;
            let delta_instrument_id = decode_instrument_id(cursor)?;

            deltas.push(OrderBookDelta {
                instrument_id: delta_instrument_id,
                action,
                order,
                flags: delta_flags,
                sequence: delta_sequence,
                ts_event: delta_ts_event,
                ts_init: delta_ts_init,
            });
        }

        Ok(Self {
            instrument_id,
            deltas,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH)
            + encoded_instrument_id_size(&self.instrument_id)
            + GROUP_HEADER_16_LENGTH
            + self
                .deltas
                .iter()
                .map(encoded_order_book_delta_size)
                .sum::<usize>()
    }
}

impl MarketSbeMessage for OrderBookDepth10 {
    const TEMPLATE_ID: u16 = template_id::ORDER_BOOK_DEPTH10;
    const BLOCK_LENGTH: u16 =
        (DEPTH10_LEVEL_BLOCK_LENGTH * 20) + (DEPTH10_COUNTS_BLOCK_LENGTH as u16 * 2) + 25;

    fn encode_body(&self, buf: &mut Vec<u8>) -> Result<(), SbeEncodeError> {
        for bid in &self.bids {
            encode_price(buf, &bid.price);
            encode_quantity(buf, &bid.size);
        }

        for ask in &self.asks {
            encode_price(buf, &ask.price);
            encode_quantity(buf, &ask.size);
        }

        for count in &self.bid_counts {
            buf.extend_from_slice(&count.to_le_bytes());
        }

        for count in &self.ask_counts {
            buf.extend_from_slice(&count.to_le_bytes());
        }
        buf.push(self.flags);
        buf.extend_from_slice(&self.sequence.to_le_bytes());
        encode_unix_nanos(buf, self.ts_event);
        encode_unix_nanos(buf, self.ts_init);
        encode_instrument_id(buf, &self.instrument_id)
    }

    fn decode_body(cursor: &mut SbeCursor<'_>) -> Result<Self, SbeDecodeError> {
        let mut bids = [BookOrder::default(); DEPTH10_LEVEL_COUNT];
        let mut asks = [BookOrder::default(); DEPTH10_LEVEL_COUNT];

        for bid in &mut bids {
            *bid = BookOrder::new(
                OrderSide::Buy,
                decode_price(cursor)?,
                decode_quantity(cursor)?,
                0,
            );
        }

        for ask in &mut asks {
            *ask = BookOrder::new(
                OrderSide::Sell,
                decode_price(cursor)?,
                decode_quantity(cursor)?,
                0,
            );
        }

        let mut bid_counts = [0u32; DEPTH10_LEVEL_COUNT];
        let mut ask_counts = [0u32; DEPTH10_LEVEL_COUNT];

        for count in &mut bid_counts {
            *count = cursor.read_u32_le()?;
        }

        for count in &mut ask_counts {
            *count = cursor.read_u32_le()?;
        }

        let flags = cursor.read_u8()?;
        let sequence = cursor.read_u64_le()?;
        let ts_event = decode_unix_nanos(cursor)?;
        let ts_init = decode_unix_nanos(cursor)?;
        let instrument_id = decode_instrument_id(cursor)?;

        Ok(Self {
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }

    fn encoded_body_size(&self) -> usize {
        usize::from(Self::BLOCK_LENGTH) + encoded_instrument_id_size(&self.instrument_id)
    }
}

fn encode_book_order(buf: &mut Vec<u8>, order: &BookOrder) {
    encode_price(buf, &order.price);
    encode_quantity(buf, &order.size);
    buf.push(order.side as u8);
    buf.extend_from_slice(&order.order_id.to_le_bytes());
}

fn decode_book_order(cursor: &mut SbeCursor<'_>) -> Result<BookOrder, SbeDecodeError> {
    let price = decode_price(cursor)?;
    let size = decode_quantity(cursor)?;
    let side = decode_order_side(cursor)?;
    let order_id = cursor.read_u64_le()?;
    Ok(BookOrder {
        side,
        price,
        size,
        order_id,
    })
}

fn encode_order_book_delta_fields(buf: &mut Vec<u8>, delta: &OrderBookDelta) {
    buf.push(delta.action as u8);
    encode_book_order(buf, &delta.order);
    buf.push(delta.flags);
    buf.extend_from_slice(&delta.sequence.to_le_bytes());
    encode_unix_nanos(buf, delta.ts_event);
    encode_unix_nanos(buf, delta.ts_init);
}

fn encoded_order_book_delta_size(delta: &OrderBookDelta) -> usize {
    usize::from(ORDER_BOOK_DELTA_GROUP_BLOCK_LENGTH)
        + encoded_instrument_id_size(&delta.instrument_id)
}
