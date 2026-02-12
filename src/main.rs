mod api;
mod controllers;
mod models;
mod store;

#[macro_use]
extern crate rocket;

use csv_db::Database;
use rocket::{fs::FileServer, tokio::sync::Mutex};
use rocket_dyn_templates::Template;

use api::*;
use controllers::*;

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/",
            routes![
                disclaimer,
                history,
                index,
                latest,
                login_form,
                login_submit,
                logout,
                play_form,
                play_submit,
                profile_form,
                profile_submit,
                register_form,
                register_submit,
                rules,
            ],
        )
        .mount("/api", routes![leaderboard])
        .register("/", catchers![unauthorized])
        .attach(Template::fairing())
        .manage(Mutex::new(Database::new("data", None)))
        .mount("/static", FileServer::from("./static"))
}
