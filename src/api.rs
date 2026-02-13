use csv_db::Database;
use itertools::Itertools;
use rocket::{State, http::Status, serde::json::Json, tokio::sync::Mutex};

use crate::store::Store;

#[derive(Responder)]
pub enum LeaderboardResponse {
    Json(Json<Vec<(String, u16)>>),
    PlainText(String),
    Irc(String),
}

#[get("/leaderboard?<format>")]
pub async fn leaderboard(
    db: &State<Mutex<Database<&str>>>,
    format: Option<&str>,
) -> Result<LeaderboardResponse, Status> {
    let store = Store::new(db);

    let normalized_results = store
        .normalized_results()
        .await
        .map_err(|_| Status::InternalServerError)?;

    let guesses = store
        .get_guesses(None, None)
        .await
        .map_err(|_| Status::InternalServerError)?;
    let scored_guesses = store.scored_guesses(&guesses, &normalized_results).await;
    let grouped_guesses = scored_guesses
        .iter()
        .into_group_map_by(|g| &g.guess.username);

    let leaderboard = store.leaderboard(grouped_guesses).await;

    match format {
        Some(kind) => match kind {
            "json" | "JSON" => Ok(LeaderboardResponse::Json(Json(leaderboard))),
            "irc" | "IRC" => {
                let irc_leaderboard: String = leaderboard
                    .iter()
                    .enumerate()
                    .map(|r| {
                        let code: String =
                            r.1.0
                                .chars()
                                .filter(|c| c.is_alphanumeric())
                                .take(3)
                                .collect();

                        format!("{}. {} {}", r.0 + 1, code.to_ascii_uppercase(), r.1.1)
                    })
                    .join(" | ");

                Ok(LeaderboardResponse::Irc(irc_leaderboard))
            }
            "text" | "TEXT" => {
                let text_leaderboard: String = leaderboard
                    .iter()
                    .enumerate()
                    .map(|r| format!("{}. {} {}", r.0 + 1, r.1.0, r.1.1))
                    .join(" | ");

                Ok(LeaderboardResponse::PlainText(text_leaderboard))
            }
            _ => return Err(Status::InternalServerError),
        },
        None => Ok(LeaderboardResponse::Json(Json(leaderboard))),
    }
}
