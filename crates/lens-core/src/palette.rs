//! Turning findings into a small, ranked, keyboard-addressable palette.
//!
//! * Candidates are every (action, finding) pair where the finding's
//!   capability is one the action accepts and the platform supports it.
//! * The secret guard removes outbound actions that would send a detected
//!   secret anywhere.
//! * Ranking blends action priority, finding specificity × confidence,
//!   learned local usage and explicit user preferences.
//! * Only the top few distinct actions are primary; everything else lives in
//!   the overflow list.

use std::collections::{HashMap, HashSet};

use crate::action::{ActionContext, ActionGroup, ActionOutcome, Effects, Item, Params, SafetyClass};
use crate::capability::{caps, Capability};
use crate::chain::Chain;
use crate::error::{LensError, Result};
use crate::finding::{Finding, FindingId};
use crate::host::{Host, HostFeatures};
use crate::registry::Registry;
use crate::selection::Selection;
use crate::settings::Settings;
use crate::usage::UsageStore;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Action(String),
    Chain(String),
}

impl Target {
    /// Key used for usage learning and preferences.
    pub fn usage_key(&self) -> String {
        match self {
            Target::Action(id) => id.clone(),
            Target::Chain(id) => format!("chain:{id}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub target: Target,
    pub finding: FindingId,
    pub capability: Capability,
    pub label: String,
    pub icon: String,
    pub group: ActionGroup,
    pub safety: SafetyClass,
    pub score: f32,
    pub key: Option<char>,
    /// Exactly what leaves the machine or gets executed, if anything.
    pub preview: Option<String>,
    pub needs_confirmation: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Palette {
    /// The handful of actions shown immediately.
    pub primary: Vec<PaletteEntry>,
    /// Index into `primary` of the Enter default (the best-ranked entry; it
    /// is not necessarily first, so late results never reorder the row).
    pub default_index: usize,
    /// Every applicable action, best first (the `•••` list).
    pub all: Vec<PaletteEntry>,
}

impl Palette {
    pub fn default_entry(&self) -> Option<&PaletteEntry> {
        self.primary.get(self.default_index).or(self.primary.first())
    }

    pub fn by_key(&self, key: char) -> Option<&PaletteEntry> {
        let key = key.to_ascii_lowercase();
        self.primary.iter().chain(&self.all).find(|e| e.key == Some(key))
    }
}

pub struct PaletteInput<'a> {
    pub registry: &'a Registry,
    pub findings: &'a [Finding],
    pub settings: &'a Settings,
    pub usage: &'a UsageStore,
    pub host: HostFeatures,
    pub chains: &'a [Chain],
    pub now: u64,
}

/// How specific (and therefore how likely to be what the user meant) a
/// capability is. The bare region is the least specific interpretation.
fn specificity(cap: &Capability) -> f32 {
    if *cap == caps::REGION {
        0.0
    } else if [caps::IMAGE, caps::TEXT, caps::UI_ELEMENT, caps::PALETTE].contains(cap) {
        0.5
    } else if *cap == caps::WINDOW {
        0.6
    } else {
        1.0
    }
}

const PREFERRED_BOOST: f32 = 40.0;
const SPECIFICITY_WEIGHT: f32 = 40.0;
const MAX_PER_GROUP: usize = 2;
const MAX_PER_FINDING: usize = 2;

pub fn build_palette(input: &PaletteInput) -> Palette {
    let PaletteInput { registry, findings, settings, usage, host, chains, now } = *input;
    let secrets: Vec<String> = if settings.privacy.guard_secrets {
        findings
            .iter()
            .filter_map(|f| match &f.value {
                Value::Secret(s) => Some(s.raw.expose().to_string()),
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let leaks = |text: &str| secrets.iter().any(|s| text.contains(s.as_str()));

    let mut entries = Vec::new();
    for f in findings {
        // A URL that *is* the selection is what the user meant; a URL inside a
        // paragraph is incidental. Scale specificity by how much of the parent
        // text the finding covers.
        let coverage = match (&f.span, f.derived_from.and_then(|p| findings.iter().find(|x| x.id == p))) {
            (Some(span), Some(parent)) => parent.value.as_text().map_or(1.0, |t| {
                let total = t.trim().len().max(1) as f32;
                (span.len() as f32 / total).min(1.0)
            }),
            _ => 1.0,
        };
        let spec = specificity(&f.capability) * f.confidence * (0.4 + 0.6 * coverage);
        for a in registry.actions() {
            let d = a.descriptor();
            if !d.in_palette
                || !d.accepts.contains(&f.capability)
                || !host.contains(d.requires)
                || settings.disabled_actions.contains(&d.id)
                || !a.applies(f, host)
            {
                continue;
            }
            let item = Item::from(f);
            let preview = a.preview(&item, settings);
            let mut needs_confirmation = a.confirmation(&item, settings).is_some();
            if !secrets.is_empty() && d.effects.intersects(Effects::OUTBOUND) {
                let outgoing = preview.clone().or_else(|| f.value.as_text().map(|t| t.into_owned()));
                match outgoing {
                    // Never offer to send a secret anywhere.
                    Some(text) if leaks(&text) || f.capability == caps::SECRET => continue,
                    Some(_) => {}
                    // Pixels may show the secret; allow, but only with confirmation.
                    None => needs_confirmation = true,
                }
            }
            let preferred = settings.preferred_actions.contains(&d.id);
            let score =
                d.priority as f32 + SPECIFICITY_WEIGHT * spec + usage.boost(f.capability.as_str(), &d.id, now) + if preferred { PREFERRED_BOOST } else { 0.0 };
            entries.push(PaletteEntry {
                target: Target::Action(d.id.clone()),
                finding: f.id,
                capability: f.capability.clone(),
                label: d.label.clone(),
                icon: d.icon.clone(),
                group: d.group,
                safety: d.safety(),
                score,
                key: None,
                preview,
                needs_confirmation,
            });
        }
    }

    for chain in chains {
        let Ok(plan) = chain.validate(registry) else { continue };
        let graph = registry.graph();
        let Some(f) = findings
            .iter()
            .filter(|f| graph.is_a(&f.capability, &plan.input))
            .max_by(|a, b| (a.capability == plan.input).cmp(&(b.capability == plan.input)).then(a.confidence.total_cmp(&b.confidence)))
        else {
            continue;
        };
        let target = Target::Chain(chain.id.clone());
        let preferred = settings.preferred_actions.contains(&target.usage_key());
        entries.push(PaletteEntry {
            score: 60.0 + usage.boost(f.capability.as_str(), &target.usage_key(), now) + if preferred { PREFERRED_BOOST } else { 0.0 },
            target,
            finding: f.id,
            capability: f.capability.clone(),
            label: chain.name.clone(),
            icon: "chain".into(),
            group: ActionGroup::System,
            safety: plan.effects.safety_class(),
            key: None,
            preview: None,
            needs_confirmation: plan.needs_confirmation,
        });
    }

    // Deterministic order: score, then stable ids.
    entries.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.target.usage_key().cmp(&b.target.usage_key())).then_with(|| a.finding.cmp(&b.finding)));

    let n = settings.primary_action_count.max(1);
    let mut primary = Vec::new();
    let mut used_targets = HashSet::new();
    let mut used_labels = HashSet::new();
    let mut per_group: HashMap<ActionGroup, usize> = HashMap::new();
    let mut per_finding: HashMap<FindingId, usize> = HashMap::new();
    // Distinct findings are worth surfacing, but never at the cost of leaving slots empty.
    let distinct = entries.iter().map(|e| e.finding).collect::<HashSet<_>>().len();
    let finding_cap = if distinct > 1 { MAX_PER_FINDING } else { usize::MAX };
    for e in &entries {
        if primary.len() >= n {
            break;
        }
        let g = per_group.entry(e.group).or_default();
        let pf = per_finding.entry(e.finding).or_default();
        // The region's baseline actions (Copy, Save, Pin) are the universal fallback: never capped.
        let capped = e.capability != caps::REGION && *pf >= finding_cap;
        if *g >= MAX_PER_GROUP || capped || used_targets.contains(&e.target) || used_labels.contains(&e.label) {
            continue;
        }
        *g += 1;
        *pf += 1;
        used_targets.insert(e.target.clone());
        used_labels.insert(e.label.clone());
        primary.push(e.clone());
    }

    let mut palette = Palette { primary, default_index: 0, all: entries };
    assign_keys(&mut palette, settings, registry);
    palette
}

/// Margin by which a late-arriving action must outrank a visible one to
/// replace it.
const REPLACE_MARGIN: f32 = 12.0;

/// Merges a newly ranked palette into what the user already sees without
/// reordering it. During the brief initial window (`settled == false`) the
/// row is simply replaced. Afterwards:
///
/// * visible entries keep their positions;
/// * entries that are no longer applicable free their slot;
/// * better actions arriving later (e.g. after OCR) fill free slots, or
///   replace the weakest visible entry *in place* if they clearly outrank it;
/// * the Enter default follows the best-ranked entry wherever it sits.
pub fn stabilize(previous: &[PaletteEntry], mut next: Palette, settled: bool, settings: &Settings, registry: &Registry) -> Palette {
    if !settled || previous.is_empty() {
        return next;
    }
    let n = settings.primary_action_count.max(1);
    let same = |a: &PaletteEntry, b: &PaletteEntry| a.target == b.target && a.finding == b.finding;
    // Refresh surviving entries (scores, previews) but keep their slots.
    let mut slots: Vec<Option<PaletteEntry>> = previous.iter().map(|p| next.all.iter().find(|e| same(e, p)).cloned()).collect();
    slots.truncate(n);
    let incoming: Vec<PaletteEntry> = next.primary.iter().filter(|e| !slots.iter().flatten().any(|s| same(s, e) || s.target == e.target)).cloned().collect();
    for e in incoming {
        if let Some(free) = slots.iter().position(Option::is_none) {
            slots[free] = Some(e);
        } else if slots.len() < n {
            slots.push(Some(e));
        } else if let Some((weakest, score)) = slots.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|s| (i, s.score))).min_by(|a, b| a.1.total_cmp(&b.1))
        {
            if e.score > score + REPLACE_MARGIN {
                slots[weakest] = Some(e);
            }
        }
    }
    next.primary = slots.into_iter().flatten().collect();
    next.default_index = next.primary.iter().enumerate().max_by(|a, b| a.1.score.total_cmp(&b.1.score).then(b.0.cmp(&a.0))).map_or(0, |(i, _)| i);
    assign_keys(&mut next, settings, registry);
    next
}

fn assign_keys(p: &mut Palette, settings: &Settings, registry: &Registry) {
    let key_for = |t: &Target| -> Option<char> {
        match t {
            Target::Action(id) => settings.action_keys.get(id).copied().or_else(|| registry.action(id).and_then(|a| a.descriptor().key)),
            Target::Chain(id) => settings.action_keys.get(&format!("chain:{id}")).copied(),
        }
        .map(|c| c.to_ascii_lowercase())
    };
    // Each key belongs to one (action, finding). Primary entries claim first,
    // so visible actions always get theirs; the overflow list reuses them.
    let mut owner: HashMap<char, (Target, FindingId)> = HashMap::new();
    for e in p.primary.iter_mut().chain(p.all.iter_mut()) {
        e.key = None;
        let Some(k) = key_for(&e.target) else { continue };
        let me = (e.target.clone(), e.finding);
        match owner.get(&k) {
            None => {
                owner.insert(k, me);
                e.key = Some(k);
            }
            Some(o) if *o == me => e.key = Some(k),
            Some(_) => {}
        }
    }
}

/// Result of attempting to run an action from the palette.
#[derive(Debug)]
pub enum Invocation {
    Done(ActionOutcome),
    /// Re-invoke with `confirmed = true` once the user accepts.
    NeedsConfirmation(crate::action::ConfirmRequest),
    /// Re-invoke with the chosen value in the `choice` parameter.
    NeedsChoice(Vec<crate::action::Choice>),
}

pub struct InvokeContext<'a> {
    pub registry: &'a Registry,
    pub findings: &'a [Finding],
    pub host: &'a dyn Host,
    pub settings: &'a Settings,
    pub selection: Option<&'a Selection>,
    pub params: &'a Params,
    pub confirmed: bool,
}

/// Runs a single action on a finding, enforcing confirmation and the secret
/// guard at execution time too (the palette is not the only entry point).
pub fn invoke(action_id: &str, finding: FindingId, cx: &InvokeContext) -> Result<Invocation> {
    let a = cx.registry.action(action_id).ok_or_else(|| LensError::InvalidInput(format!("unknown action {action_id}")))?;
    let f = cx.findings.iter().find(|f| f.id == finding).ok_or_else(|| LensError::InvalidInput("unknown finding".into()))?;
    let d = a.descriptor();
    if !d.accepts.contains(&f.capability) {
        return Err(LensError::InvalidInput(format!("{} does not accept {}", d.label, f.capability)));
    }
    let item = Item::from(f);
    if cx.settings.privacy.guard_secrets && d.effects.intersects(Effects::OUTBOUND) {
        let outgoing = a.preview(&item, cx.settings).or_else(|| f.value.as_text().map(|t| t.into_owned()));
        let leaks = f.capability == caps::SECRET
            || outgoing.is_some_and(|text| cx.findings.iter().any(|s| matches!(&s.value, Value::Secret(sv) if text.contains(sv.raw.expose()))));
        if leaks {
            return Err(LensError::Blocked("the content appears to contain a secret".into()));
        }
    }
    if !cx.params.contains_key("choice") {
        let choices = a.choices(&item, cx.settings);
        if !choices.is_empty() {
            return Ok(Invocation::NeedsChoice(choices));
        }
    }
    if !cx.confirmed {
        if let Some(req) = a.confirmation(&item, cx.settings) {
            return Ok(Invocation::NeedsConfirmation(req));
        }
    }
    let acx = ActionContext { host: cx.host, settings: cx.settings, selection: cx.selection, params: cx.params };
    let mut outcome = a.execute(&item, &acx)?;
    // A pure transform invoked from the palette has nowhere else to go: copy its result.
    if d.safety() == SafetyClass::Pure && outcome.message.is_none() {
        if let Some(out) = &outcome.output {
            if let Some(img) = out.value.as_image() {
                cx.host.set_clipboard_image(img)?;
                outcome.message = Some(format!("{} — copied", d.label));
            } else if let Some(text) = out.value.as_text() {
                cx.host.set_clipboard_text(&text)?;
                outcome.message = Some(format!("{} — copied", d.label));
            }
        }
    }
    Ok(Invocation::Done(outcome))
}
