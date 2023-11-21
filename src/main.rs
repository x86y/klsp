use lsp_server::{Connection, Message, Request, Response};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
};
use lsp_types::{InitializeResult, ServerCapabilities, Url};
use lsp_types::{Location, Position, Range, TextDocumentPositionParams};
use regex::Regex;
use std::collections::HashMap;
use tokio::runtime::Runtime;

fn handle_did_open(params: DidOpenTextDocumentParams, state: &mut ServerState) {
    let uri = params.text_document.uri;
    let text = params.text_document.text;
    state.update_document(uri, text);
}

fn handle_did_change(params: DidChangeTextDocumentParams, state: &mut ServerState) {
    let uri = params.text_document.uri;

    if let Some(change) = params.content_changes.first() {
        let text = &change.text;
        state.update_document(uri, text.clone());
    }
}

fn handle_did_close(params: DidCloseTextDocumentParams, state: &mut ServerState) {
    let uri = params.text_document.uri;
    state.remove_document(&uri);
}

struct ServerState {
    documents: HashMap<Url, String>,
}

impl ServerState {
    fn new() -> Self {
        ServerState {
            documents: HashMap::new(),
        }
    }

    fn update_document(&mut self, uri: Url, text: String) {
        self.documents.insert(uri, text);
    }

    fn remove_document(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    fn get_document_text(&self, uri: &Url) -> Option<&String> {
        self.documents.get(uri)
    }
}

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

fn main() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut state = ServerState::new();
        let (connection, io_threads) = Connection::stdio();
        while let Ok(message) = connection.receiver.recv() {
            match message {
                Message::Request(req) => {
                    if connection.handle_shutdown(&req).unwrap() {
                        break;
                    }
                    handle_request(req, &connection, &state).await;
                }
                Message::Response(_resp) => {}
                Message::Notification(notif) => {
                    match notif.method.as_str() {
                        "textDocument/didOpen" => {
                            let params: DidOpenTextDocumentParams =
                                serde_json::from_value(notif.params).unwrap();
                            handle_did_open(params, &mut state);
                        }
                        "textDocument/didChange" => {
                            let params: DidChangeTextDocumentParams =
                                serde_json::from_value(notif.params).unwrap();
                            handle_did_change(params, &mut state);
                        }
                        "textDocument/didClose" => {
                            let params: DidCloseTextDocumentParams =
                                serde_json::from_value(notif.params).unwrap();
                            handle_did_close(params, &mut state);
                        }
                        _ => {}
                    }
                }
            }
        }

        io_threads.join().unwrap();
    });
}

async fn handle_request(req: Request, connection: &Connection, state: &ServerState) {
    if req.method == "initialize" {
        let result = serde_json::to_value(InitializeResult {
            server_info: Some(lsp_types::ServerInfo {
                name: "K Language Server".into(),
                version: None,
            }),
            capabilities: ServerCapabilities {
                definition_provider: Some(lsp_types::OneOf::Left(true)),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        ..TextDocumentSyncOptions::default()
                    },
                )),
                ..ServerCapabilities::default()
            },
        })
        .unwrap();
        let response = Response::new_ok(req.id, result);
        connection.sender.send(Message::Response(response)).unwrap();
    } else if req.method == "textDocument/definition" {
        let params: TextDocumentPositionParams = serde_json::from_value(req.params).unwrap();
        let document_uri = params.text_document.uri;
        if let Some(doc_text) = state.get_document_text(&document_uri) {
            let definitions = parse(doc_text, &document_uri);
            let position = params.position;
            let line_text = doc_text.lines().nth(position.line as usize).unwrap_or("");
            let variable_name = extract_variable_at_position(line_text, position.character);
            let response = if let Some(location) = definitions.get(variable_name) {
                let mut updated_location = location.clone();
                updated_location.uri = document_uri;

                Response::new_ok(req.id, serde_json::to_value(updated_location).unwrap())
            } else {
                Response::new_err(req.id, 0, "Definition not found".into())
            };

            connection.sender.send(Message::Response(response)).unwrap();
        }
    }
}

fn extract_variable_at_position(line: &str, char_position: u32) -> &str {
    let is_variable_char = |c: char| c.is_alphanumeric() || c == '_';
    let start = line[..char_position as usize]
        .chars()
        .rev()
        .take_while(|&c| is_variable_char(c))
        .count();
    let start_index = char_position as usize - start;
    let end = line[char_position as usize..]
        .chars()
        .take_while(|&c| is_variable_char(c))
        .count();
    let end_index = char_position as usize + end;
    &line[start_index..end_index]
}
