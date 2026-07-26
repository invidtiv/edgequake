//! SPEC-089 / GH-336 — list must reconcile entity counts only after pagination.

#[test]
fn iss089_list_reconcile_after_page() {
    let list = include_str!("../src/handlers/documents/query/list.rs");

    assert!(
        list.contains("document_read_model::reconcile_entity_counts_with_graph"),
        "documents list must still call AGE entity_count reconcile"
    );
    assert!(
        list.contains("paginate_vec"),
        "documents list must paginate"
    );

    let paginate_pos = list.find("paginate_vec").expect("paginate_vec call site");
    // Prefer the assignment call site used for the response page.
    let page_assign = list
        .find("paginate_vec(documents, page, page_size)")
        .unwrap_or(paginate_pos);
    let reconcile_pos = list
        .find("document_read_model::reconcile_entity_counts_with_graph")
        .expect("reconcile call site");

    assert!(
        reconcile_pos > page_assign,
        "SPEC-089 / LAW-H1: reconcile_entity_counts_with_graph must run AFTER paginate_vec \
         (found reconcile at {reconcile_pos}, paginate at {page_assign})"
    );

    // Guard: no pre-pagination reconcile block left above filters.
    let before_paginate = &list[..page_assign];
    assert!(
        !before_paginate.contains("reconcile_entity_counts_with_graph"),
        "must not reconcile entity counts before pagination (GH-336 corpus×256 probes)"
    );
}

#[test]
fn iss089_read_model_uses_capped_counts() {
    let model = include_str!("../src/document_read_model.rs");
    assert!(
        model.contains("node_counts_by_source_prefixes_capped"),
        "P-A3 must call capped prefix counts (SPEC-089 probe bound)"
    );
    assert!(
        model.contains("reconcile_probe_limit_from_chunk_counts"),
        "P-A3 must derive probe_limit from page chunk_count"
    );
}

#[test]
fn iss089_storage_count_uses_statement_timeout() {
    let analytics =
        include_str!("../../edgequake-storage/src/adapters/postgres/graph/analytics_ops.rs");
    assert!(
        analytics.contains("LocalTimeoutTx")
            || analytics.contains("SOURCE_COUNT_STATEMENT_TIMEOUT_MS"),
        "count path must use LocalTimeoutTx / statement_timeout (LAW-H2)"
    );
    let scan = include_str!("../../edgequake-storage/src/adapters/postgres/graph/scan_ops.rs");
    assert!(
        scan.contains("LocalTimeoutTx") && scan.contains("SOURCE_DISCOVERY_STATEMENT_TIMEOUT_MS"),
        "discovery path must use LocalTimeoutTx (F-336-08)"
    );
    let tasks = include_str!("../../edgequake-tasks/src/postgres.rs");
    assert!(
        tasks.contains("SET LOCAL statement_timeout")
            && tasks.contains("STATS_STATEMENT_TIMEOUT_MS"),
        "task get_statistics must SET LOCAL (F-336-09)"
    );
    let read_path = include_str!("../src/read_path.rs");
    assert!(
        read_path.contains("ENTITY_RECONCILE_STATS_APP_TIMEOUT_MS") && read_path.contains("550"),
        "list skip stats app timeout must be 550ms > PG 500ms (F-336-16 / LAW-H2)"
    );
    let search =
        include_str!("../../edgequake-storage/src/adapters/postgres/graph/query_ops/search.rs");
    assert!(
        search.matches("LocalTimeoutTx").count() >= 2,
        "popular_labels + search_labels must use LocalTimeoutTx (F-336-15)"
    );
    let edges = include_str!("../../edgequake-storage/src/adapters/postgres/graph/edges_ops.rs");
    assert!(
        edges.contains("pg_get_incident_edges_batch") && edges.contains("LocalTimeoutTx"),
        "BFS incident edges batch must use LocalTimeoutTx (F-336-15)"
    );
    let stmt_to =
        include_str!("../../edgequake-storage/src/adapters/postgres/statement_timeout.rs");
    assert!(
        stmt_to.contains("GRAPH_QUERY_PG_HEADROOM_MS") && stmt_to.contains("saturating_sub"),
        "graph PG timeout must be under app budget (LAW-H2)"
    );
    assert!(
        stmt_to.contains("interactive_statement_timeout_ms"),
        "Phase 4 F-336-13: interactive read-path PG budget helper required"
    );
    assert!(
        analytics.contains("WORKSPACE_STATS_STATEMENT_TIMEOUT_MS"),
        "Phase 4 F-336-14: workspace AGE counts must SET LOCAL under 4s app budget"
    );
}

#[test]
fn iss089_phase4_reprocess_single_cascade() {
    let reprocess = include_str!("../src/handlers/documents/recovery/reprocess.rs");
    assert!(
        reprocess.contains("retract_document_indexes"),
        "reprocess admit must retract indexes (SPEC-059)"
    );
    // Comment may mention the forbidden helper; call site must be gone.
    assert!(
        !reprocess.contains("cleanup_document_graph_data("),
        "F-336-12: reprocess must NOT call cleanup_document_graph_data after retract"
    );
    assert!(
        reprocess.contains("single cascade") || reprocess.contains("F-336-12"),
        "reprocess admit must document single-cascade LAW-H1 fix"
    );
}
