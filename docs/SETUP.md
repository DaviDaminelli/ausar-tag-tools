# Setting up Ausar Tag Tools (for an AI assistant)

This page is written for an AI assistant (Claude Code or similar) that a person has asked to set up Ausar Tag Tools on their computer. Follow it step by step. Do every step you can yourself; ask the person only for what you can't find or decide alone. The person's own words in this conversation come first: if they asked for something different (another folder, no MCP client, only one build), do that.

The person can start it with: *"Set up Ausar Tag Tools for me, following SETUP.md in the zip I downloaded."*

The paths below are for Windows; Linux equivalents are given where they differ.

## Ground rules

- **Ask before installing anything** (a runtime, Git, an AI client). Say what it is and why it's needed, and let the person decide or do it.
- **Back up any file you change** that already exists (`<file>.bak-ausar-tag`), and check it still parses after the change (JSON, TOML).
- **Don't search whole drives for game files without asking.** A folder with `.module` files can be large and on any drive; ask the person where it is first.
- **Don't change security settings.** If Windows blocks or warns about the programs, explain the options (step 8) and let the person choose.
- **The tools only read files.** Nothing in this setup changes game files.
- Report what you did at the end: the paths you used, what you checked, and anything left for the person.

## 1. Find the release

The person downloaded a zip named `ausar-tag-tools-<version>-windows.zip` (or `-linux.zip`), usually in their Downloads folder; ask if you don't find it. It holds one folder with:

| File | What it is |
|---|---|
| `ausar-tag-mcp.exe` | The MCP server the AI client runs |
| `ausar-tag-cli.exe` | The command line: the same tools, plus `identify` |
| `h5tags.example.toml` | An example config, to copy as `h5tags.toml` |
| `README.md`, `SETUP.md` | How it works; this page |

If the person has the source repository instead of a zip, build the programs first ("Building from source", at the end), then continue with step 2.

Also find out which AI clients are installed: the `claude` command (Claude Code's command line), the Claude Code extension in VS Code, Claude Desktop. This decides step 7.

## 2. Install the programs in their own folder

Extract the zip, and copy the files from its folder into:

- Windows: `%LOCALAPPDATA%\Programs\ausar-tag-tools\` (the per-user programs folder; no administrator rights needed).
- Linux: `~/.local/share/ausar-tag-tools/` (or another folder the person prefers); make the two programs executable (`chmod +x`).

In PowerShell: `Expand-Archive <zip> -DestinationPath $env:TEMP\ausar-tag-tools`, then copy the inner folder's files to the programs folder.

**Check the one dependency (Windows):** run `ausar-tag-cli --version` from the programs folder. It should print the version. If it fails with "VCRUNTIME140.dll was not found", the computer lacks Microsoft's Visual C++ Redistributable (x64), the runtime the programs use; most computers have it. With the person's agreement, they install it (free, from Microsoft's website), then run the check again. If Windows blocks or warns instead, see step 8.

**Updating to a new version later:** disconnect or close the AI clients using the server (it keeps `ausar-tag-mcp.exe` open), copy the new files over the old ones, and keep the existing `h5tags.toml` (the zip never contains one). Then reconnect.

## 3. Get the definitions

The tools need Lord Zedd's H5Tags definitions: its `tags` folder (files like `(weap)weapon.xml`) and its `tag_resources` folder. They aren't in the zip. Ask the person whether they have them, and where.

If they don't, they're at https://github.com/Lord-Zedd/H5Tags. With the person's agreement, download the repository as a zip (the page's **Code → Download ZIP**, or `https://github.com/Lord-Zedd/H5Tags/archive/HEAD.zip`) and extract it into the programs folder, e.g. `…\ausar-tag-tools\H5Tags\`. Then find the `tags` and `tag_resources` folders inside it (they sit one folder deeper, in a folder named like `H5Tags-<commit ID>\`) and check that `tags` holds `.xml` files. No Git is needed.

A beta or other build may need its own definitions; if the person has some, use their folder as that build's `defs`.

## 4. Identify each game folder

Ask the person for each game build's deploy folder: the folder holding the `.module` files (anywhere below it; the tools search it recursively, whatever the layout). A person may have one build, several (Forge, Xbox One, beta builds), or other versions of them: configure only what they have.

Run `ausar-tag-cli identify <folder>` on each folder, from the programs folder. It needs no config:

```
folder: path D:\Halo5\deploy, modules 11
module formats: found 23 (11 modules, supported, xbox)
tag header formats: found 27 (8 modules, supported)
game versions: base yes, latest 97230-1
suggested build name: name xbox-97230
```

- **module formats:** 27 = Forge (PC), 23 = Xbox One. `not supported` (and a `not supported yet` line): this build's format can't be read yet. Tell the person; a sample `.module` from it would let it be added. Don't configure that folder.
- **tag header formats:** how many modules had a tag to sample; modules holding only a load manifest have none.
- **game versions:** the update layers found in the module names; `latest` is the newest.
- **suggested build name:** use it as the build's `name`, unless the person prefers another.
- `note: found no .module files under this folder`: wrong folder; ask again.

## 5. Write the config

Copy `h5tags.example.toml` to `h5tags.toml` in the programs folder, and fill it in: one `[[build]]` per folder from step 4, named as `identify` suggested (the first build is the default; names use `a-z`, `0-9`, `_`, `-`), with the definitions from step 3:

```toml
[[build]]
name = "forge-12719"
deploy = 'D:\Halo 5 Forge\deploy'
defs = 'C:\Users\<name>\AppData\Local\Programs\ausar-tag-tools\H5Tags\tags'
resource_defs = 'C:\Users\<name>\AppData\Local\Programs\ausar-tag-tools\H5Tags\tag_resources'
```

Use single quotes around Windows paths (TOML takes them as written). Remove the example's commented second build if there's no second build. `cache_dir` is optional: the cache (an index of the modules and two scans of the tags, rebuilt whenever the files change) goes to `%LOCALAPPDATA%\h5tags` by default, about 100 MB for a small sample and more for a full install; set it only if the person wants it on another drive.

## 6. Check it works

From the programs folder:

```
ausar-tag-cli --format text list-modules
ausar-tag-cli --format text search-tags assault_rifle --class weap
```

`list-modules` starts with the build's formats and latest game version; every module should say `ok`. The search should find weapons. The first run indexes every module and can take a while on a full install; later runs are quick. With several builds, add `--build <name>` to check each one. If a module says `!!`, tell the person what it says (an incomplete or damaged file is theirs to replace). If the programs don't start, see step 8.

## 7. Connect it to the AI client

Use the full paths of the programs folder.

- **Claude Code, with the `claude` command:**
  `claude mcp add --scope user h5tags -- <programs folder>\ausar-tag-mcp.exe --config <programs folder>\h5tags.toml`
  (`--scope user`: available in every project.)
- **Claude Code without the `claude` command** (for example only the VS Code extension): add the server to the top-level `"mcpServers"` object of `~/.claude.json` (`C:\Users\<name>\.claude.json`); create the object if it's missing. Back the file up first; change nothing else in it; check it is valid JSON afterwards, and that the backslashes in the paths survived (write them as `\\` in JSON).
  ```json
  "h5tags": {
    "type": "stdio",
    "command": "C:\\Users\\<name>\\AppData\\Local\\Programs\\ausar-tag-tools\\ausar-tag-mcp.exe",
    "args": ["--config", "C:\\Users\\<name>\\AppData\\Local\\Programs\\ausar-tag-tools\\h5tags.toml"],
    "env": {}
  }
  ```
- **Claude Desktop:** the same entry (without `"type"` and `"env"`) under `"mcpServers"` in `%APPDATA%\Claude\claude_desktop_config.json`.

Then ask the person to restart the client or start a new session, and check that `h5tags` shows as connected (`/mcp` in Claude Code). A new session sees the tools; the current one doesn't.

## 8. If Windows warns or blocks

The programs aren't signed yet. What the person may see, and the options to explain (the choice is theirs):

- **"Windows protected your PC" (SmartScreen)** when a program is started from Explorer: **More info → Run anyway**. Files downloaded from the internet carry a mark that triggers this; with the person's agreement, `Unblock-File` on the two programs removes that mark from those two files only.
- **Smart App Control** (Windows 11; the client reports a spawn error such as `uv_spawn`, or running the program says access is denied): it allows no exceptions for single programs, so the programs can't run on that computer while it's on. Turning it off affects the whole computer and, on some computers, can't be turned back on without resetting Windows; that is the person's decision. Tell whoever gave them the tools: a signed build would solve it.
- **"VCRUNTIME140.dll was not found"**: the Visual C++ Redistributable is missing; see the dependency check in step 2.

## 9. Finish

Tell the person:
- where the programs, the config, the definitions and the cache are;
- what the check in step 6 showed (builds, modules, tags found);
- that they need a new session for the tools, and an example to try: *"Find every tag related to the assault rifle."*
- that `README.md` explains how the tools work and lists every command.

## Building from source

Only for someone who has the source repository instead of a release zip.

1. Check for Rust: `cargo --version` (Rust 1.88 or later). If it's missing, ask the person to install it from https://rustup.rs (on Windows it also needs Microsoft's C++ build tools, which its installer offers). Don't install it yourself unless they ask you to.
2. In the repository folder: `cargo build --release`. It takes a few minutes the first time. The programs are `target\release\ausar-tag-cli.exe` and `target\release\ausar-tag-mcp.exe`; `dist\h5tags.example.toml` is the example config.
3. Continue with step 2, copying those files into the programs folder: an AI client keeps `ausar-tag-mcp.exe` open while it runs, so running it from `target\release` would block the next rebuild.
