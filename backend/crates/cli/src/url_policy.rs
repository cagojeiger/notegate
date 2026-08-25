use reqwest::Url;
use url::Host;

use crate::error::CliError;

pub(crate) fn canonical_origin(input: &str, name: &str) -> Result<String, CliError> {
    let url = Url::parse(input).map_err(|_error| invalid_base_origin(name))?;
    if !is_origin(&url) || !uses_secure_or_loopback_transport(&url) {
        return Err(invalid_base_origin(name));
    }
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

pub(crate) fn is_origin(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && matches!(url.path(), "" | "/")
}

pub(crate) fn uses_secure_or_loopback_transport(url: &Url) -> bool {
    match (url.scheme(), url.host()) {
        ("https", Some(_)) => true,
        ("http", Some(Host::Domain(host))) => host.eq_ignore_ascii_case("localhost"),
        ("http", Some(Host::Ipv4(host))) => host.is_loopback(),
        ("http", Some(Host::Ipv6(host))) => host.is_loopback(),
        _ => false,
    }
}

pub(crate) fn invalid_base_origin(name: &str) -> CliError {
    CliError::configuration(
        "invalid_base_url",
        format!("{name} must contain only an HTTPS origin, or a localhost/loopback HTTP origin"),
    )
}
