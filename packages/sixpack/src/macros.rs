#[macro_export]
macro_rules! __sixpack_rust_type {
    (id) => {
        ::std::string::String
    };
    (text) => {
        ::std::string::String
    };
    (int) => {
        i64
    };
    (float) => {
        f64
    };
    (bool) => {
        bool
    };
}

#[macro_export]
macro_rules! __sixpack_primitive_type {
    (id) => {
        $crate::PrimitiveType::Id
    };
    (text) => {
        $crate::PrimitiveType::Text
    };
    (int) => {
        $crate::PrimitiveType::Int
    };
    (float) => {
        $crate::PrimitiveType::Float
    };
    (bool) => {
        $crate::PrimitiveType::Bool
    };
}

#[macro_export]
macro_rules! __sixpack_schema_items {
    ($table:ident;) => {};
    ($table:ident; lookup $field:ident unique $($rest:tt)*) => {
        $table.add_lookup(stringify!($field), true).unwrap();
        $crate::__sixpack_schema_items!($table; $($rest)*);
    };
    ($table:ident; lookup $field:ident $($rest:tt)*) => {
        $table.add_lookup(stringify!($field), false).unwrap();
        $crate::__sixpack_schema_items!($table; $($rest)*);
    };
    ($table:ident; $field:ident $field_ty:ident $($rest:tt)*) => {
        $table
            .add_field(
                stringify!($field),
                $crate::__sixpack_primitive_type!($field_ty),
            )
            .unwrap();
        $crate::__sixpack_schema_items!($table; $($rest)*);
    };
}

/// Declares a compact schema surface for a `schema.sixpack` include file.
///
/// The intended authoring shape is:
///
/// ```rust
/// # use sixpack::schema;
/// schema! {
///     users {
///         id id
///         email text
///
///         lookup email unique
///     }
/// }
/// ```
///
/// This emits one module per table with a `table_schema()` function and a
/// top-level `database_schema()` function that combines all declared tables.
#[macro_export]
macro_rules! schema {
    ($($table:ident { $($body:tt)* })*) => {
        $(
            pub mod $table {
                pub const NAME: &str = stringify!($table);

                pub fn table_schema() -> $crate::TableSchema {
                    let mut table = $crate::TableSchema::new(NAME);
                    $crate::__sixpack_schema_items!(table; $($body)*);
                    table
                }
            }
        )*

        pub fn database_schema() -> $crate::DatabaseSchema {
            let mut db = $crate::DatabaseSchema::new();
            $(
                db.add_table($table::table_schema()).unwrap();
            )*
            db
        }
    };
}

/// Very small `table!` helper for a local schema-first path.
///
/// It emits:
/// - a module for the table name
/// - a typed `Row` struct using primitive Rust types
/// - a `table_schema()` function that builds a `TableSchema`
/// - a tiny `table_database()` function that wraps the schema as a 1-table DB
///
/// New schema files should prefer `schema!`; this macro remains useful for
/// narrow typed-row experiments.
///
/// This intentionally keeps syntax minimal and does not attempt full parse-time
/// validation beyond macro syntax and known primitive type names.
#[macro_export]
macro_rules! table {
    ($table:ident { $($field:ident : $field_ty:ident $(;)?)* }) => {
        pub mod $table {
            #[derive(Debug, Clone, PartialEq)]
            pub struct Row {
                $(
                    pub $field: $crate::__sixpack_rust_type!($field_ty),
                )*
            }

            pub fn table_schema() -> $crate::TableSchema {
                let mut table = $crate::TableSchema::new(stringify!($table));
                $(
                    table
                        .add_field(
                            stringify!($field),
                            $crate::__sixpack_primitive_type!($field_ty),
                        )
                        .unwrap();
                )*

                table
            }

            pub fn table_database() -> $crate::DatabaseSchema {
                let mut db = $crate::DatabaseSchema::new();
                db.add_table(table_schema()).unwrap();
                db
            }
        }
    };
}
