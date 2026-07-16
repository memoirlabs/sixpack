use std::env;
use std::fs;
use std::path::PathBuf;

use sixpack_schema_compiler::{compile_schema, emit_raw_rust};

fn main() {
    println!("cargo:rerun-if-changed=schema.sixpack");

    let source = fs::read_to_string("schema.sixpack").expect("read schema.sixpack");
    let ir = compile_schema(&source).expect("compile schema.sixpack");
    let generated = emit_raw_rust(&ir);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    fs::write(out_dir.join("ai_chat_notes_schema.rs"), generated)
        .expect("write generated chat and notes SDK");
}
