# Ausar Tag Tools

Ausar Tag Tools (`ausar-tag-mcp` for AI assistants, `ausar-tag-cli` for you) reads the tags inside Halo 5 `.module` files: search, browse and read them, every field with its name, type, value, units, options and help text, and a clear reason whenever a value is empty or can't be read. Also: what references what, one field across a whole class, the differences between two tags, which maps load a tag, and every version of a tag across game updates.

Supported builds: **Halo 5: Forge** (Windows PC, module version 27) and **Halo 5: Guardians** (Xbox One, module version 23). Reading only: it never changes a file.

## How it works

The tools were built aiming at a fast, efficient and reliable local use with an agent. Mainly resting on four things:

- **Indexing.** The first run reads every module's file list into an index: each tag's path, class, size, resources, and the modules holding a copy of it. Later runs re-read only the modules whose files changed. Searching, browsing and listing use the index and don't open the modules.
- **Caches.** Besides the index, two results that take a full pass over the tags are kept on disk and reused until the files they depend on change: a scan of every tag's references and strings (behind `find_references` and `search_values`), and a check of every tag against its definition (behind `definition_status`). A running server also keeps recently read tags in memory (`tag_cache_mb`).
- **One tool per kind of question.** Finding: `search_tags`, `browse`, `search_definitions`, `search_values`. Structure: `outline`, `get_definition`. Reading: `read_tag`. Relations: `find_references`, `tag_copies`, `map_tags`, `which_maps`. Comparing: `query_field` (one field across every tag of a class, in one call), `diff_tags` (only the fields that differ between two tags). Status: `list_modules`, `list_classes`, `list_entries`, `list_resource_kinds`, `definition_status`.
- **Parameters that keep results small.** `path=` and `section=` read one part of a tag; `depth=` and `detail=brief|normal|full` set how much of it; `class=`, `module=` and `filter=` narrow lists; `offset` and `limit` page them. A whole read above about 3,000 tokens returns the tag's outline instead, unless `confirm_large=true`.

For reliability, results explain both the tags and their fields, so the agent doesn't have to guess what it reads:

- **Fields:** at the default detail, every value comes with its field's name, type, units and range from the definitions, and enum and flag values with their option names; `detail=full` adds the field's help text. `get_definition` explains a field without reading any tag, and `search_definitions` finds the fields that deal with a behaviour ("heat", "drop") across every class.
- **Tags:** what a tag stores besides its values (references to other tags, string IDs, data, resources, copies in update layers, load manifests) is shown and explained in plain words in the guide (`docs/AGENT_GUIDE.md`, built into the server).
- **Limits:** every empty, missing or unreadable value is marked as such (`(empty)`, `(not in indexed modules)`, `!!`), and the server's instructions tell the agent not to report a failed read as a fact and to say when a conclusion is inference.

## Guidelines followed

Two references shaped the server: the [Model Context Protocol specification](https://modelcontextprotocol.io/specification/latest) and Anthropic's article [Writing effective tools for AI agents — with agents](https://www.anthropic.com/engineering/writing-tools-for-agents) (2025). What each asks, and what the tools do:

**MCP specification**

- **Instructions and resources:** a short text sent once per connection (19 lines: which tool for which question, and what the markers mean), and the full guide as a resource (`h5tags://guide`, Markdown).
- **Tool schemas:** one JSON Schema per tool, generated from the same parameter types as the command line; a parameter the tool doesn't have is refused (`additionalProperties: false`), not ignored.
- **Annotations:** every tool is marked as reading local files only (`readOnlyHint`, `openWorldHint: false`), except `reindex`, which rebuilds the tools' own index and caches.
- **Errors:** a call that fails is a result with `isError` and a hint for the next step, so the agent can correct itself; an unknown tool or resource is a protocol error (Invalid Params).
- **Not used:** structured output (`outputSchema`); `format=json` returns JSON as text.

**Tool design (Anthropic's article)**

- **Tools for questions, not one per operation:** `query_field` answers "this field in every tag of a class" and `diff_tags` "where do these two tags differ" in one call each, where reading tags one by one would take dozens; `find_references` follows references level by level with `recursive=true`.
- **Meaningful context:** tag names next to global IDs, labels for tag block entries, block indices resolved to the entry they point to, and every marker explained in the guide.
- **Token efficiency:** `offset` / `limit` on long lists, filters, `path=` and `section=` for one part of a tag, `depth=` and `detail=brief|normal|full`. A whole read above about 3,000 tokens returns the tag's outline instead and says what to read next.
- **Errors that say what to do:** each error comes with a hint: the closest field names, a field's valid options, the filters it takes.
- **Descriptions written for the agent:** what the tool does, an example, the usual next call; parameters list their valid values.
- **Evaluation:** agent sessions on real questions (see Benchmarks); their transcripts and written reports were read point by point, and each change was checked on the game files ([docs/progress.md](docs/progress.md)).
- **Not done:** a name prefix such as `h5tags_` (clients already put the server's name in front of each tool); evaluation run as a program over held-out task sets.

## Benchmarks

Measured on the two samples this was developed with, release build, Windows 11 (no full game install measured yet):

| Sample                         | Modules | Tag copies | Classes |
| ------------------------------ | ------- | ---------- | ------- |
| Forge (PC), base module 1.1 GB | 10      | 62,976     | 210     |
| Xbox One                       | 11      | 58,846     | 215     |

**Response time**

| What                                                                    | Time                                        |
| ----------------------------------------------------------------------- | ------------------------------------------- |
| A tool call inside a running MCP session                                | 6–98 ms                                     |
| Read one tag or one part of it                                          | 10–22 ms                                    |
| Index every module, first run (Forge, 87,221 file entries)              | 0.18–0.25 s                                 |
| Scan every tag's references, first time (then cached)                   | 1.6 s (Forge), 1.4 s (Xbox)                 |
| Check every tag against its definition, first time / from the cache     | 1.2 s / 0.3 s (Forge), 1.5 s / 0.3 s (Xbox) |
| One field across all 118 weapons, as a command (program start included) | 2.7 s                                       |
| Compare two weapons; which maps load a tag; a map's tags (commands)     | 0.6–0.7 s each                              |

**Tokens per result** (≈ characters ÷ 4)

| Call                                                    | Tokens                                                       |
| ------------------------------------------------------- | ------------------------------------------------------------ |
| A whole weapon tag, if read whole (battle rifle)        | ≈ 8,750: the size guard returns its outline instead, ≈ 1,050 |
| One barrel of a weapon, brief                           | ≈ 1,100                                                      |
| Magazine size of every weapon (`query_field`)           | ≈ 1,260                                                      |
| Assault rifle vs SMG barrels (`diff_tags path=barrels`) | ≈ 1,740                                                      |
| Which maps load a tag (`which_maps`)                    | ≈ 30                                                         |

**Real questions, answered by Claude Code with the tools**

| Question                                                                                                                              | Tool calls | Tool results    | Result                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------------- | ---------- | --------------- | ---------------------------------------------------------------------------------------- |
| "Which tags reference this projectile?" (fresh session)                                                                               | 4          | ≈ 230 tokens    | correct                                                                                  |
| "What is the magazine size of every weapon?"                                                                                          | 8          | ≈ 6,200 tokens  | correct (spot-checked values match)                                                      |
| "What would it take to make the battle rifle use a battery, like the plasma pistol, and what else would that change?" (fresh session) | 36         | ≈ 21,300 tokens | correct change list; HUD, ammo pickups and the tags using it covered; inference labelled |
| "Would that battery level be kept if the weapon is dropped and picked up again?" (same session)                                       | 2          | ≈ 300 tokens    | engine behaviour named as such, related fields shown, a test to confirm in game          |

**Checks behind "reliable"**

- Every tag copy of both samples read with its definition: all 62,976 Forge copies read completely (some with the harmless GUID label warning); of 58,846 Xbox copies, 19 in 5 classes don't fit their definitions: they are read only up to the last field their tag header confirms, the rest reported as not read, never misread. Where a fitting definition exists to compare (`wigl`), every value read that way matches it: 19,634 of 19,634.
- "What references this tag" matches an independent pass over every target: 76,724 (Forge) and 68,184 (Xbox).
- Values match what a player knows: the magazine sizes of the battle rifle, assault rifle, carbine and railgun, the battle rifle's 3-round burst, and the grenade limit (5 of 5). The answers in the table above were re-checked value by value against the tools.

Every measurement, with its date and method, is in [docs/progress.md](docs/progress.md).

## Requirements

- Release zips: Windows x64 (a Linux x64 zip on request). Building from source also works on macOS and on ARM64.
- On Windows: Microsoft's **Visual C++ Redistributable** (x64), the runtime the programs use. Most PCs already have it, since many programs install it; without it, the programs don't start ("VCRUNTIME140.dll was not found"). It's free from Microsoft.
- For each build: its deploy folder, the folder that holds the `.module` files.
- Lord Zedd's [H5Tags](https://github.com/Lord-Zedd/H5Tags) definitions: its `tags` folder (and `tag_resources`). They name and type every field; without them tags can be listed but not read field by field.

## Let your AI assistant set it up

Say:

> Set up Ausar Tag Tools for me, following SETUP.md in the zip I downloaded.

(With the source repository instead of a release zip: "…following docs/SETUP.md in the repository.") It installs the programs, asks you where your game files and definitions are, identifies each game folder, writes the config, checks it, and connects the tools to your AI client. The steps below are the same, done by hand.

## Install

- Download a release binary when one is published, or
- build it: `cargo build --release` (Rust 1.88 or later). This makes two programs in `target/release`: `ausar-tag-cli` (the command line, 4.5 MB) and `ausar-tag-mcp` (the MCP server, 6.9 MB). Nothing else needs installing besides the requirements above.
- Copy both to a folder of their own, for example `%LOCALAPPDATA%\Programs\h5tags` on Windows (no administrator rights needed), and put the config next to them. An AI client keeps `ausar-tag-mcp` open while it runs, so a program in `target\release` can't be replaced by a rebuild until the client lets it go.

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

Not sure which build a folder is? `ausar-tag-cli identify <folder>` (no config needed) shows its module formats (27 = Forge PC, 23 = Xbox One; others aren't supported yet), the game versions in its module names, and suggests a build name such as `xbox-97230`.

## Check it works

Without any AI client:

```
ausar-tag-cli --format text list-modules
ausar-tag-cli --format text search-tags assault_rifle --class weap
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

Every module should say `ok`. `ausar-tag-cli --help` lists the commands; each command has `--help` too.

## Connect it to an AI client

### Claude Code

```
claude mcp add h5tags -- C:\h5tags\ausar-tag-mcp.exe --config C:\h5tags\h5tags.toml
```

- Without `--scope`, the server is available in the current project only (local scope). `claude mcp add --scope user h5tags -- …` makes it available in every project; `--scope project` writes it to the project's `.mcp.json` for everyone working on it.
- Check it with `claude mcp list`, or `/mcp` inside Claude Code.

### Claude Desktop

Add an entry to `claude_desktop_config.json` (Windows: `%APPDATA%\Claude\`, macOS: `~/Library/Application Support/Claude/`), then restart Claude Desktop:

```json
{
  "mcpServers": {
    "h5tags": {
      "command": "C:\\h5tags\\ausar-tag-mcp.exe",
      "args": ["--config", "C:\\h5tags\\h5tags.toml"]
    }
  }
}
```

### Other MCP clients

`ausar-tag-mcp` is a stdio server: run `ausar-tag-mcp --config <file>` (or `--deploy … --defs …`) as the server command.

## First steps

Ask the assistant, for example:

- "Find every tag related to the assault rifle."
- "Show the firing settings of the battle rifle's first barrel, and every possible trigger mode."
- "What's different between the assault rifle's and the SMG's barrels?"

How the assistant reads the results (and how to read them yourself): [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md). The server also gives it that guide as the resource `h5tags://guide`.

## Commands

The assistant and the command line have the same tools: over MCP they're named with `_` (`read_tag`), on the command line with `-` (`ausar-tag-cli read-tag`). Three commands exist only on the command line. `ausar-tag-cli <command> --help` lists a command's parameters; `--format text` shows exactly what the assistant reads.

| Command               | What it answers                                                                          |
| --------------------- | ---------------------------------------------------------------------------------------- |
| `search-tags`         | Tags by part of their path, class or module                                              |
| `browse`              | Tag paths as folders: subfolders and the tags in each                                    |
| `list-classes`        | Every class (also those with only a definition), its tag count; `query=` by code or name |
| `list-modules`        | Every module and its status; the build's formats and latest game version                 |
| `list-entries`        | File entries (tags, assets, other files) by kind and size, from the index only           |
| `list-resource-kinds` | Every asset kind, with counts and total size                                             |
| `read-tag`            | A tag's fields: values, units, options; whole, or one `path=` or `section=`              |
| `get-definition`      | A class's fields from its definition: types, units, ranges, options, help                |
| `outline`             | The section tree of a tag or a class, with field paths and entry counts                  |
| `search-definitions`  | Which classes and fields deal with something, across every definition                    |
| `definition-status`   | How well the definitions fit each class's tags; the gap list for a class                 |
| `find-references`     | What a tag references, and what references it, with the fields (recursively if asked)    |
| `tag-copies`          | Every copy of a tag across modules and update layers                                     |
| `search-values`       | Tags holding a string_id name or a referenced tag's name                                 |
| `query-field`         | One field across every tag of a class, with `filter=`, `sort=` and more fields (`also=`) |
| `diff-tags`           | The fields that differ between two tags of a class; block entries matched by label       |
| `map-tags`            | The tags a map loads (its load manifest)                                                 |
| `which-maps`          | The maps that load a tag (`module=` for some of them)                                    |
| `reindex`             | Rebuilds the index or the caches (rarely needed: they update themselves)                 |
| `identify`            | Command line only: what a folder of modules is, and a build name for it; needs no config |
| `header-dump`         | Command line only: a tag's header lists as stored (for development)                      |
| `preferences`         | Command line only: colors and the progress display                                       |

## Guides

| Document                                                                                                                   | For                                                                | What it covers                                                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [docs/AGENT_GUIDE.md](docs/AGENT_GUIDE.md)                                                                                 | The assistant (built into the server as `h5tags://guide`), and you | How to research with the tools (which tool for which question, what a change affects), how to read every result, column and marker, and what each part of a tag means |
| [docs/SETUP.md](docs/SETUP.md)                                                                                             | An AI assistant setting this up for you                            | Step-by-step setup: programs, folders, config, checks, connecting the client                                                                                          |
| [docs/PRD.md](docs/PRD.md)                                                                                                 | Development                                                        | What the tools must do, and the milestones                                                                                                                            |
| [docs/decisions.md](docs/decisions.md), [docs/format-notes.md](docs/format-notes.md), [docs/progress.md](docs/progress.md) | Development                                                        | Why the tools behave as they do; how each format fact was verified; every check and measurement, dated                                                                |

## Troubleshooting

- **The server isn't listed / doesn't start:** check the path to `ausar-tag-mcp` and the config file in the client's settings, then restart the client. Running the same command in a terminal shows any config error (it is printed before the server starts).
- **Windows says the program was blocked, or the client reports a spawn error (`uv_spawn`):** Windows 11's Smart App Control can block programs you build yourself, since they aren't signed. Microsoft documents no way to allow a single program; it lets apps run that its cloud service judges safe or that carry a valid signature from a trusted provider. A rebuild produces a new file that is judged again; a debug build (`cargo build`, `target\debug\`) is a separate file
- **"no definition for class …":** the `defs` path doesn't point to H5Tags' `tags` folder, or that class has no definition.
- **The first call after starting is slow:** the server is still loading or building the index; later calls will use it and are way faster.
- **Logs** go to stderr, never stdout (stdout carries the MCP protocol). `H5TAGS_LOG=debug` shows more. Claude Code shows them with `claude --debug`; Claude Desktop writes them to its logs folder (`mcp-server-h5tags.log`).

## Legal

**Evaluation copy.** Ausar Tag Tools is shared with invited testers for evaluation. All rights reserved: please don't redistribute it or its source without permission.

h5tags contains no game files, no definitions and no third-party code; you supply the game files and the H5Tags definitions yourself. The module format comes from [AusarDocs](https://github.com/ElDewrito/AusarDocs) and from tests on real files; field names and types come from Lord Zedd's [H5Tags](https://github.com/Lord-Zedd/H5Tags). [Reclaimer](https://github.com/Gravemind2401/Reclaimer) was a reference for how a tag viewer behaves; none of its code is used.
