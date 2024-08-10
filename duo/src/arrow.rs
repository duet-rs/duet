use datafusion::arrow::json::{
    reader::infer_json_schema_from_iterator, ArrayWriter, ReaderBuilder,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value as JsonValue};
use std::sync::Arc;

use crate::{Log, Span};
use anyhow::Result;
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::array::{Int64Array, RecordBatch, StringArray, UInt64Array};

pub fn schema_span() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("parent_id", DataType::UInt64, true),
        Field::new("trace_id", DataType::UInt64, false),
        Field::new("name", DataType::Utf8, false),
        Field::new("process_id", DataType::Utf8, false),
        Field::new("start", DataType::Int64, false),
        Field::new("end", DataType::Int64, true),
        Field::new("tags", DataType::Utf8, true),
    ]))
}

pub fn schema_log() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("process_id", DataType::Utf8, false),
        Field::new("time", DataType::Int64, false),
        Field::new("trace_id", DataType::UInt64, true),
        Field::new("span_id", DataType::UInt64, true),
        Field::new("level", DataType::Utf8, false),
        Field::new("message", DataType::Utf8, true),
    ]))
}

pub fn convert_span_to_record_batch(spans: Vec<Span>) -> Result<RecordBatch> {
    let mut span_ids = Vec::<u64>::new();
    let mut parent_ids = Vec::<Option<u64>>::new();
    let mut trace_ids = Vec::<u64>::new();
    let mut names = Vec::<String>::new();
    let mut process_ids = Vec::<String>::new();
    let mut start_times = Vec::<i64>::new();
    let mut end_times = Vec::<Option<i64>>::new();
    let mut tags_list = Vec::<String>::new();

    for span in spans {
        let start_time = span.start_as_micros();
        let end_time = span.end_as_micros();
        span_ids.push(span.id);
        parent_ids.push(span.parent_id);
        trace_ids.push(span.trace_id);
        names.push(span.name);
        process_ids.push(span.process_id);
        start_times.push(start_time);
        end_times.push(end_time);
        tags_list.push(serde_json::to_string(&span.tags).unwrap());
    }

    if span_ids.is_empty() {
        return Ok(RecordBatch::new_empty(schema_span()));
    }

    Ok(RecordBatch::try_new(
        schema_span(),
        vec![
            Arc::new(UInt64Array::from(span_ids)),
            Arc::new(UInt64Array::from(parent_ids)),
            Arc::new(UInt64Array::from(trace_ids)),
            Arc::new(StringArray::from(names)),
            Arc::new(StringArray::from(process_ids)),
            Arc::new(Int64Array::from(start_times)),
            Arc::new(Int64Array::from(end_times)),
            Arc::new(StringArray::from(tags_list)),
        ],
    )?)
}

pub fn convert_log_to_record_batch(logs: Vec<Log>) -> Result<RecordBatch> {
    let mut data = vec![];
    let mut fields = vec![];
    for log in logs {
        let mut map = Map::new();
        let time = log.as_micros();
        map.insert("process_id".into(), log.process_id.into());
        map.insert("span_id".into(), log.span_id.into());
        map.insert("trace_id".into(), log.trace_id.into());
        map.insert("level".into(), log.level.as_str().into());
        map.insert("time".into(), time.into());
        map.insert("message".into(), log.message.into());
        let mut field_map = Map::new();
        for (key, value) in log.fields {
            field_map.insert(key.clone(), value.clone());
        }

        if !field_map.is_empty() {
            fields.push(JsonValue::Object(field_map.clone()));
            map.extend(field_map);
        }
        data.push(JsonValue::Object(map));
    }

    let inferred_field_schema = infer_json_schema_from_iterator(fields.iter().map(Ok))?;
    let schema = Schema::try_merge(vec![(*schema_log()).clone(), inferred_field_schema]).unwrap();
    let mut decoder = ReaderBuilder::new(Arc::new(schema)).build_decoder()?;
    decoder.serialize(&data)?;
    let batch = decoder.flush()?.expect("Empty record batch");
    Ok(batch)
}

pub fn serialize_record_batches<T: DeserializeOwned>(batch: &[RecordBatch]) -> Result<Vec<T>> {
    if batch.is_empty() {
        return Ok(vec![]);
    }

    let buf = Vec::new();
    let mut writer = ArrayWriter::new(buf);
    writer.write_batches(&batch.iter().collect::<Vec<_>>())?;
    writer.finish()?;
    let json_values = writer.into_inner();
    let json_rows: Vec<_> = serde_json::from_reader(json_values.as_slice()).unwrap_or_default();
    Ok(json_rows)
}
