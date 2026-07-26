# Issue study — F-090-16 PDF list blobs

## Symptom

Listing 20 PDFs reads full binaries + markdown from TOAST.

## Mechanism

`pdf_list_query.rs` SELECTs `pdf_data`, `markdown_content`.

## Fix

Metadata-only list projection; blobs on by-id path; later side-table/object storage.

## Test

`e2e_spec090_pdf_list_no_blob`
