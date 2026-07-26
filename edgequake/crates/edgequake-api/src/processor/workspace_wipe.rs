//! Worker handler for `TaskType::WorkspaceWipe`.

use edgequake_tasks::{Task, TaskResult, WorkspaceWipeTaskData};
use tokio_util::sync::CancellationToken;

use crate::services::{broadcast_wipe_failed, run_workspace_wipe_phases};

use super::DocumentTaskProcessor;

impl DocumentTaskProcessor {
    pub(super) async fn process_workspace_wipe(
        &self,
        task: &mut Task,
        data: WorkspaceWipeTaskData,
        cancel_token: CancellationToken,
    ) -> TaskResult<serde_json::Value> {
        let Some(state) = self.app_state.as_ref() else {
            return Err(edgequake_tasks::TaskError::Processing(
                "Workspace wipe requires AppState on DocumentTaskProcessor (with_app_state)"
                    .to_string(),
            ));
        };

        if cancel_token.is_cancelled() {
            broadcast_wipe_failed(state, &data, "Workspace wipe cancelled");
            return Err(edgequake_tasks::TaskError::Cancelled(
                "Workspace wipe cancelled".to_string(),
            ));
        }

        match run_workspace_wipe_phases(state, task, data).await {
            Ok(final_data) => {
                self.bump_task_progress(task, "workspace_wipe_complete".to_string(), 1, 100)
                    .await;
                Ok(serde_json::json!({
                    "wipe_track_id": final_data.wipe_track_id,
                    "deleted_count": final_data.deleted_count,
                    "total_chunks_deleted": final_data.total_chunks_deleted,
                    "total_entities_removed": final_data.total_entities_removed,
                    "total_relationships_removed": final_data.total_relationships_removed,
                    "total_pdfs_deleted": final_data.total_pdfs_deleted,
                    "phase": "completed",
                }))
            }
            Err(e) => {
                let reason = format!("Workspace wipe failed: {e}");
                // Retryable: do not broadcast BulkDeletionFailed yet — permanent
                // failure path calls on_permanent_failure.
                Err(edgequake_tasks::TaskError::Processing(reason))
            }
        }
    }
}
