use csv_db::{Database, DbError};
use itertools::Itertools;
use rocket::{
    State,
    form::Form,
    http::{Cookie, CookieJar, SameSite},
    request::FlashMessage,
    response::{Flash, Redirect},
    time::{Duration, OffsetDateTime},
    tokio::sync::Mutex,
    uri,
};
use rocket_dyn_templates::{Template, context};

use crate::store::{BetStore, EventStore, ResultStore, ScoreStore, UserStore};
use crate::{
    models::{Bet, Registration, User},
    store::DriverStore,
};

#[get("/")]
pub async fn index(cookies: &CookieJar<'_>, db: &State<Mutex<Database<&str>>>) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let event_store = EventStore::new(db);
    let bet_store = BetStore::new(db);
    let score_store = ScoreStore::new();
    let result_store = ResultStore::new(db);

    let normalized_results = match result_store.normalized_results().await {
        Ok(normalized_results) => normalized_results,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get event results.", logged_in },
            );
        }
    };

    let bets = match bet_store.get_bets(None, None).await {
        Ok(bets) => bets,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get bets.", logged_in },
            );
        }
    };
    let scored_bets = score_store.scored_bets(&bets, &normalized_results).await;
    let grouped_bets = scored_bets.into_iter().chunk_by(|b| &b.bet.username);

    let points: Vec<(usize, (&String, u16))> = grouped_bets
        .into_iter()
        .map(|(username, group)| {
            let total_points: u16 = group.into_iter().map(|b| b.points).sum();
            (username, total_points)
        })
        .sorted_by(|a, b| b.1.cmp(&a.1))
        .enumerate()
        .collect();

    let current_event = &event_store
        .next_event()
        .await
        .expect("The next event should be available on the database");

    Template::render("index", context! { logged_in, current_event, points})
}

#[get("/history")]
pub async fn history(
    cookies: &CookieJar<'_>,
    user: User,
    db: &State<Mutex<Database<&str>>>,
) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let bet_store = BetStore::new(db);
    let score_store = ScoreStore::new();
    let result_store = ResultStore::new(db);

    let normalized_results = match result_store.normalized_results().await {
        Ok(normalized_results) => normalized_results,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get event results.", logged_in },
            );
        }
    };

    let bets = match bet_store.get_bets(Some(&user.username), None).await {
        Ok(bets) => bets,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get your bet.", logged_in },
            );
        }
    };
    let scored_bets = score_store.scored_bets(&bets, &normalized_results).await;

    Template::render("history", context! {scored_bets, logged_in})
}

#[get("/bet")]
pub async fn bet_form(
    cookies: &CookieJar<'_>,
    user: User,
    db: &State<Mutex<Database<&str>>>,
) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let driver_store = DriverStore::new(db);
    let bet_store = BetStore::new(db);
    let event_store = EventStore::new(db);

    let drivers = driver_store.all_drivers().await.ok().unwrap_or_default();
    let current_event = &event_store
        .next_event()
        .await
        .expect("The next event should be available on the database")
        .name;

    let bets = match bet_store
        .get_bets(Some(&user.username), Some(current_event))
        .await
    {
        Ok(bets) => bets,
        Err(_) => {
            return Template::render(
                "bet",
                context! { current_event, drivers: drivers, bet: Bet::default(), error: "Could not get your bet.", logged_in },
            );
        }
    };
    let bet = bets.into_iter().next().unwrap_or(Bet {
        race: current_event.to_string(),
        username: user.username.clone(),
        ..Default::default()
    });

    Template::render("bet", context! { current_event, drivers, bet, logged_in })
}

#[post("/bet", data = "<form_data>")]
pub async fn bet_submit(
    cookies: &CookieJar<'_>,
    db: &State<Mutex<Database<&str>>>,
    form_data: Form<Bet>,
) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let driver_store = DriverStore::new(db);
    let bet_store = BetStore::new(db);
    let event_store = EventStore::new(db);

    let drivers = driver_store.all_drivers().await.ok().unwrap_or_default();
    let current_event = &event_store
        .next_event()
        .await
        .expect("The next event should be available on the database")
        .name;

    // We use a mutable bet binding with an updated race field to avoid a bug.
    // When posting a new bet after its deadline (through bet_submit), which was rendered by bet_form before,
    // if we don't use a new bet.race, the deadline could be abused.
    let mut bet = form_data.into_inner();
    bet.race = current_event.to_owned();

    match bet_store.update_bet(bet.clone(), current_event).await {
        Ok(_) => Template::render(
            "bet",
            context! { current_event, drivers, bet, success: "Your bet was successfully updated.", logged_in },
        ),
        Err(e) => match e {
            DbError::NoMatch => match bet_store.add_bet(bet.clone()).await {
                Ok(_) => Template::render(
                    "bet",
                    context! { current_event, drivers, bet, success: "Your bet was successfully updated.", logged_in },
                ),
                Err(_) => Template::render(
                    "bet",
                    context! { current_event, drivers, bet, error: "Problem updating bet.", logged_in },
                ),
            },
            _ => Template::render(
                "bet",
                context! { current_event, drivers, bet, error: "Problem updating bet.", logged_in },
            ),
        },
    }
}

#[get("/login")]
pub async fn login_form(flash: Option<FlashMessage<'_>>) -> Template {
    Template::render(
        "login",
        context! { flash: flash.map(|flash| flash.message().to_string()) },
    )
}

#[post("/login", data = "<form_data>")]
pub async fn login_submit(
    cookies: &CookieJar<'_>,
    db: &State<Mutex<Database<&str>>>,
    form_data: Form<Registration>,
) -> Result<Redirect, Template> {
    let user_store = UserStore::new(db);
    let registration = form_data.into_inner();

    match user_store
        .validate_user(&registration.username, &registration.password)
        .await
    {
        Some(token) => {
            // Create cookie with the token.
            let cookie = Cookie::build(("session", token))
                .http_only(true)
                .same_site(SameSite::Lax)
                .secure(true)
                .expires(OffsetDateTime::now_utc() + Duration::days(365));

            cookies.add_private(cookie);

            Ok(Redirect::to(uri! { bet_form }))
        }
        None => Err(Template::render(
            "login",
            context! { error: "Login failed." },
        )),
    }
}

#[get("/logout")]
pub async fn logout(cookies: &CookieJar<'_>) -> Redirect {
    cookies.remove_private("session");
    Redirect::to(uri!(index))
}

#[get("/register")]
pub async fn register_form() -> Template {
    Template::render("register", context! {})
}

#[post("/register", data = "<form_data>")]
pub async fn register_submit(
    _cookies: &CookieJar<'_>,
    db: &State<Mutex<Database<&str>>>,
    form_data: Form<Registration>,
) -> Result<Flash<Redirect>, Template> {
    let user_store = UserStore::new(db);
    let registration = form_data.into_inner();

    match user_store.user_exists(&registration.username).await {
        Ok(exists) => {
            if exists {
                Err(Template::render(
                    "register",
                    context! { error: "Username already exists." },
                ))
            } else {
                // Username does not exist. Insert user into the database.
                match user_store
                    .add_user(&registration.username, &registration.password)
                    .await
                {
                    Ok(_) => Ok(Flash::success(
                        Redirect::to(uri!(login_form)),
                        "Registration successful. You can now login.",
                    )),
                    Err(_) => Err(Template::render(
                        "register",
                        context! { error: "Registration failed." },
                    )),
                }
            }
        }
        Err(_) => Err(Template::render(
            "register",
            context! { error: "Error accessing database." },
        )),
    }
}

#[catch(401)]
pub fn unauthorized() -> Flash<Redirect> {
    Flash::error(Redirect::to(uri!(login_form)), "Please login to continue")
}
