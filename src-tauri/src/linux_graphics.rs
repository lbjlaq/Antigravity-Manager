//! Linux GDK / WebKit startup policy.
//!
//! niri / Hyprland / sway always expose an Xwayland `DISPLAY`. The old
//! "Wayland + DISPLAY ⇒ force GDK_BACKEND=x11" rule then sends WebKitGTK
//! down a path that draws a black window (issue #3388).
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
    // Historical GTK Wayland shm workaround for GNOME/KDE. Skip compositors
    // that always expose Xwayland — forcing X11 there makes WebKit go black.
    is_wayland && has_x11_display && !desktop_is_wlroots_family(xdg_current_desktop)
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
    nvidia_loaded || desktop_is_wlroots_family(xdg_current_desktop)
}

#[cfg(test)]
mod tests {
    use super::{
        desktop_is_wlroots_family, should_disable_webkit_dmabuf, should_force_x11_backend,
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
                "{desktop} must keep the historical GNOME/KDE X11 fallback",
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
    fn gnome_wayland_with_display_still_forces_x11() {
        assert!(should_force_x11_backend(
            false,
            false,
            false,
            true,
            true,
            "ubuntu:GNOME"
        ));
    }

    #[test]
    fn existing_gdk_backend_is_never_overridden() {
        assert!(!should_force_x11_backend(
            true, true, false, true, true, "GNOME"
        ));
    }

    #[test]
    fn force_wayland_wins_over_gnome_fallback() {
        assert!(!should_force_x11_backend(
            false, false, true, true, true, "GNOME"
        ));
    }

    #[test]
    fn force_x11_wins_even_on_niri() {
        assert!(should_force_x11_backend(
            false, true, false, true, true, "niri"
        ));
    }

    #[test]
    fn webkit_dmabuf_disabled_on_niri_wayland() {
        assert!(should_disable_webkit_dmabuf(false, true, false, "niri"));
    }

    #[test]
    fn webkit_dmabuf_disabled_on_nvidia_wayland() {
        assert!(should_disable_webkit_dmabuf(false, true, true, "GNOME"));
    }

    #[test]
    fn webkit_dmabuf_left_alone_on_gnome_amd() {
        assert!(!should_disable_webkit_dmabuf(false, true, false, "GNOME"));
    }

    #[test]
    fn webkit_dmabuf_respects_user_override() {
        assert!(!should_disable_webkit_dmabuf(true, true, true, "niri"));
    }

    #[test]
    fn webkit_dmabuf_not_touched_on_x11_session() {
        assert!(!should_disable_webkit_dmabuf(false, false, true, "niri"));
    }
}
