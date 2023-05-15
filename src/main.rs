use crate::rocket::futures::TryFutureExt;
use rocket::{
    fs::{relative, FileServer},
    http::Status,
    serde::json::Json,
};
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

#[derive(Error, Debug)]
enum PadError {
    #[error("key len {key_len} != data len {data_len}")]
    DifferentLength { key_len: usize, data_len: usize },
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

fn pad(key: &str, data: &str) -> Result<String, PadError> {
    if key.len() != data.len() {
        return Err(PadError::DifferentLength {
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
async fn get_entry(mut db: Connection<Db>, id: String) -> Result<Json<Entry>, Status> {
    let entry = sqlx::query_as!(
        Entry,
        "SELECT id, content, password FROM entries WHERE id = ?",
        id
    )
    .fetch_one(&mut *db)
    .map_ok(|r| {
        Json(Entry {
            id: r.id,
            content: r.content,
            password: None,
        })
    })
    .await
    .ok();

    if let Some(entry) = entry {
        Ok(entry)
    } else {
        Err(Status::NotFound)
    }
}

#[post("/add", data = "<entry>")]
async fn add_entry(mut db: Connection<Db>, mut entry: Json<Entry>) -> Status {
    if let Some(password) = &entry.password {
        entry.content = pad(password, &entry.content).unwrap();
    }
    sqlx::query!(
        "INSERT INTO entries (id, content, password) VALUES (?, ?, ?)",
        entry.id,
        entry.content,
        entry.password
    )
    .execute(&mut *db)
    .await
    .ok();
    Status::Ok
}

#[post("/decrypt?<id>", data = "<request>")]
async fn decrypt(
    mut db: Connection<Db>,
    id: String,
    request: Json<DecryptRequest>,
) -> Result<String, Status> {
    let entry = sqlx::query_as!(
        Entry,
        "SELECT id, content, password FROM entries WHERE id = ?",
        id
    )
    .fetch_one(&mut *db)
    .map_ok(|r| {
        Json(Entry {
            id: r.id,
            content: r.content,
            password: r.password,
        })
    })
    .await
    .ok();

    if let Some(entry) = entry {
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
        .mount("/api", routes![get_entry, add_entry, decrypt])
        .mount("/", FileServer::from(relative!("static")))
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
