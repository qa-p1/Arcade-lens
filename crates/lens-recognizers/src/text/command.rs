//! Shell commands and their risks.
//!
//! Commands are recognized per line (after stripping shell prompts and
//! joining `\` continuations). Every command is analyzed for risky
//! constructs; "Run in Terminal" always asks for confirmation and shows
//! these risks, because OCR text must never be silently executed.

use std::sync::LazyLock;

use lens_core::value::{CommandRisk, CommandValue};
use lens_core::{caps, Detection, RecognizeContext, Value};
use regex::Regex;

use super::TextInput;

/// Unmistakable prompts: `$ `, `user@host:~$ `, `PS C:\> `, `❯ `, `(venv) $ `.
static PROMPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:PS [A-Za-z]:\\[^>]*>\s?|[\w.\-]+@[\w.\-]+(?::[^$#\s]*)?\s?[$#]\s?|[$❯➜]\s+|\(\w[\w.\-]*\)\s*[$#]\s)").unwrap()
});
/// Prompts that are also markdown/comment syntax (`# `, `> `, `% `): stripped,
/// but the line still has to look like a command.
static WEAK_PROMPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[#%>»]\s+").unwrap());
static SUBSHELL_DOWNLOAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:bash|sh|zsh)\s+(?:-c\s+)?["'<]*\s*(?:<\(|\$\()\s*(?:curl|wget)"#).unwrap());
static INVOKE_EXPRESSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:iex|invoke-expression)\b").unwrap());
static ENV_ASSIGN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^[A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S*)$"#).unwrap());

const COMMANDS: &[&str] = &[
    "sudo", "doas", "su", "pkexec", "git", "gh", "npm", "npx", "pnpm", "yarn", "bun", "deno", "node", "cargo", "rustup", "rustc", "go", "python", "python3",
    "pip", "pip3", "pipx", "uv", "poetry", "conda", "ruby", "gem", "bundle", "java", "javac", "mvn", "gradle", "dotnet", "make", "cmake", "ninja", "gcc",
    "g++", "clang", "apt", "apt-get", "dpkg", "dnf", "yum", "rpm", "pacman", "yay", "paru", "zypper", "apk", "brew", "port", "snap", "flatpak", "nix",
    "nix-env", "choco", "winget", "scoop", "docker", "podman", "kubectl", "helm", "terraform", "ansible", "ssh", "scp", "rsync", "sftp", "curl", "wget",
    "ls", "cd", "pwd", "cat", "less", "head", "tail", "grep", "rg", "find", "fd", "sed", "awk", "sort", "uniq", "wc", "cut", "tr", "xargs", "tee", "echo",
    "printf", "touch", "mkdir", "rmdir", "rm", "cp", "mv", "ln", "chmod", "chown", "chgrp", "tar", "zip", "unzip", "gzip", "gunzip", "7z", "dd", "mount",
    "umount", "mkfs", "fdisk", "parted", "lsblk", "df", "du", "free", "top", "htop", "ps", "kill", "killall", "pkill", "systemctl", "journalctl",
    "service", "crontab", "ping", "traceroute", "dig", "nslookup", "host", "whois", "ip", "ifconfig", "netstat", "ss", "nc", "nmap", "openssl", "gpg",
    "ssh-keygen", "export", "source", "alias", "which", "whereis", "man", "code", "vim", "nvim", "nano", "emacs", "tmux", "screen", "bash", "sh", "zsh",
    "fish", "pwsh", "powershell", "Get-ChildItem", "Set-Location", "Remove-Item", "Invoke-WebRequest", "iwr", "irm", "iex", "Start-Process", "flutter",
    "dart", "adb", "fastboot", "xcodebuild", "swift", "ffmpeg", "convert", "magick", "jq", "yq", "psql", "mysql", "sqlite3", "redis-cli", "mongosh",
];
/// Commands that are also English words; they need command-like arguments.
const WORDY: &[&str] = &["make", "find", "man", "top", "free", "kill", "sort", "cut", "less", "head", "tail", "host", "source", "screen", "export", "convert", "echo", "touch", "go", "code"];
const STOPWORDS: &[&str] = &["the", "a", "an", "to", "and", "you", "is", "of", "that", "this", "with", "for", "your", "it", "be", "are", "will", "can", "sure"];
const INTERPRETERS: &[&str] = &["sh", "bash", "zsh", "fish", "dash", "ksh", "python", "python3", "perl", "ruby", "node", "php", "iex", "invoke-expression", "pwsh", "powershell"];
const PACKAGE_MANAGERS: &[&str] = &[
    "apt", "apt-get", "dpkg", "dnf", "yum", "rpm", "pacman", "yay", "paru", "zypper", "apk", "brew", "port", "snap", "flatpak", "nix-env", "choco", "winget", "scoop",
    "pip", "pip3", "pipx", "npm", "pnpm", "yarn", "gem", "cargo", "go", "uv", "conda",
];
const PKG_VERBS: &[&str] = &["install", "i", "add", "remove", "rm", "uninstall", "purge", "autoremove", "upgrade", "update", "reinstall", "erase", "-S", "-Syu", "-Sy", "-R", "-Rs", "-Rns", "-U", "-i", "-e", "global"];

/// Logical command lines: prompts stripped, `\` / backtick continuations joined.
fn command_lines(text: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut pending = String::new();
    let mut prompted = false;
    for line in text.lines() {
        let (body, had_prompt) = if pending.is_empty() {
            match (PROMPT.find(line), WEAK_PROMPT.find(line)) {
                (Some(m), _) => (&line[m.end()..], true),
                (None, Some(m)) => (&line[m.end()..], false),
                (None, None) => (line, false),
            }
        } else {
            (line.trim_start(), prompted)
        };
        let trimmed = body.trim_end();
        if let Some(cont) = trimmed.strip_suffix('\\').or_else(|| trimmed.strip_suffix(" `")) {
            pending.push_str(cont.trim_end());
            pending.push(' ');
            prompted = had_prompt;
            continue;
        }
        pending.push_str(trimmed.trim_start());
        out.push((std::mem::take(&mut pending), had_prompt));
        prompted = false;
    }
    if !pending.is_empty() {
        out.push((pending.trim_end().to_string(), prompted));
    }
    out
}

/// Splits on unquoted whitespace and returns tokens (quotes removed).
fn tokens(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in cmd.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Text outside quotes, for operator detection.
fn unquoted(cmd: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => out.push(' '),
            None if c == '"' || c == '\'' => quote = Some(c),
            None => out.push(c),
        }
    }
    out
}

fn base_name(tok: &str) -> &str {
    tok.rsplit(['/', '\\']).next().unwrap_or(tok)
}

pub fn analyze(cmd: &str) -> Vec<CommandRisk> {
    use CommandRisk::*;
    let mut risks = Vec::new();
    let mut add = |r: CommandRisk| {
        if !risks.contains(&r) {
            risks.push(r);
        }
    };
    let bare = unquoted(cmd);
    if bare.contains("&&") || bare.contains("||") || bare.contains(';') {
        add(Chained);
    }
    let redirect = bare.replace("2>&1", "").replace(">/dev/null", "").replace("> /dev/null", "").replace("2>", "");
    if redirect.contains('>') {
        add(Redirection);
    }
    // Pipe into an interpreter, or process substitution / $(...) of a download.
    for seg in bare.split('|').skip(1) {
        let toks = tokens(seg);
        let first = toks.iter().map(|t| base_name(t)).find(|t| !matches!(*t, "sudo" | "doas" | "-E" | "env"));
        if first.is_some_and(|f| INTERPRETERS.contains(&f.to_ascii_lowercase().as_str())) {
            add(PipeToInterpreter);
        }
    }
    if SUBSHELL_DOWNLOAD.is_match(cmd) || INVOKE_EXPRESSION.is_match(&bare) {
        add(PipeToInterpreter);
    }
    // Per-command analysis across `&&`, `;`, `|` segments.
    for seg in bare.split(['|', ';', '&']) {
        let toks = tokens(seg);
        let mut i = 0;
        while i < toks.len() && ENV_ASSIGN.is_match(&toks[i]) {
            i += 1;
        }
        let Some(first) = toks.get(i) else { continue };
        let mut name = base_name(first).to_string();
        if matches!(name.as_str(), "sudo" | "doas" | "su" | "pkexec" | "runas") || first.eq_ignore_ascii_case("start-process") && seg.to_ascii_lowercase().contains("runas") {
            add(Privileged);
            i += 1;
            while i < toks.len() && toks[i].starts_with('-') {
                i += 1;
            }
            match toks.get(i) {
                Some(t) => name = base_name(t).to_string(),
                None => continue,
            }
        }
        let args: Vec<&str> = toks.iter().skip(i + 1).map(String::as_str).collect();
        let lname = name.to_ascii_lowercase();
        match lname.as_str() {
            "rm" | "rmdir" | "del" | "erase" | "rd" | "shred" | "unlink" | "remove-item" | "ri" => add(Deletion),
            "mv" | "cp" | "chmod" | "chown" | "chgrp" | "chattr" | "ln" | "truncate" | "tee" | "install" | "mount" | "umount" | "fdisk" | "parted" | "set-content" => {
                add(FilesystemModification)
            }
            "dd" | "mkfs" | "wipefs" | "format" => {
                add(FilesystemModification);
                add(Deletion);
            }
            "curl" | "wget" | "iwr" | "irm" | "invoke-webrequest" | "invoke-restmethod" | "aria2c" | "scp" | "rsync" | "sftp" => add(NetworkDownload),
            "sed" if args.iter().any(|a| a.starts_with("-i")) => add(FilesystemModification),
            "find" if args.iter().any(|a| *a == "-delete" || *a == "-exec") => add(Deletion),
            "git" => match args.first().copied() {
                Some("clone" | "pull" | "fetch") => add(NetworkDownload),
                Some("clean") => add(Deletion),
                Some("reset") if args.contains(&"--hard") => add(Deletion),
                Some("push") if args.iter().any(|a| a.starts_with("--force") || *a == "-f") => add(Deletion),
                _ => {}
            },
            "bash" | "sh" | "zsh" | "fish" | "source" | "." | "pwsh" | "powershell" if args.iter().any(|a| !a.starts_with('-')) => add(ShellScript),
            "docker" | "podman" if args.first().is_some_and(|a| matches!(*a, "pull" | "run")) => add(NetworkDownload),
            _ => {}
        }
        if PACKAGE_MANAGERS.contains(&lname.as_str()) && args.iter().any(|a| PKG_VERBS.contains(a) || a.starts_with("-S") || a.starts_with("-R")) {
            add(PackageManagement);
            add(NetworkDownload);
        }
        if name.starts_with("./") && (name.ends_with(".sh") || name.ends_with(".ps1")) || first.ends_with(".sh") || first.ends_with(".ps1") {
            add(ShellScript);
        }
    }
    risks
}

pub fn detect(input: &TextInput, _cx: &RecognizeContext) -> Vec<Detection> {
    let mut out = Vec::new();
    let low_confidence_ocr = input.layout.is_some_and(|l| l.lines.iter().flat_map(|l| &l.words).any(|w| w.confidence < 0.75));
    for (line, prompted) in command_lines(input.text) {
        let line = line.trim();
        if line.is_empty() || line.len() > 2000 {
            continue;
        }
        let toks = tokens(line);
        let first_cmd = toks.iter().find(|t| !ENV_ASSIGN.is_match(t)).map(|t| t.as_str()).unwrap_or_default();
        let name = base_name(first_cmd);
        let known = COMMANDS.contains(&name) || name.starts_with("./") || first_cmd.starts_with("./");
        if !known && !prompted {
            continue;
        }
        let words: Vec<String> = toks.iter().map(|t| t.to_ascii_lowercase()).collect();
        let stop = words.iter().filter(|w| STOPWORDS.contains(&w.trim_end_matches(['.', ',']))).count();
        let commandish_args = toks.iter().skip(1).any(|t| t.starts_with('-') || t.contains('/') || t.contains('=') || t.contains('.') || t == "|");
        if !prompted && (stop >= 3 || (WORDY.contains(&name) && !commandish_args) || line.ends_with('.') && stop >= 1) {
            continue; // Prose that happens to start with "make", "find", ...
        }
        let mut risks = analyze(line);
        if low_confidence_ocr {
            risks.push(CommandRisk::LowOcrConfidence);
        }
        let mut d = Detection::new(caps::COMMAND, Value::Command(CommandValue { command: line.to_string(), risks: risks.clone() }))
            .confidence(if prompted { 0.95 } else { 0.8 })
            .detail("Program", name.to_string());
        if !risks.is_empty() {
            d = d.detail("Caution", risks.iter().map(|r| r.describe()).collect::<Vec<_>>().join("; "));
        }
        if let Some(start) = input.text.find(line) {
            d = d.span(start..start + line.len());
        }
        out.push(d);
        if out.len() >= 20 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::cx;
    use CommandRisk::*;

    fn cmds(text: &str) -> Vec<(String, Vec<CommandRisk>)> {
        detect(&TextInput { text, layout: None }, &cx())
            .into_iter()
            .map(|d| match d.value {
                Value::Command(c) => (c.command, c.risks),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn prompts_and_continuations() {
        assert_eq!(cmds("$ git clone https://github.com/qa-p1/Arcade-lens"), vec![("git clone https://github.com/qa-p1/Arcade-lens".into(), vec![NetworkDownload])]);
        assert_eq!(cmds("user@box:~/src$ ls -la"), vec![("ls -la".into(), vec![])]);
        assert_eq!(cmds("docker run \\\n  -p 8080:80 \\\n  nginx")[0].0, "docker run -p 8080:80 nginx");
        assert_eq!(cmds(r"PS C:\Users\ada> Get-ChildItem -Recurse")[0].0, "Get-ChildItem -Recurse");
    }

    #[test]
    fn risks() {
        assert_eq!(cmds("sudo pacman -S package")[0].1, vec![Privileged, PackageManagement, NetworkDownload]);
        assert_eq!(cmds("curl -fsSL https://get.example.sh | sh")[0].1, vec![PipeToInterpreter, NetworkDownload]);
        assert_eq!(cmds("rm -rf ./build && make")[0].1, vec![Chained, Deletion]);
        assert_eq!(cmds("echo hi > notes.txt")[0].1, vec![Redirection]);
        assert_eq!(cmds("cargo build 2>&1")[0].1, vec![]);
        assert_eq!(cmds("grep 'a|b; c > d' file.txt")[0].1, vec![]);
        assert!(cmds("bash -c \"$(curl -fsSL https://x.sh/install)\"")[0].1.contains(&PipeToInterpreter));
        assert!(cmds("iwr https://x.ps1 | iex")[0].1.contains(&PipeToInterpreter));
    }

    #[test]
    fn prose_is_not_a_command() {
        for t in ["Make sure you restart the server.", "find the best option for your team", "Kill the process if it hangs.", "This is a sentence."] {
            assert!(cmds(t).is_empty(), "{t}");
        }
        assert!(cmds("> Note: this is how it works").is_empty());
        assert!(cmds("# Install dependencies").is_empty());
        assert_eq!(cmds("# apt install curl")[0].0, "apt install curl");
        assert_eq!(cmds("make -j8 release").len(), 1);
        assert_eq!(cmds("find . -name '*.rs'").len(), 1);
    }
}
