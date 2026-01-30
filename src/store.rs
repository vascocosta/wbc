use std::collections::HashMap;

use argon2::{
    Argon2, PasswordHasher,
    password_hash::{PasswordHash, PasswordVerifier, SaltString, rand_core::OsRng},
};
use chrono::Utc;
use csv_db::{Database, DbError};
use itertools::Itertools;
use rocket::{State, futures::future::join_all, tokio::sync::Mutex};
use uuid::Uuid;

use crate::models::{Bet, Driver, Event, RaceResult, ScoredBet, User};

const CATEGORY: &str = "formula 1";
const CHANNEL: &str = "#formula1";
pub const CORRECT_PODIUM: u16 = 3;
pub const CORRECT_FIVE: u16 = 6;
pub const WRONG_PLACE: u16 = 1;
pub const PARLAY: u16 = 4;

pub struct Store<'a> {
    db: &'a State<Mutex<Database<&'static str>>>,
}

impl<'a> Store<'a> {
    pub fn new(db: &'a State<Mutex<Database<&'static str>>>) -> Self {
        Self { db }
    }

    pub async fn user_exists(&self, username: &str) -> Result<bool, DbError> {
        let users = self
            .db
            .lock()
            .await
            .find("users", |u: &User| {
                u.username.eq_ignore_ascii_case(username)
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
            country: "".to_string(),
        };

        self.db.lock().await.insert("users", user).await
    }

    pub async fn update_user(&self, user: User, token: &str) -> Result<(), DbError> {
        self.db
            .lock()
            .await
            .update("users", user, |u: &&User| u.token == token)
            .await
    }

    pub async fn get_user(token: &str, db: &State<Mutex<Database<&str>>>) -> Option<User> {
        db.lock()
            .await
            .find("users", |u: &User| u.token == token)
            .await
            .ok()?
            .into_iter()
            .next()
    }

    pub async fn validate_user(&self, username: &str, password: &str) -> Option<String> {
        let users = self
            .db
            .lock()
            .await
            .find("users", |u: &User| {
                u.username.eq_ignore_ascii_case(username)
            })
            .await
            .ok()?;

        if let Some(user) = users.first() {
            let parsed_hash = PasswordHash::new(&user.password).ok()?;

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

    pub async fn hash_password(password: &str) -> Result<String, &'static str> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        Ok(argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| "Could not hash password")?
            .to_string())
    }

    pub async fn all_drivers(&self) -> Result<Vec<Driver>, DbError> {
        self.db.lock().await.find("drivers", |_| true).await
    }

    pub async fn get_bets(
        &self,
        username: Option<&str>,
        race: Option<&str>,
    ) -> Result<Vec<Bet>, DbError> {
        self.db
            .lock()
            .await
            .find("bets", |b: &Bet| {
                (if let Some(username) = username {
                    b.username.eq_ignore_ascii_case(&username)
                } else {
                    true
                }) && (if let Some(race) = race {
                    b.race.eq_ignore_ascii_case(&race)
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
                b.username.to_lowercase() == username && b.race.eq_ignore_ascii_case(current_race)
            })
            .await
    }

    pub async fn next_event(&self) -> Result<Event, DbError> {
        self.db
            .lock()
            .await
            .find("events", |e: &Event| {
                e.datetime > Utc::now()
                    && e.channel.eq_ignore_ascii_case(CHANNEL)
                    && e.category.to_lowercase().contains(CATEGORY)
                    && e.description.eq_ignore_ascii_case("race")
            })
            .await?
            .into_iter()
            .sorted_by(|a, b| a.datetime.cmp(&b.datetime))
            .next()
            .ok_or(DbError::NoMatch)
    }

    pub async fn scored_bets(
        &self,
        bets: &'a [Bet],
        normalized_results: &HashMap<String, RaceResult>,
    ) -> Vec<ScoredBet<'a>> {
        let futures: Vec<_> = bets
            .iter()
            .map(|b| async move {
                ScoredBet {
                    bet: b,
                    points: self.score_bet(b, normalized_results).await,
                }
            })
            .collect();

        join_all(futures).await
    }

    async fn score_bet(&self, bet: &Bet, normalized_results: &HashMap<String, RaceResult>) -> u16 {
        let result = match normalized_results.get(&bet.race) {
            Some(result) => result,
            None => return 0,
        };

        let bet_positions = [&bet.p1, &bet.p2, &bet.p3, &bet.p4, &bet.p5];
        let result_positions = [&result.p1, &result.p2, &result.p3, &result.p4, &result.p5];

        let mut score = 0;

        for (pos, bet_driver) in bet_positions.iter().enumerate() {
            if bet_driver.eq_ignore_ascii_case(&result_positions[pos]) {
                score += if pos < 3 {
                    CORRECT_PODIUM
                } else {
                    CORRECT_FIVE
                };
            } else if result_positions
                .iter()
                .any(|result_driver| bet_driver.eq_ignore_ascii_case(&result_driver))
            {
                score += WRONG_PLACE;
            }
        }

        if score == 3 * CORRECT_PODIUM + 2 * CORRECT_FIVE {
            score += PARLAY;
        }

        score
    }

    pub async fn normalized_results(&self) -> Result<HashMap<String, RaceResult>, DbError> {
        let results = self.results().await?;

        Ok(results.into_iter().map(|r| (r.race.clone(), r)).collect())
    }

    pub async fn results(&self) -> Result<Vec<RaceResult>, DbError> {
        self.db
            .lock()
            .await
            .find("results", |_: &RaceResult| true)
            .await
    }
}
