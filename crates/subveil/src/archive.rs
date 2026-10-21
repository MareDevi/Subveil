//! Explicit, bounded transcript files. Nothing here runs automatically at session end.
use anyhow::{Context as _, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read as _, Write as _},
    path::Path,
};
use subveil_core::subtitle::{SubtitleState, TranslationState};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub start_ms: u64,
    pub end_ms: u64,
    pub language: Option<String>,
    pub source: String,
    pub translation: Option<String>,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    version: u32,
    pub records: Vec<Record>,
}
impl Archive {
    pub fn snapshot(state: &SubtitleState) -> Self {
        Self {
            version: 1,
            records: state
                .history()
                .iter()
                .filter(|s| s.is_final)
                .map(|s| Record {
                    start_ms: s.transcript.start_ms,
                    end_ms: s.transcript.end_ms,
                    language: s.transcript.language.clone(),
                    source: s.transcript.text.clone(),
                    translation: match &s.translation {
                        TranslationState::Complete(t) => Some(t.clone()),
                        _ => None,
                    },
                })
                .collect(),
        }
    }
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            self.version == 1 && self.records.len() <= 512,
            "字幕档案版本或段数无效"
        );
        for record in &self.records {
            ensure!(
                record.start_ms <= record.end_ms && record.end_ms <= 7 * 24 * 60 * 60 * 1000,
                "字幕时间无效"
            );
            ensure!(
                record.source.len() <= 65_536
                    && record
                        .translation
                        .as_ref()
                        .is_none_or(|s| s.len() <= 65_536)
                    && record.language.as_ref().is_none_or(|s| s.len() <= 32),
                "字幕字段过长"
            );
        }
        Ok(())
    }
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut text = String::new();
        fs::File::open(path)?
            .take(8_388_609)
            .read_to_string(&mut text)?;
        ensure!(text.len() <= 8_388_608, "字幕档案超过 8 MiB");
        // Parser diagnostics must not quote private transcript lines into errors/logs.
        let archive: Self =
            toml::from_str(&text).map_err(|_| anyhow::anyhow!("字幕档案格式无效"))?;
        archive.validate()?;
        Ok(archive)
    }
    pub fn encode(&self, format: &str) -> anyhow::Result<String> {
        self.validate()?;
        if format == "toml" {
            return Ok(toml::to_string(self)?);
        }
        ensure!(matches!(format, "txt" | "md" | "srt"), "导出格式无效");
        let mut out = String::new();
        for (index, record) in self.records.iter().enumerate() {
            let source = clean(&record.source);
            let translation = record.translation.as_deref().map(clean);
            if format == "srt" {
                out.push_str(&format!(
                    "{}\n{} --> {}\n",
                    index + 1,
                    timestamp(record.start_ms),
                    timestamp(record.end_ms.max(record.start_ms + 1))
                ));
            } else if format == "md" {
                out.push_str(&format!("### {}\n\n", timestamp(record.start_ms)));
            }
            if format == "md" {
                out.push_str(&escape_markdown(&source));
                if let Some(text) = translation {
                    out.push_str("\n\n");
                    out.push_str(&escape_markdown(&text));
                }
            } else {
                out.push_str(&source);
                if let Some(text) = translation {
                    out.push('\n');
                    out.push_str(&text);
                }
            }
            out.push_str("\n\n");
        }
        Ok(out)
    }
}
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}
fn escape_markdown(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if "\\`*_{}[]()#+-.!|>".contains(c) {
            out.push('\\');
        }
        match c {
            '<' => out.push_str("&lt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}
fn timestamp(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

pub fn save(path: &Path, text: &str) -> anyhow::Result<()> {
    ensure!(text.len() <= 8_388_608, "字幕档案超过 8 MiB");
    let parent = path.parent().context("导出目录无效")?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let temporary = parent.join(format!(".subveil-export-{}-{nonce}", std::process::id()));
    let result = (|| -> anyhow::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exported_time_text_and_html_are_bounded_and_escaped() -> anyhow::Result<()> {
        let archive = Archive {
            version: 1,
            records: vec![Record {
                start_ms: 1_234,
                end_ms: 2_345,
                language: Some("ja".into()),
                source: "<script>字\u{0}".into(),
                translation: Some("[译文](evil)".into()),
            }],
        };
        assert!(
            archive
                .encode("srt")?
                .contains("00:00:01,234 --> 00:00:02,345")
        );
        assert!(!archive.encode("srt")?.contains('\u{0}'));
        assert!(!archive.encode("md")?.contains("<script>"));
        assert!(!archive.encode("md")?.contains("[译文](evil)"));
        assert_eq!(
            toml::from_str::<Archive>(&archive.encode("toml")?)?
                .records
                .len(),
            1
        );
        let mut invalid = archive;
        invalid.records[0].end_ms = 0;
        assert!(invalid.encode("txt").is_err());
        Ok(())
    }
}
