//! The Bans page of Server Settings: the banned users of a server (loaded
//! when the page opens, filtered locally) and "Revoke Ban". Offered with
//! Ban Members (`app::permissions`).

use iced::Task;

use super::guild_admin::admin_error_text;
use super::permissions::BAN_MEMBERS;
use super::{App, Done, Message};
use crate::net::model::*;
use crate::net::rest::guild::Ban;

#[derive(Debug, Clone, PartialEq)]
pub enum BansUi {
    Filter(String),
    Unban(UserId),
    Reload,
}

#[derive(Debug, Clone)]
pub enum BansDone {
    Loaded {
        guild: GuildId,
        result: Result<Vec<Ban>, String>,
    },
    Unbanned {
        guild: GuildId,
        user: UserId,
        result: Result<(), String>,
    },
}

/// State of the Bans page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BansPage {
    pub guild: Option<GuildId>,
    pub loading: bool,
    pub list: Vec<Ban>,
    pub error: Option<String>,
    pub filter: String,
    /// Unbans in flight.
    pub pending: Vec<UserId>,
}

impl BansPage {
    /// The bans matching the filter (name, username or id), in list order.
    pub fn shown(&self) -> Vec<&Ban> {
        let f = self.filter.trim().to_lowercase();
        self.list
            .iter()
            .filter(|b| {
                f.is_empty()
                    || b.user.username.to_lowercase().contains(&f)
                    || b.user.display_name().to_lowercase().contains(&f)
                    || b.user.id.to_string() == f
            })
            .collect()
    }
}

/// Mock bans (two spammers) for `--mock`.
pub(super) fn mock_bans() -> Vec<Ban> {
    let user = |id: u64, name: &str| User {
        id: Snowflake(id),
        username: name.into(),
        ..User::default()
    };
    vec![
        Ban {
            user: user(9101, "free_nitro_bot"),
            reason: Some("Spamming invites".into()),
        },
        Ban {
            user: user(9102, "raidguy"),
            reason: None,
        },
    ]
}

impl App {
    pub fn can_ban(&self, g: GuildId) -> bool {
        self.chat.can_in_guild(g, BAN_MEMBERS)
    }

    /// Opens the Bans page of the Server Settings of `g`: loads the list.
    pub(super) fn load_bans(&mut self, g: GuildId) -> Task<Message> {
        self.admin.bans = BansPage {
            guild: Some(g),
            loading: true,
            ..Default::default()
        };
        if self.cfg.mock {
            return Task::done(Message::Done(Done::Bans(BansDone::Loaded {
                guild: g,
                result: Ok(mock_bans()),
            })));
        }
        let rest = self.rest.clone();
        Task::perform(async move { rest.bans(g).await }, move |r| {
            Message::Done(Done::Bans(BansDone::Loaded {
                guild: g,
                result: r.map_err(|e| admin_error_text(&e)),
            }))
        })
    }

    pub(super) fn update_bans(&mut self, u: BansUi) -> Task<Message> {
        let Some(g) = self.admin.bans.guild else {
            return Task::none();
        };
        match u {
            BansUi::Filter(s) => {
                self.admin.bans.filter = s.chars().take(100).collect();
                Task::none()
            }
            BansUi::Reload => self.load_bans(g),
            BansUi::Unban(user) => {
                let page = &mut self.admin.bans;
                if page.pending.contains(&user) || !page.list.iter().any(|b| b.user.id == user) {
                    return Task::none();
                }
                if !self.chat.can_in_guild(g, BAN_MEMBERS) {
                    return Task::none();
                }
                let page = &mut self.admin.bans;
                page.pending.push(user);
                page.error = None;
                if self.cfg.mock {
                    return Task::done(Message::Done(Done::Bans(BansDone::Unbanned {
                        guild: g,
                        user,
                        result: Ok(()),
                    })));
                }
                let rest = self.rest.clone();
                Task::perform(async move { rest.unban(g, user).await }, move |r| {
                    Message::Done(Done::Bans(BansDone::Unbanned {
                        guild: g,
                        user,
                        result: r.map_err(|e| admin_error_text(&e)),
                    }))
                })
            }
        }
    }

    pub(super) fn on_bans_done(&mut self, d: BansDone) -> Task<Message> {
        let page = &mut self.admin.bans;
        match d {
            BansDone::Loaded { guild, result } => {
                if page.guild != Some(guild) {
                    return Task::none();
                }
                page.loading = false;
                match result {
                    Ok(list) => page.list = list,
                    Err(e) => page.error = Some(e),
                }
            }
            BansDone::Unbanned {
                guild,
                user,
                result,
            } => {
                if page.guild != Some(guild) {
                    return Task::none();
                }
                page.pending.retain(|u| *u != user);
                match result {
                    Ok(()) => {
                        let name = page
                            .list
                            .iter()
                            .find(|b| b.user.id == user)
                            .map(|b| b.user.display_name().to_owned())
                            .unwrap_or_else(|| "The user".into());
                        page.list.retain(|b| b.user.id != user);
                        self.toast = Some(format!("{name} is no longer banned."));
                    }
                    Err(e) => page.error = Some(e),
                }
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Ui;
    use crate::app::guild_admin::AdminUi;
    use crate::app::role_admin::{RoleUi, SettingsPage};
    use crate::mock;

    fn mock_app() -> App {
        App::boot(crate::config::Config::for_tests()).0
    }

    #[test]
    fn filter_matches_names_and_ids() {
        let page = BansPage {
            list: mock_bans(),
            filter: "RAID".into(),
            ..Default::default()
        };
        assert_eq!(page.shown().len(), 1);
        let page = BansPage {
            filter: "9101".into(),
            ..page
        };
        assert_eq!(page.shown()[0].user.username, "free_nitro_bot");
    }

    #[test]
    fn bans_load_and_unban() {
        let mut a = mock_app();
        let g = mock::GUILD_GAMES;
        let _ = a.update_ui(Ui::Admin(AdminUi::OpenSettings(g)));
        let _ = a.update_ui(Ui::Roles(RoleUi::Page(SettingsPage::Bans)));
        assert_eq!(a.admin.page, SettingsPage::Bans);
        assert!(a.admin.bans.loading);
        let _ = a.on_bans_done(BansDone::Loaded {
            guild: g,
            result: Ok(mock_bans()),
        });
        assert_eq!(a.admin.bans.list.len(), 2);
        let _ = a.update_bans(BansUi::Unban(Snowflake(9102)));
        assert_eq!(a.admin.bans.pending, vec![Snowflake(9102)]);
        let _ = a.on_bans_done(BansDone::Unbanned {
            guild: g,
            user: Snowflake(9102),
            result: Err("nope".into()),
        });
        assert_eq!(a.admin.bans.list.len(), 2);
        assert_eq!(a.admin.bans.error.as_deref(), Some("nope"));
        let _ = a.update_bans(BansUi::Unban(Snowflake(9102)));
        let _ = a.on_bans_done(BansDone::Unbanned {
            guild: g,
            user: Snowflake(9102),
            result: Ok(()),
        });
        assert_eq!(a.admin.bans.list.len(), 1);
        assert!(a.admin.bans.pending.is_empty());
    }
}
