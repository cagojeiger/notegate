//! CIMD is fetched without credentials, proxies, redirects, or a second DNS lookup.
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use serde::Deserialize;
use url::Url;

#[derive(Clone,Debug,Deserialize)]
pub(super) struct ClientMetadata {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Vec<String>,
}

pub(super) async fn fetch(client_id: &str) -> Result<ClientMetadata, &'static str> {
    let url=Url::parse(client_id).map_err(|_|"invalid_client")?;
    if client_id.len()>2048 || url.scheme()!="https" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() || url.query().is_some() || url.port_or_known_default()!=Some(443) { return Err("invalid_client"); }
    let host=url.host_str().ok_or("invalid_client")?;
    let addresses:Vec<SocketAddr>=tokio::time::timeout(Duration::from_secs(3),tokio::net::lookup_host((host,443)))
        .await.map_err(|_|"invalid_client")?.map_err(|_|"invalid_client")?.take(17).collect();
    if addresses.is_empty() || addresses.len()>16 || addresses.iter().any(|a|!public_ip(a.ip())) { return Err("invalid_client"); }
    let http=reqwest::Client::builder().https_only(true).no_proxy().redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3)).timeout(Duration::from_secs(5)).resolve_to_addrs(host,&addresses).build().map_err(|_|"invalid_client")?;
    let mut response=http.get(url).header("accept","application/json").send().await.map_err(|_|"invalid_client")?;
    if !response.status().is_success() || response.content_length().is_some_and(|n|n>65536) { return Err("invalid_client"); }
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|_|"invalid_client")? {
        if bytes.len().saturating_add(chunk.len())>65536 { return Err("invalid_client"); }
        bytes.extend_from_slice(&chunk);
    }
    let metadata:ClientMetadata=serde_json::from_slice(&bytes).map_err(|_|"invalid_client")?;
    if metadata.client_id!=client_id || metadata.client_name.is_empty() || metadata.client_name.len()>200 || metadata.redirect_uris.is_empty() || metadata.redirect_uris.len()>20 { return Err("invalid_client"); }
    if metadata.token_endpoint_auth_method.as_deref().is_some_and(|m|m!="none") && !metadata.token_endpoint_auth_methods_supported.iter().any(|m|m=="none") { return Err("invalid_client"); }
    Ok(metadata)
}

impl ClientMetadata {
    pub fn allows_redirect(&self, redirect: &str) -> bool {
        if redirect.len()>2048 { return false; }
        let Ok(url)=Url::parse(redirect) else { return false; };
        if url.fragment().is_some() || !url.username().is_empty() || url.password().is_some() { return false; }
        let loopback=matches!(url.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback()) || matches!(url.host(),Some(url::Host::Ipv6(ip)) if ip.is_loopback()) || url.host_str()==Some("localhost");
        if url.scheme()!="https" && !(url.scheme()=="http" && loopback) { return false; }
        // Exact redirects, including the port. Native clients can publish several callbacks.
        self.redirect_uris.iter().any(|registered|registered==redirect)
    }
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip)=> {
            let [a,b,c,_]=ip.octets();
            !(a==0 || a==10 || a==127 || a>=224 || (a==100 && (64..=127).contains(&b)) || (a==169 && b==254) || (a==172 && (16..=31).contains(&b)) || (a==192 && (b==168 || (b==0 && (c==0 || c==2)) || (b==88 && c==99))) || (a==198 && (b==18 || b==19 || (b==51 && c==100))) || (a==203 && b==0 && c==113) || ip.octets()==[168,63,129,16])
        },
        IpAddr::V6(ip)=> {
            let [first,second,_,_,_,_,_,_]=ip.segments();
            (first&0xe000)==0x2000 && !(first==0x2001 && (second<0x0200 || second==0x0db8)) && first!=0x2002 && !(first==0x3fff && second<0x1000)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_nonpublic_destinations() {
        for ip in ["127.0.0.1","10.1.2.3","169.254.169.254","100.100.100.200","168.63.129.16","192.0.2.1","::1","::ffff:127.0.0.1","fc00::1","fe80::1","2001:db8::1","2002:7f00:1::"] {
            assert!(!super::public_ip(ip.parse().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))));
        }
        assert!(super::public_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::new(8,8,8,8))));
    }
}
