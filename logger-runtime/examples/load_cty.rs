use logger_runtime::CtyDb;
use logger_runtime::dxfeed_entity::CtyEntityResolver;
use dxfeed::resolver::entity::EntityResolver;
use std::sync::Arc;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: cargo run --example load_cty -- <cty.dat>");
    let db = CtyDb::load(std::path::Path::new(&path)).expect("load cty.dat");
    let resolver = CtyEntityResolver::new(Arc::new(db));

    let probes = [
        "W1AW", "K5ZD", "VE3EJ", "JA1BK", "G3FXB", "DL1ABC", "EA3OR",
        "VK2GGC", "ZS6CCY", "LU7DW", "VP8SGK", "KH6/W1AW", "VE7/K1ABC",
        "ZZ9ZZZ",
    ];
    for call in probes {
        match resolver.resolve(call) {
            Some(info) => println!(
                "{:<10} {:?}  cq={:>2} itu={:>2} dxcc={}",
                call, info.continent, info.cq_zone, info.itu_zone, info.entity_name,
            ),
            None => println!("{:<10} NOT FOUND", call),
        }
    }
}
