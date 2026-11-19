//! Account credentials remain in memory. HTTP diagnostics include service and
//! status only. Device codes are delivered explicitly to the caller's UI.
#![forbid(unsafe_code)]

use horizon_protocol_47::login::{Profile, parse_uuid, start};
use md5::{Digest, Md5};
use serde_json::{Value, json};
use std::{
    fmt,
    io::{self, Read},
    time::{Duration, Instant},
};

pub const DEFAULT_CLIENT_ID: &str = "00000000441cc96b";
pub const DEFAULT_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const MAX_JSON: u64 = 1 << 20;

pub struct Session {
    pub profile: Profile,
    access_token: Option<String>,
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("profile", &self.profile)
            .field("online", &self.access_token.is_some())
            .finish_non_exhaustive()
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn field<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| invalid("Authentication response is missing a required field"))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .timeout_connect(Duration::from_secs(10))
        .redirects(0)
        .user_agent("Horizon-1.8.9/0.1")
        .build()
}

fn response(
    result: Result<ureq::Response, ureq::Error>,
    service: &str,
) -> io::Result<(u16, Value)> {
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(_)) => {
            return Err(io::Error::other(format!("Could not reach {service}")));
        }
    };
    let status = response.status();
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_JSON + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JSON {
        return Err(invalid("Authentication response exceeds limit"));
    }
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .map_err(|_| invalid("Authentication response is invalid JSON"))?
    };
    Ok((status, value))
}

fn success(result: (u16, Value), service: &str) -> io::Result<Value> {
    let (status, value) = result;
    if !(200..300).contains(&status) {
        return Err(io::Error::other(format!(
            "{service} refused authentication (HTTP {status})"
        )));
    }
    Ok(value)
}

fn post_json(agent: &ureq::Agent, url: &str, body: Value, service: &str) -> io::Result<Value> {
    success(response(agent.post(url).send_json(body), service)?, service)
}

pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    device_code: String,
    expires: Instant,
    interval: Duration,
}

/// Start only when the user has requested sign-in.
pub fn begin(client_id: &str) -> io::Result<DeviceCode> {
    let agent = agent();
    let value = success(
        response(
            agent
                .post("https://login.live.com/oauth20_connect.srf")
                .send_form(&[
                    ("scope", DEFAULT_SCOPE),
                    ("client_id", client_id),
                    ("response_type", "device_code"),
                ]),
            "Microsoft",
        )?,
        "Microsoft",
    )?;
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("Missing device-code expiry"))?;
    let interval = Duration::from_secs(
        value
            .get("interval")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .max(1),
    );
    let expires = Instant::now()
        .checked_add(Duration::from_secs(expires_in))
        .ok_or_else(|| invalid("Invalid device-code expiry"))?;
    Ok(DeviceCode {
        user_code: field(&value, "user_code")?.to_owned(),
        verification_uri: field(&value, "verification_uri")?.to_owned(),
        device_code: field(&value, "device_code")?.to_owned(),
        expires,
        interval,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Poll {
    Pending,
    SlowDown,
    Complete,
}

fn poll_result(status: u16, value: &Value) -> io::Result<Poll> {
    match value.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown),
        Some("authorization_declined" | "access_denied") => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Microsoft sign-in was declined",
        )),
        Some("expired_token") => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Microsoft device code expired",
        )),
        Some(_) => Err(io::Error::other(
            "Microsoft rejected device-code authentication",
        )),
        None if status == 200 => {
            field(value, "access_token")?;
            Ok(Poll::Complete)
        }
        None => Err(io::Error::other(format!(
            "Microsoft authentication failed (HTTP {status})"
        ))),
    }
}

pub fn finish(client_id: &str, mut code: DeviceCode) -> io::Result<Session> {
    let agent = agent();
    let access = loop {
        let now = Instant::now();
        let remaining = code.expires.saturating_duration_since(now);
        if remaining <= code.interval {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Microsoft device code expired",
            ));
        }
        std::thread::sleep(code.interval);
        let (status, value) = response(
            agent
                .post("https://login.live.com/oauth20_token.srf")
                .send_form(&[
                    ("client_id", client_id),
                    ("device_code", &code.device_code),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ]),
            "Microsoft",
        )?;
        match poll_result(status, &value)? {
            Poll::Pending => continue,
            Poll::SlowDown => {
                code.interval = code
                    .interval
                    .checked_add(Duration::from_secs(5))
                    .ok_or_else(|| invalid("Invalid poll interval"))?;
            }
            Poll::Complete => break field(&value, "access_token")?.to_owned(),
        }
    };
    let ticket = if client_id == DEFAULT_CLIENT_ID {
        access
    } else {
        format!("d={access}")
    };
    let xbox = post_json(
        &agent,
        "https://user.auth.xboxlive.com/user/authenticate",
        json!({
            "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": ticket },
            "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"
        }),
        "Xbox Live",
    )?;
    let xsts = post_json(
        &agent,
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [field(&xbox, "Token")?] },
            "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT"
        }),
        "XSTS",
    )?;
    let user_hash = xsts
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("XSTS response is missing the user hash"))?;
    let minecraft = post_json(
        &agent,
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        json!({
            "identityToken": format!("XBL3.0 x={user_hash};{}", field(&xsts, "Token")?)
        }),
        "Minecraft Services",
    )?;
    let access_token = field(&minecraft, "access_token")?.to_owned();
    let profile = fetch_profile(&agent, &access_token)?;
    Ok(Session {
        profile,
        access_token: Some(access_token),
    })
}

fn fetch_profile(agent: &ureq::Agent, token: &str) -> io::Result<Profile> {
    let value = success(
        response(
            agent
                .get("https://api.minecraftservices.com/minecraft/profile")
                .set("Authorization", &format!("Bearer {token}"))
                .call(),
            "Minecraft profile",
        )?,
        "Minecraft profile",
    )?;
    let username = field(&value, "name")?.to_owned();
    start(&username)?;
    Ok(Profile {
        uuid: parse_uuid(field(&value, "id")?)?,
        username,
    })
}

impl Session {
    pub fn offline(username: &str, uuid: Option<&str>) -> io::Result<Self> {
        start(username)?;
        let uuid = match uuid {
            Some(uuid) => parse_uuid(uuid)?,
            None => {
                let mut hash: [u8; 16] =
                    Md5::digest(format!("OfflinePlayer:{username}").as_bytes()).into();
                hash[6] = (hash[6] & 0x0f) | 0x30;
                hash[8] = (hash[8] & 0x3f) | 0x80;
                u128::from_be_bytes(hash)
            }
        };
        Ok(Self {
            profile: Profile {
                uuid,
                username: username.to_owned(),
            },
            access_token: None,
        })
    }

    pub fn direct(username: &str, uuid: &str, access_token: String) -> io::Result<Self> {
        start(username)?;
        if access_token.is_empty() {
            return Err(invalid("Access token is empty"));
        }
        Ok(Self {
            profile: Profile {
                uuid: parse_uuid(uuid)?,
                username: username.to_owned(),
            },
            access_token: Some(access_token),
        })
    }

    pub fn join(&self, server_hash: &str) -> io::Result<()> {
        let token = self.access_token.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Online-mode server requires an authenticated account",
            )
        })?;
        let (status, _) = response(agent().post("https://sessionserver.mojang.com/session/minecraft/join")
            .send_json(json!({ "accessToken": token, "selectedProfile": format!("{:032x}", self.profile.uuid), "serverId": server_hash })), "Minecraft session service")?;
        if status != 204 {
            return Err(io::Error::other(format!(
                "Minecraft session join failed (HTTP {status})"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_identity_matches_vanilla_and_direct_secrets_stay_hidden() {
        let offline = Session::offline("Notch", None).unwrap();
        assert_eq!(offline.profile.uuid, 0xb50ad385829d3141a2167e7d7539ba7f);
        assert!(offline.join("test").is_err());
        let direct = Session::direct(
            "Notch",
            "b50ad385829d3141a2167e7d7539ba7f",
            "private-token".into(),
        )
        .unwrap();
        assert!(!format!("{direct:?}").contains("private-token"));
        assert!(Session::direct("Notch", "bad", "private-token".into()).is_err());
        assert!(
            Session::direct("Notch", "b50ad385829d3141a2167e7d7539ba7f", String::new()).is_err()
        );
    }

    #[test]
    fn device_poll_handles_pending_slowdown_denial_and_expiry() {
        assert_eq!(
            poll_result(400, &json!({"error":"authorization_pending"})).unwrap(),
            Poll::Pending
        );
        assert_eq!(
            poll_result(400, &json!({"error":"slow_down"})).unwrap(),
            Poll::SlowDown
        );
        assert!(poll_result(400, &json!({"error":"authorization_declined"})).is_err());
        assert_eq!(
            poll_result(400, &json!({"error":"expired_token"}))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(poll_result(200, &json!({})).is_err());
        assert_eq!(
            poll_result(200, &json!({"access_token":"private"})).unwrap(),
            Poll::Complete
        );
    }
}
