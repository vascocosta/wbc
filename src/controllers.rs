use csv_db::Database;
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

use crate::models::{Bet, Profile, Registration, ScoredBet, User};
use crate::store::{CORRECT_FIVE, CORRECT_PODIUM, PARLAY, Store, WRONG_PLACE};

#[get("/")]
pub async fn index(cookies: &CookieJar<'_>, db: &State<Mutex<Database<&str>>>) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let store = Store::new(db);

    let normalized_results = match store.normalized_results().await {
        Ok(normalized_results) => normalized_results,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get event results.", logged_in },
            );
        }
    };

    let bets = match store.get_bets(None, None).await {
        Ok(bets) => bets,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get bets.", logged_in },
            );
        }
    };
    let scored_bets = store.scored_bets(&bets, &normalized_results).await;
    let grouped_bets = scored_bets.iter().into_group_map_by(|b| &b.bet.username);

    let points: Vec<(usize, (&String, u16))> = grouped_bets
        .into_iter()
        .map(|(username, group)| {
            let total_points: u16 = group.into_iter().map(|b| b.points).sum();
            (username, total_points)
        })
        .sorted_by(|a, b| b.1.cmp(&a.1))
        .enumerate()
        .collect();

    let current_event = &store
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

    let store = Store::new(db);

    let normalized_results = match store.normalized_results().await {
        Ok(normalized_results) => normalized_results,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get event results.", logged_in },
            );
        }
    };

    let bets = match store.get_bets(Some(&user.username), None).await {
        Ok(bets) => bets,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get your bet.", logged_in },
            );
        }
    };
    let scored_bets: Vec<ScoredBet<'_>> = store
        .scored_bets(&bets, &normalized_results)
        .await
        .into_iter()
        .rev()
        .take(24)
        .collect();

    Template::render("history", context! {scored_bets, logged_in})
}

#[get("/latest")]
pub async fn latest(cookies: &CookieJar<'_>, db: &State<Mutex<Database<&str>>>) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let store = Store::new(db);

    let normalized_results = match store.normalized_results().await {
        Ok(normalized_results) => normalized_results,
        Err(_) => {
            return Template::render(
                "latest",
                context! { error: "Could not get event results.", logged_in },
            );
        }
    };

    let bets = match store.get_bets(None, None).await {
        Ok(bets) => bets,
        Err(_) => {
            return Template::render(
                "latest",
                context! { error: "Could not get bets.", logged_in },
            );
        }
    };
    let scored_bets: Vec<ScoredBet<'_>> = store
        .scored_bets(&bets, &normalized_results)
        .await
        .into_iter()
        .rev()
        .take(20)
        .collect();

    Template::render("latest", context! {scored_bets, logged_in})
}

#[get("/bet")]
pub async fn bet_form(
    cookies: &CookieJar<'_>,
    user: User,
    db: &State<Mutex<Database<&str>>>,
) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let store = Store::new(db);

    let drivers = store.all_drivers().await.ok().unwrap_or_default();
    let current_event = &store
        .next_event()
        .await
        .expect("The next event should be available on the database")
        .name;

    let bets = match store
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

    let store = Store::new(db);

    let drivers = store.all_drivers().await.ok().unwrap_or_default();
    let current_event = &store
        .next_event()
        .await
        .expect("The next event should be available on the database")
        .name;

    // We use a mutable bet binding with an updated race field to avoid a bug.
    // When posting a new bet after its deadline (through bet_submit), which was rendered by bet_form before,
    // if we don't use a new bet.race, the deadline could be abused.
    let mut bet = form_data.into_inner();
    bet.race = current_event.to_owned();

    match store.update_bet(bet.clone(), current_event).await {
        Ok(_) => Template::render(
            "bet",
            context! { current_event, drivers, bet, success: "Your bet was successfully updated.", logged_in },
        ),
        Err(_) => Template::render(
            "bet",
            context! { current_event, drivers, bet, error: "Problem updating bet.", logged_in
            },
        ),
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
    let store = Store::new(db);

    let registration = form_data.into_inner();

    match store
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

#[get("/profile")]
pub async fn profile_form(cookies: &CookieJar<'_>, db: &State<Mutex<Database<&str>>>) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let token = match cookies.get_private("session") {
        Some(token) => token.value().to_owned(),
        None => {
            return Template::render(
                "profile",
                context! { error: "Could not find your user.", logged_in},
            );
        }
    };

    let user = match Store::get_user(&token, db).await {
        Some(user) => user,
        None => {
            return Template::render(
                "profile",
                context! { error: "Could not find your user.", logged_in },
            );
        }
    };

    Template::render("profile", context! { country: &user.country, logged_in})
}

#[post("/profile", data = "<form_data>")]
pub async fn profile_submit(
    cookies: &CookieJar<'_>,
    db: &State<Mutex<Database<&str>>>,
    form_data: Form<Profile>,
) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let store = Store::new(db);

    let profile_data = form_data.into_inner();

    let token = match cookies.get_private("session") {
        Some(token) => token.value().to_owned(),
        None => {
            return Template::render(
                "profile",
                context! { error: "Could not find your user.", logged_in },
            );
        }
    };

    let mut user = match Store::get_user(&token, db).await {
        Some(user) => user,
        None => {
            return Template::render(
                "profile",
                context! { error: "Could not find your user.", logged_in },
            );
        }
    };

    user.country = profile_data.country.clone();

    if profile_data.password.len() > 0 {
        user.password = match Store::hash_password(&profile_data.password).await {
            Ok(hashed_password) => hashed_password,
            Err(_) => {
                return Template::render(
                    "profile",
                    context! { error: "Could not update your profile.", logged_in },
                );
            }
        };
    }

    if store.update_user(user, &token).await.is_err() {
        return Template::render(
            "profile",
            context! { error: "Could not update your profile.", logged_in },
        );
    }

    Template::render(
        "profile",
        context! { country: profile_data.country, success: "Profile updated successfully.", logged_in },
    )
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
    let store = Store::new(db);

    let registration = form_data.into_inner();

    match store
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

#[get("/rules")]
pub async fn rules(cookies: &CookieJar<'_>) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    Template::render(
        "rules",
        context! { logged_in, correct_podium: CORRECT_PODIUM, correct_five: CORRECT_FIVE, wrong_place: WRONG_PLACE, parlay: PARLAY },
    )
}

#[catch(401)]
pub fn unauthorized() -> Flash<Redirect> {
    Flash::error(Redirect::to(uri!(login_form)), "Please login to continue")
}
