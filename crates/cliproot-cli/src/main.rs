mod commands;
mod knowledge;
mod output;
mod skills;
mod transcript;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Table,
}

#[derive(Parser)]
#[command(
    name = "cliproot",
    about = "Local-first provenance engine for content-addressed clips"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Output format
    #[arg(long, default_value = "text", global = true)]
    format: OutputFormat,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a .cliproot/ repository in the current directory
    Init {
        /// Also generate agent/IDE configuration files (MCP configs, skills, rules)
        #[arg(long)]
        agent: bool,

        /// Install Claude Code PostToolUse hook for automatic tool-call capture
        #[arg(long)]
        hooks: bool,
    },

    /// Capture a PostToolUse hook event (reads JSON from stdin)
    #[command(name = "capture-hook")]
    CaptureHook {
        /// AI harness (claude-code, cursor, codex)
        #[arg(long, value_enum, default_value = "claude-code")]
        harness: commands::harness::Harness,
    },

    /// Surface unclipped sources from agent-log for review
    Consolidate {
        /// Session ID to consolidate
        #[arg(long)]
        session: String,

        /// Process ALL unconsolidated entries and write candidate artifact
        #[arg(long)]
        emergency: bool,

        /// Advance watermark after consolidation
        #[arg(long)]
        commit: bool,
    },

    /// Handle Stop/PreCompact hook events for consolidation (reads JSON from stdin)
    #[command(name = "consolidate-hook")]
    ConsolidateHook {
        /// AI harness (claude-code, cursor, codex)
        #[arg(long, value_enum, default_value = "claude-code")]
        harness: commands::harness::Harness,

        /// Emergency mode for PreCompact hooks
        #[arg(long)]
        emergency: bool,
    },

    /// Handle Stop hook events: spawn a detached background flush process (reads JSON from stdin)
    #[command(name = "flush-hook")]
    FlushHook {
        /// AI harness (claude-code, cursor, codex)
        #[arg(long, value_enum, default_value = "claude-code")]
        harness: commands::harness::Harness,

        /// Internal flag: run flush in the foreground (used by the spawned child)
        #[arg(long, hide = true)]
        background: bool,

        /// Path to .cliproot/ directory (required when --background is set)
        #[arg(long, hide = true)]
        cliproot_dir: Option<std::path::PathBuf>,
    },

    /// Handle Claude Code SessionStart hook events: inject a wiki snapshot as additionalContext
    #[command(name = "session-start-hook")]
    SessionStartHook {
        /// AI harness (claude-code only — other harnesses exit clean)
        #[arg(long, value_enum, default_value = "claude-code")]
        harness: commands::harness::Harness,

        /// Path to .cliproot/ directory (testing override)
        #[arg(long, hide = true)]
        cliproot_dir: Option<std::path::PathBuf>,
    },

    /// Compile today's daily digest into concept/connection/qa wiki articles
    Compile {
        /// Path to .cliproot/ directory (default: walk up from cwd)
        #[arg(long)]
        cliproot_dir: Option<std::path::PathBuf>,

        /// Run compile in a detached background process
        #[arg(long)]
        background: bool,

        /// Internal: we are the detached child — do the work synchronously
        #[arg(long, hide = true)]
        background_child: bool,
    },

    /// Lint the compiled wiki for structural and provenance invariants
    #[command(name = "wiki-lint")]
    WikiLint {
        /// Path to .cliproot/ directory (default: walk up from cwd)
        #[arg(long)]
        cliproot_dir: Option<std::path::PathBuf>,

        /// Skip the coverage pass (check #8) — run only file-structural checks 1–7
        #[arg(long)]
        structural_only: bool,

        /// Also run check #9 (pairwise contradiction detection, LLM, ~5k tokens)
        #[arg(long)]
        contradictions: bool,

        /// Treat any failing check as an error (exit 1).  Without it, only
        /// broken `[cliproot:sha256-...]` citations fail the run.
        #[arg(long)]
        strict: bool,

        /// Write a timestamped report to `<knowledge_dir>/reports/wiki-lint-YYYY-MM-DD.md`
        #[arg(long)]
        report: bool,
    },

    /// Two-phase retrieval over the compiled wiki
    Query {
        /// The natural-language question to answer
        prompt: String,

        /// Path to .cliproot/ directory (default: walk up from cwd)
        #[arg(long)]
        cliproot_dir: Option<std::path::PathBuf>,

        /// Persist the answer as `qa/<slug>.md` in the knowledge tree
        #[arg(long)]
        file_back: bool,

        /// Upper bound on articles fed to the answer phase
        #[arg(long, default_value = "6")]
        top_k: usize,
    },

    /// Reconstruct a design record from a Claude Code session
    Record {
        /// Claude Code session ID (default: most recent)
        #[arg(long)]
        session: Option<String>,

        /// Explicit Claude Code session directory path
        #[arg(long)]
        session_dir: Option<String>,

        /// Explicit path to session JSONL file
        #[arg(long)]
        jsonl: Option<String>,

        /// Path to hook-generated agent log (default: auto-detect)
        #[arg(long)]
        hook_log: Option<String>,

        /// Include last N sessions (for multi-session explorations)
        #[arg(long)]
        last: Option<u32>,

        /// Cliproot project to scope reconstruction to
        #[arg(long)]
        project: Option<String>,

        /// Include subagent transcripts (default: true)
        #[arg(long, default_value = "true")]
        include_subagents: bool,

        /// Show what would be reconstructed without writing
        #[arg(long)]
        dry_run: bool,

        /// Also create a .cliprootpack archive
        #[arg(long)]
        pack: bool,

        /// Output path for pack or session artifact
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Create a source record + clip from a URL
    Clip {
        /// Source URL
        #[arg(long)]
        url: String,

        /// Quoted text content
        #[arg(long)]
        quote: String,

        /// Source type
        #[arg(long, default_value = "external-quoted")]
        source_type: String,

        /// Optional clip id
        #[arg(long)]
        id: Option<String>,

        /// Optional document id
        #[arg(long)]
        document_id: Option<String>,

        /// Optional project id (falls back to current project)
        #[arg(long)]
        project: Option<String>,

        /// Optional title
        #[arg(long)]
        title: Option<String>,

        /// Optional activity id to associate with this clip
        #[arg(long)]
        activity: Option<String>,

        /// Optional session id to associate with this clip
        #[arg(long)]
        session: Option<String>,

        /// Also copy to the OS clipboard with provenance embedded
        #[arg(long)]
        copy: bool,
    },

    /// Copy a clip to the OS clipboard with embedded provenance
    Copy {
        /// Clip hash or id
        hash_or_id: String,

        /// Plain text only — no provenance metadata
        #[arg(long)]
        plain: bool,
    },

    /// Create a derived clip from one or more parent clips
    Derive {
        /// Parent clip hash or id (can be specified multiple times)
        #[arg(long = "from", required = true)]
        from: Vec<String>,

        /// Text content of derived clip
        #[arg(long)]
        quote: String,

        /// Transformation/activity type (e.g., summary, paraphrase, translate)
        #[arg(long)]
        activity_type: String,

        /// Optional agent id
        #[arg(long)]
        agent: Option<String>,

        /// Optional project id (falls back to current project)
        #[arg(long)]
        project: Option<String>,

        /// Optional activity id to associate with this derived clip
        #[arg(long)]
        activity: Option<String>,

        /// Optional session id to associate with this derived clip
        #[arg(long)]
        session: Option<String>,
    },

    /// Display clip details
    Inspect {
        /// Clip hash or id
        hash_or_id: String,
    },

    /// Show ancestor lineage tree
    Trace {
        /// Clip hash or id
        hash_or_id: String,
    },

    /// Verify hash integrity
    Verify {
        /// Clip hash or id (omit to verify all)
        hash_or_id: Option<String>,
    },

    /// List clips with optional filters
    List {
        /// Filter by document id
        #[arg(long)]
        document: Option<String>,

        /// Filter by source type
        #[arg(long)]
        source_type: Option<String>,

        /// Filter by project id
        #[arg(long)]
        project: Option<String>,

        /// Maximum number of results
        #[arg(long, default_value = "50")]
        limit: u32,
    },

    /// Import a CRP bundle file
    Ingest {
        /// Path to bundle JSON file
        path: String,
    },

    /// Export clip + lineage as CRP bundle
    Export {
        /// Clip hash or id
        hash: String,

        /// Output file path (stdout if omitted)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Insert inline citations into a document by matching text against stored clips
    Annotate {
        /// Path to the document file
        file: String,

        /// Annotation style: footnote (default), inline-comment, bracket
        #[arg(long, default_value = "footnote")]
        style: String,

        /// Modify the file in place instead of writing to stdout
        #[arg(long)]
        in_place: bool,

        /// Minimum match confidence threshold (0.0-1.0)
        #[arg(long, default_value = "0.4")]
        threshold: f64,
    },

    /// Generate a bibliography/citation list from clip provenance
    Cite {
        /// Path to the document file
        file: String,

        /// Minimum match confidence threshold (0.0-1.0)
        #[arg(long, default_value = "0.4")]
        threshold: f64,
    },

    /// Provenance coverage report for a document
    Doctor {
        /// Path to the document file
        file: String,

        /// Minimum match confidence threshold (0.0-1.0)
        #[arg(long, default_value = "0.4")]
        threshold: f64,
    },

    /// Start the MCP stdio server for AI agents
    Mcp {
        /// Path to .cliproot/ repository (defaults to CLIPROOT_REPO or CWD discovery)
        #[arg(long, short)]
        path: Option<String>,
    },

    /// Manage projects
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },

    /// Manage artifacts
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommands,
    },

    /// Create, inspect, verify, and import .cliprootpack archives
    Pack {
        #[command(subcommand)]
        command: PackCommands,
    },

    /// Manage registry remotes
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },

    /// Push a project's provenance to a registry
    Push {
        /// Project id (defaults to current project)
        project: Option<String>,
        /// Remote name (defaults to default remote)
        #[arg(long)]
        remote: Option<String>,
    },

    /// Pull a project's provenance from a registry
    Pull {
        /// Project name on the registry
        project: Option<String>,
        /// Remote name (defaults to default remote)
        #[arg(long)]
        remote: Option<String>,
    },

    /// Search clips on a remote registry
    Search {
        /// Search query
        query: String,
        /// Remote name (defaults to default remote)
        #[arg(long)]
        remote: Option<String>,
        /// Maximum results
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    /// Authenticate with a registry
    Login {
        /// Use a pre-existing token instead of device flow (for CI)
        #[arg(long)]
        token: Option<String>,
        /// Remote name (defaults to default remote)
        #[arg(long)]
        remote: Option<String>,
    },

    /// Log out from a registry
    Logout {
        /// Remote name (defaults to default remote)
        #[arg(long)]
        remote: Option<String>,
    },

    /// Track prompt-scoped activities
    Activity {
        #[command(subcommand)]
        command: ActivityCommands,
    },

    /// Track agent sessions that can be restored as artifacts
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },

    /// Read or write a repository config key (e.g. knowledge.level)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print the current value of a config key
    Get {
        /// Config key (e.g. knowledge.level)
        key: String,
    },
    /// Set a config key to a new value
    Set {
        /// Config key (e.g. knowledge.level)
        key: String,
        /// New value
        value: String,
    },
}

#[derive(Subcommand)]
enum ProjectCommands {
    Create {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    List,
    Use {
        project_id: String,
    },
    Delete {
        project_id: String,
    },
}

#[derive(Subcommand)]
enum ArtifactCommands {
    Add {
        path: Option<String>,
        #[arg(long)]
        content: Option<String>,
        #[arg(long)]
        file_name: Option<String>,
        #[arg(long)]
        artifact_type: String,
        #[arg(long)]
        mime_type: Option<String>,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        project: Option<String>,
    },
    List {
        #[arg(long)]
        project: Option<String>,
    },
    Get {
        artifact_hash: String,
    },
    Restore {
        artifact_hash: String,
        #[arg(short, long)]
        output: Option<String>,
    },
    Link {
        clip_hash_or_id: String,
        artifact_hash: String,
        #[arg(long)]
        relationship: String,
    },
}

#[derive(Subcommand)]
enum PackCommands {
    Create {
        /// Project id to export
        project_id: Option<String>,
        /// Root clip hash or id (repeatable)
        #[arg(long = "root")]
        roots: Vec<String>,
        /// Optional ancestor depth limit when using --root
        #[arg(long)]
        depth: Option<u32>,
        /// Output .cliprootpack path
        #[arg(short, long)]
        output: String,
    },
    Import {
        /// Path to .cliprootpack archive
        path: String,
        /// Restore imported artifacts to a directory
        #[arg(long)]
        restore_artifacts: Option<String>,
    },
    Inspect {
        /// Path to .cliprootpack archive
        path: String,
    },
    Verify {
        /// Path to .cliprootpack archive
        path: String,
    },
}

#[derive(Subcommand)]
enum ActivityCommands {
    Start {
        #[arg(long)]
        activity_type: String,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        parameters: Option<String>,
        #[arg(long)]
        session: Option<String>,
    },
    End {
        activity_id: String,
    },
}

#[derive(Subcommand)]
enum RemoteCommands {
    /// Add a registry remote
    Add {
        /// Remote name (e.g., "origin")
        name: String,
        /// Registry URL
        url: String,
        /// Owner/namespace on the registry
        #[arg(long)]
        owner: Option<String>,
    },
    /// Remove a registry remote
    Remove {
        /// Remote name
        name: String,
    },
    /// List configured remotes
    List,
}

#[derive(Subcommand)]
enum SessionCommands {
    Start {
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        metadata: Option<String>,
    },
    End {
        session_id: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init { agent, hooks } => commands::init::run(agent, hooks, None),
        Commands::CaptureHook { harness } => commands::capture_hook::run(harness),
        Commands::Consolidate {
            session,
            emergency,
            commit,
        } => commands::consolidate::run(&session, emergency, commit, &cli.format),
        Commands::ConsolidateHook { harness, emergency } => {
            commands::consolidate_hook::run(harness, emergency)
        }
        Commands::FlushHook {
            harness,
            background,
            cliproot_dir,
        } => commands::flush_hook::run(harness, background, cliproot_dir),
        Commands::SessionStartHook {
            harness,
            cliproot_dir,
        } => {
            commands::session_start_hook::run(harness, cliproot_dir);
            Ok(())
        }
        Commands::Compile {
            cliproot_dir,
            background,
            background_child,
        } => commands::compile::run(cliproot_dir, background, background_child),
        Commands::WikiLint {
            cliproot_dir,
            structural_only,
            contradictions,
            strict,
            report,
        } => commands::wiki_lint::run(
            cliproot_dir,
            structural_only,
            contradictions,
            strict,
            report,
            &cli.format,
        ),
        Commands::Query {
            prompt,
            cliproot_dir,
            file_back,
            top_k,
        } => commands::query::run(&prompt, cliproot_dir, file_back, top_k, &cli.format),
        Commands::Record {
            session,
            session_dir,
            jsonl,
            hook_log,
            last,
            project,
            include_subagents,
            dry_run,
            pack,
            output,
        } => commands::record::run(
            commands::record::RecordOptions {
                session_id: session,
                session_dir,
                jsonl,
                hook_log_path: hook_log,
                last,
                project,
                include_subagents,
                dry_run,
                pack,
                output,
            },
            &cli.format,
        ),
        Commands::Clip {
            url,
            quote,
            source_type,
            id,
            document_id,
            project,
            title,
            activity,
            session,
            copy,
        } => commands::clip::run(
            &url,
            &quote,
            &source_type,
            id,
            document_id,
            project,
            title,
            activity.as_deref(),
            session.as_deref(),
            copy,
            &cli.format,
        ),
        Commands::Copy { hash_or_id, plain } => {
            commands::copy::run(&hash_or_id, plain, &cli.format)
        }
        Commands::Derive {
            from,
            quote,
            activity_type,
            agent,
            project,
            activity,
            session,
        } => commands::derive::run(
            &from,
            &quote,
            &activity_type,
            agent.as_deref(),
            project.as_deref(),
            activity.as_deref(),
            session.as_deref(),
            &cli.format,
        ),
        Commands::Inspect { hash_or_id } => commands::inspect::run(&hash_or_id, &cli.format),
        Commands::Trace { hash_or_id } => commands::trace::run(&hash_or_id, &cli.format),
        Commands::Verify { hash_or_id } => {
            commands::verify::run(hash_or_id.as_deref(), &cli.format)
        }
        Commands::List {
            document,
            source_type,
            project,
            limit,
        } => commands::list::run(
            document.as_deref(),
            source_type.as_deref(),
            project.as_deref(),
            limit,
            &cli.format,
        ),
        Commands::Ingest { path } => commands::ingest::run(&path, &cli.format),
        Commands::Export { hash, output } => {
            commands::export::run(&hash, output.as_deref(), &cli.format)
        }
        Commands::Annotate {
            file,
            style,
            in_place,
            threshold,
        } => commands::annotate::run(&file, &style, in_place, threshold, &cli.format),
        Commands::Cite { file, threshold } => commands::cite::run(&file, threshold, &cli.format),
        Commands::Doctor { file, threshold } => {
            commands::doctor::run(&file, threshold, &cli.format)
        }
        Commands::Mcp { path } => commands::mcp::run(path.as_deref()),
        Commands::Project { command } => match command {
            ProjectCommands::Create {
                id,
                name,
                description,
            } => commands::project::create(&id, &name, description, &cli.format),
            ProjectCommands::List => commands::project::list(&cli.format),
            ProjectCommands::Use { project_id } => commands::project::use_project(&project_id),
            ProjectCommands::Delete { project_id } => commands::project::delete(&project_id),
        },
        Commands::Artifact { command } => match command {
            ArtifactCommands::Add {
                path,
                content,
                file_name,
                artifact_type,
                mime_type,
                id,
                project,
            } => commands::artifact::add(
                path.as_deref(),
                content.as_deref(),
                file_name.as_deref(),
                &artifact_type,
                mime_type.as_deref(),
                id.as_deref(),
                project.as_deref(),
                &cli.format,
            ),
            ArtifactCommands::List { project } => {
                commands::artifact::list(project.as_deref(), &cli.format)
            }
            ArtifactCommands::Get { artifact_hash } => {
                commands::artifact::get(&artifact_hash, &cli.format)
            }
            ArtifactCommands::Restore {
                artifact_hash,
                output,
            } => commands::artifact::restore(&artifact_hash, output.as_deref()),
            ArtifactCommands::Link {
                clip_hash_or_id,
                artifact_hash,
                relationship,
            } => commands::artifact::link(
                &clip_hash_or_id,
                &artifact_hash,
                &relationship,
                &cli.format,
            ),
        },
        Commands::Pack { command } => match command {
            PackCommands::Create {
                project_id,
                roots,
                depth,
                output,
            } => commands::pack::create(project_id.as_deref(), &roots, depth, &output, &cli.format),
            PackCommands::Import {
                path,
                restore_artifacts,
            } => commands::pack::import(&path, restore_artifacts.as_deref(), &cli.format),
            PackCommands::Inspect { path } => commands::pack::inspect(&path, &cli.format),
            PackCommands::Verify { path } => commands::pack::verify(&path, &cli.format),
        },
        Commands::Remote { command } => match command {
            RemoteCommands::Add { name, url, owner } => {
                commands::remote::add(&name, &url, owner.as_deref(), &cli.format)
            }
            RemoteCommands::Remove { name } => commands::remote::remove(&name, &cli.format),
            RemoteCommands::List => commands::remote::list(&cli.format),
        },
        Commands::Push { project, remote } => {
            commands::push::run(project.as_deref(), remote.as_deref(), &cli.format)
        }
        Commands::Pull { project, remote } => {
            commands::pull::run(project.as_deref(), remote.as_deref(), &cli.format)
        }
        Commands::Search {
            query,
            remote,
            limit,
        } => commands::search::run(&query, remote.as_deref(), limit, &cli.format),
        Commands::Login { token, remote } => {
            commands::login::run(token.as_deref(), remote.as_deref(), &cli.format)
        }
        Commands::Logout { remote } => commands::logout::run(remote.as_deref(), &cli.format),
        Commands::Activity { command } => match command {
            ActivityCommands::Start {
                activity_type,
                prompt,
                agent,
                project,
                parameters,
                session,
            } => commands::activity::start(
                &activity_type,
                prompt,
                agent.as_deref(),
                project.as_deref(),
                parameters.as_deref(),
                session.as_deref(),
                &cli.format,
            ),
            ActivityCommands::End { activity_id } => {
                commands::activity::end(&activity_id, &cli.format)
            }
        },
        Commands::Session { command } => match command {
            SessionCommands::Start {
                agent,
                project,
                metadata,
            } => commands::session::start(
                agent.as_deref(),
                project.as_deref(),
                metadata.as_deref(),
                &cli.format,
            ),
            SessionCommands::End { session_id } => commands::session::end(&session_id, &cli.format),
        },
        Commands::Config { action } => match action {
            ConfigAction::Get { key } => commands::config::get(&key),
            ConfigAction::Set { key, value } => commands::config::set(&key, &value),
        },
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
