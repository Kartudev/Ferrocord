//! Lucide icons (ISC licence, see `assets/icons/LICENSE`), embedded at compile time.
//! `gif.svg` is Ferrocord's own, drawn in the same style.

use std::sync::LazyLock;

use iced::widget::svg::{self, Handle};
use iced::{Color, Element, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    AtSign,
    Bell,
    ChevronDown,
    ChevronRight,
    CirclePlus,
    Compass,
    Download,
    EyeOff,
    File,
    Gif,
    Hash,
    Headphones,
    Image,
    Inbox,
    Lock,
    LogOut,
    Megaphone,
    MessageCircle,
    MessageSquareText,
    Mic,
    Paperclip,
    Pin,
    Plus,
    QrCode,
    Search,
    Settings,
    Smile,
    Sticker,
    TriangleAlert,
    UserRound,
    Users,
    Volume2,
    WifiOff,
    X,
    ArrowDown,
    ArrowUp,
    Check,
    Copy,
    Ellipsis,
    Link,
    Pencil,
    Reply,
    ReplySpine,
    SmilePlus,
    Trash,
    BellOff,
    Folder,
    Upload,
    ArrowLeft,
    PinOff,
    MicOff,
    HeadphoneOff,
    Phone,
    PhoneOff,
    Signal,
    ShieldCheck,
    VolumeX,
    Play,
    Pause,
    Maximize,
    Minimize,
    RotateCcw,
    ExternalLink,
    Star,
    StarFilled,
    Clock,
    Mail,
    UserPlus,
}

const ALL: [(Icon, &[u8]); 68] = [
    (
        Icon::AtSign,
        include_bytes!("../../assets/icons/at-sign.svg"),
    ),
    (Icon::Bell, include_bytes!("../../assets/icons/bell.svg")),
    (
        Icon::ChevronDown,
        include_bytes!("../../assets/icons/chevron-down.svg"),
    ),
    (
        Icon::ChevronRight,
        include_bytes!("../../assets/icons/chevron-right.svg"),
    ),
    (
        Icon::CirclePlus,
        include_bytes!("../../assets/icons/circle-plus.svg"),
    ),
    (
        Icon::Compass,
        include_bytes!("../../assets/icons/compass.svg"),
    ),
    (
        Icon::Download,
        include_bytes!("../../assets/icons/download.svg"),
    ),
    (
        Icon::EyeOff,
        include_bytes!("../../assets/icons/eye-off.svg"),
    ),
    (Icon::File, include_bytes!("../../assets/icons/file.svg")),
    (Icon::Gif, include_bytes!("../../assets/icons/gif.svg")),
    (Icon::Hash, include_bytes!("../../assets/icons/hash.svg")),
    (
        Icon::Headphones,
        include_bytes!("../../assets/icons/headphones.svg"),
    ),
    (Icon::Image, include_bytes!("../../assets/icons/image.svg")),
    (Icon::Inbox, include_bytes!("../../assets/icons/inbox.svg")),
    (Icon::Lock, include_bytes!("../../assets/icons/lock.svg")),
    (
        Icon::LogOut,
        include_bytes!("../../assets/icons/log-out.svg"),
    ),
    (
        Icon::Megaphone,
        include_bytes!("../../assets/icons/megaphone.svg"),
    ),
    (
        Icon::MessageCircle,
        include_bytes!("../../assets/icons/message-circle.svg"),
    ),
    (
        Icon::MessageSquareText,
        include_bytes!("../../assets/icons/message-square-text.svg"),
    ),
    (Icon::Mic, include_bytes!("../../assets/icons/mic.svg")),
    (
        Icon::Paperclip,
        include_bytes!("../../assets/icons/paperclip.svg"),
    ),
    (Icon::Pin, include_bytes!("../../assets/icons/pin.svg")),
    (Icon::Plus, include_bytes!("../../assets/icons/plus.svg")),
    (
        Icon::QrCode,
        include_bytes!("../../assets/icons/qr-code.svg"),
    ),
    (
        Icon::Search,
        include_bytes!("../../assets/icons/search.svg"),
    ),
    (
        Icon::Settings,
        include_bytes!("../../assets/icons/settings.svg"),
    ),
    (Icon::Smile, include_bytes!("../../assets/icons/smile.svg")),
    (
        Icon::Sticker,
        include_bytes!("../../assets/icons/sticker.svg"),
    ),
    (
        Icon::TriangleAlert,
        include_bytes!("../../assets/icons/triangle-alert.svg"),
    ),
    (
        Icon::UserRound,
        include_bytes!("../../assets/icons/user-round.svg"),
    ),
    (Icon::Users, include_bytes!("../../assets/icons/users.svg")),
    (
        Icon::Volume2,
        include_bytes!("../../assets/icons/volume-2.svg"),
    ),
    (
        Icon::WifiOff,
        include_bytes!("../../assets/icons/wifi-off.svg"),
    ),
    (Icon::X, include_bytes!("../../assets/icons/x.svg")),
    (
        Icon::ArrowDown,
        include_bytes!("../../assets/icons/arrow-down.svg"),
    ),
    (
        Icon::ArrowUp,
        include_bytes!("../../assets/icons/arrow-up.svg"),
    ),
    (Icon::Check, include_bytes!("../../assets/icons/check.svg")),
    (Icon::Copy, include_bytes!("../../assets/icons/copy.svg")),
    (
        Icon::Ellipsis,
        include_bytes!("../../assets/icons/ellipsis.svg"),
    ),
    (Icon::Link, include_bytes!("../../assets/icons/link.svg")),
    (
        Icon::Pencil,
        include_bytes!("../../assets/icons/pencil.svg"),
    ),
    (Icon::Reply, include_bytes!("../../assets/icons/reply.svg")),
    (
        Icon::ReplySpine,
        include_bytes!("../../assets/icons/reply-spine.svg"),
    ),
    (
        Icon::SmilePlus,
        include_bytes!("../../assets/icons/smile-plus.svg"),
    ),
    (
        Icon::Trash,
        include_bytes!("../../assets/icons/trash-2.svg"),
    ),
    (
        Icon::BellOff,
        include_bytes!("../../assets/icons/bell-off.svg"),
    ),
    (
        Icon::Folder,
        include_bytes!("../../assets/icons/folder.svg"),
    ),
    (
        Icon::Upload,
        include_bytes!("../../assets/icons/upload.svg"),
    ),
    (
        Icon::ArrowLeft,
        include_bytes!("../../assets/icons/arrow-left.svg"),
    ),
    (
        Icon::PinOff,
        include_bytes!("../../assets/icons/pin-off.svg"),
    ),
    (
        Icon::MicOff,
        include_bytes!("../../assets/icons/mic-off.svg"),
    ),
    (
        Icon::HeadphoneOff,
        include_bytes!("../../assets/icons/headphone-off.svg"),
    ),
    (Icon::Phone, include_bytes!("../../assets/icons/phone.svg")),
    (
        Icon::PhoneOff,
        include_bytes!("../../assets/icons/phone-off.svg"),
    ),
    (
        Icon::Signal,
        include_bytes!("../../assets/icons/signal.svg"),
    ),
    (
        Icon::ShieldCheck,
        include_bytes!("../../assets/icons/shield-check.svg"),
    ),
    (
        Icon::VolumeX,
        include_bytes!("../../assets/icons/volume-x.svg"),
    ),
    (Icon::Play, include_bytes!("../../assets/icons/play.svg")),
    (Icon::Pause, include_bytes!("../../assets/icons/pause.svg")),
    (
        Icon::Maximize,
        include_bytes!("../../assets/icons/maximize.svg"),
    ),
    (
        Icon::Minimize,
        include_bytes!("../../assets/icons/minimize.svg"),
    ),
    (
        Icon::RotateCcw,
        include_bytes!("../../assets/icons/rotate-ccw.svg"),
    ),
    (
        Icon::ExternalLink,
        include_bytes!("../../assets/icons/external-link.svg"),
    ),
    (Icon::Star, include_bytes!("../../assets/icons/star.svg")),
    (
        Icon::StarFilled,
        include_bytes!("../../assets/icons/star-filled.svg"),
    ),
    (Icon::Clock, include_bytes!("../../assets/icons/clock.svg")),
    (Icon::Mail, include_bytes!("../../assets/icons/mail.svg")),
    (
        Icon::UserPlus,
        include_bytes!("../../assets/icons/user-plus.svg"),
    ),
];

/// Handles are created once so iced can cache the rasterised icons.
static HANDLES: LazyLock<Vec<(Icon, Handle)>> = LazyLock::new(|| {
    ALL.iter()
        .map(|(i, bytes)| (*i, Handle::from_memory(*bytes)))
        .collect()
});

pub fn handle(icon: Icon) -> Handle {
    HANDLES
        .iter()
        .find(|(i, _)| *i == icon)
        .map(|(_, h)| h.clone())
        .unwrap_or_else(|| Handle::from_memory(&b"<svg xmlns='http://www.w3.org/2000/svg'/>"[..]))
}

/// An icon tinted with `color`.
pub fn icon<'a, M: 'a>(icon: Icon, size: f32, color: Color) -> Element<'a, M> {
    iced::widget::svg(handle(icon))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg() {
        for (_, bytes) in ALL {
            let s = std::str::from_utf8(bytes).unwrap();
            assert!(s.contains("<svg"));
            assert!(s.contains("currentColor"));
        }
    }
}
