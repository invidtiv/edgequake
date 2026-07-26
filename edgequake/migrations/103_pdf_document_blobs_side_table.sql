-- SPEC-090 F-090-16 follow-on: side table for PDF binaries (expand/contract).
-- List path already omits pdf_data (Wave 1). This table is the target for
-- by-id blob storage so base backups and WAL stop carrying every PDF twice.
-- Dual-write / cutover of pdf_documents.pdf_data is application-layer (Wave 4+).

CREATE TABLE IF NOT EXISTS pdf_document_blobs (
    pdf_id UUID PRIMARY KEY REFERENCES pdf_documents(pdf_id) ON DELETE CASCADE,
    pdf_data BYTEA NOT NULL,
    markdown_content TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

COMMENT ON TABLE pdf_document_blobs IS
  'SPEC-090: PDF binary + markdown out of pdf_documents row (list stays metadata-only)';
