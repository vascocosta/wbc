use csv_db::Database;
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

use crate::models::{Registration, User};
use crate::store::UserStore;

#[get("/")]
pub async fn index() -> Template {
    Template::render("index", context! {})
}

#[get("/bet")]
pub async fn bet(_user: User) -> Template {
    Template::render("bet", context! {})
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
) -> Template {
    let user_store = UserStore::new(db);
    let registration = form_data.into_inner();

    let error = match user_store
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

            "Login successful."
        }
        None => "Login failed.",
    };

    return Template::render("login", context! { error });
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
