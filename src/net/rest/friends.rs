//! Friends and Message Requests: relationship changes, opening a DM and
//! accepting or rejecting message requests. The gateway echoes every change
//! (`RELATIONSHIP_*`, `CHANNEL_CREATE/UPDATE/DELETE`), which is what the UI
//! follows; these calls only say whether Discord took them.

use serde_json::json;

use super::{Method, RestClient};
use crate::error::{Error, Result};
use crate::net::model::*;
use crate::net::types::WireChannel;

impl RestClient {
    /// `POST /users/@me/relationships` — sends a friend request by
    /// username (the "Add Friend" tab). A captcha demand comes back as
    /// [`Error::CaptchaRequired`]; Ferrocord never solves it.
    pub async fn send_friend_request(&self, username: &str) -> Result<()> {
        self.request(
            Method::POST,
            "/users/@me/relationships",
            Some(json!({ "username": username, "discriminator": null })),
        )
        .await
        .map(|_| ())
    }

    /// `PUT /users/@me/relationships/{id}` with `{}` — accepts an incoming
    /// request, or sends one to a user we know by id (implicit tab).
    pub async fn add_friend(&self, user: UserId) -> Result<()> {
        self.request(
            Method::PUT,
            &format!("/users/@me/relationships/{user}"),
            Some(json!({})),
        )
        .await
        .map(|_| ())
    }

    /// `DELETE /users/@me/relationships/{id}` — removes a friend, ignores an
    /// incoming request or cancels an outgoing one.
    pub async fn remove_relationship(&self, user: UserId) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/users/@me/relationships/{user}"),
            None,
        )
        .await
        .map(|_| ())
    }

    /// `POST /users/@me/channels` — opens (or returns the existing) DM with
    /// `user`.
    pub async fn open_dm(&self, user: UserId) -> Result<(Channel, Vec<User>)> {
        let v = self
            .request(
                Method::POST,
                "/users/@me/channels",
                Some(json!({ "recipients": [user] })),
            )
            .await?;
        let c: WireChannel = serde_json::from_value(v)?;
        if c.kind != 1 && c.kind != 3 {
            return Err(Error::Protocol("not a DM channel".into()));
        }
        Ok(c.into_domain(None))
    }

    /// `PUT /channels/{id}/recipients/@me` with `consent_status: 2` —
    /// accepts a message request (it becomes a normal DM).
    pub async fn accept_message_request(&self, channel: ChannelId) -> Result<()> {
        self.request(
            Method::PUT,
            &format!("/channels/{channel}/recipients/@me"),
            Some(json!({ "consent_status": 2 })),
        )
        .await
        .map(|_| ())
    }

    /// `DELETE /channels/{id}/recipients/@me` — rejects (closes) a message
    /// request.
    pub async fn reject_message_request(&self, channel: ChannelId) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/channels/{channel}/recipients/@me"),
            None,
        )
        .await
        .map(|_| ())
    }
}
