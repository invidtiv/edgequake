//! Worker handler for `TaskType::Deletion`.

use edgequake_tasks::{DeletionTaskData, Task, TaskResult};
use tokio_util::sync::CancellationToken;

use crate::middleware::TenantContext;
use crate::services::{perform_document_deletion, reset_deleting_status};

use super::DocumentTaskProcessor;

impl DocumentTaskProcessor {
    pub(super) async fn process_document_deletion(
        &self,
        task: &mut Task,
        data: DeletionTaskData,
        cancel_token: CancellationToken,
    ) -> TaskResult<serde_json::Value> {
        let Some(state) = self.app_state.as_ref() else {
            return Err(edgequake_tasks::TaskError::Processing(
                "Document deletion requires AppState on DocumentTaskProcessor (with_app_state)"
                    .to_string(),
            ));
        };

        if cancel_token.is_cancelled() {
            reset_deleting_status(
                state,
                &data.document_id,
                &data.key_prefix,
                "Deletion cancelled",
                Some(&data.deletion_track_id),
            )
            .await;
            return Err(edgequake_tasks::TaskError::Cancelled(
                "Deletion cancelled".to_string(),
            ));
        }

        let tenant_ctx = TenantContext {
            tenant_id: Some(data.tenant_id.clone()),
            workspace_id: Some(data.workspace_id.clone()),
            user_id: None,
        };

        match perform_document_deletion(state, &data, &tenant_ctx).await {
            Ok(result) => {
                self.bump_task_progress(task, "deletion_complete".to_string(), 1, 100)
                    .await;
                Ok(serde_json::json!({
                    "document_id": data.document_id,
                    "chunks_deleted": result.chunks_deleted,
                    "entities_removed": result.entities_removed,
                    "relationships_removed": result.relationships_removed,
                    "embeddings_deleted": result.embeddings_deleted,
                    "partial_failure": result.partial_failure,
                }))
            }
            Err(e) => {
                let reason = format!("Deletion failed: {e}");
                reset_deleting_status(
                    state,
                    &data.document_id,
                    &data.key_prefix,
                    &reason,
                    Some(&data.deletion_track_id),
                )
                .await;
                Err(edgequake_tasks::TaskError::Processing(reason))
            }
        }
    }
}
