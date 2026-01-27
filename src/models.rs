use chrono::{DateTime, Utc};
use csv_db::Database;
use rocket::{
    Request, State,
    http::Status,
    request::{FromRequest, Outcome},
    tokio::sync::Mutex,
};
use serde::{Deserialize, Serialize};

#[derive(FromForm)]
pub struct Registration {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Default, Deserialize, FromForm, PartialEq, Serialize)]
pub struct Bet {
    pub race: String,
    pub username: String,
    pub p1: String,
    pub p2: String,
    pub p3: String,
    pub p4: String,
    pub p5: String,
}

#[derive(Serialize)]
pub struct ScoredBet<'a> {
    pub bet: &'a Bet,
    pub points: u16,
}

#[derive(Deserialize, Serialize)]
pub struct User {
    pub token: String,
    pub username: String,
    pub password: String,
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for User {
    type Error = &'static str;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        let db = match req.guard::<&State<Mutex<Database<&str>>>>().await {
            Outcome::Success(db) => db,
            _ => {
                return Outcome::Error((Status::InternalServerError, "Could not access database"));
            }
        };
        let cookies = req.cookies();

        match cookies.get_private("session") {
            Some(token) => match get_user(token.value(), db).await {
                Some(user) => Outcome::Success(user),
                None => Outcome::Forward(Status::Unauthorized),
            },
            None => Outcome::Forward(Status::Unauthorized),
        }
    }
}

async fn get_user(token: &str, db: &State<Mutex<Database<&str>>>) -> Option<User> {
    db.lock()
        .await
        .find("users", |u: &User| u.token == token)
        .await
        .ok()?
        .into_iter()
        .next()
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
