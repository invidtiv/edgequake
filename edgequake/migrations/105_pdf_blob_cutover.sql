-- SPEC-090 F-090-16: PDF bytes SSOT is pdf_document_blobs; drop primary pdf_data.
SET search_path = public;

CREATE TABLE IF NOT EXISTS pdf_document_blobs (
  pdf_id UUID PRIMARY KEY REFERENCES pdf_documents(pdf_id) ON DELETE CASCADE,
  pdf_data BYTEA NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Backfill from primary column when present.
DO $$
BEGIN
  IF EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = 'public'
      AND table_name = 'pdf_documents'
      AND column_name = 'pdf_data'
  ) THEN
    INSERT INTO pdf_document_blobs (pdf_id, pdf_data)
    SELECT pdf_id, COALESCE(pdf_data, '\x'::bytea)
    FROM pdf_documents
    ON CONFLICT (pdf_id) DO NOTHING;
  END IF;
END $$;

-- Fail closed if any document lacks a blob after backfill.
DO $$
DECLARE
  missing bigint;
BEGIN
  SELECT COUNT(*) INTO missing
  FROM pdf_documents d
  LEFT JOIN pdf_document_blobs b ON b.pdf_id = d.pdf_id
  WHERE b.pdf_id IS NULL;
  IF missing > 0 THEN
    RAISE EXCEPTION
      'SPEC-090 M105: % pdf_documents rows missing pdf_document_blobs after backfill',
      missing;
  END IF;
END $$;

ALTER TABLE pdf_documents DROP COLUMN IF EXISTS pdf_data;
