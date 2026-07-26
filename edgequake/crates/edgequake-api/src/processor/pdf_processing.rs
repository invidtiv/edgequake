use super::*;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "postgres")]
fn strip_nul_bytes(text: String) -> String {
    if !text.contains('\0') {
        return text;
    }

    let nul_count = text.chars().filter(|&ch| ch == '\0').count();
    let sanitized = text.replace('\0', "");
    warn!(
        nul_count,
        sanitized_len = sanitized.len(),
        "Removed NUL bytes from extracted PDF markdown before persistence"
    );
    sanitized
}

#[cfg(feature = "postgres")]
fn task_error_to_vision_failure(
    error: &edgequake_tasks::TaskError,
) -> edgequake_pdf::VisionFailureKind {
    match error {
        edgequake_tasks::TaskError::Timeout(_) => edgequake_pdf::VisionFailureKind::Timeout,
        edgequake_tasks::TaskError::Processing(_) => {
            edgequake_pdf::VisionFailureKind::ConversionFailed
        }
        edgequake_tasks::TaskError::UnsupportedOperation(_) => {
            edgequake_pdf::VisionFailureKind::FeatureUnavailable
        }
        _ => edgequake_pdf::VisionFailureKind::ProviderUnavailable,
    }
}

#[cfg(feature = "postgres")]
fn vision_fallback_allowed(
    requested_backend: edgequake_pdf::PdfParserBackend,
    error: &edgequake_tasks::TaskError,
    backend_explicit: bool,
) -> bool {
    edgequake_pdf::should_fallback_to_edgeparse(
        requested_backend,
        task_error_to_vision_failure(error),
        backend_explicit,
    )
}

#[cfg(feature = "postgres")]
fn build_edgeparse_fallback_message(provider: &str, error: &edgequake_tasks::TaskError) -> String {
    edgequake_pdf::build_edgeparse_fallback_message(provider, &error.to_string())
}

#[cfg(feature = "postgres")]
fn merge_extraction_notice(
    extraction_errors: &mut Option<serde_json::Value>,
    key: &str,
    message: String,
) {
    let notice = json!({ "message": message });
    match extraction_errors {
        Some(serde_json::Value::Object(map)) => {
            map.insert(key.to_string(), notice);
        }
        _ => {
            *extraction_errors = Some(json!({ key: notice }));
        }
    }
}

#[cfg(feature = "postgres")]
fn should_resume_pdf_conversion(has_existing_document: bool, restart_from_scratch: bool) -> bool {
    has_existing_document && !restart_from_scratch
}

/// DRY builder for the follow-on Insert payload after PDF convert (SPEC-057 P2).
#[cfg(feature = "postgres")]
#[allow(clippy::too_many_arguments)]
fn build_text_insert_from_pdf_convert(
    markdown: String,
    filename: &str,
    data: &edgequake_tasks::PdfProcessingData,
    document_id: &str,
    page_count_opt: Option<i32>,
    file_size_bytes: i64,
    sha256_checksum: &str,
    vision_model: Option<String>,
    extraction_method: Option<&str>,
    extraction_warning: Option<String>,
) -> edgequake_tasks::TextInsertData {
    edgequake_tasks::TextInsertData {
        text: markdown,
        file_source: filename.to_string(),
        workspace_id: data.workspace_id.to_string(),
        metadata: Some(json!({
            "document_id": document_id,
            "source": "pdf_upload",
            "source_type": "pdf",
            "document_type": "pdf",
            "pdf_id": data.pdf_id.to_string(),
            "filename": filename,
            "page_count": page_count_opt,
            "file_size_bytes": file_size_bytes,
            "sha256_checksum": sha256_checksum,
            "tenant_id": data.tenant_id.to_string(),
            "workspace_id": data.workspace_id.to_string(),
            "pdf_vision_model": vision_model,
            "pdf_extraction_method": extraction_method,
            "pdf_extraction_warning": extraction_warning,
            "force_fresh_extraction": data.restart_from_scratch,
            "merge_only": data.reprocess_mode
                .map(|m| m.merge_only())
                .unwrap_or(false),
        })),
    }
}

#[cfg(feature = "postgres")]
fn should_restart_pdf_conversion(has_existing_document: bool, restart_from_scratch: bool) -> bool {
    has_existing_document && restart_from_scratch
}

#[cfg(feature = "postgres")]
fn compute_safe_pdf_resource_profile(
    page_count: usize,
    file_size_bytes: i64,
    vision_provider: &str,
) -> (usize, u32) {
    use crate::safety_limits::is_local_provider;

    let is_local = is_local_provider(vision_provider);
    let large_file = file_size_bytes >= 25 * 1024 * 1024;
    let huge_file = file_size_bytes >= 50 * 1024 * 1024;

    // SPEC-083 X-12: cloud page-count match was decorative (all arms → 2).
    // Documented constant until a real VRAM/pages schedule exists.
    const CLOUD_PDF_PAGE_CONCURRENCY: usize = 2;
    let concurrency = {
        let computed = if is_local {
            if huge_file || page_count >= 200 {
                1
            } else {
                2
            }
        } else if huge_file || page_count >= 1000 {
            1
        } else {
            // P-G13: cap cloud concurrency — N concurrent PDF tasks × page
            // parallelism → multi-MiB base64 buffers can OOM-kill the API.
            CLOUD_PDF_PAGE_CONCURRENCY
        };
        std::env::var("EDGEQUAKE_PDF_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .map(|cap| computed.min(cap.max(1)))
            .unwrap_or(computed)
    };

    let dpi = if huge_file || page_count >= 1000 {
        96
    } else if large_file || page_count >= 500 {
        110
    } else if page_count >= 200 {
        120
    } else {
        150
    };

    (concurrency.max(1), dpi)
}

impl DocumentTaskProcessor {
    /// Enqueue KG ingest as `TaskType::Insert` after durable convert (SPEC-057 P2).
    ///
    /// Idempotent: reuses an already-active Insert for the same `pdf_id`.
    #[cfg(feature = "postgres")]
    async fn enqueue_pdf_ingest_insert(
        &self,
        convert_task: &Task,
        data: &edgequake_tasks::PdfProcessingData,
        text_data: edgequake_tasks::TextInsertData,
        ingest_timeout_secs: u64,
    ) -> TaskResult<String> {
        let storage = self.task_storage.as_ref().ok_or_else(|| {
            edgequake_tasks::TaskError::Processing(
                "Task storage required to enqueue PDF ingest Insert (SPEC-057 P2)".to_string(),
            )
        })?;
        let queue = self.task_queue.as_ref().ok_or_else(|| {
            edgequake_tasks::TaskError::Processing(
                "Task queue required to enqueue PDF ingest Insert (SPEC-057 P2)".to_string(),
            )
        })?;

        if let Some(existing) = storage
            .find_active_pdf_ingest_task(data.pdf_id, data.workspace_id)
            .await
            .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?
        {
            info!(
                pdf_id = %data.pdf_id,
                ingest_track_id = %existing.track_id,
                "RESUME: Reusing active Insert for PDF ingest (idempotent)"
            );
            return Ok(existing.track_id);
        }

        let mut insert_task = Task::new(
            convert_task.tenant_id,
            convert_task.workspace_id,
            TaskType::Insert,
            serde_json::to_value(&text_data).map_err(|e| {
                edgequake_tasks::TaskError::InvalidPayload(format!(
                    "Failed to serialize TextInsertData: {e}"
                ))
            })?,
        );
        insert_task.metadata = Some(json!({
            "processing_timeout_secs": ingest_timeout_secs,
            "source": "pdf_convert_follow_on",
            "pdf_id": data.pdf_id.to_string(),
            "convert_track_id": convert_task.track_id,
        }));

        let ingest_track_id = insert_task.track_id.clone();
        let notifier = self
            .task_notifier
            .as_ref()
            .map(|n| n.as_ref() as &dyn edgequake_tasks::TaskNotifier);
        let noop = edgequake_tasks::NoopTaskNotifier;
        edgequake_tasks::enqueue_with_delivery(
            storage,
            queue,
            notifier.unwrap_or(&noop),
            self.task_delivery_mode,
            insert_task,
        )
        .await
        .map_err(|e| {
            edgequake_tasks::TaskError::Processing(format!(
                "Failed to enqueue PDF ingest Insert: {e}"
            ))
        })?;

        info!(
            pdf_id = %data.pdf_id,
            convert_track_id = %convert_task.track_id,
            ingest_track_id = %ingest_track_id,
            ingest_timeout_secs,
            "Enqueued TaskType::Insert for PDF knowledge-graph ingest (SPEC-057 P2)"
        );
        Ok(ingest_track_id)
    }

    /// Persist convert barrier + link PDF, then enqueue Insert (no inline KG).
    #[cfg(feature = "postgres")]
    #[allow(clippy::too_many_arguments)]
    async fn finish_pdf_convert_and_enqueue_ingest(
        &self,
        task: &mut Task,
        data: &edgequake_tasks::PdfProcessingData,
        pdf_storage: &dyn edgequake_storage::PdfDocumentStorage,
        early_doc_id: &str,
        filename: &str,
        markdown: String,
        page_count_opt: Option<i32>,
        file_size_bytes: i64,
        sha256_checksum: &str,
        vision_model: Option<String>,
        extraction_method_str: Option<&str>,
        extraction_warning: Option<String>,
        cancel_token: &CancellationToken,
    ) -> TaskResult<serde_json::Value> {
        self.check_cancelled(cancel_token, "pre-ingest-enqueue", early_doc_id)
            .await?;

        let _ = self
            .update_document_status(
                early_doc_id,
                "processing",
                Some("PDF converted — queueing knowledge-graph ingest"),
            )
            .await;

        if let Ok(document_uuid) = uuid::Uuid::parse_str(early_doc_id) {
            let truncate_at = markdown.len().min(65_536);
            let safe_truncate = markdown[..truncate_at]
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= 65_536)
                .last()
                .unwrap_or(0);
            let _ = pdf_storage
                .ensure_document_record(
                    &document_uuid,
                    &data.workspace_id,
                    Some(&data.tenant_id),
                    filename,
                    &markdown[..safe_truncate],
                    "processing",
                )
                .await;
            let _ = pdf_storage
                .link_pdf_to_document(&data.pdf_id, &document_uuid)
                .await;
        }

        let profile = crate::services::LargeDocumentProfile::new(
            page_count_opt.unwrap_or(1).max(1) as usize,
            file_size_bytes.max(0) as u64,
        );
        let ingest_timeout_secs = profile.ingest_timeout_secs();

        let text_data = build_text_insert_from_pdf_convert(
            markdown.clone(),
            filename,
            data,
            early_doc_id,
            page_count_opt,
            file_size_bytes,
            sha256_checksum,
            vision_model,
            extraction_method_str,
            extraction_warning,
        );

        self.bump_task_progress(task, "enqueue_ingest".to_string(), 5, 90)
            .await;
        let ingest_track_id = self
            .enqueue_pdf_ingest_insert(task, data, text_data, ingest_timeout_secs)
            .await?;

        self.bump_task_progress(task, "complete".to_string(), 6, 100)
            .await;
        info!(
            pdf_id = %data.pdf_id,
            document_id = %early_doc_id,
            ingest_track_id = %ingest_track_id,
            markdown_len = markdown.len(),
            "PDF convert completed — ingest Insert enqueued (SPEC-057 P2)"
        );
        edgequake_observability::record_document_processing(
            "pdf_processing",
            "conversion",
            "success",
            0.0,
        );

        let state = self.pipeline_state.clone();
        let track_id = task.track_id.clone();
        tokio::spawn(async move {
            state.remove_pdf_progress(&track_id).await;
        });

        Ok(json!({
            "status": "converted",
            "pdf_id": data.pdf_id.to_string(),
            "document_id": early_doc_id,
            "ingest_track_id": ingest_track_id,
            "markdown_len": markdown.len(),
            "phase": "convert_complete",
        }))
    }

    /// Process PDF processing task (SPEC-007).
    ///
    /// This method handles the complete PDF processing pipeline:
    /// 1. Load PDF from storage using pdf_id
    /// 2. Extract content (text mode only for now, vision TODO)
    /// 3. Convert to markdown
    /// 4. Create document and trigger standard ingestion
    /// 5. Update PDF status with results
    ///
    /// @implements SPEC-007: PDF Upload Support with Vision LLM Integration
    /// @implements FEAT0704: PDF processing worker
    /// @implements UC0704: System processes PDF in background
    /// @enforces BR0704: PDF processed async with retry logic
    #[cfg(feature = "postgres")]
    pub(super) async fn process_pdf_processing(
        &self,
        task: &mut Task,
        data: edgequake_tasks::PdfProcessingData,
        cancel_token: CancellationToken,
    ) -> TaskResult<serde_json::Value> {
        use edgequake_storage::{
            ExtractionMethod, PdfProcessingStatus, UpdatePdfProcessingRequest,
        };

        info!(
            pdf_id = %data.pdf_id,
            workspace_id = %data.workspace_id,
            enable_vision = data.enable_vision,
            "Starting PDF processing task"
        );

        // 1. Get PDF storage
        let pdf_storage = self.pdf_storage.as_ref().ok_or_else(|| {
            edgequake_tasks::TaskError::UnsupportedOperation(
                "PDF storage not available (postgres feature enabled but storage not initialized)"
                    .to_string(),
            )
        })?;

        // 2. Load PDF from storage
        let pdf = pdf_storage.get_pdf(&data.pdf_id).await.map_err(|e| {
            edgequake_tasks::TaskError::Storage(format!(
                "Failed to load PDF {}: {}",
                data.pdf_id, e
            ))
        })?;

        // Handle case where PDF not found
        let pdf = pdf.ok_or_else(|| {
            edgequake_tasks::TaskError::NotFound(format!("PDF not found: {}", data.pdf_id))
        })?;

        info!(
            pdf_id = %data.pdf_id,
            filename = %pdf.filename,
            size = pdf.file_size_bytes,
            pages = ?pdf.page_count,
            "Loaded PDF from storage"
        );

        let filename = pdf.filename.clone();
        let file_size_bytes = pdf.file_size_bytes;
        // Heal missing page_count: pdfium SSOT first, byte-scan last resort.
        // Accurate pages drive vision_outer_timeout_secs (avoid 520s under-budget).
        let page_count_opt = match pdf.page_count.filter(|&n| n > 0) {
            Some(n) => Some(n),
            None => {
                let healed = crate::handlers::pdf_upload::extract_page_count(&pdf.pdf_data).await;
                if let Some(n) = healed.filter(|&n| n > 0) {
                    info!(
                        pdf_id = %data.pdf_id,
                        page_count = n,
                        "Healed missing pdf_documents.page_count from PDF bytes (pdfium/heuristic)"
                    );
                    if let Err(e) = pdf_storage.update_pdf_page_count(&data.pdf_id, n).await {
                        warn!(
                            pdf_id = %data.pdf_id,
                            error = %e,
                            "Failed to persist healed page_count (non-fatal)"
                        );
                    }
                    Some(n)
                } else {
                    None
                }
            }
        };
        let sha256_checksum = pdf.sha256_checksum.clone();
        let pdf_data = pdf.pdf_data;

        // 3. Update status to processing
        pdf_storage
            .update_pdf_status(&data.pdf_id, PdfProcessingStatus::Processing)
            .await
            .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?;

        // == Progress: loading complete, preparing for conversion ==
        self.bump_task_progress(task, "pdf_loading".to_string(), 1, 5)
            .await;

        let tenant_ctx = crate::middleware::TenantContext {
            tenant_id: Some(data.tenant_id.to_string()),
            workspace_id: Some(data.workspace_id.to_string()),
            user_id: None,
        };

        // 3.1 Create document metadata early with "converting" stage
        // WHY: Users need to see the document appear in the UI immediately with visual feedback
        // showing that PDF → Markdown conversion is happening.
        // OODA-ITERATION-03: Include track_id for cancel button support
        // WHY: Frontend cancel button requires doc.track_id to call POST /tasks/{track_id}/cancel
        // FIX-REBUILD: When rebuilding/reprocessing, reuse the existing document ID
        // to avoid creating orphaned duplicates. Without this, the old document still
        // references the same pdf_id whose markdown_content gets overwritten, causing
        // it to display wrong/hallucinated content from the new extraction.
        let early_doc_id = crate::services::resolve_worker_pdf_document_id(
            crate::services::WorkerPdfDocumentIdRequest {
                kv_storage: &self.kv_storage,
                pdf_document_id: pdf.document_id,
                pdf_id: data.pdf_id,
                task,
                data: &data,
                task_storage: self.task_storage.as_ref(),
                tenant_ctx: Some(&tenant_ctx),
                workspace_service: self.workspace_service.as_deref(),
                #[cfg(feature = "postgres")]
                pg_pool: self.pg_pool.as_ref(),
                #[cfg(feature = "postgres")]
                postgres_capabilities: self.postgres_capabilities.as_ref(),
            },
        )
        .await?;

        let metadata_key = edgequake_storage::kv_keys::doc_metadata(&early_doc_id);
        let has_existing_document = self
            .kv_storage
            .get_by_id(&metadata_key)
            .await
            .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?
            .is_some();
        let should_resume_from_checkpoint =
            should_resume_pdf_conversion(has_existing_document, data.restart_from_scratch);
        let should_cleanup_existing_content =
            should_restart_pdf_conversion(has_existing_document, data.restart_from_scratch);
        // OODA-04: Include file_size_bytes and sha256_checksum in early metadata
        // WHY: Enables complete lineage from the moment the document appears in UI.
        // Without these, users see metadata gaps until processing completes.
        let mut metadata_json = json!({
            "id": early_doc_id,
            "title": filename.clone(),
            "file_name": filename.clone(),
            "source_type": "pdf",
            "document_type": "pdf",
            "status": "processing",
            "current_stage": "converting",
            "stage_message": if should_resume_from_checkpoint {
                match page_count_opt {
                    Some(n) if n > 0 => format!("Resuming PDF to Markdown conversion from saved progress (up to {} pages)", n),
                    _ => "Resuming PDF to Markdown conversion from saved progress...".to_string(),
                }
            } else {
                match page_count_opt {
                    Some(n) if n > 0 => format!("Converting PDF to Markdown (0/{} pages)", n),
                    _ => "Converting PDF to Markdown (detecting pages...)".to_string(),
                }
            },
            "stage_progress": 0.0,
            "pdf_id": data.pdf_id.to_string(),
            "file_size_bytes": file_size_bytes,
            "sha256_checksum": sha256_checksum,
            "page_count": page_count_opt,
            "tenant_id": data.tenant_id.to_string(),
            "workspace_id": data.workspace_id.to_string(),
            "track_id": task.track_id.clone(),
            "created_at": chrono::Utc::now().to_rfc3339(),
            "updated_at": chrono::Utc::now().to_rfc3339(),
        });
        if let Some(obj) = metadata_json.as_object_mut() {
            crate::services::apply_process_options_to_metadata(
                obj,
                data.multimodal_process_options.as_deref(),
            );
        }

        crate::services::upsert_metadata_kv_with_index(
            self.kv_storage.as_ref(),
            &metadata_key,
            metadata_json,
        )
        .await
        .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?;

        // FIX-REBUILD: When reprocessing, clean up old content and chunk KV entries
        // WHY: Old chunks with stale content must be removed before the pipeline
        // creates new ones, otherwise the document ends up with a mix of old and new chunks.
        if should_cleanup_existing_content {
            info!(
                document_id = %early_doc_id,
                pdf_id = %data.pdf_id,
                "Fresh reprocess requested: cleaning up old content and chunks before re-extraction"
            );
            // Remove old content entry
            let content_key = edgequake_storage::kv_keys::doc_content(&early_doc_id);
            let _ = self.kv_storage.delete(&[content_key]).await;

            // P-G7 (RC-12): use the index-friendly `keys_with_prefix` instead
            // of scanning every key and filtering in-memory. The chunk-id prefix
            // `{doc_id}-chunk-` is index-friendly (B-tree prefix scan in
            // Postgres). This collapses an O(W) full-table scan into O(log N + K).
            let chunk_prefix = edgequake_storage::kv_keys::doc_chunk_prefix(&early_doc_id);
            let chunk_keys: Vec<String> = self
                .kv_storage
                .keys_with_prefix(&chunk_prefix)
                .await
                .unwrap_or_default();
            if !chunk_keys.is_empty() {
                info!(
                    document_id = %early_doc_id,
                    chunk_count = chunk_keys.len(),
                    "Removing old chunk entries"
                );
                let _ = self.kv_storage.delete(&chunk_keys).await;
            }
        }

        info!(
            document_id = %early_doc_id,
            pdf_id = %data.pdf_id,
            has_existing_document,
            should_resume_from_checkpoint,
            retry_count = task.retry_count,
            "{}document metadata with 'converting' stage",
            if should_resume_from_checkpoint {
                "Resumed existing "
            } else if has_existing_document {
                "Updated existing "
            } else {
                "Created early "
            }
        );

        // OODA-09: Create progress callback for real-time page-by-page feedback
        // WHY: Users need to see extraction progress like "Extracting page 5/10..."
        // OODA-10: Also attach progress broadcaster if available for WebSocket delivery
        // OODA-16: Add filename for progress display
        let mut callback = PipelineProgressCallback::new(
            self.pipeline_state.clone(),
            data.pdf_id.to_string(),
            task.track_id.clone(),
        )
        .with_filename(filename.clone())
        .with_document_metadata(early_doc_id.clone(), Arc::clone(&self.kv_storage));

        if let Some(ref broadcaster) = self.progress_broadcaster {
            callback = callback.with_broadcaster(broadcaster.clone());
        }
        // Keep concrete Arc so we can report post-OCR converting status + finish phase.
        let progress_callback = Arc::new(callback);

        // 4. Extract content (vision or text mode)
        //
        // RESUME SHORTCUT: If this is a retry and the markdown was already stored
        // in the pdf_documents table from the previous run, skip the expensive
        // PDF→Markdown conversion entirely and jump straight to text_insert.
        //
        // WHY: A failed job (e.g., entity extraction failed at chunk 140/142)
        // should not redo the multi-minute PDF conversion. The markdown is
        // already in the DB; we only need to re-run the ingestion pipeline.
        if should_resume_from_checkpoint {
            if let Some(stored_markdown) = pdf.markdown_content.clone() {
                if !stored_markdown.is_empty() {
                    info!(
                        document_id = %early_doc_id,
                        pdf_id = %data.pdf_id,
                        markdown_len = stored_markdown.len(),
                        "RESUME: Markdown already stored — skipping PDF conversion, enqueueing ingest Insert"
                    );

                    self.bump_task_progress(task, "resume_convert_barrier".to_string(), 3, 45)
                        .await;

                    self.check_cancelled(&cancel_token, "pre-ingest-enqueue-resume", &early_doc_id)
                        .await?;

                    let stored_extraction_method = pdf.extraction_method;
                    let stored_vision_model = pdf.vision_model.clone();

                    // RESUME: apply multimodal analyze stage (LightRAG parity — was skipped before 4d).
                    let mm_asset_base = crate::services::multimodal_asset_base_dir(
                        &early_doc_id,
                        data.multimodal_process_options.as_deref(),
                    );
                    let mm_outcome =
                        crate::services::run_multimodal_analyze_stage_outcome_with_cancel(
                            stored_markdown,
                            data.multimodal_process_options.as_deref(),
                            &filename,
                            self.workspace_service.as_ref(),
                            data.workspace_id,
                            Arc::clone(&self.llm_provider),
                            mm_asset_base.as_deref(),
                            Some(&early_doc_id),
                            Some(Arc::clone(&self.kv_storage)),
                            None,
                            Some(cancel_token.clone()),
                        )
                        .await;
                    if crate::services::multimodal::should_abort_multimodal_hard_error(
                        mm_outcome.hard_error.as_deref(),
                    ) {
                        return Err(edgequake_tasks::TaskError::Processing(format!(
                            "Multimodal analyze failed: {}",
                            mm_outcome.hard_error.as_deref().unwrap_or("unknown")
                        )));
                    }
                    let stored_markdown = mm_outcome.markdown;

                    let doc_content_key = edgequake_storage::kv_keys::doc_content(&early_doc_id);
                    let doc_content = json!({ "content": stored_markdown.clone() });
                    self.kv_storage
                        .upsert(&[(doc_content_key, doc_content)])
                        .await
                        .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?;

                    // Re-affirm Completed: process start sets Processing, but convert
                    // barrier SSOT must remain Completed when we skip reconvert (P2).
                    let extraction_method_str =
                        stored_extraction_method.as_ref().map(|m| m.as_str());
                    let update_req = UpdatePdfProcessingRequest {
                        pdf_id: data.pdf_id,
                        processing_status: PdfProcessingStatus::Completed,
                        markdown_content: Some(stored_markdown.clone()),
                        extraction_method: stored_extraction_method,
                        extraction_errors: None,
                        document_id: None,
                        vision_model: stored_vision_model.clone(),
                    };
                    pdf_storage
                        .update_pdf_processing(update_req)
                        .await
                        .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?;

                    return self
                        .finish_pdf_convert_and_enqueue_ingest(
                            task,
                            &data,
                            pdf_storage.as_ref(),
                            &early_doc_id,
                            &filename,
                            stored_markdown,
                            page_count_opt,
                            file_size_bytes,
                            &sha256_checksum,
                            stored_vision_model,
                            extraction_method_str,
                            None,
                            &cancel_token,
                        )
                        .await;
                }
            }
        }

        // == Progress: starting conversion (this can take 5-10+ minutes) ==
        self.bump_task_progress(task, "pdf_converting".to_string(), 2, 10)
            .await;

        // ── CANCELLATION GATE: before vision extraction (most expensive PDF stage) ──
        self.check_cancelled(&cancel_token, "pre-vision-extraction", &early_doc_id)
            .await?;

        let backend = data.pdf_parser_backend;
        // Pass 0 when unknown — vision_outer_timeout_secs applies the
        // deterministic UNKNOWN_PAGE_COUNT_VISION_BUDGET_ASSUMPTION (50).
        // Do NOT coerce to 1: that under-budgets large PDFs with missing metadata.
        let page_count = page_count_opt.unwrap_or(0) as usize;
        let mut extraction_method = match backend {
            edgequake_pdf::PdfParserBackend::Vision => ExtractionMethod::Vision,
            edgequake_pdf::PdfParserBackend::EdgeParse => ExtractionMethod::EdgeParse,
        };

        let default_vision_model = || {
            use crate::vision_env::default_vision_model_for_provider;
            data.vision_model
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| default_vision_model_for_provider(&data.vision_provider))
        };

        let mut vision_model = match backend {
            edgequake_pdf::PdfParserBackend::Vision => Some(default_vision_model()),
            edgequake_pdf::PdfParserBackend::EdgeParse => None,
        };
        let mut fallback_warning: Option<String> = None;

        // SPEC-038: born-digital PDFs default to Vision but often have embedded text.
        // Try EdgeParse first when routing is automatic to avoid O(pages × LLM) conversion.
        let mut precomputed_markdown: Option<String> = None;
        if crate::services::should_try_edgeparse_before_vision(
            backend,
            data.pdf_parser_backend_explicit,
        ) {
            if let Some(markdown) =
                crate::services::try_edgeparse_fast_path(&pdf_data, page_count, &filename).await
            {
                info!(
                    pdf_id = %data.pdf_id,
                    page_count = page_count,
                    markdown_len = markdown.len(),
                    "SPEC-038: auto-routed born-digital PDF to EdgeParse (skipped Vision OCR)"
                );
                let _ = self
                    .update_document_status(
                        &early_doc_id,
                        "processing",
                        Some("Born-digital text detected — using fast parse (EdgeParse)"),
                    )
                    .await;
                extraction_method = ExtractionMethod::EdgeParse;
                vision_model = None;
                precomputed_markdown = Some(markdown);
            }
        }

        let converter = if precomputed_markdown.is_some() {
            edgequake_pdf::create_pdf_converter(edgequake_pdf::PdfParserBackend::EdgeParse)
        } else {
            match backend {
                edgequake_pdf::PdfParserBackend::Vision => {
                    if !data.enable_vision {
                        let error = edgequake_tasks::TaskError::UnsupportedOperation(
                            "Vision PDF extraction requires enable_vision=true.".to_string(),
                        );
                        let message =
                            build_edgeparse_fallback_message(&data.vision_provider, &error);
                        warn!(
                            pdf_id = %data.pdf_id,
                            "Vision disabled for requested vision extraction; falling back to EdgeParse"
                        );
                        let _ = self
                            .update_document_status(&early_doc_id, "processing", Some(&message))
                            .await;
                        fallback_warning = Some(message);
                        extraction_method = ExtractionMethod::EdgeParse;
                        vision_model = None;
                        edgequake_pdf::create_pdf_converter(
                            edgequake_pdf::PdfParserBackend::EdgeParse,
                        )
                    } else {
                        #[cfg(feature = "vision")]
                        {
                            use crate::safety_limits::check_vision_provider_available;

                            match check_vision_provider_available(
                                &data.vision_provider,
                                vision_model.as_deref().unwrap_or_default(),
                            ) {
                                Ok(()) => edgequake_pdf::create_pdf_converter(backend),
                                Err(e) => {
                                    let error = edgequake_tasks::TaskError::Processing(format!(
                                        "Failed to create vision provider '{}': {e}",
                                        data.vision_provider
                                    ));
                                    if !vision_fallback_allowed(
                                        backend,
                                        &error,
                                        data.pdf_parser_backend_explicit,
                                    ) {
                                        return Err(error);
                                    }
                                    let message = build_edgeparse_fallback_message(
                                        &data.vision_provider,
                                        &error,
                                    );
                                    warn!(
                                        pdf_id = %data.pdf_id,
                                        error = %error,
                                        "Vision provider setup failed; falling back to EdgeParse"
                                    );
                                    let _ = self
                                        .update_document_status(
                                            &early_doc_id,
                                            "processing",
                                            Some(&message),
                                        )
                                        .await;
                                    fallback_warning = Some(message);
                                    extraction_method = ExtractionMethod::EdgeParse;
                                    vision_model = None;
                                    edgequake_pdf::create_pdf_converter(
                                        edgequake_pdf::PdfParserBackend::EdgeParse,
                                    )
                                }
                            }
                        }
                        #[cfg(not(feature = "vision"))]
                        {
                            let error = edgequake_tasks::TaskError::UnsupportedOperation(
                                "Vision extraction requires the 'vision' feature flag".to_string(),
                            );
                            let message =
                                build_edgeparse_fallback_message(&data.vision_provider, &error);
                            warn!(
                                pdf_id = %data.pdf_id,
                                "Vision feature is unavailable; falling back to EdgeParse"
                            );
                            let _ = self
                                .update_document_status(&early_doc_id, "processing", Some(&message))
                                .await;
                            fallback_warning = Some(message);
                            extraction_method = ExtractionMethod::EdgeParse;
                            vision_model = None;
                            edgequake_pdf::create_pdf_converter(
                                edgequake_pdf::PdfParserBackend::EdgeParse,
                            )
                        }
                    }
                }
                edgequake_pdf::PdfParserBackend::EdgeParse => {
                    edgequake_pdf::create_pdf_converter(backend)
                }
            }
        };

        // WHY: Local providers (Ollama, LM Studio) run on a single GPU that is
        // memory-bound. High concurrency causes VRAM thrashing and *increases*
        // total conversion time. Cap local concurrency at 2. Cloud providers
        // retain the original scale-with-page-count formula.
        // See ADR-04-003 in mission/04-heavy-pdf.md.
        let (safe_concurrency, safe_dpi) =
            compute_safe_pdf_resource_profile(page_count, file_size_bytes, &data.vision_provider);
        let concurrency = std::env::var("EDGEQUAKE_PDF_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(safe_concurrency)
            .max(1)
            .min(safe_concurrency);
        let dpi = std::env::var("EDGEQUAKE_PDF_DPI")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(safe_dpi)
            .clamp(96, safe_dpi.max(96));
        let checkpoint_dir =
            crate::services::durable_vision_checkpoint_dir(&data.pdf_id.to_string());
        let page_drawing_assets = match extraction_method {
            ExtractionMethod::Vision => {
                Some(crate::services::page_drawing_assets_config_for_vision(
                    &early_doc_id,
                    data.multimodal_process_options.as_deref(),
                ))
            }
            _ => crate::services::page_drawing_assets_config(
                &early_doc_id,
                data.multimodal_process_options.as_deref(),
            ),
        };
        // Stall-watchdog heartbeat: resets on every page/status progress signal so
        // slow-but-progressing Vision conversions are not killed by a fixed wall clock.
        let vision_heartbeat = crate::services::VisionProgressHeartbeat::new();
        let conversion_config = edgequake_pdf::PdfConversionConfig {
            page_count_hint: page_count_opt.map(|count| count as usize),
            table_method: None,
            filename: Some(filename.clone()),
            page_drawing_assets,
            vision: vision_model.clone().map(|model| {
                let hb = Arc::clone(&vision_heartbeat);
                let status_hook: edgequake_pdf::VisionStatusHook = {
                    let cb = progress_callback.clone();
                    let hb_status = Arc::clone(&hb);
                    Arc::new(move |message: &str, progress: f64| {
                        hb_status.touch();
                        cb.report_converting_status(message, progress);
                    })
                };
                let wrapped_progress = crate::services::HeartbeatProgressCallback::new(
                    progress_callback.clone()
                        as Arc<dyn edgequake_pdf2md::ConversionProgressCallback>,
                    Arc::clone(&hb),
                );
                edgequake_pdf::VisionConversionConfig {
                    provider_name: Some(data.vision_provider.clone()),
                    model: Some(model),
                    concurrency: Some(concurrency),
                    dpi: Some(dpi),
                    checkpoint_dir: Some(checkpoint_dir),
                    // Retries must resume from durable checkpoints so pages accumulate.
                    // Fresh restart (`restart_from_scratch`) still clears via no_resume.
                    no_resume: should_cleanup_existing_content,
                    progress_callback: Some(wrapped_progress),
                    status_hook: Some(status_hook),
                }
            }),
        };

        let edgeparse_config = edgequake_pdf::PdfConversionConfig {
            vision: None,
            ..conversion_config.clone()
        };

        let markdown = if let Some(md) = precomputed_markdown {
            md
        } else {
            match extraction_method {
                ExtractionMethod::Vision => {
                    // Absolute budget (backstop). Primary hang detection is the
                    // progress/stall watchdog — slow-but-progressing docs complete.
                    // EDGEQUAKE_VISION_TIMEOUT_SECS overrides the computed budget.
                    let base_timeout_secs: u64 = std::env::var("EDGEQUAKE_VISION_TIMEOUT_SECS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let vision_budget_secs = if base_timeout_secs > 0 {
                        base_timeout_secs
                    } else {
                        use crate::safety_limits::vision_outer_timeout_secs;
                        vision_outer_timeout_secs(&data.vision_provider, page_count)
                    };
                    // Absolute backstop: env override OR 24h — never kill a progressing doc
                    // solely because the soft page budget elapsed.
                    use crate::safety_limits::VISION_MAX_OUTER_TIMEOUT_SECS;
                    let absolute_secs = if base_timeout_secs > 0 {
                        base_timeout_secs
                    } else {
                        VISION_MAX_OUTER_TIMEOUT_SECS
                    };
                    let stall_secs = crate::services::vision_stall_timeout_secs();

                    let _vision_job_permit = if let Some(semaphore) = &self.pdf_vision {
                        info!(
                            pdf_id = %data.pdf_id,
                            max_concurrent_jobs = semaphore.max_concurrent(),
                            "Waiting for vision PDF admission slot"
                        );
                        Some(semaphore.acquire_owned().await.ok_or_else(|| {
                            edgequake_tasks::TaskError::Processing(
                                "Vision PDF admission semaphore closed".to_string(),
                            )
                        })?)
                    } else {
                        None
                    };

                    info!(
                        pdf_id = %data.pdf_id,
                        vision_provider = %data.vision_provider,
                        vision_model = %vision_model.clone().unwrap_or_default(),
                        page_count = page_count,
                        concurrency = concurrency,
                        dpi = dpi,
                        budget_secs = vision_budget_secs,
                        stall_secs = stall_secs,
                        absolute_secs = absolute_secs,
                        checkpoint_dir = %crate::services::durable_vision_checkpoint_dir(&data.pdf_id.to_string()),
                        "Starting Vision PDF conversion (stall watchdog)"
                    );

                    // Cooperative cancel + stall watchdog (replaces fixed wall-clock timeout).
                    let convert_result = tokio::select! {
                        biased;
                        _ = cancel_token.cancelled() => {
                            let _ = pdf_storage
                                .update_pdf_status(
                                    &data.pdf_id,
                                    crate::services::pdf_status_for_cancel(),
                                )
                                .await;
                            return Err(edgequake_tasks::TaskError::Cancelled(
                                "Cancelled during vision PDF conversion".to_string(),
                            ));
                        }
                        result = crate::services::run_with_vision_stall_watchdog(
                            converter.convert(&pdf_data, &conversion_config),
                            Arc::clone(&vision_heartbeat),
                            stall_secs,
                            absolute_secs,
                        ) => result,
                    };

                    match convert_result {
                        Ok(Ok(markdown)) => markdown,
                        Ok(Err(e)) => {
                            let error = edgequake_tasks::TaskError::Processing(format!(
                                "PDF conversion failed: {e}"
                            ));
                            if !vision_fallback_allowed(
                                backend,
                                &error,
                                data.pdf_parser_backend_explicit,
                            ) {
                                return Err(error);
                            }

                            let message =
                                build_edgeparse_fallback_message(&data.vision_provider, &error);
                            warn!(
                                pdf_id = %data.pdf_id,
                                error = %error,
                                "Vision conversion failed; falling back to EdgeParse"
                            );
                            let _ = self
                                .update_document_status(&early_doc_id, "processing", Some(&message))
                                .await;
                            fallback_warning = Some(message);
                            extraction_method = ExtractionMethod::EdgeParse;
                            vision_model = None;

                            edgequake_pdf::create_pdf_converter(
                                edgequake_pdf::PdfParserBackend::EdgeParse,
                            )
                            .convert(&pdf_data, &edgeparse_config)
                            .await
                            .map_err(|e| {
                                edgequake_tasks::TaskError::Processing(format!(
                                    "PDF conversion failed after EdgeParse fallback: {e}"
                                ))
                            })?
                        }
                        Err(abort) => {
                            let made_progress = vision_heartbeat.made_progress();
                            let raw = abort.as_timeout_message(
                                &data.pdf_id.to_string(),
                                &data.vision_provider,
                            );
                            let annotated =
                                crate::services::annotate_timeout_progress(raw, made_progress);
                            let error = edgequake_tasks::TaskError::Timeout(annotated);
                            if !vision_fallback_allowed(
                                backend,
                                &error,
                                data.pdf_parser_backend_explicit,
                            ) {
                                return Err(error);
                            }

                            let message =
                                build_edgeparse_fallback_message(&data.vision_provider, &error);
                            warn!(
                                pdf_id = %data.pdf_id,
                                made_progress = made_progress,
                                pages_completed = vision_heartbeat.pages_completed(),
                                "Vision extraction stalled/timed out; falling back to EdgeParse"
                            );
                            let _ = self
                                .update_document_status(&early_doc_id, "processing", Some(&message))
                                .await;
                            fallback_warning = Some(message);
                            extraction_method = ExtractionMethod::EdgeParse;
                            vision_model = None;

                            edgequake_pdf::create_pdf_converter(
                                edgequake_pdf::PdfParserBackend::EdgeParse,
                            )
                            .convert(&pdf_data, &edgeparse_config)
                            .await
                            .map_err(|e| {
                                edgequake_tasks::TaskError::Processing(format!(
                                    "PDF conversion failed after EdgeParse fallback: {e}"
                                ))
                            })?
                        }
                    }
                }
                ExtractionMethod::EdgeParse | ExtractionMethod::Text | ExtractionMethod::Hybrid => {
                    info!(
                        pdf_id = %data.pdf_id,
                        page_count = page_count,
                        "Starting EdgeParse PDF conversion"
                    );
                    converter
                        .convert(&pdf_data, &edgeparse_config)
                        .await
                        .map_err(|e| {
                            edgequake_tasks::TaskError::Processing(format!(
                                "PDF conversion failed: {e}"
                            ))
                        })?
                }
            }
        };

        let markdown = strip_nul_bytes(markdown);
        drop(pdf_data);

        // SPEC-047: durable mm-assets in DB (lineage: document_id + page_num + asset_id).
        // Materialize from DB when resume skipped vision writes (disk cache miss).
        #[cfg(feature = "postgres")]
        {
            let assets_root = crate::services::document_mm_assets_root(&early_doc_id);
            // Persist whenever vision wrote page PNGs (viewer) and/or `i` analyze ran.
            let should_persist_mm = conversion_config.page_drawing_assets.is_some()
                || crate::services::multimodal_images_requested(
                    data.multimodal_process_options.as_deref(),
                );
            if should_persist_mm {
                progress_callback.report_converting_status("Saving page images to storage…", 0.97);
                if let Some(ref mm_store) = self.mm_asset_storage {
                    if let Err(e) = crate::services::materialize_mm_assets_to_dir(
                        mm_store.as_ref(),
                        uuid::Uuid::parse_str(&early_doc_id).unwrap_or(uuid::Uuid::nil()),
                        data.workspace_id,
                        &assets_root,
                    )
                    .await
                    {
                        tracing::warn!(
                            document_id = %early_doc_id,
                            error = %e,
                            "Failed to materialize mm-assets from DB"
                        );
                    }
                }
                match crate::services::persist_mm_assets_with_storage(
                    self.mm_asset_storage.as_ref(),
                    self.kv_storage.as_ref(),
                    &early_doc_id,
                    data.workspace_id,
                    &assets_root,
                )
                .await
                {
                    Ok(n) => {
                        if n > 0 {
                            tracing::info!(
                                document_id = %early_doc_id,
                                count = n,
                                "Persisted mm-assets after vision ingest"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            document_id = %early_doc_id,
                            error = %e,
                            "Failed to persist mm-assets to database after vision ingest"
                        );
                    }
                }
            }
        }

        let mm_asset_base = crate::services::multimodal_asset_base_dir(
            &early_doc_id,
            data.multimodal_process_options.as_deref(),
        );
        let converting_substep: Option<crate::services::ConvertingSubstepReporter> = {
            let cb = progress_callback.clone();
            Some(Arc::new(move |message, progress| {
                cb.report_converting_status(message, progress);
            }))
        };
        let markdown = {
            if crate::services::multimodal_images_requested(
                data.multimodal_process_options.as_deref(),
            ) {
                let figure_total =
                    edgequake_pdf::inline_images::scan_inline_image_refs(&markdown).len();
                if figure_total > 0 {
                    let profile = crate::services::LocalMmProfile::resolve_from_env();
                    let analyze_cap = profile.figures_to_analyze(figure_total);
                    let start_msg = if profile.is_local && profile.classify_only {
                        crate::services::vision_figure_analyze_message_local(
                            0,
                            analyze_cap,
                            figure_total,
                        )
                    } else {
                        crate::services::vision_figure_analyze_message(0, analyze_cap)
                    };
                    progress_callback.report_converting_status(
                        start_msg,
                        crate::services::vision_figure_analyze_progress_01(0, analyze_cap),
                    );
                }
            }
            let mm_outcome = crate::services::run_multimodal_analyze_stage_outcome_with_cancel(
                markdown,
                data.multimodal_process_options.as_deref(),
                &filename,
                self.workspace_service.as_ref(),
                data.workspace_id,
                Arc::clone(&self.llm_provider),
                mm_asset_base.as_deref(),
                Some(&early_doc_id),
                Some(Arc::clone(&self.kv_storage)),
                converting_substep,
                Some(cancel_token.clone()),
            )
            .await;
            if crate::services::multimodal::should_abort_multimodal_hard_error(
                mm_outcome.hard_error.as_deref(),
            ) {
                return Err(edgequake_tasks::TaskError::Processing(format!(
                    "Multimodal analyze failed: {}",
                    mm_outcome.hard_error.as_deref().unwrap_or("unknown")
                )));
            }
            mm_outcome.markdown
        };

        // Post-OCR converting work finished — advance PdfConversion phase for track ETA.
        progress_callback.report_converting_status(
            "PDF conversion finished — starting knowledge-graph pipeline…",
            1.0,
        );
        progress_callback.complete_pdf_conversion_phase();

        let mut extraction_errors = if extraction_method == ExtractionMethod::EdgeParse {
            let avg_chars_per_page = markdown.len() / page_count.max(1);
            if avg_chars_per_page < 50 {
                warn!(
                    pdf_id = %data.pdf_id,
                    avg_chars_per_page,
                    "Low text content from EdgeParse — PDF may be scanned/image-only"
                );
                Some(json!({
                    "low_content_warning": {
                        "avg_chars_per_page": avg_chars_per_page,
                        "message": "Low text content detected. This PDF may be image-only. Consider using Vision extraction."
                    }
                }))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(message) = fallback_warning.take() {
            merge_extraction_notice(&mut extraction_errors, "vision_fallback", message);
        }
        let extraction_warning = extraction_errors
            .as_ref()
            .and_then(|value| {
                value
                    .get("vision_fallback")
                    .or_else(|| value.get("low_content_warning"))
            })
            .and_then(|value| value.get("message"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        info!(
            pdf_id = %data.pdf_id,
            markdown_len = markdown.len(),
            extraction_method = ?extraction_method,
            "Extracted markdown from PDF"
        );

        // == Progress: conversion done, storing markdown ==
        self.bump_task_progress(task, "storing_markdown".to_string(), 3, 45)
            .await;

        // 5. Store markdown in pdf_documents (convert barrier SSOT — SPEC-057 P2)
        let update_req = UpdatePdfProcessingRequest {
            pdf_id: data.pdf_id,
            processing_status: PdfProcessingStatus::Completed,
            markdown_content: Some(markdown.clone()),
            extraction_method: Some(extraction_method),
            extraction_errors: extraction_errors.clone(),
            document_id: None, // Linked when enqueueing ingest
            vision_model: vision_model.clone(),
        };

        pdf_storage
            .update_pdf_processing(update_req.clone())
            .await
            .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?;

        // Mirror markdown into KV so GET /documents/{id} and the WebUI content pane
        // can render without a second round-trip (pdf_documents remains canonical for PDF APIs).
        let doc_content_key = edgequake_storage::kv_keys::doc_content(&early_doc_id);
        let doc_content = json!({ "content": markdown.clone() });
        self.kv_storage
            .upsert(&[(doc_content_key, doc_content)])
            .await
            .map_err(|e| edgequake_tasks::TaskError::Storage(e.to_string()))?;

        // 6. End convert task; enqueue TaskType::Insert for KG ingest (SPEC-057 P2).
        let extraction_method_str = update_req.extraction_method.as_ref().map(|m| m.as_str());
        self.finish_pdf_convert_and_enqueue_ingest(
            task,
            &data,
            pdf_storage.as_ref(),
            &early_doc_id,
            &filename,
            markdown,
            page_count_opt,
            file_size_bytes,
            &sha256_checksum,
            vision_model,
            extraction_method_str,
            extraction_warning,
            &cancel_token,
        )
        .await
    }

    #[cfg(not(feature = "postgres"))]
    pub(super) async fn process_pdf_processing(
        &self,
        _task: &mut Task,
        data: edgequake_tasks::PdfProcessingData,
        _cancel_token: CancellationToken,
    ) -> TaskResult<serde_json::Value> {
        warn!(
            pdf_id = %data.pdf_id,
            "PDF processing not available (postgres feature disabled)"
        );
        Err(edgequake_tasks::TaskError::UnsupportedOperation(
            "PDF processing requires postgres feature".to_string(),
        ))
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use super::*;
    use edgequake_pdf::{PdfParserBackend, VisionFailureKind};
    use edgequake_storage::{
        CreatePdfRequest, ExtractionMethod, MemoryPdfStorage, PdfDocumentStorage,
        PdfProcessingStatus, UpdatePdfProcessingRequest,
    };
    use edgequake_tasks::{
        memory::MemoryTaskStorage, queue::ChannelTaskQueue, NoopTaskNotifier, TaskDeliveryMode,
        TaskError, TaskType,
    };
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;

    #[test]
    fn vision_timeouts_trigger_edgeparse_fallback_when_implicit() {
        let error = TaskError::Timeout(
            "Vision extraction timed out after 480s for PDF abc. Provider 'ollama' may be unresponsive."
                .to_string(),
        );

        assert!(vision_fallback_allowed(
            PdfParserBackend::Vision,
            &error,
            false
        ));
        assert!(
            !vision_fallback_allowed(PdfParserBackend::Vision, &error, true),
            "explicit workspace/upload Vision must fail closed"
        );
    }

    #[test]
    fn edgeparse_requests_do_not_self_fallback() {
        let error = TaskError::Timeout("EdgeParse timed out".to_string());

        assert!(!edgequake_pdf::should_fallback_to_edgeparse(
            PdfParserBackend::EdgeParse,
            VisionFailureKind::Timeout,
            false,
        ));
        assert!(!vision_fallback_allowed(
            PdfParserBackend::EdgeParse,
            &error,
            false
        ));
    }

    #[test]
    fn existing_document_prefers_resume_by_default() {
        assert!(should_resume_pdf_conversion(true, false));
        assert!(!should_restart_pdf_conversion(true, false));
    }

    #[test]
    fn explicit_restart_starts_clean() {
        assert!(!should_resume_pdf_conversion(true, true));
        assert!(should_restart_pdf_conversion(true, true));
    }

    #[test]
    #[serial_test::serial]
    fn large_local_pdfs_are_throttled_aggressively() {
        // Operator shells / `make dev` may export EDGEQUAKE_PDF_CONCURRENCY=1.
        let prev = std::env::var("EDGEQUAKE_PDF_CONCURRENCY").ok();
        // SAFETY: serialised; restore below.
        unsafe {
            std::env::remove_var("EDGEQUAKE_PDF_CONCURRENCY");
        }
        let (concurrency, dpi) = compute_safe_pdf_resource_profile(250, 60 * 1024 * 1024, "ollama");
        assert_eq!(concurrency, 1);
        assert_eq!(dpi, 96);
        unsafe {
            match prev {
                Some(v) => std::env::set_var("EDGEQUAKE_PDF_CONCURRENCY", v),
                None => std::env::remove_var("EDGEQUAKE_PDF_CONCURRENCY"),
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn small_cloud_pdfs_keep_reasonable_parallelism() {
        // Operator shells / `make dev` may export EDGEQUAKE_PDF_CONCURRENCY=1 for
        // local Ollama; this contract asserts the uncapped cloud default.
        let prev = std::env::var("EDGEQUAKE_PDF_CONCURRENCY").ok();
        // SAFETY: serialised; restore below.
        unsafe {
            std::env::remove_var("EDGEQUAKE_PDF_CONCURRENCY");
        }
        let (concurrency, dpi) = compute_safe_pdf_resource_profile(40, 4 * 1024 * 1024, "openai");
        assert_eq!(concurrency, 2);
        assert_eq!(dpi, 150);
        unsafe {
            match prev {
                Some(v) => std::env::set_var("EDGEQUAKE_PDF_CONCURRENCY", v),
                None => std::env::remove_var("EDGEQUAKE_PDF_CONCURRENCY"),
            }
        }
    }

    // ── Resume-shortcut logic tests ──────────────────────────────────────────

    /// should_resume_pdf_conversion is the gate for the resume shortcut.
    /// Without an existing document there is nothing to resume.
    #[test]
    fn new_document_never_resumes() {
        assert!(!should_resume_pdf_conversion(false, false));
        assert!(!should_resume_pdf_conversion(false, true));
    }

    /// When an existing document is present AND restart is not requested, the
    /// shortcut should be taken so we never re-run PDF→Markdown conversion.
    #[test]
    fn retry_without_restart_flag_takes_shortcut() {
        // has_existing_document=true, restart_from_scratch=false
        assert!(should_resume_pdf_conversion(true, false));
        // No cleanup of old content should happen
        assert!(!should_restart_pdf_conversion(true, false));
    }

    /// An explicit "restart from scratch" request overrides the shortcut.
    #[test]
    fn explicit_restart_bypasses_resume_shortcut() {
        assert!(!should_resume_pdf_conversion(true, true));
        assert!(should_restart_pdf_conversion(true, true));
    }

    /// ReprocessMode::Full is the single source of truth that drives the
    /// restart flag, and that flag in turn drives the resume gate. This pins
    /// the contract end-to-end at the logic level so a regression in either
    /// link surfaces as a test failure.
    #[test]
    fn full_reprocess_mode_forces_fresh_conversion() {
        let restart = edgequake_tasks::ReprocessMode::Full.restart_from_scratch();
        assert!(restart, "Full mode must request a fresh conversion");
        // With an existing document + restart=true the shortcut is bypassed
        // and the conversion path is selected.
        assert!(!should_resume_pdf_conversion(true, restart));
        assert!(should_restart_pdf_conversion(true, restart));
    }

    #[test]
    fn entities_reprocess_mode_keeps_resume_shortcut() {
        let restart = edgequake_tasks::ReprocessMode::EntitiesOnly.restart_from_scratch();
        assert!(!restart, "Entities mode must reuse cached markdown");
        assert!(should_resume_pdf_conversion(true, restart));
        assert!(!should_restart_pdf_conversion(true, restart));
    }

    /// SPEC-057 P2 e2e: resume after convert barrier enqueues Insert (no inline KG).
    #[tokio::test]
    async fn resume_convert_barrier_enqueues_insert_task() {
        use edgequake_pipeline::Pipeline;
        use edgequake_storage::{
            MemoryGraphStorage, MemoryKVStorage, MemoryVectorStorage, MemoryWorkspaceVectorRegistry,
        };
        use edgequake_tasks::{PipelineState, Task, TaskProcessor};

        let pdf_storage = Arc::new(MemoryPdfStorage::new());
        let task_storage: Arc<dyn edgequake_tasks::TaskStorage> =
            Arc::new(MemoryTaskStorage::new());
        let task_queue: Arc<dyn edgequake_tasks::TaskQueue> = Arc::new(ChannelTaskQueue::new(16));
        let kv: Arc<dyn edgequake_storage::traits::KVStorage> =
            Arc::new(MemoryKVStorage::new("p2-resume-enqueue"));
        let vector: Arc<dyn edgequake_storage::traits::VectorStorage> =
            Arc::new(MemoryVectorStorage::new("p2-resume-enqueue", 1536));
        let vector_registry: Arc<dyn edgequake_storage::traits::WorkspaceVectorRegistry> =
            Arc::new(MemoryWorkspaceVectorRegistry::new(Arc::clone(&vector)));
        let graph: Arc<dyn edgequake_storage::traits::GraphStorage> =
            Arc::new(MemoryGraphStorage::new("p2-resume-enqueue"));

        let tenant_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let pdf_bytes = b"%PDF-1.4\n1 0 obj<<>>endobj\ntrailer<<>>\n%%EOF\n";
        let pdf_id = pdf_storage
            .create_pdf(CreatePdfRequest {
                workspace_id,
                filename: "p2-resume.pdf".to_string(),
                content_type: "application/pdf".to_string(),
                file_size_bytes: pdf_bytes.len() as i64,
                sha256_checksum: format!("p2-resume-{}", Uuid::new_v4()),
                page_count: Some(1),
                pdf_data: pdf_bytes.to_vec(),
                vision_model: None,
            })
            .await
            .unwrap();

        let markdown = "# Barrier markdown from prior convert\n";
        pdf_storage
            .update_pdf_processing(UpdatePdfProcessingRequest {
                pdf_id,
                processing_status: PdfProcessingStatus::Completed,
                markdown_content: Some(markdown.to_string()),
                extraction_method: Some(ExtractionMethod::EdgeParse),
                extraction_errors: None,
                document_id: None,
                vision_model: None,
            })
            .await
            .unwrap();

        let doc_id = format!("doc-{}", pdf_id);
        let meta_key = edgequake_storage::kv_keys::doc_metadata(&doc_id);
        kv.upsert(&[(
            meta_key,
            serde_json::json!({
                "id": doc_id,
                "status": "processing",
                "tenant_id": tenant_id.to_string(),
                "workspace_id": workspace_id.to_string(),
            }),
        )])
        .await
        .unwrap();

        let processor =
            DocumentTaskProcessor::new(
                Arc::new(Pipeline::default_pipeline()),
                Arc::new(edgequake_llm::MockProvider::new()),
                Arc::clone(&kv),
                vector,
                vector_registry,
                graph,
                PipelineState::new(),
            )
            .with_pdf_storage(
                Arc::clone(&pdf_storage) as Arc<dyn edgequake_storage::PdfDocumentStorage>
            )
            .with_task_enqueue(
                Arc::clone(&task_storage) as edgequake_tasks::SharedTaskStorage,
                Arc::clone(&task_queue) as edgequake_tasks::SharedTaskQueue,
                Arc::new(NoopTaskNotifier) as edgequake_tasks::SharedTaskNotifier,
                TaskDeliveryMode::Local,
            );

        let data = edgequake_tasks::PdfProcessingData {
            pdf_id,
            tenant_id,
            workspace_id,
            enable_vision: false,
            vision_provider: "mock".to_string(),
            vision_model: None,
            existing_document_id: Some(doc_id.clone()),
            pdf_parser_backend: PdfParserBackend::EdgeParse,
            pdf_parser_backend_explicit: true,
            restart_from_scratch: false,
            reprocess_mode: None,
            multimodal_process_options: None,
        };
        let mut task = Task::new(
            tenant_id,
            workspace_id,
            TaskType::PdfProcessing,
            serde_json::to_value(&data).unwrap(),
        );

        let result = processor
            .process(&mut task, CancellationToken::new())
            .await
            .expect("convert resume must succeed");
        assert_eq!(result["phase"], "convert_complete");
        assert_eq!(result["status"], "converted");
        let ingest_track = result["ingest_track_id"].as_str().expect("ingest_track_id");

        let ingest = task_storage
            .get_task(ingest_track)
            .await
            .unwrap()
            .expect("Insert task row");
        assert_eq!(ingest.task_type, TaskType::Insert);
        assert_eq!(ingest.pdf_id(), Some(pdf_id));
        let timeout = ingest
            .metadata
            .as_ref()
            .and_then(|m| m.get("processing_timeout_secs"))
            .and_then(|v| v.as_u64());
        assert!(
            timeout.is_some(),
            "Insert must carry ingest timeout metadata"
        );

        let pdf = pdf_storage.get_pdf(&pdf_id).await.unwrap().unwrap();
        assert_eq!(pdf.processing_status, PdfProcessingStatus::Completed);
        assert_eq!(pdf.markdown_content.as_deref(), Some(markdown));
    }
}
