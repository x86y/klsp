use dashmap::DashMap;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

fn parse(text: &str, document_uri: &Url) -> HashMap<String, Location> {
    let mut definitions = HashMap::new();
    let re = Regex::new(r"(?m)^(\w+):\s*.*").unwrap();

    for cap in re.captures_iter(text) {
        if let Some(var_name_match) = cap.get(1) {
            let var_name = var_name_match.as_str();

            // Calculate the line number correctly
            let byte_index = var_name_match.start();
            let line_number = text[..byte_index].matches('\n').count() as u32;

            let location = Location {
                uri: document_uri.clone(),
                range: Range {
                    start: Position {
                        line: line_number,
                        character: 0,
                    },
                    end: Position {
                        line: line_number,
                        character: var_name.len() as u32,
                    },
                },
            };
            definitions.insert(var_name.to_string(), location);
        }
    }

    definitions
}

fn extract_variable_at_position(line: &str, char_position: u32) -> &str {
    let is_variable_char = |c: char| c.is_alphanumeric() || c == '_';
    let char_pos = char_position as usize;
    let start = line[..char_pos]
        .char_indices()
        .rev()
        .find(|&(_, c)| !is_variable_char(c))
        .map_or(0, |(idx, _)| idx + 1);

    let end = line[char_pos..]
        .char_indices()
        .find(|&(_, c)| !is_variable_char(c))
        .map_or(line.len(), |(idx, _)| char_pos + idx);

    if start <= line.len() && end <= line.len() && start <= end {
        &line[start..end]
    } else {
        ""
    }
}

struct KLanguageServer {
    client: Client,
    documents: DashMap<Url, String>,
    definitions: DashMap<Url, HashMap<String, Location>>,
}

impl KLanguageServer {
    async fn diagnostics(&self, uri: Url) {
        self.client
            .publish_diagnostics(
                uri.clone(),
                get_diagnostics(
                    &uri.to_file_path().unwrap(),
                    self.documents
                        .get(&uri)
                        .unwrap()
                        .split('\n')
                        .map(|x| x.trim().to_owned())
                        .collect(),
                )
                .await,
                None,
            )
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for KLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "K Language Server".to_string(),
                version: None,
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(
                    TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)
                ),
                definition_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                // hover_provider: Some(HoverProviderCapability::Simple(true)),
                // completion_provider: Some(CompletionOptions::default()),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let document_uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        if let Some(doc_text) = self.documents.get(&document_uri) {
            let definitions = parse(&doc_text, &document_uri);
            let line_text = doc_text.lines().nth(position.line as usize).unwrap_or("");
            let variable_name = extract_variable_at_position(line_text, position.character);

            let mut changes = HashMap::new();

            if definitions.contains_key(variable_name) {
                let mut edits = Vec::new();
                for (line_index, line) in doc_text.lines().enumerate() {
                    let mut start_char_index = 0;
                    while let Some(found_pos) = line[start_char_index..].find(variable_name) {
                        let start = start_char_index + found_pos;
                        let end = start + variable_name.len();
                        if (start == 0 || !line.chars().nth(start - 1).unwrap().is_alphanumeric())
                            && (end == line.len()
                                || !line.chars().nth(end).unwrap().is_alphanumeric())
                        {
                            let range = Range {
                                start: Position::new(line_index as u32, start as u32),
                                end: Position::new(line_index as u32, end as u32),
                            };
                            edits.push(TextEdit {
                                range,
                                new_text: new_name.clone(),
                            });
                        }
                        start_char_index = end;
                    }
                }
                if !edits.is_empty() {
                    changes.insert(document_uri, edits);
                }
            }

            Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                document_changes: None,
                change_annotations: None,
            }))
        } else {
            Err(tower_lsp::jsonrpc::Error::new(tower_lsp::jsonrpc::ErrorCode::ParseError))
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.documents.insert(uri.clone(), text.clone());
        let definitions = parse(&text, &uri);
        self.definitions.insert(uri.clone(), definitions);
        self.diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = &params.content_changes[0].text;
        self.documents
            .insert(uri.clone(), text.lines().map(str::to_owned).collect());
        self.diagnostics(uri.clone()).await;

        self.definitions.clear();
        let definitions = parse(text, &uri);
        self.definitions.insert(uri.clone(), definitions);
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let document_uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc_text) = self.documents.get(&document_uri) {
            if let Some(definitions) = self.definitions.get(&document_uri) {
                let line_text = doc_text.lines().nth(position.line as usize).unwrap_or("");
                let variable_name = extract_variable_at_position(line_text, position.character);

                let response =
                    definitions.get(variable_name).map(|location| {
                        GotoDefinitionResponse::Scalar(Location {
                            uri: document_uri.clone(),
                            range: location.range,
                        })
                    });

                Ok(response)
            } else {
                Err(tower_lsp::jsonrpc::Error::new(tower_lsp::jsonrpc::ErrorCode::InternalError))
            }
        } else {
            Err(tower_lsp::jsonrpc::Error::new(tower_lsp::jsonrpc::ErrorCode::ParseError))
        }
    }
}

async fn get_diagnostics(s: &PathBuf, doc_lines: Vec<String>) -> Vec<Diagnostic> {
    let output = tokio::process::Command::new("/usr/local/bin/k")
        .arg(s)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to execute process")
        .wait_with_output()
        .await
        .expect("failed to wait on child");

    if !output.status.success() {
        parse_diagnostics_from_stderr(
            String::from_utf8_lossy(&output.stderr).to_string(),
            &doc_lines,
        )
    } else {
        vec![] // Return an empty vector if the process fails
    }
}

fn parse_diagnostics_from_stderr(stderr_output: String, doc_lines: &[String]) -> Vec<Diagnostic> {
    dbg!(&stderr_output);
    let mut diagnostics = Vec::new();
    let stderr_lines = stderr_output.lines();
    let error_message = format!("Syntax error at: {stderr_output}");
    let mut character = 0;
    let mut line_number = 0;

    for line in stderr_lines {
        if line.trim().starts_with('^') {
            character = line.find('^').unwrap_or(0) as u64;
        } else if !line.trim().starts_with("'parse") {
            dbg!(&doc_lines);
            line_number = doc_lines
                .iter()
                .position(|r| r.trim() == line.trim())
                .unwrap_or(0);
        }
    }
    let diagnostic =
        Diagnostic::new(
            Range::new(
                Position::new(line_number as u32, character as u32),
                Position::new(line_number as u32, character as u32 + 1),
            ),
            Some(DiagnosticSeverity::ERROR),
            None,
            Some("k-language-server".to_string()),
            error_message.clone(),
            None,
            None,
        );
    diagnostics.push(diagnostic);
    diagnostics
}

#[tokio::main]
async fn main() {
    let (service, socket) = LspService::new(|client| KLanguageServer {
        client,
        documents: DashMap::new(),
        definitions: DashMap::new(),
    });
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}
