//! Identifier levels and garbage collection, ported from upstream `decide.cpp`
//! ("Ownership Calculations": `post_link_addition`, `post_link_removal`, `do_promotion`,
//! `do_demotion`, `garbage_collect_id`).
//!
//! Every WME and every preference whose value (or binary referent) is an identifier is a link
//! from its identifier to that value; each goal has one special link. An identifier's level is
//! the highest goal (lowest level number) it is linked from. Link additions promote; link
//! removals make levels unknown, and at the end of the phase identifiers left without links,
//! or no longer reachable from any goal, are garbage collected.

use super::SoarAgent;
use crate::preference::PreferenceType;
use crate::symbol::SymbolId;
use crate::wm::WmeKey;
use alloc::vec::Vec;

/// Level of an identifier not (yet) linked to any goal (upstream
/// `LOWEST_POSSIBLE_GOAL_LEVEL`).
pub(super) const LOWEST_POSSIBLE_GOAL_LEVEL: usize = usize::MAX / 2;

#[derive(Debug, Clone)]
struct IdLinks {
    id: SymbolId,
    level: usize,
    promotion_level: usize,
    link_count: i64,
    could_be_a_link_from_below: bool,
    unknown_level: bool,
    tc: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum LinkUpdateMode {
    #[default]
    Normally,
    UpdateDisconnectedIdsList,
    JustUpdateCount,
}

#[derive(Debug, Default)]
pub(super) struct Links {
    records: Vec<IdLinks>,
    promoted_ids: Vec<SymbolId>,
    ids_with_unknown_level: Vec<SymbolId>,
    disconnected_ids: Vec<SymbolId>,
    mode: LinkUpdateMode,
    next_tc: u64,
}

impl Links {
    fn index(&mut self, id: SymbolId) -> usize {
        if let Some(i) = self.records.iter().position(|r| r.id == id) {
            return i;
        }
        self.records.push(IdLinks {
            id,
            level: LOWEST_POSSIBLE_GOAL_LEVEL,
            promotion_level: LOWEST_POSSIBLE_GOAL_LEVEL,
            link_count: 0,
            could_be_a_link_from_below: false,
            unknown_level: false,
            tc: 0,
        });
        self.records.len() - 1
    }

    fn get(&self, id: SymbolId) -> Option<&IdLinks> {
        self.records.iter().find(|r| r.id == id)
    }

    /// Goal level (1 = top) of an identifier linked to the goal stack.
    pub(super) fn level(&self, id: SymbolId) -> Option<usize> {
        self.get(id).map(|r| r.level).filter(|l| *l < LOWEST_POSSIBLE_GOAL_LEVEL)
    }

    /// Every identifier linked to the goal stack, with its level.
    pub(super) fn known_levels(&self) -> Vec<(SymbolId, usize)> {
        self.records.iter().filter(|r| r.level < LOWEST_POSSIBLE_GOAL_LEVEL).map(|r| (r.id, r.level)).collect()
    }

    /// Identifiers whose level is at most `level`.
    pub(super) fn ids_at_or_above(&self, level: usize) -> Vec<SymbolId> {
        self.records.iter().filter(|r| r.level <= level).map(|r| r.id).collect()
    }

    /// Upstream `make_new_identifier(letter, level)`: a new identifier starts at a level.
    pub(super) fn set_new_id_level(&mut self, id: SymbolId, level: usize) {
        let i = self.index(id);
        self.records[i].level = level;
        self.records[i].promotion_level = level;
    }

    fn new_tc(&mut self) -> u64 {
        self.next_tc += 1;
        self.next_tc
    }

    fn mark_unknown(&mut self, id: SymbolId) {
        let i = self.index(id);
        if !self.records[i].unknown_level {
            self.records[i].unknown_level = true;
            self.ids_with_unknown_level.push(id);
        }
    }
}

fn is_identifier<S: crate::pool::Layout>(agent: &SoarAgent<S>, s: SymbolId) -> bool {
    agent.symbols.is_identifier(s)
}

impl<S: crate::pool::Layout> SoarAgent<S> {
    /// Upstream `post_link_addition`; `from == None` is a goal's special link.
    pub(super) fn post_link_addition(&mut self, from: Option<SymbolId>, to: SymbolId) {
        if from.is_some() && self.symbols.is_goal(to) {
            return;
        }
        let links = &mut self.decider.links;
        let t = links.index(to);
        links.records[t].link_count += 1;
        let Some(from) = from else { return };
        let f = links.index(from);
        let (from_promotion, to_promotion) = (links.records[f].promotion_level, links.records[t].promotion_level);
        if from_promotion == to_promotion {
            return;
        }
        if from_promotion > to_promotion {
            links.records[t].could_be_a_link_from_below = true;
            return;
        }
        links.records[t].promotion_level = from_promotion;
        links.promoted_ids.push(to);
    }

    /// Upstream `post_link_removal`; `from == None` is a goal's special link.
    pub(super) fn post_link_removal(&mut self, from: Option<SymbolId>, to: SymbolId) {
        if from.is_some() && self.symbols.is_goal(to) {
            return;
        }
        let links = &mut self.decider.links;
        let t = links.index(to);
        links.records[t].link_count -= 1;
        match links.mode {
            LinkUpdateMode::JustUpdateCount => return,
            LinkUpdateMode::UpdateDisconnectedIdsList if links.records[t].link_count == 0 => {
                links.ids_with_unknown_level.retain(|i| *i != to);
                links.records[t].unknown_level = true;
                if !links.disconnected_ids.contains(&to) {
                    links.disconnected_ids.push(to);
                }
                return;
            }
            _ => {}
        }
        // A link removed from another level means another link exists at this one.
        if let Some(from) = from {
            let f = links.index(from);
            if links.records[f].level != links.records[t].level {
                return;
            }
        }
        links.mark_unknown(to);
    }

    /// Links for a WME entering or leaving working memory.
    pub(super) fn post_wme_links(&mut self, id: SymbolId, value: SymbolId, added: bool) {
        if is_identifier(self, value) {
            if added {
                self.post_link_addition(Some(id), value);
            } else {
                self.post_link_removal(Some(id), value);
            }
        }
    }

    /// Links for a preference entering or leaving preference memory.
    pub(super) fn post_preference_links(&mut self, id: SymbolId, value: SymbolId, preference_type: PreferenceType, added: bool) {
        let referent = match preference_type {
            PreferenceType::Better(r) | PreferenceType::Worse(r) | PreferenceType::BinaryIndifferent(r) | PreferenceType::NumericIndifferent(r) => Some(r),
            _ => None,
        };
        for to in core::iter::once(value).chain(referent) {
            if is_identifier(self, to) {
                if added {
                    self.post_link_addition(Some(id), to);
                } else {
                    self.post_link_removal(Some(id), to);
                }
            }
        }
    }

    /// Identifiers an identifier links to: values of its WMEs (other than a goal's impasse
    /// WMEs) and values and referents of its preferences.
    fn linked_ids(&self, id: SymbolId) -> Vec<SymbolId> {
        let impasse: Vec<WmeKey> = self.decider.goals.iter().flat_map(|g| g.impasse_wmes.iter().copied()).collect();
        let mut out: Vec<SymbolId> = self
            .wm
            .iter()
            .filter(|w| w.id == id && !w.acceptable && !impasse.contains(&w.key))
            .map(|w| w.val)
            .collect();
        for p in self.preferences.iter().filter(|p| p.id == id) {
            out.push(p.value);
            match p.preference_type {
                PreferenceType::Better(r) | PreferenceType::Worse(r) | PreferenceType::BinaryIndifferent(r) | PreferenceType::NumericIndifferent(r) => {
                    out.push(r)
                }
                _ => {}
            }
        }
        out.retain(|s| is_identifier(self, *s));
        out
    }

    /// Upstream `promote_id_and_tc`.
    fn promote_id_and_tc(&mut self, id: SymbolId, new_level: usize) {
        let mut stack = alloc::vec![id];
        while let Some(id) = stack.pop() {
            let links = &mut self.decider.links;
            let i = links.index(id);
            let r = &mut links.records[i];
            if r.level <= new_level || r.promotion_level < new_level {
                continue;
            }
            r.level = new_level;
            r.promotion_level = new_level;
            r.could_be_a_link_from_below = true;
            if self.symbols.is_goal(id) {
                continue; // upstream treats promoting a goal as an internal error
            }
            stack.extend(self.linked_ids(id));
        }
    }

    /// Upstream `garbage_collect_id`: removes an identifier's WMEs (not a goal's impasse
    /// WMEs) and its preferences.
    fn garbage_collect_id(&mut self, id: SymbolId) {
        let impasse: Vec<WmeKey> = self.decider.goals.iter().flat_map(|g| g.impasse_wmes.iter().copied()).collect();
        let keys: Vec<WmeKey> = self.wm.iter().filter(|w| w.id == id && !impasse.contains(&w.key)).map(|w| w.key).collect();
        for key in keys {
            self.remove_wme_quietly(key);
        }
        self.remove_preferences(|p| p.id == id);
        // Upstream frees the symbol here; it no longer has a level.
        let i = self.decider.links.index(id);
        let r = &mut self.decider.links.records[i];
        r.level = LOWEST_POSSIBLE_GOAL_LEVEL;
        r.promotion_level = LOWEST_POSSIBLE_GOAL_LEVEL;
    }

    /// Upstream `mark_id_and_tc_as_unknown_level`.
    fn mark_id_and_tc_as_unknown_level(&mut self, root: SymbolId, tc: u64, start_level: usize, range: &mut (usize, usize)) {
        let mut stack = alloc::vec![root];
        while let Some(id) = stack.pop() {
            let links = &mut self.decider.links;
            let i = links.index(id);
            let r = &mut links.records[i];
            if r.tc == tc || r.level < start_level {
                continue;
            }
            r.tc = tc;
            range.0 = range.0.min(r.level);
            range.1 = range.1.max(r.level);
            if r.could_be_a_link_from_below {
                range.1 = LOWEST_POSSIBLE_GOAL_LEVEL;
            }
            links.mark_unknown(id);
            stack.extend(self.linked_ids(id));
        }
    }

    /// Upstream `walk_and_update_levels`.
    fn walk_and_update_levels(&mut self, root: SymbolId, walk_level: usize, tc: u64) {
        let mut stack = alloc::vec![root];
        while let Some(id) = stack.pop() {
            let links = &mut self.decider.links;
            let i = links.index(id);
            links.records[i].tc = tc;
            let r = &links.records[i];
            if !r.unknown_level && r.level < walk_level {
                continue;
            }
            if r.unknown_level {
                links.ids_with_unknown_level.retain(|x| *x != id);
                let r = &mut links.records[i];
                r.unknown_level = false;
                r.level = walk_level;
                r.promotion_level = walk_level;
            }
            for next in self.linked_ids(id) {
                let n = self.decider.links.index(next);
                if self.decider.links.records[n].tc != tc {
                    stack.push(next);
                }
            }
        }
    }

    /// Upstream `do_demotion`.
    fn do_demotion(&mut self) {
        let links = &mut self.decider.links;
        for id in core::mem::take(&mut links.ids_with_unknown_level) {
            let i = links.index(id);
            if links.records[i].link_count == 0 {
                links.disconnected_ids.push(id);
            } else {
                links.ids_with_unknown_level.push(id);
            }
        }

        self.decider.links.mode = LinkUpdateMode::UpdateDisconnectedIdsList;
        while let Some(id) = self.decider.links.disconnected_ids.pop() {
            let i = self.decider.links.index(id);
            self.decider.links.records[i].unknown_level = false;
            self.garbage_collect_id(id);
        }
        self.decider.links.mode = LinkUpdateMode::Normally;
        if self.decider.links.ids_with_unknown_level.is_empty() {
            return;
        }

        // Mark everything that might have fallen, then walk down from the goals that could
        // still reach it.
        let mut range = (LOWEST_POSSIBLE_GOAL_LEVEL, 0);
        let tc = self.decider.links.new_tc();
        for id in self.decider.links.ids_with_unknown_level.clone() {
            let start = self.decider.links.get(id).map_or(LOWEST_POSSIBLE_GOAL_LEVEL, |r| r.level);
            self.mark_id_and_tc_as_unknown_level(id, tc, start, &mut range);
        }
        for (index, goal) in self.decider.goals.iter().map(|g| g.id).enumerate().collect::<Vec<_>>() {
            let level = index + 1;
            if level > range.1 {
                break;
            }
            if level >= range.0 {
                let walk_tc = self.decider.links.new_tc();
                self.walk_and_update_levels(goal, level, walk_tc);
            }
        }

        self.decider.links.mode = LinkUpdateMode::JustUpdateCount;
        while let Some(id) = self.decider.links.ids_with_unknown_level.pop() {
            let i = self.decider.links.index(id);
            self.decider.links.records[i].unknown_level = false;
            self.garbage_collect_id(id);
        }
        self.decider.links.mode = LinkUpdateMode::Normally;
    }

    /// Upstream `do_buffered_link_changes`: promotions, then demotions and garbage
    /// collection.
    pub(super) fn do_buffered_link_changes(&mut self) {
        let links = &self.decider.links;
        if links.promoted_ids.is_empty() && links.ids_with_unknown_level.is_empty() && links.disconnected_ids.is_empty() {
            return;
        }
        while let Some(id) = self.decider.links.promoted_ids.pop() {
            let level = self.decider.links.get(id).map_or(LOWEST_POSSIBLE_GOAL_LEVEL, |r| r.promotion_level);
            self.promote_id_and_tc(id, level);
        }
        self.do_demotion();
    }

    /// Registers a goal at `level` with its special link.
    pub(super) fn link_goal(&mut self, goal: SymbolId, level: usize) {
        self.decider.links.set_new_id_level(goal, level);
        self.post_link_addition(None, goal);
        // Structure built before the goal existed (hand-built agents) joins its level.
        for value in self.linked_ids(goal) {
            let i = self.decider.links.index(value);
            if self.decider.links.records[i].promotion_level > level {
                self.decider.links.records[i].promotion_level = level;
                self.decider.links.promoted_ids.push(value);
            }
        }
    }

    /// Removes a goal's special link.
    pub(super) fn unlink_goal(&mut self, goal: SymbolId) {
        self.post_link_removal(None, goal);
    }

    /// Goal level (1 = top) of an identifier, if it is linked to the goal stack.
    pub fn identifier_level(&self, id: SymbolId) -> Option<usize> {
        self.decider.links.level(id)
    }
}
