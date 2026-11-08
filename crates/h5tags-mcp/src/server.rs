//! The MCP side of the tools (PRD §9a): the server instructions, the tool
//! list, calls, and the guide resource. A thin adapter, like the CLI: the
//! tools, their parameters and their output are `h5tags-core`'s.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use h5tags_core::tools::tool_specs;
use h5tags_core::{Command, Format, Session, ToolError};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, InitializeRequestParams,
    InitializeResult, JsonObject, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource, ResourceContents,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Value, json};

/// Sent once per connection (PRD §9a, exact text).
pub const INSTRUCTIONS: &str = "\
h5tags reads Halo 5 .module tags (builds: see list_modules / the build parameter). Values come only from these tools; never fill them in from memory.
Guide: AGENT_GUIDE.md, resource h5tags://guide. Read it before a task that needs more context about the tool or the game: what a change would affect, whether something is used in the game (§2.2), tag internals (dependencies, string tables, resources), or a result these lines don't explain. Simple lookups don't need it.
Questions about many tags: ask the build, not tag by tag. query_field (one field in every tag of a class; nested [*] works: barrels[*].firing effects[*].firing effect), find_references direction=in (every tag that uses a tag), search_values (tags by text they hold), which_maps (the maps that load a tag, scripts too) / map_tags (a map's tags).
Find a tag: search_tags (by path or class), browse (a folder's shape), list_entries (assets by kind or size), search_definitions (which class and field control something).
One tag: outline(tag) → read_tag(section= or path=) → diff_tags against another. outline(class=) is the short get_definition.
Never read a whole large tag (scnr, effe, vehi, bipd, char, gint, unit, weap): call outline first.
name#2 = another field with the same name (flags, flags#2), not an entry. References are tag fields only: objects a map variant places aren't seen.
detail: brief for many tags, normal by default, full only for fields you explain or change.
Text output: '# ' tag line, '## ' class section, '### '+ sections/structs/blocks, indented lines = fields (name, type, value, units, extras).
'→ path=...' = not expanded; call again with that path. '… N more (offset=X)' = next page.
(empty) (none) (none set) = real facts about the tag. (no definition…) (no option…) (unnamed…) = value is real, meaning unknown.
(unresolved) = string_id not resolved; don't guess it. (not in indexed modules) = say so; don't assume it's missing from the game.
'!!' or (not read) = the TOOL failed: never state those values, or their absence, as facts; tell the user. 'warning: definition GUID mismatch' = the values were read; use them and mention the warning.
copies: N (differ) = the tag differs between update layers (game versions); tools read the latest layer. tag_copies lists every version; module= reads another. Say which you used.
Field names and help text are hints about behaviour, not proof; say where an explanation comes from.
What a change affects (guide §2.1): diff_tags against a tag that already behaves as wanted; the fields that differ are candidates, not causes.
Check a candidate on a third tag or across the class (query_field) before calling it the cause.
Follow references out (HUD, projectiles, effects) and in (find_references direction=in): a change reaches every tag that uses the tag.
Tags hold settings; the engine decides what happens at runtime: say when a conclusion is inference. Answer with a change list: tag, field path, current → proposed value, why.";

pub const GUIDE_URI: &str = "h5tags://guide";
const GUIDE_MIME: &str = "text/markdown";

/// `docs/AGENT_GUIDE.md`, as built into the server.
pub const GUIDE: &str = include_str!("../../../docs/AGENT_GUIDE.md");

/// The argument every tool takes over MCP, besides its own (PRD §7.1).
const FORMAT: &str = "format";

/// Every tool, as `tools/list` returns it: its parameters plus `format`.
pub fn tools() -> Vec<Tool> {
    tool_specs()
        .into_iter()
        .map(|spec| {
            let mut schema = spec.input_schema;
            if let Some(Value::Object(props)) = schema.get_mut("properties") {
                props.insert(
                    FORMAT.into(),
                    json!({ "type": "string", "enum": ["text", "json"], "default": "text",
                            "description": "text (default: compact, what the guide describes) or json" }),
                );
            }
            Tool::new(spec.name, spec.description, Arc::new(schema))
        })
        .collect()
}

/// `format`, taken out of the arguments (the tool's own parameters stay).
fn take_format(args: &mut JsonObject) -> Result<Format, ToolError> {
    match args.remove(FORMAT) {
        None => Ok(Format::Text),
        Some(Value::String(f)) if f == "text" => Ok(Format::Text),
        Some(Value::String(f)) if f == "json" => Ok(Format::Json),
        Some(other) => Err(ToolError::new(format!("format can't be {other}")).hint("format: text or json")),
    }
}

/// One tool call: its rendered result, and whether it is an error.
pub fn call(session: &mut Session, name: &str, mut args: JsonObject) -> (String, bool) {
    let format = match take_format(&mut args) {
        Ok(f) => f,
        Err(e) => return (e.render(Format::Text), true),
    };
    let run = Command::from_json(name, Value::Object(args)).and_then(|c| c.run(session));
    match run {
        Ok(out) => (out.render(format), false),
        Err(e) => (e.render(format), true),
    }
}

/// The session, opened in the background at start (indexing can take a
/// while; the client shouldn't time out connecting), or why it couldn't be.
pub type SessionSlot = Arc<OnceLock<Result<Mutex<Session>, String>>>;

#[derive(Clone)]
pub struct Server {
    session: SessionSlot,
    /// Built once: the tool list doesn't change while the server runs.
    tools: Arc<Vec<Tool>>,
}

impl Server {
    pub fn new(session: SessionSlot) -> Server {
        Server { session, tools: Arc::new(tools()) }
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_instructions(INSTRUCTIONS)
            .with_server_info(Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")))
    }

    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        // Whether the client can show the user a confirmation dialog
        // (elicitation; decisions D38): recorded, not relied on.
        tracing::info!(
            client = %request.client_info.name,
            version = %request.client_info.version,
            elicitation = request.capabilities.elicitation.is_some(),
            "client connected"
        );
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tools.as_ref().clone()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let slot = self.session.clone();
        let name = request.name.to_string();
        let args = request.arguments.unwrap_or_default();
        // Tools read files and decompress: off the async threads.
        let (text, is_error) = tokio::task::spawn_blocking(move || match slot.wait() {
            Ok(session) => call(&mut session.lock().unwrap_or_else(PoisonError::into_inner), &name, args),
            Err(e) => (ToolError::new(e.clone()).render(Format::Text), true),
        })
        .await
        .map_err(|e| McpError::internal_error(format!("the tool stopped: {e}"), None))?;
        let content = vec![ContentBlock::text(text)];
        Ok(if is_error { CallToolResult::error(content) } else { CallToolResult::success(content) }.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let mut guide = Resource::new(GUIDE_URI, "guide");
        guide.description = Some("How to read h5tags results: output format, markers, workflow, budget rules".into());
        guide.mime_type = Some(GUIDE_MIME.into());
        Ok(ListResourcesResult::with_all_items(vec![guide]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri != GUIDE_URI {
            return Err(McpError::resource_not_found(
                format!("no resource {}; the guide is {GUIDE_URI}", request.uri),
                None,
            ));
        }
        let guide = ResourceContents::TextResourceContents {
            uri: GUIDE_URI.into(),
            mime_type: Some(GUIDE_MIME.into()),
            text: GUIDE.into(),
            meta: None,
        };
        Ok(ReadResourceResult::new(vec![guide]).into())
    }
}

#[cfg(test)]
mod tests {
    use h5tags_core::testdeploy::{TestDeploy, WEAPON};
    use h5tags_core::tools::TOOL_NAMES;

    use super::*;

    fn args(v: Value) -> JsonObject {
        let Value::Object(o) = v else { panic!("{v}") };
        o
    }

    /// PRD §9a gives the exact text; the server sends exactly that.
    #[test]
    fn the_instructions_are_the_prd_text() {
        let prd = include_str!("../../../docs/PRD.md");
        let start = prd.find("1. **MCP server `instructions`**").unwrap();
        let block = &prd[start..];
        let open = block.find("```\n").unwrap() + 4;
        let close = block[open..].find("\n```").unwrap();
        assert_eq!(INSTRUCTIONS, &block[open..open + close]);
    }

    #[test]
    fn every_tool_is_listed_with_a_format_parameter() {
        let tools = tools();
        assert_eq!(tools.iter().map(|t| t.name.as_ref()).collect::<Vec<_>>(), TOOL_NAMES);
        for t in &tools {
            let props = t.input_schema["properties"].as_object().unwrap();
            assert_eq!(props[FORMAT]["enum"], json!(["text", "json"]), "{}", t.name);
            assert!(t.description.as_deref().is_some_and(|d| !d.is_empty()), "{}", t.name);
        }
    }

    #[test]
    fn a_call_returns_the_tool_result_in_the_format_asked() {
        let mut d = TestDeploy::new("mcp-call");
        let (text, error) = call(&mut d.session, "read_tag", args(json!({ "tag": WEAPON, "path": "count" })));
        assert!(!error && text.contains("\ncount  int32  0\n"), "{text}");
        let (text, error) =
            call(&mut d.session, "read_tag", args(json!({ "tag": WEAPON, "path": "count", "format": "json" })));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(!error && v["fields"][1]["value"] == 0, "{text}");
    }

    #[test]
    fn a_bad_call_is_a_tool_error_with_its_hint() {
        let mut d = TestDeploy::new("mcp-bad-call");
        let (text, error) = call(&mut d.session, "read_tag", args(json!({ "tag": WEAPON, "paths": "count" })));
        assert!(error, "{text}");
        assert!(text.starts_with("!! error: read_tag has no parameter 'paths'\nhint: parameters: "), "{text}");
        let (text, error) = call(&mut d.session, "read_tag", args(json!({ "tag": WEAPON, "format": "xml" })));
        assert!(error && text.contains("format can't be \"xml\""), "{text}");
    }
}
