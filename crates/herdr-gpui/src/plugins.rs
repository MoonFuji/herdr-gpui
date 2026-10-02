//! Plugin actions each connected daemon exposes to endpoint clients.
//!
//! Herdr's endpoint offers no `plugin.*` method, so a GUI client cannot list,
//! enable, disable, or read the logs of plugins. What it does offer is the
//! snapshot's command manifest: actions a host binds with a `[[keys.command]]`
//! of `type = "plugin_action"`, runnable through `command.invoke`. This module
//! projects those per host and runs them; it never installs anything.
use crate::{Error, HerdrWindow, Result, palette::Target};
use herdr_client::{
    Method,
    protocol::{ClientShellCommand, ClientShellCommandAction, ClientShellSnapshot},
};

/// A bound plugin action, as listed by one daemon boot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginAction {
    pub command_id: String,
    pub label: String,
    pub bindings: Vec<String>,
}

impl PluginAction {
    fn new(command: &ClientShellCommand) -> Self {
        let mut bindings = command.binding_labels.clone();
        if !command.binding_label.is_empty() && !bindings.contains(&command.binding_label) {
            bindings.push(command.binding_label.clone());
        }
        bindings.retain(|binding| !binding.trim().is_empty());
        Self {
            command_id: command.command_id.clone(),
            label: command
                .description
                .as_deref()
                .map(str::trim)
                .filter(|description| !description.is_empty())
                .unwrap_or("Unnamed plugin action")
                .to_owned(),
            bindings,
        }
    }

    /// Case-insensitive match on the label or any binding; `query` is already lowercase.
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.label.to_lowercase().contains(query)
            || self
                .bindings
                .iter()
                .any(|binding| binding.to_lowercase().contains(query))
    }
}

/// The plugin actions in `snapshot`'s command manifest, in manifest order.
pub(crate) fn plugin_actions(snapshot: &ClientShellSnapshot) -> Vec<PluginAction> {
    snapshot
        .commands
        .iter()
        .filter(|command| command.action == ClientShellCommandAction::PluginAction)
        .map(PluginAction::new)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostState {
    /// The window's selected host; actions run on its focused pane.
    Selected {
        ready: bool,
    },
    /// Connected, but actions only run on the selected host.
    Other,
    Disconnected,
}

/// One enabled host and the plugin actions its current boot lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginHost {
    pub endpoint: String,
    pub label: String,
    pub boot: String,
    pub state: HostState,
    pub actions: Vec<PluginAction>,
}

impl HerdrWindow {
    /// Every enabled host, in sidebar order, with its bound plugin actions.
    pub(crate) fn plugin_hosts(&self) -> Vec<PluginHost> {
        self.endpoints
            .iter()
            .enumerate()
            .filter(|(_, endpoint)| endpoint.enabled)
            .map(|(index, endpoint)| {
                let selected = index == self.selected_endpoint;
                let live = if selected { &self.live } else { &endpoint.live };
                let snapshot = live
                    .snapshot
                    .as_deref()
                    .filter(|_| live.status.is_connected());
                let state = match snapshot {
                    None => HostState::Disconnected,
                    Some(_) if selected => HostState::Selected {
                        ready: self.plugin_action_ready(),
                    },
                    Some(_) => HostState::Other,
                };
                PluginHost {
                    endpoint: endpoint.id.clone(),
                    label: endpoint.label.clone(),
                    boot: snapshot.map(|s| s.boot_id.clone()).unwrap_or_default(),
                    state,
                    actions: snapshot.map(plugin_actions).unwrap_or_default(),
                }
            })
            .collect()
    }

    fn plugin_action_ready(&self) -> bool {
        self.activation_deadline.is_none()
            && self
                .endpoints
                .get(self.selected_endpoint)
                .is_some_and(|endpoint| endpoint.surface_requested())
            && self.input_ready()
    }

    /// Runs a plugin action listed by `endpoint`'s `boot` on that host's focused
    /// workspace, tab, and pane, as the palette runs a configured command.
    /// Success means the request was queued, not that the daemon accepted it.
    pub(crate) fn run_plugin_action(
        &mut self,
        endpoint: &str,
        boot: &str,
        command_id: &str,
    ) -> Result<()> {
        if self
            .endpoints
            .get(self.selected_endpoint)
            .is_none_or(|selected| selected.id != endpoint)
        {
            return Err(Error::PluginHostNotSelected);
        }
        if !self.plugin_action_ready() {
            return Err(Error::PluginHostNotReady);
        }
        let snapshot = self.live.snapshot.as_ref().ok_or(Error::NoSnapshot)?;
        if snapshot.boot_id != boot
            || !snapshot.commands.iter().any(|command| {
                command.command_id == command_id
                    && command.action == ClientShellCommandAction::PluginAction
            })
        {
            return Err(Error::PluginActionChanged);
        }
        let params = Target::capture(snapshot).invocation(
            snapshot,
            command_id,
            ClientShellCommandAction::PluginAction,
        )?;
        if !self.request_focus_change(Method::CommandInvoke.as_str(), None, |handle, boot| {
            handle.request(boot, Method::CommandInvoke, params)
        }) {
            return Err(Error::NotConnected);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn command(id: &str, action: ClientShellCommandAction) -> ClientShellCommand {
        ClientShellCommand {
            command_id: id.into(),
            binding_label: "prefix+p".into(),
            binding_labels: vec!["prefix+p".into(), "ctrl+alt+p".into()],
            action,
            description: Some(" Open dashboard ".into()),
        }
    }

    fn snapshot(commands: Vec<ClientShellCommand>) -> ClientShellSnapshot {
        let mut snapshot: ClientShellSnapshot = serde_json::from_str(include_str!(
            "../../herdr-protocol/tests/fixtures/endpoint-snapshot-v1.json"
        ))
        .unwrap();
        snapshot.commands = commands;
        snapshot
    }

    #[test]
    fn lists_only_plugin_actions_in_manifest_order() {
        let snapshot = snapshot(vec![
            command("shell", ClientShellCommandAction::Shell),
            command("b", ClientShellCommandAction::PluginAction),
            command("popup", ClientShellCommandAction::Popup),
            command("a", ClientShellCommandAction::PluginAction),
            command("future", ClientShellCommandAction::Unknown),
        ]);
        let actions = plugin_actions(&snapshot);
        assert_eq!(
            actions
                .iter()
                .map(|action| action.command_id.as_str())
                .collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(actions[0].label, "Open dashboard");
        assert_eq!(actions[0].bindings, ["prefix+p", "ctrl+alt+p"]);
    }

    #[test]
    fn labels_and_bindings_fall_back_without_duplicates() {
        let mut command = command("x", ClientShellCommandAction::PluginAction);
        command.description = Some("   ".into());
        command.binding_labels = vec![" ".into()];
        command.binding_label = "prefix+y".into();
        let action = PluginAction::new(&command);
        assert_eq!(action.label, "Unnamed plugin action");
        assert_eq!(action.bindings, ["prefix+y"]);
        command.description = None;
        command.binding_label = String::new();
        command.binding_labels.clear();
        let action = PluginAction::new(&command);
        assert_eq!(action.label, "Unnamed plugin action");
        assert!(action.bindings.is_empty());
    }

    #[test]
    fn search_matches_label_or_binding_case_insensitively() {
        let action = PluginAction::new(&command("x", ClientShellCommandAction::PluginAction));
        assert!(action.matches(""));
        assert!(action.matches("dashboard"));
        assert!(action.matches("ctrl+alt"));
        assert!(!action.matches("missing"));
        // Callers pass a lowercased query; an uppercase one never matches.
        assert!(!action.matches("DASHBOARD"));
    }
}
