// Part of the APK.audio project — http://APK.audio — made by Anthony Kuzub
// MIT Licence. Free, for everyone, for ever. Full text in LICENSE at the root.
//! The one HTTP GET, and what its status means.

use std::time::Duration;

use crate::Oui;

pub(super) enum Fetched {
    Vendor(String),
    Unassigned,
    Throttled,
    Failed,
}

/// One GET. The API answers with a bare string, or 404, or 429.
pub(super) fn fetch(oui: Oui) -> Fetched {
    let url = format!("https://api.macvendors.com/{}", oui.as_query());
    match ureq::get(&url)
        .timeout(Duration::from_secs(10))
        .call()
    {
        Ok(response) => match response.into_string() {
            Ok(body) => {
                let name = body.trim().to_string();
                if name.is_empty() {
                    Fetched::Unassigned
                } else {
                    Fetched::Vendor(name)
                }
            }
            Err(_) => Fetched::Failed,
        },
        // 404 is an answer: no such assignment. Anything else in 4xx/5xx is not.
        Err(ureq::Error::Status(404, _)) => Fetched::Unassigned,
        Err(ureq::Error::Status(429, _)) => Fetched::Throttled,
        Err(_) => Fetched::Failed,
    }
}
