# mtputty_sync

Builds the MTPutty session tree from NetBox. It reads sites, device roles and devices, then rewrites one top-level folder (default `NetBox`) in `mtputty.xml` as **Site > Role > Device**. Your hand-made folders and hosts are never touched.

## Quick start

```powershell
cd C:\path\to\mtputty_sync
cargo run --release -- --dry-run   # preview: writes mtputty.dryrun.xml, changes nothing
# close MTPutty, then:
cargo run --release                # real sync
```

Reopen MTPutty to see the `NetBox` folder.

## Getting started

### Prerequisites
- Rust (`cargo`) installed.
- Environment variables set in the shell you run from:
  - `netbox_url` - e.g. `https://netbox.example.org`
  - `netbox_token` - a NetBox API token with read access
- MTPutty's `mtputty.xml` (default: `%APPDATA%\TTYPlus\mtputty.xml`).

### Build
```powershell
cargo build --release
```
Produces `target\release\mtputty_sync.exe`.

### Run
```powershell
.\target\release\mtputty_sync.exe [path\to\config.toml] [--dry-run]
```
- Reads `config.toml` from the current directory, then the folder containing the exe, then the project folder it was built from, unless you pass a path.
- `--dry-run` writes `mtputty.dryrun.xml` next to where you run it and leaves the real file alone. Use it to check the result first.
- A real run saves the old file as `mtputty.xml.bak`, then rewrites `mtputty.xml` in place.
- It refuses to run while `mtputty.exe` is open. MTPutty rewrites the file on exit and would discard the sync.

### Package for distribution
```powershell
.\package.ps1
```
Builds the release exe and recreates `dist\` containing `mtputty_sync.exe`, `config.toml` and `README.md` side by side. Copy that folder to the target machine, then:
1. Edit `mtputty_xml` in `config.toml` to that user's own `mtputty.xml` path (the shipped value is a local path).
2. Set the `netbox_url` and `netbox_token` environment variables.
3. Run `.\mtputty_sync.exe --dry-run`, then `.\mtputty_sync.exe` with MTPutty closed.

### What gets written
- One top-level folder (`root_folder`), rebuilt from scratch each run. Anything you add inside it by hand is lost on the next sync.
- Each device has `ServerName` set to its NetBox primary IP, and `UserName` and `Password` left blank. Set those in MTPutty.
- The connection is SSH, or telnet when the NetBox custom field named in `telnet_if_custom_field` is true.
- Device UIDs come from the NetBox device id, so they stay the same between runs.

## Configuration (`config.toml`)

| Setting | What it does |
|---|---|
| `mtputty_xml` | Path to the MTPutty file to update |
| `root_folder` | Name of the folder the tool owns |
| `tree` | Folder levels, outermost first: `"site"`, `"role"` |
| `include_roles` | Only these roles. Empty means all roles except `exclude_roles` |
| `exclude_roles` | Roles to leave out (default: Access Point, IP Phone, Mitel, Unknown) |
| `include_status` | NetBox statuses to include (default: `active`) |
| `skip_without_primary_ip` | Skip devices with no primary IP |
| `exclude_sites` | Sites to leave out |
| `strip_name_suffixes` | Suffixes removed from displayed names, e.g. `["(1)"]` turns `sw-a1(1)` into `sw-a1` |
| `saved_session` | PuTTY saved session each entry launches |
| `default_protocol` | `"ssh"` or `"telnet"` |
| `telnet_if_custom_field` | NetBox custom field that forces telnet when true. `""` disables it |

Common edits:
- **Add a role to the tree:** delete it from `exclude_roles`.
- **Limit to a few roles:** set `include_roles = ["Core", "Firewall"]`.
- **Flatten the tree by role only:** set `tree = ["role"]`.

## Notes and limits
- Folder removal is a plain text scan of the XML. It works on current MTPutty files, but would break if MTPutty ever wrote self-closing `<Node/>` tags.
- If two devices in the same folder end up with the same displayed name after suffix stripping, both are kept.
- Tests: `cargo test`.
