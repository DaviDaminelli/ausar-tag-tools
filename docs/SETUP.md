# Setting up h5tags (for an AI assistant)

This page is written for an AI assistant (Claude Code or similar) that a person has asked to set up h5tags on their computer. Follow it step by step. Do every step you can yourself; ask the person only for what you can't find or decide alone. The person's own words in this conversation come first: if they asked for something different (another folder, no MCP client, a second build), do that.

The person can start it with: *"Set up h5tags for me, following docs/SETUP.md in the h5tags folder."*

## Ground rules

- **Ask before installing anything** (Rust, Git, a client). Say what it is, why it's needed and how big it is, and let the person decide or do it.
- **Back up any file you change** that already exists (`<file>.bak-h5tags`), and check it still parses after the change (JSON, TOML).
- **Don't search whole drives for game files without asking.** A folder with `.module` files can be large and on any drive; ask the person where it is first.
- **h5tags only reads files.** Nothing in this setup changes game files.
- Report what you did at the end: the paths you used, what you checked, and anything left for the person.

## 1. Find out what's there

1. The operating system (the paths below are for Windows; macOS and Linux equivalents are given where they differ).
2. Whether the programs already exist: `h5tags-mcp` and `h5tags` (`.exe` on Windows), prebuilt or in the h5tags folder's `target\release\`. If they exist, skip step 2.
3. Which AI clients are installed: the `claude` command (Claude Code's command line), the Claude Code extension in VS Code, Claude Desktop. This decides step 6.

## 2. Build the programs (only if there are none)

1. Check for Rust: `cargo --version` (Rust 1.88 or later). If it's missing, ask the person to install it from https://rustup.rs (on Windows it also needs Microsoft's C++ build tools, which its installer offers), or to get prebuilt programs from whoever gave them h5tags. Don't install it yourself unless they ask you to.
2. In the h5tags folder: `cargo build --release`. It takes a few minutes the first time. The programs are `target\release\h5tags.exe` and `target\release\h5tags-mcp.exe`.

## 3. Install them in their own folder

Copy both programs out of the build folder, so that rebuilding h5tags later never replaces a program an AI client is running:

- Windows: `%LOCALAPPDATA%\Programs\h5tags\` (the per-user programs folder; no administrator rights needed).
- macOS and Linux: `~/.local/share/h5tags/` (or another folder the person prefers).

To update later: close or disconnect the AI clients using h5tags, copy the new programs over the old ones, reconnect.

## 4. Find the definitions and the game files

h5tags needs two things the person supplies. Ask for both paths; offer to look in a folder they name.

- **The definitions:** Lord Zedd's H5Tags, its `tags` folder (holding files like `(weap)weapon.xml`) and its `tag_resources` folder. If the person doesn't have them, they are at https://github.com/Lord-Zedd/H5Tags; with their agreement, `git clone` it next to the programs.
- **Each build's deploy folder:** the folder holding the `.module` files (anywhere below it; h5tags searches it recursively, whatever the layout). Forge (PC) modules are version 27, Xbox One modules version 23. A person may have one build or both.

Check each path exists and holds what it should (`.xml` files in `tags`; `.module` files below the deploy folder) before writing the config.

## 5. Write the config and check it

Write `h5tags.toml` next to the programs (step 3's folder). One `[[build]]` per deploy folder; the first is the default. Names use `a-z`, `0-9`, `_`, `-`.

```toml
[[build]]
name = "forge"
deploy = 'D:\Halo 5 Forge\deploy'
defs = 'C:\H5Tags\tags'
resource_defs = 'C:\H5Tags\tag_resources'
```

Use single quotes around Windows paths (TOML takes them as written). `cache_dir` is optional: the cache (an index of the modules, rebuilt whenever they change) goes to `%LOCALAPPDATA%\h5tags` by default, about 100 MB for a small sample and more for a full install; set `cache_dir` only if the person wants it on another drive.

Then check, from the programs' folder:

```
h5tags --format text list-modules
h5tags --format text search-tags assault_rifle --class weap
```

Every module should say `ok`, and the search should find weapons. The first run indexes every module and can take a while on a full install. If a module says `!!`, tell the person what it says (an incomplete or damaged file is theirs to replace). If the programs won't start at all on Windows, see "Smart App Control" below.

## 6. Connect it to the AI client

Use the full paths of step 3's programs and config.

- **Claude Code, with the `claude` command:**
  `claude mcp add --scope user h5tags -- <programs folder>\h5tags-mcp.exe --config <programs folder>\h5tags.toml`
  (`--scope user`: available in every project.)
- **Claude Code without the `claude` command** (for example only the VS Code extension): add the server to the top-level `"mcpServers"` object of `~/.claude.json` (`C:\Users\<name>\.claude.json`); create the object if it's missing. Back the file up first; change nothing else in it; check it is valid JSON afterwards, and that the backslashes in the paths survived (write them as `\\` in JSON).
  ```json
  "h5tags": {
    "type": "stdio",
    "command": "C:\\Users\\<name>\\AppData\\Local\\Programs\\h5tags\\h5tags-mcp.exe",
    "args": ["--config", "C:\\Users\\<name>\\AppData\\Local\\Programs\\h5tags\\h5tags.toml"],
    "env": {}
  }
  ```
- **Claude Desktop:** the same entry (without `"type"` and `"env"`) under `"mcpServers"` in `%APPDATA%\Claude\claude_desktop_config.json` (macOS: `~/Library/Application Support/Claude/`).

Then ask the person to restart the client or start a new session, and check that `h5tags` shows as connected (`/mcp` in Claude Code). A new session sees the tools; the current one doesn't.

## 7. Smart App Control (Windows 11)

Programs built on the person's own computer aren't signed, and Smart App Control can block them: the client then reports a spawn error (`uv_spawn`), or running the program says access is denied. There's no way to allow a single program. Tell the person; don't change security settings yourself. Rebuilding makes a new file that Windows judges again, and a debug build (`cargo build`, `target\debug\`) is a separate file that is sometimes allowed. Turning Smart App Control off affects the whole computer and, on some computers, can't be undone without resetting Windows: that is the person's decision.

## 8. Finish

Tell the person:
- where the programs, the config and the cache are;
- what the check in step 5 showed (modules, tags found);
- that they need a new session for the tools, and an example to try: *"Find every tag related to the assault rifle."*
- that the README's "First steps" and `docs/AGENT_GUIDE.md` say how the tools are used.
