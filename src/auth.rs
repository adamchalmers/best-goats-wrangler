use crate::console_logf;
use cookie::Cookie;
use time::Duration;
use web_sys::Request;

static USER_ID_COOKIE: &'static str = "user_id";

/// Extract the user ID from the user's cookie, if one exists.
pub fn get_user_id(req: &Request) -> Option<String> {
    let headers = req.headers();
    console_logf!("{:?}", headers);
    let cookie_header = match headers.get("cookie") {
        Ok(Some(v)) => v,
        _ => return None,
    };
    for cookie_str in cookie_header.split(';').map(|s| s.trim()) {
        if let Ok(c) = Cookie::parse(cookie_str) {
            if c.name() == USER_ID_COOKIE {
                return Some(c.value().to_owned());
            }
        }
    }
    None
}

/// Make a new cookie containing the user ID.
pub fn user_id_cookie(new_user_id: String) -> Cookie<'static> {
    Cookie::build(USER_ID_COOKIE, new_user_id)
        .http_only(true)
        .secure(true)
        .max_age(Duration::days(365 * 20))
        .finish()
}
