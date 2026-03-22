use crate::OutputFormat;
use cliproot_core::Clip;
use colored::Colorize;

pub fn print_clip(clip: &Clip, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(clip).unwrap());
        }
        OutputFormat::Text | OutputFormat::Table => {
            println!("{}: {}", "Hash".bold(), clip.clip_hash);
            if let Some(id) = &clip.id {
                println!("{}: {}", "ID".bold(), id);
            }
            if let Some(doc) = &clip.document_id {
                println!("{}: {}", "Document".bold(), doc);
            }
            println!("{}: {}", "Text Hash".bold(), clip.text_hash);
            println!("{}: {:?}", "Sources".bold(), clip.source_refs);
            if let Some(content) = &clip.content {
                let preview = if content.len() > 80 {
                    format!("{}...", &content[..80])
                } else {
                    content.clone()
                };
                println!("{}: {}", "Content".bold(), preview);
            }
        }
    }
}

pub fn print_clip_row(clip: &Clip, format: &OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string(clip).unwrap());
        }
        OutputFormat::Table => {
            let hash_short = &clip.clip_hash.0[..std::cmp::min(20, clip.clip_hash.0.len())];
            let id = clip.id.as_ref().map(|i| i.0.as_str()).unwrap_or("-");
            let content_preview = clip
                .content
                .as_ref()
                .map(|c| {
                    if c.len() > 40 {
                        format!("{}...", &c[..40])
                    } else {
                        c.clone()
                    }
                })
                .unwrap_or_default();
            println!("{hash_short:<22} {id:<16} {content_preview}");
        }
        OutputFormat::Text => {
            let hash_short = &clip.clip_hash.0[..std::cmp::min(30, clip.clip_hash.0.len())];
            let content_preview = clip
                .content
                .as_ref()
                .map(|c| {
                    if c.len() > 60 {
                        format!("{}...", &c[..60])
                    } else {
                        c.clone()
                    }
                })
                .unwrap_or_default();
            println!("{}  {}", hash_short.dimmed(), content_preview);
        }
    }
}
