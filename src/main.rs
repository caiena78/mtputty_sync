use serde::Deserialize;
use serde_json::Value;
use std::{env, fs, process::Command};

#[derive(Deserialize)]
struct Config {
    mtputty_xml: String,
    root_folder: String,
    tree: Vec<String>, // "site" / "role"
    #[serde(default)]
    include_roles: Vec<String>,
    #[serde(default)]
    exclude_roles: Vec<String>,
    include_status: Vec<String>,
    skip_without_primary_ip: bool,
    #[serde(default)]
    exclude_sites: Vec<String>,
    #[serde(default)]
    strip_name_suffixes: Vec<String>,
    saved_session: String,
    default_protocol: String,
    #[serde(default)]
    telnet_if_custom_field: String,
}

struct Dev {
    path: Vec<String>, // folder names, outermost first
    name: String,
    ip: String,
    id: u64,
    telnet: bool,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn fetch(cfg: &Config) -> Result<Vec<Dev>, Box<dyn std::error::Error>> {
    let base = env::var("netbox_url")?.trim_end_matches('/').to_string();
    let token = env::var("netbox_token")?;
    let client = reqwest::blocking::Client::new();
    let mut url = Some(format!("{base}/api/dcim/devices/?limit=1000&exclude=config_context"));
    if cfg.skip_without_primary_ip {
        url = url.map(|u| u + "&has_primary_ip=true");
    }
    for s in &cfg.include_status {
        url = url.map(|u| format!("{u}&status={s}"));
    }
    let mut out = vec![];
    while let Some(u) = url {
        let page: Value = client
            .get(&u)
            .header("Authorization", format!("Token {token}"))
            .send()?
            .error_for_status()?
            .json()?;
        for d in page["results"].as_array().ok_or("unexpected NetBox response")? {
            let s = |v: &Value| v.as_str().unwrap_or("").to_string();
            let (site, role, mut name) = (s(&d["site"]["name"]), s(&d["role"]["name"]), s(&d["name"]));
            for suf in &cfg.strip_name_suffixes {
                if let Some(n) = name.strip_suffix(suf.as_str()) {
                    name = n.to_string();
                }
            }
            if cfg.exclude_sites.contains(&site)
                || cfg.exclude_roles.contains(&role)
                || (!cfg.include_roles.is_empty() && !cfg.include_roles.contains(&role))
                || name.is_empty()
            {
                continue;
            }
            // primary_ip.address is "10.0.0.1/22"
            let ip = s(&d["primary_ip"]["address"]).split('/').next().unwrap_or("").to_string();
            if ip.is_empty() && cfg.skip_without_primary_ip {
                continue;
            }
            let path = cfg
                .tree
                .iter()
                .map(|t| match t.as_str() {
                    "site" => Ok(site.clone()),
                    "role" => Ok(role.clone()),
                    o => Err(format!("unknown tree level '{o}' (use site/role)")),
                })
                .collect::<Result<_, _>>()?;
            let telnet = if cfg.telnet_if_custom_field.is_empty() {
                cfg.default_protocol == "telnet"
            } else {
                d["custom_fields"][&cfg.telnet_if_custom_field].as_bool().unwrap_or(cfg.default_protocol == "telnet")
            };
            out.push(Dev { path, name, ip, id: d["id"].as_u64().unwrap_or(0), telnet });
        }
        url = page["next"].as_str().map(String::from);
    }
    let key = |d: &Dev| (d.path.iter().map(|p| p.to_lowercase()).collect::<Vec<_>>(), d.name.to_lowercase());
    out.sort_by_key(key);
    Ok(out)
}

fn build(cfg: &Config, devs: &[Dev]) -> String {
    let mut x = String::new();
    let ind = |n: usize| "\t".repeat(3 + n);
    x += &format!("{}<Node Type=\"0\" Expanded=\"0\">\n{}<DisplayName>{}</DisplayName>\n", ind(0), ind(1), esc(&cfg.root_folder));
    let mut cur: Vec<&str> = vec![];
    for d in devs {
        let common = cur.iter().zip(&d.path).take_while(|(a, b)| *a == &b.as_str()).count();
        while cur.len() > common {
            cur.pop();
            x += &format!("{}</Node>\n", ind(cur.len() + 1));
        }
        for p in &d.path[common..] {
            x += &format!("{}<Node Type=\"0\" Expanded=\"0\">\n{}<DisplayName>{}</DisplayName>\n", ind(cur.len() + 1), ind(cur.len() + 2), esc(p));
            cur.push(p);
        }
        let (i, t) = (ind(cur.len() + 1), ind(cur.len() + 2));
        let (con, flag) = if d.telnet { (0, "-telnet") } else { (4, "-ssh") };
        x += &format!(
            "{i}<Node Type=\"1\">\n{t}<SavedSession>{}</SavedSession>\n{t}<DisplayName>{}</DisplayName>\n{t}<UID>{{4E425800-0000-0000-0000-{:012X}}}</UID>\n{t}<ServerName>{}</ServerName>\n{t}<PuttyConType>{con}</PuttyConType>\n{t}<Port>0</Port>\n{t}<UserName></UserName>\n{t}<Password></Password>\n{t}<PasswordDelay>0</PasswordDelay>\n{t}<CLParams>{} {flag}</CLParams>\n{t}<ScriptDelay>0</ScriptDelay>\n{i}</Node>\n",
            esc(&cfg.saved_session), esc(&d.name), d.id, esc(&d.ip), esc(&d.ip)
        );
    }
    for n in (0..cur.len()).rev() {
        x += &format!("{}</Node>\n", ind(n + 1));
    }
    x += &format!("{}</Node>\n", ind(0));
    x
}

/// Remove the existing top-level folder named `root` under <Putty> (depth-counting on <Node>/</Node>).
/// shortcut: text scan, assumes MTPutty never writes self-closing <Node/>; upgrade to an XML parser if it does.
fn strip_root(xml: &str, root: &str) -> String {
    let Some(mut pos) = xml.find("<Putty>") else { return xml.to_string() };
    pos += "<Putty>".len();
    let tag = format!("<DisplayName>{}</DisplayName>", esc(root));
    let (mut depth, mut start) = (0i32, None);
    while let Some(off) = xml[pos..].find(['<']) {
        let i = pos + off;
        let rest = &xml[i..];
        if rest.starts_with("</Putty>") {
            break;
        } else if rest.starts_with("<Node") {
            if depth == 0 && xml[i..].find('>').map(|e| xml[i + e + 1..].trim_start().starts_with(&tag)) == Some(true) {
                start = Some(i);
            }
            depth += 1;
        } else if rest.starts_with("</Node>") {
            depth -= 1;
            if depth == 0 {
                if let Some(s) = start {
                    let line_start = xml[..s].rfind('\n').map_or(s, |n| n + 1);
                    let end = i + "</Node>".len();
                    let end = xml[end..].find('\n').map_or(end, |n| end + n + 1);
                    return format!("{}{}", &xml[..line_start], &xml[end..]);
                }
            }
        }
        pos = i + 1;
    }
    xml.to_string()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let dry = args.iter().any(|a| a == "--dry-run");
    // explicit arg, else ./config.toml, else next to the exe, else the project folder
    let cfg_path = match args.iter().find(|a| !a.starts_with("--")) {
        Some(p) => std::path::PathBuf::from(p),
        None if std::path::Path::new("config.toml").exists() => "config.toml".into(),
        None => {
            let beside_exe = env::current_exe()?.with_file_name("config.toml");
            // shortcut: falls back to the source folder baked in at build time; fine for a personal tool
            if beside_exe.exists() { beside_exe } else { std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml") }
        }
    };
    let cfg: Config = toml::from_str(&fs::read_to_string(&cfg_path).map_err(|e| format!("{}: {e}", cfg_path.display()))?)?;

    let devs = fetch(&cfg)?;
    let mut block = build(&cfg, &devs);

    let xml = fs::read_to_string(&cfg.mtputty_xml)?;
    let crlf = xml.contains("\r\n");
    let xml = xml.replace("\r\n", "\n");
    let mut xml = strip_root(&xml, &cfg.root_folder);
    let at = xml.find("</Putty>").ok_or("no </Putty> in xml")?;
    let line_start = xml[..at].rfind('\n').map_or(at, |n| n + 1);
    xml.insert_str(line_start, &block);
    block.clear();
    if crlf {
        xml = xml.replace('\n', "\r\n");
    }

    if dry {
        println!("{} devices; dry run, nothing written", devs.len());
        fs::write("mtputty.dryrun.xml", xml)?;
        println!("preview: mtputty.dryrun.xml");
        return Ok(());
    }
    // MTPutty rewrites the file on exit and would silently discard this sync.
    let tl = Command::new("tasklist").args(["/FI", "IMAGENAME eq mtputty.exe", "/NH"]).output()?;
    if String::from_utf8_lossy(&tl.stdout).to_lowercase().contains("mtputty.exe") {
        return Err("MTPutty is running; close it and rerun".into());
    }
    fs::copy(&cfg.mtputty_xml, format!("{}.bak", cfg.mtputty_xml))?;
    fs::write(&cfg.mtputty_xml, xml)?;
    println!("wrote {} devices to {} (backup: .bak)", devs.len(), cfg.mtputty_xml);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_removes_only_root_and_is_idempotent() {
        let xml = "<Putty>\n\t<Node Type=\"0\">\n\t\t<DisplayName>NetBox</DisplayName>\n\t\t<Node Type=\"1\">\n\t\t</Node>\n\t</Node>\n\t<Node Type=\"0\">\n\t\t<DisplayName>keep</DisplayName>\n\t</Node>\n</Putty>\n";
        let out = strip_root(xml, "NetBox");
        assert!(!out.contains("NetBox") && out.contains("keep"));
        assert_eq!(strip_root(&out, "NetBox"), out);
    }
}
