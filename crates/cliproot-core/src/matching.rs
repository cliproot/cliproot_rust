//! Text matching engine for document annotation, citation generation, and
//! provenance coverage reporting. All functions are pure (no I/O).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────────

/// How citation markers are inserted into the annotated document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationStyle {
    /// `[1]` markers with a `Sources` section appended.
    Footnote,
    /// `<!-- cliproot:sha256-... -->` HTML comments after each matched paragraph.
    InlineComment,
    /// `[cliproot:sha256-...]` markers after each matched paragraph.
    Bracket,
}

/// A clip from the store, ready for matching against document text.
#[derive(Debug, Clone)]
pub struct MatchCandidate {
    pub clip_hash: String,
    pub clip_content: String,
    pub source_url: Option<String>,
    pub source_title: Option<String>,
}

/// A match between a document paragraph and a stored clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMatch {
    pub paragraph_index: usize,
    pub clip_hash: String,
    pub confidence: f64,
}

/// A numbered citation entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub index: usize,
    pub clip_hash: String,
    pub source_url: Option<String>,
    pub source_title: Option<String>,
    pub confidence: f64,
}

/// Result of annotating a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotateResult {
    pub annotated_text: String,
    pub citations: Vec<Citation>,
}

/// Provenance coverage report for a document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorResult {
    pub total_paragraphs: usize,
    pub covered_paragraphs: usize,
    pub uncovered_paragraphs: usize,
    pub paragraph_reports: Vec<ParagraphReport>,
}

/// Per-paragraph coverage info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParagraphReport {
    pub index: usize,
    pub text_preview: String,
    pub status: CoverageStatus,
    pub matched_clips: Vec<String>,
}

/// Coverage status for a paragraph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverageStatus {
    Covered,
    Uncovered,
}

// ── Public API ────────────────────────────────────────────────────────────

/// Parse an annotation style string into the enum.
pub fn parse_annotation_style(s: &str) -> Result<AnnotationStyle, String> {
    match s.to_lowercase().as_str() {
        "footnote" => Ok(AnnotationStyle::Footnote),
        "inline-comment" => Ok(AnnotationStyle::InlineComment),
        "bracket" => Ok(AnnotationStyle::Bracket),
        other => Err(format!(
            "unknown annotation style: {other:?}. Expected: footnote, inline-comment, bracket"
        )),
    }
}

/// Find matches between document paragraphs and stored clips.
///
/// Returns the best match per paragraph (highest confidence) above `threshold`.
/// Clips shorter than 20 characters are skipped to avoid false positives.
pub fn find_matches(
    document: &str,
    candidates: &[MatchCandidate],
    threshold: f64,
) -> Vec<TextMatch> {
    let paragraphs = split_paragraphs(document);
    let mut matches = Vec::new();

    for (para_idx, para) in paragraphs.iter().enumerate() {
        let para_trimmed = para.trim();
        if para_trimmed.is_empty() {
            continue;
        }

        let mut best: Option<TextMatch> = None;

        for candidate in candidates {
            if candidate.clip_content.len() < 20 {
                continue;
            }

            let confidence = compute_similarity(para_trimmed, &candidate.clip_content);
            if confidence >= threshold {
                if best.as_ref().map_or(true, |b| confidence > b.confidence) {
                    best = Some(TextMatch {
                        paragraph_index: para_idx,
                        clip_hash: candidate.clip_hash.clone(),
                        confidence,
                    });
                }
            }
        }

        if let Some(m) = best {
            matches.push(m);
        }
    }

    matches
}

/// Annotate a document by inserting citation markers at matched paragraphs.
pub fn annotate_document(
    document: &str,
    matches: &[TextMatch],
    candidates: &[MatchCandidate],
    style: AnnotationStyle,
) -> AnnotateResult {
    let paragraphs = split_paragraphs(document);
    let citations = generate_citations(matches, candidates);

    // Map clip_hash → citation index for fast lookup
    let hash_to_index: HashMap<&str, usize> = citations
        .iter()
        .map(|c| (c.clip_hash.as_str(), c.index))
        .collect();

    // Map paragraph_index → citation index
    let para_to_citation: HashMap<usize, usize> = matches
        .iter()
        .filter_map(|m| {
            hash_to_index.get(m.clip_hash.as_str()).map(|&idx| (m.paragraph_index, idx))
        })
        .collect();

    let mut annotated_paragraphs: Vec<String> = Vec::new();

    for (i, para) in paragraphs.iter().enumerate() {
        if let Some(&cite_idx) = para_to_citation.get(&i) {
            let marker = match &style {
                AnnotationStyle::Footnote => format!("[{}]", cite_idx),
                AnnotationStyle::InlineComment => {
                    let clip_hash = matches
                        .iter()
                        .find(|m| m.paragraph_index == i)
                        .map(|m| m.clip_hash.as_str())
                        .unwrap_or("");
                    format!("<!-- cliproot:{} -->", clip_hash)
                }
                AnnotationStyle::Bracket => {
                    let clip_hash = matches
                        .iter()
                        .find(|m| m.paragraph_index == i)
                        .map(|m| m.clip_hash.as_str())
                        .unwrap_or("");
                    format!("[cliproot:{}]", clip_hash)
                }
            };

            let trimmed = para.trim_end();
            annotated_paragraphs.push(format!("{} {}", trimmed, marker));
        } else {
            annotated_paragraphs.push(para.to_string());
        }
    }

    let mut annotated_text = annotated_paragraphs.join("\n\n");

    // Append sources section for footnote style
    if matches!(style, AnnotationStyle::Footnote) && !citations.is_empty() {
        annotated_text.push_str("\n\n---\n\nSources\n");
        for c in &citations {
            let label = c
                .source_title
                .as_deref()
                .or(c.source_url.as_deref())
                .unwrap_or(&c.clip_hash);
            let url = c.source_url.as_deref().unwrap_or("(no URL)");
            annotated_text.push_str(&format!("\n[{}] {} — {}", c.index, label, url));
        }
        annotated_text.push('\n');
    }

    AnnotateResult {
        annotated_text,
        citations,
    }
}

/// Generate deduplicated citations from matches.
///
/// Each unique clip_hash gets one citation number (1-based).
pub fn generate_citations(
    matches: &[TextMatch],
    candidates: &[MatchCandidate],
) -> Vec<Citation> {
    let candidate_map: HashMap<&str, &MatchCandidate> = candidates
        .iter()
        .map(|c| (c.clip_hash.as_str(), c))
        .collect();

    let mut seen: HashSet<String> = HashSet::new();
    let mut citations = Vec::new();

    for m in matches {
        if seen.contains(&m.clip_hash) {
            continue;
        }
        seen.insert(m.clip_hash.clone());

        let (source_url, source_title) = candidate_map
            .get(m.clip_hash.as_str())
            .map(|c| (c.source_url.clone(), c.source_title.clone()))
            .unwrap_or((None, None));

        citations.push(Citation {
            index: citations.len() + 1,
            clip_hash: m.clip_hash.clone(),
            source_url,
            source_title,
            confidence: m.confidence,
        });
    }

    citations
}

/// Generate a provenance coverage report for a document.
pub fn generate_doctor_report(document: &str, matches: &[TextMatch]) -> DoctorResult {
    let paragraphs = split_paragraphs(document);

    // Map paragraph_index → matched clip hashes
    let mut para_matches: HashMap<usize, Vec<String>> = HashMap::new();
    for m in matches {
        para_matches
            .entry(m.paragraph_index)
            .or_default()
            .push(m.clip_hash.clone());
    }

    let mut reports = Vec::new();
    let mut covered = 0;
    let mut uncovered = 0;

    for (i, para) in paragraphs.iter().enumerate() {
        let trimmed = para.trim();
        if trimmed.is_empty() {
            continue;
        }

        let preview = if trimmed.len() > 80 {
            format!("{}...", &trimmed[..80])
        } else {
            trimmed.to_string()
        };

        let matched_clips = para_matches.get(&i).cloned().unwrap_or_default();
        let status = if matched_clips.is_empty() {
            uncovered += 1;
            CoverageStatus::Uncovered
        } else {
            covered += 1;
            CoverageStatus::Covered
        };

        reports.push(ParagraphReport {
            index: i,
            text_preview: preview,
            status,
            matched_clips,
        });
    }

    DoctorResult {
        total_paragraphs: covered + uncovered,
        covered_paragraphs: covered,
        uncovered_paragraphs: uncovered,
        paragraph_reports: reports,
    }
}

// ── Internal Helpers ──────────────────────────────────────────────────────

/// Split document into paragraphs on blank lines.
fn split_paragraphs(text: &str) -> Vec<&str> {
    let mut paragraphs = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Look for \n\n (blank line separator)
        if bytes[i] == b'\n' && i + 1 < len && bytes[i + 1] == b'\n' {
            let para = &text[start..i];
            if !para.trim().is_empty() {
                paragraphs.push(para);
            }
            // Skip past all consecutive newlines
            while i < len && bytes[i] == b'\n' {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }

    // Last paragraph
    if start < len {
        let para = &text[start..];
        if !para.trim().is_empty() {
            paragraphs.push(para);
        }
    }

    paragraphs
}

/// Normalize text for matching: lowercase, collapse whitespace.
fn normalize_for_matching(text: &str) -> String {
    text.split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Tokenize text into lowercase word tokens.
fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Compute Jaccard similarity between two token sets.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let set_a = tokenize(a);
    let set_b = tokenize(b);

    if set_a.is_empty() && set_b.is_empty() {
        return 0.0;
    }

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Compute similarity between a document paragraph and a clip's content.
///
/// Tiered approach:
/// 1. Exact normalized substring → confidence 1.0
/// 2. Jaccard token similarity → confidence = jaccard score
fn compute_similarity(paragraph: &str, clip_content: &str) -> f64 {
    let norm_para = normalize_for_matching(paragraph);
    let norm_clip = normalize_for_matching(clip_content);

    // Tier 1: exact substring
    if norm_para.contains(&norm_clip) || norm_clip.contains(&norm_para) {
        return 1.0;
    }

    // Tier 2: Jaccard token similarity
    jaccard_similarity(paragraph, clip_content)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_candidate(hash: &str, content: &str, url: Option<&str>) -> MatchCandidate {
        MatchCandidate {
            clip_hash: hash.to_string(),
            clip_content: content.to_string(),
            source_url: url.map(|s| s.to_string()),
            source_title: None,
        }
    }

    #[test]
    fn test_exact_substring_match() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "Redis uses a single-threaded event loop",
            Some("https://redis.io/docs"),
        )];
        let doc = "Redis uses a single-threaded event loop which simplifies concurrency.";
        let matches = find_matches(doc, &candidates, 0.4);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].confidence, 1.0);
    }

    #[test]
    fn test_case_insensitive_match() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "redis uses a single-threaded event loop",
            None,
        )];
        let doc = "Redis Uses A Single-Threaded Event Loop.";
        let matches = find_matches(doc, &candidates, 0.4);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].confidence, 1.0);
    }

    #[test]
    fn test_jaccard_above_threshold() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "Redis processes commands sequentially using a single-threaded model",
            None,
        )];
        let doc = "Redis uses a single-threaded model to process commands sequentially.";
        let matches = find_matches(doc, &candidates, 0.4);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].confidence >= 0.4);
        assert!(matches[0].confidence < 1.0);
    }

    #[test]
    fn test_no_match_below_threshold() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "PostgreSQL uses multi-version concurrency control for isolation",
            None,
        )];
        let doc = "Redis uses a single-threaded event loop.";
        let matches = find_matches(doc, &candidates, 0.4);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_skip_short_clips() {
        let candidates = vec![make_candidate("sha256-abc", "Redis", None)];
        let doc = "Redis uses a single-threaded event loop.";
        let matches = find_matches(doc, &candidates, 0.1);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_annotate_footnote_style() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "Redis uses a single-threaded event loop",
            Some("https://redis.io/docs"),
        )];
        let doc = "Redis uses a single-threaded event loop which simplifies concurrency.";
        let matches = find_matches(doc, &candidates, 0.4);
        let result = annotate_document(doc, &matches, &candidates, AnnotationStyle::Footnote);
        assert!(result.annotated_text.contains("[1]"));
        assert!(result.annotated_text.contains("Sources"));
        assert!(result.annotated_text.contains("redis.io"));
        assert_eq!(result.citations.len(), 1);
    }

    #[test]
    fn test_annotate_inline_comment_style() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "Redis uses a single-threaded event loop",
            None,
        )];
        let doc = "Redis uses a single-threaded event loop which simplifies concurrency.";
        let matches = find_matches(doc, &candidates, 0.4);
        let result =
            annotate_document(doc, &matches, &candidates, AnnotationStyle::InlineComment);
        assert!(result.annotated_text.contains("<!-- cliproot:sha256-abc -->"));
    }

    #[test]
    fn test_annotate_bracket_style() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "Redis uses a single-threaded event loop",
            None,
        )];
        let doc = "Redis uses a single-threaded event loop which simplifies concurrency.";
        let matches = find_matches(doc, &candidates, 0.4);
        let result = annotate_document(doc, &matches, &candidates, AnnotationStyle::Bracket);
        assert!(result.annotated_text.contains("[cliproot:sha256-abc]"));
    }

    #[test]
    fn test_doctor_report() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "Redis uses a single-threaded event loop",
            None,
        )];
        let doc =
            "Redis uses a single-threaded event loop.\n\nThis paragraph has no matching source.";
        let matches = find_matches(doc, &candidates, 0.4);
        let report = generate_doctor_report(doc, &matches);
        assert_eq!(report.total_paragraphs, 2);
        assert_eq!(report.covered_paragraphs, 1);
        assert_eq!(report.uncovered_paragraphs, 1);
    }

    #[test]
    fn test_citation_deduplication() {
        let candidates = vec![make_candidate(
            "sha256-abc",
            "Redis uses a single-threaded event loop",
            None,
        )];
        // Same clip matches two paragraphs
        let matches = vec![
            TextMatch {
                paragraph_index: 0,
                clip_hash: "sha256-abc".to_string(),
                confidence: 1.0,
            },
            TextMatch {
                paragraph_index: 2,
                clip_hash: "sha256-abc".to_string(),
                confidence: 0.8,
            },
        ];
        let citations = generate_citations(&matches, &candidates);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].index, 1);
    }

    #[test]
    fn test_split_paragraphs() {
        let doc = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let paras = split_paragraphs(doc);
        assert_eq!(paras.len(), 3);
        assert_eq!(paras[0], "First paragraph.");
        assert_eq!(paras[1], "Second paragraph.");
        assert_eq!(paras[2], "Third paragraph.");
    }

    #[test]
    fn test_parse_annotation_style() {
        assert!(matches!(
            parse_annotation_style("footnote"),
            Ok(AnnotationStyle::Footnote)
        ));
        assert!(matches!(
            parse_annotation_style("inline-comment"),
            Ok(AnnotationStyle::InlineComment)
        ));
        assert!(matches!(
            parse_annotation_style("bracket"),
            Ok(AnnotationStyle::Bracket)
        ));
        assert!(parse_annotation_style("unknown").is_err());
    }
}
