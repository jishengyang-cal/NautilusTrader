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

use std::collections::HashMap;

use arrow::{datatypes::Schema, error::ArrowError, record_batch::RecordBatch};
use nautilus_model::data::InstrumentStatus;

use super::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_INSTRUMENT_ID,
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

const INSTRUMENT_STATUS_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("instrument_id", false),
    JsonFieldSpec::utf8("action", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
    JsonFieldSpec::utf8("reason", true),
    JsonFieldSpec::utf8("trading_event", true),
    JsonFieldSpec::boolean("is_trading", true),
    JsonFieldSpec::boolean("is_quoting", true),
    JsonFieldSpec::boolean("is_short_sell_restricted", true),
];

impl ArrowSchemaProvider for InstrumentStatus {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        schema_for_type("InstrumentStatus", metadata, INSTRUMENT_STATUS_FIELDS)
    }
}

impl EncodeToRecordBatch for InstrumentStatus {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        encode_batch("InstrumentStatus", metadata, data, INSTRUMENT_STATUS_FIELDS)
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = metadata_for_type("InstrumentStatus");
        metadata.insert(
            KEY_INSTRUMENT_ID.to_string(),
            self.instrument_id.to_string(),
        );
        metadata
    }
}

impl DecodeTypedFromRecordBatch for InstrumentStatus {
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        decode_batch(
            metadata,
            &record_batch,
            INSTRUMENT_STATUS_FIELDS,
            Some("InstrumentStatus"),
        )
    }
}
