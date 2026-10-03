//! Commands, code, errors and file paths.

use std::path::{Path, PathBuf};

use lens_core::action::{ActionGroup as G, ConfirmRequest, Item};
use lens_core::builder::action;
use lens_core::host::{HostFeatures as F, OpenPathMode};
use lens_core::registry::PluginRegistrar;
use lens_core::value::{CommandValue, PathKind, PathValue};
use lens_core::{caps, ActionOutcome, Effects, Finding, LensError, Result, Value};

use crate::util::*;

fn command(i: &Item) -> Result<&CommandValue> {
    match &i.value {
        Value::Command(c) => Ok(c),
        _ => Err(LensError::InvalidInput("not a command".into())),
    }
}

/// Local, deterministic explanation of a shell command's structure.
pub fn explain(cmd: &str) -> String {
    const PROGRAMS: &[(&str, &str)] = &[
        ("sudo", "run the rest of the command as the superuser"),
        ("git", "Git version control"),
        ("pacman", "Arch Linux package manager"),
        ("apt", "Debian/Ubuntu package manager"),
        ("apt-get", "Debian/Ubuntu package manager"),
        ("dnf", "Fedora package manager"),
        ("brew", "Homebrew package manager"),
        ("npm", "Node.js package manager"),
        ("cargo", "Rust build tool and package manager"),
        ("pip", "Python package installer"),
        ("docker", "container engine"),
        ("kubectl", "Kubernetes CLI"),
        ("curl", "transfer data from or to a URL"),
        ("wget", "download files from the web"),
        ("rm", "remove files or directories"),
        ("cp", "copy files"),
        ("mv", "move or rename files"),
        ("chmod", "change file permissions"),
        ("chown", "change file owner"),
        ("ls", "list directory contents"),
        ("cd", "change directory"),
        ("grep", "search text with patterns"),
        ("find", "search for files"),
        ("tar", "create or extract archives"),
        ("ssh", "open a remote shell"),
        ("sh", "POSIX shell"),
        ("bash", "Bash shell"),
        ("echo", "print text"),
        ("cat", "print file contents"),
        ("make", "run build recipes from a Makefile"),
        ("systemctl", "control systemd services"),
    ];
    const SUBCOMMANDS: &[(&str, &str, &str)] = &[
        ("git", "clone", "download a repository"),
        ("git", "pull", "fetch and merge remote changes"),
        ("git", "push", "upload local commits"),
        ("git", "reset", "move the current branch (with --hard, discard local changes)"),
        ("git", "checkout", "switch branches or restore files"),
        ("npm", "install", "install dependencies"),
        ("cargo", "build", "compile the project"),
        ("cargo", "install", "install a Rust binary"),
        ("apt", "install", "install packages"),
        ("docker", "run", "start a container"),
    ];
    const FLAGS: &[(&str, &str, &str)] = &[
        ("pacman", "-S", "install (sync) packages"),
        ("pacman", "-Syu", "refresh the package database and upgrade everything"),
        ("pacman", "-R", "remove packages"),
        ("rm", "-r", "recursive: remove directories and their contents"),
        ("rm", "-rf", "recursive and force: delete without prompting"),
        ("rm", "-f", "force: ignore missing files, never prompt"),
        ("curl", "-f", "fail on HTTP errors"),
        ("curl", "-s", "silent"),
        ("curl", "-S", "show errors even when silent"),
        ("curl", "-L", "follow redirects"),
        ("curl", "-fsSL", "fail on errors, silent, show errors, follow redirects"),
        ("curl", "-o", "write output to a file"),
        ("ls", "-l", "long listing"),
        ("ls", "-a", "include hidden files"),
        ("ls", "-la", "long listing including hidden files"),
        ("chmod", "+x", "make executable"),
        ("tar", "-xzf", "extract a gzip-compressed archive"),
        ("tar", "-czf", "create a gzip-compressed archive"),
        ("grep", "-r", "search recursively"),
        ("grep", "-i", "ignore case"),
        ("docker", "-it", "interactive terminal"),
        ("docker", "-p", "publish a port"),
        ("docker", "--rm", "remove the container when it exits"),
    ];
    let mut out = Vec::new();
    for (n, segment) in cmd.split("&&").enumerate() {
        if n > 0 {
            out.push("&&  run the next command only if the previous one succeeded".to_string());
        }
        for (m, part) in segment.split('|').enumerate() {
            if m > 0 {
                out.push("|   send the output of the previous command into the next one".to_string());
            }
            let toks: Vec<&str> = part.split_whitespace().collect();
            let mut program = "";
            for (k, t) in toks.iter().enumerate() {
                let line = if k == 0 || program == "sudo" && !t.starts_with('-') && *t != program {
                    program = t;
                    match PROGRAMS.iter().find(|(p, _)| p == t) {
                        Some((_, d)) => format!("{t}  {d}"),
                        None => format!("{t}  program"),
                    }
                } else if let Some((_, _, d)) = SUBCOMMANDS.iter().find(|(p, s, _)| *p == program && s == t) {
                    format!("{t}  {d}")
                } else if let Some((_, _, d)) = FLAGS.iter().find(|(p, f, _)| *p == program && f == t) {
                    format!("{t}  {d}")
                } else if t.starts_with('-') {
                    format!("{t}  option")
                } else if t.starts_with('>') {
                    format!("{t}  redirect output to a file (overwrites it)")
                } else {
                    format!("{t}  argument")
                };
                out.push(line);
            }
        }
    }
    out.join("\n")
}

fn path(i: &Item) -> Result<&PathValue> {
    match &i.value {
        Value::Path(p) => Ok(p),
        _ => Err(LensError::InvalidInput("not a path".into())),
    }
}

fn resolved(i: &Item) -> Result<PathBuf> {
    path(i)?.resolved.clone().map(PathBuf::from).ok_or_else(|| LensError::InvalidInput("path cannot be resolved on this computer".into()))
}

fn kind(f: &Finding) -> Option<PathKind> {
    match &f.value {
        Value::Path(p) => p.exists,
        _ => None,
    }
}

fn parent(p: &Path) -> PathBuf {
    p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.to_path_buf())
}

fn language_extension(lang: Option<&str>) -> &'static str {
    match lang {
        Some("Rust") => "rs",
        Some("Python") => "py",
        Some("JavaScript") => "js",
        Some("TypeScript") => "ts",
        Some("Java") => "java",
        Some("C#") => "cs",
        Some("C/C++") => "cpp",
        Some("Go") => "go",
        Some("Shell") => "sh",
        Some("HTML") => "html",
        Some("CSS") => "css",
        Some("SQL") => "sql",
        Some("JSON") => "json",
        Some("YAML") => "yaml",
        Some("Ruby") => "rb",
        Some("PHP") => "php",
        Some("Kotlin") => "kt",
        Some("Swift") => "swift",
        _ => "txt",
    }
}

fn code_text(i: &Item) -> Result<(String, Option<String>)> {
    match &i.value {
        Value::Code(c) => Ok((c.without_line_numbers.clone().unwrap_or_else(|| c.text.clone()), c.language.clone())),
        _ => Err(LensError::InvalidInput("not code".into())),
    }
}

pub fn register(r: &mut PluginRegistrar) {
    // Commands
    let c = caps::COMMAND;
    r.action(
        action("core.command.copy", "Copy Command")
            .icon("copy")
            .group(G::Copy)
            .accepts(c.clone())
            .passthrough()
            .priority(92)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &command(i)?.command, "command")),
    );
    r.action(
        action("core.command.paste", "Open Terminal + Paste")
            .icon("terminal")
            .group(G::Open)
            .accepts(c.clone())
            .priority(80)
            .key('o')
            .effects(LAUNCH)
            .requires(F::TERMINAL)
            .run(|i, cx| {
                cx.host.terminal(None, Some(&command(i)?.command), false)?;
                Ok(ActionOutcome::done("Pasted into a new terminal — review it, then press Enter"))
            }),
    );
    r.action(
        action("core.command.run", "Run in Terminal")
            .icon("play")
            .group(G::System)
            .accepts(c.clone())
            .priority(40)
            .effects(Effects::EXECUTES_COMMAND | LAUNCH)
            .requires(F::TERMINAL)
            // Always confirmed, with the exact text and every detected risk.
            .confirm(|i, _| {
                let cmd = command(i).ok()?;
                let mut reasons = vec!["This text was read from the screen. Check every character before running it.".to_string()];
                reasons.extend(cmd.risks.iter().map(|r| format!("This command {}.", r.describe())));
                Some(ConfirmRequest { title: "Run command?".into(), subject: cmd.command.clone(), reasons })
            })
            .run(|i, cx| {
                cx.host.terminal(None, Some(&command(i)?.command), true)?;
                Ok(ActionOutcome::done("Running in terminal"))
            }),
    );
    r.action(
        action("core.command.explain", "Explain Syntax")
            .icon("help")
            .group(G::Inspect)
            .accepts(c)
            .priority(55)
            .key('x')
            .run(|i, _| Ok(ActionOutcome::done(explain(&command(i)?.command)))),
    );

    // Code
    let k = caps::CODE;
    r.action(
        action("core.code.copy", "Copy Code")
            .icon("code")
            .group(G::Copy)
            .accepts(k.clone())
            .passthrough()
            .priority(88)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "code")),
    );
    r.action(
        action("core.code.copy-clean", "Copy Without Line Numbers")
            .icon("list-x")
            .group(G::Copy)
            .accepts(k.clone())
            .priority(95)
            .key('l')
            .effects(COPY)
            .applies(|f, _| matches!(&f.value, Value::Code(c) if c.without_line_numbers.is_some()))
            .run(|i, cx| copy(cx, &code_text(i)?.0, "code without line numbers")),
    );
    r.action(
        action("core.code.save", "Save as File")
            .icon("file-code")
            .group(G::Save)
            .accepts(k.clone())
            .produces(caps::FILE)
            .priority(50)
            .key('s')
            .effects(SAVE)
            .requires(F::SAVE_FILE)
            .run(|i, cx| {
                let (text, lang) = code_text(i)?;
                save_text(cx, &timestamp_name("snippet", language_extension(lang.as_deref())), &text, "text/plain")
            }),
    );
    r.action(
        action("core.code.editor", "Open in Editor")
            .icon("edit")
            .group(G::Open)
            .accepts(k)
            .priority(60)
            .key('e')
            .effects(SAVE | LAUNCH)
            .requires(F::SAVE_FILE | F::EDITOR)
            .run(|i, cx| {
                let (text, lang) = code_text(i)?;
                let out = save_text(cx, &timestamp_name("snippet", language_extension(lang.as_deref())), &text, "text/plain")?;
                if let Some(Value::File(f)) = out.output.as_ref().map(|o| &o.value) {
                    cx.host.open_path(&f.path, OpenPathMode::Editor)?;
                }
                Ok(out)
            }),
    );

    // Errors
    let e = caps::ERROR;
    let err = |i: &Item| match &i.value {
        Value::Error(e) => Ok(e.clone()),
        _ => Err(LensError::InvalidInput("not an error".into())),
    };
    r.action(
        action("core.error.copy", "Copy Error")
            .icon("copy")
            .group(G::Copy)
            .accepts(e.clone())
            .priority(88)
            .key('c')
            .effects(COPY)
            .run(move |i, cx| copy(cx, &err(i)?.raw, "error")),
    );
    r.action(
        action("core.error.search-clean", "Search Cleaned Error")
            .icon("search")
            .group(G::Search)
            .accepts(e.clone())
            .priority(84)
            .key('w')
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .preview(move |i, _| err(i).ok().map(|e| e.cleaned_query))
            .run(move |i, cx| search(cx, &err(i)?.cleaned_query)),
    );
    r.action(
        action("core.error.search-exact", "Search Exact Error")
            .icon("search")
            .group(G::Search)
            .accepts(e.clone())
            .priority(60)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .preview(move |i, _| err(i).ok().map(|e| format!("\"{}\"", e.headline)))
            .run(move |i, cx| search(cx, &format!("\"{}\"", err(i)?.headline))),
    );
    r.action(
        action("core.error.copy-trace", "Copy Stack Trace")
            .icon("layers")
            .group(G::Copy)
            .accepts(e)
            .priority(65)
            .effects(COPY)
            .applies(|f, _| matches!(&f.value, Value::Error(e) if e.stack_trace.is_some()))
            .run(move |i, cx| copy(cx, &err(i)?.stack_trace.unwrap_or_default(), "stack trace")),
    );

    // Paths
    let p = caps::PATH;
    let open_mode = |id: &str,
                     label: &str,
                     icon: &str,
                     priority: i32,
                     key: Option<char>,
                     needs: F,
                     mode: OpenPathMode,
                     applies: fn(Option<PathKind>) -> bool,
                     target: fn(&Path, Option<PathKind>) -> PathBuf| {
        let mut b = action(id, label)
            .icon(icon)
            .group(G::Open)
            .accepts(caps::PATH)
            .priority(priority)
            .effects(LAUNCH)
            .requires(needs)
            .applies(move |f, _| applies(kind(f)));
        if let Some(k) = key {
            b = b.key(k);
        }
        b.run(move |i, cx| {
            let pv = resolved(i)?;
            cx.host.open_path(&target(&pv, path(i)?.exists), mode)?;
            Ok(ActionOutcome::default())
        })
    };
    let is_file = |k: Option<PathKind>| k == Some(PathKind::File);
    let exists = |k: Option<PathKind>| matches!(k, Some(PathKind::File | PathKind::Directory));
    let same = |p: &Path, _: Option<PathKind>| p.to_path_buf();
    let folder = |p: &Path, k: Option<PathKind>| if k == Some(PathKind::Directory) { p.to_path_buf() } else { parent(p) };
    r.action(open_mode("core.path.open", "Open File", "file", 92, Some('o'), F::OPEN_PATH, OpenPathMode::Default, is_file, same));
    r.action(open_mode("core.path.folder", "Open Folder", "folder", 85, Some('f'), F::OPEN_PATH, OpenPathMode::Default, exists, folder));
    r.action(open_mode("core.path.reveal", "Reveal in Folder", "folder-search", 60, None, F::REVEAL_PATH, OpenPathMode::Reveal, is_file, same));
    r.action(open_mode("core.path.editor", "Open in Editor", "edit", 75, Some('e'), F::EDITOR, OpenPathMode::Editor, is_file, same));
    r.action(open_mode("core.path.terminal", "Open in Terminal", "terminal", 55, None, F::TERMINAL, OpenPathMode::Terminal, exists, folder));
    r.action(open_mode("core.path.quick-look", "Quick Look", "eye", 50, None, F::QUICK_LOOK, OpenPathMode::QuickLook, exists, same));
    r.action(
        action("core.path.copy", "Copy Path")
            .icon("copy")
            .group(G::Copy)
            .accepts(p.clone())
            .passthrough()
            .priority(88)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &path(i)?.raw, "path")),
    );
    r.action(action("core.path.copy-parent", "Copy Parent Path").icon("corner-left-up").group(G::Copy).accepts(p).priority(45).effects(COPY).run(|i, cx| {
        let raw = &path(i)?.raw;
        let trimmed = raw.trim_end_matches(['/', '\\']);
        let cut = trimmed.rfind(['/', '\\']).map_or(trimmed, |n| &trimmed[..n.max(1)]);
        copy(cx, cut, "parent path")
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_structure() {
        let e = explain("sudo pacman -S package");
        assert!(e.contains("sudo  run the rest of the command as the superuser"), "{e}");
        assert!(e.contains("pacman  Arch Linux package manager"), "{e}");
        assert!(e.contains("-S  install (sync) packages"), "{e}");
        let e = explain("curl -fsSL https://x.sh | sh");
        assert!(e.contains("|   send the output"), "{e}");
    }
}
