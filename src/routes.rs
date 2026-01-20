use csv_db::{Database, DbError};
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

use crate::store::{BetStore, EventStore, UserStore};
use crate::{
    models::{Bet, Registration, User},
    store::DriverStore,
};

#[get("/")]
pub async fn index(cookies: &CookieJar<'_>) -> Template {
    let logged_in = cookies.get("session").is_some();

    Template::render("index", context! { logged_in })
}

#[get("/bet")]
pub async fn bet_form(
    cookies: &CookieJar<'_>,
    user: User,
    db: &State<Mutex<Database<&str>>>,
) -> Result<Template, Template> {
    let logged_in = cookies.get("session").is_some();

    let driver_store = DriverStore::new(db);
    let bet_store = BetStore::new(db);
    let event_store = EventStore::new(db);

    let drivers = driver_store.all_drivers().await.ok().unwrap_or_default();
    let current_race = &event_store
        .next_event()
        .await
        .expect("The next event should be available on the database")
        .name;

    let bets = match bet_store.get_bet(&user.username, current_race).await {
        Ok(bets) => bets,
        Err(_) => {
            return Err(Template::render(
                "bet",
                context! { current_race, drivers: drivers, bet: Bet::default(), error: "Could not get your bet.", logged_in },
            ));
        }
    };
    let bet = bets.into_iter().next().unwrap_or(Bet {
        race: current_race.to_string(),
        username: user.username.clone(),
        ..Default::default()
    });

    Ok(Template::render(
        "bet",
        context! { current_race, drivers, bet, logged_in },
    ))
}

#[post("/bet", data = "<form_data>")]
pub async fn bet_submit(
    cookies: &CookieJar<'_>,
    db: &State<Mutex<Database<&str>>>,
    form_data: Form<Bet>,
) -> Result<Template, Template> {
    let logged_in = cookies.get("session").is_some();

    let driver_store = DriverStore::new(db);
    let bet_store = BetStore::new(db);
    let event_store = EventStore::new(db);

    let drivers = driver_store.all_drivers().await.ok().unwrap_or_default();
    let current_race = &event_store
        .next_event()
        .await
        .expect("The next event should be available on the database")
        .name;

    let bet = form_data.into_inner();

    match bet_store.update_bet(bet.clone(), current_race).await {
        Ok(_) => Ok(Template::render(
            "bet",
            context! { current_race, drivers, bet, error: "Your bet was successfully updated.", logged_in },
        )),
        Err(e) => match e {
            DbError::NoMatch => match bet_store.add_bet(bet.clone()).await {
                Ok(_) => Ok(Template::render(
                    "bet",
                    context! { current_race, drivers, bet, error: "Your bet was successfully updated.", logged_in },
                )),
                Err(_) => Err(Template::render(
                    "bet",
                    context! { current_race, drivers, bet, error: "Problem updating bet.", logged_in },
                )),
            },
            _ => Err(Template::render(
                "bet",
                context! { current_race, drivers, bet, error: "Problem updating bet.", logged_in },
            )),
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
                .expires(OffsetDateTime::now_utc() + Duration::days(1));

            cookies.add(cookie);

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
    cookies.remove("session");
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
) -> Template {
    let user_store = UserStore::new(db);
    let registration = form_data.into_inner();

    let error = match user_store.user_exists(&registration.username).await {
        Ok(exists) => {
            if exists {
                "Username already exists."
            } else {
                // Username does not exist. Insert user into the database.
                match user_store
                    .add_user(&registration.username, &registration.password)
                    .await
                {
                    Ok(_) => "Registration successful.",
                    Err(_) => "Registation failed.",
                }
            }
        }
        Err(_) => "Database error.",
    };

    Template::render("register", context! { error })
}

#[catch(401)]
pub fn unauthorized() -> Flash<Redirect> {
    Flash::error(Redirect::to(uri!(login_form)), "Please login to continue")
}
