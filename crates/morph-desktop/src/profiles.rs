//! Profile management behind the profiles panel, free of GPUI so it is
//! tested here: list, create, rename, delete and switch.

use std::path::{Path, PathBuf};

use morph_core::profiles::{self, Profile};

/// One row of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub profile: Profile,
    pub current: bool,
    /// The profile asks for its password before it opens: it has an
    /// identity, the password is on, and it is not open in this process.
    pub locked: bool,
}

pub struct Profiles {
    home: PathBuf,
    /// The profile this window shows (the catalog's current one may move
    /// while editing; the window switches only on request).
    shown: String,
    pub error: Option<String>,
}

impl Profiles {
    pub fn new(home: &Path, shown: &str) -> Self {
        Self { home: home.to_path_buf(), shown: shown.to_string(), error: None }
    }

    #[cfg(test)]
    pub fn shown(&self) -> &str {
        &self.shown
    }

    pub fn shown_name(&self) -> String {
        self.entries().into_iter().find(|e| e.current).map(|e| e.profile.name).unwrap_or_default()
    }

    pub fn entries(&self) -> Vec<Entry> {
        profiles::list(&self.home)
            .unwrap_or_default()
            .into_iter()
            .map(|profile| Entry { current: profile.id == self.shown, locked: shows_locked(&self.home, &profile.id), profile })
            .collect()
    }

    fn result<T>(&mut self, r: Result<T, impl std::fmt::Display>) -> Option<T> {
        match r {
            Ok(v) => {
                self.error = None;
                Some(v)
            }
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        }
    }

    /// A new profile. Returns its id; the window switches to it separately.
    pub fn create(&mut self, name: &str) -> Option<String> {
        let created = profiles::create(&self.home, name);
        let id = self.result(created)?.id;
        // `create` makes it current in the catalog; the window decides.
        let _ = profiles::set_current(&self.home, &self.shown);
        Some(id)
    }

    pub fn rename(&mut self, id: &str, name: &str) {
        let r = profiles::rename(&self.home, id, name);
        self.result(r);
    }

    /// Delete a profile other than the one shown.
    pub fn delete(&mut self, id: &str) {
        if id == self.shown {
            self.error = Some("Switch to another profile before deleting this one.".into());
            return;
        }
        let r = morph_services::profiles::delete(&self.home, id);
        self.result(r);
    }

    /// Make `id` current; the caller rebuilds the window's components.
    pub fn switch(&mut self, id: &str) -> bool {
        let r = profiles::set_current(&self.home, id);
        if self.result(r).is_some() {
            self.shown = id.to_string();
            true
        } else {
            false
        }
    }
}

/// Whether the picker shows `id` as locked: opening it would ask for its
/// password. A profile without an identity, one whose password is off (it
/// opens from the keystore) and one already open here are not.
pub fn shows_locked(home: &Path, id: &str) -> bool {
    morph_core::profile_lock::has_identity(home, id)
        && morph_core::profile_lock::requires_password(home, id)
        && morph_services::profile_lock::is_locked(home, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use morph_identity::profile_identity::SecretString;

    #[test]
    fn locked_profiles_show_as_locked() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        let mut p = Profiles::new(&home, profiles::DEFAULT_PROFILE_ID);
        let work = p.create("Work").unwrap();
        let locked = |p: &Profiles, id: &str| p.entries().into_iter().find(|e| e.profile.id == id).unwrap().locked;
        assert!(!locked(&p, &work), "no identity: never locked");
        let password = SecretString::from("profile password".to_string());
        morph_services::identity::create(&home, &work, &password).unwrap();
        assert!(!locked(&p, &work), "open right after onboarding");
        morph_services::profile_lock::lock(&home, &work);
        assert!(locked(&p, &work), "locked after a lock");
        assert!(!locked(&p, profiles::DEFAULT_PROFILE_ID));
        morph_services::library::unlock(&home, &work, &password).unwrap();
        assert!(!locked(&p, &work), "open again after the unlock");
        morph_services::profile_lock::set_require_password(&home, &work, &password, false).unwrap();
        morph_services::profile_lock::lock(&home, &work);
        assert!(!locked(&p, &work), "password off: opens without a prompt");
    }

    #[test]
    fn create_rename_switch_and_delete() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("h");
        profiles::load_catalog(&home).unwrap();
        let mut p = Profiles::new(&home, profiles::DEFAULT_PROFILE_ID);
        assert_eq!(p.entries().len(), 1);
        assert!(p.entries()[0].current);

        let work = p.create("Work").unwrap();
        assert_eq!(p.entries().len(), 2);
        assert_eq!(profiles::current(&home).unwrap().id, profiles::DEFAULT_PROFILE_ID, "creating does not switch");
        assert!(p.create("   ").is_none());
        assert!(p.error.is_some());

        p.rename(&work, "Job");
        assert!(p.entries().iter().any(|e| e.profile.name == "Job"));

        assert!(p.switch(&work));
        assert_eq!(p.shown(), work);
        assert_eq!(p.shown_name(), "Job");
        assert_eq!(profiles::current(&home).unwrap().id, work);

        p.delete(&work);
        assert_eq!(p.error.as_deref(), Some("Switch to another profile before deleting this one."));
        assert!(p.switch(profiles::DEFAULT_PROFILE_ID));
        p.delete(&work);
        assert!(p.error.is_none());
        assert_eq!(p.entries().len(), 1);
        assert!(!p.switch("nope"));
    }
}
