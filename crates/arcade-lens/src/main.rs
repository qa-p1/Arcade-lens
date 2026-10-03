//! `arcade-lens` command-line interface.
//!
//! The overlay (global shortcut → freeze → draw → palette) is the primary
//! experience; this CLI drives the exact same engine on image files so every
//! recognizer and action can be exercised, scripted and tested headlessly.

mod config;
mod gui;
mod host;
mod net;

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use image::{Rgba, RgbaImage};
use lens_core::action::Params;
use lens_core::cancel::CancelToken;
use lens_core::chain::ChainRunContext;
use lens_core::geometry::Rect;
use lens_core::host::{Host, HostCall, RecordingHost};
use lens_core::palette::{build_palette, invoke, Invocation, InvokeContext, Palette, PaletteEntry, PaletteInput, Target};
use lens_core::recognizer::LocalEnvironment;
use lens_core::value::{TextLayout, TextLine};
use lens_core::{AnalysisEvent, AnalysisReport, Engine, Finding, FindingId, Selection, Settings, Value};
use lens_recognizers::ocr::OcrEngine;

use crate::config::Paths;

#[derive(Parser)]
#[command(name = "arcade-lens", version, about = "Select anything on screen. Do something useful with it.")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(clap::Args, Clone)]
struct Input {
    /// Image to analyze (PNG, JPEG, WebP). Omit with --text to analyze text alone.
    image: Option<PathBuf>,
    /// Use this text as the OCR result instead of running OCR.
    #[arg(long)]
    text: Option<String>,
    /// Skip OCR entirely.
    #[arg(long)]
    no_ocr: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show everything Lens recognizes in an image and the actions it would offer.
    Analyze {
        #[command(flatten)]
        input: Input,
        /// List every applicable action, not only the primary ones.
        #[arg(long)]
        all: bool,
        /// Print findings and actions as JSON.
        #[arg(long)]
        json: bool,
        /// Show when each result arrived (progressive recognition).
        #[arg(long)]
        timeline: bool,
    },
    /// Run an action (or `chain:<id>`) on an image.
    Run {
        /// Action id such as `core.url.open`, or `chain:clean-copy`.
        action: String,
        #[command(flatten)]
        input: Input,
        /// Finding id to act on (default: the best match).
        #[arg(long)]
        finding: Option<u32>,
        /// Answer yes to confirmation prompts.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Pre-select a choice (index into the listed options).
        #[arg(long)]
        choice: Option<usize>,
        /// Extra parameters as key=value (values parsed as JSON when possible).
        #[arg(long = "param", short = 'p')]
        params: Vec<String>,
        /// Don't touch the system; print what would happen.
        #[arg(long)]
        dry_run: bool,
    },
    /// List registered actions.
    Actions {
        /// Only actions accepting this capability.
        #[arg(long)]
        capability: Option<String>,
    },
    /// List registered recognizers.
    Recognizers,
    /// Manage action chains.
    Chains {
        #[command(subcommand)]
        command: ChainCmd,
    },
    /// Manage local OCR models.
    Models {
        #[command(subcommand)]
        command: ModelCmd,
    },
    /// Show configuration locations or write default settings.
    Config {
        #[command(subcommand)]
        command: ConfigCmd,
    },
    /// Forget learned action preferences.
    ResetUsage,
    /// Run Arcade Lens in the background: global shortcut, pins, settings.
    Start,
    /// Open the selection overlay now (in the running instance, or standalone).
    Capture,
    /// Open the settings window.
    Settings,
    /// Pin an image file on screen.
    Pin { image: PathBuf },
    /// Stop the background instance.
    Quit,
    /// Start Arcade Lens when you log in.
    Autostart {
        #[arg(value_parser = ["enable", "disable", "status"])]
        action: String,
    },
    /// Add Arcade Lens to the desktop's applications menu (Linux).
    InstallLauncher,
    /// Manage plugins.
    Plugins {
        #[command(subcommand)]
        command: PluginCmd,
    },
}

#[derive(Subcommand)]
enum PluginCmd {
    /// Installed plugins and their status.
    List,
    /// Copy a plugin directory into the plugins folder (it stays disabled).
    Install { dir: PathBuf },
    /// Allow a plugin to run.
    Enable { id: String },
    /// Stop using a plugin.
    Disable { id: String },
}

#[derive(Subcommand)]
enum ChainCmd {
    List,
    /// Write the default chains to the chains file (overwrites it).
    Reset,
}

#[derive(Subcommand)]
enum ModelCmd {
    Status,
    Download,
}

#[derive(Subcommand)]
enum ConfigCmd {
    Path,
    Show,
    /// Write default settings if no settings file exists.
    Init,
}

fn main() -> ExitCode {
    // Behave like other CLI tools when piped into `head`: exit quietly on EPIPE.
    #[cfg(unix)]
    // SAFETY: restoring the default disposition of SIGPIPE before any threads start.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type AnyResult<T = ()> = Result<T, String>;

struct FixedText(String);

impl OcrEngine for FixedText {
    fn name(&self) -> &str {
        "--text"
    }
    fn recognize(&self, _: &RgbaImage, _: &[String], _: &CancelToken) -> lens_core::Result<TextLayout> {
        let lines = self
            .0
            .lines()
            .enumerate()
            .map(|(i, l)| TextLine { text: l.into(), bbox: Rect::new(0, i as i32 * 20, 10 * l.len() as u32, 18), words: vec![] })
            .collect();
        Ok(TextLayout { lines })
    }
}

fn ocr_engine(input: &Input, paths: &Paths) -> (Option<Arc<dyn OcrEngine>>, String) {
    if let Some(t) = &input.text {
        return (Some(Arc::new(FixedText(t.replace("\\n", "\n")))), "simulated (--text)".into());
    }
    if input.no_ocr {
        return (None, "off".into());
    }
    #[cfg(feature = "ocrs")]
    {
        use lens_recognizers::ocr::ocrs_engine::OcrsEngine;
        let dir = paths.models();
        if OcrsEngine::models_present(&dir) {
            return match OcrsEngine::load(&dir) {
                Ok(e) => (Some(Arc::new(e)), "ocrs".into()),
                Err(e) => (None, format!("unavailable ({e})")),
            };
        }
        (None, "not installed — run `arcade-lens models download`".into())
    }
    #[cfg(not(feature = "ocrs"))]
    {
        let _ = paths;
        (None, "not built in".into())
    }
}

fn load_image(input: &Input) -> AnyResult<RgbaImage> {
    match &input.image {
        Some(p) => image::open(p).map(|i| i.to_rgba8()).map_err(|e| format!("{}: {e}", p.display())),
        // Text-only analysis: a text-like texture so the OCR stage is scheduled.
        None if input.text.is_some() => {
            Ok(RgbaImage::from_fn(320, 64, |x, y| if (x / 3 + y / 5) % 3 == 0 { Rgba([24, 24, 27, 255]) } else { Rgba([250, 250, 250, 255]) }))
        }
        None => Err("provide an image path or --text".into()),
    }
}

struct Session {
    paths: Paths,
    settings: Settings,
    engine: Engine,
    ocr_status: String,
}

fn session(input: Option<&Input>) -> AnyResult<Session> {
    let paths = Paths::discover();
    let mut settings = config::load_settings(&paths)?;
    if input.is_some_and(|i| i.image.is_none()) {
        // Text-only input uses a placeholder image; pixel recognizers would describe the placeholder.
        settings.disabled_recognizers.extend(["core.color", "core.palette", "core.inspect", "core.image-kind", "core.codes"].map(String::from));
    }
    let (ocr, ocr_status) = input.map(|i| ocr_engine(i, &paths)).unwrap_or((None, "off".into()));
    let mut registry = lens_actions::standard_registry(ocr).map_err(|e| e.to_string())?;
    for p in lens_plugins::load_enabled(&mut registry, &paths.plugins(), &settings.enabled_plugins) {
        if let Some(e) = p.error {
            eprintln!("plugin {}: {e}", p.id);
        }
    }
    let engine = Engine::new(Arc::new(registry), Arc::new(LocalEnvironment), Arc::new(settings.clone()));
    Ok(Session { paths, settings, engine, ocr_status })
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Session {
    fn analyze(&self, img: RgbaImage, timeline: bool) -> (Selection, AnalysisReport) {
        let sel = Selection::from_image(img);
        if !timeline {
            let report = self.engine.analyze_blocking(sel.clone(), CancelToken::new());
            return (sel, report);
        }
        let start = Instant::now();
        let analysis = self.engine.analyze(sel.clone());
        let mut report = AnalysisReport::default();
        while let Ok(e) = analysis.events().recv() {
            let t = start.elapsed().as_secs_f64() * 1000.0;
            match &e {
                AnalysisEvent::Findings(fs) => {
                    for f in fs {
                        println!("{t:>8.1} ms  + {:<12} {}", f.capability, f.summary());
                    }
                }
                AnalysisEvent::RecognizerFailed { recognizer, error } => println!("{t:>8.1} ms  ! {recognizer}: {error}"),
                AnalysisEvent::Finished { .. } => println!("{t:>8.1} ms  done"),
            }
            let done = matches!(e, AnalysisEvent::Finished { .. });
            report.apply(e);
            if done {
                break;
            }
        }
        println!();
        (sel, report)
    }

    fn palette(&self, report: &AnalysisReport, host: &dyn Host) -> Palette {
        let usage = config::load_usage(&self.paths);
        let chains = config::load_chains(&self.paths);
        build_palette(&PaletteInput {
            registry: self.engine.registry(),
            findings: &report.findings,
            settings: &self.settings,
            usage: &usage,
            host: host.features(),
            chains: &chains,
            now: now(),
        })
    }
}

fn finding_line(f: &Finding) -> String {
    let mut s = format!("  #{:<3} {:<12} {}", f.id.0, f.capability.as_str(), f.summary());
    let details: Vec<String> = f.details.iter().filter(|(k, _)| k != "Engine").map(|(k, v)| format!("{k}: {v}")).collect();
    if !details.is_empty() {
        s.push_str(&format!("\n        {}", details.join(" · ")));
    }
    s
}

fn entry_line(e: &PaletteEntry, default: bool) -> String {
    let key = if default { "⏎".to_string() } else { e.key.map(|k| k.to_string()).unwrap_or_else(|| " ".into()) };
    let badge = match e.safety {
        lens_core::SafetyClass::External => " ↗",
        lens_core::SafetyClass::Dangerous => " ⚠",
        _ => "",
    };
    let mut s = format!("  {key}  {:<28} {} #{}", format!("{}{badge}", e.label), e.capability, e.finding.0);
    if let Some(p) = &e.preview {
        s.push_str(&format!("\n        sends: {p}"));
    }
    if e.needs_confirmation {
        s.push_str("  (asks first)");
    }
    s
}

fn analyze(input: Input, all: bool, json: bool, timeline: bool) -> AnyResult {
    let s = session(Some(&input))?;
    let img = load_image(&input)?;
    let host = host::DesktopHost::new(s.settings.clone(), s.paths.collections());
    let (sel, report) = s.analyze(img, timeline && !json);
    let palette = s.palette(&report, &host);
    if json {
        let findings: Vec<_> = report
            .findings
            .iter()
            .map(|f| serde_json::json!({ "id": f.id.0, "capability": f.capability.as_str(), "summary": f.summary(), "confidence": f.confidence, "recognizer": f.recognizer, "details": f.details }))
            .collect();
        let entries = |v: &[PaletteEntry]| -> Vec<serde_json::Value> {
            v.iter()
                .map(|e| serde_json::json!({ "action": e.target.usage_key(), "label": e.label, "finding": e.finding.0, "key": e.key, "safety": format!("{:?}", e.safety), "preview": e.preview, "confirm": e.needs_confirmation }))
                .collect()
        };
        let out = serde_json::json!({
            "size": [sel.rect.width, sel.rect.height],
            "elapsed_ms": report.elapsed.as_secs_f64() * 1000.0,
            "ocr": s.ocr_status,
            "findings": findings,
            "failures": report.failures,
            "primary": entries(&palette.primary),
            "all": entries(&palette.all),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return Ok(());
    }
    println!("Selection {} × {} px · analyzed in {:.0} ms · OCR: {}\n", sel.rect.width, sel.rect.height, report.elapsed.as_secs_f64() * 1000.0, s.ocr_status);
    println!("FOUND");
    for f in &report.findings {
        println!("{}", finding_line(f));
    }
    for (r, e) in &report.failures {
        println!("  ! {r} failed: {e}");
    }
    println!("\nACTIONS");
    for (i, e) in palette.primary.iter().enumerate() {
        println!("{}", entry_line(e, i == 0));
    }
    let rest: Vec<&PaletteEntry> = palette.all.iter().filter(|e| !palette.primary.iter().any(|p| p.target == e.target && p.finding == e.finding)).collect();
    if all {
        println!("\nMORE");
        for e in rest {
            println!("{}", entry_line(e, false));
        }
    } else if !rest.is_empty() {
        println!("  •••  {} more (--all)", rest.len());
    }
    Ok(())
}

fn parse_params(raw: &[String]) -> AnyResult<Params> {
    let mut params = Params::new();
    for p in raw {
        let (k, v) = p.split_once('=').ok_or_else(|| format!("parameter `{p}` must be key=value"))?;
        params.insert(k.into(), serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.into())));
    }
    Ok(params)
}

fn ask(prompt: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{prompt} [y/N] ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok();
    matches!(line.trim(), "y" | "Y" | "yes")
}

fn pick(n: usize) -> Option<usize> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    print!("Choose 1-{n}: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok();
    line.trim().parse::<usize>().ok().filter(|i| (1..=n).contains(i)).map(|i| i - 1)
}

fn print_calls(calls: &[HostCall]) {
    for c in calls {
        println!("  would {c:?}");
    }
}

#[allow(clippy::too_many_arguments)]
fn run_action(input: Input, action: String, finding: Option<u32>, yes: bool, choice: Option<usize>, raw_params: Vec<String>, dry_run: bool) -> AnyResult {
    let s = session(Some(&input))?;
    let img = load_image(&input)?;
    let desktop = host::DesktopHost::new(s.settings.clone(), s.paths.collections());
    let recording = RecordingHost::new(desktop.features());
    let host: &dyn Host = if dry_run { &recording } else { &desktop };
    let (sel, report) = s.analyze(img, false);
    let palette = s.palette(&report, host);

    let target = match action.strip_prefix("chain:") {
        Some(id) => Target::Chain(id.into()),
        None => Target::Action(action.clone()),
    };
    let entry = palette
        .all
        .iter()
        .find(|e| e.target == target && finding.is_none_or(|f| e.finding == FindingId(f)))
        .ok_or_else(|| format!("`{action}` is not applicable to this selection (see `arcade-lens analyze --all`)"))?;
    let usage_cap = entry.capability.clone();

    let message = match &target {
        Target::Chain(id) => {
            let chains = config::load_chains(&s.paths);
            let chain = chains.iter().find(|c| &c.id == id).ok_or_else(|| format!("no chain {id}"))?;
            let plan = chain.validate(s.engine.registry()).map_err(|e| e.to_string())?;
            let confirmed = !plan.needs_confirmation || yes || ask(&format!("Chain \"{}\" may {:?}. Run it?", chain.name, plan.effects));
            let outcomes = chain
                .run(ChainRunContext {
                    registry: s.engine.registry(),
                    findings: &report.findings,
                    host,
                    settings: &s.settings,
                    selection: Some(&sel),
                    cancel: &CancelToken::new(),
                    confirmed,
                })
                .map_err(|e| e.to_string())?;
            outcomes.iter().filter_map(|o| o.message.clone()).collect::<Vec<_>>().join("\n")
        }
        Target::Action(id) => {
            let mut params = parse_params(&raw_params)?;
            let mut confirmed = false;
            loop {
                let cx = InvokeContext {
                    registry: s.engine.registry(),
                    findings: &report.findings,
                    host,
                    settings: &s.settings,
                    selection: Some(&sel),
                    params: &params,
                    confirmed,
                };
                match invoke(id, entry.finding, &cx).map_err(|e| e.to_string())? {
                    Invocation::Done(out) => {
                        let mut msg = out.message.unwrap_or_default();
                        if let Some(o) = out.output {
                            if let Value::File(f) = &o.value {
                                msg = format!("{msg}\n{}", f.path.display());
                            }
                        }
                        break msg;
                    }
                    Invocation::NeedsConfirmation(req) => {
                        println!("{}\n\n    {}\n", req.title, req.subject);
                        for r in &req.reasons {
                            println!("  • {r}");
                        }
                        if !(yes || ask("Proceed?")) {
                            return Err("not confirmed (pass --yes to confirm non-interactively)".into());
                        }
                        confirmed = true;
                    }
                    Invocation::NeedsChoice(choices) => {
                        for (i, c) in choices.iter().enumerate() {
                            println!("  {}. {}", i + 1, c.label);
                        }
                        let i = choice.or_else(|| pick(choices.len())).ok_or("this action needs a choice (pass --choice N, 0-based)")?;
                        let c = choices.get(i).ok_or("choice out of range")?;
                        params.insert("choice".into(), c.value.clone());
                    }
                }
            }
        }
    };
    if !message.is_empty() {
        println!("{message}");
    }
    if dry_run {
        print_calls(&recording.calls());
    } else if s.settings.privacy.learn_action_usage {
        let mut usage = config::load_usage(&s.paths);
        usage.record(usage_cap.as_str(), &target.usage_key(), now());
        config::save_usage(&s.paths, &usage).ok();
    }
    Ok(())
}

/// Sends a command to the running instance, or runs a standalone one.
fn remote_or_local(command: &str, trigger: gui::Trigger) -> AnyResult {
    let paths = Paths::discover();
    if lens_platform::ipc::send(&paths.endpoint(), command).is_ok() {
        return Ok(());
    }
    gui::run(gui::Launch { one_shot: true, initial: Some(trigger), daemon: false })
}

fn run(cli: Cli) -> AnyResult {
    match cli.command {
        Cmd::Analyze { input, all, json, timeline } => analyze(input, all, json, timeline),
        Cmd::Run { input, action, finding, yes, choice, params, dry_run } => run_action(input, action, finding, yes, choice, params, dry_run),
        Cmd::Actions { capability } => {
            let s = session(None)?;
            for a in s.engine.registry().actions() {
                let d = a.descriptor();
                if capability.as_deref().is_some_and(|c| !d.accepts.iter().any(|a| a.as_str() == c)) {
                    continue;
                }
                let accepts: Vec<&str> = d.accepts.iter().map(|c| c.as_str()).collect();
                let tag = if d.in_palette { "" } else { "  (chain step)" };
                println!("{:<32} {:<28} {:<10?} {}{tag}", d.id, d.label, d.safety(), accepts.join(","));
            }
            Ok(())
        }
        Cmd::Recognizers => {
            let s = session(None)?;
            for r in s.engine.registry().recognizers() {
                let d = r.descriptor();
                let consumes: Vec<&str> = d.consumes.iter().map(|c| c.as_str()).collect();
                let produces: Vec<&str> = d.produces.iter().map(|c| c.as_str()).collect();
                println!("{:<24} {:<10?} {} → {}", d.id, d.cost, consumes.join(","), produces.join(","));
            }
            Ok(())
        }
        Cmd::Chains { command } => {
            let s = session(None)?;
            match command {
                ChainCmd::List => {
                    for c in config::load_chains(&s.paths) {
                        let status = match c.validate(s.engine.registry()) {
                            Ok(p) => format!("from {} · {}", p.input, if p.needs_confirmation { "asks first" } else { "local" }),
                            Err(e) => format!("INVALID: {e}"),
                        };
                        println!("{:<16} {:<16} {status}", c.id, c.name);
                    }
                }
                ChainCmd::Reset => {
                    config::save_chains(&s.paths, &lens_actions::default_chains()).map_err(|e| e.to_string())?;
                    println!("Wrote {}", s.paths.chains().display());
                }
            }
            Ok(())
        }
        Cmd::Models { command } => models(command),
        Cmd::Config { command } => {
            let paths = Paths::discover();
            match command {
                ConfigCmd::Path => {
                    println!(
                        "settings: {}\nchains:   {}\nusage:    {}\nmodels:   {}",
                        paths.settings().display(),
                        paths.chains().display(),
                        paths.usage().display(),
                        paths.models().display()
                    )
                }
                ConfigCmd::Show => print!("{}", toml::to_string_pretty(&config::load_settings(&paths)?).unwrap()),
                ConfigCmd::Init => {
                    if paths.settings().exists() {
                        println!("{} already exists", paths.settings().display());
                    } else {
                        config::save_settings(&paths, &Settings::default()).map_err(|e| e.to_string())?;
                        println!("Wrote {}", paths.settings().display());
                    }
                }
            }
            Ok(())
        }
        Cmd::Start => {
            let paths = Paths::discover();
            if lens_platform::ipc::send(&paths.endpoint(), "ping").is_ok() {
                println!("Arcade Lens is already running.");
                return Ok(());
            }
            gui::run(gui::Launch { one_shot: false, initial: None, daemon: true })
        }
        Cmd::Capture => remote_or_local("capture", gui::Trigger::Capture),
        Cmd::Settings => remote_or_local("settings", gui::Trigger::Settings),
        Cmd::Pin { image } => {
            let abs = std::fs::canonicalize(&image).map_err(|e| format!("{}: {e}", image.display()))?;
            remote_or_local(&format!("pin {}", abs.display()), gui::Trigger::Pin(abs))
        }
        Cmd::Quit => {
            let paths = Paths::discover();
            lens_platform::ipc::send(&paths.endpoint(), "quit").map(|_| ()).map_err(|_| "Arcade Lens is not running".to_string())
        }
        Cmd::Autostart { action } => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            match action.as_str() {
                "enable" => lens_platform::autostart::enable(&exe).map_err(|e| e.to_string())?,
                "disable" => lens_platform::autostart::disable().map_err(|e| e.to_string())?,
                _ => {}
            }
            println!("Start at login: {}", if lens_platform::autostart::is_enabled() { "on" } else { "off" });
            Ok(())
        }
        Cmd::InstallLauncher => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            for p in lens_platform::autostart::install_launcher(&exe).map_err(|e| e.to_string())? {
                println!("Wrote {}", p.display());
            }
            Ok(())
        }
        Cmd::Plugins { command } => plugins(command),
        Cmd::ResetUsage => {
            let paths = Paths::discover();
            config::save_usage(&paths, &Default::default()).map_err(|e| e.to_string())?;
            println!("Learned action preferences cleared.");
            Ok(())
        }
    }
}

fn plugins(command: PluginCmd) -> AnyResult {
    let paths = Paths::discover();
    let dir = paths.plugins();
    let mut settings = config::load_settings(&paths)?;
    match command {
        PluginCmd::List => {
            let found = lens_plugins::discover(&dir);
            if found.is_empty() {
                println!("No plugins in {}", dir.display());
            }
            for (path, m) in found {
                match m {
                    Ok(m) => {
                        let on = settings.enabled_plugins.contains(&m.plugin.id);
                        println!("{} {:<28} {:<24} permissions: {}", if on { "●" } else { "○" }, m.plugin.id, m.plugin.name, m.plugin.permissions.join(", "));
                    }
                    Err(e) => println!("! {}: {e}", path.display()),
                }
            }
        }
        PluginCmd::Install { dir: src } => {
            let m = lens_plugins::Manifest::load(&src)?;
            let dest = dir.join(&m.plugin.id);
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            for e in std::fs::read_dir(&src).map_err(|e| e.to_string())?.flatten() {
                if e.path().is_file() {
                    std::fs::copy(e.path(), dest.join(e.file_name())).map_err(|e| e.to_string())?;
                }
            }
            println!(
                "Installed {} to {}.\nIt can: {}.\nEnable it with `arcade-lens plugins enable {}`.",
                m.plugin.name,
                dest.display(),
                m.plugin.permissions.join(", "),
                m.plugin.id
            );
        }
        PluginCmd::Enable { id } => {
            if !lens_plugins::discover(&dir).iter().any(|(_, m)| m.as_ref().is_ok_and(|m| m.plugin.id == id)) {
                return Err(format!("no installed plugin {id}"));
            }
            if !settings.enabled_plugins.contains(&id) {
                settings.enabled_plugins.push(id.clone());
            }
            config::save_settings(&paths, &settings).map_err(|e| e.to_string())?;
            println!("Enabled {id}. Restart Arcade Lens (arcade-lens quit && arcade-lens start) to load it.");
        }
        PluginCmd::Disable { id } => {
            settings.enabled_plugins.retain(|p| p != &id);
            config::save_settings(&paths, &settings).map_err(|e| e.to_string())?;
            println!("Disabled {id}.");
        }
    }
    Ok(())
}

fn models(command: ModelCmd) -> AnyResult {
    let paths = Paths::discover();
    let dir = paths.models();
    match command {
        ModelCmd::Status => {
            let (_, status) = gui::runtime::ocr_engine(&paths);
            println!("OCR engine: {status}\nModels: {}", dir.display());
        }
        ModelCmd::Download => {
            println!("Downloading OCR models (~12 MB)…");
            net::download_models(&dir)?;
            println!("OCR ready.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn params_parse_json_or_strings() {
        let p = parse_params(&["percent=25".into(), "format=webp".into()]).unwrap();
        assert_eq!(p["percent"], serde_json::json!(25));
        assert_eq!(p["format"], serde_json::json!("webp"));
        assert!(parse_params(&["oops".into()]).is_err());
    }
}
