mod controllers;
mod models;
mod store;

#[macro_use]
extern crate rocket;

use csv_db::Database;
use rocket::{fs::FileServer, tokio::sync::Mutex};
use rocket_dyn_templates::Template;

use controllers::*;

#[launch]
fn rocket() -> _ {
    rocket::build()
        .mount(
            "/",
            routes![
                bet_form,
                bet_submit,
                history,
                index,
                latest,
                login_form,
                login_submit,
                logout,
                register_form,
                register_submit,
                rules,
            ],
        )
        .register("/", catchers![unauthorized])
        .attach(Template::fairing())
        .manage(Mutex::new(Database::new("data", None)))
        .mount("/static", FileServer::from("./static"))
}
