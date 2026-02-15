use logger_core::QsoDraft;

#[derive(Debug, Default)]
pub struct FakeLog {
    pub next_id: u64,
    pub rows: Vec<(u64, QsoDraft)>,
}

impl FakeLog {
    pub fn insert(&mut self, draft: QsoDraft) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.rows.push((id, draft));
        id
    }
}
