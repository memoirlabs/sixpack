use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use sixpack::{
    Database, DatabaseError, DatabaseSchema, Operation, PlanEnvelope, PlanError, PlanOp,
    PlanOutcome, PrimitiveType, Record, Value, WriteChange,
};
use sixpack_schema_compiler::{compile_schema, database_schema_from_ir};

#[derive(Debug)]
pub(crate) struct BridgeError(String);

impl fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BridgeError {}

pub(crate) fn run(mut args: impl Iterator<Item = String>) -> Result<(), BridgeError> {
    let options = BridgeOptions::parse(&mut args)?;
    let mut request = String::new();
    io::stdin()
        .read_to_string(&mut request)
        .map_err(|error| BridgeError(format!("could not read bridge request: {error}")))?;
    println!("{}", execute(&options, &request)?);
    Ok(())
}

#[derive(Debug)]
struct BridgeOptions {
    root: PathBuf,
    workspace: String,
    schema: PathBuf,
}

impl BridgeOptions {
    fn parse(args: &mut impl Iterator<Item = String>) -> Result<Self, BridgeError> {
        let mut root = None;
        let mut workspace = None;
        let mut schema = None;

        while let Some(argument) = args.next() {
            let value = args.next().ok_or_else(|| {
                BridgeError(format!("bridge option `{argument}` is missing its value"))
            })?;
            match argument.as_str() {
                "--root" => root = Some(PathBuf::from(value)),
                "--workspace" => workspace = Some(value),
                "--schema" => schema = Some(PathBuf::from(value)),
                _ => return Err(BridgeError(format!("unknown bridge option `{argument}`"))),
            }
        }

        Ok(Self {
            root: root.ok_or_else(|| BridgeError("bridge requires --root".to_owned()))?,
            workspace: workspace
                .ok_or_else(|| BridgeError("bridge requires --workspace".to_owned()))?,
            schema: schema.ok_or_else(|| BridgeError("bridge requires --schema".to_owned()))?,
        })
    }
}

#[derive(Debug, Deserialize)]
struct BridgeRequest {
    schema_hash: String,
    #[serde(flatten)]
    operation: BridgeOperation,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum BridgeOperation {
    Init,
    Get {
        table: String,
        lookup: String,
        key: JsonValue,
    },
    Find {
        table: String,
        lookup: String,
        key: JsonValue,
        limit: Option<usize>,
        cursor: Option<String>,
    },
    Scan {
        table: String,
        limit: Option<usize>,
        cursor: Option<String>,
    },
    Count {
        table: String,
    },
    Add {
        table: String,
        row: JsonMap<String, JsonValue>,
    },
    Set {
        table: String,
        row: JsonMap<String, JsonValue>,
    },
    Edit {
        table: String,
        lookup: String,
        key: JsonValue,
        patch: JsonMap<String, JsonValue>,
    },
    Remove {
        table: String,
        lookup: String,
        key: JsonValue,
    },
    WriteMany {
        changes: Vec<WireChange>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireChange {
    Add {
        table: String,
        row: JsonMap<String, JsonValue>,
    },
    Set {
        table: String,
        row: JsonMap<String, JsonValue>,
    },
    Edit {
        table: String,
        lookup: String,
        key: JsonValue,
        patch: JsonMap<String, JsonValue>,
    },
    Remove {
        table: String,
        lookup: String,
        key: JsonValue,
    },
}

fn execute(options: &BridgeOptions, request: &str) -> Result<String, BridgeError> {
    let source = fs::read_to_string(&options.schema).map_err(|error| {
        BridgeError(format!(
            "could not read schema `{}`: {error}",
            options.schema.display()
        ))
    })?;
    let ir = compile_schema(&source)
        .map_err(|error| BridgeError(format!("schema compilation failed: {error}")))?;
    let request: BridgeRequest = serde_json::from_str(request)
        .map_err(|error| BridgeError(format!("invalid bridge request: {error}")))?;
    if request.schema_hash != ir.schema_hash() {
        return response_json(Err(WireError::new(
            "schema_mismatch",
            format!(
                "generated TypeScript schema hash `{}` does not match runtime schema hash `{}`",
                request.schema_hash,
                ir.schema_hash()
            ),
        )));
    }

    let schema = database_schema_from_ir(&ir)
        .map_err(|error| BridgeError(format!("could not build runtime schema: {error}")))?;
    let db = Database::open_local_with_schema(&options.root, &options.workspace, schema.clone());
    response_json(execute_operation(&db, &schema, request.operation))
}

fn execute_operation(
    db: &Database,
    schema: &DatabaseSchema,
    operation: BridgeOperation,
) -> Result<JsonValue, WireError> {
    match operation {
        BridgeOperation::Init => {
            db.init().map_err(WireError::from_database)?;
            Ok(JsonValue::Null)
        }
        BridgeOperation::Get { table, lookup, key } => {
            let key = decode_lookup(schema, &table, &lookup, key)?;
            let plan = PlanEnvelope::new(PlanOp::Get, &table)
                .with_lookup(&lookup)
                .with_key(&lookup, key);
            match db.execute_plan(plan).map_err(WireError::from_database)? {
                PlanOutcome::Row(Some(row)) => Ok(JsonValue::Object(record_to_json(&row)?)),
                PlanOutcome::Row(None) => Ok(JsonValue::Null),
                _ => unreachable!("get returns a row outcome"),
            }
        }
        BridgeOperation::Find {
            table,
            lookup,
            key,
            limit,
            cursor,
        } => {
            let key = decode_lookup(schema, &table, &lookup, key)?;
            let mut plan = PlanEnvelope::new(PlanOp::Find, &table)
                .with_lookup(&lookup)
                .with_key(&lookup, key);
            plan.limit = limit;
            plan.cursor = cursor;
            page_outcome(db.execute_plan(plan).map_err(WireError::from_database)?)
        }
        BridgeOperation::Scan {
            table,
            limit,
            cursor,
        } => {
            let mut plan = PlanEnvelope::new(PlanOp::Scan, table);
            plan.limit = limit;
            plan.cursor = cursor;
            page_outcome(db.execute_plan(plan).map_err(WireError::from_database)?)
        }
        BridgeOperation::Count { table } => {
            match db
                .execute_plan(PlanEnvelope::new(PlanOp::Count, table))
                .map_err(WireError::from_database)?
            {
                PlanOutcome::Count(count) => Ok(JsonValue::String(count.to_string())),
                _ => unreachable!("count returns a count outcome"),
            }
        }
        BridgeOperation::Add { table, row } => {
            let record = decode_record(schema, &table, row)?;
            let outcome = db
                .execute_plan(PlanEnvelope::new(PlanOp::Insert, table).with_record_value(record))
                .map_err(WireError::from_database)?;
            append_outcome(outcome)
        }
        BridgeOperation::Set { table, row } => {
            let record = decode_record(schema, &table, row)?;
            let outcome = db
                .execute_plan(PlanEnvelope::new(PlanOp::Upsert, table).with_record_value(record))
                .map_err(WireError::from_database)?;
            append_outcome(outcome)
        }
        BridgeOperation::Edit {
            table,
            lookup,
            key,
            patch,
        } => {
            let key = decode_lookup(schema, &table, &lookup, key)?;
            let values = decode_fields(schema, &table, patch)?;
            let plan = PlanEnvelope::new(PlanOp::Patch, table)
                .with_lookup(&lookup)
                .with_key(&lookup, key)
                .with_values(values);
            append_outcome(db.execute_plan(plan).map_err(WireError::from_database)?)
        }
        BridgeOperation::Remove { table, lookup, key } => {
            let key = decode_lookup(schema, &table, &lookup, key)?;
            let plan = PlanEnvelope::new(PlanOp::Remove, table)
                .with_lookup(&lookup)
                .with_key(&lookup, key);
            append_outcome(db.execute_plan(plan).map_err(WireError::from_database)?)
        }
        BridgeOperation::WriteMany { changes } => {
            let changes = changes
                .into_iter()
                .map(|change| decode_change(schema, change))
                .collect::<Result<Vec<_>, _>>()?;
            let results = db.write_many(&changes).map_err(WireError::from_database)?;
            Ok(JsonValue::Array(
                results.into_iter().map(append_to_json).collect(),
            ))
        }
    }
}

fn decode_change(schema: &DatabaseSchema, change: WireChange) -> Result<WriteChange, WireError> {
    match change {
        WireChange::Add { table, row } => {
            Ok(WriteChange::add_record(decode_record(schema, &table, row)?))
        }
        WireChange::Set { table, row } => {
            Ok(WriteChange::set_record(decode_record(schema, &table, row)?))
        }
        WireChange::Edit {
            table,
            lookup,
            key,
            patch,
        } => Ok(WriteChange::edit(
            &table,
            &lookup,
            decode_lookup(schema, &table, &lookup, key)?,
            decode_fields(schema, &table, patch)?,
        )),
        WireChange::Remove { table, lookup, key } => Ok(WriteChange::remove(
            &table,
            &lookup,
            decode_lookup(schema, &table, &lookup, key)?,
        )),
    }
}

fn decode_record(
    schema: &DatabaseSchema,
    table: &str,
    fields: JsonMap<String, JsonValue>,
) -> Result<Record, WireError> {
    let values = decode_fields(schema, table, fields)?;
    let mut record = Record::new(table);
    for (name, value) in values {
        record
            .insert_field(name, value)
            .map_err(|error| WireError::new("invalid_value", error.to_string()))?;
    }
    Ok(record)
}

fn decode_fields(
    schema: &DatabaseSchema,
    table_name: &str,
    fields: JsonMap<String, JsonValue>,
) -> Result<BTreeMap<String, Value>, WireError> {
    let table = schema
        .table(table_name)
        .ok_or_else(|| WireError::new("unknown_table", format!("unknown table `{table_name}`")))?;
    fields
        .into_iter()
        .map(|(name, value)| {
            let field = table.field(&name).ok_or_else(|| {
                WireError::new(
                    "unknown_field",
                    format!("unknown field `{name}` for table `{table_name}`"),
                )
            })?;
            Ok((name, decode_value(field.kind(), value)?))
        })
        .collect()
}

fn decode_lookup(
    schema: &DatabaseSchema,
    table_name: &str,
    lookup: &str,
    value: JsonValue,
) -> Result<Value, WireError> {
    let table = schema
        .table(table_name)
        .ok_or_else(|| WireError::new("unknown_table", format!("unknown table `{table_name}`")))?;
    let field = table.field(lookup).ok_or_else(|| {
        WireError::new(
            "unknown_lookup",
            format!("unknown lookup `{lookup}` for table `{table_name}`"),
        )
    })?;
    decode_value(field.kind(), value)
}

fn decode_value(kind: PrimitiveType, value: JsonValue) -> Result<Value, WireError> {
    let invalid = || {
        WireError::new(
            "type_mismatch",
            format!("expected a valid {kind} wire value"),
        )
    };
    match kind {
        PrimitiveType::Id => value.as_str().map(|value| Value::Id(value.to_owned())),
        PrimitiveType::Text => value.as_str().map(|value| Value::Text(value.to_owned())),
        PrimitiveType::Int => match value {
            JsonValue::String(value) => value.parse::<i64>().ok().map(Value::Int),
            JsonValue::Number(value) => value.as_i64().map(Value::Int),
            _ => None,
        },
        PrimitiveType::Float => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Value::Float),
        PrimitiveType::Bool => value.as_bool().map(Value::Bool),
    }
    .ok_or_else(invalid)
}

fn record_to_json(record: &Record) -> Result<JsonMap<String, JsonValue>, WireError> {
    record
        .fields()
        .iter()
        .map(|(name, value)| Ok((name.clone(), value_to_json(value)?)))
        .collect()
}

fn value_to_json(value: &Value) -> Result<JsonValue, WireError> {
    match value {
        Value::Id(value) | Value::Text(value) => Ok(JsonValue::String(value.clone())),
        Value::Int(value) => Ok(JsonValue::String(value.to_string())),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .ok_or_else(|| WireError::new("invalid_value", "cannot return a non-finite float")),
        Value::Bool(value) => Ok(JsonValue::Bool(*value)),
    }
}

fn page_outcome(outcome: PlanOutcome) -> Result<JsonValue, WireError> {
    match outcome {
        PlanOutcome::Rows(page) => Ok(json!({
            "rows": page
                .rows
                .iter()
                .map(record_to_json)
                .collect::<Result<Vec<_>, _>>()?,
            "next_cursor": page.next_cursor,
        })),
        _ => unreachable!("find and scan return page outcomes"),
    }
}

fn append_outcome(outcome: PlanOutcome) -> Result<JsonValue, WireError> {
    match outcome {
        PlanOutcome::Append(result) => Ok(append_to_json(result)),
        _ => unreachable!("write operations return append outcomes"),
    }
}

fn append_to_json(result: sixpack::AppendResult) -> JsonValue {
    json!({
        "tx_id": result.tx_id.to_string(),
        "operation": match result.operation {
            Operation::Put => "put",
            Operation::Delete => "delete",
        },
        "bytes_written": result.bytes_written.to_string(),
    })
}

#[derive(Debug)]
struct WireError {
    code: &'static str,
    message: String,
}

impl WireError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn from_database(error: DatabaseError) -> Self {
        let code = match &error {
            DatabaseError::Schema(_) => "schema_error",
            DatabaseError::Plan(PlanError::Invalid(_)) => "invalid_plan",
            DatabaseError::Plan(PlanError::NotFound(_)) => "not_found",
            DatabaseError::Io(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                "already_exists"
            }
            DatabaseError::Io(_) => "io_error",
        };
        Self::new(code, error.to_string())
    }
}

fn response_json(result: Result<JsonValue, WireError>) -> Result<String, BridgeError> {
    let response = match result {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(error) => json!({
            "ok": false,
            "error": { "code": error.code, "message": error.message },
        }),
    };
    serde_json::to_string(&response)
        .map_err(|error| BridgeError(format!("could not encode bridge response: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SCHEMA: &str = "schema! { users { id id \n email text \n age int \n score float \n active bool \n lookup email unique \n lookup score \n lookup active \n } }";

    #[test]
    fn bridge_executes_typed_write_and_read() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let schema_path = root.join("schema.sixpack");
        fs::write(&schema_path, SCHEMA).unwrap();
        let options = BridgeOptions {
            root: root.clone(),
            workspace: "typescript".to_owned(),
            schema: schema_path,
        };
        let hash = compile_schema(SCHEMA).unwrap().schema_hash();

        assert_ok(execute(&options, &request(&hash, json!({ "op": "init" }))).unwrap());
        assert_ok(
            execute(
                &options,
                &request(
                    &hash,
                    json!({
                        "op": "add",
                        "table": "users",
                        "row": {
                            "id": "u1",
                            "email": "a@test.com",
                            "age": "9007199254740993",
                            "score": 1.5,
                            "active": true
                        },
                    }),
                ),
            )
            .unwrap(),
        );
        let response = execute(
            &options,
            &request(
                &hash,
                json!({ "op": "get", "table": "users", "lookup": "email", "key": "a@test.com" }),
            ),
        )
        .unwrap();
        let response: JsonValue = serde_json::from_str(&response).unwrap();
        assert_eq!(response["result"]["age"], "9007199254740993");
        assert_eq!(response["result"]["score"], 1.5);
        assert_eq!(response["result"]["active"], true);

        for (lookup, key) in [("score", json!(1.5)), ("active", json!(true))] {
            let response = execute(
                &options,
                &request(
                    &hash,
                    json!({
                        "op": "find",
                        "table": "users",
                        "lookup": lookup,
                        "key": key,
                        "limit": 10
                    }),
                ),
            )
            .unwrap();
            let response: JsonValue = serde_json::from_str(&response).unwrap();
            assert_eq!(response["result"]["rows"].as_array().unwrap().len(), 1);
        }

        let response = execute(&options, &request("wrong", json!({ "op": "init" }))).unwrap();
        let response: JsonValue = serde_json::from_str(&response).unwrap();
        assert_eq!(response["error"]["code"], "schema_mismatch");

        let _ = fs::remove_dir_all(root);
    }

    fn request(hash: &str, operation: JsonValue) -> String {
        let mut request = operation.as_object().unwrap().clone();
        request.insert("schema_hash".to_owned(), JsonValue::String(hash.to_owned()));
        JsonValue::Object(request).to_string()
    }

    fn assert_ok(response: String) {
        let response: JsonValue = serde_json::from_str(&response).unwrap();
        assert_eq!(response["ok"], true);
    }

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("sixpack-typescript-{}-{nonce}", std::process::id()))
    }
}
