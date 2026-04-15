use cliproot_store::{KnowledgeConfig, KnowledgeLevel, Repository};

/// `cliproot config get <key>`
pub fn get(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let cfg = repo.knowledge_config()?;
    let value = read_key(&cfg, key)?;
    println!("{value}");
    Ok(())
}

/// `cliproot config set <key> <value>`
pub fn set(key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let mut cfg = repo.knowledge_config()?;
    write_key(&mut cfg, key, value)?;
    repo.set_knowledge_config(cfg)?;
    println!("Set {key} = {value}");
    Ok(())
}

// ── Key accessors ─────────────────────────────────────────────────────────────

fn read_key(cfg: &KnowledgeConfig, key: &str) -> Result<String, Box<dyn std::error::Error>> {
    match key {
        "knowledge.level" => Ok(level_to_str(&cfg.level).to_string()),
        "knowledge.maxBgTokensPerDay" => Ok(cfg.max_bg_tokens_per_day.to_string()),
        "knowledge.maxBgCostPerDayUsd" => Ok(cfg.max_bg_cost_per_day_usd.to_string()),
        "knowledge.models.flush" => Ok(cfg.models.flush.clone()),
        "knowledge.models.compile" => Ok(cfg.models.compile.clone()),
        _ => Err(format!("unknown config key: {key}\n\nSupported keys:\n  knowledge.level  (minimal|curator|digest|wiki|team)\n  knowledge.maxBgTokensPerDay\n  knowledge.maxBgCostPerDayUsd\n  knowledge.models.flush\n  knowledge.models.compile").into()),
    }
}

fn write_key(
    cfg: &mut KnowledgeConfig,
    key: &str,
    value: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match key {
        "knowledge.level" => {
            cfg.level = str_to_level(value)?;
        }
        "knowledge.maxBgTokensPerDay" => {
            cfg.max_bg_tokens_per_day = value
                .parse::<u64>()
                .map_err(|_| format!("invalid u64 for {key}: {value}"))?;
        }
        "knowledge.maxBgCostPerDayUsd" => {
            cfg.max_bg_cost_per_day_usd = value
                .parse::<f64>()
                .map_err(|_| format!("invalid f64 for {key}: {value}"))?;
        }
        "knowledge.models.flush" => {
            cfg.models.flush = value.to_string();
        }
        "knowledge.models.compile" => {
            cfg.models.compile = value.to_string();
        }
        _ => {
            return Err(format!("unknown config key: {key}\n\nSupported keys:\n  knowledge.level  (minimal|curator|digest|wiki|team)\n  knowledge.maxBgTokensPerDay\n  knowledge.maxBgCostPerDayUsd\n  knowledge.models.flush\n  knowledge.models.compile").into());
        }
    }
    Ok(())
}

fn level_to_str(level: &KnowledgeLevel) -> &'static str {
    match level {
        KnowledgeLevel::Minimal => "minimal",
        KnowledgeLevel::Curator => "curator",
        KnowledgeLevel::Digest => "digest",
        KnowledgeLevel::Wiki => "wiki",
        KnowledgeLevel::Team => "team",
    }
}

fn str_to_level(s: &str) -> Result<KnowledgeLevel, Box<dyn std::error::Error>> {
    match s {
        "minimal" => Ok(KnowledgeLevel::Minimal),
        "curator" => Ok(KnowledgeLevel::Curator),
        "digest" => Ok(KnowledgeLevel::Digest),
        "wiki" => Ok(KnowledgeLevel::Wiki),
        "team" => Ok(KnowledgeLevel::Team),
        _ => Err(format!("invalid level: {s}  (choose: minimal|curator|digest|wiki|team)").into()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_level() {
        for (s, lvl) in [
            ("minimal", KnowledgeLevel::Minimal),
            ("curator", KnowledgeLevel::Curator),
            ("digest", KnowledgeLevel::Digest),
            ("wiki", KnowledgeLevel::Wiki),
            ("team", KnowledgeLevel::Team),
        ] {
            assert_eq!(str_to_level(s).unwrap(), lvl);
            assert_eq!(level_to_str(&lvl), s);
        }
    }

    #[test]
    fn unknown_key_errors() {
        let cfg = KnowledgeConfig::default();
        assert!(read_key(&cfg, "bogus.key").is_err());
    }

    #[test]
    fn set_level_digest() {
        let mut cfg = KnowledgeConfig::default();
        write_key(&mut cfg, "knowledge.level", "digest").unwrap();
        assert_eq!(cfg.level, KnowledgeLevel::Digest);
    }

    #[test]
    fn set_token_budget() {
        let mut cfg = KnowledgeConfig::default();
        write_key(&mut cfg, "knowledge.maxBgTokensPerDay", "50000").unwrap();
        assert_eq!(cfg.max_bg_tokens_per_day, 50_000);
    }

    #[test]
    fn get_set_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        cliproot_store::Repository::init(dir.path()).unwrap();
        let repo = cliproot_store::Repository::open(dir.path()).unwrap();

        let mut cfg = repo.knowledge_config().unwrap();
        assert_eq!(cfg.level, KnowledgeLevel::Curator);

        cfg.level = KnowledgeLevel::Digest;
        repo.set_knowledge_config(cfg).unwrap();

        let reloaded = repo.knowledge_config().unwrap();
        assert_eq!(reloaded.level, KnowledgeLevel::Digest);
    }
}
