use argon2::{
    Argon2, PasswordHasher,
    password_hash::{PasswordHash, PasswordVerifier, SaltString, rand_core::OsRng},
};
use chrono::Utc;
use csv_db::{Database, DbError};
use itertools::Itertools;
use rocket::{State, tokio::sync::Mutex};
use uuid::Uuid;

use crate::models::{Bet, Driver, Event, User};

const CATEGORY: &str = "fr oceania";
const CHANNEL: &str = "#formula1";

pub struct UserStore<'a> {
    db: &'a State<Mutex<Database<&'static str>>>,
}

impl<'a> UserStore<'a> {
    pub fn new(db: &'a State<Mutex<Database<&'static str>>>) -> Self {
        Self { db }
    }

    pub async fn user_exists(&self, username: &str) -> Result<bool, DbError> {
        let users = self
            .db
            .lock()
            .await
            .find("users", |u: &User| {
                u.username.to_lowercase() == username.to_lowercase()
            })
            .await?;

        if users.is_empty() {
            Ok(false)
        } else {
            Ok(true)
        }
    }

    pub async fn add_user(&self, username: &str, password: &str) -> Result<(), DbError> {
        let user = User {
            token: Uuid::new_v4().to_string(),
            username: username.to_lowercase(),
            password: Self::hash_password(password)
                .await
                .map_err(|_| DbError::NoMatch)?,
        };

        self.db.lock().await.insert("users", user).await
    }

    pub async fn validate_user(&self, username: &str, password: &str) -> Option<String> {
        let users = self
            .db
            .lock()
            .await
            .find("users", |u: &User| {
                u.username.to_lowercase() == username.to_lowercase()
            })
            .await
            .ok()?;

        if let Some(user) = users.first() {
            let parsed_hash = PasswordHash::new(&user.password).unwrap();

            if Argon2::default()
                .verify_password(password.as_bytes(), &parsed_hash)
                .is_ok()
            {
                Some(user.token.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    async fn hash_password(password: &str) -> Result<String, &'static str> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        Ok(argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| "Could not hash password")?
            .to_string())
    }
}

pub struct DriverStore<'a> {
    db: &'a State<Mutex<Database<&'static str>>>,
}

impl<'a> DriverStore<'a> {
    pub fn new(db: &'a State<Mutex<Database<&'static str>>>) -> Self {
        Self { db }
    }

    pub async fn all_drivers(&self) -> Result<Vec<Driver>, DbError> {
        self.db.lock().await.find("drivers", |_| true).await
    }
}

pub struct BetStore<'a> {
    db: &'a State<Mutex<Database<&'static str>>>,
}

impl<'a> BetStore<'a> {
    pub fn new(db: &'a State<Mutex<Database<&'static str>>>) -> Self {
        Self { db }
    }

    pub async fn get_bet(&self, username: &str, race: Option<&str>) -> Result<Vec<Bet>, DbError> {
        self.db
            .lock()
            .await
            .find("bets", |b: &Bet| {
                b.username.to_lowercase() == username.to_ascii_lowercase()
                    && (if let Some(race) = race {
                        b.race.to_lowercase() == race.to_lowercase()
                    } else {
                        true
                    })
            })
            .await
    }

    pub async fn add_bet(&self, bet: Bet) -> Result<(), DbError> {
        self.db.lock().await.insert("bets", bet).await
    }

    pub async fn update_bet(&self, bet: Bet, current_race: &str) -> Result<(), DbError> {
        let username = bet.username.to_lowercase();

        self.db
            .lock()
            .await
            .update("bets", bet, |b: &&Bet| {
                b.username.to_lowercase() == username
                    && b.race.to_lowercase() == current_race.to_lowercase()
            })
            .await
    }
}

pub struct EventStore<'a> {
    db: &'a State<Mutex<Database<&'static str>>>,
}

impl<'a> EventStore<'a> {
    pub fn new(db: &'a State<Mutex<Database<&'static str>>>) -> Self {
        Self { db }
    }

    pub async fn next_event(&self) -> Result<Event, DbError> {
        self.db
            .lock()
            .await
            .find("events", |e: &Event| {
                e.datetime > Utc::now()
                    && e.channel.to_lowercase() == CHANNEL.to_lowercase()
                    && e.category.to_lowercase().contains(CATEGORY)
                    && e.description.eq_ignore_ascii_case("race")
            })
            .await?
            .into_iter()
            .sorted_by(|a, b| a.datetime.cmp(&b.datetime))
            .next()
            .ok_or(DbError::NoMatch)
    }
}
