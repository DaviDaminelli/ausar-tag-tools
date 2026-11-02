# h5tags

`h5tags` reads the tags inside Halo 5 `.module` files and lets an AI assistant (Claude, over MCP) or you (on the command line) search, browse and read them: every field with its name, type, value, units, options and help text, and a clear reason whenever a value is empty or can't be read.

Supported builds: **Halo 5: Forge** (Windows PC, module version 27) and **Halo 5: Guardians** (Xbox One, module version 23). Reading only: h5tags never changes a file.

## Requirements

- Windows, Linux or macOS (x64 or ARM64).
- For each build: its deploy folder, the folder that holds the `.module` files.
- Lord Zedd's [H5Tags](https://github.com/Lord-Zedd/H5Tags) definitions: its `tags` folder (and `tag_resources`). They name and type every field; without them tags can be listed but not read field by field.

## Let your AI assistant set it up

With Claude Code (or another assistant that can run commands), say:

> Set up h5tags for me, following docs/SETUP.md in the h5tags folder.

It builds or finds the programs, installs them, asks you where your game files and definitions are, writes the config, checks it, and connects h5tags to your AI client. The steps below are the same, done by hand.

## Install

- Download a release binary when one is published, or
- build it: `cargo build --release` (Rust 1.88 or later). This makes two programs in `target/release`: `h5tags` (the command line, 4.5 MB) and `h5tags-mcp` (the MCP server, 6.9 MB). Nothing else needs installing.
- Copy both to a folder of their own, for example `%LOCALAPPDATA%\Programs\h5tags` on Windows (no administrator rights needed), and put the config next to them. An AI client keeps `h5tags-mcp` open while it runs, so a program in `target\release` can't be replaced by a rebuild until the client lets it go.

## Configure

Write an `h5tags.toml`; by default the programs look for it next to themselves (`--config <file>` points elsewhere). Relative paths are relative to the config file.

```toml
cache_dir = 'C:\h5tags\cache'          # where the index, header scan and definition check are kept; default: your OS cache folder (Windows: %LOCALAPPDATA%\h5tags)
tag_cache_mb = 256                     # decompressed tags kept in memory by the MCP server; default 256
large_read_tokens = 3000               # a whole read_tag bigger than this returns the outline instead; default 3000
guid_check = "warn"                    # a definition GUID mismatch: off, warn (default: shown, values kept) or enforce (values not shown)
# notes_file = '...'                   # accepted and ignored for now

[[build]]                              # the first build is the default one
name = "forge"                         # a-z, 0-9, _ and -; unique
deploy = 'D:\Halo 5 Forge\deploy'      # the folder with the .module files
defs = 'C:\H5Tags\tags'
resource_defs = 'C:\H5Tags\tag_resources'

[[build]]
name = "xbox"
deploy = 'D:\h5_xbox\deploy'
defs = 'C:\H5Tags\tags'                # the same definitions, unless a build needs its own copy
resource_defs = 'C:\H5Tags\tag_resources'
```

For a single build without a config file: `--deploy <folder> --defs <folder> [--resource-defs <folder>]`. `--cache-dir <folder>` overrides `cache_dir`.

The first run indexes every module (about 0.2 s for a 1.1 GB folder of modules); later runs reuse the index and only re-read modules whose files changed.

## Check it works

Without any AI client:

```
h5tags --format text list-modules
h5tags --format text search-tags assault_rifle --class weap
```

Correct output looks like this (your modules and counts will differ):

```
total: 10
module	version	tags	load manifest	status
any\levels\globals-rtx-1.module	27	62334	no	ok
…

total: 4
in objects\weapons\rifle\:
global ID	class	display name	copies
0x00008595	weap	assault_rifle\assault_rifle.weapon	2
…
```

Every module should say `ok`. `h5tags --help` lists the commands; each command has `--help` too.

## Connect it to an AI client

### Claude Code

```
claude mcp add h5tags -- C:\h5tags\h5tags-mcp.exe --config C:\h5tags\h5tags.toml
```

- Without `--scope`, the server is available in the current project only (local scope). `claude mcp add --scope user h5tags -- …` makes it available in every project; `--scope project` writes it to the project's `.mcp.json` for everyone working on it.
- Check it with `claude mcp list`, or `/mcp` inside Claude Code.

### Claude Desktop

Add an entry to `claude_desktop_config.json` (Windows: `%APPDATA%\Claude\`, macOS: `~/Library/Application Support/Claude/`), then restart Claude Desktop:

```json
{
  "mcpServers": {
    "h5tags": {
      "command": "C:\\h5tags\\h5tags-mcp.exe",
      "args": ["--config", "C:\\h5tags\\h5tags.toml"]
    }
  }
}
```

### Other MCP clients

`h5tags-mcp` is a stdio server: run `h5tags-mcp --config <file>` (or `--deploy … --defs …`) as the server command.

## First steps

Ask the assistant, for example:

- "Find every tag related to the assault rifle."
- "Show the firing settings of the battle rifle's first barrel, and every possible trigger mode."
- "What's different between the assault rifle's and the SMG's barrels?"

How the assistant reads the results (and how to read them yourself): [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md). The server also gives it that guide as the resource `h5tags://guide`.

## Troubleshooting

- **The server isn't listed / doesn't start:** check the path to `h5tags-mcp` and the config file in the client's settings, then restart the client. Running the same command in a terminal shows any config error (it is printed before the server starts).
- **Windows says the program was blocked, or the client reports a spawn error (`uv_spawn`):** Windows 11's Smart App Control can block programs you build yourself, since they aren't signed. Microsoft documents no way to allow a single program; it lets apps run that its cloud service judges safe or that carry a valid signature from a trusted provider. A rebuild produces a new file that is judged again; a debug build (`cargo build`, `target\debug\`) is a separate file. Turning Smart App Control off affects the whole PC, and on some PCs it can't be turned back on without resetting Windows.
- **"no definition for class …":** the `defs` path doesn't point to H5Tags' `tags` folder, or that class has no definition.
- **The first call after starting is slow:** the server is still loading or building the index; later calls are fast.
- **Logs** go to stderr, never stdout (stdout carries the MCP protocol). `H5TAGS_LOG=debug` shows more. Claude Code shows them with `claude --debug`; Claude Desktop writes them to its logs folder (`mcp-server-h5tags.log`).

## Legal

h5tags contains no game files, no definitions and no third-party code; you supply the game files and the H5Tags definitions yourself. The module format comes from [AusarDocs](https://github.com/ElDewrito/AusarDocs) and from tests on real files; field names and types come from Lord Zedd's [H5Tags](https://github.com/Lord-Zedd/H5Tags). [Reclaimer](https://github.com/Gravemind2401/Reclaimer) was a reference for how a tag viewer behaves; none of its code is used.
