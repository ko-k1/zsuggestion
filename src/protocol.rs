use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 9;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestEnvelope {
    pub version: u32,
    #[serde(flatten)]
    pub request: Request,
}

impl RequestEnvelope {
    pub fn new(request: Request) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            request,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Ping,
    Shutdown,
    Record {
        command: String,
        cwd: String,
        exit_code: i32,
        observed_at_ms: i64,
        session_id: String,
    },
    Complete {
        buffer: String,
        cursor_byte: usize,
        cwd: String,
        limit: Option<usize>,
        /// Shell aliases relevant to this buffer. The frontend sends only
        /// aliases whose names extend the command word, or the alias chain of
        /// the first word, so the shared daemon never stores shell state.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        aliases: Vec<Alias>,
    },
    Fuzzy {
        query: String,
        cwd: String,
        limit: Option<usize>,
    },
    ImportHistory {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Pong { version: String },
    Recorded,
    ShuttingDown,
    Completion(CompletionResponse),
    Imported { imported: usize, skipped: bool },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompletionResponse {
    pub replace_start_byte: usize,
    pub replace_end_byte: usize,
    pub candidates: Vec<Candidate>,
    pub enrichment_pending: bool,
}

impl CompletionResponse {
    pub fn empty(cursor_byte: usize) -> Self {
        Self {
            replace_start_byte: cursor_byte,
            replace_end_byte: cursor_byte,
            candidates: Vec::new(),
            enrichment_pending: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Candidate {
    pub display: String,
    pub description: String,
    pub description_pending: bool,
    pub kind: CandidateKind,
    pub insert_text: String,
    pub accept_text: String,
    pub source: CandidateSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    History,
    Command,
    File,
    Directory,
    Option,
    Subcommand,
    Value,
}

impl CandidateKind {
    /// The stable wire name, identical to the serde representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Command => "command",
            Self::File => "file",
            Self::Directory => "directory",
            Self::Option => "option",
            Self::Subcommand => "subcommand",
            Self::Value => "value",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    History,
    Command,
    Filesystem,
    Help,
}

impl CandidateSource {
    /// The stable wire name, identical to the serde representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Command => "command",
            Self::Filesystem => "filesystem",
            Self::Help => "help",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_match_serde() {
        for kind in [
            CandidateKind::History,
            CandidateKind::Command,
            CandidateKind::File,
            CandidateKind::Directory,
            CandidateKind::Option,
            CandidateKind::Subcommand,
            CandidateKind::Value,
        ] {
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{}\"", kind.as_str())
            );
        }
        for source in [
            CandidateSource::History,
            CandidateSource::Command,
            CandidateSource::Filesystem,
            CandidateSource::Help,
        ] {
            assert_eq!(
                serde_json::to_string(&source).unwrap(),
                format!("\"{}\"", source.as_str())
            );
        }
    }
}
