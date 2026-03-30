use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use cliproot_core::{Artifact, Clip, ClipArtifactRef, CrpBundle, Edge, Project};

use crate::error::StoreError;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE IF NOT EXISTS clips (
    clip_hash TEXT PRIMARY KEY,
    id TEXT,
    project_id TEXT,
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

CREATE TABLE IF NOT EXISTS edges (
    id TEXT PRIMARY KEY,
    edge_type TEXT NOT NULL,
    subject_ref TEXT NOT NULL,
    object_ref TEXT NOT NULL,
    transformation_type TEXT,
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
    project_id TEXT,
    activity_type TEXT NOT NULL,
    agent_id TEXT,
    prompt TEXT,
    parameters TEXT,
    created_at TEXT NOT NULL,
    ended_at TEXT
);

CREATE TABLE IF NOT EXISTS artifacts (
    artifact_hash TEXT PRIMARY KEY,
    id TEXT,
    project_id TEXT,
    artifact_type TEXT NOT NULL,
    file_name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    metadata TEXT,
    created_at TEXT,
    bundle_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS clip_artifact_refs (
    clip_hash TEXT NOT NULL,
    artifact_hash TEXT NOT NULL,
    relationship TEXT NOT NULL,
    PRIMARY KEY (clip_hash, artifact_hash, relationship)
);

CREATE INDEX IF NOT EXISTS idx_edges_subject ON edges(subject_ref);
CREATE INDEX IF NOT EXISTS idx_edges_object ON edges(object_ref);
CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
CREATE INDEX IF NOT EXISTS idx_clips_document ON clips(document_id);
CREATE INDEX IF NOT EXISTS idx_clips_id ON clips(id);
CREATE INDEX IF NOT EXISTS idx_clips_project ON clips(project_id);
CREATE INDEX IF NOT EXISTS idx_activities_project ON activities(project_id);
CREATE INDEX IF NOT EXISTS idx_artifacts_project ON artifacts(project_id);
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
        self.add_column_if_missing("clips", "project_id", "TEXT")?;
        self.add_column_if_missing("activities", "project_id", "TEXT")?;
        self.add_column_if_missing("activities", "prompt", "TEXT")?;
        self.add_column_if_missing("activities", "parameters", "TEXT")?;
        self.add_column_if_missing("activities", "ended_at", "TEXT")?;
        Ok(())
    }

    fn add_column_if_missing(
        &self,
        table: &str,
        column: &str,
        definition: &str,
    ) -> Result<(), StoreError> {
        let pragma = format!("PRAGMA table_info({table})");
        let mut stmt = self.conn.prepare(&pragma)?;
        let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
        for column_name in columns {
            if column_name? == column {
                return Ok(());
            }
        }
        self.conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
        Ok(())
    }

    pub fn index_bundle(&self, bundle: &CrpBundle, bundle_hash: &str) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;

        if let Some(project) = &bundle.project {
            tx.execute(
                "INSERT OR REPLACE INTO projects (id, name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    project.id.0,
                    project.name,
                    project.description,
                    project.created_at,
                    project.updated_at,
                ],
            )?;
        }

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

        for agent in &bundle.agents {
            tx.execute(
                "INSERT OR REPLACE INTO agents (id, agent_type, name, uri) VALUES (?1, ?2, ?3, ?4)",
                params![
                    agent.id.0,
                    serde_json::to_value(&agent.agent_type)?
                        .as_str()
                        .unwrap_or(""),
                    agent.name,
                    agent.uri,
                ],
            )?;
        }

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

        for clip in &bundle.clips {
            tx.execute(
                "INSERT OR REPLACE INTO clips (clip_hash, id, project_id, document_id, text_hash, content, bundle_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    clip.clip_hash.0,
                    clip.id.as_ref().map(|i| &i.0),
                    clip.project_id.as_ref().map(|i| &i.0),
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

        for edge in &bundle.edges {
            tx.execute(
                "INSERT OR REPLACE INTO edges (id, edge_type, subject_ref, object_ref, transformation_type, agent_id, confidence, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    edge.id.0,
                    serde_json::to_value(&edge.edge_type)?.as_str().unwrap_or(""),
                    edge.subject_ref.0,
                    edge.object_ref.0,
                    edge.transformation_type
                        .as_ref()
                        .and_then(|t| serde_json::to_value(t).ok())
                        .and_then(|v| v.as_str().map(|s| s.to_string())),
                    edge.agent_id.as_ref().map(|a| &a.0),
                    edge.confidence,
                    edge.created_at,
                ],
            )?;
        }

        for activity in &bundle.activities {
            tx.execute(
                "INSERT OR REPLACE INTO activities (id, project_id, activity_type, agent_id, prompt, parameters, created_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    activity.id.0,
                    activity.project_id.as_ref().map(|p| &p.0),
                    serde_json::to_value(&activity.activity_type)?.as_str().unwrap_or(""),
                    activity.agent_id.as_ref().map(|a| &a.0),
                    activity.prompt,
                    activity.parameters.as_ref().map(serde_json::to_string).transpose()?,
                    activity.created_at,
                    activity.ended_at,
                ],
            )?;
        }

        for artifact in &bundle.artifacts {
            tx.execute(
                "INSERT OR REPLACE INTO artifacts (artifact_hash, id, project_id, artifact_type, file_name, mime_type, byte_size, metadata, created_at, bundle_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    artifact.artifact_hash.0,
                    artifact.id.as_ref().map(|i| &i.0),
                    artifact.project_id.as_ref().map(|p| &p.0),
                    serde_json::to_value(&artifact.artifact_type)?.as_str().unwrap_or(""),
                    artifact.file_name,
                    artifact.mime_type,
                    artifact.byte_size as i64,
                    artifact.metadata.as_ref().map(serde_json::to_string).transpose()?,
                    artifact.created_at,
                    bundle_hash,
                ],
            )?;
        }

        for link in &bundle.clip_artifact_refs {
            tx.execute(
                "INSERT OR REPLACE INTO clip_artifact_refs (clip_hash, artifact_hash, relationship) VALUES (?1, ?2, ?3)",
                params![
                    link.clip_hash.0,
                    link.artifact_hash.0,
                    serde_json::to_value(&link.relationship)?.as_str().unwrap_or("unknown"),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn upsert_project(&self, project: &Project) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO projects (id, name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                project.id.0,
                project.name,
                project.description,
                project.created_at,
                project.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, created_at, updated_at FROM projects ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Project {
                id: cliproot_core::CrpId(row.get(0)?),
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_project_by_id(&self, project_id: &str) -> Result<Option<Project>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, created_at, updated_at FROM projects WHERE id = ?1",
        )?;
        stmt.query_row(params![project_id], |row| {
            Ok(Project {
                id: cliproot_core::CrpId(row.get(0)?),
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .optional()
        .map_err(Into::into)
    }

    pub fn delete_project(&self, project_id: &str) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", params![project_id])?;
        Ok(())
    }

    pub fn find_clip_by_hash(&self, hash: &str) -> Result<Option<ClipRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT clip_hash, id, project_id, document_id, text_hash, content, bundle_hash FROM clips WHERE clip_hash = ?1",
        )?;
        stmt.query_row(params![hash], |row| {
            Ok(ClipRow {
                clip_hash: row.get(0)?,
                id: row.get(1)?,
                project_id: row.get(2)?,
                document_id: row.get(3)?,
                text_hash: row.get(4)?,
                content: row.get(5)?,
                bundle_hash: row.get(6)?,
            })
        })
        .optional()
        .map_err(Into::into)
    }

    pub fn find_clip_by_id(&self, id: &str) -> Result<Option<ClipRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT clip_hash, id, project_id, document_id, text_hash, content, bundle_hash FROM clips WHERE id = ?1",
        )?;
        stmt.query_row(params![id], |row| {
            Ok(ClipRow {
                clip_hash: row.get(0)?,
                id: row.get(1)?,
                project_id: row.get(2)?,
                document_id: row.get(3)?,
                text_hash: row.get(4)?,
                content: row.get(5)?,
                bundle_hash: row.get(6)?,
            })
        })
        .optional()
        .map_err(Into::into)
    }

    pub fn list_clips(
        &self,
        document_id: Option<&str>,
        source_type: Option<&str>,
        project_id: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<ClipRow>, StoreError> {
        let limit = limit.unwrap_or(100);

        if let Some(doc_id) = document_id {
            let mut stmt = self.conn.prepare(
                "SELECT clip_hash, id, project_id, document_id, text_hash, content, bundle_hash FROM clips WHERE document_id = ?1 AND (?2 IS NULL OR project_id = ?2) LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![doc_id, project_id, limit], |row| {
                Ok(ClipRow {
                    clip_hash: row.get(0)?,
                    id: row.get(1)?,
                    project_id: row.get(2)?,
                    document_id: row.get(3)?,
                    text_hash: row.get(4)?,
                    content: row.get(5)?,
                    bundle_hash: row.get(6)?,
                })
            })?;
            return rows.collect::<Result<Vec<_>, _>>().map_err(Into::into);
        }

        if let Some(src_type) = source_type {
            let mut stmt = self.conn.prepare(
                "SELECT DISTINCT c.clip_hash, c.id, c.project_id, c.document_id, c.text_hash, c.content, c.bundle_hash
                 FROM clips c
                 JOIN clip_source_refs csr ON c.clip_hash = csr.clip_hash
                 JOIN sources s ON csr.source_ref = s.id
                 WHERE s.source_type = ?1 AND (?2 IS NULL OR c.project_id = ?2)
                 LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![src_type, project_id, limit], |row| {
                Ok(ClipRow {
                    clip_hash: row.get(0)?,
                    id: row.get(1)?,
                    project_id: row.get(2)?,
                    document_id: row.get(3)?,
                    text_hash: row.get(4)?,
                    content: row.get(5)?,
                    bundle_hash: row.get(6)?,
                })
            })?;
            return rows.collect::<Result<Vec<_>, _>>().map_err(Into::into);
        }

        let mut stmt = self.conn.prepare(
            "SELECT clip_hash, id, project_id, document_id, text_hash, content, bundle_hash
             FROM clips
             WHERE (?1 IS NULL OR project_id = ?1)
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![project_id, limit], |row| {
            Ok(ClipRow {
                clip_hash: row.get(0)?,
                id: row.get(1)?,
                project_id: row.get(2)?,
                document_id: row.get(3)?,
                text_hash: row.get(4)?,
                content: row.get(5)?,
                bundle_hash: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn find_derivation_parents(&self, clip_hash: &str) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, edge_type, subject_ref, object_ref, transformation_type, agent_id, confidence, created_at
             FROM edges WHERE edge_type = 'wasDerivedFrom' AND subject_ref = ?1",
        )?;
        let rows = stmt.query_map(params![clip_hash], |row| {
            Ok(EdgeRow {
                id: row.get(0)?,
                edge_type: row.get(1)?,
                subject_ref: row.get(2)?,
                object_ref: row.get(3)?,
                transformation_type: row.get(4)?,
                agent_id: row.get(5)?,
                confidence: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn trace_lineage(&self, clip_hash: &str) -> Result<Vec<LineageNode>, StoreError> {
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE lineage(subject_ref, object_ref, transformation_type, depth) AS (
                SELECT subject_ref, object_ref, transformation_type, 1
                FROM edges
                WHERE edge_type = 'wasDerivedFrom' AND subject_ref = ?1
                UNION ALL
                SELECT e.subject_ref, e.object_ref, e.transformation_type, l.depth + 1
                FROM edges e
                JOIN lineage l ON e.subject_ref = l.object_ref
                WHERE e.edge_type = 'wasDerivedFrom' AND l.depth < 100
            )
            SELECT subject_ref, object_ref, transformation_type, depth FROM lineage ORDER BY depth",
        )?;
        let rows = stmt.query_map(params![clip_hash], |row| {
            Ok(LineageNode {
                clip_hash: row.get(0)?,
                parent_hash: row.get(1)?,
                transformation_type: row
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "unknown".to_string()),
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
                    id: row.id.map(cliproot_core::CrpId),
                    project_id: row.project_id.map(cliproot_core::CrpId),
                    document_id: row.document_id.map(cliproot_core::CrpId),
                    source_refs,
                    selectors: None,
                    content: row.content,
                    text_hash: cliproot_core::ContentHash(row.text_hash),
                    created_by_activity_id: None,
                }))
            }
        }
    }

    pub fn get_edges_for_subject(&self, subject_ref: &str) -> Result<Vec<Edge>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, edge_type, subject_ref, object_ref, transformation_type, agent_id, confidence, created_at FROM edges WHERE subject_ref = ?1",
        )?;
        let rows = stmt.query_map(params![subject_ref], |row| {
            Ok(Edge {
                id: cliproot_core::CrpId(row.get(0)?),
                edge_type: serde_json::from_value(serde_json::Value::String(
                    row.get::<_, String>(1)?,
                ))
                .unwrap_or(cliproot_core::EdgeType::WasDerivedFrom),
                subject_ref: cliproot_core::CrpId(row.get(2)?),
                object_ref: cliproot_core::CrpId(row.get(3)?),
                transformation_type: row.get::<_, Option<String>>(4)?.map(|t| {
                    serde_json::from_value(serde_json::Value::String(t))
                        .unwrap_or(cliproot_core::TransformationType::Unknown)
                }),
                agent_id: row.get::<_, Option<String>>(5)?.map(cliproot_core::CrpId),
                confidence: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_source_by_id(&self, source_id: &str) -> Result<Option<SourceRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, source_type, title, source_uri FROM sources WHERE id = ?1")?;
        stmt.query_row(params![source_id], |row| {
            Ok(SourceRow {
                id: row.get(0)?,
                source_type: row.get(1)?,
                title: row.get(2)?,
                source_uri: row.get(3)?,
            })
        })
        .optional()
        .map_err(Into::into)
    }

    pub fn get_artifact_by_hash(
        &self,
        artifact_hash: &str,
    ) -> Result<Option<Artifact>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT artifact_hash, id, project_id, artifact_type, file_name, mime_type, byte_size, metadata, created_at FROM artifacts WHERE artifact_hash = ?1",
        )?;
        stmt.query_row(params![artifact_hash], |row| {
            Ok(Artifact {
                artifact_hash: cliproot_core::ContentHash(row.get(0)?),
                id: row.get::<_, Option<String>>(1)?.map(cliproot_core::CrpId),
                project_id: row.get::<_, Option<String>>(2)?.map(cliproot_core::CrpId),
                artifact_type: serde_json::from_value(serde_json::Value::String(
                    row.get::<_, String>(3)?,
                ))
                .unwrap_or(cliproot_core::ArtifactType::Unknown),
                file_name: row.get(4)?,
                mime_type: row.get(5)?,
                byte_size: row.get::<_, i64>(6)? as u64,
                metadata: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|json| serde_json::from_str(&json).ok()),
                content_base64: None,
                created_at: row.get(8)?,
            })
        })
        .optional()
        .map_err(Into::into)
    }

    pub fn list_artifacts(&self, project_id: Option<&str>) -> Result<Vec<Artifact>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT artifact_hash, id, project_id, artifact_type, file_name, mime_type, byte_size, metadata, created_at
             FROM artifacts
             WHERE (?1 IS NULL OR project_id = ?1)
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(Artifact {
                artifact_hash: cliproot_core::ContentHash(row.get(0)?),
                id: row.get::<_, Option<String>>(1)?.map(cliproot_core::CrpId),
                project_id: row.get::<_, Option<String>>(2)?.map(cliproot_core::CrpId),
                artifact_type: serde_json::from_value(serde_json::Value::String(
                    row.get::<_, String>(3)?,
                ))
                .unwrap_or(cliproot_core::ArtifactType::Unknown),
                file_name: row.get(4)?,
                mime_type: row.get(5)?,
                byte_size: row.get::<_, i64>(6)? as u64,
                metadata: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|json| serde_json::from_str(&json).ok()),
                content_base64: None,
                created_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_artifact(
        &self,
        artifact: &Artifact,
        bundle_hash: &str,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO artifacts (artifact_hash, id, project_id, artifact_type, file_name, mime_type, byte_size, metadata, created_at, bundle_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                artifact.artifact_hash.0,
                artifact.id.as_ref().map(|i| &i.0),
                artifact.project_id.as_ref().map(|p| &p.0),
                serde_json::to_value(&artifact.artifact_type)?.as_str().unwrap_or("unknown"),
                artifact.file_name,
                artifact.mime_type,
                artifact.byte_size as i64,
                artifact.metadata.as_ref().map(serde_json::to_string).transpose()?,
                artifact.created_at,
                bundle_hash,
            ],
        )?;
        Ok(())
    }

    pub fn link_clip_artifact(&self, link: &ClipArtifactRef) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO clip_artifact_refs (clip_hash, artifact_hash, relationship) VALUES (?1, ?2, ?3)",
            params![
                link.clip_hash.0,
                link.artifact_hash.0,
                serde_json::to_value(&link.relationship)?.as_str().unwrap_or("unknown"),
            ],
        )?;
        Ok(())
    }

    pub fn get_clip_artifact_refs_for_clip(
        &self,
        clip_hash: &str,
    ) -> Result<Vec<ClipArtifactRef>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT clip_hash, artifact_hash, relationship FROM clip_artifact_refs WHERE clip_hash = ?1",
        )?;
        let rows = stmt.query_map(params![clip_hash], |row| {
            Ok(ClipArtifactRef {
                clip_hash: cliproot_core::ContentHash(row.get(0)?),
                artifact_hash: cliproot_core::ContentHash(row.get(1)?),
                relationship: serde_json::from_value(serde_json::Value::String(
                    row.get::<_, String>(2)?,
                ))
                .unwrap_or(cliproot_core::ClipArtifactRelationship::Unknown),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[derive(Debug, Clone)]
pub struct SourceRow {
    pub id: String,
    pub source_type: String,
    pub title: Option<String>,
    pub source_uri: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClipRow {
    pub clip_hash: String,
    pub id: Option<String>,
    pub project_id: Option<String>,
    pub document_id: Option<String>,
    pub text_hash: String,
    pub content: Option<String>,
    pub bundle_hash: String,
}

#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub id: String,
    pub edge_type: String,
    pub subject_ref: String,
    pub object_ref: String,
    pub transformation_type: Option<String>,
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
