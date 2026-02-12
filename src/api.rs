use csv_db::Database;
use itertools::Itertools;
use rocket::{State, http::Status, serde::json::Json, tokio::sync::Mutex};

use crate::store::Store;

#[get("/leaderboard")]
pub async fn leaderboard(
    db: &State<Mutex<Database<&str>>>,
) -> Result<Json<Vec<(String, u16)>>, Status> {
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

    Ok(Json(store.leaderboard(grouped_guesses).await))
}
