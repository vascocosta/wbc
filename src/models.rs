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

#[derive(Clone, Debug, Deserialize, Serialize)]
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

        match cookies.get("session") {
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
        .first()
        .map(|u| u.to_owned())
}
