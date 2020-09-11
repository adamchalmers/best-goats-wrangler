extern crate cfg_if;
extern crate wasm_bindgen;

mod auth;
mod extensions;
mod models;
mod templates;
mod utils;

use crate::extensions::*;
use crate::models::*;
use cfg_if::cfg_if;
use handlebars::Handlebars;
use http::StatusCode;
use js_sys::{Array, Promise};
use lazy_static::lazy_static;
use serde::Serialize;
use url::Url;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{future_to_promise as ftp, JsFuture};
use web_sys::{
    FetchEvent, FormData, Headers, Request, Response, ResponseInit, ServiceWorkerGlobalScope,
};

cfg_if! {
    // When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
    // allocator.
    if #[cfg(feature = "wee_alloc")] {
        extern crate wee_alloc;
        #[global_allocator]
        static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
    }
}

type JsResult = Result<JsValue, JsValue>;

static BASE_LAYOUT_TEMPLATE: &'static str = "BASE_LAYOUT";
static ERROR_PAGE_TEMPLATE: &'static str = "ERROR_PAGE";
static FAVORITES_PAGE_TEMPLATE: &'static str = "FAVORITES_PAGE";
static GOAT_LIST_PARTIAL_TEMPLATE: &'static str = "GOAT_LIST_PARTIAL";
static HOME_PAGE_TEMPLATE: &'static str = "HOME_PAGE";
static DEFAULT_TITLE: &'static str = "The Best Goats";

lazy_static! {
    static ref HBARS: Handlebars = {
        let mut reg = Handlebars::new();

        assert!(reg
            .register_template_string(BASE_LAYOUT_TEMPLATE, templates::base::BASE_LAYOUT)
            .is_ok());
        assert!(reg
            .register_template_string(ERROR_PAGE_TEMPLATE, templates::error::ERROR_PAGE)
            .is_ok());
        assert!(reg
            .register_template_string(
                FAVORITES_PAGE_TEMPLATE,
                templates::favorites::FAVORITES_PAGE
            )
            .is_ok());
        assert!(reg
            .register_template_string(
                GOAT_LIST_PARTIAL_TEMPLATE,
                templates::goat_list::GOAT_LIST_PARTIAL
            )
            .is_ok());
        assert!(reg
            .register_template_string(HOME_PAGE_TEMPLATE, templates::home::HOME_PAGE)
            .is_ok());

        reg
    };
}

// The Cloudflare Workers environment will bind your Workers KV namespaces to
// the name "GoatsNs". This is configured in `wrangler.toml`. When your worker
// is run on the Cloudflare edge, there'll be functions called GoatsNs.get,
// GoatsNs.put and GoatsNs.delete, in the top-level JS namespace. This `extern`
// block just tells Rust that those functions will be there at runtime.
#[wasm_bindgen]
extern "C" {
    type GoatsNs;

    #[wasm_bindgen(static_method_of = GoatsNs)]
    fn get(key: &str, data_type: &str) -> Promise;

    #[wasm_bindgen(static_method_of = GoatsNs)]
    fn put(key: &str, val: &str) -> Promise;

    #[wasm_bindgen(static_method_of = GoatsNs)]
    fn delete(key: &str) -> Promise;
}

#[wasm_bindgen]
extern "C" {
    type FavoritesNs;

    #[wasm_bindgen(static_method_of = FavoritesNs)]
    fn get(key: &str, data_type: &str) -> Promise;

    #[wasm_bindgen(static_method_of = FavoritesNs)]
    fn put(key: &str, val: &str) -> Promise;

    #[wasm_bindgen(static_method_of = FavoritesNs)]
    fn delete(key: &str) -> Promise;
}

#[derive(Serialize)]
pub struct GoatListItem {
    pub id: GoatId,
    pub name: String,
    pub image: String,
    pub image_small: String,
    pub is_favorite: bool,
}

fn render_error(status: StatusCode) -> Promise {
    match generate_error_response(status, None) {
        Ok(v) => Promise::resolve(&v),
        Err(e) => Promise::reject(&e),
    }
}

fn generate_error_response(status: StatusCode, msg: Option<&str>) -> JsResult {
    #[derive(Serialize)]
    struct Data {
        title: String,
        parent: &'static str,
        show_favorites: bool,
        error_message: String,
    }
    let status_error_msg = format!(
        "{} {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or("Unknown Error")
    );
    let error_message = match msg {
        Some(v) => v.to_owned(),
        None => status_error_msg.to_owned(),
    };
    let data = Data {
        title: format!("{} - {}", &status_error_msg, DEFAULT_TITLE),
        parent: BASE_LAYOUT_TEMPLATE,
        show_favorites: false,
        error_message,
    };

    let body = HBARS.render(ERROR_PAGE_TEMPLATE, &data).ok_or_js_err()?;

    let headers = Headers::new()?;
    headers.append("content-type", "text/html")?;
    let resp = generate_response(&body, status.as_u16(), &headers)?;
    Ok(JsValue::from(resp))
}

fn generate_response(body: &str, status: u16, headers: &Headers) -> Result<Response, JsValue> {
    let mut init = ResponseInit::new();
    init.status(status);
    init.headers(&JsValue::from(headers));
    Response::new_with_opt_str_and_init(Some(body), &init)
}

async fn get_favorites_from_user_id(user_id: &Option<String>) -> JsResult {
    let js_fut = match user_id {
        Some(sid) => JsFuture::from(FavoritesNs::get(&sid, "json")),
        None => JsFuture::from(Promise::resolve(&JsValue::from(Array::new()))),
    };
    js_fut.await
}

async fn get_featured_goats() -> Result<Vec<Goat>, JsValue> {
    let promise = GoatsNs::get("featured", "arrayBuffer");
    let val = JsFuture::from(promise)
        .await
        .ok_or_js_err_with_msg("couldn't load Goats from Workers KV")?;
    let typebuf: js_sys::Uint8Array = js_sys::Uint8Array::new(&val);
    let mut body = vec![0; typebuf.length() as usize];
    typebuf.copy_to(&mut body[..]);

    let all_goats: Vec<Goat> = rmp_serde::from_read_ref(&body).ok_or_js_err()?;
    Ok(all_goats)
}

async fn render_home(request: Request) -> JsResult {
    let user_id = auth::get_user_id(&request);
    console_logf!("Rendering home for user {:?}", user_id);
    let favorites_value = get_favorites_from_user_id(&user_id).await?;
    let favorites: Vec<GoatId> = favorites_value.into_serde().ok_or_js_err()?;
    let all_goats = get_featured_goats().await?;

    #[derive(Serialize)]
    struct Data {
        title: &'static str,
        parent: &'static str,
        goat_list_template: &'static str,
        show_favorites: bool,
        fav_count: usize,
        goats: Vec<GoatListItem>,
    }
    let data = Data {
        title: DEFAULT_TITLE,
        parent: BASE_LAYOUT_TEMPLATE,
        goat_list_template: GOAT_LIST_PARTIAL_TEMPLATE,
        show_favorites: true,
        fav_count: favorites.len(),
        goats: get_goat_list_items(all_goats, &favorites),
    };
    let body = HBARS.render(HOME_PAGE_TEMPLATE, &data).ok_or_js_err()?;

    let headers = Headers::new()?;
    headers.append("content-type", "text/html")?;
    let resp = generate_response(&body, 200, &headers)?;

    Ok(JsValue::from(resp))
}

fn get_goat_list_items(goats: Vec<Goat>, favorites: &Vec<GoatId>) -> Vec<GoatListItem> {
    goats
        .into_iter()
        .map(|goat| {
            let is_favorite = favorites.contains(&goat.id);
            GoatListItem {
                id: goat.id,
                name: goat.name,
                image: goat.image,
                image_small: goat.image_small,
                is_favorite,
            }
        })
        .collect()
}

fn proxy_image(path: String) -> Promise {
    let url = format!("https://storage.googleapis.com/best_goats{}", path);
    let request = match Request::new_with_str(&url) {
        Ok(v) => v,
        Err(e) => return Promise::reject(&e),
    };

    match js_sys::global().dyn_into::<ServiceWorkerGlobalScope>() {
        Ok(scope) => scope.fetch_with_request(&request),
        Err(e) => Promise::reject(&e),
    }
}

enum FavoritesAction {
    Add,
    Remove,
}

// Returns the referrer URL if there is one, otherwise
// returns the URL of the request
fn get_referrer_or_orig_url(req: &Request) -> String {
    let req_headers = req.headers();
    match req_headers.get("referer") {
        Ok(Some(v)) => v,
        _ => req.url(),
    }
}

fn generate_redirect_headers(url: &str) -> Result<Headers, JsValue> {
    let headers = Headers::new()?;
    headers.set("location", url)?;
    Ok(headers)
}

async fn modify_favorites(
    event: FetchEvent,
    modification: FavoritesAction,
) -> Result<JsValue, JsValue> {
    let req = &event.request();
    let orig_user_id = auth::get_user_id(&req);
    let redirect_url = get_referrer_or_orig_url(&req);
    let form_data_fut = match req
        .form_data()
        .ok_or_js_err_with_msg("failed to get form_data")
    {
        Ok(v) => v,
        Err(e) => Promise::reject(&e),
    };
    let form_data_value = JsFuture::from(form_data_fut).await?;
    let favorites_value = get_favorites_from_user_id(&orig_user_id).await?;

    let form_data: FormData = form_data_value.dyn_into()?;
    let mut favorites: Vec<GoatId> = favorites_value.into_serde().ok_or_js_err()?;
    let goat_id_str: String = match form_data.get("id").as_string() {
        Some(v) => v,
        None => {
            return generate_error_response(StatusCode::BAD_REQUEST, Some("Missing id parameter"));
        }
    };
    let goat_id: GoatId = match goat_id_str.parse() {
        Ok(v) => v,
        Err(_e) => {
            return generate_error_response(StatusCode::BAD_REQUEST, Some("Invalid id parameter"));
        }
    };

    let modified = match modification {
        FavoritesAction::Add => {
            if !favorites.contains(&goat_id) {
                favorites.insert(0, goat_id);
                true
            } else {
                false
            }
        }
        FavoritesAction::Remove => {
            if let Some(idx) = favorites.iter().position(|x| *x == goat_id) {
                favorites.remove(idx);
                true
            } else {
                false
            }
        }
    };

    if !modified {
        let headers = generate_redirect_headers(&redirect_url)?;
        let resp = generate_response("", 302, &headers)?;
        return Ok(JsValue::from(&resp));
    }

    let new_user_id = random_str();
    let favorites_json = serde_json::to_string(&favorites).ok_or_js_err()?;
    let update_favorites_promise: Promise = FavoritesNs::put(&new_user_id, &favorites_json);
    let update_favorites_value = JsFuture::from(update_favorites_promise).await;
    match update_favorites_value {
        Ok(_v) => {
            let headers = generate_redirect_headers(&redirect_url)?;
            let cookie = auth::user_id_cookie(new_user_id);
            headers.set("set-cookie", &cookie.to_string())?;

            // Delete the old user_id favorites from KV
            if let Some(uid) = orig_user_id {
                let delete_old_favorites_promise = FavoritesNs::delete(&uid);
                event.wait_until(&delete_old_favorites_promise)?;
            }
            let resp = &generate_response("", 302, &headers)?;
            Ok(JsValue::from(resp))
        }
        Err(_e) => generate_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some("Error updating favorites"),
        ),
    }
}

fn random_str() -> String {
    use rand::distributions::Alphanumeric;
    use rand::{thread_rng, Rng};

    thread_rng().sample_iter(&Alphanumeric).take(30).collect()
}

async fn render_favorites(req: Request) -> Result<JsValue, JsValue> {
    let user_id = auth::get_user_id(&req);
    let favorites_value = get_favorites_from_user_id(&user_id).await?;
    let favorites: Vec<GoatId> = favorites_value.into_serde().ok_or_js_err()?;
    let all_goats = get_featured_goats().await?;

    let favorite_goats = all_goats
        .into_iter()
        .filter(|goat| favorites.contains(&goat.id))
        .collect();

    #[derive(Serialize)]
    struct Data {
        title: String,
        parent: &'static str,
        goat_list_template: &'static str,
        show_favorites: bool,
        fav_count: usize,
        has_favorites: bool,
        goats: Vec<GoatListItem>,
    }
    let data = Data {
        title: format!("{} - {}", "Favorites", DEFAULT_TITLE),
        parent: BASE_LAYOUT_TEMPLATE,
        goat_list_template: GOAT_LIST_PARTIAL_TEMPLATE,
        show_favorites: true,
        fav_count: favorites.len(),
        has_favorites: favorites.len() > 0,
        goats: get_goat_list_items(favorite_goats, &favorites),
    };
    let body = HBARS
        .render(FAVORITES_PAGE_TEMPLATE, &data)
        .ok_or_js_err()?;

    let headers = Headers::new()?;
    headers.append("content-type", "text/html")?;
    let headers = Headers::new()?;
    headers.append("content-type", "text/html")?;
    let resp = generate_response(&body, 200, &headers)?;

    Ok(JsValue::from(resp))
}

#[wasm_bindgen]
pub fn main(event: FetchEvent) -> Promise {
    let req = event.request();
    let url = match Url::parse(&req.url()).ok_or_js_err() {
        Ok(v) => v,
        Err(e) => return Promise::reject(&e),
    };
    let path = url.path().to_lowercase();
    let method = req.method().to_lowercase();
    let not_allowed = || render_error(StatusCode::METHOD_NOT_ALLOWED);

    match path.split("/").nth(1) {
        Some("") => match method.as_ref() {
            "get" => ftp(render_home(req)),
            _ => not_allowed(),
        },
        Some("favorites") => match method.as_ref() {
            "get" => ftp(render_favorites(req)),
            _ => not_allowed(),
        },
        Some("add-favorite") => match method.as_ref() {
            "post" => ftp(modify_favorites(event, FavoritesAction::Add)),
            _ => not_allowed(),
        },
        Some("remove-favorite") => match method.as_ref() {
            "post" => ftp(modify_favorites(event, FavoritesAction::Remove)),
            _ => not_allowed(),
        },
        Some("images") => match method.as_ref() {
            "get" => proxy_image(path),
            _ => not_allowed(),
        },
        _ => render_error(StatusCode::NOT_FOUND),
    }
}
