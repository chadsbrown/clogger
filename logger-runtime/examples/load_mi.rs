use logger_runtime::CallHistoryDb;
use logger_core::CallHistoryLookup;

fn main() {
    let db = CallHistoryDb::load(std::path::Path::new("/home/cbrown/src/clogger/QSOP_MI-2025-001.txt")).unwrap();
    for call in &["NA8V", "AA8CQ", "K8ZT", "W8MJ", "N9UNX", "XXXNOEXIST"] {
        match db.lookup(call) {
            Some(pairs) => println!("{}: {:?}", call, pairs),
            None => println!("{}: NOT FOUND", call),
        }
    }
}
