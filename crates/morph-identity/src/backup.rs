//! Identity backups: a profile's 24 recovery words in a password-sealed file.
//!
//! A backup is an ASCII-armored age file (scrypt passphrase recipient, work
//! factor [`weirdnet_keys::agefile::WORK_FACTOR`]) whose plaintext is the 24
//! words and nothing else, the same content as weirdnet-keys' `words.txt.age`
//! export. Armor keeps it text, so it can be saved, shared or pasted anywhere,
//! and any age tool opens it with the backup password.
//!
//! Restoring accepts such a file (armored or binary), or an unsealed recovery
//! phrase: the bare words, or the phrase file Morph saves (numbered words plus
//! the words on one line).
use std::path::Path;

use weirdnet_keys::words::from_words;
use weirdnet_keys::{ExposeSecret, SecretString, Zeroizing, agefile};

use crate::profile_identity::{self, LibraryKey, ProfileIdentity};
use crate::{Error, Result};

/// Suggested file extension.
pub const EXTENSION: &str = "age";
/// Files larger than this are not backups or phrases.
pub const MAX_LEN: usize = 64 * 1024;
const ARMOR_BEGIN: &[u8] = b"-----BEGIN AGE ENCRYPTED FILE-----";

/// What a file offered for restore holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A sealed backup: needs the backup password.
    Sealed,
    /// An unsealed recovery phrase.
    Phrase,
    /// Neither.
    Unknown,
}

/// What `contents` is, without any password.
pub fn kind(contents: &[u8]) -> Kind {
    if contents.len() > MAX_LEN {
        return Kind::Unknown;
    }
    if is_sealed(contents) {
        return Kind::Sealed;
    }
    match std::str::from_utf8(contents) {
        Ok(text) if phrase_words(text).is_some() => Kind::Phrase,
        _ => Kind::Unknown,
    }
}

fn is_sealed(contents: &[u8]) -> bool {
    let head = contents.trim_ascii_start();
    agefile::is_age(head) || head.starts_with(ARMOR_BEGIN)
}

/// A backup of the profile's identity sealed under `backup_password`, after
/// checking the profile password. The backup password may differ from it.
pub fn export(profile_dir: &Path, password: &SecretString, backup_password: &SecretString) -> Result<Zeroizing<String>> {
    profile_identity::check_password(backup_password)?;
    let words = profile_identity::recovery_words(profile_dir, password)?;
    seal_words(&words, backup_password)
}

fn seal_words(words: &str, backup_password: &SecretString) -> Result<Zeroizing<String>> {
    let mut recipient = age::scrypt::Recipient::new(backup_password.clone());
    recipient.set_work_factor(agefile::WORK_FACTOR);
    let armored = age::encrypt_and_armor(&recipient, words.as_bytes()).map_err(|e| Error::Message(format!("could not seal the backup: {e}")))?;
    Ok(Zeroizing::new(armored))
}

/// The 24 words in a backup or phrase file. A sealed backup needs
/// `backup_password`; a phrase ignores it.
pub fn words(contents: &[u8], backup_password: Option<&SecretString>) -> Result<Zeroizing<String>> {
    match kind(contents) {
        Kind::Sealed => {
            let password = backup_password
                .filter(|p| !p.expose_secret().is_empty())
                .ok_or_else(|| Error::Message("this backup needs its backup password".into()))?;
            // age's armor reader wants the marker at the first byte, and a
            // pasted or edited backup may start with a blank line.
            // weirdnet-keys after 0.1.0 accepts both itself; this and
            // ARMOR_BEGIN can go once Morph pins that release.
            let plain = agefile::decrypt(password, contents.trim_ascii_start())
                .map_err(|_| Error::Message("wrong backup password, or the backup is damaged".into()))?;
            let text = std::str::from_utf8(&plain).map_err(|_| Error::Message("the backup does not hold recovery words".into()))?;
            phrase_words(text).ok_or_else(|| Error::Message("the backup does not hold recovery words".into()))
        }
        Kind::Phrase => {
            let text = std::str::from_utf8(contents).map_err(|_| Error::Message("not a recovery phrase".into()))?;
            phrase_words(text).ok_or_else(|| Error::Message("not a recovery phrase".into()))
        }
        Kind::Unknown => Err(Error::Message("this file is not a Morph backup or recovery phrase".into())),
    }
}

/// Restore the identity in `contents` into this profile, sealed under the
/// profile's new `password`. Returns its library key too.
pub fn restore(
    profile_dir: &Path,
    contents: &[u8],
    backup_password: Option<&SecretString>,
    password: &SecretString,
    now_ms: i64,
) -> Result<(ProfileIdentity, LibraryKey)> {
    profile_identity::check_password(password)?;
    let words = words(contents, backup_password)?;
    profile_identity::import_unlocked(profile_dir, &words, password, now_ms)
}

/// Find 24 valid recovery words in `text`: a line holding all of them, the
/// numbered list (`1. word`), or every word in the text.
fn phrase_words(text: &str) -> Option<Zeroizing<String>> {
    let valid = |candidate: &str| -> Option<Zeroizing<String>> {
        let joined = Zeroizing::new(candidate.split_whitespace().collect::<Vec<_>>().join(" "));
        (joined.split(' ').count() == 24 && from_words(&joined).is_ok()).then_some(joined)
    };
    if let Some(found) = text.lines().find_map(valid) {
        return Some(found);
    }
    let numbered: Vec<&str> = text
        .lines()
        .filter_map(|line| {
            let (number, word) = line.trim().split_once(". ")?;
            number.trim().parse::<u8>().ok()?;
            Some(word.trim())
        })
        .collect();
    valid(&numbered.join(" ")).or_else(|| valid(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile_identity::{Source, create, load, recovery_words};

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    fn created() -> (tempfile::TempDir, Zeroizing<String>) {
        let dir = tempfile::tempdir().unwrap();
        let c = create(dir.path(), &pw("profile-password"), 1).unwrap();
        (dir, c.words)
    }

    #[test]
    fn a_backup_restores_the_same_identity_in_another_profile() {
        let (a, words) = created();
        let backup = export(a.path(), &pw("profile-password"), &pw("backup-password")).unwrap();
        assert!(backup.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
        assert!(!backup.contains(words.split(' ').next().unwrap()), "the words are sealed");
        assert_eq!(kind(backup.as_bytes()), Kind::Sealed);

        let b = tempfile::tempdir().unwrap();
        let (identity, key) = restore(b.path(), backup.as_bytes(), Some(&pw("backup-password")), &pw("new-password"), 2).unwrap();
        let original = load(a.path()).unwrap().unwrap();
        assert_eq!(identity.user_id, original.user_id);
        assert_eq!(identity.source, Source::Imported);
        assert_eq!(*recovery_words(b.path(), &pw("new-password")).unwrap(), *words);
        assert_eq!(key.expose(), profile_identity::library_key(a.path(), &pw("profile-password")).unwrap().expose());
    }

    #[test]
    fn export_checks_both_passwords() {
        let (a, _) = created();
        assert!(export(a.path(), &pw("wrong-password"), &pw("backup-password")).is_err());
        assert!(export(a.path(), &pw("profile-password"), &pw("short")).is_err());
        let empty = tempfile::tempdir().unwrap();
        assert!(export(empty.path(), &pw("profile-password"), &pw("backup-password")).is_err());
    }

    #[test]
    fn a_sealed_backup_needs_the_right_password() {
        let (a, _) = created();
        let backup = export(a.path(), &pw("profile-password"), &pw("backup-password")).unwrap();
        let b = tempfile::tempdir().unwrap();
        for wrong in [None, Some(pw("")), Some(pw("not-the-backup-password"))] {
            let e = restore(b.path(), backup.as_bytes(), wrong.as_ref(), &pw("new-password"), 2).unwrap_err();
            assert!(e.to_string().contains("backup password"), "{e}");
        }
        assert!(load(b.path()).unwrap().is_none(), "nothing written");
        // Damage one character inside the armor body.
        let mut damaged = backup.as_bytes().to_vec();
        let at = damaged.len() / 2;
        damaged[at] = if damaged[at] == b'A' { b'B' } else { b'A' };
        assert!(restore(b.path(), &damaged, Some(&pw("backup-password")), &pw("new-password"), 2).is_err());
        assert!(load(b.path()).unwrap().is_none());
    }

    #[test]
    fn whitespace_around_the_armor_and_crlf_are_fine() {
        let (a, words) = created();
        let backup = export(a.path(), &pw("profile-password"), &pw("backup-password")).unwrap();
        for variant in [format!("\n \n{}\n\n", *backup), backup.replace('\n', "\r\n")] {
            assert_eq!(kind(variant.as_bytes()), Kind::Sealed);
            assert_eq!(*super::words(variant.as_bytes(), Some(&pw("backup-password"))).unwrap(), *words);
        }
    }

    #[test]
    fn binary_age_files_of_words_restore_too() {
        let (_a, words) = created();
        let binary = agefile::encrypt(&pw("backup-password"), words.as_bytes()).unwrap();
        assert_eq!(kind(&binary), Kind::Sealed);
        assert_eq!(*super::words(&binary, Some(&pw("backup-password"))).unwrap(), *words);
    }

    #[test]
    fn unsealed_phrases_restore_without_a_backup_password() {
        let (_a, words) = created();
        let list: Vec<&str> = words.split(' ').collect();
        let numbered: String = list.iter().enumerate().map(|(i, w)| format!("{:>2}. {w}\n", i + 1)).collect();
        let morph_file = format!(
            "Morph recovery phrase\nProfile: Work\nUser: u1\n\nAnyone with these 24 words can restore this identity.\n\n{numbered}\n{}\n",
            list.join(" ")
        );
        let spread = list.chunks(6).map(|c| c.join(" ")).collect::<Vec<_>>().join("\n");
        for text in [words.to_string(), format!("  {}  \n", *words), numbered.clone(), morph_file, spread] {
            assert_eq!(kind(text.as_bytes()), Kind::Phrase, "{text:?}");
            assert_eq!(*super::words(text.as_bytes(), None).unwrap(), *words);
        }
        let b = tempfile::tempdir().unwrap();
        restore(b.path(), numbered.as_bytes(), None, &pw("new-password"), 2).unwrap();
        assert!(load(b.path()).unwrap().is_some());
    }

    #[test]
    fn other_files_are_refused() {
        let (_a, words) = created();
        let mut wrong = words.split(' ').collect::<Vec<_>>();
        wrong[0] = "notaword";
        let short = words.split(' ').take(23).collect::<Vec<_>>().join(" ");
        for text in [b"hello".to_vec(), wrong.join(" ").into_bytes(), short.into_bytes(), vec![0xff, 0xfe, 0x00], vec![b'a'; MAX_LEN + 1]] {
            assert_eq!(kind(&text), Kind::Unknown);
            assert!(super::words(&text, None).is_err());
        }
    }

    #[test]
    fn restore_refuses_a_short_profile_password_and_an_existing_identity() {
        let (a, words) = created();
        let b = tempfile::tempdir().unwrap();
        assert!(restore(b.path(), words.as_bytes(), None, &pw("short"), 2).is_err());
        assert!(restore(a.path(), words.as_bytes(), None, &pw("new-password"), 2).is_err());
    }
}
