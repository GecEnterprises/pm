# pm-mcp

A stdio [Model Context Protocol](https://modelcontextprotocol.io) server over a
`pm` project's `.pm/pm.json5` ticket store (PM-5).

This is a **library** — it ships inside the single `pm` binary as the `pm mcp`
subcommand. It works **directly on disk** through `pm-core` (no running `pm` GUI
required); a GUI that *is* open picks up writes through its filesystem watch.

## Run

```
pm mcp --project /path/to/repo          # installed
cargo run -q -p pm -- mcp --project .    # from source
```

The project root defaults to `--project` / a positional argument / the current
directory. Every tool also takes an optional `project` path argument that
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

`pm --setup` does this for you (`claude mcp add pm -s user -- pm --mcp`, or it
writes the entry into `~/.claude.json`). To wire it up by hand:

```json
{
  "mcpServers": {
    "pm": { "command": "pm", "args": ["--mcp"] }
  }
}
```

From a source checkout without an installed `pm`, use
`{ "command": "cargo", "args": ["run", "-q", "-p", "pm", "--", "--mcp"] }`.
