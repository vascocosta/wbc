use argon2::{
    Argon2, PasswordHasher,
    password_hash::{PasswordHash, PasswordVerifier, SaltString, rand_core::OsRng},
};
use csv_db::{Database, DbError};
use rocket::{State, tokio::sync::Mutex};
use uuid::Uuid;

use crate::models::{Driver, User};

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
