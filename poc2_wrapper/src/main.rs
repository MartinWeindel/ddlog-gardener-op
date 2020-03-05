use differential_datalog::record::*;
use differential_datalog::DDlog;
use poc2_ddlog::api::HDDlog;

fn run(mut hddlog: HDDlog) -> Result<(), String> {
    hddlog.stop()
}

fn main() -> Result<(), String> {
    println!("Hello, world!");

    fn record_upd(table: usize, rec: &Record, w: isize) {
        eprintln!(
            "{}({:+}) {:?} {}",
            if w >= 0 { "insert" } else { "delete" },
            w,
            table,
            *rec
        );
    }

    let workers = 4;
    let store = true;
    match HDDlog::run(workers, store, record_upd) {
        Ok(hddlog) => run(hddlog),
        Err(err) => Err(format!("Failed to run differential datalog: {}", err)),
    }
}
