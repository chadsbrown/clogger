use std::collections::HashMap;

use logger_core::RadioState;

#[derive(Debug, Default)]
pub struct FakeRig {
    pub states: HashMap<u8, RadioState>,
}

impl FakeRig {
    pub fn set(&mut self, radio: u8, state: RadioState) {
        self.states.insert(radio, state);
    }
}
