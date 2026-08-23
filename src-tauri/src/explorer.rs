//! The "Archive with Tree Archiver" verb in the File Explorer context menu.
//!
//! Everything is written under `HKEY_CURRENT_USER\Software\Classes`, which
//! needs no elevation and affects only the user who ticked the box. Two keys
//! are registered: one for files (`*`) and one for folders (`Directory`).
//!
//! Explorer starts one process per selected item, so the running instance
//! coalesces the arrivals — see `commands::stage_external`.
//!
//! On Windows 11 a verb registered this way appears under "Show more options"
//! rather than in the compact menu. Reaching the compact menu requires a
//! packaged `IExplorerCommand` handler, which is a different piece of work.

/// The subkey name written under each class. Not shown to the user.
pub const VERB: &str = "TreeArchiver";

/// The classes the verb is registered for: every file, and every folder.
pub const CLASSES: [&str; 2] = ["*", "Directory"];

/// `Software\Classes\<class>\shell\TreeArchiver`, the key holding the label.
pub fn verb_key(class: &str) -> String {
    format!(r"Software\Classes\{class}\shell\{VERB}")
}

/// The `command` subkey, whose default value is the command line to run.
pub fn command_key(class: &str) -> String {
    format!(r"{}\command", verb_key(class))
}

/// `"<exe>" --add "%1"` — one path per invocation, which is all a plain verb
/// can carry. The quotes matter: paths routinely contain spaces.
pub fn command_line(exe: &str) -> String {
    format!("\"{exe}\" --add \"%1\"")
}

/// `"<exe>",0` — the first icon resource in the executable.
pub fn icon_value(exe: &str) -> String {
    format!("\"{exe}\",0")
}

#[cfg(windows)]
mod imp {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    fn exe_path() -> Result<String, String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("could not locate the application: {e}"))?;
        Ok(crate::fsutil::display_path(&exe))
    }

    /// True when the verb is registered for every class it should cover. A
    /// partial registration counts as absent, so ticking the box repairs it.
    pub fn is_installed() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        CLASSES
            .iter()
            .all(|c| hkcu.open_subkey_with_flags(command_key(c), KEY_READ).is_ok())
    }

    /// `label` is the menu text, passed in already translated.
    pub fn install(label: &str) -> Result<(), String> {
        let exe = exe_path()?;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        for class in CLASSES {
            let (verb, _) = hkcu
                .create_subkey_with_flags(verb_key(class), KEY_WRITE)
                .map_err(|e| format!("could not write the {class} menu entry: {e}"))?;
            verb.set_value("", &label)
                .map_err(|e| format!("could not set the {class} menu label: {e}"))?;
            verb.set_value("Icon", &icon_value(&exe))
                .map_err(|e| format!("could not set the {class} menu icon: {e}"))?;

            let (cmd, _) = hkcu
                .create_subkey_with_flags(command_key(class), KEY_WRITE)
                .map_err(|e| format!("could not write the {class} command: {e}"))?;
            cmd.set_value("", &command_line(&exe))
                .map_err(|e| format!("could not set the {class} command: {e}"))?;
        }
        Ok(())
    }

    /// Removes both keys. A key that is already gone is not an error — the
    /// desired end state is "not registered", and it is.
    pub fn uninstall() -> Result<(), String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        for class in CLASSES {
            match hkcu.delete_subkey_all(verb_key(class)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("could not remove the {class} menu entry: {e}")),
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_installed() -> bool {
        false
    }
    pub fn install(_label: &str) -> Result<(), String> {
        Err("the Explorer menu is a Windows feature".into())
    }
    pub fn uninstall() -> Result<(), String> {
        Err("the Explorer menu is a Windows feature".into())
    }
}

pub use imp::{install, is_installed, uninstall};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_land_under_the_per_user_class_root() {
        assert_eq!(verb_key("*"), r"Software\Classes\*\shell\TreeArchiver");
        assert_eq!(
            command_key("Directory"),
            r"Software\Classes\Directory\shell\TreeArchiver\command"
        );
    }

    #[test]
    fn both_files_and_folders_are_covered() {
        assert!(CLASSES.contains(&"*"));
        assert!(CLASSES.contains(&"Directory"));
    }

    /// A path with spaces is the normal case on Windows, not the exception.
    #[test]
    fn the_command_line_quotes_the_exe_and_the_argument() {
        let cmd = command_line(r"C:\Program Files\Tree Archiver\tree-archiver.exe");
        assert_eq!(
            cmd,
            r#""C:\Program Files\Tree Archiver\tree-archiver.exe" --add "%1""#
        );
    }

    #[test]
    fn the_icon_points_at_the_first_resource_in_the_exe() {
        assert_eq!(icon_value(r"C:\app.exe"), r#""C:\app.exe",0"#);
    }
}
