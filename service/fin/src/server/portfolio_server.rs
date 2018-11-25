use crate::api;
use crate::data::{self, TickerBackend, UserBackend};
use crate::errors::{FinError, ResultFin};
use crate::portfolio::{self, Ticker, TickerId};
use rocket::http::Status;
use rocket::request::Form;
use rocket::request::{self, FromRequest, Request};
use rocket::response::status;
use rocket::Outcome;
use rocket::{http::Method, State};
use rocket_contrib::Json;
use rocket_cors::{AllowedHeaders, AllowedOrigins};
use std::ops::Deref;
use std::sync::RwLock;

use lru_time_cache::LruCache;
use postgres::{Connection, TlsMode};

use super::{CACHE_MAX_COUNT, CACHE_TTL, DB_URI};

pub struct UserAuth {
    user_id: i64,
}

impl<'a, 'r> FromRequest<'a, 'r> for UserAuth {
    type Error = ();

    fn from_request(
        request: &'a Request<'r>,
    ) -> request::Outcome<UserAuth, ()> {
        request.cookies().get("sess").map_or(
            Outcome::Failure((Status::Unauthorized, ())),
            |cookie| {
                if (cookie.value() == "") {
                    Outcome::Failure((Status::Unauthorized, ()))
                } else {
                    match cookie.value().parse::<i64>() {
                        Ok(id) => Outcome::Success(UserAuth { user_id: id }),
                        Err(_) => Outcome::Failure((Status::Unauthorized, ())),
                    }
                }
            },
        )
    }
}

#[get("/?<query>")]
pub(super) fn portfolio(
    query: api::PortfolioStateQuery,
    auth: UserAuth,
) -> ResultFin<String> {
    let conn = Connection::connect(DB_URI, TlsMode::None)
        .expect("cannot connect to postgres");

    let lru_cache = LruCache::<String, f32>::with_expiry_duration_and_capacity(
        CACHE_TTL,
        CACHE_MAX_COUNT,
    );
    let mut db = data::PgTickerDatabase {
        conn: conn,
        lru: lru_cache,
    };

    // get port
    let actual = db.get_actual(&auth.user_id, &query.goal_id)?;
    let goal_tickers = db.get_goal(&query.goal_id);
    let port_goal = db
        .get_port_goal(&query.goal_id)
        .unwrap()
        .to_port_goal(goal_tickers);
    let mut port = portfolio::Portfolio::new(&mut db, &actual, &port_goal);

    // get state
    let port_state = port.get_state();
    Ok(serde_json::to_string(&port_state).unwrap())
}

#[get("/buy?<query>")]
pub(super) fn get_buy_next<'r>(
    query: api::BuyNextQuery,
    auth: UserAuth,
) -> ResultFin<String> {
    let conn = Connection::connect(DB_URI, TlsMode::None)
        .expect("cannot connect to postgres");

    let lru_cache = LruCache::<String, f32>::with_expiry_duration_and_capacity(
        CACHE_TTL,
        CACHE_MAX_COUNT,
    );
    let mut db = data::PgTickerDatabase {
        conn: conn,
        lru: lru_cache,
    };

    // get port
    let actual = db.get_actual(&auth.user_id, &query.goal_id)?;

    debug!("amount to buy for: {}", query.amount);
    let resp = portfolio::Portfolio::get_buy_next(
        &mut db,
        &actual,
        query.amount,
        &query.goal_id,
    );
    let resp = api::BuyNextResp::from_data(resp, query.amount);

    Ok(serde_json::to_string(&resp).unwrap())
}

#[post("/buy", data = "<form>")]
pub(super) fn post_buy_next(
    form: Json<api::BuyNextForm>,
    auth: UserAuth,
) -> ResultFin<status::Created<String>> {
    let conn = Connection::connect(DB_URI, TlsMode::None)
        .expect("cannot connect to postgres");

    let lru_cache = LruCache::<String, f32>::with_expiry_duration_and_capacity(
        CACHE_TTL,
        CACHE_MAX_COUNT,
    );
    let mut db = data::PgTickerDatabase {
        conn: conn,
        lru: lru_cache,
    };

    let port = portfolio::Portfolio::execute_action(
        &mut db,
        &auth.user_id,
        &form.goal_id,
        &form.actions,
    );
    let port = port.unwrap().get_state();

    Ok(status::Created(
        "/portfolio?user_id=_&goal_id=_".to_string(),
        Some(serde_json::to_string(&port).unwrap()),
    ))
}
