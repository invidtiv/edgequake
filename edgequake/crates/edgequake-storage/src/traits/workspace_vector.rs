//! Workspace-scoped vector storage registry.
//!
//! # Implements
//!
//! - **FEAT0350**: Per-workspace vector storage with independent dimensions
//!
//! # Enforces
//!
//! - **BR0350**: Each workspace has isolated vector storage
//! - **BR0351**: Dimension is determined by workspace embedding configuration
//! - **BR0352**: Workspaces cannot cross-query vectors with different dimensions
//!
//! # WHY: Workspace Vector Isolation
//!
//! Different workspaces may use different embedding providers:
//! - OpenAI text-embedding-3-small: 1536 dimensions
//! - Ollama nomic-embed-text: 768 dimensions
//! - Cohere embed-v3: 1024 dimensions
//!
//! Mixing dimensions in a single vector table causes:
//! - Corrupt similarity scores (comparing apples to oranges)
//! - Index inefficiency (can't optimize for single dimension)
//! - Provider lock-in (can't change per-workspace)
//!
//! This registry creates per-workspace vector tables with:
//! - Correct dimension for the workspace's embedding provider
//! - Isolated HNSW/IVFFlat indices
//! - Independent lifecycle (can rebuild one workspace without affecting others)

use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use super::VectorStorage;
use crate::error::Result;

/// Configuration for workspace vector storage.
#[derive(Debug, Clone)]
pub struct WorkspaceVectorConfig {
    /// Workspace UUID
    pub workspace_id: Uuid,
    /// Embedding dimension for this workspace
    pub dimension: usize,
    /// Optional namespace prefix (default: "default")
    pub namespace: String,
}

impl WorkspaceVectorConfig {
    /// Create a new workspace vector configuration.
    pub fn new(workspace_id: Uuid, dimension: usize) -> Self {
        Self {
            workspace_id,
            dimension,
            namespace: "default".to_string(),
        }
    }

    /// Set the namespace prefix.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Full-entropy workspace slug for NEW tables (hyphens → underscores).
    pub fn workspace_slug(&self) -> String {
        workspace_slug_full(&self.workspace_id)
    }

    /// Legacy 8-char prefix slug (pre SPEC-090 F-090-17).
    pub fn legacy_workspace_slug(&self) -> String {
        workspace_slug_legacy(&self.workspace_id)
    }

    /// PostgresConfig namespace for this workspace (full slug).
    pub fn namespace_prefix(&self) -> String {
        format!("{}_ws_{}", self.namespace, self.workspace_slug())
    }

    /// Legacy namespace prefix (8-char slug).
    pub fn legacy_namespace_prefix(&self) -> String {
        format!("{}_ws_{}", self.namespace, self.legacy_workspace_slug())
    }

    /// Generate the table name for this workspace (full slug).
    pub fn table_name(&self) -> String {
        format!("eq_{}_ws_{}_vectors", self.namespace, self.workspace_slug())
    }

    /// Legacy table name (8-char slug) for backward compatibility reads.
    pub fn legacy_table_name(&self) -> String {
        format!(
            "eq_{}_ws_{}_vectors",
            self.namespace,
            self.legacy_workspace_slug()
        )
    }
}

/// Full-entropy workspace slug: UUID with hyphens replaced by underscores.
pub fn workspace_slug_full(workspace_id: &Uuid) -> String {
    workspace_id.to_string().replace('-', "_")
}

/// Legacy 8-char UUID prefix (pre F-090-17).
pub fn workspace_slug_legacy(workspace_id: &Uuid) -> String {
    workspace_id.to_string()[..8].to_string()
}

/// Registry for managing per-workspace vector storage instances.
///
/// This registry provides lazy initialization of workspace-specific
/// vector storage with correct dimensions. Each workspace gets its
/// own PostgreSQL table with:
/// - Correct vector dimension
/// - Optimized HNSW index
/// - Isolated data lifecycle
///
/// # Thread Safety
///
/// The registry is thread-safe and can be shared across request handlers.
/// Internal locking ensures safe concurrent access to the instance cache.
///
/// # Example
///
/// ```ignore
/// // Get vector storage for a workspace
/// let config = WorkspaceVectorConfig::new(workspace_id, 1536);
/// let storage = registry.get_or_create(config).await?;
///
/// // Use the workspace-specific storage
/// storage.upsert(&vectors).await?;
/// ```
#[async_trait]
pub trait WorkspaceVectorRegistry: Send + Sync {
    /// Get or create vector storage for a workspace.
    ///
    /// If the workspace already has a storage instance cached, returns it.
    /// Otherwise, creates a new storage with the specified dimension.
    ///
    /// # Arguments
    ///
    /// * `config` - Workspace vector configuration including dimension
    ///
    /// # Returns
    ///
    /// Arc to the workspace's vector storage instance.
    async fn get_or_create(&self, config: WorkspaceVectorConfig) -> Result<Arc<dyn VectorStorage>>;

    /// Get existing vector storage for a workspace without creating.
    ///
    /// Returns None if the workspace doesn't have a cached storage instance.
    async fn get(&self, workspace_id: &Uuid) -> Option<Arc<dyn VectorStorage>>;

    /// Check if a workspace has vector storage initialized.
    async fn has_storage(&self, workspace_id: &Uuid) -> bool;

    /// Get the dimension of a workspace's vector storage.
    ///
    /// Returns None if the workspace doesn't have storage initialized.
    async fn get_dimension(&self, workspace_id: &Uuid) -> Option<usize>;

    /// List all workspace IDs that have vector storage.
    async fn list_workspaces(&self) -> Vec<Uuid>;

    /// Remove a workspace's vector storage from the cache.
    ///
    /// This does NOT delete the underlying table, just removes
    /// the cached instance. Useful for forcing re-initialization.
    async fn evict(&self, workspace_id: &Uuid);

    /// Drop the underlying PostgreSQL vector table for a workspace.
    ///
    /// WHY: `evict()` only clears the in-memory cache. After `delete_workspace`,
    /// the physical table `eq_{ns}_ws_{id}_vectors` would otherwise remain as an
    /// orphan, consuming disk space and potentially causing confusion on re-create
    /// (SPEC-054 fix for GitHub #297).
    ///
    /// Default is a no-op (memory backend has no physical table to drop).
    async fn drop_workspace_table(&self, _workspace_id: &Uuid) -> Result<()> {
        Ok(())
    }

    /// Clear all cached instances.
    async fn clear_cache(&self);

    /// Get the default/fallback vector storage.
    ///
    /// Used for backward compatibility when workspace_id is not specified.
    fn default_storage(&self) -> Arc<dyn VectorStorage>;
}
