//! SPEC-090 Wave 2 contract + unit gates.
//!
//! Run:
//!   cargo test -p edgequake-storage --features postgres --test e2e_spec090_wave2

#![cfg(feature = "postgres")]

use edgequake_storage::VectorStorageMode;
use std::fs;

#[test]
fn contract_spec090_halfvec_default() {
    let prev = std::env::var("EDGEQUAKE_VECTOR_STORAGE").ok();
    std::env::remove_var("EDGEQUAKE_VECTOR_STORAGE");
    assert_eq!(VectorStorageMode::from_env(), VectorStorageMode::Half);
    if let Some(v) = prev {
        std::env::set_var("EDGEQUAKE_VECTOR_STORAGE", v);
    }
}

#[test]
fn contract_spec090_clear_workspace_no_bare_or() {
    let src =
        fs::read_to_string("crates/edgequake-storage/src/adapters/postgres/vector/storage_impl.rs")
            .or_else(|_| fs::read_to_string("src/adapters/postgres/vector/storage_impl.rs"))
            .expect("storage_impl.rs");
    let start = src
        .find("async fn clear_workspace")
        .expect("clear_workspace");
    let body = &src[start..start + 1200.min(src.len() - start)];
    assert!(
        body.contains("WHERE ctid IN") && body.contains("UNION"),
        "clear_workspace must use UNION ctid delete arms (F-090-09)"
    );
    assert!(
        !body.contains("workspace_id = $1 OR metadata"),
        "clear_workspace must not use bare OR predicate"
    );
}

#[test]
fn contract_spec090_delete_by_document_no_bare_or() {
    let src =
        fs::read_to_string("crates/edgequake-storage/src/adapters/postgres/vector/storage_impl.rs")
            .or_else(|_| fs::read_to_string("src/adapters/postgres/vector/storage_impl.rs"))
            .expect("storage_impl.rs");
    let start = src
        .find("async fn delete_by_document")
        .expect("delete_by_document");
    let body = &src[start..start + 1800.min(src.len() - start)];
    assert!(
        body.contains("WHERE ctid IN") && body.contains("UNION"),
        "delete_by_document must use UNION ctid delete arms (F-090-09)"
    );
    assert!(
        !body.contains("document_id = $1\n               OR metadata"),
        "delete_by_document must not use bare OR predicate"
    );
}

#[test]
fn contract_spec090_vector_query_statement_timeout() {
    let src =
        fs::read_to_string("crates/edgequake-storage/src/adapters/postgres/vector/storage_impl.rs")
            .or_else(|_| fs::read_to_string("src/adapters/postgres/vector/storage_impl.rs"))
            .expect("storage_impl.rs");
    assert!(
        src.contains("LocalTimeoutTx") && src.contains("vector_query_statement_timeout_ms"),
        "vector query paths must wrap LocalTimeoutTx (F-090-27)"
    );
}

#[test]
fn contract_spec090_index_ddl_concurrently() {
    let ddl = fs::read_to_string("crates/edgequake-storage/src/adapters/postgres/vector/ddl.rs")
        .or_else(|_| fs::read_to_string("src/adapters/postgres/vector/ddl.rs"))
        .expect("ddl.rs");
    assert!(
        ddl.contains("CREATE INDEX CONCURRENTLY IF NOT EXISTS"),
        "execute_index_ddl must upgrade to CONCURRENTLY for non-empty tables (F-090-08)"
    );
    assert!(
        ddl.contains("vector_index_validity") && ddl.contains("DROP INDEX CONCURRENTLY IF EXISTS"),
        "execute_index_ddl must clean INVALID leftovers (F-090-08)"
    );
}
