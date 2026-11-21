//! Serialization of completion responses for the shell frontends.
//!
//! The `zsh*` formats are NUL-separated records so the Zsh integration can
//! split them with `${(0)...}` without any quoting concerns:
//!
//! - `zsh`: per candidate `insert, display, description, kind, source`.
//! - `zsh-v2`: as `zsh`, plus a trailing `description_pending` boolean.
//! - `zsh-v3`: as `zsh-v2`, preceded by one `enrichment_pending` boolean.

use std::io::Write;

use anyhow::Result;
use clap::ValueEnum;

use crate::transport::protocol::CompletionResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// The full response as one JSON object.
    Json,
    /// Only the accept text of the top candidate.
    Insert,
    Zsh,
    ZshV2,
    ZshV3,
}

/// Writes `completion` to `output` in the requested `format`.
pub fn write_completion(
    output: &mut impl Write,
    completion: &CompletionResponse,
    format: OutputFormat,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            serde_json::to_writer(&mut *output, completion)?;
            output.write_all(b"\n")?;
        }
        OutputFormat::Insert => {
            if let Some(candidate) = completion.candidates.first() {
                output.write_all(candidate.accept_text.as_bytes())?;
            }
        }
        OutputFormat::Zsh | OutputFormat::ZshV2 | OutputFormat::ZshV3 => {
            let with_pending = format != OutputFormat::Zsh;
            if format == OutputFormat::ZshV3 {
                write_field(output, bool_str(completion.enrichment_pending))?;
            }
            for candidate in &completion.candidates {
                for field in [
                    candidate.insert_text.as_str(),
                    candidate.display.as_str(),
                    candidate.description.as_str(),
                    candidate.kind.as_str(),
                    candidate.source.as_str(),
                ] {
                    write_field(output, field)?;
                }
                if with_pending {
                    write_field(output, bool_str(candidate.description_pending))?;
                }
            }
        }
    }
    Ok(())
}

fn write_field(output: &mut impl Write, field: &str) -> std::io::Result<()> {
    output.write_all(field.as_bytes())?;
    output.write_all(&[0])
}

fn bool_str(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::protocol::{Candidate, CandidateKind, CandidateSource};

    fn response() -> CompletionResponse {
        CompletionResponse {
            replace_start_byte: 2,
            replace_end_byte: 2,
            candidates: vec![Candidate {
                display: "git".to_owned(),
                description: "version control".to_owned(),
                description_pending: true,
                kind: CandidateKind::Command,
                insert_text: "t".to_owned(),
                accept_text: "t ".to_owned(),
                source: CandidateSource::Command,
            }],
            enrichment_pending: false,
        }
    }

    fn render(format: OutputFormat) -> Vec<u8> {
        let mut output = Vec::new();
        write_completion(&mut output, &response(), format).unwrap();
        output
    }

    #[test]
    fn zsh_formats_are_nul_separated() {
        assert_eq!(
            render(OutputFormat::Zsh),
            b"t\0git\0version control\0command\0command\0"
        );
        assert_eq!(
            render(OutputFormat::ZshV2),
            b"t\0git\0version control\0command\0command\0true\0"
        );
        assert_eq!(
            render(OutputFormat::ZshV3),
            b"false\0t\0git\0version control\0command\0command\0true\0"
        );
    }

    #[test]
    fn insert_format_emits_accept_text() {
        assert_eq!(render(OutputFormat::Insert), b"t ");
    }
}
