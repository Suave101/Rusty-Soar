use crate::rete::ReteNetwork;
use crate::symbol::SymbolTable;
use crate::wm::{SupportType, WmeArena};

pub struct SoarAgent {
    pub symbols: SymbolTable,
    pub wm: WmeArena,
    pub rete: ReteNetwork,
}

impl SoarAgent {
    pub fn new() -> Self {
        Self {
            symbols: SymbolTable::new(),
            wm: WmeArena::new(),
            rete: ReteNetwork::new(),
        }
    }

    /// Synchronizes Working Memory insertions into the RETE network.
    pub fn insert_wme(
        &mut self,
        s: crate::symbol::SymbolId,
        a: crate::symbol::SymbolId,
        v: crate::symbol::SymbolId,
    ) -> crate::wm::WmeKey {
        let key = self.wm.insert(s, a, v, SupportType::ISupport);
        self.rete.add_wme(key, s, a, v);
        key
    }

    /// Synchronizes Working Memory retractions into the RETE network.
    pub fn remove_wme(&mut self, key: crate::wm::WmeKey) {
        self.wm.remove(key);
        self.rete.remove_wme(key);
    }
}