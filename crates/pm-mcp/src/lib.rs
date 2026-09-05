//! `pm-mcp` — a stdio [Model Context Protocol](https://modelcontextprotocol.io)
//! server over a `pm` project's `.pm/pm.json5` ticket store (PM-5).
//!
//! It works directly on disk through `pm-core`; no running GUI is required. The
//! project is chosen per-call via an optional `project` path argument, defaulting
//! to the `default_root` the server was started with.
//!
//! This is a library — the `pm` binary drives it via [`serve_stdio`] as the
//! `pm mcp` subcommand (PM-5 / PM-14: one binary).

mod ops;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone)]
struct PmServer {
    default_root: Arc<PathBuf>,
    #[allow(dead_code)] // read by the #[tool_handler] macro's generated code
    tool_router: ToolRouter<PmServer>,
}

/// Turn an `ops` result into an MCP tool result — pretty JSON on success, a
/// tool-level error (not a protocol error) on failure so the model can react.
fn reply(r: Result<Value>) -> Result<CallToolResult, McpError> {
    match r {
        Ok(v) => Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
        )])),
        Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(e.to_string())])),
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListTickets {
    #[schemars(description = "Project path (default: the server's project / cwd)")]
    project: Option<String>,
    #[schemars(description = "Filter: open | in_progress | blocked | done | wontfix")]
    status: Option<String>,
    #[schemars(description = "Filter: only tickets carrying this label")]
    label: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TicketRef {
    project: Option<String>,
    #[schemars(description = "Numeric ticket id (the number in PM-N)")]
    id: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddComment {
    project: Option<String>,
    id: u64,
    body: String,
    #[schemars(description = "Author to record — any name; unverified (PM-15)")]
    author: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateTicket {
    project: Option<String>,
    title: String,
    body: Option<String>,
    #[schemars(description = "Author to record — any name; unverified (PM-15)")]
    author: Option<String>,
    #[schemars(description = "low | normal | high | urgent")]
    priority: Option<String>,
    labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EditTicket {
    project: Option<String>,
    id: u64,
    title: Option<String>,
    body: Option<String>,
    #[schemars(description = "open | in_progress | blocked | done | wontfix")]
    status: Option<String>,
    #[schemars(description = "low | normal | high | urgent")]
    priority: Option<String>,
    labels: Option<Vec<String>>,
    #[schemars(description = "String to assign, or null to clear")]
    assignee: Option<Value>,
    #[schemars(description = "Author to record against each edit — any name; unverified (PM-15)")]
    author: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenProject {
    #[schemars(description = "Project path to open in the pm GUI")]
    project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListProjects {
    #[schemars(description = "Directory to scan (default: cwd)")]
    root: Option<String>,
    #[schemars(description = "Directory recursion depth (default: 3)")]
    depth: Option<usize>,
}

#[tool_router]
impl PmServer {
    fn new(default_root: PathBuf) -> Self {
        Self {
            default_root: Arc::new(default_root),
            tool_router: Self::tool_router(),
        }
    }

    fn root(&self, arg: Option<&str>) -> PathBuf {
        ops::resolve_root(arg, &self.default_root)
    }

    #[tool(description = "List tickets in a pm project (id, title, status, priority, author).")]
    fn list_tickets(&self, Parameters(a): Parameters<ListTickets>) -> Result<CallToolResult, McpError> {
        let root = self.root(a.project.as_deref());
        reply(ops::list_tickets(&root, a.status.as_deref(), a.label.as_deref()))
    }

    #[tool(description = "Read one ticket in full: body, comments, and code anchors.")]
    fn get_ticket(&self, Parameters(a): Parameters<TicketRef>) -> Result<CallToolResult, McpError> {
        let root = self.root(a.project.as_deref());
        reply(ops::get_ticket(&root, a.id))
    }

    #[tool(description = "Append a comment to a ticket. `author` may be any name (unverified).")]
    fn add_comment(&self, Parameters(a): Parameters<AddComment>) -> Result<CallToolResult, McpError> {
        let root = self.root(a.project.as_deref());
        reply(ops::add_comment(&root, a.id, &a.body, a.author.as_deref()))
    }

    #[tool(description = "Create a ticket. Returns its new PM-N id.")]
    fn create_ticket(&self, Parameters(a): Parameters<CreateTicket>) -> Result<CallToolResult, McpError> {
        let root = self.root(a.project.as_deref());
        reply(ops::create_ticket(
            &root,
            &a.title,
            a.body.as_deref(),
            a.author.as_deref(),
            a.priority.as_deref(),
            a.labels,
        ))
    }

    #[tool(description = "Edit a ticket's title/body/status/priority/labels/assignee.")]
    fn edit_ticket(&self, Parameters(a): Parameters<EditTicket>) -> Result<CallToolResult, McpError> {
        let root = self.root(a.project.as_deref());
        reply(ops::edit_ticket(
            &root,
            a.id,
            a.title.as_deref(),
            a.body.as_deref(),
            a.status.as_deref(),
            a.priority.as_deref(),
            a.labels,
            a.assignee,
            a.author.as_deref(),
        ))
    }

    #[tool(description = "Open a project in the pm GUI (launches the `pm` binary).")]
    fn open_project(&self, Parameters(a): Parameters<OpenProject>) -> Result<CallToolResult, McpError> {
        let root = self.root(Some(&a.project));
        reply(ops::open_project(&root))
    }

    #[tool(description = "Scan a directory tree for pm projects (.pm/pm.json5).")]
    fn list_projects(&self, Parameters(a): Parameters<ListProjects>) -> Result<CallToolResult, McpError> {
        let root = a
            .root
            .map(PathBuf::from)
            .unwrap_or_else(|| (*self.default_root).clone());
        reply(ops::list_projects(&root, a.depth.unwrap_or(3)))
    }
}

#[tool_handler]
impl ServerHandler for PmServer {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = Implementation::new("pm", pm_core::buildinfo::VERSION);
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "pm is the ticket tracker for this project. Tickets live in the repo at \
             .pm/pm.json5 and are committed alongside the code; each has an id written \
             `PM-<n>` that is the shared handle for a piece of work — use it in commit \
             messages, ticket bodies, and comments.\n\
             \n\
             When to reach for it:\n\
             - Before starting a piece of work, check whether a ticket already tracks it \
             (list_tickets, or get_ticket by id). Don't open a second ticket for work an \
             existing one already describes.\n\
             - When you finish work a ticket describes, set its status to `done` \
             (edit_ticket).\n\
             - Record what you learned on the ticket as a comment (add_comment) — scoping \
             decisions, dead ends, why an approach was rejected. This is where the \
             project's real reasoning lives, and the next agent will read it.\n\
             - File a ticket for a bug or follow-up you notice but aren't fixing now.\n\
             \n\
             Always pass `author` on create_ticket / add_comment / edit_ticket, otherwise \
             your work is attributed to the human running the session (it falls back to \
             their git name). Authorship is a free, unverified string.\n\
             \n\
             Tools: list_tickets, get_ticket, create_ticket, edit_ticket, add_comment, \
             open_project (launches the GUI), list_projects."
                .to_string(),
        );
        info
    }
}

/// Run the MCP server on stdio until the client disconnects. Blocks; builds its
/// own Tokio runtime so the (synchronous) `pm` binary can just call it.
pub fn serve_stdio(default_root: PathBuf) -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();

    eprintln!("pm mcp: project root {}", default_root.display());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let service = PmServer::new(default_root)
            .serve(rmcp::transport::stdio())
            .await?;
        service.waiting().await?;
        Ok::<(), anyhow::Error>(())
    })
}
