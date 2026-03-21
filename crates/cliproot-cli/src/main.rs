mod commands;
mod output;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Table,
}

#[derive(Parser)]
#[command(name = "cliproot", about = "Local-first provenance engine for content-addressed clips")]
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
    Init,

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

        /// Optional title
        #[arg(long)]
        title: Option<String>,
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
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::run(),
        Commands::Clip {
            url,
            quote,
            source_type,
            id,
            document_id,
            title,
        } => commands::clip::run(&url, &quote, &source_type, id, document_id, title, &cli.format),
        Commands::Derive {
            from,
            quote,
            activity_type,
            agent,
        } => commands::derive::run(&from, &quote, &activity_type, agent.as_deref(), &cli.format),
        Commands::Inspect { hash_or_id } => {
            commands::inspect::run(&hash_or_id, &cli.format)
        }
        Commands::Trace { hash_or_id } => {
            commands::trace::run(&hash_or_id, &cli.format)
        }
        Commands::Verify { hash_or_id } => {
            commands::verify::run(hash_or_id.as_deref(), &cli.format)
        }
        Commands::List {
            document,
            source_type,
            limit,
        } => commands::list::run(
            document.as_deref(),
            source_type.as_deref(),
            limit,
            &cli.format,
        ),
        Commands::Ingest { path } => commands::ingest::run(&path, &cli.format),
        Commands::Export { hash, output } => {
            commands::export::run(&hash, output.as_deref(), &cli.format)
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
