use csv_db::Database;
use itertools::Itertools;
use rocket::{
    Request, State,
    form::Form,
    http::{Cookie, CookieJar, SameSite},
    request::FlashMessage,
    response::{Flash, Redirect},
    time::{Duration, OffsetDateTime},
    tokio::sync::Mutex,
    uri,
};
use rocket_dyn_templates::{Template, context};

use crate::models::{Guess, Profile, Registration, ScoredGuess, User};
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

    let guesses = match store.get_guesses(None, None).await {
        Ok(guesses) => guesses,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get guesses.", logged_in },
            );
        }
    };
    let scored_guesses = store.scored_guesses(&guesses, &normalized_results).await;
    let grouped_guesses = scored_guesses
        .iter()
        .into_group_map_by(|g| &g.guess.username);

    let leaderboard = store.leaderboard(grouped_guesses).await;

    let current_event = &store
        .next_event()
        .await
        .expect("The next event should be available on the database");

    Template::render("index", context! { logged_in, current_event, leaderboard})
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

    let guesses = match store.get_guesses(Some(&user.username), None).await {
        Ok(guesses) => guesses,
        Err(_) => {
            return Template::render(
                "history",
                context! { error: "Could not get your guess.", logged_in },
            );
        }
    };
    let scored_guesses: Vec<ScoredGuess<'_>> = store
        .scored_guesses(&guesses, &normalized_results)
        .await
        .into_iter()
        .rev()
        .take(24)
        .collect();

    Template::render("history", context! {scored_guesses, logged_in})
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

    let guesses = match store.get_guesses(None, None).await {
        Ok(guesses) => guesses,
        Err(_) => {
            return Template::render(
                "latest",
                context! { error: "Could not get guesses.", logged_in },
            );
        }
    };
    let scored_guesses: Vec<ScoredGuess<'_>> = store
        .scored_guesses(&guesses, &normalized_results)
        .await
        .into_iter()
        .rev()
        .take(20)
        .collect();

    Template::render("latest", context! {scored_guesses, logged_in})
}

#[get("/play")]
pub async fn play_form(
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
        .expect("The next event should be available on the database");

    let guesses = match store
        .get_guesses(Some(&user.username), Some(&current_event.name))
        .await
    {
        Ok(guesses) => guesses,
        Err(_) => {
            return Template::render(
                "play",
                context! { current_event, drivers: drivers, guess: Guess::default(), error: "Could not get your guess.", logged_in },
            );
        }
    };
    let guess = guesses.into_iter().next().unwrap_or(Guess {
        race: current_event.name.to_string(),
        username: user.username.clone(),
        ..Default::default()
    });

    Template::render(
        "play",
        context! { current_event, drivers, guess, logged_in },
    )
}

#[post("/play", data = "<form_data>")]
pub async fn play_submit(
    cookies: &CookieJar<'_>,
    user: User,
    db: &State<Mutex<Database<&str>>>,
    form_data: Form<Guess>,
) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    let store = Store::new(db);

    let drivers = store.all_drivers().await.ok().unwrap_or_default();
    let current_event = &store
        .next_event()
        .await
        .expect("The next event should be available on the database");

    let mut guess = form_data.into_inner();

    // Return early with an authentication error if guess.username differs from user.username.
    // Since user is a User guard, it can only be instanced with a valid private session cookie.
    // Therefore this if guarantees that the username in the guess must be from an authenticated user.
    // In other words the username in the guess must be from the user creating/updating the guess.
    // Unless a user can guess the encrypted private session cookie from another user, we are safe. :)
    if !guess.username.eq_ignore_ascii_case(&user.username) {
        return Template::render(
            "play",
            context! { current_event, drivers, guess, error: "Unauthenticated.", logged_in },
        );
    }

    // When posting a new guess after its deadline (through guess_submit), which was rendered by guess_form before,
    // if we don't use a new guess.race, the deadline could be abused.
    guess.race = current_event.name.clone();

    // Make sure we always store a guess with consistent case for every field.
    guess.normalize();

    if !guess.valid(&drivers) {
        return Template::render(
            "play",
            context! {
                current_event,
                drivers,
                guess,
                error: "Your guess must contain 5 different driver codes.",
                logged_in,
            },
        );
    }

    match store.update_guess(guess.clone(), &current_event.name).await {
        Ok(_) => Template::render(
            "play",
            context! { current_event, drivers, guess, success: "Your guess was successfully updated.", logged_in },
        ),
        Err(_) => Template::render(
            "play",
            context! { current_event, drivers, guess, error: "Problem updating.", logged_in
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

            Ok(Redirect::to(uri! { play_form }))
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

#[get("/profile?<token>")]
pub async fn profile_form(
    token: Option<String>,
    cookies: &CookieJar<'_>,
    db: &State<Mutex<Database<&str>>>,
) -> Result<Template, Flash<Redirect>> {
    let logged_in = cookies.get_private("session").is_some();

    let token = match token {
        Some(token) => {
            // Create cookie with the token.
            let cookie = Cookie::build(("session", token.clone()))
                .http_only(true)
                .same_site(SameSite::Lax)
                .secure(true)
                .expires(OffsetDateTime::now_utc() + Duration::days(365));

            cookies.add_private(cookie);

            token
        }
        None => match cookies.get_private("session") {
            Some(token) => token.value().to_owned(),
            None => {
                return Err(Flash::error(
                    Redirect::to(uri!(login_form)),
                    "Please login to continue.",
                ));
            }
        },
    };

    let user = match Store::get_user(&token, db).await {
        Some(user) => user,
        None => {
            return Err(Flash::error(
                Redirect::to(uri!(login_form)),
                "Could not find your user.",
            ));
        }
    };

    Ok(Template::render(
        "profile",
        context! { country: &user.country, logged_in},
    ))
}

#[post("/profile", data = "<form_data>")]
pub async fn profile_submit(
    cookies: &CookieJar<'_>,
    _user: User,
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
        .add_user(
            &registration.username,
            &registration.password,
            registration.country,
        )
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

#[get("/disclaimer")]
pub async fn disclaimer(cookies: &CookieJar<'_>) -> Template {
    let logged_in = cookies.get_private("session").is_some();

    Template::render("disclaimer", context! { logged_in })
}

#[catch(401)]
pub fn unauthorized(req: &Request) -> Result<Flash<Redirect>, &'static str> {
    match req.headers().get_one("x-api-key") {
        Some(_) => Err("Unauthorized"),
        None => Ok(Flash::error(
            Redirect::to(uri!(login_form)),
            "Please login to continue.",
        )),
    }
}
