use super::*;
#[cfg(feature = "legacy-jsonl")]
use serde_json::Value as JsonValue;
#[cfg(feature = "legacy-jsonl")]
use sixpack_core::DatabaseSchema;
use sixpack_core::{PrimitiveType, Record, TableSchema};

#[cfg(feature = "legacy-jsonl")]
#[test]
fn log_record_round_trip() {
    let mut table = TableSchema::new("messages");
    table.add_field("id", PrimitiveType::Id).unwrap();
    table.add_field("body", PrimitiveType::Text).unwrap();
    let mut schema = DatabaseSchema::new();
    schema.add_table(table).unwrap();

    let record = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello")
        .unwrap();
    let encoded = encode_log_record(&LogRecord::put(1, &record, now_ms())).unwrap();
    let decoded = decode_log_record(&encoded).unwrap();
    assert_eq!(decoded.version, FORMAT_VERSION);
    assert_eq!(decoded.tx_id, 1);
    assert_eq!(decoded.operation, Operation::Put);
    assert_eq!(decoded.table, "messages");
    assert_eq!(
        decoded.data.get("id"),
        Some(&JsonValue::String("m1".to_string()))
    );
    assert!(schema.table("messages").is_some());
}

#[test]
fn six_row_round_trip() {
    let mut table = TableSchema::new("messages");
    table.add_field("id", PrimitiveType::Id).unwrap();
    table.add_field("body", PrimitiveType::Text).unwrap();
    table.add_field("created_at", PrimitiveType::Int).unwrap();

    let record = Record::new("messages")
        .with_id("m1")
        .unwrap()
        .with_field("body", "hello\tworld\nagain")
        .unwrap()
        .with_field("created_at", 42i64)
        .unwrap();

    assert_eq!(encode_six_header(&table), "id\tbody\tcreated_at");
    let encoded = encode_six_row(&table, &record).unwrap();
    assert_eq!(encoded, "m1\thello\\tworld\\nagain\t42");
    let decoded = decode_six_row(&table, &encoded).unwrap();
    assert_eq!(decoded.fields(), record.fields());
}

#[test]
fn sixb_binary_round_trip() {
    let cache = SixbCache {
        version: SIXB_BINARY_VERSION,
        table: "messages".to_owned(),
        schema_hash: "schema".to_owned(),
        source_hash: "source".to_owned(),
        rows: vec![SixbRowEntry {
            id: "m1".to_owned(),
            ptr: RowPointer {
                chunk_name: "zzz.6".to_owned(),
                offset: 42,
                len: 18,
                tx_id: 7,
            },
        }],
        lookups: vec![SixbLookupEntry {
            field_name: "conversation_id".to_owned(),
            key: "cv1".to_owned(),
            id: "m1".to_owned(),
        }],
    };

    let encoded = encode_sixb_cache(&cache);
    assert!(encoded.starts_with(b"SIXB\0"));
    assert_eq!(
        u32::from_le_bytes(encoded[5..9].try_into().unwrap()),
        SIXB_BINARY_VERSION
    );
    let decoded = decode_sixb_cache(&encoded).unwrap();
    assert_eq!(decoded, cache);
}

#[test]
fn legacy_text_sixb_still_decodes() {
    let legacy = b"SIXB\t1\tmessages\tschema\tsource\nrow\tm1\tzzz.6\t42\t18\t7\nlookup\tconversation_id\tcv1\tm1\n";
    let decoded = decode_sixb_cache(legacy).unwrap();
    assert_eq!(decoded.version, 1);
    assert_eq!(decoded.table, "messages");
    assert_eq!(decoded.rows.len(), 1);
    assert_eq!(decoded.lookups.len(), 1);
}
