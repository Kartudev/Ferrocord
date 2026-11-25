//! Discord's two-factor ("MFA") demand for sensitive actions and its answer.
//!
//! When an action (deleting a server, for one) needs a fresh 2FA check,
//! Discord answers HTTP 401 with code 60003 and a ticket:
//!
//! ```text
//! { "message": "Two factor is required for this operation", "code": 60003,
//!   "mfa": { "ticket": "…", "methods": [ { "type": "totp",
//!            "backup_codes_allowed": true }, { "type": "sms" },
//!            { "type": "password" } ] } }
//! ```
//!
//! The official client asks for a code, sends
//! `POST /mfa/finish {ticket, mfa_type, data}` and gets `{token}` back, then
//! repeats the *same* request with `X-Discord-MFA-Authorization: <token>`.
//! Ferrocord supports authenticator (TOTP) and backup codes; the ticket and
//! the token are secrets (never logged, kept only in memory for the retry).

use serde_json::Value;

use crate::auth::secrets::Secret;

/// The JSON error code of "two factor is required".
pub const MFA_REQUIRED_CODE: u64 = 60_003;
/// Header of the repeated request.
pub const MFA_HEADER: &str = "X-Discord-MFA-Authorization";
/// Tickets are short; refuse anything huge (it goes back in a body).
const MAX_TICKET: usize = 4096;

/// How the user proves 2FA.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfaMethod {
    /// Authenticator app code (6 digits).
    Totp,
    /// One of the 8-character backup codes.
    Backup,
    /// Text message, password, security key: not offered in Ferrocord.
    Other,
}

impl MfaMethod {
    /// `mfa_type` of `POST /mfa/finish`.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::Backup => "backup",
            Self::Other => "",
        }
    }
}

/// A parsed 2FA demand.
#[derive(Clone)]
pub struct MfaChallenge {
    pub ticket: Secret,
    /// The methods Discord accepts, in its order.
    pub methods: Vec<MfaMethod>,
}

impl std::fmt::Debug for MfaChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MfaChallenge")
            .field("ticket", &"[REDACTED]")
            .field("methods", &self.methods)
            .finish()
    }
}

impl PartialEq for MfaChallenge {
    fn eq(&self, other: &Self) -> bool {
        self.ticket.expose() == other.ticket.expose() && self.methods == other.methods
    }
}

impl MfaChallenge {
    /// The demand in an error body, if it is one.
    pub fn parse(body: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(body).ok()?;
        if v.get("code").and_then(Value::as_u64) != Some(MFA_REQUIRED_CODE) {
            return None;
        }
        let mfa = v.get("mfa")?;
        let ticket = mfa.get("ticket")?.as_str()?;
        if ticket.is_empty() || ticket.len() > MAX_TICKET {
            return None;
        }
        let mut methods = Vec::new();
        for m in mfa
            .get("methods")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match m.get("type").and_then(Value::as_str) {
                Some("totp") => {
                    methods.push(MfaMethod::Totp);
                    if m.get("backup_codes_allowed").and_then(Value::as_bool) == Some(true) {
                        methods.push(MfaMethod::Backup);
                    }
                }
                Some("backup") => methods.push(MfaMethod::Backup),
                _ => methods.push(MfaMethod::Other),
            }
        }
        methods.dedup();
        Some(Self {
            ticket: Secret::new(ticket),
            methods,
        })
    }

    /// Whether Ferrocord can answer it (authenticator or backup code).
    pub fn supported(&self) -> bool {
        self.methods
            .iter()
            .any(|m| matches!(m, MfaMethod::Totp | MfaMethod::Backup))
    }

    pub fn allows(&self, m: MfaMethod) -> bool {
        self.methods.contains(&m)
    }
}

/// Normalises a typed code: authenticator codes are 6 digits, backup codes
/// 8 letters/digits (dashes and spaces dropped). `None` when it cannot be
/// one.
pub fn clean_code(method: MfaMethod, typed: &str) -> Option<String> {
    let c: String = typed
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    let ok = match method {
        MfaMethod::Totp => c.len() == 6 && c.chars().all(|c| c.is_ascii_digit()),
        MfaMethod::Backup => c.len() == 8 && c.chars().all(|c| c.is_ascii_alphanumeric()),
        MfaMethod::Other => false,
    };
    ok.then(|| c.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_demand() {
        let body = r#"{"message":"Two factor is required for this operation","code":60003,
            "mfa":{"ticket":"tkt","methods":[{"type":"totp","backup_codes_allowed":true},
            {"type":"sms"},{"type":"password"}]}}"#;
        let c = MfaChallenge::parse(body).unwrap();
        assert_eq!(c.ticket.expose(), "tkt");
        assert_eq!(
            c.methods,
            vec![MfaMethod::Totp, MfaMethod::Backup, MfaMethod::Other]
        );
        assert!(c.supported());
        assert!(!format!("{c:?}").contains("tkt"));
    }

    #[test]
    fn other_errors_are_not_demands() {
        assert!(MfaChallenge::parse(r#"{"code":50013,"message":"Missing Permissions"}"#).is_none());
        assert!(MfaChallenge::parse(r#"{"code":60003,"message":"x"}"#).is_none());
        assert!(MfaChallenge::parse("not json").is_none());
        let sms_only = r#"{"code":60003,"mfa":{"ticket":"t","methods":[{"type":"sms"}]}}"#;
        assert!(!MfaChallenge::parse(sms_only).unwrap().supported());
    }

    #[test]
    fn codes_are_checked() {
        assert_eq!(
            clean_code(MfaMethod::Totp, " 123 456 ").as_deref(),
            Some("123456")
        );
        assert_eq!(clean_code(MfaMethod::Totp, "12345"), None);
        assert_eq!(clean_code(MfaMethod::Totp, "12345a"), None);
        assert_eq!(
            clean_code(MfaMethod::Backup, "ABCD-1234").as_deref(),
            Some("abcd1234")
        );
        assert_eq!(clean_code(MfaMethod::Backup, "abc"), None);
    }
}
