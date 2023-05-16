use rocket::fs::FileServer;
use rocket::State;
use rocket::{http::Status, serde::json::Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::{thread, time};
use thiserror::Error;

#[macro_use]
extern crate rocket;

struct Clipboard {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

impl Clipboard {
    fn init() -> Clipboard {
        Clipboard {
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn add(&self, entry: Entry) -> Result<(), Error> {
        let ret = self.entries.lock().unwrap().insert(entry.id.clone(), entry);
        match ret {
            Some(_) => Err(Error::DuplicateEntry),
            None => Ok(()),
        }
    }

    fn get(&self, id: &str) -> Option<Entry> {
        self.entries.lock().unwrap().get(id).cloned()
    }
}

#[derive(Error, Debug, Serialize)]
enum Error {
    #[error("key len {key_len} != data len {data_len}")]
    PadDiffLength { key_len: usize, data_len: usize },
    #[error("entry with same id already exists")]
    DuplicateEntry,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Entry {
    id: String,
    content: String,
    encrypted: bool,
    key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct DecryptRequest {
    key: String,
}

fn pad(key: &str, data: &str) -> Result<String, Error> {
    use Error::*;
    if key.len() != data.len() {
        return Err(PadDiffLength {
            key_len: key.len(),
            data_len: data.len(),
        });
    }

    let data_chars: Vec<char> = data.chars().collect();
    let key_chars: Vec<char> = key.chars().collect();

    let mut out = String::new();
    for i in 0..key_chars.len() {
        let out_char = char::from_u32(u32::from(data_chars[i]) ^ u32::from(key_chars[i]));
        out.push(out_char.unwrap());
    }
    Ok(out)
}

fn not_so_constant_time_strcmp(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();

    for i in 0..a.len() {
        thread::sleep(time::Duration::from_millis(100));
        if a[i] != b[i] {
            return false;
        }
    }
    true
}

#[get("/get?<id>")]
fn get_entry(id: String, data: &State<Clipboard>) -> Result<Json<Entry>, Status> {
    let entry = data.get(&id);

    if let Some(entry) = entry {
        Ok(Json(Entry {
            id: entry.id,
            content: entry.content,
            encrypted: entry.encrypted,
            key: None,
        }))
    } else {
        Err(Status::NotFound)
    }
}

#[post("/add", data = "<entry>")]
fn add_entry(mut entry: Json<Entry>, data: &State<Clipboard>) -> Status {
    if entry.encrypted {
        if let Some(key) = &entry.key {
            if let Ok(ct) = pad(key, &entry.content) {
                entry.content = ct;
            } else {
                return Status::InternalServerError;
            }
        }
    }
    let res = data.add(entry.into_inner());

    if res.is_ok() {
        Status::Ok
    } else {
        Status::InternalServerError
    }
}

#[post("/decrypt?<id>", data = "<request>")]
fn decrypt(
    id: String,
    request: Json<DecryptRequest>,
    data: &State<Clipboard>,
) -> Result<String, Status> {
    if let Some(entry) = data.get(&id) {
        let key = match &entry.key {
            Some(key) => key,
            None => return Err(Status::InternalServerError),
        };
        if not_so_constant_time_strcmp(&request.key, key) {
            if let Ok(pt) = pad(&request.key, &entry.content) {
                Ok(pt)
            } else {
                Err(Status::InternalServerError)
            }
        } else {
            Err(Status::InternalServerError)
        }
    } else {
        Err(Status::NotFound)
    }
}

#[launch]
fn rocket() -> _ {
    let clipboard = Clipboard::init();
    rocket::build()
        .manage(clipboard)
        .mount("/", FileServer::from("static"))
        .mount("/api", routes![get_entry, add_entry, decrypt])
}

#[cfg(test)]
mod tests {
    use crate::pad;
    #[test]
    fn one_time_pad() {
        let pt = String::from("0123456789abcdef");
        let key = String::from("supersecreptkey!");
        let ct = pad(&key, &pt).unwrap();
        assert_eq!(pt, pad(&key, &ct).unwrap());
    }
}
