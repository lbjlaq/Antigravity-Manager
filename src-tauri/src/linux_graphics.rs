//! Linux GDK / WebKit startup policy.
//!
//! niri / Hyprland / sway and KDE Plasma always expose an Xwayland `DISPLAY`.
//! The old "Wayland + DISPLAY ⇒ force GDK_BACKEND=x11" rule then sends WebKitGTK
//! down a path that draws a black window or freezes frame updates (issues #3388, #3581).
//!
//! This module is std-only so the decision table can be tested with
//! `rustc --test src/linux_graphics.rs` without building the Tauri crate.

/// Compositors that keep an Xwayland `DISPLAY` even for native Wayland clients.
pub const WLROOTS_FAMILY_DESKTOPS: &[&str] =
    &["niri", "hyprland", "sway", "river", "labwc", "wayfire"];

pub fn desktop_is_wlroots_family(xdg_current_desktop: &str) -> bool {
    xdg_current_desktop.split(':').any(|part| {
        let name = part.trim().to_ascii_lowercase();
        WLROOTS_FAMILY_DESKTOPS.iter().any(|known| name == *known)
    })
}

pub fn desktop_is_kde(xdg_current_desktop: &str) -> bool {
    xdg_current_desktop.split(':').any(|part| {
        let name = part.trim().to_ascii_lowercase();
        name == "kde" || name == "plasma"
    })
}

pub fn desktop_is_gnome(xdg_current_desktop: &str) -> bool {
    xdg_current_desktop.split(':').any(|part| {
        let name = part.trim().to_ascii_lowercase();
        name == "gnome"
    })
}

pub fn should_force_x11_backend(
    gdk_backend_already_set: bool,
    force_x11: bool,
    force_wayland: bool,
    is_wayland: bool,
    has_x11_display: bool,
    xdg_current_desktop: &str,
) -> bool {
    if gdk_backend_already_set {
        return false;
    }
    if force_x11 {
        return true;
    }
    if force_wayland {
        return false;
    }
    // Historical GTK Wayland shm workaround for legacy environments.
    // Modern Wayland compositors (wlroots, KDE Plasma, GNOME, etc.) run WebKitGTK natively.
    // Forcing X11 (Xwayland) there causes WebKitGTK to draw a black window, freeze frame updates,
    // or fail transparent window composition (issues #3388, #3581, #3605).
    is_wayland
        && has_x11_display
        && !desktop_is_wlroots_family(xdg_current_desktop)
        && !desktop_is_kde(xdg_current_desktop)
        && !desktop_is_gnome(xdg_current_desktop)
}

pub fn should_disable_webkit_dmabuf(
    already_set: bool,
    is_wayland: bool,
    nvidia_loaded: bool,
    xdg_current_desktop: &str,
) -> bool {
    if already_set || !is_wayland {
        return false;
    }
    nvidia_loaded
        || desktop_is_wlroots_family(xdg_current_desktop)
        || desktop_is_kde(xdg_current_desktop)
        || desktop_is_gnome(xdg_current_desktop)
}

#[cfg(test)]
mod tests {
    use super::{
        desktop_is_gnome, desktop_is_kde, desktop_is_wlroots_family, should_disable_webkit_dmabuf,
        should_force_x11_backend,
    };

    #[test]
    fn wlroots_family_matches_known_desktops() {
        for desktop in ["niri", "Hyprland", "sway", "niri:wlroots", "river"] {
            assert!(
                desktop_is_wlroots_family(desktop),
                "{desktop} should be treated as wlroots-family",
                desktop = desktop
            );
        }
        for desktop in ["GNOME", "ubuntu:GNOME", "KDE", "XFCE", ""] {
            assert!(
                !desktop_is_wlroots_family(desktop),
                "{desktop} should not be treated as wlroots-family",
                desktop = desktop
            );
        }
    }

    #[test]
    fn kde_matches_known_desktops() {
        for desktop in ["KDE", "plasma", "KDE:plasma", "ubuntu:KDE"] {
            assert!(
                desktop_is_kde(desktop),
                "{desktop} should be recognized as KDE",
                desktop = desktop
            );
        }
        for desktop in ["GNOME", "ubuntu:GNOME", "niri", "sway", "XFCE", ""] {
            assert!(
                !desktop_is_kde(desktop),
                "{desktop} should not be recognized as KDE",
                desktop = desktop
            );
        }
    }

    #[test]
    fn gnome_matches_known_desktops() {
        for desktop in ["GNOME", "ubuntu:GNOME", "GNOME:classic"] {
            assert!(
                desktop_is_gnome(desktop),
                "{desktop} should be recognized as GNOME",
                desktop = desktop
            );
        }
        for desktop in ["KDE", "plasma", "niri", "sway", "XFCE", ""] {
            assert!(
                !desktop_is_gnome(desktop),
                "{desktop} should not be recognized as GNOME",
                desktop = desktop
            );
        }
    }

    #[test]
    fn niri_with_xwayland_display_does_not_force_x11() {
        assert!(!should_force_x11_backend(
            false, false, false, true, true, "niri"
        ));
    }

    #[test]
    fn kde_with_xwayland_display_does_not_force_x11() {
        assert!(!should_force_x11_backend(
            false, false, false, true, true, "KDE"
        ));
        assert!(!should_force_x11_backend(
            false, false, false, true, true, "plasma"
        ));
    }

    #[test]
    fn gnome_with_xwayland_display_does_not_force_x11() {
        assert!(!should_force_x11_backend(
            false,
            false,
            false,
            true,
            true,
            "ubuntu:GNOME"
        ));
        assert!(!should_force_x11_backend(
            false, false, false, true, true, "GNOME"
        ));
    }

    #[test]
    fn legacy_desktop_with_display_still_forces_x11() {
        assert!(should_force_x11_backend(
            false, false, false, true, true, "XFCE"
        ));
    }

    #[test]
    fn existing_gdk_backend_is_never_overridden() {
        assert!(!should_force_x11_backend(
            true, true, false, true, true, "GNOME"
        ));
        assert!(!should_force_x11_backend(
            true, false, false, true, true, "KDE"
        ));
    }

    #[test]
    fn force_wayland_wins_over_gnome_fallback() {
        assert!(!should_force_x11_backend(
            false, false, true, true, true, "GNOME"
        ));
    }

    #[test]
    fn force_x11_wins_even_on_niri_kde_and_gnome() {
        assert!(should_force_x11_backend(
            false, true, false, true, true, "niri"
        ));
        assert!(should_force_x11_backend(
            false, true, false, true, true, "KDE"
        ));
        assert!(should_force_x11_backend(
            false, true, false, true, true, "GNOME"
        ));
    }

    #[test]
    fn webkit_dmabuf_disabled_on_niri_wayland() {
        assert!(should_disable_webkit_dmabuf(false, true, false, "niri"));
    }

    #[test]
    fn webkit_dmabuf_disabled_on_kde_wayland() {
        assert!(should_disable_webkit_dmabuf(false, true, false, "KDE"));
        assert!(should_disable_webkit_dmabuf(false, true, false, "plasma"));
    }

    #[test]
    fn webkit_dmabuf_disabled_on_nvidia_wayland() {
        assert!(should_disable_webkit_dmabuf(false, true, true, "GNOME"));
    }

    #[test]
    fn webkit_dmabuf_disabled_on_gnome_wayland() {
        assert!(should_disable_webkit_dmabuf(false, true, false, "GNOME"));
        assert!(should_disable_webkit_dmabuf(
            false,
            true,
            false,
            "ubuntu:GNOME"
        ));
    }

    #[test]
    fn webkit_dmabuf_respects_user_override() {
        assert!(!should_disable_webkit_dmabuf(true, true, true, "niri"));
        assert!(!should_disable_webkit_dmabuf(true, true, true, "KDE"));
    }

    #[test]
    fn webkit_dmabuf_not_touched_on_x11_session() {
        assert!(!should_disable_webkit_dmabuf(false, false, true, "niri"));
        assert!(!should_disable_webkit_dmabuf(false, false, true, "KDE"));
    }
}
