//! EdgeQuake observability — single init point for tracing, metrics, and correlation.
//!
//! ## SOLID
//! - **S**: This crate only configures observability; handlers use `tracing` macros.
//! - **D**: Application code depends on `RequestContext`, not OTEL types.

pub mod error_context;
pub mod http_span;
pub mod propagation;
pub mod rag_span;
pub mod request_context;
pub mod subscriber;

#[cfg(feature = "otel")]
pub mod trace_context;

pub mod query_guard;

#[cfg(feature = "metrics")]
pub mod metrics;

pub use error_context::ErrorEvent;
pub use http_span::{record_http_error, record_http_status, with_http_span};
pub use query_guard::{QueryFailureGuard, QueryOutcomeGuard};
pub use rag_span::{
    query_preview, record_rag_retrieval_outcome, with_rag_generation_span, with_rag_retrieval_span,
    RagRetrievalAttrs,
};

pub use propagation::{harvest_propagation_headers, PropagationHeaders};
pub use request_context::{
    current_llm_provider, current_request_id, parse_trace_id_from_traceparent, resolve_request_id,
    scope_llm_provider, scope_request_id, synthesize_traceparent_from_request_id,
    trace_id_from_request_id, RequestContext, CORRELATION_ID_HEADER, REQUEST_ID_HEADER,
    TRACEPARENT_HEADER, TRACESTATE_HEADER,
};
pub use subscriber::{
    init_observability, log_format_label, LogFormat, ObservabilityConfig, ObservabilityGuard,
};
#[cfg(feature = "otel")]
pub use trace_context::{extract_from_headers, inject_current_context};

#[cfg(feature = "metrics")]
pub use metrics::{
    init_metrics, record_chunk_strategy_degraded, record_community_sampled,
    record_compensate_shared_entity_skipped, record_compensation_quarantine, record_db_pool_stats,
    record_db_pool_stats_for_role, record_document_processing,
    record_document_processing_with_labels, record_faithfulness_sample, record_graph_quality,
    record_http_request, record_ingest_stage_duration, record_ingestion_failure,
    record_llm_request, record_pipeline_error, record_popular_node_fallback,
    record_query_arm_duration, record_query_completed, record_rate_limit_exceeded,
    record_retract_on_cancel, record_sparse_retrieval_outcome, record_storage_drift,
    record_storage_error, record_storage_op_duration, record_task_queue_stats,
    record_vector_dim_mismatch_rejected, render_prometheus_metrics, set_storage_drift_critical,
    set_vector_ann_index_missing,
};
