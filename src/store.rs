use std::{
    collections::HashMap,
    io::{Error, ErrorKind},
};

use argon2::{
    Argon2, PasswordHasher,
    password_hash::{PasswordHash, PasswordVerifier, SaltString, rand_core::OsRng},
};
use chrono::Utc;
use csv_db::{Database, DbError};
use itertools::Itertools;
use rocket::{State, futures::future::join_all, tokio::sync::Mutex};
use uuid::Uuid;

use crate::models::{Driver, Event, Guess, RaceResult, ScoredGuess, User};

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

    pub async fn add_user(
        &self,
        username: &str,
        password: &str,
        country: Option<String>,
    ) -> Result<(), DbError> {
        let db_lock = self.db.lock().await;

        let users = db_lock
            .find("users", |u: &User| {
                u.username.eq_ignore_ascii_case(username)
            })
            .await?;

        if users.is_empty() {
            let user = User {
                token: Uuid::new_v4().to_string(),
                username: username.to_lowercase(),
                password: Self::hash_password(password)
                    .await
                    .map_err(|_| DbError::NoMatch)?,
                country: country.unwrap_or_default(),
            };

            db_lock.insert("users", user).await
        } else {
            Err(DbError::Io(Error::from(ErrorKind::AlreadyExists)))
        }
    }

    pub async fn all_users(&self) -> Result<Vec<User>, DbError> {
        self.db.lock().await.find("users", |_| true).await
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

    pub async fn get_guesses(
        &self,
        username: Option<&str>,
        race: Option<&str>,
    ) -> Result<Vec<Guess>, DbError> {
        self.db
            .lock()
            .await
            .find("guesses", |g: &Guess| {
                (if let Some(username) = username {
                    g.username.eq_ignore_ascii_case(&username)
                } else {
                    true
                }) && (if let Some(race) = race {
                    g.race.eq_ignore_ascii_case(&race)
                } else {
                    true
                })
            })
            .await
    }

    pub async fn update_guess(&self, guess: Guess, current_race: &str) -> Result<(), DbError> {
        let username = guess.username.to_lowercase();

        let db_lock = self.db.lock().await;

        if let Err(e) = db_lock
            .update("guesses", guess.clone(), |g: &&Guess| {
                g.username.to_lowercase() == username && g.race.eq_ignore_ascii_case(current_race)
            })
            .await
        {
            match e {
                DbError::NoMatch => match db_lock.insert("guesses", guess).await {
                    Ok(_) => return Ok(()),
                    Err(_) => return Err(DbError::Io(Error::from(ErrorKind::Other))),
                },
                _ => return Err(DbError::Io(Error::from(ErrorKind::Other))),
            }
        }

        Ok(())
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

    pub async fn scored_guesses(
        &self,
        guesses: &'a [Guess],
        normalized_results: &HashMap<String, RaceResult>,
    ) -> Vec<ScoredGuess<'a>> {
        let futures: Vec<_> = guesses
            .iter()
            .map(|g| async move {
                ScoredGuess {
                    guess: g,
                    points: self.score_guess(g, normalized_results).await,
                }
            })
            .collect();

        join_all(futures).await
    }

    async fn score_guess(
        &self,
        guess: &Guess,
        normalized_results: &HashMap<String, RaceResult>,
    ) -> u16 {
        let result = match normalized_results.get(&guess.race) {
            Some(result) => result,
            None => return 0,
        };

        let guess_positions = [&guess.p1, &guess.p2, &guess.p3, &guess.p4, &guess.p5];
        let result_positions = [&result.p1, &result.p2, &result.p3, &result.p4, &result.p5];

        let mut score = 0;

        for (pos, guess_driver) in guess_positions.iter().enumerate() {
            if guess_driver.eq_ignore_ascii_case(&result_positions[pos]) {
                score += if pos < 3 {
                    CORRECT_PODIUM
                } else {
                    CORRECT_FIVE
                };
            } else if result_positions
                .iter()
                .any(|result_driver| guess_driver.eq_ignore_ascii_case(&result_driver))
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

#[cfg(test)]
mod tests {
    use super::*;
    use rocket::tokio;

    fn normalized_results() -> HashMap<String, RaceResult> {
        HashMap::from([(
            "Test GP".to_string(),
            RaceResult {
                race: "Test GP".to_string(),
                p1: "NOR".to_string(),
                p2: "VER".to_string(),
                p3: "PIA".to_string(),
                p4: "RUS".to_string(),
                p5: "LEC".to_string(),
            },
        )])
    }

    fn perfect_guess() -> Guess {
        Guess {
            race: "Test GP".to_string(),
            username: "test".to_string(),
            p1: "NOR".to_string(),
            p2: "VER".to_string(),
            p3: "PIA".to_string(),
            p4: "RUS".to_string(),
            p5: "LEC".to_string(),
        }
    }

    fn mixed_guess() -> Guess {
        Guess {
            race: "Test GP".to_string(),
            username: "test".to_string(),
            p1: "VER".to_string(),
            p2: "NOR".to_string(),
            p3: "PIA".to_string(),
            p4: "LEC".to_string(),
            p5: "RUS".to_string(),
        }
    }

    fn partial_guess() -> Guess {
        Guess {
            race: "Test GP".to_string(),
            username: "test".to_string(),
            p1: "NOR".to_string(),
            p2: "HAM".to_string(),
            p3: "PIA".to_string(),
            p4: "ANT".to_string(),
            p5: "LEC".to_string(),
        }
    }

    #[tokio::test]
    async fn score_guess() {
        let db = Mutex::new(Database::new("test_data", None));
        let store = Store::new(State::from(&db));

        let perfect_score = store
            .score_guess(&perfect_guess(), &normalized_results())
            .await;
        let mixed_score = store
            .score_guess(&mixed_guess(), &normalized_results())
            .await;
        let partial_score = store
            .score_guess(&partial_guess(), &normalized_results())
            .await;

        assert!(perfect_score == 25);
        assert!(mixed_score == 7);
        assert!(partial_score == 12);
    }

    #[tokio::test]
    async fn scored_guesses() {
        let db = Mutex::new(Database::new("test_data", None));
        let store = Store::new(State::from(&db));

        let guesses = [perfect_guess(), mixed_guess(), partial_guess()];
        let scored_guesses = store.scored_guesses(&guesses, &normalized_results()).await;

        assert!(
            scored_guesses[0].points + scored_guesses[1].points + scored_guesses[2].points == 44
        )
    }

    #[tokio::test]
    async fn update_guess() {
        let db = Mutex::new(Database::new("test_data", None));
        let store = Store::new(State::from(&db));

        let result = store.update_guess(perfect_guess(), "Test GP").await;
        let guess = store
            .get_guesses(Some("test"), Some("Test GP"))
            .await
            .unwrap_or_default();

        assert!(
            result.is_ok()
                && guess[0].race == "Test GP"
                && guess[0].username == "test"
                && guess[0].p1 == "NOR"
                && guess[0].p2 == "VER"
                && guess[0].p3 == "PIA"
                && guess[0].p4 == "RUS"
                && guess[0].p5 == "LEC"
        )
    }
}
