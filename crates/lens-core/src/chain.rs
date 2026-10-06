//! Action chains: user-defined, typed pipelines such as
//! `take text → remove line breaks → copy` saved as "Clean Copy".
//!
//! Chains are validated against action input/output capabilities before they
//! can be saved or run, so "resize" can never receive a URL. A chain's
//! effects are the union of its steps' effects; chains containing anything
//! Dangerous or External require confirmation.

use serde::{Deserialize, Serialize};

use crate::action::{ActionContext, ActionOutcome, Effects, Item, Params, Produces, SafetyClass};
use crate::cancel::CancelToken;
use crate::capability::{caps, Capability};
use crate::error::{LensError, Result};
use crate::finding::Finding;
use crate::host::Host;
use crate::registry::Registry;
use crate::selection::Selection;
use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub id: String,
    pub name: String,
    pub steps: Vec<ChainStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum ChainStep {
    /// Pick the best finding with (a descendant of) this capability from the
    /// analysis — e.g. `text` for "OCR", `table` for "OCR Table".
    Take { capability: Capability },
    /// Run an action on the current value.
    Run {
        action: String,
        #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
        params: Params,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChainPlan {
    /// Capability the chain starts from.
    pub input: Capability,
    pub effects: Effects,
    pub needs_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("step {step}: {message}")]
pub struct ChainError {
    pub step: usize,
    pub message: String,
}

impl Chain {
    /// Type-checks the chain. Returns what it needs and what it may do.
    pub fn validate(&self, registry: &Registry) -> std::result::Result<ChainPlan, ChainError> {
        let err = |step: usize, message: String| ChainError { step, message };
        let graph = registry.graph();
        let (input, mut rest) = match self.steps.first() {
            Some(ChainStep::Take { capability }) => (capability.clone(), &self.steps[1..]),
            Some(ChainStep::Run { .. }) => (caps::REGION, &self.steps[..]),
            None => return Err(err(0, "chain is empty".into())),
        };
        let offset = self.steps.len() - rest.len();
        let mut current = Some(input.clone());
        let mut effects = Effects::empty();
        let mut index = offset;
        while let Some((step, tail)) = rest.split_first() {
            let ChainStep::Run { action, .. } = step else {
                return Err(err(index, "`take` is only allowed as the first step".into()));
            };
            let Some(a) = registry.action(action) else {
                return Err(err(index, format!("unknown action {action}")));
            };
            let d = a.descriptor();
            let Some(cur) = current.clone() else {
                return Err(err(index, "the previous step produces nothing to continue with".into()));
            };
            if !d.accepts.iter().any(|acc| graph.is_a(&cur, acc)) {
                return Err(err(index, format!("{} does not accept {}", d.label, cur)));
            }
            effects |= d.effects;
            current = match &d.produces {
                Produces::Nothing => None,
                Produces::Same => Some(cur),
                Produces::Capability(c) => Some(c.clone()),
            };
            rest = tail;
            index += 1;
        }
        if index == offset {
            return Err(err(index, "chain has no actions".into()));
        }
        let needs_confirmation = effects.safety_class() >= SafetyClass::External;
        Ok(ChainPlan { input, effects, needs_confirmation })
    }

    /// Runs the chain against an analysis. `confirmed` must be true if the
    /// plan requires confirmation; the UI is expected to have shown the plan.
    pub fn run(&self, cx: ChainRunContext) -> Result<Vec<ActionOutcome>> {
        let plan = self.validate(cx.registry).map_err(|e| LensError::InvalidInput(e.to_string()))?;
        if plan.needs_confirmation && !cx.confirmed {
            return Err(LensError::Blocked(format!("chain \"{}\" requires confirmation", self.name)));
        }
        let graph = cx.registry.graph();
        let start = cx
            .findings
            .iter()
            .filter(|f| graph.is_a(&f.capability, &plan.input))
            // Prefer an exact capability match over descendants (text over url).
            .max_by(|a, b| (a.capability == plan.input).cmp(&(b.capability == plan.input)).then(a.confidence.total_cmp(&b.confidence)))
            .ok_or_else(|| LensError::InvalidInput(format!("nothing in the selection provides {}", plan.input)))?;
        let mut current = Item::from(start);
        let mut outcomes = Vec::new();
        for step in &self.steps {
            let ChainStep::Run { action, params } = step else { continue };
            cx.cancel.check()?;
            let a = cx.registry.action(action).expect("validated");
            let acx = ActionContext { host: cx.host, settings: cx.settings, selection: cx.selection, params, cancel: Some(cx.cancel) };
            let outcome = a.execute(&current, &acx)?;
            if let Some(next) = &outcome.output {
                current = next.clone();
            } else if a.descriptor().produces != Produces::Same {
                outcomes.push(outcome);
                break;
            }
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }
}

pub struct ChainRunContext<'a> {
    pub registry: &'a Registry,
    pub findings: &'a [Finding],
    pub host: &'a dyn Host,
    pub settings: &'a Settings,
    pub selection: Option<&'a Selection>,
    pub cancel: &'a CancelToken,
    pub confirmed: bool,
}
