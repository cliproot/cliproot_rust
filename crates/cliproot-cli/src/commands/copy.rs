use cliproot_clipboard::{ClipboardWriter, WriteResult};
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(
    hash_or_id: &str,
    plain: bool,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let bundle = repo.export_bundle(hash_or_id)?;

    let text = bundle
        .clips
        .first()
        .and_then(|c| c.content.as_deref())
        .unwrap_or("");

    let mut cb = ClipboardWriter::new()?;

    if plain {
        cb.write_plain(text)?;
        match format {
            OutputFormat::Json => {
                println!(r#"{{"status":"copied","mode":"plain","hash":"{hash_or_id}"}}"#);
            }
            _ => println!("copied (plain): {hash_or_id}"),
        }
        return Ok(());
    }

    let bundle_json = serde_json::to_string(&bundle)?;
    let result = cb.write_with_html(text, &bundle_json)?;

    match format {
        OutputFormat::Json => {
            let mode = match result {
                WriteResult::HtmlOnly => "html",
            };
            println!(r#"{{"status":"copied","mode":"{mode}","hash":"{hash_or_id}"}}"#);
        }
        _ => match result {
            WriteResult::HtmlOnly => {
                println!("copied (html with data-crp-bundle): {hash_or_id}");
            }
        },
    }

    Ok(())
}
