//! Prometheus metrics registry (DRY — single recorder for `/metrics`).

use std::sync::OnceLock;

use metrics::{
    counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram, Unit,
};

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

const HTTP_REQUESTS: &str = "edgequake_http_requests_total";
const HTTP_DURATION: &str = "edgequake_http_request_duration_seconds";
const QUERY_REQUESTS: &str = "edgequake_query_requests_total";
const QUERY_DURATION: &str = "edgequake_query_duration_seconds";
const RATE_LIMIT_EXCEEDED: &str = "edgequake_rate_limit_exceeded_total";
const LLM_REQUESTS: &str = "edgequake_llm_requests_total";
const LLM_DURATION: &str = "edgequake_llm_request_duration_seconds";
const DOCUMENT_PROCESSING: &str = "edgequake_document_processing_total";
const DOCUMENT_DURATION: &str = "edgequake_document_processing_duration_seconds";
const STORAGE_ERRORS: &str = "edgequake_storage_errors_total";
const PIPELINE_ERRORS: &str = "edgequake_pipeline_errors_total";
const DB_POOL_CONNECTIONS: &str = "edgequake_db_pool_connections";
const TASK_QUEUE_PENDING: &str = "edgequake_task_queue_pending";
const TASK_QUEUE_PROCESSING: &str = "edgequake_task_queue_processing";
const TASK_QUEUE_FAILED: &str = "edgequake_task_queue_failed";
const INGESTION_CHUNK_STRATEGY: &str = "edgequake_ingestion_chunk_strategy_total";
const INGESTION_SECTION_CONTEXT: &str = "edgequake_ingestion_section_context_total";
const INGESTION_FAILURES: &str = "edgequake_ingestion_failures_total";
const COMPENSATION_QUARANTINE: &str = "edgequake_compensation_quarantine_total";
const COMPENSATE_SHARED_SKIPPED: &str = "edgequake_compensate_shared_entity_skipped_total";
const RETRACT_ON_CANCEL: &str = "edgequake_retract_on_cancel_total";
const VECTOR_DIM_MISMATCH_REJECTED: &str = "edgequake_vector_dim_mismatch_rejected_total";
const CHUNK_STRATEGY_DEGRADED: &str = "edgequake_ingestion_chunk_strategy_degraded_total";
const VECTOR_ANN_INDEX_MISSING: &str = "edgequake_vector_ann_index_missing";
const COMMUNITY_SAMPLED: &str = "edgequake_community_detection_sampled_total";
const POPULAR_NODE_FALLBACK: &str = "edgequake_query_popular_node_fallback_total";
const SPARSE_RETRIEVAL: &str = "edgequake_query_sparse_retrieval_total";
const STORAGE_DRIFT: &str = "edgequake_storage_drift_violations_total";
const STORAGE_DRIFT_CRITICAL: &str = "edgequake_storage_drift_critical";
const FAITHFULNESS_SAMPLES: &str = "edgequake_faithfulness_samples_total";
const FAITHFULNESS_SCORE: &str = "edgequake_faithfulness_score";
const GRAPH_QUALITY_NODES: &str = "edgequake_graph_quality_nodes";
const GRAPH_QUALITY_EDGES: &str = "edgequake_graph_quality_edges";
const GRAPH_QUALITY_AVG_DEGREE: &str = "edgequake_graph_quality_avg_degree";
const GRAPH_QUALITY_ORPHAN_RATE: &str = "edgequake_graph_quality_orphan_rate";
const GRAPH_QUALITY_EMPTY_DESC_RATE: &str = "edgequake_graph_quality_empty_description_rate";
const GRAPH_QUALITY_SPARSE: &str = "edgequake_graph_quality_sparse";
const INGEST_STAGE_DURATION: &str = "edgequake_ingest_stage_duration_seconds";
const QUERY_ARM_DURATION: &str = "edgequake_query_arm_duration_seconds";
const STORAGE_OP_DURATION: &str = "edgequake_storage_op_duration_seconds";

/// Pre-register metric metadata so `/metrics` is never an empty body before first request.
fn describe_http_metrics() {
    describe_counter!(
        HTTP_REQUESTS,
        "Total HTTP requests handled by EdgeQuake API"
    );
    describe_histogram!(
        HTTP_DURATION,
        Unit::Seconds,
        "HTTP request duration in seconds"
    );
    describe_counter!(QUERY_REQUESTS, "Total RAG query executions");
    describe_histogram!(
        QUERY_DURATION,
        Unit::Seconds,
        "RAG query end-to-end duration in seconds"
    );
    describe_counter!(
        RATE_LIMIT_EXCEEDED,
        "HTTP requests rejected due to rate limiting"
    );
    describe_counter!(
        LLM_REQUESTS,
        "LLM provider calls (query generation and errors)"
    );
    describe_histogram!(LLM_DURATION, Unit::Seconds, "LLM call duration in seconds");
    describe_counter!(
        DOCUMENT_PROCESSING,
        "Document and PDF processing outcomes by task type and stage"
    );
    describe_histogram!(
        DOCUMENT_DURATION,
        Unit::Seconds,
        "Document processing duration in seconds"
    );
    describe_counter!(
        STORAGE_ERRORS,
        "Storage layer errors surfaced to the API by category"
    );
    describe_counter!(
        PIPELINE_ERRORS,
        "Pipeline errors surfaced to the API by category"
    );
    describe_gauge!(
        DB_POOL_CONNECTIONS,
        "PostgreSQL pool connections (sampled on /metrics scrape)"
    );
    describe_gauge!(TASK_QUEUE_PENDING, "Pending tasks in the worker queue");
    describe_gauge!(TASK_QUEUE_PROCESSING, "Tasks currently being processed");
    describe_gauge!(
        TASK_QUEUE_FAILED,
        "Failed tasks awaiting operator attention"
    );
    describe_counter!(
        INGESTION_CHUNK_STRATEGY,
        "Document ingest completions by chunk strategy (SPEC-026)"
    );
    describe_counter!(
        INGESTION_SECTION_CONTEXT,
        "Document ingest completions where section context was applied to chunks"
    );
    describe_counter!(
        INGESTION_FAILURES,
        "Document ingestion terminal failures by failure_class and workspace"
    );
    describe_counter!(
        COMPENSATION_QUARANTINE,
        "Saga compensation cleanup failures requiring operator quarantine"
    );
    describe_counter!(
        COMPENSATE_SHARED_SKIPPED,
        "SPEC-059: shared entity/rel vectors excluded from compensate delete lists"
    );
    describe_counter!(
        RETRACT_ON_CANCEL,
        "SPEC-059: document index retract operations (cancel/orphan)"
    );
    describe_counter!(
        VECTOR_DIM_MISMATCH_REJECTED,
        "SPEC-059: vector dimension mismatch rejected (fail-closed, no DROP)"
    );
    describe_counter!(
        CHUNK_STRATEGY_DEGRADED,
        "Semantic (or other) chunk strategy degraded to fallback"
    );
    describe_gauge!(
        VECTOR_ANN_INDEX_MISSING,
        "Count of vector tables missing HNSW/IVFFlat ANN index"
    );
    describe_counter!(
        COMMUNITY_SAMPLED,
        "Community detection runs that used a sampled subgraph"
    );
    describe_counter!(
        POPULAR_NODE_FALLBACK,
        "Queries that fell back to popular-node graph retrieval"
    );
    describe_counter!(
        SPARSE_RETRIEVAL,
        "Sparse/FTS fusion outcomes (postgres_fts, in_memory_bm25, fallbacks)"
    );
    describe_counter!(
        STORAGE_DRIFT,
        "Storage inspector invariant violations by severity"
    );
    describe_gauge!(
        STORAGE_DRIFT_CRITICAL,
        "Count of CRITICAL storage drift violations from last inspect"
    );
    describe_counter!(FAITHFULNESS_SAMPLES, "Online faithfulness samples recorded");
    describe_histogram!(
        FAITHFULNESS_SCORE,
        "Online faithfulness heuristic score in [0,1]"
    );
    describe_gauge!(
        GRAPH_QUALITY_NODES,
        "Knowledge-graph node count from last quality sample (SPEC-046 OPS-23)"
    );
    describe_gauge!(
        GRAPH_QUALITY_EDGES,
        "Knowledge-graph edge count from last quality sample (SPEC-046 OPS-23)"
    );
    describe_gauge!(
        GRAPH_QUALITY_AVG_DEGREE,
        "Knowledge-graph average degree from last quality sample (SPEC-046 OPS-23)"
    );
    describe_gauge!(
        GRAPH_QUALITY_ORPHAN_RATE,
        "Fraction of orphan nodes from last quality sample (SPEC-046 OPS-23)"
    );
    describe_gauge!(
        GRAPH_QUALITY_EMPTY_DESC_RATE,
        "Fraction of nodes with empty description from last quality sample"
    );
    describe_gauge!(
        GRAPH_QUALITY_SPARSE,
        "1 when graph quality sample is sparse (avg_degree < 2 on ≥10 nodes)"
    );
    describe_histogram!(
        INGEST_STAGE_DURATION,
        Unit::Seconds,
        "SPEC-060: ingest persist stage wall time (kv, vector, merge, compensate)"
    );
    describe_histogram!(
        QUERY_ARM_DURATION,
        Unit::Seconds,
        "SPEC-060: Mix/Hybrid arm wall time (local, global, naive)"
    );
    describe_histogram!(
        STORAGE_OP_DURATION,
        Unit::Seconds,
        "SPEC-060: storage op wall time (query_filtered, text_search, expand)"
    );
}

static PROMETHEUS: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the global Prometheus recorder. Idempotent.
pub fn init_metrics() {
    let _ = PROMETHEUS.get_or_init(|| {
        let handle = PrometheusBuilder::new()
            .install_recorder()
            .expect("failed to install Prometheus metrics recorder");
        describe_http_metrics();
        // Exporter registers series on first sample; seed so cold `/metrics` is valid Prometheus text.
        counter!(
            HTTP_REQUESTS,
            "method" => "GET",
            "path" => "_bootstrap",
            "status" => "0"
        )
        .increment(0);
        histogram!(HTTP_DURATION, "method" => "GET", "path" => "_bootstrap").record(0.0);
        counter!(
            QUERY_REQUESTS,
            "mode" => "bootstrap",
            "outcome" => "success"
        )
        .increment(0);
        histogram!(QUERY_DURATION, "mode" => "bootstrap").record(0.0);
        counter!(RATE_LIMIT_EXCEEDED, "scope" => "bootstrap").increment(0);
        counter!(
            LLM_REQUESTS,
            "provider" => "bootstrap",
            "operation" => "query",
            "outcome" => "success"
        )
        .increment(0);
        histogram!(LLM_DURATION, "provider" => "bootstrap", "operation" => "query").record(0.0);
        counter!(
            DOCUMENT_PROCESSING,
            "task_type" => "bootstrap",
            "stage" => "pipeline",
            "outcome" => "success"
        )
        .increment(0);
        histogram!(
            DOCUMENT_DURATION,
            "task_type" => "bootstrap",
            "stage" => "pipeline"
        )
        .record(0.0);
        counter!(
            STORAGE_ERRORS,
            "category" => "bootstrap",
            "error_code" => "BOOTSTRAP"
        )
        .increment(0);
        counter!(
            PIPELINE_ERRORS,
            "category" => "bootstrap",
            "error_code" => "BOOTSTRAP"
        )
        .increment(0);
        gauge!(DB_POOL_CONNECTIONS, "state" => "total").set(0.0);
        gauge!(DB_POOL_CONNECTIONS, "state" => "idle").set(0.0);
        gauge!(DB_POOL_CONNECTIONS, "state" => "active").set(0.0);
        gauge!(TASK_QUEUE_PENDING).set(0.0);
        gauge!(TASK_QUEUE_PROCESSING).set(0.0);
        gauge!(TASK_QUEUE_FAILED).set(0.0);
        counter!(
            INGESTION_CHUNK_STRATEGY,
            "chunk_strategy" => "bootstrap",
            "outcome" => "success"
        )
        .increment(0);
        counter!(
            INGESTION_SECTION_CONTEXT,
            "used" => "false",
            "outcome" => "success"
        )
        .increment(0);
        counter!(
            INGESTION_FAILURES,
            "failure_class" => "bootstrap",
            "workspace" => "bootstrap"
        )
        .increment(0);
        counter!(
            COMPENSATION_QUARANTINE,
            "kind" => "bootstrap"
        )
        .increment(0);
        handle
    });
}

/// Record a storage error surfaced as an API failure.
pub fn record_storage_error(category: &str, error_code: &str) {
    init_metrics();
    counter!(
        STORAGE_ERRORS,
        "category" => category.to_string(),
        "error_code" => error_code.to_string()
    )
    .increment(1);
}

/// Record a pipeline error surfaced as an API failure.
pub fn record_pipeline_error(category: &str, error_code: &str) {
    init_metrics();
    counter!(
        PIPELINE_ERRORS,
        "category" => category.to_string(),
        "error_code" => error_code.to_string()
    )
    .increment(1);
}

/// Update task queue depth gauges (call from `/health` operational snapshot).
pub fn record_task_queue_stats(pending: u64, processing: u64, failed: u64) {
    init_metrics();
    gauge!(TASK_QUEUE_PENDING).set(pending as f64);
    gauge!(TASK_QUEUE_PROCESSING).set(processing as f64);
    gauge!(TASK_QUEUE_FAILED).set(failed as f64);
}

/// Update DB pool gauges (call before Prometheus scrape when pool is available).
pub fn record_db_pool_stats(size: u32, idle: u32) {
    record_db_pool_stats_for_role("primary", size, idle);
}

/// Per-role pool gauges (SPEC-090 F-090-28): `role` ∈ query|ingest|queue|admin.
pub fn record_db_pool_stats_for_role(role: &str, size: u32, idle: u32) {
    init_metrics();
    let active = size.saturating_sub(idle);
    let role = role.to_string();
    gauge!(DB_POOL_CONNECTIONS, "state" => "total", "role" => role.clone()).set(size as f64);
    gauge!(DB_POOL_CONNECTIONS, "state" => "idle", "role" => role.clone()).set(idle as f64);
    gauge!(DB_POOL_CONNECTIONS, "state" => "active", "role" => role).set(active as f64);
}

/// Record document/PDF pipeline processing (task processor layer).
pub fn record_document_processing(task_type: &str, stage: &str, outcome: &str, duration_secs: f64) {
    record_document_processing_with_labels(task_type, stage, outcome, duration_secs, None, false);
}

/// Record ingest pipeline outcome with SPEC-026 chunk strategy / section context labels.
pub fn record_document_processing_with_labels(
    task_type: &str,
    stage: &str,
    outcome: &str,
    duration_secs: f64,
    chunk_strategy: Option<&str>,
    section_context_used: bool,
) {
    init_metrics();
    counter!(
        DOCUMENT_PROCESSING,
        "task_type" => task_type.to_string(),
        "stage" => stage.to_string(),
        "outcome" => outcome.to_string()
    )
    .increment(1);
    histogram!(
        DOCUMENT_DURATION,
        "task_type" => task_type.to_string(),
        "stage" => stage.to_string()
    )
    .record(duration_secs);

    if let Some(strategy) = chunk_strategy {
        counter!(
            INGESTION_CHUNK_STRATEGY,
            "chunk_strategy" => strategy.to_string(),
            "outcome" => outcome.to_string()
        )
        .increment(1);
        counter!(
            INGESTION_SECTION_CONTEXT,
            "used" => section_context_used.to_string(),
            "outcome" => outcome.to_string()
        )
        .increment(1);
    }
}

/// Record an LLM provider call.
pub fn record_llm_request(provider: &str, operation: &str, outcome: &str, duration_secs: f64) {
    init_metrics();
    let provider = if provider.is_empty() {
        "unknown".to_string()
    } else {
        provider.to_string()
    };
    counter!(
        LLM_REQUESTS,
        "provider" => provider.clone(),
        "operation" => operation.to_string(),
        "outcome" => outcome.to_string()
    )
    .increment(1);
    histogram!(
        LLM_DURATION,
        "provider" => provider,
        "operation" => operation.to_string()
    )
    .record(duration_secs);
}

/// Record a rate-limited request (429).
pub fn record_rate_limit_exceeded(scope: &str) {
    init_metrics();
    counter!(RATE_LIMIT_EXCEEDED, "scope" => scope.to_string()).increment(1);
}

/// Record a terminal ingestion failure by taxonomy (SPEC-045 SRE-I06).
pub fn record_ingestion_failure(failure_class: &str, workspace: &str) {
    init_metrics();
    counter!(
        INGESTION_FAILURES,
        "failure_class" => failure_class.to_string(),
        "workspace" => workspace.to_string()
    )
    .increment(1);
}

/// Record saga compensation quarantine (SPEC-045 SRE-I07).
pub fn record_compensation_quarantine(kind: &str) {
    init_metrics();
    counter!(COMPENSATION_QUARANTINE, "kind" => kind.to_string()).increment(1);
}

/// Record shared-entity compensate skips (SPEC-058/059).
pub fn record_compensate_shared_entity_skipped(n: u64) {
    if n == 0 {
        return;
    }
    init_metrics();
    counter!(COMPENSATE_SHARED_SKIPPED).increment(n);
}

/// Record document index retract (cancel / orphan) (SPEC-059).
pub fn record_retract_on_cancel() {
    init_metrics();
    counter!(RETRACT_ON_CANCEL).increment(1);
}

/// Record fail-closed vector dimension mismatch (SPEC-058/059).
pub fn record_vector_dim_mismatch_rejected() {
    init_metrics();
    counter!(VECTOR_DIM_MISMATCH_REJECTED).increment(1);
}

/// SPEC-060: record ingest persist stage duration (`kv`, `chunk_vector`, `merge`, `compensate`).
pub fn record_ingest_stage_duration(stage: &str, duration_secs: f64) {
    init_metrics();
    histogram!(INGEST_STAGE_DURATION, "stage" => stage.to_string()).record(duration_secs.max(0.0));
}

/// SPEC-060: record Mix/Hybrid arm wall time (`local`, `global`, `naive`).
pub fn record_query_arm_duration(arm: &str, duration_secs: f64) {
    init_metrics();
    histogram!(QUERY_ARM_DURATION, "arm" => arm.to_string()).record(duration_secs.max(0.0));
}

/// SPEC-060: record storage op duration (`query_filtered`, `text_search_filtered`, `incident_edges`).
pub fn record_storage_op_duration(op: &str, duration_secs: f64) {
    init_metrics();
    histogram!(STORAGE_OP_DURATION, "op" => op.to_string()).record(duration_secs.max(0.0));
}

/// Record chunk strategy degradation (SPEC-046 OPS-P0.1).
pub fn record_chunk_strategy_degraded(requested: &str, effective: &str) {
    init_metrics();
    counter!(
        CHUNK_STRATEGY_DEGRADED,
        "requested" => requested.to_string(),
        "effective" => effective.to_string()
    )
    .increment(1);
}

/// Set missing ANN index gauge (SPEC-046 OPS-P0.3).
pub fn set_vector_ann_index_missing(count: u64) {
    init_metrics();
    gauge!(VECTOR_ANN_INDEX_MISSING).set(count as f64);
}

/// Record sampled community detection (SPEC-046 OPS-P0.2).
pub fn record_community_sampled() {
    init_metrics();
    counter!(COMMUNITY_SAMPLED).increment(1);
}

/// Record a completed RAG query (API handler).
pub fn record_query_completed(mode: &str, outcome: &str, duration_secs: f64) {
    init_metrics();
    counter!(
        QUERY_REQUESTS,
        "mode" => mode.to_string(),
        "outcome" => outcome.to_string()
    )
    .increment(1);
    histogram!(QUERY_DURATION, "mode" => mode.to_string()).record(duration_secs);
}

/// Record popular-node fallback (SPEC-046 OPS-P2.14).
pub fn record_popular_node_fallback(arm: &str) {
    init_metrics();
    counter!(POPULAR_NODE_FALLBACK, "arm" => arm.to_string()).increment(1);
}

/// Record sparse/FTS fusion outcome (SPEC-046 OPS-P2.15).
pub fn record_sparse_retrieval_outcome(outcome: &str) {
    init_metrics();
    counter!(SPARSE_RETRIEVAL, "outcome" => outcome.to_string()).increment(1);
}

/// Record storage drift violations (SPEC-046 OPS-P2.19).
pub fn record_storage_drift(invariant: &str, severity: &str, count: u64) {
    init_metrics();
    counter!(
        STORAGE_DRIFT,
        "invariant" => invariant.to_string(),
        "severity" => severity.to_string()
    )
    .increment(count);
}

/// Set CRITICAL drift gauge from last inspect (OPS-P2.19).
pub fn set_storage_drift_critical(count: u64) {
    init_metrics();
    gauge!(STORAGE_DRIFT_CRITICAL).set(count as f64);
}

/// Record an online faithfulness sample (OPS-P2.20).
pub fn record_faithfulness_sample(score: f64) {
    init_metrics();
    counter!(FAITHFULNESS_SAMPLES).increment(1);
    histogram!(FAITHFULNESS_SCORE).record(score.clamp(0.0, 1.0));
}

/// Record graph structural quality gauges (SPEC-046 OPS-P3.23).
///
/// Call after ingest merge / quality sample. Labels keep multi-workspace
/// scrapes distinguishable without exploding cardinality (workspace only).
pub fn record_graph_quality(
    workspace: &str,
    node_count: u64,
    edge_count: u64,
    avg_degree: f64,
    orphan_rate: f64,
    empty_description_rate: f64,
    sparse: bool,
) {
    init_metrics();
    let ws = if workspace.is_empty() {
        "default".to_string()
    } else {
        workspace.to_string()
    };
    gauge!(GRAPH_QUALITY_NODES, "workspace" => ws.clone()).set(node_count as f64);
    gauge!(GRAPH_QUALITY_EDGES, "workspace" => ws.clone()).set(edge_count as f64);
    gauge!(GRAPH_QUALITY_AVG_DEGREE, "workspace" => ws.clone()).set(avg_degree);
    gauge!(GRAPH_QUALITY_ORPHAN_RATE, "workspace" => ws.clone()).set(orphan_rate.clamp(0.0, 1.0));
    gauge!(GRAPH_QUALITY_EMPTY_DESC_RATE, "workspace" => ws.clone())
        .set(empty_description_rate.clamp(0.0, 1.0));
    gauge!(GRAPH_QUALITY_SPARSE, "workspace" => ws).set(if sparse { 1.0 } else { 0.0 });
}

/// Render metrics in Prometheus text exposition format.
pub fn render_prometheus_metrics() -> String {
    init_metrics();
    PROMETHEUS
        .get()
        .map(|h| h.render())
        .unwrap_or_else(|| "# metrics not initialized\n".to_string())
}

/// Record one HTTP request (called from API middleware).
pub fn record_http_request(method: &str, path: &str, status: u16, duration_secs: f64) {
    init_metrics();
    let status_label = status.to_string();
    let route = normalize_route(path);

    counter!(
        HTTP_REQUESTS,
        "method" => method.to_string(),
        "path" => route.clone(),
        "status" => status_label
    )
    .increment(1);

    histogram!(
        HTTP_DURATION,
        "method" => method.to_string(),
        "path" => route
    )
    .record(duration_secs);
}

/// Normalize paths for metric cardinality (replace UUID-like segments).
pub fn normalize_route(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    let normalized: Vec<String> = parts
        .iter()
        .map(|p| {
            let looks_like_uuid = p.len() == 36 && p.chars().filter(|c| *c == '-').count() == 4;
            let looks_like_hex_id =
                p.len() > 20 && p.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
            if looks_like_uuid || looks_like_hex_id {
                ":id".to_string()
            } else {
                (*p).to_string()
            }
        })
        .collect();
    normalized.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_uuid_in_path() {
        let p = "/api/v1/workspaces/550e8400-e29b-41d4-a716-446655440000/documents";
        let n = normalize_route(p);
        assert!(n.contains(":id"));
    }

    #[test]
    fn scrape_includes_described_metrics_before_traffic() {
        let body = render_prometheus_metrics();
        assert!(
            body.contains(HTTP_REQUESTS),
            "metrics scrape should list HTTP counter before any request: {body:?}"
        );
        assert!(
            body.contains(DOCUMENT_PROCESSING),
            "metrics scrape should list document processing counter: {body:?}"
        );
        assert!(
            body.contains(STORAGE_ERRORS),
            "metrics scrape should list storage error counter: {body:?}"
        );
        assert!(
            body.contains(DB_POOL_CONNECTIONS),
            "metrics scrape should list db pool gauge: {body:?}"
        );
        assert!(
            body.contains(TASK_QUEUE_PENDING),
            "metrics scrape should list task queue pending gauge: {body:?}"
        );
        assert!(
            body.contains(INGESTION_CHUNK_STRATEGY),
            "metrics scrape should list ingestion chunk strategy counter: {body:?}"
        );
    }

    #[test]
    fn spec060_stage_and_arm_helpers_record() {
        record_ingest_stage_duration("kv_upsert", 0.012);
        record_query_arm_duration("local", 0.045);
        record_storage_op_duration("query_filtered", 0.008);
        let body = render_prometheus_metrics();
        assert!(
            body.contains(INGEST_STAGE_DURATION),
            "ingest stage histogram missing: {body:?}"
        );
        assert!(
            body.contains(QUERY_ARM_DURATION),
            "query arm histogram missing: {body:?}"
        );
        assert!(
            body.contains(STORAGE_OP_DURATION),
            "storage op histogram missing: {body:?}"
        );
    }
}
