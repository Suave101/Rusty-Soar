//! Episodic Memory (EpMem) temporal autobiographical memory store for `rusty-soar`.

use crate::symbol::SymbolId;
use crate::wm::WmeArena;
use alloc::vec::Vec;

/// Identifier handle for a discrete temporal episode in EpMem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EpisodeId(pub usize);

/// Snapshot of Working Memory captured at a specific time step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    /// Unique temporal episode handle.
    pub id: EpisodeId,
    /// Decision cycle time step when this episode was recorded.
    pub time_step: usize,
    /// Working Memory triples active during this episode.
    pub wmes: Vec<(SymbolId, SymbolId, SymbolId)>,
}

/// Long-Term Autobiographical Episodic Memory store.
#[derive(Debug, Default)]
pub struct EpisodicMemory {
    episodes: Vec<Episode>,
}

impl EpisodicMemory {
    /// Creates a new empty `EpisodicMemory` store.
    pub fn new() -> Self {
        Self {
            episodes: Vec::new(),
        }
    }

    /// Records a new temporal episode snapshot from the current Working Memory state.
    pub fn record_episode(&mut self, time_step: usize, wm: &WmeArena) -> EpisodeId {
        let id = EpisodeId(self.episodes.len());
        let mut active_wmes = Vec::new();

        // Capture all active WME triples
        for wme in wm.iter() {
            active_wmes.push((wme.id, wme.attr, wme.val));
        }

        self.episodes.push(Episode {
            id,
            time_step,
            wmes: active_wmes,
        });

        id
    }

    /// Queries Episodic Memory for the episode matching the highest number of cue WMEs.
    pub fn query(&self, cue: &[(SymbolId, SymbolId, SymbolId)]) -> Option<EpisodeId> {
        let mut best_match: Option<(EpisodeId, usize)> = None;

        for episode in &self.episodes {
            let mut matches = 0;
            for cue_wme in cue {
                if episode.wmes.contains(cue_wme) {
                    matches += 1;
                }
            }

            if matches > 0 {
                match best_match {
                    Some((_, max_matches)) => {
                        if matches > max_matches {
                            best_match = Some((episode.id, matches));
                        }
                    }
                    None => {
                        best_match = Some((episode.id, matches));
                    }
                }
            }
        }

        best_match.map(|(id, _)| id)
    }

    /// Retrieves a recorded episode by its handle key.
    pub fn retrieve(&self, id: EpisodeId) -> Option<&Episode> {
        self.episodes.get(id.0)
    }

    /// Returns the total number of recorded temporal episodes.
    pub fn len(&self) -> usize {
        self.episodes.len()
    }

    /// Returns `true` if Episodic Memory contains no recorded episodes.
    pub fn is_empty(&self) -> bool {
        self.episodes.is_empty()
    }
}
