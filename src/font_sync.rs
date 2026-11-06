//! Event-driven font-family updates from the configured desktop user.
use crate::config::{try_load_font, Config};
use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify};
use std::{
    fs::OpenOptions,
    io::Read,
    os::{
        fd::AsFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::PathBuf,
};

pub struct FontSync {
    uid: u32,
    directory: PathBuf,
    watch: Inotify,
    current: Option<String>,
}

fn valid_family(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '=' | '/' | '\\'))
}

impl FontSync {
    pub fn new(uid: u32) -> std::io::Result<Self> {
        if uid == 0 {
            return Err(std::io::Error::other("Font sync requires a desktop user"));
        }
        let directory = PathBuf::from(format!("/run/omarchy-touchbar-font-{uid}"));
        let watch = Inotify::init(InitFlags::IN_NONBLOCK).map_err(std::io::Error::from)?;
        watch
            .add_watch(
                &directory,
                AddWatchFlags::IN_CLOSE_WRITE | AddWatchFlags::IN_MOVED_TO,
            )
            .map_err(std::io::Error::from)?;
        Ok(Self {
            uid,
            directory,
            watch,
            current: None,
        })
    }

    pub fn fd(&self) -> impl AsFd + '_ {
        &self.watch
    }

    pub fn update(&mut self, cfg: &mut Config, initial: bool) -> bool {
        if !initial && self.watch.read_events().is_err() {
            return false;
        }
        // A user controls only this bounded family string, never root config or
        // a command. Refuse symlinks, devices/FIFOs, other owners and patterns.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(self.directory.join("current-font"));
        let Ok(file) = file else { return false };
        let Ok(metadata) = file.metadata() else {
            return false;
        };
        if !metadata.is_file() || metadata.uid() != self.uid || metadata.len() > 256 {
            return false;
        }
        let mut name = String::new();
        if file.take(257).read_to_string(&mut name).is_err() {
            return false;
        }
        let name = name.trim();
        if !valid_family(name) || self.current.as_deref() == Some(name) {
            return false;
        }
        match try_load_font(name) {
            Ok(font) => {
                cfg.font_face = font;
                self.current = Some(name.to_owned());
                println!("Updated Touch Bar font: {name}");
                true
            }
            Err(error) => {
                eprintln!("Ignoring Touch Bar font update: {error}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_families_but_rejects_patterns_and_unbounded_input() {
        assert!(valid_family("JetBrainsMono Nerd Font"));
        assert!(valid_family("CaskaydiaMono Nerd Font"));
        for name in [
            "",
            "monospace:file=/etc/passwd",
            "name\0",
            "name\n",
            "/tmp/font.ttf",
        ] {
            assert!(!valid_family(name));
        }
        assert!(!valid_family(&"a".repeat(257)));
    }
}
