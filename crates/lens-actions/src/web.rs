//! URLs, contacts, places and network identifiers.

use lens_core::action::{ActionGroup as G, Choice, Item};
use lens_core::builder::action;
use lens_core::host::HostFeatures as F;
use lens_core::registry::PluginRegistrar;
use lens_core::settings::fill_template;
use lens_core::value::{IpScope, IpValue, UrlValue};
use lens_core::{caps, ActionOutcome, Effects, LensError, Result, Value};

use crate::util::*;

fn url(i: &Item) -> Result<&UrlValue> {
    match &i.value {
        Value::Url(u) => Ok(u),
        _ => Err(LensError::InvalidInput("not a URL".into())),
    }
}

fn ip(i: &Item) -> Result<&IpValue> {
    match &i.value {
        Value::Ip(v) => Ok(v),
        _ => Err(LensError::InvalidInput("not an IP address".into())),
    }
}

pub fn vcard(name: Option<&str>, email: Option<&str>, phone: Option<&str>) -> String {
    let mut v = String::from("BEGIN:VCARD\r\nVERSION:3.0\r\n");
    let fn_ = name.or(email).or(phone).unwrap_or("New Contact");
    v.push_str(&format!("FN:{}\r\n", ical_escape(fn_)));
    if let Some(e) = email {
        v.push_str(&format!("EMAIL;TYPE=INTERNET:{e}\r\n"));
    }
    if let Some(p) = phone {
        v.push_str(&format!("TEL;TYPE=CELL:{p}\r\n"));
    }
    v.push_str("END:VCARD\r\n");
    v
}

/// Generated terminal commands are built from parsed, validated values
/// (never from raw OCR text), so they can run without a confirmation step.
fn terminal_cmd(cx: &lens_core::ActionContext, cmd: String) -> Result<ActionOutcome> {
    cx.host.terminal(None, Some(&cmd), true)?;
    Ok(ActionOutcome::done(format!("Running `{cmd}`")))
}

fn ping_cmd(host: &str) -> String {
    if cfg!(windows) {
        format!("ping {host}")
    } else {
        format!("ping -c 4 {host}")
    }
}

fn traceroute_cmd(host: &str) -> String {
    if cfg!(windows) {
        format!("tracert {host}")
    } else {
        format!("traceroute {host}")
    }
}

pub fn register(r: &mut PluginRegistrar) {
    // URL
    let u = caps::URL;
    r.action(
        action("core.url.open", "Open")
            .icon("external-link")
            .group(G::Open)
            .accepts(u.clone())
            .priority(95)
            .key('o')
            .effects(OPEN_WEB)
            .requires(F::OPEN_URI)
            .run(|i, cx| open(cx, &url(i)?.url)),
    );
    r.action(
        action("core.url.copy", "Copy URL")
            .icon("link")
            .group(G::Copy)
            .accepts(u.clone())
            .passthrough()
            .priority(90)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &url(i)?.url, "URL")),
    );
    r.action(
        action("core.url.private", "Open Private Window")
            .icon("incognito")
            .group(G::Open)
            .accepts(u.clone())
            .priority(50)
            .effects(OPEN_WEB)
            .requires(F::PRIVATE_BROWSING)
            .run(|i, cx| {
                cx.host.open_uri(&url(i)?.url, true)?;
                Ok(ActionOutcome::done("Opened privately"))
            }),
    );
    r.action(
        action("core.url.qr", "Generate QR")
            .icon("qr")
            .group(G::Transform)
            .accepts(u.clone())
            .produces(caps::IMAGE)
            .priority(60)
            .key('q')
            .run(|i, _| crate::data::qr_image(&url(i)?.url)),
    );
    r.action(
        action("core.url.download", "Download")
            .icon("download")
            .group(G::Save)
            .accepts(u.clone())
            .priority(40)
            .effects(Effects::NETWORK | SAVE)
            .requires(F::DOWNLOAD)
            .applies(|f, _| matches!(&f.value, Value::Url(u) if u.scheme == "https" || u.scheme == "http"))
            .run(|i, cx| {
                let p = cx.host.download(&url(i)?.url)?;
                Ok(ActionOutcome::done(format!("Downloaded to {}", p.display())))
            }),
    );
    r.action(
        action("core.url.send", "Send to Phone")
            .icon("smartphone")
            .group(G::Share)
            .accepts(u.clone())
            .priority(55)
            .effects(Effects::SENDS_TO_DEVICE)
            .requires(F::SEND_TO_DEVICE)
            .run(|i, cx| {
                cx.host.send_to_device(Some(&url(i)?.url), None)?;
                Ok(ActionOutcome::done("Sent to phone"))
            }),
    );
    r.action(
        action("core.url.copy-domain", "Copy Domain")
            .icon("globe")
            .group(G::Copy)
            .accepts(u)
            .priority(45)
            .effects(COPY)
            .run(|i, cx| copy(cx, url(i)?.host.as_deref().unwrap_or_default(), "domain")),
    );

    // Email
    let e = caps::EMAIL;
    r.action(
        action("core.email.compose", "Compose Email")
            .icon("mail")
            .group(G::Open)
            .accepts(e.clone())
            .priority(92)
            .key('o')
            .effects(LAUNCH)
            .requires(F::OPEN_URI)
            .run(|i, cx| open(cx, &format!("mailto:{}", text_of(i)?))),
    );
    r.action(
        action("core.email.copy", "Copy Email")
            .icon("copy")
            .group(G::Copy)
            .accepts(e.clone())
            .priority(90)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "email")),
    );
    r.action(action("core.email.copy-domain", "Copy Domain").icon("globe").group(G::Copy).accepts(e.clone()).priority(40).effects(COPY).run(|i, cx| {
        let s = text_of(i)?;
        copy(cx, s.rsplit('@').next().unwrap_or_default(), "domain")
    }));
    r.action(
        action("core.email.contact", "Add Contact")
            .icon("user-plus")
            .group(G::Save)
            .accepts(e)
            .priority(45)
            .effects(SAVE | LAUNCH)
            .requires(F::SAVE_FILE | F::OPEN_PATH)
            .run(|i, cx| {
                let email = text_of(i)?;
                let out = save_text(cx, &format!("{}.vcf", slug(&email, 40)), &vcard(None, Some(&email), None), "text/vcard")?;
                open_saved(cx, out)
            }),
    );

    // Phone
    let p = caps::PHONE;
    let digits = |i: &Item| match &i.value {
        Value::Phone(p) => Ok(p.clone()),
        _ => Err(LensError::InvalidInput("not a phone number".into())),
    };
    r.action(
        action("core.phone.copy", "Copy Number")
            .icon("copy")
            .group(G::Copy)
            .accepts(p.clone())
            .priority(90)
            .key('c')
            .effects(COPY)
            .run(move |i, cx| copy(cx, &digits(i)?.raw, "number")),
    );
    r.action(
        action("core.phone.call", "Call")
            .icon("phone")
            .group(G::Open)
            .accepts(p.clone())
            .priority(85)
            .key('o')
            .effects(LAUNCH)
            .requires(F::OPEN_URI)
            .run(move |i, cx| open(cx, &digits(i)?.tel_uri())),
    );
    r.action(
        action("core.phone.call-on-phone", "Call on Phone")
            .icon("smartphone")
            .group(G::Share)
            .accepts(p.clone())
            .priority(70)
            .effects(Effects::SENDS_TO_DEVICE)
            .requires(F::SEND_TO_DEVICE)
            .run(move |i, cx| {
                cx.host.send_to_device(Some(&digits(i)?.tel_uri()), None)?;
                Ok(ActionOutcome::done("Sent to phone"))
            }),
    );
    r.action(
        action("core.phone.contact", "Create Contact")
            .icon("user-plus")
            .group(G::Save)
            .accepts(p)
            .priority(45)
            .effects(SAVE | LAUNCH)
            .requires(F::SAVE_FILE | F::OPEN_PATH)
            .run(move |i, cx| {
                let ph = digits(i)?;
                let out = save_text(cx, &format!("{}.vcf", slug(&ph.digits, 20)), &vcard(None, None, Some(&ph.digits)), "text/vcard")?;
                open_saved(cx, out)
            }),
    );

    // Address
    let a = caps::ADDRESS;
    r.action(
        action("core.address.maps", "Open Maps")
            .icon("map")
            .group(G::Open)
            .accepts(a.clone())
            .priority(90)
            .key('o')
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .preview(|i, _| i.value.as_text().map(|s| s.into_owned()))
            .run(|i, cx| open(cx, &fill_template(&cx.settings.providers.maps_search, &[("query", &text_of(i)?)]))),
    );
    r.action(
        action("core.address.copy", "Copy Address")
            .icon("copy")
            .group(G::Copy)
            .accepts(a.clone())
            .priority(88)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "address")),
    );
    r.action(
        action("core.address.search", "Search Address")
            .icon("search")
            .group(G::Search)
            .accepts(a)
            .priority(50)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .preview(|i, _| i.value.as_text().map(|s| s.into_owned()))
            .run(|i, cx| search(cx, &text_of(i)?)),
    );

    // Coordinates
    let g = caps::COORDINATES;
    r.action(
        action("core.geo.map", "Open Map").icon("map-pin").group(G::Open).accepts(g.clone()).priority(92).key('o').effects(SEARCH).requires(F::OPEN_URI).run(
            |i, cx| {
                let Value::Coordinates(c) = &i.value else { return Err(LensError::InvalidInput("not coordinates".into())) };
                let (lat, lon) = (format!("{:.6}", c.lat), format!("{:.6}", c.lon));
                open(cx, &fill_template(&cx.settings.providers.maps_coordinates, &[("lat", &lat), ("lon", &lon)]))
            },
        ),
    );
    r.action(
        action("core.geo.copy", "Copy Coordinates")
            .icon("copy")
            .group(G::Copy)
            .accepts(g)
            .priority(88)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "coordinates")),
    );

    // IP addresses
    let n = caps::IP_ADDRESS;
    let pingable =
        |f: &lens_core::Finding| matches!(&f.value, Value::Ip(v) if !matches!(v.scope, IpScope::Unspecified | IpScope::Multicast | IpScope::Documentation));
    r.action(
        action("core.ip.copy", "Copy IP")
            .icon("copy")
            .group(G::Copy)
            .accepts(n.clone())
            .priority(90)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &ip(i)?.display(), "IP address")),
    );
    r.action(
        action("core.ip.ping", "Ping")
            .icon("activity")
            .group(G::Inspect)
            .accepts(n.clone())
            .priority(70)
            .effects(Effects::NETWORK | LAUNCH)
            .requires(F::TERMINAL)
            .applies(move |f, _| pingable(f))
            .run(|i, cx| terminal_cmd(cx, ping_cmd(&ip(i)?.addr.to_string()))),
    );
    r.action(
        action("core.ip.http", "Open HTTP")
            .icon("external-link")
            .group(G::Open)
            .accepts(n.clone())
            .priority(60)
            .effects(OPEN_WEB)
            .requires(F::OPEN_URI)
            .applies(move |f, _| pingable(f))
            .run(|i, cx| open(cx, &format!("http://{}", ip(i)?.url_host()))),
    );
    r.action(
        action("core.ip.https", "Open HTTPS")
            .icon("lock")
            .group(G::Open)
            .accepts(n.clone())
            .priority(62)
            .effects(OPEN_WEB)
            .requires(F::OPEN_URI)
            .applies(move |f, _| pingable(f))
            .run(|i, cx| open(cx, &format!("https://{}", ip(i)?.url_host()))),
    );
    r.action(
        action("core.ip.ssh", "SSH")
            .icon("terminal")
            .group(G::Open)
            .accepts(n.clone())
            .priority(55)
            .effects(Effects::NETWORK | LAUNCH)
            .requires(F::TERMINAL)
            .applies(move |f, _| pingable(f))
            .run(|i, cx| {
                let v = ip(i)?;
                cx.host.terminal(None, Some(&format!("ssh {}", v.addr)), false)?;
                Ok(ActionOutcome::done("SSH command ready in terminal"))
            }),
    );
    r.action(
        action("core.ip.traceroute", "Traceroute")
            .icon("route")
            .group(G::Inspect)
            .accepts(n.clone())
            .priority(40)
            .effects(Effects::NETWORK | LAUNCH)
            .requires(F::TERMINAL)
            .applies(move |f, _| pingable(f))
            .run(|i, cx| terminal_cmd(cx, traceroute_cmd(&ip(i)?.addr.to_string()))),
    );
    r.action(
        action("core.ip.lookup", "Lookup")
            .icon("search")
            .group(G::Search)
            .accepts(n)
            .priority(45)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            // Private addresses mean nothing to public lookup services and reveal network layout.
            .applies(|f, _| matches!(&f.value, Value::Ip(v) if v.scope == IpScope::Public))
            .preview(|i, _| ip(i).ok().map(|v| v.addr.to_string()))
            .run(|i, cx| open(cx, &fill_template(&cx.settings.providers.whois, &[("query", &ip(i)?.addr.to_string())]))),
    );

    // Domains
    let d = caps::DOMAIN;
    r.action(
        action("core.domain.open", "Open")
            .icon("external-link")
            .group(G::Open)
            .accepts(d.clone())
            .priority(90)
            .key('o')
            .effects(OPEN_WEB)
            .requires(F::OPEN_URI)
            .run(|i, cx| open(cx, &format!("https://{}", text_of(i)?))),
    );
    r.action(
        action("core.domain.copy", "Copy Domain")
            .icon("copy")
            .group(G::Copy)
            .accepts(d.clone())
            .priority(85)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "domain")),
    );
    r.action(
        action("core.domain.dns", "DNS Lookup")
            .icon("server")
            .group(G::Inspect)
            .accepts(d.clone())
            .priority(50)
            .effects(Effects::NETWORK | LAUNCH)
            .requires(F::TERMINAL)
            .run(|i, cx| {
                let host = text_of(i)?;
                terminal_cmd(cx, if cfg!(windows) { format!("nslookup {host}") } else { format!("dig +short {host} A {host} AAAA {host} MX") })
            }),
    );
    r.action(
        action("core.domain.ping", "Ping")
            .icon("activity")
            .group(G::Inspect)
            .accepts(d.clone())
            .priority(45)
            .effects(Effects::NETWORK | LAUNCH)
            .requires(F::TERMINAL)
            .run(|i, cx| terminal_cmd(cx, ping_cmd(&text_of(i)?))),
    );
    r.action(
        action("core.domain.whois", "WHOIS")
            .icon("search")
            .group(G::Search)
            .accepts(d.clone())
            .priority(40)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .run(|i, cx| open(cx, &fill_template(&cx.settings.providers.whois, &[("query", &text_of(i)?)]))),
    );
    if cfg!(unix) {
        r.action(action("core.domain.tls", "TLS Info").icon("lock").group(G::Inspect).accepts(d).priority(35).effects(Effects::NETWORK | LAUNCH).requires(F::TERMINAL).run(|i, cx| {
            let h = text_of(i)?;
            terminal_cmd(cx, format!("echo | openssl s_client -connect {h}:443 -servername {h} 2>/dev/null | openssl x509 -noout -subject -issuer -dates -ext subjectAltName"))
        }));
    }

    // Currency
    let m = caps::CURRENCY;
    r.action(
        action("core.currency.copy", "Copy")
            .icon("copy")
            .group(G::Copy)
            .accepts(m.clone())
            .priority(80)
            .key('c')
            .effects(COPY)
            .run(|i, cx| copy(cx, &text_of(i)?, "amount")),
    );
    r.action(action("core.currency.copy-number", "Copy Number").icon("hash").group(G::Copy).accepts(m.clone()).priority(75).effects(COPY).run(|i, cx| {
        let Value::Currency(c) = &i.value else { return Err(LensError::InvalidInput("not currency".into())) };
        copy(cx, &crate::data::trim_float(c.amount), "number")
    }));
    r.action(
        action("core.currency.convert", "Convert Currency")
            .icon("repeat")
            .group(G::Search)
            .accepts(m)
            .priority(70)
            .effects(SEARCH)
            .requires(F::OPEN_URI)
            .choices(|i, s| {
                let Value::Currency(c) = &i.value else { return vec![] };
                let from = c.code.clone().unwrap_or_default();
                let mut targets: Vec<String> = s.home_currency.iter().cloned().collect();
                for t in ["USD", "EUR", "GBP", "INR", "JPY"] {
                    if !targets.iter().any(|x| x == t) {
                        targets.push(t.into());
                    }
                }
                targets.into_iter().filter(|t| *t != from).take(5).map(|t| Choice { label: format!("{from} → {t}"), value: serde_json::json!(t) }).collect()
            })
            .preview(|i, _| match &i.value {
                Value::Currency(c) => Some(format!("{} {}", crate::data::trim_float(c.amount), c.code.clone().unwrap_or_default())),
                _ => None,
            })
            .run(|i, cx| {
                // Exchange rates change constantly; the conversion is done by the configured search provider.
                let Value::Currency(c) = &i.value else { return Err(LensError::InvalidInput("not currency".into())) };
                let to = cx.param_str("choice").unwrap_or("USD");
                search(cx, &format!("{} {} to {to}", crate::data::trim_float(c.amount), c.code.clone().unwrap_or_default()))
            }),
    );
}

fn open_saved(cx: &lens_core::ActionContext, out: ActionOutcome) -> Result<ActionOutcome> {
    if let Some(Value::File(f)) = out.output.as_ref().map(|o| &o.value) {
        cx.host.open_path(&f.path, lens_core::host::OpenPathMode::Default)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vcards() {
        let v = vcard(None, Some("ada@example.com"), None);
        assert!(v.contains("FN:ada@example.com\r\n") && v.contains("EMAIL;TYPE=INTERNET:ada@example.com\r\n"));
    }
}
