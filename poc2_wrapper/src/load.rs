use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;
use unstructured::Document;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct ObjectMeta {
    pub name: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct ImportSpec {
    #[serde(rename = "sourceType")]
    pub source_type: String,
    pub name: String,
    pub select: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct ComponentSpec {
    #[serde(rename = "type")]
    pub ctype: Option<String>,
    pub config: Option<Document>,
    pub imports: Option<BTreeMap<String, ImportSpec>>,
    pub exports: Option<BTreeMap<String, Document>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct Component {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: ObjectMeta,
    pub spec: ComponentSpec,
}

pub trait ComponentFileChangeCallback {
    fn created(&mut self, comp: Component) -> Result<(), String>;

    fn updated(&mut self, old: Component, new: Component) -> Result<(), String>;

    fn deleted(&mut self, comp: Component) -> Result<(), String>;
}

pub struct ComponentsDirectory<'a> {
    input_dir: String,
    loaded_components: BTreeMap<String, Component>,
    callback: &'a mut dyn ComponentFileChangeCallback,
}

impl ComponentsDirectory<'_> {
    pub fn new<'a>(
        input_dir: &str,
        cb: &'a mut dyn ComponentFileChangeCallback,
    ) -> ComponentsDirectory<'a> {
        ComponentsDirectory {
            input_dir: input_dir.to_string(),
            loaded_components: BTreeMap::new(),
            callback: cb,
        }
    }

    pub fn watch_input_dir(&mut self) {
        // Create a channel to receive the events.
        let (tx, rx) = channel();
        // Create a watcher object, delivering raw events.
        // The notification back-end is selected based on the platform.
        let mut watcher = watcher(tx, Duration::from_millis(250)).unwrap();
        // Add a path to be watched. All files and directories at that path
        // will be monitored for changes.
        watcher
            .watch(&self.input_dir, RecursiveMode::NonRecursive)
            .unwrap();
        self.rescan_input_dir();
        loop {
            let res = match rx.recv() {
                Ok(DebouncedEvent::Create(path)) => {
                    if path.ends_with("stop") {
                        break;
                    }
                    println!("create {:?}", path);
                    self.changed(&path.as_path())
                }
                Ok(DebouncedEvent::Write(path)) => {
                    println!("write {:?}", path);
                    self.changed(path.as_path())
                }
                Ok(DebouncedEvent::Remove(path)) => {
                    println!("remove {:?}", path);
                    self.removed(path.as_path())
                }
                Ok(DebouncedEvent::Rename(old_path, new_path)) => {
                    println!("rename {:?} {:?}", old_path, new_path);
                    self.renamed(old_path.as_path(), new_path.as_path())
                }
                Ok(DebouncedEvent::NoticeWrite(_)) => Ok(()),
                Ok(DebouncedEvent::NoticeRemove(_)) => Ok(()),
                Ok(DebouncedEvent::Chmod(_)) => Ok(()),
                Ok(DebouncedEvent::Rescan) => {
                    self.rescan_input_dir();
                    Ok(())
                }
                Ok(DebouncedEvent::Error(err, opt)) => Err(format!("{:?} {:?}", err, opt)),
                Err(e) => Err(e.to_string()),
            };
            print_error("watch", res);
        }
    }

    fn changed(&mut self, path: &Path) -> Result<(), String> {
        match self.load_component(path) {
            Some((None, Ok(comp))) => self.callback.created(comp),
            Some((Some(old), Ok(comp))) => self.callback.updated(old, comp),
            Some((Some(old), Err(e))) => {
                result_combine("deleted", self.callback.deleted(old), "load", Err(e))
            }
            Some((None, Err(e))) => Err(e),
            None => Ok(()),
        }
    }

    fn removed(&mut self, path: &Path) -> Result<(), String> {
        match self.unload_component(path) {
            Some(comp) => self.callback.deleted(comp),
            None => Ok(()),
        }
    }

    fn renamed(&mut self, old_path: &Path, new_path: &Path) -> Result<(), String> {
        match self.unload_component(old_path) {
            Some(old_comp) => match self.load_component(new_path) {
                Some((None, Ok(comp))) => self.callback.updated(old_comp, comp),
                Some((Some(obsolete_comp), Ok(comp))) => {
                    let res1 = self.callback.deleted(obsolete_comp);
                    let res2 = self.callback.updated(old_comp, comp);
                    result_combine("deleted", res1, "updated", res2)
                }
                Some((Some(obsolete_comp), Err(e))) => {
                    let res1 = self.callback.deleted(old_comp);
                    let res2 = self.callback.deleted(obsolete_comp);
                    result_combine3("deleted", res1, "deleted", res2, "load", Err(e))
                }
                Some((None, Err(e))) => {
                    let res1 = self.callback.deleted(old_comp);
                    result_combine("deleted", res1, "load", Err(e))
                }
                None => self.callback.deleted(old_comp),
            },
            None => self.changed(new_path),
        }
    }

    fn rescan_input_dir(&mut self) {
        let dir = Path::new(&self.input_dir);
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_file() {
                print_error("rescan", self.changed(&path));
            }
        }
    }

    fn load_component(
        &mut self,
        path: &Path,
    ) -> Option<(Option<Component>, Result<Component, String>)> {
        if is_yaml(path) {
            let key = path.file_name().unwrap().to_string_lossy().to_string();
            let result = to_res(fs::read_to_string(path))
                .and_then(|s| to_res(serde_yaml::from_str::<Component>(&s)));
            let old = match &result {
                Ok(comp) => self.loaded_components.insert(key, comp.clone()),
                _ => self.loaded_components.remove(&key),
            };
            Some((old, result))
        } else {
            None
        }
    }

    fn unload_component(&mut self, path: &Path) -> Option<Component> {
        if is_yaml(path) {
            let key = path.file_name().unwrap().to_string_lossy().to_string();
            self.loaded_components.remove(&key)
        } else {
            None
        }
    }
}

fn to_res<U, T: ToString>(res: Result<U, T>) -> Result<U, String> {
    res.map_err(|e| e.to_string())
}

fn is_yaml(path: &Path) -> bool {
    if path.is_dir() {
        return false;
    }
    match path.extension() {
        Some(s) => s.to_str().map(|s| s == "yaml").unwrap_or_default(),
        None => false,
    }
}

fn print_error(label: &str, result: Result<(), String>) {
    match result {
        Ok(()) => (),
        Err(e) => println!("{:?} error: {:?}", label, e),
    };
}

pub fn result_combine(
    label1: &str,
    result1: Result<(), String>,
    label2: &str,
    result2: Result<(), String>,
) -> Result<(), String> {
    match &result1 {
        Ok(()) => result2,
        Err(e1) => match result2 {
            Ok(()) => result1,
            Err(e2) => Err(format!("{:?}: {:?}, {:?}: {:?}", label1, e1, label2, e2)),
        },
    }
}

fn result_combine3(
    label1: &str,
    result1: Result<(), String>,
    label2: &str,
    result2: Result<(), String>,
    label3: &str,
    result3: Result<(), String>,
) -> Result<(), String> {
    match &result1 {
        Ok(()) => result_combine(label2, result2, label3, result3),
        Err(e1) => match &result2 {
            Ok(()) => result_combine(label1, result1, label3, result3),
            Err(e2) => match &result3 {
                Ok(()) => result_combine(label1, result1, label2, result2),
                Err(e3) => Err(format!(
                    "{:?}: {:?}, {:?}: {:?}, {:?}: {:?}",
                    label1, e1, label2, e2, label3, e3
                )),
            },
        },
    }
}
