use rusqlite::{params, Connection};
use std::path::Path;

use cliproot_core::{Clip, CrpBundle, DerivationEdge};

use crate::error::StoreError;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS clips (
    clip_hash TEXT PRIMARY KEY,
    id TEXT,
    document_id TEXT,
    text_hash TEXT NOT NULL,
    content TEXT,
    bundle_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS clip_source_refs (
    clip_hash TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    PRIMARY KEY (clip_hash, source_ref),
    FOREIGN KEY (clip_hash) REFERENCES clips(clip_hash)
);

CREATE TABLE IF NOT EXISTS derivation_edges (
    id TEXT PRIMARY KEY,
    child_clip_hash TEXT NOT NULL,
    parent_clip_hash TEXT NOT NULL,
    transformation_type TEXT NOT NULL,
    agent_id TEXT,
    confidence REAL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    uri TEXT,
    title TEXT,
    canonical_hash TEXT
);

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    agent_type TEXT NOT NULL,
    name TEXT,
    uri TEXT
);

CREATE TABLE IF NOT EXISTS sources (
    id TEXT PRIMARY KEY,
    source_type TEXT NOT NULL,
    title TEXT,
    source_uri TEXT,
    author_agent_id TEXT,
    created_at TEXT
);

CREATE TABLE IF NOT EXISTS activities (
    id TEXT PRIMARY KEY,
    activity_type TEXT NOT NULL,
    agent_id TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_derivation_child ON derivation_edges(child_clip_hash);
CREATE INDEX IF NOT EXISTS idx_derivation_parent ON derivation_edges(parent_clip_hash);
CREATE INDEX IF NOT EXISTS idx_clips_document ON clips(document_id);
CREATE INDEX IF NOT EXISTS idx_clips_id ON clips(id);
"#;

pub struct IndexDb {
    conn: Connection,
}

impl IndexDb {
    pub fn open(db_path: &Path) -> Result<Self, StoreError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Ok(Self { conn })
    }

    pub fn init(&self) -> Result<(), StoreError> {
        self.conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    pub fn index_bundle(&self, bundle: &CrpBundle, bundle_hash: &str) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;

        // Index document
        if let Some(doc) = &bundle.document {
            tx.execute(
                "INSERT OR REPLACE INTO documents (id, uri, title, canonical_hash) VALUES (?1, ?2, ?3, ?4)",
                params![
                    doc.id.0,
                    doc.uri,
                    doc.title,
                    doc.canonical_hash.as_ref().map(|h| &h.0),
                ],
            )?;
        }

        // Index agents
        for agent in &bundle.agents {
            tx.execute(
                "INSERT OR REPLACE INTO agents (id, agent_type, name, uri) VALUES (?1, ?2, ?3, ?4)",
                params![
                    agent.id.0,
                    serde_json::to_value(&agent.agent_type)?.as_str().unwrap_or(""),
                    agent.name,
                    agent.uri,
                ],
            )?;
        }

        // Index sources
        for source in &bundle.sources {
            tx.execute(
                "INSERT OR REPLACE INTO sources (id, source_type, title, source_uri, author_agent_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    source.id.0,
                    serde_json::to_value(&source.source_type)?.as_str().unwrap_or(""),
                    source.title,
                    source.source_uri,
                    source.author_agent_id.as_ref().map(|a| &a.0),
                    source.created_at,
                ],
            )?;
        }

        // Index clips
        for clip in &bundle.clips {
            tx.execute(
                "INSERT OR REPLACE INTO clips (clip_hash, id, document_id, text_hash, content, bundle_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    clip.clip_hash.0,
                    clip.id.as_ref().map(|i| &i.0),
                    clip.document_id.as_ref().map(|i| &i.0),
                    clip.text_hash.0,
                    clip.content,
                    bundle_hash,
                ],
            )?;

            for source_ref in &clip.source_refs {
                tx.execute(
                    "INSERT OR REPLACE INTO clip_source_refs (clip_hash, source_ref) VALUES (?1, ?2)",
                    params![clip.clip_hash.0, source_ref],
                )?;
            }
        }

        // Index derivation edges
        for edge in &bundle.derivation_edges {
            tx.execute(
                "INSERT OR REPLACE INTO derivation_edges (id, child_clip_hash, parent_clip_hash, transformation_type, agent_id, confidence, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    edge.id.0,
                    edge.child_clip_hash.0,
                    edge.parent_clip_hash.0,
                    serde_json::to_value(&edge.transformation_type)?.as_str().unwrap_or(""),
                    edge.agent_id.as_ref().map(|a| &a.0),
                    edge.confidence,
                    edge.created_at,
                ],
            )?;
        }

        // Index activities
        for activity in &bundle.activities {
            tx.execute(
                "INSERT OR REPLACE INTO activities (id, activity_type, agent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    activity.id.0,
                    serde_json::to_value(&activity.activity_type)?.as_str().unwrap_or(""),
                    activity.agent_id.as_ref().map(|a| &a.0),
                    activity.created_at,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn find_clip_by_hash(&self, hash: &str) -> Result<Option<ClipRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT clip_hash, id, document_id, text_hash, content, bundle_hash FROM clips WHERE clip_hash = ?1",
        )?;
        let mut rows = stmt.query_map(params![hash], |row| {
            Ok(ClipRow {
                clip_hash: row.get(0)?,
                id: row.get(1)?,
                document_id: row.get(2)?,
                text_hash: row.get(3)?,
                content: row.get(4)?,
                bundle_hash: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn find_clip_by_id(&self, id: &str) -> Result<Option<ClipRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT clip_hash, id, document_id, text_hash, content, bundle_hash FROM clips WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ClipRow {
                clip_hash: row.get(0)?,
                id: row.get(1)?,
                document_id: row.get(2)?,
                text_hash: row.get(3)?,
                content: row.get(4)?,
                bundle_hash: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_clips(
        &self,
        document_id: Option<&str>,
        source_type: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<ClipRow>, StoreError> {
        let limit = limit.unwrap_or(100);

        if let Some(doc_id) = document_id {
            let mut stmt = self.conn.prepare(
                "SELECT clip_hash, id, document_id, text_hash, content, bundle_hash FROM clips WHERE document_id = ?1 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![doc_id, limit], |row| {
                Ok(ClipRow {
                    clip_hash: row.get(0)?,
                    id: row.get(1)?,
                    document_id: row.get(2)?,
                    text_hash: row.get(3)?,
                    content: row.get(4)?,
                    bundle_hash: row.get(5)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else if let Some(src_type) = source_type {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT c.clip_hash, c.id, c.document_id, c.text_hash, c.content, c.bundle_hash
                 FROM clips c
                 JOIN clip_source_refs csr ON c.clip_hash = csr.clip_hash
                 JOIN sources s ON csr.source_ref = s.id
                 WHERE s.source_type = ?1
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![src_type, limit], |row| {
                Ok(ClipRow {
                    clip_hash: row.get(0)?,
                    id: row.get(1)?,
                    document_id: row.get(2)?,
                    text_hash: row.get(3)?,
                    content: row.get(4)?,
                    bundle_hash: row.get(5)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT clip_hash, id, document_id, text_hash, content, bundle_hash FROM clips LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], |row| {
                Ok(ClipRow {
                    clip_hash: row.get(0)?,
                    id: row.get(1)?,
                    document_id: row.get(2)?,
                    text_hash: row.get(3)?,
                    content: row.get(4)?,
                    bundle_hash: row.get(5)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        }
    }

    pub fn find_derivation_parents(&self, clip_hash: &str) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, child_clip_hash, parent_clip_hash, transformation_type, agent_id, confidence, created_at
             FROM derivation_edges WHERE child_clip_hash = ?1",
        )?;
        let rows = stmt.query_map(params![clip_hash], |row| {
            Ok(EdgeRow {
                id: row.get(0)?,
                child_clip_hash: row.get(1)?,
                parent_clip_hash: row.get(2)?,
                transformation_type: row.get(3)?,
                agent_id: row.get(4)?,
                confidence: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Walk derivation_edges upward recursively using a CTE.
    pub fn trace_lineage(&self, clip_hash: &str) -> Result<Vec<LineageNode>, StoreError> {
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE lineage(clip_hash, parent_hash, transformation_type, depth) AS (
                SELECT child_clip_hash, parent_clip_hash, transformation_type, 1
                FROM derivation_edges
                WHERE child_clip_hash = ?1
                UNION ALL
                SELECT de.child_clip_hash, de.parent_clip_hash, de.transformation_type, l.depth + 1
                FROM derivation_edges de
                JOIN lineage l ON de.child_clip_hash = l.parent_hash
                WHERE l.depth < 100
            )
            SELECT clip_hash, parent_hash, transformation_type, depth FROM lineage ORDER BY depth",
        )?;
        let rows = stmt.query_map(params![clip_hash], |row| {
            Ok(LineageNode {
                clip_hash: row.get(0)?,
                parent_hash: row.get(1)?,
                transformation_type: row.get(2)?,
                depth: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_source_refs(&self, clip_hash: &str) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT source_ref FROM clip_source_refs WHERE clip_hash = ?1")?;
        let rows = stmt.query_map(params![clip_hash], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_clip_full(&self, clip_hash: &str) -> Result<Option<Clip>, StoreError> {
        let row = self.find_clip_by_hash(clip_hash)?;
        match row {
            None => Ok(None),
            Some(row) => {
                let source_refs = self.get_source_refs(clip_hash)?;
                Ok(Some(Clip {
                    clip_hash: cliproot_core::ContentHash(row.clip_hash),
                    id: row.id.map(|s| cliproot_core::CrpId(s)),
                    document_id: row.document_id.map(|s| cliproot_core::CrpId(s)),
                    source_refs,
                    selectors: None, // selectors not stored in index
                    content: row.content,
                    text_hash: cliproot_core::ContentHash(row.text_hash),
                    created_by_activity_id: None,
                }))
            }
        }
    }

    pub fn get_edges_for_clip(&self, clip_hash: &str) -> Result<Vec<DerivationEdge>, StoreError> {
        let edges = self.find_derivation_parents(clip_hash)?;
        Ok(edges
            .into_iter()
            .map(|e| DerivationEdge {
                id: cliproot_core::CrpId(e.id),
                child_clip_hash: cliproot_core::ContentHash(e.child_clip_hash),
                parent_clip_hash: cliproot_core::ContentHash(e.parent_clip_hash),
                transformation_type: serde_json::from_value(
                    serde_json::Value::String(e.transformation_type),
                )
                .unwrap_or(cliproot_core::TransformationType::Unknown),
                agent_id: e.agent_id.map(|s| cliproot_core::CrpId(s)),
                confidence: e.confidence,
                created_at: e.created_at,
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct ClipRow {
    pub clip_hash: String,
    pub id: Option<String>,
    pub document_id: Option<String>,
    pub text_hash: String,
    pub content: Option<String>,
    pub bundle_hash: String,
}

#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub id: String,
    pub child_clip_hash: String,
    pub parent_clip_hash: String,
    pub transformation_type: String,
    pub agent_id: Option<String>,
    pub confidence: Option<f64>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct LineageNode {
    pub clip_hash: String,
    pub parent_hash: String,
    pub transformation_type: String,
    pub depth: u32,
}
