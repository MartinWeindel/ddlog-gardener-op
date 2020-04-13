mod load;

use differential_datalog::record::*;
use differential_datalog::DDlog;
use load::Component;
use poc2_ddlog::api::HDDlog;
use types;
use unstructured::Document;
use value::Relations;

struct CB {
    hddlog: HDDlog,
}

impl CB {
    fn build_config(&mut self, comp: &Component) -> types::Config {
        let config = match &comp.spec.config {
            Some(config) => match config {
                d @ Document::Map(_) => Ok(types::document_Document { x: d.clone() }),
                Document::Unit => Ok(types::document_doc_unit()),
                _ => Err(format!("invalid value of field spec.config")),
            },
            None => Ok(types::document_doc_unit()),
        };
        types::Config {
            cname: comp.metadata.name.clone(),
            spec: types::res2std(config),
        }
    }

    fn build_component(&mut self, comp: &Component) -> types::Component {
        let config = match &comp.spec.config {
            Some(config) => match config {
                d @ Document::Map(_) => Ok(types::document_Document { x: d.clone() }),
                Document::Unit => Ok(types::document_doc_unit()),
                _ => Err(format!("invalid value of field spec.config")),
            },
            None => Ok(types::document_doc_unit()),
        };
        types::Component {
            cname: comp.metadata.name.clone(),
            typeName: comp
                .spec
                .ctype
                .as_ref()
                .unwrap_or(&String::from(""))
                .clone(),
            spec: types::res2std(config),
        }
    }

    fn apply_update1(&mut self, cmd: UpdCmd) -> Result<(), String> {
        let mut upds: Vec<UpdCmd> = Vec::new();
        upds.push(cmd);
        self.apply_updates(upds)
    }

    fn apply_update2(&mut self, cmd1: UpdCmd, cmd2: UpdCmd) -> Result<(), String> {
        let mut upds: Vec<UpdCmd> = Vec::new();
        upds.push(cmd1);
        upds.push(cmd2);
        self.apply_updates(upds)
    }

    fn apply_updates(&mut self, upds: Vec<UpdCmd>) -> Result<(), String> {
        self.hddlog.transaction_start()?;
        match self.hddlog.apply_updates(upds.iter()) {
            Ok(()) => self.hddlog.transaction_commit(),
            err @ Err(_) => load::result_combine(
                "rollback",
                self.hddlog.transaction_rollback(),
                "apply_updates",
                err,
            ),
        }
    }

    fn insert_record(&mut self, rel: Relations, record: Record) -> Result<(), String> {
        self.apply_update1(UpdCmd::Insert(RelIdentifier::RelId(rel as usize), record))
    }

    fn update_record(
        &mut self,
        rel_old: Relations,
        record_old: Record,
        rel_new: Relations,
        record_new: Record,
    ) -> Result<(), String> {
        self.apply_update2(
            UpdCmd::Delete(RelIdentifier::RelId(rel_old as usize), record_old),
            UpdCmd::Insert(RelIdentifier::RelId(rel_new as usize), record_new),
        )
    }

    fn make_record(&mut self, comp: Component) -> Result<(Relations, Record), String> {
        match comp.kind.as_str() {
            "Config" => {
                let c = self.build_config(&comp);
                Ok((Relations::Config, c.into_record()))
            }
            "Component" => {
                let c = self.build_component(&comp);
                Ok((Relations::Component, c.into_record()))
            }
            s => Err(format!("unexpected kind: {:?}", s)),
        }
    }
}

impl load::ComponentFileChangeCallback for CB {
    fn created(&mut self, comp: Component) -> Result<(), String> {
        self.make_record(comp)
            .and_then(|t| self.insert_record(t.0, t.1))
    }

    fn updated(&mut self, old: Component, new: Component) -> Result<(), String> {
        let old_record = self.make_record(old);
        let new_record = self.make_record(new);
        match (old_record, new_record) {
            (Ok(ot), Ok(nt)) => self.update_record(ot.0, ot.1, nt.0, nt.1),
            (Ok(_), Err(e)) => Err(e),
            (Err(e), Ok(_)) => Err(e),
            (Err(e1), Err(e2)) => load::result_combine("old", Err(e1), "new", Err(e2)),
        }
    }

    fn deleted(&mut self, comp: Component) -> Result<(), String> {
        let record = self.make_record(comp);
        match record {
            Ok(t) => self.apply_update1(UpdCmd::Delete(RelIdentifier::RelId(t.0 as usize), t.1)),
            Err(e) => Err(e),
        }
    }
}

fn run(hddlog: HDDlog) -> Result<(), String> {
    let mut cb = CB { hddlog: hddlog };
    let mut cd = load::ComponentsDirectory::new("./input", &mut cb);
    cd.watch_input_dir();
    cb.hddlog.stop()
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
