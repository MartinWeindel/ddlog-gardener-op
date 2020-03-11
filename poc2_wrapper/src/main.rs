use notify::{Watcher, RecursiveMode, watcher, DebouncedEvent};
use std::time::Duration;
use std::sync::mpsc::channel;
use std::path::Path;
use std::io;
use std::fs;
use differential_datalog::record::*;
use differential_datalog::DDlog;
use poc2_ddlog::api::HDDlog;
use unstructured::Document;
use types;

fn rescan_input_dir(hddlog: &HDDlog, input_dir: &str) -> io::Result<()> {
    let dir = Path::new(input_dir);
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                changed(hddlog, &path)
            }
        }
    }
    Ok(())
}

fn is_yaml(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match path.extension() {
        Some(s) => s.to_str().map(|s| s == "yaml").unwrap_or_default(),
        None => false
    }
}

fn expect_string_field_value(doc: &Document, key: &str, expected_val: &str) -> Result<(), String>  {
    extract_string_field(doc, key).
        and_then(|v| if v == expected_val {
            Ok(())
            } else {
            Err(format!("field {} has unexpected value {} != {}", key, v, expected_val))
        })
}


fn extract_string_field(doc: &Document, key: &str) -> Result<String, String>  {
    extract_opt_string_field(doc, key).
        and_then(|opt| match opt {
            Some(s) => Ok(s),
            None => Err(format!("field {} not found in doc", key))
        })
}

fn extract_opt_string_field(doc: &Document, key: &str) -> Result<Option<String>, String>  {
    match doc {
        Document::Map(map) => {
            match map.get(&Document::String(key.to_string())) {
                Some(Document::String(value)) => Ok(Some(value.clone())),
                Some(_) => Err(format!("value of field {} is no string", key)),
                None => Ok(None)
            }
        },
        _ => Err("invalid document".to_string())
    }
}

fn changed_config(_hddlog: &HDDlog, path: &Path, doc: &Document) -> Result<(),String> {
    let name = match doc.select(".metadata.name") {
        Ok(Document::String(name)) => name,
        Ok(_) => return Err(format!("value of field metadata.name is no string")),
        Err(e) => return Err(e.to_string()) 
    };
    let config_spec = match doc.select(".spec.config") {
        Ok(d @ Document::Map(_)) => Ok(types::document_Document{x:d.clone()}),
        Ok(Document::Unit) => Ok(types::document_doc_unit()),
        Ok(_) => Err(format!("invalid value of field spec.config")),
        Err(e) => Err(e.to_string()) 
    };
    let c = types::Config{cname: name.clone(), spec: types::res2std(config_spec)};
    println!("{:?}", c);
    Ok(())
}

fn changed_component(_hddlog: &HDDlog, path: &Path, doc: &Document) -> Result<(),String>  {
    let name = match doc.select(".metadata.name") {
        Ok(Document::String(name)) => name,
        Ok(_) => return Err(format!("value of field metadata.name is no string")),
        Err(e) => return Err(e.to_string()) 
    };
    let type_name = match doc.select(".spec.type") {
        Ok(Document::String(name)) => name,
        Ok(_) => return Err(format!("value of field spec.type is no string")),
        Err(e) => return Err(e.to_string()) 
    };
    let config_spec = match doc.select(".spec.config") {
        Ok(d @ Document::Map(_)) => Ok(types::document_Document{x:d.clone()}),
        Ok(Document::Unit) => Ok(types::document_doc_unit()),
        Ok(_) => Err(format!("invalid value of field spec.config")),
        Err(e) => Err(e.to_string()) 
    };
    let c = types::Component{cname: name.clone(), typeName: type_name.clone(), spec: types::res2std(config_spec)};
    println!("{:?}", c);
    Ok(())
}

fn changed(hddlog: &HDDlog, path: &Path) {
    if is_yaml(&path) {
        let result = fs::read_to_string(path).
            map_err(|e| e.to_string()).
            and_then(|s| serde_yaml::from_str::<Document>(&s).map_err(|e| e.to_string()));
        match result {
            Ok(doc) => {
                let kind = expect_string_field_value(&doc, "apiVersion", "poc").
                    and_then(|_| extract_string_field(&doc, "kind"));
                match kind {
                    Ok(s) if s == "Config" => changed_config(hddlog, path, &doc).unwrap(),
                    Ok(s) if s == "Component" => changed_component(hddlog, path, &doc).unwrap(),
                    Ok(s) => println!("***changed {:?} unexpected kind {:?}", path, s),
                    Err(e) => println!("***changed {:?} error {:?}", path, e) 
                }
            },
            Err(e) => {
                println!("***changed {:?} error {:?}", path, e) 
            }
        };
    }
}

fn removed(_hddlog: &HDDlog, path: &Path) {
    if is_yaml(&path) {
        println!("***removed {:?}", path)
    }
}

fn renamed(hddlog: &HDDlog, old_path: &Path, new_path: &Path) {
    if is_yaml(&old_path) {
        if !is_yaml(&new_path) {
            removed(hddlog, old_path)
        }
    } else if is_yaml(&new_path) {
        changed(hddlog, new_path)
    }
}

fn monitor_input_dir(hddlog: &HDDlog, input_dir: &str) {
    // Create a channel to receive the events.
    let (tx, rx) = channel();

    // Create a watcher object, delivering raw events.
    // The notification back-end is selected based on the platform.
    let mut watcher = watcher(tx, Duration::from_millis(250)).unwrap();

    // Add a path to be watched. All files and directories at that path 
    // will be monitored for changes.
    watcher.watch(input_dir, RecursiveMode::NonRecursive).unwrap();
    rescan_input_dir(hddlog, input_dir).unwrap();

    loop {
        match rx.recv() {
            Ok(DebouncedEvent::Create(path)) => {
                if path.ends_with("stop") {
                    break;
                }
                println!("create {:?}", path);
                changed(hddlog, &path.as_path())
            },
            Ok(DebouncedEvent::Write(path)) => {
                println!("write {:?}", path);
                changed(hddlog, path.as_path())
            },
            Ok(DebouncedEvent::Remove(path)) => {
                println!("remove {:?}", path);
                removed(hddlog, path.as_path())
            },
            Ok(DebouncedEvent::Rename(old_path, new_path)) => {
                println!("rename {:?} {:?}", old_path, new_path);
                renamed(hddlog, old_path.as_path(), new_path.as_path())
            },
            Ok(DebouncedEvent::NoticeWrite(_)) => (),
            Ok(DebouncedEvent::NoticeRemove(_)) => (),
            Ok(DebouncedEvent::Chmod(_)) => (),
            Ok(DebouncedEvent::Rescan) => {
                rescan_input_dir(hddlog, input_dir).unwrap()
            },
            Ok(DebouncedEvent::Error(err, opt)) => {
                println!("error {:?} {:?}", err, opt)
            },
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}

fn run(mut hddlog: HDDlog) -> Result<(), String> {
    monitor_input_dir(&hddlog, "./input");
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
