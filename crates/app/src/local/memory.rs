//! An in-memory [`Backend`] for tests.

use std::cell::RefCell;

use super::{Backend, PersistOp, Snapshot, apply_ops};

#[derive(Debug, Default)]
pub struct MemoryBackend {
    pub data: RefCell<Snapshot>,
}

impl MemoryBackend {
    pub fn apply(&self, ops: &[PersistOp]) {
        apply_ops(&mut self.data.borrow_mut(), ops);
    }
}

impl Backend for MemoryBackend {
    async fn load(&self) -> Result<Snapshot, String> {
        Ok(self.data.borrow().clone())
    }

    async fn write(&self, ops: Vec<PersistOp>) -> Result<(), String> {
        self.apply(&ops);
        Ok(())
    }
}
