//! Actor that owns a Repository on a dedicated blocking thread.
//!
//! `Repository` holds a rusqlite::Connection which is !Send, so it cannot be
//! shared across .await points. This module wraps it in an mpsc-channel actor:
//! a blocking OS thread owns the Repository exclusively and processes commands
//! sent from async tool handlers.

use cliproot_core::model::{Clip, CrpBundle};
use cliproot_store::{Repository, StoreError};
use cliproot_store::index_db::LineageNode;
use tokio::sync::{mpsc, oneshot};

type Tx<T> = oneshot::Sender<Result<T, StoreError>>;

pub enum RepoCmd {
    StoreBundle  { bundle: CrpBundle,              tx: Tx<String> },
    GetClip      { hash_or_id: String,             tx: Tx<Option<Clip>> },
    #[allow(dead_code)]
    GetClipFull  { hash_or_id: String,             tx: Tx<Option<Clip>> },
    ResolveHash  { hash_or_id: String,             tx: Tx<Option<String>> },
    ListClips    { document_id: Option<String>, source_type: Option<String>, limit: Option<u32>, tx: Tx<Vec<Clip>> },
    Trace        { hash_or_id: String,             tx: Tx<Vec<LineageNode>> },
    VerifyClip   { hash_or_id: String,             tx: Tx<()> },
    VerifyAll    {                                  tx: Tx<Vec<String>> },
    ExportBundle { hash_or_id: String,             tx: Tx<CrpBundle> },
}

/// Send + Sync + Clone handle to the blocking Repository thread.
#[derive(Clone)]
pub struct RepoHandle {
    tx: mpsc::Sender<RepoCmd>,
}

impl RepoHandle {
    /// Spawn a dedicated OS thread that owns `repo` and processes commands.
    pub fn spawn(repo: Repository) -> Self {
        let (tx, mut rx) = mpsc::channel::<RepoCmd>(32);
        std::thread::spawn(move || {
            while let Some(cmd) = rx.blocking_recv() {
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
                    RepoCmd::ListClips { document_id, source_type, limit, tx } => {
                        let _ = tx.send(repo.list_clips(
                            document_id.as_deref(),
                            source_type.as_deref(),
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
                }
            }
        });
        Self { tx }
    }

    async fn send<T>(&self, cmd: RepoCmd, rx: oneshot::Receiver<Result<T, StoreError>>) -> Result<T, StoreError> {
        self.tx.send(cmd).await
            .map_err(|_| StoreError::Other("repo thread gone".into()))?;
        rx.await.map_err(|_| StoreError::Other("repo thread dropped response".into()))?
    }

    pub async fn store_bundle(&self, bundle: CrpBundle) -> Result<String, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::StoreBundle { bundle, tx }, rx).await
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
        limit: Option<u32>,
    ) -> Result<Vec<Clip>, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.send(RepoCmd::ListClips { document_id, source_type, limit, tx }, rx).await
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
        self.send(RepoCmd::ExportBundle { hash_or_id, tx }, rx).await
    }
}
