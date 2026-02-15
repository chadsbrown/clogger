#[derive(Debug, Default)]
pub struct FakeKeyer {
    pub sent: Vec<(u8, String)>,
}

impl FakeKeyer {
    pub fn send(&mut self, radio: u8, text: String) {
        self.sent.push((radio, text));
    }

    pub fn joined_text(&self) -> String {
        self.sent
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
