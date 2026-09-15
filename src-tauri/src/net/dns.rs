//! Host lookups that do not rest on the phone's resolver alone.
//!
//! Android looks names up for an app through the phone's DNS: the network's, a
//! VPN's or a Private DNS server's. When that gives no answer, every request
//! fails with "No address associated with hostname", even though a connection
//! to an address would go through. A lookup the system cannot answer is then
//! asked of a public DNS-over-HTTPS resolver, reached by its IP address so that
//! the query needs no DNS of its own. It carries only the host name, and
//! nothing is asked of it while the system's resolver answers.
//!
//! The engine runs in a process of its own and has the same fallback in
//! `sitecustomize.py`; [`prefer_fallback`] is how it learns that the phone's
//! resolver has stopped answering, so that it does not wait on it again.
//!
//! Only the Android build installs [`FallbackResolver`]. The module is compiled
//! everywhere so that it is tested everywhere.

#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures_util::stream::{FuturesUnordered, StreamExt};
use once_cell::sync::Lazy;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::ACCEPT;
use serde_json::Value;

use crate::log_info;

/// Asked in parallel, and the first answer is used. Two operators, so that one
/// being unreachable from a network does not stop the fallback. The engine's
/// copy of this list is in `sitecustomize.py`.
const RESOLVERS: [&str; 2] = ["https://1.1.1.1/dns-query", "https://8.8.8.8/resolve"];

/// How long a resolver has to answer. A network that silently drops this app's
/// traffic would otherwise hold the request for the whole connect timeout.
const TIMEOUT: Duration = Duration::from_secs(3);

/// How long the public resolvers are asked first once the system's failed where
/// they answered. A resolver that does not answer can take seconds to say so,
/// and would otherwise be waited on again for every new host.
const PREFER_FALLBACK_FOR: Duration = Duration::from_secs(5 * 60);

/// Answers kept at most; the expired ones are dropped when there are this many.
const ANSWER_LIMIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Record {
    A = 1,
    Aaaa = 28,
}

/// Addresses the public resolvers gave, by name and record type, with when
/// they expire.
type Answers = HashMap<(String, Record), (Instant, Vec<IpAddr>)>;

static ANSWERS: Lazy<Mutex<Answers>> = Lazy::new(|| Mutex::new(HashMap::new()));

static SYSTEM_FAILED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Reaches the resolvers by their addresses, so it looks nothing up itself. A
/// proxy in the settings is for reaching media and is not used for this.
static CLIENT: Lazy<Result<reqwest::Client, String>> = Lazy::new(|| {
    reqwest::Client::builder()
        .use_rustls_tls()
        .no_proxy()
        .timeout(TIMEOUT)
        .build()
        .map_err(|err| format!("the lookup client could not be created: {err}"))
});

/// Whether lookups have lately failed on the system's resolver where a public
/// one answered them.
pub fn prefer_fallback() -> bool {
    SYSTEM_FAILED_AT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some_and(|at| at.elapsed() < PREFER_FALLBACK_FOR)
}

/// The system's resolver, with the public ones behind it.
pub struct FallbackResolver;

impl Resolve for FallbackResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let (found, covered) = resolve_with(&host, system_lookup, lookup).await?;
            if let Some(failure) = covered {
                log_info!("dns", "the system could not look up {host} ({failure}); a public resolver did");
            }
            Ok(Box::new(found.into_iter()) as Addrs)
        })
    }
}

async fn system_lookup(host: String) -> Result<Vec<IpAddr>, String> {
    tokio::net::lookup_host((host.as_str(), 0))
        .await
        .map(|found| found.map(|address| address.ip()).collect())
        .map_err(|err| err.to_string())
}

/// Look `host` up on the system's resolver and, when that finds nothing, on
/// the public ones. Also returns the system's failure when a public resolver
/// covered for it. Both lookups are parameters, so the order can be tested
/// without a network that fails on cue.
async fn resolve_with<S, SF, P, PF>(
    host: &str,
    system: S,
    public: P,
) -> Result<(Vec<SocketAddr>, Option<String>), String>
where
    S: FnOnce(String) -> SF,
    SF: Future<Output = Result<Vec<IpAddr>, String>>,
    P: Fn(String) -> PF,
    PF: Future<Output = Result<Vec<IpAddr>, String>>,
{
    let sockets = |found: Vec<IpAddr>| -> Vec<SocketAddr> {
        found.into_iter().map(|ip| SocketAddr::new(ip, 0)).collect()
    };
    let name = public_name(host);

    if let Some(name) = name.clone().filter(|_| prefer_fallback()) {
        if let Ok(found) = public(name).await {
            if !found.is_empty() {
                return Ok((sockets(found), None));
            }
        }
    }

    let failure = match system(host.to_string()).await {
        Ok(found) if !found.is_empty() => return Ok((sockets(found), None)),
        Ok(_) => "no address".to_string(),
        Err(err) => err,
    };
    let Some(name) = name else {
        return Err(failure);
    };

    match public(name).await {
        Ok(found) if !found.is_empty() => {
            *SYSTEM_FAILED_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            Ok((sockets(found), Some(failure)))
        }
        Ok(_) => Err(failure),
        Err(reason) => Err(format!("{failure}; backup lookup: {reason}")),
    }
}

/// The name a public resolver could know, normalised. An address needs no
/// lookup, and a name only the local network knows -- `localhost`, a printer,
/// anything under `.local` -- is not sent out.
fn public_name(host: &str) -> Option<String> {
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    let local = !name.contains('.') || name.ends_with(".localhost") || name.ends_with(".local");
    (!local && name.parse::<IpAddr>().is_err()).then_some(name)
}

/// Addresses for `name` from the public resolvers: its IPv4 ones, or the IPv6
/// ones of a name that has none.
pub async fn lookup(name: String) -> Result<Vec<IpAddr>, String> {
    let found = ask(&name, Record::A).await?;
    if !found.is_empty() {
        return Ok(found);
    }
    ask(&name, Record::Aaaa).await
}

async fn ask(name: &str, record: Record) -> Result<Vec<IpAddr>, String> {
    let key = (name.to_string(), record);
    let cached = {
        let answers = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
        answers
            .get(&key)
            .filter(|(expires, _)| *expires > Instant::now())
            .map(|(_, found)| found.clone())
    };
    if let Some(found) = cached {
        return Ok(found);
    }

    let client = CLIENT.as_ref().map_err(Clone::clone)?;
    let mut replies: FuturesUnordered<_> = RESOLVERS
        .iter()
        .map(|endpoint| query(client, endpoint, name, record))
        .collect();

    let mut failures = Vec::new();
    while let Some(reply) = replies.next().await {
        match reply {
            Ok((found, ttl)) => {
                let mut answers = ANSWERS.lock().unwrap_or_else(|e| e.into_inner());
                if answers.len() >= ANSWER_LIMIT {
                    let now = Instant::now();
                    answers.retain(|_, (expires, _)| *expires > now);
                }
                answers.insert(key, (Instant::now() + ttl, found.clone()));
                return Ok(found);
            }
            Err(failure) => failures.push(failure),
        }
    }
    Err(failures.join(", "))
}

async fn query(
    client: &reqwest::Client,
    endpoint: &str,
    name: &str,
    record: Record,
) -> Result<(Vec<IpAddr>, Duration), String> {
    let kind = (record as u16).to_string();
    let reply = async {
        let response = client
            .get(endpoint)
            .query(&[("name", name), ("type", kind.as_str())])
            .header(ACCEPT, "application/dns-json")
            .send()
            .await
            .map_err(|err| root_cause(&err))?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status().as_u16()));
        }
        let body: Value = response.json().await.map_err(|err| root_cause(&err))?;
        parse_answer(&body, record)
    };

    let server = endpoint
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(endpoint);
    reply.await.map_err(|reason| format!("{server} {reason}"))
}

/// The innermost cause of an error: "Network is unreachable" rather than
/// "error sending request for url (...)".
fn root_cause(err: &(dyn std::error::Error + 'static)) -> String {
    let mut cause = err;
    while let Some(source) = cause.source() {
        cause = source;
    }
    cause.to_string()
}

/// The addresses of `record`'s type in a JSON DNS answer, and how long they may
/// be kept. A name that does not exist is an answer too: one with no addresses.
fn parse_answer(body: &Value, record: Record) -> Result<(Vec<IpAddr>, Duration), String> {
    match body.get("Status").and_then(Value::as_u64) {
        Some(0) => {}
        Some(3) => return Ok((Vec::new(), Duration::from_secs(300))),
        Some(status) => return Err(format!("DNS status {status}")),
        None => return Err("not a DNS answer".into()),
    }

    let mut found = Vec::new();
    let mut ttl = 600;
    for entry in body.get("Answer").and_then(Value::as_array).into_iter().flatten() {
        // Aliases come first in an answer; only the addresses they lead to
        // are wanted.
        if entry.get("type").and_then(Value::as_u64) != Some(record as u64) {
            continue;
        }
        let Some(address) = entry
            .get("data")
            .and_then(Value::as_str)
            .and_then(|data| data.parse::<IpAddr>().ok())
            .filter(|address| address.is_ipv4() == (record == Record::A))
        else {
            continue;
        };
        found.push(address);
        ttl = ttl.min(entry.get("TTL").and_then(Value::as_u64).unwrap_or(60));
    }
    Ok((found, Duration::from_secs(ttl.max(30))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn an_answer_gives_the_addresses_its_aliases_lead_to() {
        // What 1.1.1.1 answered for a TikTok short link.
        let body = json!({
            "Status": 0,
            "Answer": [
                { "name": "vt.tiktok.com", "type": 5, "TTL": 556, "data": "vt.tiktok.com.edgesuite.net." },
                { "name": "vt.tiktok.com.edgesuite.net", "type": 5, "TTL": 21556, "data": "a1801.r.akamai.net." },
                { "name": "a1801.r.akamai.net", "type": 1, "TTL": 120, "data": "2.20.134.193" },
                { "name": "a1801.r.akamai.net", "type": 1, "TTL": 90, "data": "2.19.193.193" },
            ],
        });
        let (found, ttl) = parse_answer(&body, Record::A).unwrap();
        assert_eq!(found, [ip("2.20.134.193"), ip("2.19.193.193")]);
        assert_eq!(ttl, Duration::from_secs(90));
    }

    #[test]
    fn a_record_of_the_other_family_is_not_taken() {
        let body = json!({
            "Status": 0,
            "Answer": [
                { "type": 28, "TTL": 5, "data": "2606:4700::6810:84e5" },
                { "type": 28, "TTL": 5, "data": "104.16.132.229" },
                { "type": 28, "TTL": 5, "data": "not an address" },
            ],
        });
        let (found, ttl) = parse_answer(&body, Record::Aaaa).unwrap();
        assert_eq!(found, [ip("2606:4700::6810:84e5")]);
        // A short TTL is not taken as a reason to ask again for every request.
        assert_eq!(ttl, Duration::from_secs(30));
        assert!(parse_answer(&body, Record::A).unwrap().0.is_empty());
    }

    #[test]
    fn a_name_that_does_not_exist_is_an_answer_and_a_failure_is_not() {
        let (found, _) = parse_answer(&json!({ "Status": 3 }), Record::A).unwrap();
        assert!(found.is_empty());
        assert!(parse_answer(&json!({ "Status": 2 }), Record::A).is_err());
        assert!(parse_answer(&json!({ "error": "bad request" }), Record::A).is_err());
    }

    #[test]
    fn only_names_the_internet_could_know_are_sent_out() {
        assert_eq!(public_name("VT.TikTok.com."), Some("vt.tiktok.com".into()));
        for local in ["localhost", "printer", "nas.local", "app.localhost", "192.168.1.20", "::1", "2606:4700::1111"] {
            assert_eq!(public_name(local), None, "{local} would have been sent out");
        }
    }

    async fn unanswered(_: String) -> Result<Vec<IpAddr>, String> {
        Err("No address associated with hostname".into())
    }

    #[tokio::test]
    async fn a_public_resolver_answers_when_the_system_does_not() {
        let (found, covered) = resolve_with("vt.tiktok.com", unanswered, |name| async move {
            assert_eq!(name, "vt.tiktok.com");
            Ok(vec![ip("2.20.134.193")])
        })
        .await
        .unwrap();

        assert_eq!(found, [SocketAddr::new(ip("2.20.134.193"), 0)]);
        assert_eq!(covered.as_deref(), Some("No address associated with hostname"));
        assert!(prefer_fallback());

        // From now on the public resolvers are asked first, and the system
        // is not waited on.
        let (found, covered) = resolve_with(
            "www.tiktok.com",
            |_| async { panic!("the system's resolver was waited on again") },
            |_| async { Ok(vec![ip("23.58.223.169")]) },
        )
        .await
        .unwrap();
        assert_eq!(found, [SocketAddr::new(ip("23.58.223.169"), 0)]);
        assert_eq!(covered, None);
    }

    #[tokio::test]
    async fn when_both_fail_both_reasons_are_given() {
        let failure = resolve_with("vt.tiktok.com", unanswered, |_| async {
            Err("1.1.1.1 operation timed out, 8.8.8.8 operation timed out".to_string())
        })
        .await
        .unwrap_err();
        assert_eq!(
            failure,
            "No address associated with hostname; backup lookup: 1.1.1.1 operation timed out, 8.8.8.8 operation timed out"
        );

        // The public resolvers do not know the name either: the system's
        // answer is the one to report.
        let failure = resolve_with("gone.example", unanswered, |_| async { Ok(Vec::new()) })
            .await
            .unwrap_err();
        assert_eq!(failure, "No address associated with hostname");
    }

    #[tokio::test]
    async fn a_local_name_is_never_asked_of_a_public_resolver() {
        let failure = resolve_with("printer", unanswered, |name| async move {
            panic!("{name} was sent to a public resolver")
        })
        .await
        .unwrap_err();
        assert_eq!(failure, "No address associated with hostname");
    }

    #[tokio::test]
    #[ignore = "needs the internet"]
    async fn the_public_resolvers_find_a_real_host() {
        let found = lookup("vt.tiktok.com".into()).await.unwrap();
        assert!(!found.is_empty());
        // Asked again, the answer comes from what was kept.
        assert_eq!(lookup("vt.tiktok.com".into()).await.unwrap(), found);
    }
}
