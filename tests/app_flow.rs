//! App-level flows driven through `App::update` in mock mode (no window).

use ferrocord::app::{App, Message, Screen, Ui, View};
use ferrocord::config::Config;
use ferrocord::mock;

fn app() -> App {
    App::boot(Config::for_tests()).0
}

#[test]
fn mock_boot_lands_in_a_guild_channel() {
    let a = app();
    assert_eq!(a.screen(), Screen::Main);
    assert_eq!(a.open_channel(), Some(mock::CH_GENERAL));
    assert!(a.title().contains("#general"));
}

#[test]
fn selecting_channels_marks_them_read() {
    let mut a = app();
    assert!(a.chat.is_unread(mock::CH_HELP));
    let _ = a.update(Message::Ui(Ui::SelectChannel(mock::CH_HELP)));
    assert_eq!(a.open_channel(), Some(mock::CH_HELP));
    assert!(!a.chat.is_unread(mock::CH_HELP));
    assert_eq!(a.chat.mention_count(mock::CH_HELP), 0);

    let _ = a.update(Message::Ui(Ui::SelectHome));
    assert!(matches!(a.view, View::Home(Some(_))));
    let _ = a.update(Message::Ui(Ui::SelectChannel(mock::DM_ALICE)));
    assert_eq!(a.view, View::Home(Some(mock::DM_ALICE)));
    assert_eq!(a.chat.mention_count(mock::DM_ALICE), 0);
}

#[test]
fn voice_channels_cannot_be_opened() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::SelectChannel(
        ferrocord::net::model::Snowflake(140),
    )));
    assert_eq!(a.open_channel(), Some(mock::CH_GENERAL));
}

#[test]
fn sending_adds_a_pending_message_and_clears_composer() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::ComposerChanged("hello **there**".into())));
    let _ = a.update(Message::Ui(Ui::Send));
    assert!(a.composer.is_empty());
    let last = a.chat.messages[&mock::CH_GENERAL].messages.back().unwrap();
    assert!(last.pending);
    assert_eq!(last.content, "hello **there**");
    // Blank messages are ignored.
    let before = a.chat.messages[&mock::CH_GENERAL].messages.len();
    let _ = a.update(Message::Ui(Ui::ComposerChanged("   ".into())));
    let _ = a.update(Message::Ui(Ui::Send));
    assert_eq!(a.chat.messages[&mock::CH_GENERAL].messages.len(), before);
}

#[test]
fn categories_collapse_and_spoilers_reveal() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::ToggleCategory(mock::CAT_INFO)));
    assert!(a.collapsed.contains(&mock::CAT_INFO));
    let _ = a.update(Message::Ui(Ui::ToggleCategory(mock::CAT_INFO)));
    assert!(!a.collapsed.contains(&mock::CAT_INFO));
    let id = a.chat.messages[&mock::CH_GENERAL]
        .messages
        .back()
        .unwrap()
        .id;
    let _ = a.update(Message::Ui(Ui::RevealSpoiler(id)));
    assert!(a.revealed_spoilers.contains(&id));
}

#[test]
fn logout_returns_to_login() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::Logout));
    assert_eq!(a.screen(), Screen::Login);
    assert!(a.chat.guilds.is_empty());
}

#[test]
fn views_render_without_panicking() {
    let mut a = app();
    let _ = ferrocord::ui::view(&a);
    let _ = a.update(Message::Ui(Ui::SelectHome));
    let _ = ferrocord::ui::view(&a);
    let _ = a.update(Message::Ui(Ui::Logout));
    let _ = ferrocord::ui::view(&a);
}

#[test]
fn masked_links_need_confirmation() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::ConfirmLink(
        "https://example.com/real".into(),
    )));
    assert_eq!(a.pending_link.as_deref(), Some("https://example.com/real"));
    let _ = ferrocord::ui::view(&a);
    let _ = a.update(Message::Ui(Ui::CancelLink));
    assert!(a.pending_link.is_none());
    let _ = a.update(Message::Ui(Ui::ConfirmLink(
        "https://example.com/real".into(),
    )));
    let _ = a.update(Message::Ui(Ui::OpenUrl("https://example.com/real".into())));
    assert!(a.pending_link.is_none());
}

#[test]
fn markdown_is_parsed_once_per_message() {
    let a = app();
    let id = ferrocord::net::model::Snowflake(42);
    let first = a.parsed_markdown(id, "**hi**");
    let second = a.parsed_markdown(id, "**hi**");
    assert!(std::rc::Rc::ptr_eq(&first, &second));
    // Edited content gets a fresh parse.
    let edited = a.parsed_markdown(id, "**hi** there");
    assert!(!std::rc::Rc::ptr_eq(&first, &edited));
}

#[test]
fn inline_custom_emoji_render_as_images_when_loaded() {
    let mut a = app();
    // The mock #general history ends with inline `<:ferris:1001>`.
    let key = ferrocord::app::emoji_url(&a.cfg.cdn_url, ferrocord::net::model::Snowflake(1001));
    a.images.insert(
        key.clone(),
        iced::widget::image::Handle::from_rgba(1, 1, vec![255, 0, 0, 255]),
        4,
    );
    assert!(a.image(&key).is_some());
    let _ = ferrocord::ui::view(&a);
}

// -- round 2: replies, edit/delete, reactions, uploads, members, search ------------

use ferrocord::app::Done;
use ferrocord::app::emoji::{PickerTarget, Section};
use ferrocord::app::upload::UploadEvent;
use ferrocord::net::model::{Emoji, Message as Msg, Presence, Snowflake};
use ferrocord::net::rest::UploadFile;

fn ui(a: &mut App, u: Ui) {
    let _ = a.update(Message::Ui(u));
}

fn window(a: &App) -> Vec<&Msg> {
    a.chat.messages[&mock::CH_GENERAL].messages.iter().collect()
}

fn last_of(a: &App, user: u64) -> Msg {
    window(a)
        .into_iter()
        .rev()
        .find(|m| m.author.id == Snowflake(user))
        .cloned()
        .expect("message by user")
}

#[test]
fn reply_flow_from_bar_to_pending_message() {
    let mut a = app();
    let target = last_of(&a, 3); // Bob
    ui(&mut a, Ui::StartReply(target.id));
    let r = a.reply.clone().expect("reply draft");
    assert_eq!(r.message, target.id);
    assert!(r.mention, "ping is on by default");
    ui(&mut a, Ui::ToggleReplyMention);
    assert!(!a.reply.as_ref().unwrap().mention);
    let _ = ferrocord::ui::view(&a);

    ui(&mut a, Ui::ComposerChanged("on it".into()));
    ui(&mut a, Ui::Send);
    assert!(a.reply.is_none(), "reply bar closes after sending");
    let pending = a.chat.messages[&mock::CH_GENERAL].messages.back().unwrap();
    assert!(pending.pending);
    assert_eq!(pending.reply_to, Some(target.id));
    assert_eq!(pending.kind, 19);
    let preview = a.chat.reply_preview(pending).unwrap();
    assert_eq!(preview.author.unwrap().id, Snowflake(3));
    assert!(!preview.deleted);
    // The reply header renders.
    let _ = ferrocord::ui::view(&a);
}

#[test]
fn replying_to_yourself_never_pings_and_escape_cancels() {
    let mut a = app();
    let mine = last_of(&a, 1);
    ui(&mut a, Ui::StartReply(mine.id));
    assert!(!a.reply.as_ref().unwrap().mention);
    ui(&mut a, Ui::ToggleReplyMention);
    assert!(!a.reply.as_ref().unwrap().mention, "toggle is a no-op");
    ui(&mut a, Ui::Escape);
    assert!(a.reply.is_none());
}

#[test]
fn reply_draft_is_dropped_when_switching_channels() {
    let mut a = app();
    let target = last_of(&a, 3);
    ui(&mut a, Ui::StartReply(target.id));
    ui(&mut a, Ui::SelectChannel(mock::CH_HELP));
    assert!(a.reply.is_none());
}

#[test]
fn edit_flow_up_arrow_save_and_edited_marker() {
    let mut a = app();
    let mine = last_of(&a, 1);
    // Up arrow in an empty composer edits the last own message.
    ui(&mut a, Ui::EditLast);
    let e = a.editing.clone().expect("editing");
    assert_eq!(e.message, mine.id);
    assert_eq!(e.text.text(), mine.content);
    let _ = ferrocord::ui::view(&a);
    ui(
        &mut a,
        Ui::EditChanged("agreed, and `let` chains are great".into()),
    );
    ui(&mut a, Ui::SubmitEdit);
    assert!(a.editing.is_none());
    let m = a.chat.message(mock::CH_GENERAL, mine.id).unwrap();
    assert_eq!(m.content, "agreed, and `let` chains are great");
    assert!(m.edited);
}

#[test]
fn edit_rules() {
    let mut a = app();
    // Not your message: ignored.
    let bobs = last_of(&a, 3);
    ui(&mut a, Ui::StartEdit(bobs.id));
    assert!(a.editing.is_none());
    // Up arrow does nothing while the composer has text.
    ui(&mut a, Ui::ComposerChanged("draft".into()));
    ui(&mut a, Ui::EditLast);
    assert!(a.editing.is_none());
    ui(&mut a, Ui::ComposerChanged(String::new()));

    let mine = last_of(&a, 1);
    // Unchanged text closes the editor without marking it edited.
    ui(&mut a, Ui::StartEdit(mine.id));
    ui(&mut a, Ui::SubmitEdit);
    assert!(a.editing.is_none());
    assert_eq!(
        a.chat.message(mock::CH_GENERAL, mine.id).unwrap().edited,
        mine.edited
    );
    // Too long: stays open with a toast.
    ui(&mut a, Ui::StartEdit(mine.id));
    ui(&mut a, Ui::EditChanged("x".repeat(2001)));
    ui(&mut a, Ui::SubmitEdit);
    assert!(a.editing.is_some());
    assert!(a.toast.is_some());
    // The toast hides itself: only the timer of the latest toast counts.
    ui(&mut a, Ui::ToastExpired(0));
    assert!(a.toast.is_some());
    ui(&mut a, Ui::ToastExpired(1));
    assert!(a.toast.is_none());
    // Emptying the message offers to delete it instead.
    ui(&mut a, Ui::EditChanged("  ".into()));
    ui(&mut a, Ui::SubmitEdit);
    assert!(a.editing.is_none());
    assert_eq!(a.pending_delete, Some((mock::CH_GENERAL, mine.id)));
    ui(&mut a, Ui::CancelDelete);
    // Escape cancels an edit.
    ui(&mut a, Ui::StartEdit(mine.id));
    ui(&mut a, Ui::Escape);
    assert!(a.editing.is_none());
}

/// One test, because it toggles the process-wide Shift state.
#[test]
fn delete_confirms_unless_shift_is_held() {
    use ferrocord::app::input::set_shift_for_tests;
    let mut a = app();
    let mine = last_of(&a, 1);
    let bobs = last_of(&a, 3);
    set_shift_for_tests(false);
    // Others' messages cannot be deleted.
    ui(&mut a, Ui::DeleteClicked(bobs.id));
    assert!(a.pending_delete.is_none());
    // Plain click opens the confirmation (which renders).
    ui(&mut a, Ui::DeleteClicked(mine.id));
    assert_eq!(a.pending_delete, Some((mock::CH_GENERAL, mine.id)));
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::CancelDelete);
    assert!(a.chat.message(mock::CH_GENERAL, mine.id).is_some());
    ui(&mut a, Ui::DeleteClicked(mine.id));
    ui(&mut a, Ui::ConfirmDelete);
    assert!(a.chat.message(mock::CH_GENERAL, mine.id).is_none());
    assert!(a.pending_delete.is_none());

    // Shift+click skips the confirmation.
    let again = mock::text_message(
        mock::CH_GENERAL,
        Snowflake::from_timestamp_ms(mock::now_ms(), 77),
        mock::user(1),
        "delete me fast",
    );
    let id = again.id;
    a.chat
        .messages
        .get_mut(&mock::CH_GENERAL)
        .unwrap()
        .upsert(again);
    set_shift_for_tests(true);
    ui(&mut a, Ui::DeleteClicked(id));
    set_shift_for_tests(false);
    assert!(a.pending_delete.is_none());
    assert!(a.chat.message(mock::CH_GENERAL, id).is_none());
}

#[test]
fn context_menu_opens_and_closes() {
    let mut a = app();
    let m = last_of(&a, 1);
    ui(&mut a, Ui::ContextMenu(m.id));
    assert_eq!(a.menu.unwrap().message, m.id);
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::Escape);
    assert!(a.menu.is_none());
    // Opening the reaction picker from the menu replaces the menu.
    ui(&mut a, Ui::ContextMenu(m.id));
    ui(
        &mut a,
        Ui::OpenEmojiPicker(PickerTarget::Reaction {
            channel: mock::CH_GENERAL,
            message: m.id,
        }),
    );
    assert!(a.menu.is_none() && a.emoji_picker.is_some());
    let _ = ferrocord::ui::view(&a);
}

#[test]
fn reactions_toggle_and_picker_flows() {
    let mut a = app();
    let m = last_of(&a, 3);
    let thumbs = Emoji::unicode("👍");
    ui(&mut a, Ui::ToggleReaction(m.id, thumbs.clone()));
    let r = &a.chat.message(mock::CH_GENERAL, m.id).unwrap().reactions;
    assert_eq!((r[0].count, r[0].me), (1, true));
    ui(&mut a, Ui::ToggleReaction(m.id, thumbs.clone()));
    assert!(
        a.chat
            .message(mock::CH_GENERAL, m.id)
            .unwrap()
            .reactions
            .is_empty()
    );

    // Picker opened from the toolbar reacts to that message.
    ui(
        &mut a,
        Ui::OpenEmojiPicker(PickerTarget::Reaction {
            channel: mock::CH_GENERAL,
            message: m.id,
        }),
    );
    // The mock account has recently used emoji, so it opens on them.
    assert_eq!(
        a.emoji_picker.as_ref().unwrap().picker.section,
        Section::Recent
    );
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::EmojiQuery("crab".into()));
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::PickEmoji(Emoji::unicode("🦀")));
    assert!(a.emoji_picker.is_none());
    assert_eq!(
        a.chat.message(mock::CH_GENERAL, m.id).unwrap().reactions[0].emoji,
        Emoji::unicode("🦀")
    );

    // Picker opened from the composer inserts text, custom emoji as markup.
    ui(&mut a, Ui::OpenEmojiPicker(PickerTarget::Composer));
    ui(&mut a, Ui::EmojiSection(Section::Unicode(3)));
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::PickEmoji(Emoji::unicode("🍕")));
    ui(&mut a, Ui::OpenEmojiPicker(PickerTarget::Composer));
    ui(
        &mut a,
        Ui::PickEmoji(Emoji {
            id: Some(Snowflake(1001)),
            name: "ferris".into(),
            animated: false,
        }),
    );
    assert_eq!(a.composer.text(), "🍕 <:ferris:1001> ");
    // Reacting and picking put the emoji first in the recently used ones.
    let recent: Vec<&str> = a
        .recent_emoji
        .list()
        .iter()
        .take(3)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(recent, vec!["ferris", "🍕", "🦀"]);
}

#[test]
fn recent_emoji_follow_sent_messages_and_reactions() {
    let mut a = app();
    let m = last_of(&a, 3);
    // Without recent emoji the picker opens on the server's custom emoji.
    a.recent_emoji = Default::default();
    ui(
        &mut a,
        Ui::OpenEmojiPicker(PickerTarget::Reaction {
            channel: mock::CH_GENERAL,
            message: m.id,
        }),
    );
    assert!(matches!(
        a.emoji_picker.as_ref().unwrap().picker.section,
        Section::Server(_)
    ));
    ui(&mut a, Ui::CloseEmojiPicker);
    // Emoji written in a sent message count as used, in order.
    a.composer.set_text("gg 🎉 <:party:1002> 🎉");
    ui(&mut a, Ui::Send);
    let names = |a: &ferrocord::app::App| -> Vec<String> {
        a.recent_emoji
            .list()
            .iter()
            .map(|e| e.name.clone())
            .collect()
    };
    assert_eq!(names(&a), vec!["🎉", "party"]);
    // Adding a reaction counts; removing it does not.
    ui(&mut a, Ui::ToggleReaction(m.id, Emoji::unicode("🔥")));
    ui(&mut a, Ui::ToggleReaction(m.id, Emoji::unicode("🎉")));
    ui(&mut a, Ui::ToggleReaction(m.id, Emoji::unicode("🔥")));
    assert_eq!(names(&a), vec!["🎉", "🔥", "party"]);
    // The picker now opens on them, and the hover bar offers them.
    ui(
        &mut a,
        Ui::OpenEmojiPicker(PickerTarget::Reaction {
            channel: mock::CH_GENERAL,
            message: m.id,
        }),
    );
    assert_eq!(
        a.emoji_picker.as_ref().unwrap().picker.section,
        Section::Recent
    );
    let _ = ferrocord::ui::view(&a);
}

#[test]
fn failed_reaction_request_is_reverted_with_a_toast() {
    let mut a = app();
    let m = last_of(&a, 3);
    let e = Emoji::unicode("🎈");
    ui(&mut a, Ui::ToggleReaction(m.id, e.clone()));
    let _ = a.update(Message::Done(Done::ReactionFailed {
        channel: mock::CH_GENERAL,
        message: m.id,
        emoji: e,
        change: ferrocord::app::state::ReactionChange::Added,
        error: "Unknown Emoji".into(),
    }));
    assert!(
        a.chat
            .message(mock::CH_GENERAL, m.id)
            .unwrap()
            .reactions
            .is_empty()
    );
    assert!(a.toast.as_deref().unwrap().contains("Unknown Emoji"));
}

fn file(name: &str, size: u64) -> Result<(UploadFile, bool), String> {
    Ok((
        UploadFile {
            path: format!("/tmp/{name}").into(),
            name: name.into(),
            size,
        },
        false,
    ))
}

#[test]
fn attachments_upload_with_progress_and_caption() {
    let mut a = app();
    let _ = a.update(Message::Done(Done::FilesReady(vec![
        file("shot.png", 2_000),
        file("huge.bin", 11 * 1024 * 1024),
        file("empty.txt", 0),
    ])));
    // The oversized and empty files are refused with a toast.
    assert_eq!(a.draft.len(), 1);
    assert!(a.toast.is_some());
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::ComposerChanged("look at this".into()));
    ui(&mut a, Ui::Send);
    assert!(a.draft.is_empty() && a.composer.is_empty());
    assert_eq!(a.uploads.len(), 1);
    let id = a.uploads[0].id;
    assert_eq!(a.uploads[0].total, 2_000);
    let _ = ferrocord::ui::view(&a);

    let _ = a.update(Message::Done(Done::Upload {
        id,
        event: UploadEvent::Progress {
            sent: 500,
            total: 2_000,
        },
    }));
    assert!((a.uploads[0].fraction() - 0.25).abs() < 1e-6);

    let mut m = mock::text_message(
        mock::CH_GENERAL,
        Snowflake::from_timestamp_ms(mock::now_ms(), 9),
        mock::user(1),
        "look at this",
    );
    m.attachments = vec![];
    let _ = a.update(Message::Done(Done::Upload {
        id,
        event: UploadEvent::Done(Ok(Box::new(m.clone()))),
    }));
    assert!(a.uploads.is_empty());
    assert!(a.chat.message(mock::CH_GENERAL, m.id).is_some());
}

#[test]
fn upload_errors_and_cancel() {
    let mut a = app();
    let _ = a.update(Message::Done(Done::FilesReady(vec![file("a.txt", 10)])));
    ui(&mut a, Ui::Send);
    let id = a.uploads[0].id;
    let _ = a.update(Message::Done(Done::Upload {
        id,
        event: UploadEvent::Done(Err("Request Entity Too Large".into())),
    }));
    assert!(a.uploads.is_empty());
    assert_eq!(a.toast.as_deref(), Some("Request Entity Too Large"));

    let _ = a.update(Message::Done(Done::FilesReady(vec![file("b.txt", 10)])));
    ui(&mut a, Ui::Send);
    let id = a.uploads[0].id;
    ui(&mut a, Ui::CancelUpload(id));
    assert!(a.uploads.is_empty());
    // Removing a draft file.
    let _ = a.update(Message::Done(Done::FilesReady(vec![file("c.txt", 10)])));
    assert_eq!(a.draft.len(), 1);
    ui(&mut a, Ui::RemoveAttachment(0));
    assert!(a.draft.is_empty());
    // Dropped files are ignored while editing.
    let mine = last_of(&a, 1).id;
    ui(&mut a, Ui::StartEdit(mine));
    ui(&mut a, Ui::FileDropped("/tmp/x.png".into()));
}

#[test]
fn member_list_shows_groups_and_presence() {
    let mut a = app();
    let l = a.chat.member_list.as_ref().expect("mock member list");
    assert_eq!(l.loaded().count(), 11);
    assert_eq!(a.chat.presence_of(Snowflake(3)), Presence::Idle);
    assert_eq!(a.chat.presence_of(Snowflake(6)), Presence::Offline);
    let _ = ferrocord::ui::view(&a);
    // The panel can be toggled off and on.
    ui(&mut a, Ui::ToggleMembers);
    assert!(!a.members_open);
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::ToggleMembers);
    assert!(a.members_open);
    // DMs have no member list.
    ui(&mut a, Ui::SelectChannel(mock::DM_ALICE));
    assert!(a.chat.member_list.is_none());
    let _ = ferrocord::ui::view(&a);
    // Back in the server it is rebuilt.
    ui(&mut a, Ui::SelectChannel(mock::CH_GENERAL));
    assert!(a.chat.member_list.is_some());
}

#[test]
fn own_status_can_be_changed() {
    let mut a = app();
    assert_eq!(a.chat.my_status, Presence::Online);
    ui(&mut a, Ui::ToggleStatusMenu);
    assert!(a.status_menu);
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::SetStatus(Presence::Dnd));
    assert_eq!(a.chat.my_status, Presence::Dnd);
    assert!(!a.status_menu);
    assert_eq!(a.chat.presence_of(Snowflake(1)), Presence::Dnd);
    let _ = ferrocord::ui::view(&a);
}

#[test]
fn search_results_and_jump_to_message() {
    let mut a = app();
    ui(&mut a, Ui::SearchChanged("let".into()));
    assert!(a.search.open);
    ui(&mut a, Ui::SubmitSearch);
    assert!(a.search.panel_visible());
    assert!(a.search.total >= 2, "total {}", a.search.total);
    let _ = ferrocord::ui::view(&a);
    let hit = a.search.results[0].clone();

    // Jump from another channel: switches back and flashes the message.
    ui(&mut a, Ui::SelectChannel(mock::CH_HELP));
    ui(
        &mut a,
        Ui::JumpTo {
            channel: hit.channel_id,
            message: hit.id,
        },
    );
    assert_eq!(a.open_channel(), Some(hit.channel_id));
    assert_eq!(a.highlight, Some(hit.id));
    let _ = ferrocord::ui::view(&a);
    let _ = a.update(Message::Done(Done::ClearHighlight(hit.id)));
    assert!(a.highlight.is_none());

    // Escape closes the search panel; blank queries do nothing.
    ui(&mut a, Ui::SearchChanged("   ".into()));
    ui(&mut a, Ui::SubmitSearch);
    ui(&mut a, Ui::Escape);
    assert!(!a.search.panel_visible() && !a.search.open);
}

#[test]
fn search_in_dms_and_no_results() {
    let mut a = app();
    ui(&mut a, Ui::SelectChannel(mock::DM_ALICE));
    ui(&mut a, Ui::SearchChanged("iced".into()));
    ui(&mut a, Ui::SubmitSearch);
    assert!(a.search.total >= 1);
    for m in &a.search.results {
        assert_eq!(m.channel_id, mock::DM_ALICE);
    }
    ui(&mut a, Ui::SearchChanged("zzzz-nothing".into()));
    ui(&mut a, Ui::SubmitSearch);
    assert_eq!(a.search.total, 0);
    let _ = ferrocord::ui::view(&a);
    // Home without an open DM cannot search.
    ui(&mut a, Ui::CloseSearch);
    ui(&mut a, Ui::SelectHome);
}

#[test]
fn notification_click_opens_the_channel() {
    let mut a = app();
    ui(&mut a, Ui::OpenChannel(mock::DM_ALICE));
    assert_eq!(a.view, View::Home(Some(mock::DM_ALICE)));
    ui(&mut a, Ui::OpenChannel(mock::CH_RANDOM));
    assert_eq!(a.open_channel(), Some(mock::CH_RANDOM));
    assert!(matches!(a.view, View::Guild { guild, .. } if guild == mock::GUILD_RUST));
    // A channel that no longer exists is ignored.
    ui(&mut a, Ui::OpenChannel(Snowflake(999_999)));
    assert_eq!(a.open_channel(), Some(mock::CH_RANDOM));
}

#[test]
fn jump_to_present_reloads_a_detached_window() {
    let mut a = app();
    a.chat.messages.get_mut(&mock::CH_GENERAL).unwrap().detached = true;
    let _ = ferrocord::ui::view(&a);
    ui(&mut a, Ui::JumpToPresent);
    let w = &a.chat.messages[&mock::CH_GENERAL];
    assert!(!w.detached);
}

#[test]
fn live_messages_are_not_appended_to_a_detached_window() {
    let mut a = app();
    a.chat.messages.get_mut(&mock::CH_GENERAL).unwrap().detached = true;
    let before = a.chat.messages[&mock::CH_GENERAL].messages.len();
    let m = mock::text_message(
        mock::CH_GENERAL,
        Snowflake::from_timestamp_ms(mock::now_ms() + 5_000, 1),
        mock::user(3),
        "new live message",
    );
    let _ = a.update(Message::Net(ferrocord::net::Event::MessageCreate(
        Box::new(m),
    )));
    assert_eq!(a.chat.messages[&mock::CH_GENERAL].messages.len(), before);
}

// -- tray and background mode ----------------------------------------------------

mod tray_flow {
    use std::sync::Arc;

    use ferrocord::app::{App, Message, Ui};
    use ferrocord::config::Config;
    use ferrocord::mock;
    use ferrocord::net::model::Presence;
    use ferrocord::tray::{self, Command, TrayModel};
    use tokio::sync::watch;

    fn app_with_tray() -> (App, watch::Receiver<TrayModel>) {
        let mut a = App::boot(Config::for_tests()).0;
        let (tx, rx) = watch::channel(TrayModel::default());
        let control = tray::Control::new(Arc::new(tx));
        let _ = a.update(Message::Tray(tray::Event::Ready(control)));
        (a, rx)
    }

    fn command(a: &mut App, c: Command) {
        let _ = a.update(Message::Tray(tray::Event::Command(c)));
    }

    #[test]
    fn closing_hides_to_the_tray_once_registered() {
        let (mut a, _rx) = app_with_tray();
        assert!(a.win.visible);
        let _ = a.update(Message::Ui(Ui::CloseRequested));
        assert!(!a.win.visible, "close hides while a tray exists");
        let _ = a.update(Message::Ui(Ui::ShowWindow));
        assert!(a.win.visible);
    }

    #[test]
    fn left_click_toggles_the_window() {
        let (mut a, _rx) = app_with_tray();
        command(&mut a, Command::Toggle);
        assert!(!a.win.visible);
        command(&mut a, Command::Toggle);
        assert!(a.win.visible);
        command(&mut a, Command::Hide);
        assert!(!a.win.visible);
        command(&mut a, Command::Show);
        assert!(a.win.visible);
    }

    #[test]
    fn tray_quit_stops_everything_once() {
        let (mut a, _rx) = app_with_tray();
        assert!(!a.quitting);
        command(&mut a, Command::Quit);
        assert!(a.quitting, "Quit starts the shutdown");
        // A second Quit (double click, Ctrl+Q meanwhile) starts nothing new.
        let again = a.update(Message::Ui(Ui::Quit));
        assert_eq!(again.units(), 0);
        assert!(a.video.is_none());
    }

    #[test]
    fn without_a_tray_closing_does_not_hide() {
        let mut a = App::boot(Config::for_tests()).0;
        let _ = a.update(Message::Ui(Ui::CloseRequested));
        // `iced::exit()` was requested; the window state keeps "visible".
        assert!(a.win.visible);
        let _ = a.update(Message::Ui(Ui::HideWindow));
        assert!(a.win.visible, "hiding needs a tray host");
    }

    #[test]
    fn losing_the_tray_restores_a_hidden_window() {
        let (mut a, _rx) = app_with_tray();
        command(&mut a, Command::Hide);
        assert!(!a.win.visible);
        let _ = a.update(Message::Tray(tray::Event::Unavailable));
        assert!(a.win.visible);
        let _ = a.update(Message::Ui(Ui::CloseRequested));
        assert!(a.win.visible, "no tray any more: close quits");
    }

    #[test]
    fn minimized_start_waits_for_the_tray() {
        let mut cfg = Config::for_tests();
        cfg.tray = true;
        cfg.minimized = true;
        let mut a = App::boot(cfg.clone()).0;
        assert!(!a.win.visible, "--minimized opens no window");
        let _ = a.update(Message::Tray(tray::Event::Unavailable));
        assert!(a.win.visible, "no tray host: show the window");
        // --minimized is ignored without a tray at all.
        cfg.tray = false;
        assert!(App::boot(cfg).0.win.visible);
    }

    #[test]
    fn menu_status_uses_the_status_setter() {
        let (mut a, rx) = app_with_tray();
        command(&mut a, Command::SetStatus(Presence::Dnd));
        assert_eq!(a.chat.my_status, Presence::Dnd);
        assert_eq!(rx.borrow().status, Presence::Dnd);
        assert!(rx.borrow().logged_in);
    }

    #[test]
    fn menu_toggles_update_prefs_and_tray() {
        let (mut a, rx) = app_with_tray();
        assert!(!a.prefs.muted);
        command(&mut a, Command::ToggleMute);
        assert!(a.prefs.muted);
        assert!(rx.borrow().muted);
        command(&mut a, Command::ToggleStartMinimized);
        assert!(a.prefs.start_minimized && rx.borrow().start_minimized);
        command(&mut a, Command::ToggleMute);
        assert!(!rx.borrow().muted);
    }

    #[test]
    fn tray_shows_the_unread_mention_count() {
        let (mut a, rx) = app_with_tray();
        // The mock account has unread mentions and DMs.
        let n = a.chat.attention_count();
        assert!(n > 0);
        assert_eq!(rx.borrow().mentions, n);
        assert!(rx.borrow().needs_attention());
        assert!(rx.borrow().tooltip_title().contains(&n.to_string()));
        // Reading a mention-carrying channel lowers it.
        let (c, k) = [mock::DM_ALICE, mock::CH_HELP]
            .into_iter()
            .map(|c| (c, a.chat.mention_count(c)))
            .find(|(_, k)| *k > 0)
            .unwrap();
        let _ = a.update(Message::Ui(Ui::SelectChannel(c)));
        assert_eq!(rx.borrow().mentions, n - k);
    }

    #[test]
    fn clicking_a_notification_shows_the_hidden_window_on_that_chat() {
        let (mut a, _rx) = app_with_tray();
        command(&mut a, Command::Hide);
        assert!(!a.win.visible);
        let _ = a.update(Message::Ui(Ui::OpenChannel(mock::CH_HELP)));
        assert!(a.win.visible);
        assert_eq!(a.open_channel(), Some(mock::CH_HELP));
    }

    #[test]
    fn settings_toggles_flip_preferences() {
        let mut a = App::boot(Config::for_tests()).0;
        assert!(a.prefs.close_to_tray);
        let _ = a.update(Message::Ui(Ui::OpenSettings));
        assert!(a.settings_open);
        let _ = a.update(Message::Ui(Ui::ToggleCloseToTray));
        assert!(!a.prefs.close_to_tray);
        assert!(!a.win.close_to_tray);
        // Mock mode flips the autostart toggle without touching the disk.
        let _ = a.update(Message::Ui(Ui::ToggleAutostart));
        assert!(a.autostart);
        let _ = a.update(Message::Ui(Ui::Escape));
        assert!(!a.settings_open);
    }
}

// -- widget tree stability (flicker regression) ---------------------------------

mod tree_shape {
    use super::*;
    use iced::advanced::widget::Tree;

    /// Structural fingerprint of a widget (sub)tree: widget type tags and
    /// child counts, recursively.
    fn shape(t: &Tree) -> String {
        let kids: Vec<String> = t.children.iter().map(shape).collect();
        format!("{:?}[{}]", t.tag, kids.join(","))
    }

    /// The part of the tree that holds the whole app body: layer stack
    /// → base layer (banner slot, body).
    fn body_shape(t: &Tree) -> String {
        // t.children[0] = content column (banner slot, body).
        shape(&t.children[0].children[1])
    }

    /// Diffs `tree` against the current view and returns the body shape.
    fn diffed(tree: &mut Tree, a: &App) -> String {
        let el = ferrocord::ui::view(a);
        tree.diff(&el);
        body_shape(tree)
    }

    fn msg_id(a: &App) -> ferrocord::net::model::MessageId {
        a.chat.messages[&mock::CH_GENERAL]
            .messages
            .iter()
            .find(|m| m.author.id == ferrocord::net::model::Snowflake(2))
            .unwrap()
            .id
    }

    #[test]
    fn opening_and_closing_popups_keeps_the_body_widget_tree() {
        let mut a = app();
        let mut tree = {
            let el = ferrocord::ui::view(&a);
            Tree::new(&el)
        };
        let base = body_shape(&tree);
        let id = msg_id(&a);
        let layers = ferrocord::ui::LAYER_SLOTS;
        assert_eq!(tree.children.len(), layers);

        for (name, open, close) in [
            ("context menu", Ui::ContextMenu(id), Ui::CloseMenu),
            ("status menu", Ui::ToggleStatusMenu, Ui::ToggleStatusMenu),
            ("settings", Ui::OpenSettings, Ui::CloseSettings),
            ("delete dialog", Ui::DeleteClicked(id), Ui::CancelDelete),
            (
                "link dialog",
                Ui::ConfirmLink("https://example.com".into()),
                Ui::CancelLink,
            ),
        ] {
            ferrocord::app::input::set_shift_for_tests(false);
            let _ = a.update(Message::Ui(open));
            assert_eq!(
                diffed(&mut tree, &a),
                base,
                "{name}: opening reshaped the body"
            );
            assert_eq!(tree.children.len(), layers, "{name}");
            let _ = a.update(Message::Ui(close));
            assert_eq!(
                diffed(&mut tree, &a),
                base,
                "{name}: closing reshaped the body"
            );
        }
    }

    #[test]
    fn a_toast_banner_keeps_the_body_widget_tree() {
        let mut a = app();
        let mut tree = {
            let el = ferrocord::ui::view(&a);
            Tree::new(&el)
        };
        let base = body_shape(&tree);
        a.toast = Some("Could not do that".into());
        assert_eq!(diffed(&mut tree, &a), base);
        a.toast = None;
        assert_eq!(diffed(&mut tree, &a), base);
    }
}

#[test]
fn image_handles_are_stable_across_views() {
    // A handle built inside `view()` gets a fresh id every frame and makes
    // the renderer re-upload the texture; the QR code is cached instead.
    let a = app();
    let modules: Vec<bool> = (0..25 * 25).map(|i| i % 3 == 0).collect();
    let h1 = a.qr_handle(25, &modules);
    let h2 = a.qr_handle(25, &modules);
    assert_eq!(h1.id(), h2.id());
    let other: Vec<bool> = modules.iter().map(|b| !b).collect();
    assert_ne!(a.qr_handle(25, &other).id(), h1.id());
    // Icons are cached too.
    assert_eq!(
        ferrocord::ui::icons::handle(ferrocord::ui::icons::Icon::Bell).id(),
        ferrocord::ui::icons::handle(ferrocord::ui::icons::Icon::Bell).id()
    );
}

// -- round 3 flows (mock mode) ---------------------------------------------------

mod round3 {
    use super::*;
    use ferrocord::app::panels::Popover;
    use ferrocord::app::rail::RailItem;
    use ferrocord::app::servers::{JoinStep, ServerDialog};
    use ferrocord::net::extras::{MuteDuration, NotifyLevel};
    use ferrocord::net::model::Snowflake;

    fn ui(a: &mut App, u: Ui) {
        let _ = a.update(Message::Ui(u));
    }

    #[test]
    fn popovers_toggle_and_switch() {
        let mut a = app();
        ui(&mut a, Ui::TogglePopover(Popover::Pins));
        assert_eq!(a.panels.open, Some(Popover::Pins));
        assert!(!a.panels.pins.items.is_empty());
        ui(&mut a, Ui::TogglePopover(Popover::Inbox));
        assert_eq!(a.panels.open, Some(Popover::Inbox));
        assert!(!a.panels.inbox.items.is_empty());
        ui(&mut a, Ui::TogglePopover(Popover::Inbox));
        assert_eq!(a.panels.open, None);
        ui(&mut a, Ui::TogglePopover(Popover::Bell));
        ui(&mut a, Ui::Escape);
        assert_eq!(a.panels.open, None);
        // Views render with every popover open.
        for k in [Popover::Bell, Popover::Pins, Popover::Inbox] {
            ui(&mut a, Ui::TogglePopover(k));
            let _ = ferrocord::ui::view(&a);
        }
    }

    #[test]
    fn bell_mutes_and_sets_levels() {
        let mut a = app();
        let c = mock::CH_GENERAL;
        ui(&mut a, Ui::MuteChannel(c, MuteDuration::Hour1));
        assert!(a.chat.is_channel_muted(c));
        ui(&mut a, Ui::SetChannelLevel(c, NotifyLevel::Mentions));
        assert_eq!(a.chat.channel_notify(c).level, NotifyLevel::Mentions);
        assert!(a.chat.is_channel_muted(c), "a level change keeps the mute");
        ui(&mut a, Ui::UnmuteChannel(c));
        assert!(!a.chat.is_channel_muted(c));
        ui(&mut a, Ui::SetChannelLevel(c, NotifyLevel::Default));
        assert!(a.chat.channel_notify.is_empty());
    }

    #[test]
    fn unpin_removes_from_the_list_in_mock_mode() {
        let mut a = app();
        ui(&mut a, Ui::TogglePopover(Popover::Pins));
        let first = a.panels.pins.items[0].clone();
        let n = a.panels.pins.items.len();
        ui(&mut a, Ui::Unpin(first.channel_id, first.id));
        assert_eq!(a.panels.pins.items.len(), n - 1);
    }

    #[test]
    fn jumping_closes_the_popover() {
        let mut a = app();
        ui(&mut a, Ui::TogglePopover(Popover::Inbox));
        let m = a.panels.inbox.items[0].clone();
        ui(
            &mut a,
            Ui::JumpTo {
                channel: m.channel_id,
                message: m.id,
            },
        );
        assert_eq!(a.panels.open, None);
    }

    #[test]
    fn search_filters_apply_and_bad_filters_report() {
        let mut a = app();
        ui(&mut a, Ui::SearchChanged("from:alice has:link".into()));
        ui(&mut a, Ui::SubmitSearch);
        assert!(!a.search.results.is_empty());
        assert!(a.search.results.iter().all(|m| m.content.contains("http")));
        assert!(a.search.results.iter().all(|m| m.author.id == Snowflake(2)));
        ui(&mut a, Ui::SearchChanged("from:nobody-here".into()));
        ui(&mut a, Ui::SubmitSearch);
        assert!(
            a.search
                .error
                .as_deref()
                .unwrap_or("")
                .contains("nobody-here")
        );
        assert!(a.search.results.is_empty());
    }

    #[test]
    fn create_server_flow() {
        let mut a = app();
        ui(&mut a, Ui::OpenAddServer);
        assert_eq!(a.server_dialog, Some(ServerDialog::Choose));
        ui(&mut a, Ui::AddServerCreate);
        ui(&mut a, Ui::CreateNameChanged("x".into()));
        ui(&mut a, Ui::SubmitCreate);
        assert!(a.server_dialog.is_some(), "a one-letter name is refused");
        ui(&mut a, Ui::CreateNameChanged("Brand New".into()));
        let before = a.chat.guilds.len();
        // Mock mode answers through a task; feed its result directly.
        let _ = a.update(Message::Done(ferrocord::app::Done::Created(Ok((
            Snowflake(900_002),
            "Brand New".into(),
        )))));
        assert_eq!(a.chat.guilds.len(), before + 1);
        assert!(a.server_dialog.is_none());
        assert!(matches!(a.view, View::Guild { guild, .. } if guild == Snowflake(900_002)));
        // New servers are appended to the rail.
        assert_eq!(
            a.chat.folders.flat_order().last(),
            Some(&Snowflake(900_002))
        );
    }

    #[test]
    fn join_flow_resolves_then_joins_and_reports_errors() {
        let mut a = app();
        ui(&mut a, Ui::OpenAddServer);
        ui(&mut a, Ui::AddServerJoin);
        ui(&mut a, Ui::JoinInputChanged("nonsense with spaces".into()));
        ui(&mut a, Ui::SubmitJoin);
        let Some(ServerDialog::Join(f)) = &a.server_dialog else {
            panic!()
        };
        assert!(f.error.is_some());
        ui(
            &mut a,
            Ui::JoinInputChanged("https://discord.gg/rust-lang".into()),
        );
        ui(&mut a, Ui::SubmitJoin);
        let Some(ServerDialog::Join(f)) = &a.server_dialog else {
            panic!()
        };
        let g = f.generation();
        let pv = mock::invite_preview("rust-lang").unwrap();
        let _ = a.update(Message::Done(ferrocord::app::Done::Invite {
            generation: g,
            result: Ok(pv.clone()),
        }));
        let Some(ServerDialog::Join(f)) = &a.server_dialog else {
            panic!()
        };
        assert_eq!(f.step, JoinStep::Preview(pv));
        // A captcha demand keeps the card and shows the message.
        ui(&mut a, Ui::ConfirmJoin);
        let Some(ServerDialog::Join(f)) = &a.server_dialog else {
            panic!()
        };
        let g = f.generation();
        let msg = ferrocord::error::Error::CaptchaRequired(Box::new(
            ferrocord::net::captcha::CaptchaChallenge::parse(r#"{"captcha_key":[]}"#).unwrap(),
        ))
        .user_message();
        let _ = a.update(Message::Done(ferrocord::app::Done::Joined {
            generation: g,
            result: Err(msg),
        }));
        let Some(ServerDialog::Join(f)) = &a.server_dialog else {
            panic!()
        };
        assert!(matches!(f.step, JoinStep::Preview(_)));
        assert!(f.error.as_deref().unwrap().contains("captcha"));
        let _ = ferrocord::ui::view(&a);
    }

    #[test]
    fn discover_loads_filters_and_joins() {
        let mut a = app();
        ui(&mut a, Ui::OpenDiscover);
        let d = a.discover.as_ref().unwrap();
        assert!(!d.guilds.is_empty() && !d.categories.is_empty());
        let all = d.guilds.len();
        ui(&mut a, Ui::DiscoverCategory(Some(3)));
        assert!(a.discover.as_ref().unwrap().guilds.len() < all);
        ui(&mut a, Ui::DiscoverCategory(None));
        ui(&mut a, Ui::DiscoverInput("iced".into()));
        ui(&mut a, Ui::DiscoverSubmit);
        assert_eq!(a.discover.as_ref().unwrap().guilds.len(), 1);
        let id = a.discover.as_ref().unwrap().guilds[0].id;
        ui(&mut a, Ui::DiscoverDetails(Some(id)));
        let _ = ferrocord::ui::view(&a);
        let _ = a.update(Message::Done(ferrocord::app::Done::DiscoverJoined {
            guild: id,
            result: Ok(()),
        }));
        assert!(a.discover.is_none(), "the page closes after joining");
        assert!(a.chat.guild(id).is_some());
        ui(&mut a, Ui::OpenDiscover);
        ui(&mut a, Ui::Escape);
        assert!(a.discover.is_none());
    }

    #[test]
    fn folders_create_drag_rename_remove_ungroup() {
        let mut a = app();
        let (rust, iced_g, linux) = (mock::GUILD_RUST, mock::GUILD_ICED, mock::GUILD_LINUX);
        // Create from the context menu.
        ui(&mut a, Ui::CreateFolder(rust));
        let fid = a.chat.folders.folder_of(rust).unwrap().id.unwrap();
        // Drag one server onto another: press, release elsewhere.
        ui(&mut a, Ui::RailPress(RailItem::Guild(iced_g)));
        ui(&mut a, Ui::RailRelease(RailItem::Guild(linux)));
        let f2 = a.chat.folders.folder_of(linux).unwrap();
        assert_eq!(f2.guild_ids, vec![linux, iced_g]);
        // Drop on a folder header.
        ui(&mut a, Ui::RailPress(RailItem::Guild(mock::GUILD_GAMES)));
        ui(&mut a, Ui::RailRelease(RailItem::Folder(fid)));
        assert!(
            a.chat
                .folders
                .folder(fid)
                .unwrap()
                .guild_ids
                .contains(&mock::GUILD_GAMES)
        );
        // A press that ended elsewhere (DragEnd) never drops.
        ui(&mut a, Ui::RailPress(RailItem::Guild(mock::GUILD_CAFE)));
        ui(&mut a, Ui::DragEnd);
        ui(&mut a, Ui::RailRelease(RailItem::Guild(rust)));
        assert!(a.chat.folders.folder_of(mock::GUILD_CAFE).is_none());
        // Settings dialog: rename and colour.
        ui(&mut a, Ui::OpenFolderSettings(fid));
        ui(&mut a, Ui::FolderNameChanged("Work".into()));
        ui(&mut a, Ui::FolderColor(Some(0xED4245)));
        ui(&mut a, Ui::SaveFolder);
        let f = a.chat.folders.folder(fid).unwrap();
        assert_eq!((f.name.as_deref(), f.color), (Some("Work"), Some(0xED4245)));
        // Remove a server from its folder, then ungroup.
        ui(&mut a, Ui::RemoveFromFolder(mock::GUILD_GAMES));
        ui(&mut a, Ui::UngroupFolder(fid));
        assert!(a.chat.folders.folder(fid).is_none());
        // Clicking toggles; the rail renders in every state.
        ui(&mut a, Ui::CreateFolder(mock::GUILD_CAFE));
        let cid = a
            .chat
            .folders
            .folder_of(mock::GUILD_CAFE)
            .unwrap()
            .id
            .unwrap();
        ui(&mut a, Ui::RailPress(RailItem::Folder(cid)));
        ui(&mut a, Ui::RailRelease(RailItem::Folder(cid)));
        assert!(
            !a.rail.expanded.contains(&cid),
            "created open, click closes"
        );
        ui(&mut a, Ui::RailMenu(RailItem::Guild(rust)));
        let _ = ferrocord::ui::view(&a);
        ui(&mut a, Ui::Escape);
        assert!(a.rail.menu.is_none());
    }

    #[test]
    fn overlays_do_not_reshape_the_tree() {
        use iced::advanced::widget::Tree;
        let mut a = app();
        let mut tree = {
            let el = ferrocord::ui::view(&a);
            Tree::new(&el)
        };
        let base = format!("{:?}", tree.children[0].children[1].tag);
        for open in [
            Ui::TogglePopover(Popover::Pins),
            Ui::OpenAddServer,
            Ui::OpenDiscover,
            Ui::OpenFolderSettings(0),
            Ui::RailMenu(RailItem::Guild(mock::GUILD_RUST)),
        ] {
            ui(&mut a, open);
            {
                let el = ferrocord::ui::view(&a);
                tree.diff(&el);
            }
            assert_eq!(tree.children.len(), ferrocord::ui::LAYER_SLOTS);
            assert_eq!(format!("{:?}", tree.children[0].children[1].tag), base);
            ui(&mut a, Ui::Escape);
            ui(&mut a, Ui::Escape);
        }
    }
}

#[test]
fn gif_picker_opens_searches_and_sends() {
    use ferrocord::app::Done;
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::OpenGifPicker));
    let p = a.gif_picker.as_ref().expect("picker open");
    assert_eq!(p.results.len(), 8, "trending loaded");
    // Typing only searches once the debounce timer of the last keystroke fires.
    let _ = a.update(Message::Ui(Ui::GifQuery("ca".into())));
    let _ = a.update(Message::Ui(Ui::GifQuery("cat".into())));
    let last = a.gif_picker.as_ref().unwrap().generation;
    let _ = a.update(Message::Done(Done::GifSearchDue(last - 1)));
    assert_eq!(a.gif_picker.as_ref().unwrap().results.len(), 8);
    let _ = a.update(Message::Done(Done::GifSearchDue(last)));
    assert_eq!(a.gif_picker.as_ref().unwrap().results.len(), 4);
    // Picking sends the Tenor URL as a message and closes the picker,
    // leaving the composer alone.
    let _ = a.update(Message::Ui(Ui::ComposerChanged("draft".into())));
    let url = a.gif_picker.as_ref().unwrap().results[0].url.clone();
    let _ = a.update(Message::Ui(Ui::PickGif(url.clone())));
    assert!(a.gif_picker.is_none());
    assert_eq!(a.composer.text(), "draft");
    let last_msg = a.chat.messages[&mock::CH_GENERAL].messages.back().unwrap();
    assert_eq!(last_msg.content, url);
}

#[test]
fn sticker_picker_sends_a_sticker() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::OpenStickerPicker));
    assert!(a.sticker_picker.is_some());
    let _ = a.update(Message::Ui(Ui::StickerQuery("party".into())));
    assert_eq!(a.sticker_picker.as_ref().unwrap().query, "party");
    let party = mock::mock_stickers()[1].clone();
    let _ = a.update(Message::Ui(Ui::PickSticker(party.id)));
    assert!(a.sticker_picker.is_none());
    // In mock mode the echo arrives as a task; apply it like the runtime would.
    let mut echo = mock::text_message(
        mock::CH_GENERAL,
        ferrocord::net::model::Snowflake::from_timestamp_ms(ferrocord::app::now_ms() + 60_000, 0),
        mock::user(1),
        "",
    );
    echo.stickers = vec![party.clone()];
    let _ = a.update(Message::Net(ferrocord::net::Event::MessageCreate(
        Box::new(echo),
    )));
    let last = a.chat.messages[&mock::CH_GENERAL].messages.back().unwrap();
    assert_eq!(last.stickers, vec![party]);
}

#[test]
fn switching_channel_closes_the_gif_and_sticker_pickers() {
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::OpenGifPicker));
    assert!(a.gif_picker.is_some());
    let _ = a.update(Message::Ui(Ui::SelectChannel(mock::CH_HELP)));
    assert!(a.gif_picker.is_none());
    let _ = a.update(Message::Ui(Ui::OpenStickerPicker));
    assert!(a.sticker_picker.is_some());
    let _ = a.update(Message::Ui(Ui::SelectChannel(mock::CH_GENERAL)));
    assert!(a.sticker_picker.is_none());
}

// -- voice channels and calls ----------------------------------------------------

mod voice {
    use super::app;
    use ferrocord::app::voice::{Phase, VoiceUi};
    use ferrocord::app::{Message, Ui, View};
    use ferrocord::mock;
    use ferrocord::net::Event;
    use ferrocord::net::model::*;

    fn v(u: VoiceUi) -> Message {
        Message::Ui(Ui::Voice(u))
    }

    #[test]
    fn mock_ready_fills_voice_channels() {
        let a = app();
        let lounge: Vec<u64> = a
            .voice
            .channel_members(mock::VC_LOUNGE)
            .iter()
            .map(|s| s.user_id.0)
            .collect();
        assert_eq!(lounge, vec![3, 4]);
        assert!(a.voice.conn.is_none());
    }

    #[test]
    fn join_mute_deafen_leave() {
        let mut a = app();
        let _ = a.update(v(VoiceUi::Join(mock::VC_LOUNGE)));
        let c = a.voice.conn.clone().unwrap();
        assert_eq!(c.channel, mock::VC_LOUNGE);
        assert_eq!(c.guild, Some(mock::GUILD_RUST));
        assert_eq!(c.phase, Phase::Connected);
        assert!(
            a.voice
                .channel_members(mock::VC_LOUNGE)
                .iter()
                .any(|s| s.user_id == mock::ME)
        );
        // The text channel stays open (joining voice does not navigate).
        assert_eq!(a.open_channel(), Some(mock::CH_GENERAL));

        let _ = a.update(v(VoiceUi::ToggleMute));
        assert!(a.voice.self_mute && a.voice.control.self_mute());
        assert!(a.voice.members[&mock::ME].self_mute);
        let _ = a.update(v(VoiceUi::ToggleDeafen));
        assert!(a.voice.self_deaf && a.voice.control.self_deaf());
        assert!(a.voice.members[&mock::ME].self_deaf);
        let _ = a.update(v(VoiceUi::ToggleDeafen));
        assert!(!a.voice.self_deaf && a.voice.self_mute);
        let _ = a.update(v(VoiceUi::ToggleMute));
        assert!(!a.voice.control.self_mute());

        // Switching channels moves us.
        let _ = a.update(v(VoiceUi::Join(mock::VC_PAIR)));
        assert_eq!(a.voice.connected_channel(), Some(mock::VC_PAIR));

        let _ = a.update(v(VoiceUi::Leave));
        assert!(a.voice.conn.is_none());
        assert!(!a.voice.members.contains_key(&mock::ME));
    }

    #[test]
    fn text_channels_cannot_be_joined_as_voice() {
        let mut a = app();
        let _ = a.update(v(VoiceUi::Join(mock::CH_GENERAL)));
        assert!(a.voice.conn.is_none());
        let _ = a.update(v(VoiceUi::StartCall(mock::CH_GENERAL)));
        assert!(a.voice.conn.is_none());
    }

    #[test]
    fn others_joining_and_speaking_update_the_list() {
        let mut a = app();
        let vs = VoiceState {
            guild_id: Some(mock::GUILD_RUST),
            channel_id: Some(mock::VC_PAIR),
            user_id: Snowflake(2),
            ..Default::default()
        };
        let _ = a.update(Message::Net(Event::VoiceState(Box::new(vs.clone()))));
        assert!(
            a.voice
                .channel_members(mock::VC_PAIR)
                .iter()
                .any(|s| s.user_id.0 == 2)
        );
        let _ = a.update(v(VoiceUi::Join(mock::VC_PAIR)));
        let _ = a.update(Message::Voice(
            ferrocord::net::voice::VoiceEvent::Speaking {
                user: 2,
                speaking: true,
            },
        ));
        assert!(a.voice.is_speaking(Snowflake(2)));
        let left = VoiceState {
            channel_id: None,
            ..vs
        };
        let _ = a.update(Message::Net(Event::VoiceState(Box::new(left))));
        assert!(
            !a.voice
                .channel_members(mock::VC_PAIR)
                .iter()
                .any(|s| s.user_id.0 == 2)
        );
        assert!(!a.voice.is_speaking(Snowflake(2)));
    }

    fn ring(a: &mut ferrocord::app::App, ringing: Vec<UserId>) {
        let call = CallState {
            channel_id: mock::DM_ALICE,
            message_id: None,
            ringing,
            voice_states: vec![VoiceState {
                channel_id: Some(mock::DM_ALICE),
                user_id: Snowflake(2),
                ..Default::default()
            }],
            unavailable: false,
        };
        let _ = a.update(Message::Net(Event::Call(Box::new(call))));
    }

    #[test]
    fn incoming_call_accept() {
        let mut a = app();
        ring(&mut a, vec![mock::ME]);
        assert_eq!(a.voice.incoming, Some(mock::DM_ALICE));
        // The tray asks for attention while it rings.
        assert_eq!(a.tray_model().mentions, a.chat.attention_count() + 1);
        let _ = a.update(v(VoiceUi::AcceptCall(mock::DM_ALICE)));
        assert_eq!(a.voice.incoming, None);
        assert_eq!(a.voice.connected_channel(), Some(mock::DM_ALICE));
        assert_eq!(a.view, View::Home(Some(mock::DM_ALICE)));
        assert_eq!(a.tray_model().mentions, a.chat.attention_count());
        let _ = a.update(Message::Net(Event::CallDelete(mock::DM_ALICE)));
        assert!(!a.voice.calls.contains_key(&mock::DM_ALICE));
    }

    #[test]
    fn incoming_call_decline() {
        let mut a = app();
        ring(&mut a, vec![mock::ME]);
        let _ = a.update(v(VoiceUi::DeclineCall(mock::DM_ALICE)));
        assert_eq!(a.voice.incoming, None);
        assert!(a.voice.conn.is_none());
        // CALL_UPDATE still ringing: stays dismissed.
        ring(&mut a, vec![mock::ME]);
        assert_eq!(a.voice.incoming, None);
    }

    #[test]
    fn start_call_from_dm_header() {
        let mut a = app();
        let _ = a.update(Message::Ui(Ui::SelectChannel(mock::DM_BOB)));
        let _ = a.update(v(VoiceUi::StartCall(mock::DM_BOB)));
        let c = a.voice.conn.clone().unwrap();
        assert_eq!((c.guild, c.channel), (None, mock::DM_BOB));
        assert!(a.voice.active_call(mock::DM_BOB));
    }

    #[test]
    fn user_volume_menu_and_settings() {
        let mut a = app();
        let _ = a.update(v(VoiceUi::UserMenu(Snowflake(3))));
        assert_eq!(a.voice.user_menu.map(|m| m.user), Some(Snowflake(3)));
        let _ = a.update(v(VoiceUi::UserVolume(Snowflake(3), 50.0)));
        assert_eq!(a.voice.control.gain(3), 0.5);
        let _ = a.update(v(VoiceUi::ToggleUserMute(Snowflake(3))));
        assert_eq!(a.voice.control.gain(3), 0.0);
        let _ = a.update(Message::Ui(Ui::Escape));
        let _ = a.update(v(VoiceUi::CloseUserMenu));
        assert!(a.voice.user_menu.is_none());
        let _ = a.update(v(VoiceUi::Threshold(-30.0)));
        assert_eq!(a.voice.control.threshold_db(), -30.0);
        let _ = a.update(Message::Ui(Ui::OpenSettings));
        assert!(!a.voice.devices.0.is_empty());
        let _ = a.update(v(VoiceUi::InputDevice(Some("USB Headset".into()))));
        assert_eq!(
            a.voice.control.devices().input.as_deref(),
            Some("USB Headset")
        );
    }

    #[test]
    fn call_from_profile_popout() {
        let mut a = app();
        // Alice (2) has a 1:1 DM; we never call ourselves.
        let dm = a.dm_with(Snowflake(2)).expect("DM with Alice");
        assert_eq!(dm, mock::DM_ALICE);
        assert_eq!(a.dm_with(mock::ME), None);
        let _ = a.update(Message::Ui(Ui::OpenProfile {
            user: Snowflake(2),
            guild: None,
        }));
        assert!(a.profiles.popout.is_some());
        let _ = a.update(v(VoiceUi::StartCall(dm)));
        assert!(a.profiles.popout.is_none());
        assert_eq!(a.voice.connected_channel(), Some(mock::DM_ALICE));
    }

    #[test]
    fn logout_leaves_voice() {
        let mut a = app();
        let _ = a.update(v(VoiceUi::Join(mock::VC_LOUNGE)));
        let _ = a.update(Message::Ui(Ui::Logout));
        assert!(a.voice.conn.is_none());
        assert!(a.voice.members.is_empty());
    }
}
// -- composer (text_editor) ---------------------------------------------------

fn type_text(a: &mut App, s: &str) {
    use iced::widget::text_editor::{Action, Edit};
    for ch in s.chars() {
        let edit = if ch == '\n' {
            Edit::Enter
        } else {
            Edit::Insert(ch)
        };
        ui(a, Ui::ComposerAction(Action::Edit(edit)));
    }
}

#[test]
fn multi_line_text_is_sent_with_its_newlines() {
    let mut a = app();
    type_text(&mut a, "first\nsecond\n\nthird");
    assert_eq!(a.composer.text(), "first\nsecond\n\nthird");
    ui(&mut a, Ui::Send);
    assert!(a.composer.is_empty());
    let c = a.open_channel().expect("channel");
    let last = a.chat.messages[&c]
        .messages
        .back()
        .expect("pending")
        .clone();
    assert_eq!(last.content, "first\nsecond\n\nthird");
}

#[test]
fn drafts_are_kept_per_channel() {
    let mut a = app();
    ui(&mut a, Ui::SelectChannel(mock::CH_GENERAL));
    type_text(&mut a, "half written\nline two");
    ui(&mut a, Ui::SelectChannel(mock::CH_HELP));
    assert!(a.composer.is_empty());
    type_text(&mut a, "other");
    ui(&mut a, Ui::SelectChannel(mock::CH_GENERAL));
    assert_eq!(a.composer.text(), "half written\nline two");
    ui(&mut a, Ui::SelectChannel(mock::CH_HELP));
    assert_eq!(a.composer.text(), "other");
    // Sending clears the draft for good.
    ui(&mut a, Ui::Send);
    ui(&mut a, Ui::SelectChannel(mock::CH_GENERAL));
    ui(&mut a, Ui::SelectChannel(mock::CH_HELP));
    assert!(a.composer.is_empty());
}

#[test]
fn editing_a_multi_line_message_keeps_its_lines() {
    let mut a = app();
    let c = a.open_channel().expect("channel");
    // Up arrow edits the last own message; give it two lines first.
    ui(&mut a, Ui::EditLast);
    let id = a.editing.as_ref().expect("editing").message;
    ui(&mut a, Ui::CancelEdit);
    a.chat.set_content(c, id, "one\ntwo ü🍕");
    ui(&mut a, Ui::EditLast);
    let e = a.editing.clone().expect("editing");
    assert_eq!(e.text.text(), "one\ntwo ü🍕");
    type_text_edit(&mut a, "\nthree");
    ui(&mut a, Ui::SubmitEdit);
    assert!(a.editing.is_none());
    let m = a.chat.message(c, id).expect("message");
    assert_eq!(m.content, "one\ntwo ü🍕\nthree");
}

fn type_text_edit(a: &mut App, s: &str) {
    use iced::widget::text_editor::{Action, Edit};
    for ch in s.chars() {
        let edit = if ch == '\n' {
            Edit::Enter
        } else {
            Edit::Insert(ch)
        };
        ui(a, Ui::EditAction(Action::Edit(edit)));
    }
}

// -- leaving Discover ---------------------------------------------------------

#[test]
fn picking_a_destination_leaves_discover() {
    let g = mock::GUILD_RUST;
    for dest in [
        Ui::SelectHome,
        Ui::SelectGuild(g),
        Ui::SelectChannel(mock::CH_HELP),
        Ui::Escape,
        Ui::CloseDiscover,
    ] {
        let mut a = app();
        ui(&mut a, Ui::OpenDiscover);
        assert!(a.discover.is_some());
        ui(&mut a, dest.clone());
        assert!(a.discover.is_none(), "{dest:?} must leave Discover");
    }
}

#[test]
fn clicking_the_current_server_still_leaves_discover() {
    let mut a = app();
    assert!(matches!(a.view, View::Guild { guild, .. } if guild == mock::GUILD_RUST));
    ui(&mut a, Ui::OpenDiscover);
    ui(&mut a, Ui::SelectGuild(mock::GUILD_RUST));
    assert!(a.discover.is_none());
    assert!(matches!(a.view, View::Guild { guild, .. } if guild == mock::GUILD_RUST));
}

#[test]
fn gif_picker_home_favorites_and_star() {
    use ferrocord::app::gifs::GifPage;
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::SelectChannel(mock::CH_GENERAL)));
    let _ = a.update(Message::Ui(Ui::OpenGifPicker));
    let p = a.gif_picker.as_ref().unwrap();
    // Opens on the Favorites / Trending GIFs tiles, like the official picker.
    assert_eq!(p.page, GifPage::Home);
    assert_eq!(p.results.len(), 8);
    // Every mock GIF has its still ready (made once, outside `view`).
    for g in &p.results {
        assert!(a.gif_thumb(&g.url).is_some(), "{}", g.url);
    }
    let saved: Vec<String> = a.gif_favorites.items().into_iter().map(|g| g.url).collect();
    assert_eq!(saved, [mock::gif_page_url(3), mock::gif_page_url(6)]);

    let _ = a.update(Message::Ui(Ui::GifPage(GifPage::Favorites)));
    assert_eq!(a.gif_picker.as_ref().unwrap().page, GifPage::Favorites);

    // Star a trending GIF: saved first; star it again: gone.
    let first = a.gif_picker.as_ref().unwrap().results[0].clone();
    let _ = a.update(Message::Ui(Ui::ToggleGifFavorite(Box::new(first.clone()))));
    assert_eq!(a.gif_favorites.items()[0].url, first.url);
    let _ = a.update(Message::Ui(Ui::ToggleGifFavorite(Box::new(first.clone()))));
    assert!(!a.gif_favorites.contains(&first.url));
    assert_eq!(a.gif_favorites.items().len(), 2);
    // The thumb handle is the same across views (no per-frame re-upload).
    let id = a.gif_thumb(&first.url).unwrap().id();
    let _ = ferrocord::ui::view_window(&a, iced::window::Id::unique());
    assert_eq!(a.gif_thumb(&first.url).unwrap().id(), id);
}

#[test]
fn gif_picker_tiles_animate_near_view_and_free_their_frames() {
    use ferrocord::app::Done;
    use ferrocord::app::gifs::GifPage;
    use std::sync::Arc;
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::OpenGifPicker));
    let _ = a.update(Message::Ui(Ui::GifPage(GifPage::Trending)));
    let urls: Vec<String> = a
        .gif_picker
        .as_ref()
        .unwrap()
        .results
        .iter()
        .map(|g| g.url.clone())
        .collect();
    // A decoded tile (the mock decodes like a real GIF, through the
    // picker limits) lands in the picker cache and the view draws it.
    let anim = mock::gif_anim(1).map(Arc::new);
    assert!(anim.as_ref().is_some_and(|x| x.frames.len() == 32));
    let _ = a.update(Message::Done(Done::GifAnim {
        key: urls[0].clone(),
        anim: anim.clone(),
    }));
    let got = a.gif_anim(&urls[0]).expect("tile animation cached").clone();
    // Same frames (same handles) on every view rebuild.
    let _ = ferrocord::ui::view(&a);
    let _ = ferrocord::ui::view(&a);
    assert!(Arc::ptr_eq(&got, a.gif_anim(&urls[0]).unwrap()));
    // A result for a GIF the picker does not show is not kept.
    let _ = a.update(Message::Done(Done::GifAnim {
        key: "https://tenor.example/view/elsewhere".into(),
        anim: anim.clone(),
    }));
    assert!(a.gif_anim("https://tenor.example/view/elsewhere").is_none());
    // Scrolling far away drops the frames of tiles out of view (only 8
    // mock tiles: scroll beyond them).
    let _ = a.update(Message::Ui(Ui::GifScrolled(5000.0)));
    assert!(
        a.gif_anim(&urls[0]).is_none(),
        "far tiles lose their frames"
    );
    let _ = a.update(Message::Ui(Ui::GifScrolled(0.0)));
    let _ = a.update(Message::Done(Done::GifAnim {
        key: urls[0].clone(),
        anim,
    }));
    assert!(a.gif_anim(&urls[0]).is_some());
    assert!(a.gif_anims.cache.bytes() <= ferrocord::app::gif_anim::PICKER_BUDGET);
    // Closing the picker frees every frame: nothing animates when closed.
    let _ = a.update(Message::Ui(Ui::CloseGifPicker));
    assert!(a.gif_anims.cache.is_empty());
    assert!(a.gif_anim(&urls[0]).is_none());
    // Mock mode never writes a disk cache.
    assert!(a.gif_disk.is_none());
}

// -- custom emoji and stickers without Nitro ------------------------------------

fn iced_emoji(a: &App, id: u64) -> Emoji {
    let (_, e) = ferrocord::app::emoji::find_custom(&a.chat.guilds, Snowflake(id)).unwrap();
    Emoji::custom(e)
}

#[test]
fn locked_emoji_offers_send_as_image_instead_of_inserting() {
    let mut a = app();
    assert_eq!(a.chat.premium_type, 0);
    ui(&mut a, Ui::OpenEmojiPicker(PickerTarget::Composer));
    // From this server: inserted as usual.
    let ferris = iced_emoji(&a, 1001);
    ui(&mut a, Ui::PickEmoji(ferris));
    assert_eq!(a.composer.text(), "<:ferris:1001> ");
    // From another server: the picker stays open with the lock footer.
    ui(&mut a, Ui::OpenEmojiPicker(PickerTarget::Composer));
    ui(&mut a, Ui::EmojiSection(Section::Server(mock::GUILD_ICED)));
    let cube = iced_emoji(&a, 1003);
    ui(&mut a, Ui::PickEmoji(cube.clone()));
    let open = a.emoji_picker.as_ref().unwrap();
    assert_eq!(open.picker.locked, Some(cube));
    assert_eq!(a.composer.text(), "<:ferris:1001> ");
    let _ = ferrocord::ui::view(&a);
    // Dismiss, then the explicit choice closes the picker and starts it.
    ui(&mut a, Ui::DismissLocked);
    assert!(a.emoji_picker.as_ref().unwrap().picker.locked.is_none());
    let spinner = iced_emoji(&a, 1004);
    ui(&mut a, Ui::PickEmoji(spinner));
    ui(&mut a, Ui::SendLockedAsImage);
    assert!(a.emoji_picker.is_none());
    assert_eq!(a.composer.text(), "<:ferris:1001> ");
}

#[test]
fn locked_emoji_is_not_used_as_a_reaction() {
    let mut a = app();
    let m = last_of(&a, 3);
    let before = m.reactions.len();
    ui(
        &mut a,
        Ui::OpenEmojiPicker(PickerTarget::Reaction {
            channel: mock::CH_GENERAL,
            message: m.id,
        }),
    );
    let cube = iced_emoji(&a, 1003);
    ui(&mut a, Ui::PickEmoji(cube.clone()));
    assert_eq!(
        a.chat
            .message(mock::CH_GENERAL, m.id)
            .unwrap()
            .reactions
            .len(),
        before
    );
    assert_eq!(a.emoji_picker.as_ref().unwrap().picker.locked, Some(cube));
    // No image can be sent as a reaction: nothing happens.
    ui(&mut a, Ui::SendLockedAsImage);
    assert!(a.emoji_picker.is_some());
    let _ = ferrocord::ui::view(&a);
}

#[test]
fn nitro_users_use_every_emoji_normally() {
    let mut a = app();
    a.chat.premium_type = 2;
    ui(&mut a, Ui::OpenEmojiPicker(PickerTarget::Composer));
    let spinner = iced_emoji(&a, 1004);
    ui(&mut a, Ui::PickEmoji(spinner));
    assert!(a.emoji_picker.is_none());
    assert_eq!(a.composer.text(), "<a:spinner:1004> ");
}

#[test]
fn locked_stickers_show_the_choice_and_lottie_cannot_be_sent() {
    let mut a = app();
    ui(&mut a, Ui::OpenStickerPicker);
    ui(&mut a, Ui::PickSticker(Snowflake(1106)));
    assert_eq!(
        a.sticker_picker.as_ref().unwrap().locked,
        Some(Snowflake(1106))
    );
    let _ = ferrocord::ui::view(&a);
    // Lottie: no image to send, the picker stays with its explanation.
    ui(&mut a, Ui::SendLockedAsImage);
    assert!(a.sticker_picker.is_some());
    ui(&mut a, Ui::PickSticker(Snowflake(1105)));
    assert_eq!(
        a.sticker_picker.as_ref().unwrap().locked,
        Some(Snowflake(1105))
    );
    ui(&mut a, Ui::SendLockedAsImage);
    assert!(a.sticker_picker.is_none());
}

#[test]
fn image_copies_upload_as_an_attachment() {
    let mut a = app();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ferrocord-image-test.png");
    std::fs::write(&path, b"\x89PNG fake").unwrap();
    let _ = a.update(Message::Done(Done::ImageCopy {
        channel: mock::CH_GENERAL,
        reply: None,
        result: Ok(UploadFile {
            path,
            name: "iced_cube.png".into(),
            size: 9,
        }),
    }));
    assert_eq!(a.uploads.len(), 1);
    assert_eq!(a.uploads[0].label, "iced_cube.png");
    assert_eq!(a.uploads[0].channel, mock::CH_GENERAL);
    let _ = a.update(Message::Done(Done::ImageCopy {
        channel: mock::CH_GENERAL,
        reply: None,
        result: Err("That image is too large to convert.".into()),
    }));
    assert!(a.toast.as_deref().unwrap().contains("too large"));
}

#[test]
fn gif_picker_honours_autoplay_and_reduce_motion() {
    use ferrocord::app::Done;
    use ferrocord::app::gifs::GifPage;
    use std::sync::Arc;
    let mut a = app();
    let _ = a.update(Message::Ui(Ui::OpenGifPicker));
    let _ = a.update(Message::Ui(Ui::GifPage(GifPage::Trending)));
    let urls: Vec<String> = a
        .gif_picker
        .as_ref()
        .unwrap()
        .results
        .iter()
        .map(|g| g.url.clone())
        .collect();
    let anim = mock::gif_anim(1).map(Arc::new);
    // GIF autoplay off: tiles stay still; only a hovered tile is decoded
    // and plays.
    a.prefs.autoplay_gifs = false;
    let _ = a.update(Message::Done(Done::GifAnim {
        key: urls[0].clone(),
        anim: anim.clone(),
    }));
    assert!(a.gif_anim(&urls[0]).is_none(), "not hovered: not kept");
    assert!(!a.picker_playing(&urls[0]));
    let _ = a.update(Message::Ui(Ui::GifHoverEnter(urls[0].clone())));
    let _ = a.update(Message::Done(Done::GifAnim {
        key: urls[0].clone(),
        anim: anim.clone(),
    }));
    assert!(a.gif_anim(&urls[0]).is_some());
    assert!(a.picker_playing(&urls[0]));
    assert!(!a.picker_playing(&urls[1]));
    // A late leave of another tile keeps the hover; leaving drops frames.
    let _ = a.update(Message::Ui(Ui::GifHoverLeave(urls[1].clone())));
    assert!(a.picker_playing(&urls[0]));
    let _ = a.update(Message::Ui(Ui::GifHoverLeave(urls[0].clone())));
    assert!(!a.picker_playing(&urls[0]));
    assert!(a.gif_anim(&urls[0]).is_none());
    // Autoplay on: every tile near view plays.
    a.prefs.autoplay_gifs = true;
    let _ = a.update(Message::Done(Done::GifAnim {
        key: urls[0].clone(),
        anim: anim.clone(),
    }));
    assert!(a.gif_anim(&urls[0]).is_some());
    assert!(a.picker_playing(&urls[0]) && a.picker_playing(&urls[1]));
    // Reduce motion: stills only.
    a.prefs.reduce_motion = true;
    assert!(!a.picker_playing(&urls[0]));
    assert!(a.gif_anim(&urls[0]).is_none());
}
