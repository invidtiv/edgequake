//! Entity identity newtype and the single normalization entry point.
//!
//! # WHY THIS EXISTS (RC-6 / P-G1)
//!
//! Before this module, entity identity was a *convention* enforced nowhere:
//! - The orchestrator merger wrote graph nodes as `JOHN_DOE` (normalized) and
//!   entity vectors as the bare `JOHN_DOE`.
//! - The async processor wrote graph nodes as `John Doe` (raw) and entity
//!   vectors as `entity:John Doe` (raw, prefixed).
//! - The sync upload path wrote graph nodes as `JOHN_DOE` and vectors as
//!   `entity:JOHN_DOE`.
//!
//! Three writers, three conventions. The result was a silently fragmented
//! knowledge graph: the same real entity became multiple nodes, and the entity
//! vectors written by the processor were invisible to the query layer (which
//! looked them up by the graph node id `JOHN_DOE`).
//!
//! First principle: **identity is a value, not a convention.** An `EntityId` is
//! a normalized newtype. The graph node id and the entity vector id are both
//! *derived* from it, so they can never diverge by construction. No writer can
//! build an un-normalized entity id.
//!
//! # Canonical convention
//!
//! - Graph node id (legacy / no workspace) = `EntityId::as_graph_node_id()` → bare `JOHN_DOE`
//! - Graph node id (workspace-scoped, SPEC-032 / B3b) =
//!   `EntityId::scoped_graph_node_id(workspace_id)` → `{workspace_id}::JOHN_DOE`
//! - Entity vector id = `EntityId::as_vector_id()` → `entity:JOHN_DOE`
//!
//! WHY scoped graph ids: a shared AGE graph with bare `node_id` + UNIQUE
//! `eq_node_id` lets the first workspace to extract `SURGERY` own that vertex;
//! later Acc workspaces merge into foreign `workspace_id` rows and Mix query
//! (workspace filter) cannot see them. Vectors stay workspace-table-scoped, so
//! extract density looks healthy while the graph arm is starved.
//!
//! The `entity:` prefix on the vector id is what [`VectorId`] decodes back into
//! an [`EntityId`]; keeping the prefix makes the storage id self-describing
//! even when metadata is absent. Display `label` stays the bare normalized name.

//!
//! [`VectorId`]: crate::vector_id::VectorId

use crate::vector_id::VectorId;

/// A normalized entity identity.
///
/// Constructed exclusively via [`EntityId::new`], which runs the single
/// canonical normalizer. The wrapped string is always normalized
/// (UPPERCASE_UNDERSCORE, prefixes/possessives stripped). The empty string is
/// a valid interior value only when the input was empty/whitespace; callers
/// should check [`EntityId::is_empty`] and skip the write in that case (E1).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EntityId(String);

impl EntityId {
    /// Construct a normalized entity id from any raw name.
    ///
    /// Defensively strips a leading `entity:` prefix if a caller accidentally
    /// passes an already-prefixed value (E2), so `EntityId::new("entity:Foo")`
    /// and `EntityId::new("Foo")` produce the same identity.
    pub fn new(raw: &str) -> Self {
        let stripped = raw.strip_prefix("entity:").unwrap_or(raw);
        Self(normalize_entity_name(stripped))
    }

    /// Construct from an already-normalized string, bypassing normalization.
    ///
    /// This is the mirror of [`as_str`](EntityId::as_str) and exists so trusted
    /// readers (e.g. reconstructing an id from a graph node) can avoid
    /// re-normalizing. The caller guarantees the input is already normalized.
    pub fn from_normalized(normalized: impl Into<String>) -> Self {
        Self(normalized.into())
    }

    /// Separator between workspace UUID and normalized entity name in scoped
    /// graph node ids. Chosen so it cannot appear in UUIDs and is distinct from
    /// relationship `A::B` vector ids (those use a single `::` between *two
    /// entity names*, not a workspace prefix).
    pub const WORKSPACE_SCOPE_SEP: &str = "::";

    /// The bare normalized name (legacy graph node id when no workspace).
    pub fn as_graph_node_id(&self) -> &str {
        &self.0
    }

    /// Workspace-scoped graph node id: `{workspace_id}::{NORMALIZED_NAME}`.
    ///
    /// Empty / whitespace workspace falls back to the bare id (single-tenant /
    /// tests without a workspace context).
    pub fn scoped_graph_node_id(&self, workspace_id: &str) -> String {
        let ws = workspace_id.trim();
        if ws.is_empty() || self.0.is_empty() {
            return self.0.clone();
        }
        format!("{ws}{}{}", Self::WORKSPACE_SCOPE_SEP, self.0)
    }

    /// Resolve graph node id: scoped when `workspace_id` is `Some` and non-empty.
    pub fn graph_node_id_for_workspace(&self, workspace_id: Option<&str>) -> String {
        match workspace_id.map(str::trim).filter(|s| !s.is_empty()) {
            Some(ws) => self.scoped_graph_node_id(ws),
            None => self.0.clone(),
        }
    }

    /// Strip a leading `{workspace_id}::` scope when present; otherwise return
    /// the bare normalized name (for display / keyword match).
    pub fn bare_name_from_graph_node_id(graph_node_id: &str) -> &str {
        match graph_node_id.split_once(Self::WORKSPACE_SCOPE_SEP) {
            Some((maybe_ws, rest))
                if !rest.is_empty()
                    && maybe_ws.len() == 36
                    && maybe_ws.chars().filter(|c| *c == '-').count() == 4 =>
            {
                rest
            }
            _ => graph_node_id,
        }
    }

    /// The prefixed vector storage id (`entity:NAME`), used as the entity
    /// vector id.
    pub fn as_vector_id(&self) -> String {
        format!("entity:{}", self.0)
    }

    /// The bare normalized name as a borrowed string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True if the normalized name is empty (E1). Callers should skip writes
    /// for empty ids and log a warning.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Decode an entity vector storage id (e.g. `entity:JOHN_DOE`) back into an
    /// `EntityId`. Returns `None` for non-entity vector ids (chunk/relationship)
    /// or for an empty entity name.
    pub fn from_vector_storage_id(storage_id: &str) -> Option<Self> {
        VectorId::from_storage_id(storage_id).and_then(|vid| match vid {
            VectorId::Entity { name } => {
                let name = name.strip_prefix("entity:").unwrap_or(&name);
                if name.is_empty() {
                    None
                } else {
                    Some(Self::from_normalized(name))
                }
            }
            _ => None,
        })
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&EntityId> for String {
    fn from(id: &EntityId) -> Self {
        id.0.clone()
    }
}

/// The single canonical entity-name normalizer.
///
/// This is the **only** place entity names are normalized in the codebase.
/// `edgequake-pipeline` re-exports this function as
/// `edgequake_pipeline::prompts::normalize_entity_name` for backwards
/// compatibility; do not duplicate the logic elsewhere (DRY).
///
/// Transformations (SPEC-083 C-14):
/// 1. Trim surrounding whitespace
/// 2. Unicode NFC
/// 3. Casefold (`to_lowercase` — stable Unicode case-folding proxy)
/// 4. Strip leading articles (`the` / `a` / `an`, case-insensitive via fold)
/// 5. Strip possessive suffixes per word (`'s` ASCII and U+2019 `’s`)
/// 6. Title-case each word, join with `_`, uppercase the result
///
/// Empty / whitespace-only input yields the empty string (E1).
///
/// Also rejects LightRAG `normalize_extracted_info` numeric empties (056):
/// pure digits with `len < 3`, or digits+dots with `len < 6` and at least one dot.
///
/// Also rejects opaque machine identifiers (067): UUID/GUID, ULID, Mongo
/// ObjectId, long hex hashes, AWS ARNs. Multimodal `im-…` identities are
/// **kept** (066 Drawing path uses them as stable node ids).
pub fn normalize_entity_name(raw_name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;

    let trimmed = raw_name.trim();
    if trimmed.is_empty()
        || is_lightrag_rejected_numeric_name(trimmed)
        || is_opaque_identifier(trimmed)
    {
        return String::new();
    }

    // NFC → casefold before article/possessive strips so THE/The/the unify.
    let nfc: String = trimmed.nfc().collect();
    let folded = nfc.to_lowercase();
    let without_articles = strip_leading_articles(folded.trim());

    let normalized = without_articles
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|word| {
            let without_possessive = strip_possessive_suffix(word);
            to_title_case(without_possessive)
        })
        .collect::<Vec<_>>()
        .join("_")
        .to_uppercase();

    // Re-check after fold (e.g. dotted forms that survived whitespace split).
    if is_lightrag_rejected_numeric_name(&normalized) || is_opaque_identifier(&normalized) {
        return String::new();
    }
    normalized
}

/// Strip leading English articles after casefold (`the` / `a` / `an`).
fn strip_leading_articles(s: &str) -> &str {
    let mut rest = s;
    loop {
        let next = rest
            .strip_prefix("the ")
            .or_else(|| rest.strip_prefix("a "))
            .or_else(|| rest.strip_prefix("an "));
        match next {
            Some(n) => rest = n.trim_start(),
            None => break,
        }
    }
    rest
}

/// Strip trailing possessive `'s` (ASCII) or `’s` (U+2019).
fn strip_possessive_suffix(word: &str) -> &str {
    word.strip_suffix("'s")
        .or_else(|| word.strip_suffix("\u{2019}s"))
        .unwrap_or(word)
}

/// True when `raw` is an opaque machine identifier unsuitable as a semantic
/// entity name (067).
///
/// Strips a leading workspace scope (`{uuid}::NAME`) before checking so
/// detection runs on the bare name. Multimodal `im-…` / `IM-…` item ids are
/// **not** opaque for write-reject (they are valid Drawing/Table/Equation
/// identities); callers that only need display soft-labels should still treat
/// bare UUID-shaped names as opaque.
pub fn is_opaque_identifier(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }

    let bare = EntityId::bare_name_from_graph_node_id(trimmed);
    let bare_l = bare.to_ascii_lowercase();

    // Multimodal item ids (066): stable identity, not rejected at write time.
    if bare_l.starts_with("im-") {
        return false;
    }

    // AWS ARN (conservative prefix match).
    if bare_l.starts_with("arn:aws:") {
        return true;
    }

    let mut candidate = bare.trim();
    candidate = candidate
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(candidate);
    if let Some(rest) = candidate
        .strip_prefix("urn:uuid:")
        .or_else(|| candidate.strip_prefix("URN:UUID:"))
        .or_else(|| candidate.strip_prefix("uuid:"))
        .or_else(|| candidate.strip_prefix("UUID:"))
    {
        candidate = rest;
    }
    // Underscore fold from normalizer: UUID segments may be joined with `_`
    // only when whitespace was present; hyphenated forms stay hyphenated.
    let compact = candidate.replace(['-', '_'], "");

    if is_uuid_shape(candidate) {
        return true;
    }
    // Prefixed / tokenized opaque forms: `RESOURCE_<uuid>`, `org:<uuid>`,
    // or a name whose *only* underscore/colon-separated tokens are a short
    // prefix plus a full RFC-4122 UUID (072). Do not reject names that merely
    // contain a short hex substring.
    if opaque_prefixed_or_token_uuid(candidate) {
        return true;
    }
    // Compact UUID / 32-hex digest (no separators).
    if compact.len() == 32 && compact.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // SHA-1 (40) / SHA-256 (64) / similar digests.
    if (compact.len() == 40 || compact.len() == 64 || compact.len() == 128)
        && compact.chars().all(|c| c.is_ascii_hexdigit())
    {
        return true;
    }
    // MongoDB ObjectId.
    if compact.len() == 24 && compact.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // ULID (26 Crockford base32).
    if is_ulid_shape(candidate) {
        return true;
    }

    false
}

/// True when the whole candidate is a short prefix plus a UUID token.
fn opaque_prefixed_or_token_uuid(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    // Split on `_` or `:` — require exactly one non-UUID prefix token and one UUID.
    let tokens: Vec<&str> = s.split(['_', ':']).filter(|t| !t.is_empty()).collect();
    if tokens.len() == 2 {
        let (a, b) = (tokens[0], tokens[1]);
        let a_uuid = is_uuid_shape(a);
        let b_uuid = is_uuid_shape(b);
        if a_uuid ^ b_uuid {
            // Prefix must be short alphanumeric (RESOURCE, org, id, …).
            let prefix = if b_uuid { a } else { b };
            return prefix.len() <= 32
                && prefix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-');
        }
    }
    // Trailing / leading UUID glued with a single underscore after strip of
    // common `uuid` word already handled above; also catch `NAME_<uuid>` with
    // multi-segment prefix when the *last* token is a UUID and the rest has
    // no letters forming a long human phrase — keep conservative: only when
    // the last token is UUID and every other token is <=12 ASCII alnum chars.
    if tokens.len() >= 2 {
        if let Some(last) = tokens.last() {
            if is_uuid_shape(last) {
                let prefix_ok = tokens[..tokens.len() - 1]
                    .iter()
                    .all(|t| t.len() <= 12 && t.chars().all(|c| c.is_ascii_alphanumeric()));
                if prefix_ok {
                    return true;
                }
            }
        }
        if let Some(first) = tokens.first() {
            if is_uuid_shape(first) {
                let suffix_ok = tokens[1..]
                    .iter()
                    .all(|t| t.len() <= 12 && t.chars().all(|c| c.is_ascii_alphanumeric()));
                if suffix_ok {
                    return true;
                }
            }
        }
    }
    false
}

/// RFC-4122 UUID shape: 8-4-4-4-12 hexadecimal groups.
fn is_uuid_shape(s: &str) -> bool {
    let s = s.trim();
    if s.len() != 36 {
        return false;
    }
    let b = s.as_bytes();
    if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
        return false;
    }
    for (i, ch) in s.chars().enumerate() {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            continue;
        }
        if !ch.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// ULID: 26 chars from Crockford's Base32 alphabet.
fn is_ulid_shape(s: &str) -> bool {
    let s = s.trim();
    if s.len() != 26 {
        return false;
    }
    s.chars().all(|c| {
        matches!(
            c.to_ascii_uppercase(),
            '0'..='9' | 'A'..='H' | 'J'..='K' | 'M'..='N' | 'P'..='T' | 'V'..='Z'
        )
    })
}

/// LightRAG `normalize_extracted_info` empty-name filters for short numeric labels.
fn is_lightrag_rejected_numeric_name(name: &str) -> bool {
    let t = name.trim();
    if t.is_empty() {
        return false;
    }
    if t.len() < 3 && t.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let digits_and_dots = t.chars().all(|c| c.is_ascii_digit() || c == '.');
    if t.len() < 6 && digits_and_dots && t.contains('.') {
        return true;
    }
    false
}

/// Convert a word to title case (first letter uppercase, rest lowercase).
fn to_title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(|c| c.to_lowercase()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_normalizes_casing_variants_to_one_identity() {
        // P-G1 acceptance (unit level): three casing variants → one EntityId.
        let a = EntityId::new("John Doe");
        let b = EntityId::new("john doe");
        let c = EntityId::new("JOHN DOE");
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a.as_graph_node_id(), "JOHN_DOE");
    }

    #[test]
    fn graph_node_id_and_vector_id_are_derived_consistently() {
        let id = EntityId::new("Sarah Chen");
        assert_eq!(id.as_graph_node_id(), "SARAH_CHEN");
        assert_eq!(id.as_vector_id(), "entity:SARAH_CHEN");
    }

    #[test]
    fn vector_id_round_trips_through_storage_id() {
        // P-G1: EntityId → vector id → from_storage_id → EntityId.
        let id = EntityId::new("Apple Inc");
        let vid = id.as_vector_id();
        let decoded = EntityId::from_vector_storage_id(&vid).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn strips_accidental_entity_prefix() {
        // E2: a caller passing an already-prefixed value must not double-prefix.
        assert_eq!(EntityId::new("entity:Foo Bar"), EntityId::new("Foo Bar"));
        assert_eq!(
            EntityId::new("entity:Foo Bar").as_vector_id(),
            "entity:FOO_BAR"
        );
    }

    #[test]
    fn empty_name_is_empty_identity() {
        // E1.
        assert!(EntityId::new("").is_empty());
        assert!(EntityId::new("   ").is_empty());
        assert_eq!(EntityId::new("").as_graph_node_id(), "");
    }

    #[test]
    fn non_entity_storage_id_decodes_to_none() {
        assert!(EntityId::from_vector_storage_id("doc-123-chunk-0").is_none());
        assert!(EntityId::from_vector_storage_id("A::B").is_none());
    }

    #[test]
    fn prefixes_and_possessives_stripped() {
        assert_eq!(EntityId::new("The Company").as_str(), "COMPANY");
        assert_eq!(EntityId::new("John's").as_str(), "JOHN");
    }

    #[test]
    #[allow(non_snake_case)] // Matrix ID: unit_normalize_THE_COMPANY
    fn unit_normalize_THE_COMPANY() {
        // Matrix Cluster 03 / C-14: article strip after casefold.
        assert_eq!(normalize_entity_name("THE COMPANY"), "COMPANY");
        assert_eq!(normalize_entity_name("The Company"), "COMPANY");
        assert_eq!(normalize_entity_name("the company"), "COMPANY");
        assert_eq!(EntityId::new("THE COMPANY"), EntityId::new("The Company"));
    }

    #[test]
    fn unit_normalize_curly_apostrophe() {
        // Matrix Cluster 03 / C-14: U+2019 RIGHT SINGLE QUOTATION MARK.
        assert_eq!(normalize_entity_name("John's"), "JOHN");
        assert_eq!(normalize_entity_name("John\u{2019}s"), "JOHN");
        assert_eq!(EntityId::new("John's"), EntityId::new("John\u{2019}s"));
    }

    #[test]
    fn non_ascii_preserved_and_normalized() {
        // E3: non-ASCII names are handled by the existing title-case logic.
        assert_eq!(EntityId::new("René Descartes").as_str(), "RENÉ_DESCARTES");
    }

    #[test]
    fn hyphens_and_special_chars_preserved() {
        assert_eq!(EntityId::new("New-York").as_str(), "NEW-YORK");
        assert_eq!(EntityId::new("C++").as_str(), "C++");
    }

    #[test]
    fn scoped_graph_node_id_prefixes_workspace() {
        let id = EntityId::new("Basal Cell");
        let ws = "e0270f5f-0b6c-4e90-882f-5f9b0eac8cff";
        assert_eq!(id.scoped_graph_node_id(ws), format!("{ws}::BASAL_CELL"));
        assert_eq!(
            id.graph_node_id_for_workspace(Some(ws)),
            id.scoped_graph_node_id(ws)
        );
        assert_eq!(id.graph_node_id_for_workspace(None), "BASAL_CELL");
        assert_eq!(id.scoped_graph_node_id("  "), "BASAL_CELL");
    }

    #[test]
    fn bare_name_strips_uuid_workspace_scope() {
        let ws = "e0270f5f-0b6c-4e90-882f-5f9b0eac8cff";
        let scoped = format!("{ws}::BASAL_CELL");
        assert_eq!(
            EntityId::bare_name_from_graph_node_id(&scoped),
            "BASAL_CELL"
        );
        assert_eq!(
            EntityId::bare_name_from_graph_node_id("BASAL_CELL"),
            "BASAL_CELL"
        );
        // Relationship-style A::B must not be stripped (left side is not a UUID).
        assert_eq!(
            EntityId::bare_name_from_graph_node_id("ALPHA::BETA"),
            "ALPHA::BETA"
        );
    }

    #[test]
    fn lightrag_short_numeric_names_normalize_empty() {
        // 056 / LightRAG normalize_extracted_info
        assert_eq!(normalize_entity_name("42"), "");
        assert_eq!(normalize_entity_name("7"), "");
        assert_eq!(normalize_entity_name("1.2"), "");
        assert_eq!(normalize_entity_name("12.3"), "");
        // Kept: years and real names
        assert_eq!(normalize_entity_name("2022"), "2022");
        assert_eq!(normalize_entity_name("BRCA1"), "BRCA1");
        assert_eq!(normalize_entity_name("5 Fluorouracil"), "5_FLUOROURACIL");
    }

    #[test]
    fn opaque_uuid_names_normalize_empty() {
        // 067 — UUID/GUID must not become entity identity
        assert!(is_opaque_identifier("84b69e27-e38b-444a-83dd-5e6a537c6f12"));
        assert!(is_opaque_identifier(
            "{84B69E27-E38B-444A-83DD-5E6A537C6F12}"
        ));
        assert!(is_opaque_identifier(
            "urn:uuid:84b69e27-e38b-444a-83dd-5e6a537c6f12"
        ));
        assert_eq!(
            normalize_entity_name("84b69e27-e38b-444a-83dd-5e6a537c6f12"),
            ""
        );
        assert!(EntityId::new("84b69e27-E38b-444a-83dd-5e6a537c6f12").is_empty());
        // Compact 32-hex
        assert_eq!(
            normalize_entity_name("84b69e27e38b444a83dd5e6a537c6f12"),
            ""
        );
    }

    #[test]
    fn opaque_ulid_objectid_hash_arn_rejected() {
        assert!(is_opaque_identifier("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert_eq!(normalize_entity_name("01ARZ3NDEKTSV4RRFFQ69G5FAV"), "");
        assert!(is_opaque_identifier("507f1f77bcf86cd799439011"));
        assert_eq!(normalize_entity_name("507f1f77bcf86cd799439011"), "");
        let sha = "a".repeat(40);
        assert!(is_opaque_identifier(&sha));
        assert_eq!(normalize_entity_name(&sha), "");
        assert!(is_opaque_identifier("arn:aws:s3:::my-bucket/object-key"));
        assert_eq!(
            normalize_entity_name("arn:aws:s3:::my-bucket/object-key"),
            ""
        );
    }

    #[test]
    fn multimodal_im_ids_and_semantic_names_kept() {
        // 066 Drawing identity must survive EntityId::new
        let mm = "im-019f7028-d3e3-7684-8b3b-a9259368329a-page-0002-fig-01";
        assert!(!is_opaque_identifier(mm));
        assert!(!EntityId::new(mm).is_empty());
        assert!(EntityId::new(mm).as_str().starts_with("IM-"));

        assert!(!is_opaque_identifier("Acme Corp"));
        assert_eq!(normalize_entity_name("Acme Corp"), "ACME_CORP");
        assert_eq!(normalize_entity_name("New-York"), "NEW-YORK");
        assert_eq!(normalize_entity_name("BRCA1"), "BRCA1");
        assert_eq!(normalize_entity_name("2022"), "2022");
    }

    #[test]
    fn opaque_check_strips_workspace_scope() {
        let ws = "e0270f5f-0b6c-4e90-882f-5f9b0eac8cff";
        let scoped_uuid = format!("{ws}::84b69e27-e38b-444a-83dd-5e6a537c6f12");
        assert!(is_opaque_identifier(&scoped_uuid));
        let scoped_name = format!("{ws}::ACME_CORP");
        assert!(!is_opaque_identifier(&scoped_name));
    }

    #[test]
    fn opaque_prefixed_uuid_rejected() {
        // 072 — PREFIX_UUID / uuid: / org:uuid
        assert!(is_opaque_identifier(
            "RESOURCE_84B69E27-E38B-444A-83DD-5E6A537C6F12"
        ));
        assert!(is_opaque_identifier(
            "org:84b69e27-e38b-444a-83dd-5e6a537c6f12"
        ));
        assert!(is_opaque_identifier(
            "uuid:84b69e27-e38b-444a-83dd-5e6a537c6f12"
        ));
        assert_eq!(
            normalize_entity_name("RESOURCE_84B69E27-E38B-444A-83DD-5E6A537C6F12"),
            ""
        );
        // Human names that merely contain short hex must stay.
        assert!(!is_opaque_identifier("Room 84b6"));
        assert!(!is_opaque_identifier("ACME_CORP"));
        assert_eq!(normalize_entity_name("Room 84b6"), "ROOM_84B6");
    }
}
