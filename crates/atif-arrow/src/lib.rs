//! A fixed Arrow representation for ATIF trajectory documents.
//!
//! This initial release defines the schema contract. Parsing and conversion
//! will follow separately; command-line and IPC concerns live in the CLI crate.

//! ```
//! let schema = atif_arrow::trajectory_schema();
//! assert!(!schema.field_with_name("steps")?.is_nullable());
//! # Ok::<(), arrow_schema::ArrowError>(())
//! ```

mod schema;

pub use schema::{trajectory_schema, SCHEMA_VERSION};
