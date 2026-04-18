//! Actor that owns a Repository on a dedicated blocking thread.
//!
//! `Repository` holds a rusqlite::Connection which is !Send, so it cannot be
//! shared across .await points. This module wraps it in an mpsc-channel actor:
//! a blocking OS thread owns the Repository exclusively and processes commands
//! sent from async tool handlers.

use std::path::PathBuf;

use cliproot_core::matching::{AnnotateResult, AnnotationStyle, Citation, DoctorResult};
use cliproot_core::model::{
    Activity, Artifact, ArtifactType, Clip, ClipArtifactRef, ClipArtifactRelationship, CrpBundle,
    Project,
};
use cliproot_store::index_db::LineageNode;
use cliproot_store::{PackManifest, Repository, SessionRecord, StoreError};
use tokio::sync::{mpsc, oneshot};

type Tx<T> = oneshot::Sender<Result<T, StoreError>>;

pub enum RepoCmd {
    StoreBundle {
        bundle: Box<CrpBundle>,
        tx: Tx<String>,
    },
    GetClip {
        hash_or_id: String,
        tx: Tx<Option<Clip>>,
    },
    #[allow(dead_code)]
    GetClipFull {
        hash_or_id: String,
        tx: Tx<Option<Clip>>,
    },
    ResolveHash {
        hash_or_id: String,
        tx: Tx<Option<String>>,
    },
    ListClips {
        document_id: Option<String>,
        source_type: Option<String>,
        project_id: Option<String>,
        limit: Option<u32>,
        tx: Tx<Vec<Clip>>,
    },
    Trace {
        hash_or_id: String,
        tx: Tx<Vec<LineageNode>>,
    },
    VerifyClip {
        hash_or_id: String,
        tx: Tx<()>,
    },
    VerifyAll {
        tx: Tx<Vec<String>>,
    },
    ExportBundle {
        hash_or_id: String,
        tx: Tx<CrpBundle>,
    },
    Annotate {
        document_text: String,
        style: AnnotationStyle,
        threshold: f64,
        tx: Tx<AnnotateResult>,
    },
    Cite {
        document_text: String,
        threshold: f64,
        tx: Tx<Vec<Citation>>,
    },
    Doctor {
        document_text: String,
        threshold: f64,
        tx: Tx<DoctorResult>,
    },
    CreateProject {
        id: String,
        name: String,
        description: Option<String>,
        tx: Tx<Project>,
    },
    ListProjects {
        tx: Tx<Vec<Project>>,
    },
    UseProject {
        project_id: String,
        tx: Tx<()>,
    },
    DeleteProject {
        project_id: String,
        tx: Tx<()>,
    },
    AddArtifact {
        path: Option<PathBuf>,
        content: Option<Vec<u8>>,
        file_name: Option<String>,
        artifact_type: ArtifactType,
        mime_type: Option<String>,
        id: Option<String>,
        project_id: Option<String>,
        metadata: Option<serde_json::Value>,
        tx: Tx<Artifact>,
    },
    ListArtifacts {
        project_id: Option<String>,
        tx: Tx<Vec<Artifact>>,
    },
    GetArtifact {
        artifact_hash: String,
        tx: Tx<Option<Artifact>>,
    },
    GetArtifactBytes {
        artifact_hash: String,
        tx: Tx<Vec<u8>>,
    },
    LinkClipArtifact {
        clip_hash_or_id: String,
        artifact_hash: String,
        relationship: ClipArtifactRelationship,
        tx: Tx<ClipArtifactRef>,
    },
    CreatePack {
        project_id: Option<String>,
        roots: Vec<String>,
        depth: Option<u32>,
        output_path: PathBuf,
        tx: Tx<PackManifest>,
    },
    ImportPack {
        path: PathBuf,
        restore_artifacts_to: Option<PathBuf>,
        tx: Tx<PackManifest>,
    },
    InspectPack {
        path: PathBuf,
        tx: Tx<PackManifest>,
    },
    VerifyPack {
        path: PathBuf,
        tx: Tx<PackManifest>,
    },
    StartActivity {
        activity_type: cliproot_core::ActivityType,
        project_id: Option<String>,
        agent_id: Option<String>,
        prompt: Option<String>,
        parameters: Option<serde_json::Value>,
        session_id: Option<String>,
        tx: Tx<Activity>,
    },
    EndActivity {
        activity_id: String,
        tx: Tx<Activity>,
    },
    StartSession {
        project_id: Option<String>,
        agent_id: Option<String>,
        metadata: Option<serde_json::Value>,
        tx: Tx<SessionRecord>,
    },
    EndSession {
        session_id: String,
        tx: Tx<SessionRecord>,
    },
    RecordClipTracking {
        clip_hash: String,
        activity_id: Option<String>,
        session_id: Option<String>,
        used_refs: Vec<String>,
        tx: Tx<()>,
    },
}

/// Send + Sync + Clone handle to the blocking Repository thread.
#[derive(Clone)]
pub struct RepoHandle {
    tx: mpsc::Sender<RepoCmd>,
}

impl RepoHandle {
    /// Spawn a dedicated OS thread that owns the `Repository` and processes commands.
    ///
    /// Accepts a `Result` so the MCP server can start even when no repository is
    /// available — every command will reply with the stored error message until
    /// the server is restarted against a valid repo.
    pub fn spawn(repo: Result<Repository, StoreError>) -> Self {
        let (tx, mut rx) = mpsc::channel::<RepoCmd>(32);
        std::thread::spawn(move || {
            let (repo_opt, err_msg) = match repo {
                Ok(r) => (Some(r), None),
                Err(e) => (None, Some(e.to_string())),
            };
            while let Some(cmd) = rx.blocking_recv() {
                let repo = match &repo_opt {
                    Some(r) => r,
                    None => {
                        reply_unavailable(
                            cmd,
                            err_msg
                                .as_deref()
                                .unwrap_or("no cliproot repository configured"),
                        );
                        continue;
                    }
                };
                match cmd {
                    RepoCmd::StoreBundle { bundle, tx } => {
                        let _ = tx.send(repo.store_bundle(&bundle));
                    }
                    RepoCmd::GetClip { hash_or_id, tx } => {
                        let _ = tx.send(repo.get_clip(&hash_or_id));
                    }
                    RepoCmd::GetClipFull { hash_or_id, tx } => {
                        let _ = tx.send(repo.get_clip_full(&hash_or_id));
                    }
                    RepoCmd::ResolveHash { hash_or_id, tx } => {
                        let _ = tx.send(repo.resolve_clip_hash(&hash_or_id));
                    }
                    RepoCmd::ListClips {
                        document_id,
                        source_type,
                        project_id,
                        limit,
                        tx,
                    } => {
                        let _ = tx.send(repo.list_clips(
                            document_id.as_deref(),
                            source_type.as_deref(),
                            project_id.as_deref(),
                            limit,
                        ));
                    }
                    RepoCmd::Trace { hash_or_id, tx } => {
                        let _ = tx.send(repo.trace(&hash_or_id));
                    }
                    RepoCmd::VerifyClip { hash_or_id, tx } => {
                        let _ = tx.send(repo.verify_clip(&hash_or_id));
                    }
                    RepoCmd::VerifyAll { tx } => {
                        let _ = tx.send(repo.verify_all());
                    }
                    RepoCmd::ExportBundle { hash_or_id, tx } => {
                        let _ = tx.send(repo.export_bundle(&hash_or_id));
                    }
                    RepoCmd::Annotate {
                        document_text,
                        style,
                        threshold,
                        tx,
                    } => {
                        let _ = tx.send(repo.annotate(&document_text, style, threshold));
                    }
                    RepoCmd::Cite {
                        document_text,
                        threshold,
                        tx,
                    } => {
                        let _ = tx.send(repo.cite(&document_text, threshold));
                    }
                    RepoCmd::Doctor {
                        document_text,
                        threshold,
                        tx,
                    } => {
                        let _ = tx.send(repo.doctor(&document_text, threshold));
                    }
                    RepoCmd::CreateProject {
                        id,
                        name,
                        description,
                        tx,
                    } => {
                        let _ = tx.send(repo.create_project(&id, &name, description));
                    }
                    RepoCmd::ListProjects { tx } => {
                        let _ = tx.send(repo.list_projects());
                    }
                    RepoCmd::UseProject { project_id, tx } => {
                        let _ = tx.send(repo.use_project(&project_id));
                    }
                    RepoCmd::DeleteProject { project_id, tx } => {
                        let _ = tx.send(repo.delete_project(&project_id));
                    }
                    RepoCmd::AddArtifact {
                        path,
                        content,
                        file_name,
                        artifact_type,
                        mime_type,
                        id,
                        project_id,
                        metadata,
                        tx,
                    } => {
                        let _ = tx.send(repo.add_artifact(
                            path.as_deref(),
                            content.as_deref(),
                            file_name.as_deref(),
                            artifact_type,
                            mime_type.as_deref(),
                            id.as_deref(),
                            project_id.as_deref(),
                            metadata,
                        ));
                    }
                    RepoCmd::ListArtifacts { project_id, tx } => {
                        let _ = tx.send(repo.list_artifacts(project_id.as_deref()));
                    }
                    RepoCmd::GetArtifact { artifact_hash, tx } => {
                        let _ = tx.send(repo.get_artifact(&artifact_hash));
                    }
                    RepoCmd::GetArtifactBytes { artifact_hash, tx } => {
                        let _ = tx.send(repo.get_artifact_bytes(&artifact_hash));
                    }
                    RepoCmd::LinkClipArtifact {
                        clip_hash_or_id,
                        artifact_hash,
                        relationship,
                        tx,
                    } => {
                        let _ = tx.send(repo.link_clip_artifact(
                            &clip_hash_or_id,
                            &artifact_hash,
                            relationship,
                        ));
                    }
                    RepoCmd::CreatePack {
                        project_id,
                        roots,
                        depth,
                        output_path,
                        tx,
                    } => {
                        let _ = tx.send(repo.create_pack(
                            project_id.as_deref(),
                            &roots,
                            depth,
                            &output_path,
                        ));
                    }
                    RepoCmd::ImportPack {
                        path,
                        restore_artifacts_to,
                        tx,
                    } => {
                        let _ = tx.send(repo.import_pack(&path, restore_artifacts_to.as_deref()));
                    }
                    RepoCmd::InspectPack { path, tx } => {
                        let _ = tx.send(Repository::inspect_pack(&path));
                    }
                    RepoCmd::VerifyPack { path, tx } => {
                        let _ = tx.send(Repository::verify_pack(&path));
                    }
                    RepoCmd::StartActivity {
                        activity_type,
                        project_id,
                        agent_id,
                        prompt,
                        parameters,
                        session_id,
                        tx,
                    } => {
                        let _ = tx.send(repo.start_activity(
                            activity_type,
                            project_id.as_deref(),
                            agent_id.as_deref(),
                            prompt,
                            parameters,
                            session_id.as_deref(),
                        ));
                    }
                    RepoCmd::EndActivity { activity_id, tx } => {
                        let _ = tx.send(repo.end_activity(&activity_id));
                    }
                    RepoCmd::StartSession {
                        project_id,
                        agent_id,
                        metadata,
                        tx,
                    } => {
                        let _ = tx.send(repo.start_session(
                            project_id.as_deref(),
                            agent_id.as_deref(),
                            metadata,
                        ));
                    }
                    RepoCmd::EndSession { session_id, tx } => {
                        let _ = tx.send(repo.end_session(&session_id));
                    }
                    RepoCmd::RecordClipTracking {
                        clip_hash,
                        activity_id,
                        session_id,
                        used_refs,
                        tx,
                    } => {
                        let _ = tx.send(repo.record_clip_tracking(
                            &clip_hash,
                            activity_id.as_deref(),
                            session_id.as_deref(),
                            &used_refs,
                        ));
                    }
                }
            }
        });
        Self { tx }
    }

    async fn send<T>(
        &self,
        cmd: RepoCmd,
        rx: oneshot::Receiver<Result<T, StoreError>>,
    ) -> Result<T, StoreError> {
        self.tx
            .send(cmd)
            .await
            .map_err(|_| StoreError::Other("repo thread gone".into()))?;
        rx.await
            .map_err(|_| StoreError::Other("repo thread dropped response".into()))?
    }

    pub async fn store_bundle(&self, bundle: CrpBundle) -> Result<String, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::StoreBundle {
                bundle: Box::new(bundle),
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn get_clip(&self, hash_or_id: String) -> Result<Option<Clip>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::GetClip { hash_or_id, tx }, rx).await
    }

    pub async fn resolve_hash(&self, hash_or_id: String) -> Result<Option<String>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::ResolveHash { hash_or_id, tx }, rx).await
    }

    pub async fn list_clips(
        &self,
        document_id: Option<String>,
        source_type: Option<String>,
        project_id: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<Clip>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::ListClips {
                document_id,
                source_type,
                project_id,
                limit,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn trace(&self, hash_or_id: String) -> Result<Vec<LineageNode>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::Trace { hash_or_id, tx }, rx).await
    }

    pub async fn verify_clip(&self, hash_or_id: String) -> Result<(), StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::VerifyClip { hash_or_id, tx }, rx).await
    }

    pub async fn verify_all(&self) -> Result<Vec<String>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::VerifyAll { tx }, rx).await
    }

    pub async fn export_bundle(&self, hash_or_id: String) -> Result<CrpBundle, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::ExportBundle { hash_or_id, tx }, rx)
            .await
    }

    pub async fn annotate(
        &self,
        document_text: String,
        style: AnnotationStyle,
        threshold: f64,
    ) -> Result<AnnotateResult, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::Annotate {
                document_text,
                style,
                threshold,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn cite(
        &self,
        document_text: String,
        threshold: f64,
    ) -> Result<Vec<Citation>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::Cite {
                document_text,
                threshold,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn doctor(
        &self,
        document_text: String,
        threshold: f64,
    ) -> Result<DoctorResult, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::Doctor {
                document_text,
                threshold,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn create_project(
        &self,
        id: String,
        name: String,
        description: Option<String>,
    ) -> Result<Project, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::CreateProject {
                id,
                name,
                description,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::ListProjects { tx }, rx).await
    }

    pub async fn use_project(&self, project_id: String) -> Result<(), StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::UseProject { project_id, tx }, rx).await
    }

    pub async fn delete_project(&self, project_id: String) -> Result<(), StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::DeleteProject { project_id, tx }, rx)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_artifact(
        &self,
        path: Option<PathBuf>,
        content: Option<Vec<u8>>,
        file_name: Option<String>,
        artifact_type: ArtifactType,
        mime_type: Option<String>,
        id: Option<String>,
        project_id: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> Result<Artifact, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::AddArtifact {
                path,
                content,
                file_name,
                artifact_type,
                mime_type,
                id,
                project_id,
                metadata,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn list_artifacts(
        &self,
        project_id: Option<String>,
    ) -> Result<Vec<Artifact>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::ListArtifacts { project_id, tx }, rx)
            .await
    }

    pub async fn get_artifact(
        &self,
        artifact_hash: String,
    ) -> Result<Option<Artifact>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::GetArtifact { artifact_hash, tx }, rx)
            .await
    }

    pub async fn get_artifact_bytes(&self, artifact_hash: String) -> Result<Vec<u8>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::GetArtifactBytes { artifact_hash, tx }, rx)
            .await
    }

    pub async fn link_clip_artifact(
        &self,
        clip_hash_or_id: String,
        artifact_hash: String,
        relationship: ClipArtifactRelationship,
    ) -> Result<ClipArtifactRef, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::LinkClipArtifact {
                clip_hash_or_id,
                artifact_hash,
                relationship,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn create_pack(
        &self,
        project_id: Option<String>,
        roots: Vec<String>,
        depth: Option<u32>,
        output_path: PathBuf,
    ) -> Result<PackManifest, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::CreatePack {
                project_id,
                roots,
                depth,
                output_path,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn import_pack(
        &self,
        path: PathBuf,
        restore_artifacts_to: Option<PathBuf>,
    ) -> Result<PackManifest, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::ImportPack {
                path,
                restore_artifacts_to,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn inspect_pack(&self, path: PathBuf) -> Result<PackManifest, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::InspectPack { path, tx }, rx).await
    }

    pub async fn verify_pack(&self, path: PathBuf) -> Result<PackManifest, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::VerifyPack { path, tx }, rx).await
    }

    pub async fn start_activity(
        &self,
        activity_type: cliproot_core::ActivityType,
        project_id: Option<String>,
        agent_id: Option<String>,
        prompt: Option<String>,
        parameters: Option<serde_json::Value>,
        session_id: Option<String>,
    ) -> Result<Activity, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::StartActivity {
                activity_type,
                project_id,
                agent_id,
                prompt,
                parameters,
                session_id,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn end_activity(&self, activity_id: String) -> Result<Activity, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::EndActivity { activity_id, tx }, rx)
            .await
    }

    pub async fn start_session(
        &self,
        project_id: Option<String>,
        agent_id: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> Result<SessionRecord, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::StartSession {
                project_id,
                agent_id,
                metadata,
                tx,
            },
            rx,
        )
        .await
    }

    pub async fn end_session(&self, session_id: String) -> Result<SessionRecord, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::EndSession { session_id, tx }, rx).await
    }

    pub async fn record_clip_tracking(
        &self,
        clip_hash: String,
        activity_id: Option<String>,
        session_id: Option<String>,
        used_refs: Vec<String>,
    ) -> Result<(), StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(
            RepoCmd::RecordClipTracking {
                clip_hash,
                activity_id,
                session_id,
                used_refs,
                tx,
            },
            rx,
        )
        .await
    }
}

/// Reply to every command variant with an `Other` error carrying `err_msg`.
/// Used when the actor thread could not open a repository: each pending tool
/// call gets a clean error back instead of the handshake hanging on a dead
/// channel.
fn reply_unavailable(cmd: RepoCmd, err_msg: &str) {
    macro_rules! respond {
        ($tx:ident) => {{
            let _ = $tx.send(Err(StoreError::Other(err_msg.to_string())));
        }};
    }
    match cmd {
        RepoCmd::StoreBundle { tx, .. } => respond!(tx),
        RepoCmd::GetClip { tx, .. } => respond!(tx),
        RepoCmd::GetClipFull { tx, .. } => respond!(tx),
        RepoCmd::ResolveHash { tx, .. } => respond!(tx),
        RepoCmd::ListClips { tx, .. } => respond!(tx),
        RepoCmd::Trace { tx, .. } => respond!(tx),
        RepoCmd::VerifyClip { tx, .. } => respond!(tx),
        RepoCmd::VerifyAll { tx } => respond!(tx),
        RepoCmd::ExportBundle { tx, .. } => respond!(tx),
        RepoCmd::Annotate { tx, .. } => respond!(tx),
        RepoCmd::Cite { tx, .. } => respond!(tx),
        RepoCmd::Doctor { tx, .. } => respond!(tx),
        RepoCmd::CreateProject { tx, .. } => respond!(tx),
        RepoCmd::ListProjects { tx } => respond!(tx),
        RepoCmd::UseProject { tx, .. } => respond!(tx),
        RepoCmd::DeleteProject { tx, .. } => respond!(tx),
        RepoCmd::AddArtifact { tx, .. } => respond!(tx),
        RepoCmd::ListArtifacts { tx, .. } => respond!(tx),
        RepoCmd::GetArtifact { tx, .. } => respond!(tx),
        RepoCmd::GetArtifactBytes { tx, .. } => respond!(tx),
        RepoCmd::LinkClipArtifact { tx, .. } => respond!(tx),
        RepoCmd::CreatePack { tx, .. } => respond!(tx),
        RepoCmd::ImportPack { tx, .. } => respond!(tx),
        RepoCmd::InspectPack { tx, .. } => respond!(tx),
        RepoCmd::VerifyPack { tx, .. } => respond!(tx),
        RepoCmd::StartActivity { tx, .. } => respond!(tx),
        RepoCmd::EndActivity { tx, .. } => respond!(tx),
        RepoCmd::StartSession { tx, .. } => respond!(tx),
        RepoCmd::EndSession { tx, .. } => respond!(tx),
        RepoCmd::RecordClipTracking { tx, .. } => respond!(tx),
    }
}
