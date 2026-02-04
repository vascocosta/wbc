use chrono::{DateTime, Utc};
use csv_db::Database;
use rocket::{
    Request, State,
    http::Status,
    request::{FromRequest, Outcome},
    tokio::sync::Mutex,
};
use serde::{Deserialize, Serialize};

use crate::store::Store;

#[derive(FromForm)]
pub struct Registration {
    pub username: String,
    pub password: String,
}

#[derive(FromForm)]
pub struct Profile {
    pub country: String,
    pub password: String,
}

#[derive(Clone, Deserialize, FromForm, PartialEq, Serialize)]
pub struct Guess {
    pub race: String,
    pub username: String,
    pub p1: String,
    pub p2: String,
    pub p3: String,
    pub p4: String,
    pub p5: String,
}

impl Default for Guess {
    fn default() -> Self {
        Self {
            race: "".to_string(),
            username: "".to_string(),
            p1: "NOR".to_string(),
            p2: "VER".to_string(),
            p3: "PIA".to_string(),
            p4: "RUS".to_string(),
            p5: "LEC".to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct ScoredGuess<'a> {
    pub guess: &'a Guess,
    pub points: u16,
}

#[derive(Default, Deserialize, PartialEq, Serialize)]
pub struct User {
    pub token: String,
    pub username: String,
    pub password: String,
    pub country: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for User {
    type Error = &'static str;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let cookies = req.cookies();
        let db = match req.guard::<&State<Mutex<Database<&str>>>>().await {
            Outcome::Success(db) => db,
            _ => {
                return Outcome::Error((Status::InternalServerError, "Could not access database."));
            }
        };

        match cookies.get_private("session") {
            Some(token) => match Store::get_user(token.value(), db).await {
                Some(user) => Outcome::Success(user),
                None => Outcome::Forward(Status::Unauthorized),
            },
            None => Outcome::Forward(Status::Unauthorized),
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct Driver {
    pub number: u8,
    pub code: String,
    pub name: String,
}

#[derive(Deserialize, Serialize)]
pub struct Event {
    pub category: String,
    pub name: String,
    pub description: String,
    pub datetime: DateTime<Utc>,
    pub channel: String,
    tags: String,
    notify: bool,
}

#[derive(Deserialize, Serialize)]
pub struct RaceResult {
    pub race: String,
    pub p1: String,
    pub p2: String,
    pub p3: String,
    pub p4: String,
    pub p5: String,
}
