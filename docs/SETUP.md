# Setting up Ausar Tag Tools (for an AI assistant)

For an AI assistant asked to set up Ausar Tag Tools on someone's computer. Do each step yourself; ask the person only for what you can't find or decide. Their own wishes come first. Paths are for Windows.

**Rules:** ask before installing anything; back up any file you change (`<file>.bak-ausar-tag`) and check it still parses; don't search drives for game files without asking; don't change security settings; the tools only read files.

## Steps

1. **Find the zip** `ausar-tag-tools-<version>-windows.zip` (usually in Downloads). It holds `ausar-tag-mcp.exe` (the MCP server), `ausar-tag-cli.exe` (the command line), `h5tags.example.toml`, `README.md` and this file. Note which AI clients are installed (the `claude` command, Claude Code in VS Code, Claude Desktop).
2. **Install:** extract it and copy its files into `%LOCALAPPDATA%\Programs\ausar-tag-tools\`. Check it starts: `ausar-tag-cli --version`. If it says "VCRUNTIME140.dll was not found", the person needs Microsoft's free Visual C++ Redistributable (x64); with their agreement they install it, then check again.
3. **Definitions:** Lord Zedd's H5Tags, its `tags` and `tag_resources` folders. Ask if the person has them. If not, with their agreement, download https://github.com/Lord-Zedd/H5Tags/archive/HEAD.zip and extract it into the programs folder; the two folders are inside `H5Tags-<commit ID>\`. No Git needed. A beta build may need its own definitions.
4. **Identify each game folder** the person has (the folder above their `.module` files; any layout): `ausar-tag-cli --format text identify <folder>` (without `--format text` it prints JSON). It shows the module format (27 = Forge, 23 = Xbox One; `not supported`: don't configure that folder, tell the person), the latest game version, and a suggested build name. `found no .module files`: wrong folder.
5. **Config:** copy `h5tags.example.toml` to `h5tags.toml` in the programs folder. One `[[build]]` per folder, named as `identify` suggested, with the definitions paths; single quotes around Windows paths. Configure only the builds the person has.
6. **Check:** `ausar-tag-cli --format text list-modules` (every module `ok`) and `ausar-tag-cli --format text search-tags assault_rifle --class weap` (finds weapons). For another build, `--build <name>` goes after the command: `list-modules --build xbox-97230`. The first run indexes the modules and can take a while on a full install.
7. **Connect:** register `ausar-tag-mcp.exe --config <programs folder>\h5tags.toml` under a name not already used (e.g. `ausar-tag`):
   - with the `claude` command: `claude mcp add --scope user ausar-tag -- <programs folder>\ausar-tag-mcp.exe --config <programs folder>\h5tags.toml`;
   - otherwise, an entry in the top-level `"mcpServers"` of `C:\Users\<name>\.claude.json` (`"type": "stdio"`, `"command"`, `"args"`; backslashes written as `\\`), or of `%APPDATA%\Claude\claude_desktop_config.json` for Claude Desktop.

   Then the person starts a new session; `/mcp` should show it connected. Note: the server reports its name as `h5tags-mcp` (an internal name, unchanged for now); that's expected.
8. **If Windows warns or blocks:** SmartScreen ("Windows protected your PC"): More info → Run anyway, or with the person's agreement `Unblock-File` on the two programs. Smart App Control (a spawn error such as `uv_spawn`, or access denied): it allows no exceptions; turning it off is the person's decision. Tell whoever gave them the tools.
9. **Finish:** tell the person where the programs, config, definitions and cache (`%LOCALAPPDATA%\h5tags`) are, what the checks showed, and to try in a new session: *"Find every tag related to the assault rifle."*

**Updating later:** disconnect the client, copy the new zip's files over the old ones, keep `h5tags.toml`.

**From the source repository instead of a zip:** `cargo build --release` (Rust 1.88+; ask before installing it), then use `target\release\ausar-tag-*.exe` and `dist\h5tags.example.toml` as the zip's files in step 2, copied out of `target` (a running server would block the next build).
