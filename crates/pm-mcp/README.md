# pm-mcp

A stdio [Model Context Protocol](https://modelcontextprotocol.io) server over a
`pm` project's `.pm/pm.json5` ticket store (PM-5).

It works **directly on disk** through `pm-core` — no running `pm` GUI is required.
A GUI that *is* open picks up writes through its filesystem watch.

## Run

```
cargo run -p pm-mcp -- --project /path/to/repo
```

The project root defaults to `--project` / the first positional argument / the
current directory. Every tool also takes an optional `project` path argument that
overrides it per call; if the path has no `.pm/pm.json5` the server walks up to
the nearest ancestor that does.

## Tools

| tool | purpose |
|---|---|
| `list_tickets` | id, title, status, priority, author, comment count — optional `status` / `label` filter |
| `get_ticket` | one ticket in full: body, comments, code anchors |
| `add_comment` | append a comment (`author` optional) |
| `create_ticket` | new ticket (`title`, optional `body`/`author`/`priority`/`labels`) → new `PM-N` |
| `edit_ticket` | change `title`/`body`/`status`/`priority`/`labels`/`assignee` |
| `open_project` | launch the `pm` GUI on a project (`$PM_BIN`, a sibling `pm`/`pm.exe`, or `pm` on `PATH`) |
| `list_projects` | scan a directory tree for `.pm/pm.json5` (`root`, `depth`) |

## Authorship (PM-15)

`author` is a free, **unverified** string on both tickets and comments. Pass it to
attribute work to a specific name; omit it and the server falls back to the
`author` field of `~/.pm/config.json`, then the repo's git `user.name`, then
`"unknown"`.

## Register with Claude Code

`.mcp.json` in the repo root:

```json
{
  "mcpServers": {
    "pm": { "command": "cargo", "args": ["run", "-q", "-p", "pm-mcp", "--"] }
  }
}
```
