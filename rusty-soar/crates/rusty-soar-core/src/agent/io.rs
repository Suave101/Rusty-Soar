//! Input and output links (upstream `io_link.cpp`).
//!
//! Input: upstream input functions run in the input phase and call `add_input_wme` and
//! `remove_input_wme`. Here the caller makes the same calls between decision cycles, which is
//! equivalent: nothing runs between one cycle's output phase and the next cycle's input phase.
//!
//! Output: upstream calls a registered output function for each changed output link in the
//! output phase. Here the output phase records the same calls as [`OutputEvent`]s, which the
//! caller takes with [`SoarAgent::take_output_events`]. The link monitored is `output-link`, the
//! one SML registers.

use super::SoarAgent;
use crate::symbol::SymbolId;
use crate::wm::{SupportType, WmeKey};
use alloc::vec::Vec;

/// A WME reported to an output function (upstream `io_wme`). `key` stands in for upstream's
/// timetag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoWme {
    pub key: WmeKey,
    pub id: SymbolId,
    pub attr: SymbolId,
    pub value: SymbolId,
}

/// Why an output function was called (upstream `output_call_info.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// `ADDED_OUTPUT_COMMAND`: the link WME was just added.
    Added,
    /// `MODIFIED_OUTPUT_COMMAND`: a WME in the link's transitive closure changed.
    Modified,
    /// `REMOVED_OUTPUT_COMMAND`: the link WME was removed.
    Removed,
}

/// One output-function call: the link WME and every WME in the transitive closure of its value.
/// A removed link reports only the link WME.
///
/// Order follows upstream's list construction (ids in reverse discovery order, the link WME
/// last), except that WMEs of one identifier come in arena order rather than newest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputEvent {
    pub mode: OutputMode,
    pub wmes: Vec<IoWme>,
}

/// Upstream output-link statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkStatus {
    New,
    Unchanged,
    ModifiedButSameTc,
    Modified,
    Removed,
}

#[derive(Debug, Clone)]
struct OutputLink {
    status: LinkStatus,
    /// The `(io ^output-link X)` WME, kept after its removal as upstream keeps a reference.
    wme: IoWme,
    /// Identifiers in the transitive closure of the link's value, as of the last output phase.
    ids_in_tc: Vec<SymbolId>,
}

/// I/O state of an agent.
#[derive(Debug, Default)]
pub(super) struct Io {
    header: Option<SymbolId>,
    input_link: Option<SymbolId>,
    output_link: Option<SymbolId>,
    /// Input WMEs currently in working memory (upstream `id->input_wmes`).
    input_wmes: Vec<WmeKey>,
    /// Upstream `existing_output_links`, newest first.
    links: Vec<OutputLink>,
    events: Vec<OutputEvent>,
    /// Upstream `output_link_changed`: set when the agent changes an output link, cleared by the
    /// input phase.
    output_link_changed: bool,
}

impl<S: crate::pool::Layout> SoarAgent<S> {
    /// Creates `(S1 ^io I1)`, `(I1 ^input-link I2)` and `(I1 ^output-link I3)` as input WMEs and
    /// runs one input and one output cycle, as upstream `init_agent_memory` does. The output
    /// cycle registers the output link; its report is discarded, as no client can see it.
    pub(super) fn create_io_links(&mut self, state: SymbolId) -> SymbolId {
        let header = self.new_input_identifier('I');
        let input_link = self.new_input_identifier('I');
        let output_link = self.new_input_identifier('I');
        self.io.header = Some(header);
        self.io.input_link = Some(input_link);
        self.io.output_link = Some(output_link);
        let io = self.symbols.predefined.io;
        let input = self.symbols.predefined.input_link;
        let output = self.symbols.predefined.output_link;
        self.add_input_wme(state, io, header);
        self.add_input_wme(header, input, input_link);
        self.add_input_wme(header, output, output_link);
        self.do_input_cycle();
        self.do_output_cycle();
        self.io.events.clear();
        input_link
    }

    /// The input link, `I2`, once the top state exists.
    pub fn input_link(&self) -> Option<SymbolId> {
        self.io.input_link
    }

    /// The output link, `I3`, once the top state exists.
    pub fn output_link(&self) -> Option<SymbolId> {
        self.io.output_link
    }

    /// A new identifier for input structure (upstream `get_new_io_identifier`), at the top
    /// goal's level.
    pub fn new_input_identifier(&mut self, letter: char) -> SymbolId {
        self.new_identifier(letter, 1)
    }

    /// Adds an input WME (upstream `add_input_wme`). It has no preference, only
    /// [`SoarAgent::remove_input_wme`] removes it, and, as upstream, adding the same triple twice
    /// makes two WMEs.
    pub fn add_input_wme(&mut self, id: SymbolId, attr: SymbolId, value: SymbolId) -> WmeKey {
        let key = self.add_wme_to_wm(id, attr, value, false, SupportType::Input);
        if self.wm.get(key).is_some() {
            self.io.input_wmes.push(key);
        }
        key
    }

    /// Removes an input WME (upstream `remove_input_wme`), first removing any substate whose goal
    /// dependency set holds it. Returns `false`, with upstream's error message, for a WME that is
    /// not a current input WME.
    pub fn remove_input_wme(&mut self, key: WmeKey) -> bool {
        let Some(index) = self.io.input_wmes.iter().position(|k| *k == key) else {
            self.output.push_str("Error: an input routine called remove_input_wme on a wme that\n");
            self.output.push_str("isn't one of the input wmes currently in working memory.\n");
            return false;
        };
        self.io.input_wmes.remove(index);
        self.remove_wme(key);
        true
    }

    /// Output-function calls made by output phases since the last call.
    pub fn take_output_events(&mut self) -> Vec<OutputEvent> {
        core::mem::take(&mut self.io.events)
    }

    /// Whether the agent changed an output link since the last input phase (upstream
    /// `output_link_changed`).
    pub fn output_link_changed(&self) -> bool {
        self.io.output_link_changed
    }

    /// Upstream `do_input_cycle`. Input changes were made before it; it only clears
    /// `output_link_changed`, so changes made by input do not count as output.
    pub(super) fn do_input_cycle(&mut self) {
        self.io.output_link_changed = false;
    }

    /// Upstream `inform_output_module_of_wm_changes`, for one WME added to or removed from
    /// working memory.
    pub(super) fn inform_output_module(&mut self, wme: IoWme, added: bool) {
        if Some(wme.id) == self.io.header {
            if added {
                self.update_for_top_state_wme_addition(wme);
                self.io.output_link_changed = true;
            } else if let Some(link) = self.io.links.iter_mut().find(|l| l.wme.key == wme.key) {
                link.status = LinkStatus::Removed;
            }
        }
        let value_is_id = self.symbols.is_identifier(wme.value);
        let mut associated = false;
        for link in self.io.links.iter_mut().filter(|l| l.ids_in_tc.contains(&wme.id)) {
            associated = true;
            link.status = match (link.status, value_is_id) {
                (LinkStatus::Unchanged | LinkStatus::ModifiedButSameTc, true) => LinkStatus::Modified,
                (LinkStatus::Unchanged, false) => LinkStatus::ModifiedButSameTc,
                (status, _) => status,
            };
        }
        if associated && added {
            self.io.output_link_changed = true;
        }
    }

    /// Upstream `update_for_top_state_wme_addition`: a WME on the io header whose attribute
    /// names an output function starts a new output link.
    fn update_for_top_state_wme_addition(&mut self, wme: IoWme) {
        let is_output_function = wme.attr == self.symbols.predefined.output_link;
        if is_output_function {
            self.io.links.insert(0, OutputLink { status: LinkStatus::New, wme, ids_in_tc: Vec::new() });
        }
    }

    /// Upstream `do_output_cycle`.
    pub(super) fn do_output_cycle(&mut self) {
        let mut i = 0;
        while i < self.io.links.len() {
            let mode = match self.io.links[i].status {
                LinkStatus::Unchanged => None,
                LinkStatus::New => {
                    self.calculate_output_link_tc(i);
                    Some(OutputMode::Added)
                }
                LinkStatus::ModifiedButSameTc => Some(OutputMode::Modified),
                LinkStatus::Modified => {
                    self.calculate_output_link_tc(i);
                    Some(OutputMode::Modified)
                }
                LinkStatus::Removed => {
                    self.io.links[i].ids_in_tc.clear();
                    Some(OutputMode::Removed)
                }
            };
            if let Some(mode) = mode {
                let wmes = self.io_wmes_for_output_link(i);
                self.io.events.push(OutputEvent { mode, wmes });
            }
            if mode == Some(OutputMode::Removed) {
                self.io.links.remove(i);
            } else {
                self.io.links[i].status = LinkStatus::Unchanged;
                i += 1;
            }
        }
    }

    /// Upstream `calculate_output_link_tc_info`: the identifiers reachable from the link's
    /// value through WMEs, in upstream's list order (most recently reached first).
    fn calculate_output_link_tc(&mut self, link: usize) {
        let mut ids = Vec::new();
        let value = self.io.links[link].wme.value;
        if self.symbols.is_identifier(value) {
            self.add_id_to_output_link_tc(value, &mut ids);
        }
        ids.reverse();
        self.io.links[link].ids_in_tc = ids;
    }

    /// Upstream `add_id_to_output_link_tc`, depth first; `ids` is in discovery order.
    fn add_id_to_output_link_tc(&self, id: SymbolId, ids: &mut Vec<SymbolId>) {
        if ids.contains(&id) {
            return;
        }
        ids.push(id);
        let values: Vec<SymbolId> = self
            .wm
            .iter()
            .filter(|w| w.id == id && !w.acceptable && self.symbols.is_identifier(w.val))
            .map(|w| w.val)
            .collect();
        for value in values {
            self.add_id_to_output_link_tc(value, ids);
        }
    }

    /// Upstream `get_io_wmes_for_output_link`: the link WME and the WMEs of every identifier in
    /// its transitive closure, each added at the head of the list.
    fn io_wmes_for_output_link(&self, link: usize) -> Vec<IoWme> {
        let link = &self.io.links[link];
        let mut collected = alloc::vec![link.wme];
        for id in &link.ids_in_tc {
            collected.extend(
                self.wm
                    .iter()
                    .filter(|w| w.id == *id && !w.acceptable)
                    .map(|w| IoWme { key: w.key, id: w.id, attr: w.attr, value: w.val }),
            );
        }
        collected.reverse();
        collected
    }
}
