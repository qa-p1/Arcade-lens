//! Closure-based action construction, so simple providers don't need a type
//! per action.

use crate::action::{Action, ActionContext, ActionDescriptor, ActionGroup, ActionOutcome, Choice, ConfirmRequest, Effects, Item, ParamSpec, Produces};
use crate::capability::Capability;
use crate::error::Result;
use crate::finding::Finding;
use crate::host::HostFeatures;
use crate::settings::Settings;

type AppliesFn = Box<dyn Fn(&Finding, HostFeatures) -> bool + Send + Sync>;
type TextFn = Box<dyn Fn(&Item, &Settings) -> Option<String> + Send + Sync>;
type ConfirmFn = Box<dyn Fn(&Item, &Settings) -> Option<ConfirmRequest> + Send + Sync>;
type ChoicesFn = Box<dyn Fn(&Item, &Settings) -> Vec<Choice> + Send + Sync>;
type RunFn = Box<dyn Fn(&Item, &ActionContext) -> Result<ActionOutcome> + Send + Sync>;

pub struct FnAction {
    descriptor: ActionDescriptor,
    applies: Option<AppliesFn>,
    preview: Option<TextFn>,
    confirm: Option<ConfirmFn>,
    choices: Option<ChoicesFn>,
    run: RunFn,
}

impl Action for FnAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn applies(&self, input: &Finding, host: HostFeatures) -> bool {
        self.applies.as_ref().is_none_or(|f| f(input, host))
    }

    fn preview(&self, input: &Item, settings: &Settings) -> Option<String> {
        self.preview.as_ref().and_then(|f| f(input, settings))
    }

    fn confirmation(&self, input: &Item, settings: &Settings) -> Option<ConfirmRequest> {
        match &self.confirm {
            Some(f) => f(input, settings),
            None => {
                let d = &self.descriptor;
                (d.safety() == crate::action::SafetyClass::Dangerous).then(|| ConfirmRequest {
                    title: d.label.clone(),
                    subject: self.preview(input, settings).or_else(|| input.value.as_text().map(|t| t.into_owned())).unwrap_or_default(),
                    reasons: vec!["This action can make changes that are hard to undo.".into()],
                })
            }
        }
    }

    fn choices(&self, input: &Item, settings: &Settings) -> Vec<Choice> {
        self.choices.as_ref().map_or_else(Vec::new, |f| f(input, settings))
    }

    fn execute(&self, input: &Item, cx: &ActionContext) -> Result<ActionOutcome> {
        (self.run)(input, cx)
    }
}

pub struct ActionBuilder {
    d: ActionDescriptor,
    applies: Option<AppliesFn>,
    preview: Option<TextFn>,
    confirm: Option<ConfirmFn>,
    choices: Option<ChoicesFn>,
}

/// Starts building an action with the given namespaced id and label.
pub fn action(id: impl Into<String>, label: impl Into<String>) -> ActionBuilder {
    ActionBuilder {
        d: ActionDescriptor {
            id: id.into(),
            label: label.into(),
            icon: String::new(),
            group: ActionGroup::Copy,
            accepts: Vec::new(),
            produces: Produces::Nothing,
            priority: 50,
            key: None,
            effects: Effects::empty(),
            requires: HostFeatures::empty(),
            in_palette: true,
            params: Vec::new(),
        },
        applies: None,
        preview: None,
        confirm: None,
        choices: None,
    }
}

impl ActionBuilder {
    pub fn icon(mut self, icon: &str) -> Self {
        self.d.icon = icon.into();
        self
    }
    pub fn group(mut self, g: ActionGroup) -> Self {
        self.d.group = g;
        self
    }
    pub fn accepts(mut self, c: Capability) -> Self {
        self.d.accepts.push(c);
        self
    }
    pub fn accepts_all(mut self, cs: impl IntoIterator<Item = Capability>) -> Self {
        self.d.accepts.extend(cs);
        self
    }
    pub fn produces(mut self, c: Capability) -> Self {
        self.d.produces = Produces::Capability(c);
        self
    }
    pub fn passthrough(mut self) -> Self {
        self.d.produces = Produces::Same;
        self
    }
    pub fn priority(mut self, p: i32) -> Self {
        self.d.priority = p;
        self
    }
    pub fn key(mut self, k: char) -> Self {
        self.d.key = Some(k);
        self
    }
    pub fn effects(mut self, e: Effects) -> Self {
        self.d.effects = e;
        self
    }
    pub fn requires(mut self, f: HostFeatures) -> Self {
        self.d.requires = f;
        self
    }
    pub fn chain_only(mut self) -> Self {
        self.d.in_palette = false;
        self
    }
    pub fn param(mut self, name: &str, description: &str, default: serde_json::Value) -> Self {
        self.d.params.push(ParamSpec { name: name.into(), description: description.into(), default });
        self
    }
    pub fn applies(mut self, f: impl Fn(&Finding, HostFeatures) -> bool + Send + Sync + 'static) -> Self {
        self.applies = Some(Box::new(f));
        self
    }
    pub fn preview(mut self, f: impl Fn(&Item, &Settings) -> Option<String> + Send + Sync + 'static) -> Self {
        self.preview = Some(Box::new(f));
        self
    }
    pub fn confirm(mut self, f: impl Fn(&Item, &Settings) -> Option<ConfirmRequest> + Send + Sync + 'static) -> Self {
        self.confirm = Some(Box::new(f));
        self
    }
    pub fn choices(mut self, f: impl Fn(&Item, &Settings) -> Vec<Choice> + Send + Sync + 'static) -> Self {
        self.choices = Some(Box::new(f));
        self
    }
    pub fn run(self, f: impl Fn(&Item, &ActionContext) -> Result<ActionOutcome> + Send + Sync + 'static) -> FnAction {
        FnAction { descriptor: self.d, applies: self.applies, preview: self.preview, confirm: self.confirm, choices: self.choices, run: Box::new(f) }
    }
}
