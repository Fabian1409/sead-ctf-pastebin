use rocket::fs::{relative, FileServer};
use rocket::{http::Status, serde::json::Json};
use std::{thread, time};
use thiserror::Error;

use rocket_db_pools::sqlx;
use rocket_db_pools::{Connection, Database};
use serde::{Deserialize, Serialize};

#[macro_use]
extern crate rocket;

#[derive(Database)]
#[database("clipboard")]
struct Db(sqlx::SqlitePool);

impl Db {
    async fn get_entry(mut db: Connection<Db>, id: String) -> Option<Entry> {
        sqlx::query_as!(
            Entry,
            "SELECT id, content, password FROM entries WHERE id = ?",
            id
        )
        .fetch_one(&mut *db)
        .await
        .ok()
    }

    async fn add_entry(mut db: Connection<Db>, entry: Entry) -> Result<(), Error> {
        let res = sqlx::query!(
            "INSERT INTO entries (id, content, password) VALUES (?, ?, ?)",
            entry.id,
            entry.content,
            entry.password
        )
        .execute(&mut *db)
        .await;

        if res.is_ok() {
            Ok(())
        } else {
            Err(Error::AddEntryError)
        }
    }
}

#[derive(Error, Debug, Serialize)]
enum Error {
    #[error("key len {key_len} != data len {data_len}")]
    PadDiffLength { key_len: usize, data_len: usize },
    #[error("failed to add entry")]
    AddEntryError,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Entry {
    id: String,
    content: String,
    password: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct DecryptRequest {
    password: String,
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
async fn get_entry(db: Connection<Db>, id: String) -> Result<Json<Entry>, Status> {
    let entry = Db::get_entry(db, id);

    if let Some(entry) = entry.await {
        Ok(Json(Entry {
            id: entry.id,
            content: entry.content,
            password: None,
        }))
    } else {
        Err(Status::NotFound)
    }
}

#[post("/add", data = "<entry>")]
async fn add_entry(db: Connection<Db>, mut entry: Json<Entry>) -> Status {
    if let Some(password) = &entry.password {
        if let Ok(ct) = pad(password, &entry.content) {
            entry.content = ct;
        } else {
            return Status::InternalServerError;
        }
    }
    let res = Db::add_entry(db, entry.into_inner()).await;

    if res.is_ok() {
        Status::Ok
    } else {
        Status::InternalServerError
    }
}

#[post("/decrypt?<id>", data = "<request>")]
async fn decrypt(
    db: Connection<Db>,
    id: String,
    request: Json<DecryptRequest>,
) -> Result<String, Status> {
    if let Some(entry) = Db::get_entry(db, id).await {
        let password = match &entry.password {
            Some(password) => password,
            None => return Err(Status::InternalServerError),
        };
        if not_so_constant_time_strcmp(&request.password, password) {
            if let Ok(pt) = pad(&request.password, &entry.content) {
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
    rocket::build()
        .attach(Db::init())
        .mount("/", FileServer::from(relative!("static")))
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
