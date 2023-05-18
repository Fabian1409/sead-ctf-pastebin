use rocket::fs::FileServer;
use rocket::response::Responder;
use rocket::serde::json::Json;
use std::thread;
use std::time::{Duration, Instant};
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
    async fn get_entry(mut db: Connection<Db>, id: &str) -> Option<Entry> {
        sqlx::query_as!(
            Entry,
            "SELECT id, content, encrypted, key FROM entries WHERE id = ?",
            id
        )
        .fetch_one(&mut *db)
        .await
        .ok()
    }

    async fn add_entry(mut db: Connection<Db>, entry: Entry) -> Result<(), Error> {
        let res = sqlx::query!(
            "INSERT INTO entries (id, content, encrypted, key) VALUES (?, ?, ?, ?)",
            entry.id,
            entry.content,
            entry.encrypted,
            entry.key
        )
        .execute(&mut *db)
        .await;

        if res.is_ok() {
            Ok(())
        } else {
            Err(Error::EntryAlreadyExists)
        }
    }
}

#[derive(Responder)]
#[response(status = 500, content_type = "json")]
struct ErrorResponse {
    error: Json<Error>,
}

#[derive(Error, Debug, Serialize)]
enum Error {
    #[error("key len {key_len} != data len {data_len}")]
    InvalidKeyLen { key_len: usize, data_len: usize },
    #[error("entry already exists")]
    EntryAlreadyExists,
    #[error("invaild key, took {took} ms")]
    InvalidKey { took: u128 },
    #[error("no entry with {0} exits")]
    EntryNotFound(String),
    #[error("entry with {0} is not encrypted")]
    EntryNotEncrypted(String),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct Entry {
    id: String,
    content: String,
    encrypted: i64,
    key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct DecryptRequest {
    key: String,
}

fn pad(key: &[u8], data: &[u8]) -> Result<Vec<u8>, Error> {
    use Error::*;
    if key.len() != data.len() {
        return Err(InvalidKeyLen {
            key_len: key.len(),
            data_len: data.len(),
        });
    }

    let mut out = Vec::new();
    for i in 0..key.len() {
        out.push(data[i] ^ key[i]);
    }
    Ok(out)
}

fn not_so_constant_time_strcmp(a: &str, b: &str) -> Result<(), Error> {
    let start = Instant::now();
    if a.len() != b.len() {
        return Err(Error::InvalidKeyLen {
            key_len: a.len(),
            data_len: b.len(),
        });
    }

    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();

    for i in 0..a.len() {
        thread::sleep(Duration::from_millis(10));
        if a[i] != b[i] {
            return Err(Error::InvalidKey {
                took: start.elapsed().as_millis(),
            });
        }
    }
    Ok(())
}

#[get("/get?<id>")]
async fn get_entry(db: Connection<Db>, id: String) -> Result<Json<Entry>, ErrorResponse> {
    let entry = Db::get_entry(db, &id);
    if let Some(entry) = entry.await {
        Ok(Json(Entry {
            id: entry.id,
            content: entry.content,
            encrypted: entry.encrypted,
            key: None,
        }))
    } else {
        Err(ErrorResponse {
            error: Json(Error::EntryNotFound(id)),
        })
    }
}

#[post("/add", data = "<entry>")]
async fn add_entry(db: Connection<Db>, entry: Json<Entry>) -> Result<(), ErrorResponse> {
    Db::add_entry(db, entry.into_inner())
        .await
        .map_err(|err| ErrorResponse { error: Json(err) })
}

#[post("/decrypt?<id>", data = "<request>")]
async fn decrypt(
    db: Connection<Db>,
    id: String,
    request: Json<DecryptRequest>,
) -> Result<String, ErrorResponse> {
    if let Some(entry) = Db::get_entry(db, &id).await {
        let key = &entry.key.ok_or(ErrorResponse {
            error: Json(Error::EntryNotEncrypted(id)),
        })?;
        not_so_constant_time_strcmp(&request.key, key)
            .map_err(|err| ErrorResponse { error: Json(err) })?;
        let key = hex::decode(&request.key).unwrap();
        let data = hex::decode(entry.content).unwrap();
        let pt = pad(&key, &data).map_err(|err| ErrorResponse { error: Json(err) })?;
        Ok(hex::encode(pt))
    } else {
        Err(ErrorResponse {
            error: Json(Error::EntryNotFound(id)),
        })
    }
}

#[launch]
fn rocket() -> _ {
    rocket::build()
        .attach(Db::init())
        .mount("/", FileServer::from("/opt/app/static"))
        .mount("/api", routes![get_entry, add_entry, decrypt])
}

#[cfg(test)]
mod tests {
    use crate::pad;
    #[test]
    fn one_time_pad() {
        let pt = String::from("0123456789abcdef").into_bytes();
        let key = String::from("supersecreptkey!").into_bytes();
        let ct = pad(&key, &pt).unwrap();
        assert_eq!(pt, pad(&key, &ct).unwrap());
    }
}
