//! Romshelf's Steam Input layout for its shortcuts: Steam's own gamepad template with the four
//! back grips sending F9 to F12, which the emulator configs Romshelf writes turn into hotkeys.
//!
//! Steam keeps a shortcut's layout in `Steam Controller Configs/<id>/config/<key>/
//! controller_neptune.vdf`, named in `configset_controller_neptune.vdf` with `"autosave" "1"`.
//! Found on the Deck (2026-10-19): `<key>` is the shortcut's name in lower case without dots
//! ("romshelf.AppImage kirb" became "romshelfappimage kirb").

/// Steam's grip names and the keys Romshelf gives them: L4 and R4 are the upper grips.
pub const GRIP_KEYS: &[(&str, &str)] = &[
    ("button_back_left_upper", "F9"),
    ("button_back_right_upper", "F10"),
    ("button_back_left", "F11"),
    ("button_back_right", "F12"),
];

/// The folder name Steam gives a shortcut's layout.
pub fn config_key(app_name: &str) -> String {
    app_name
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '.' | '/' | '\\'))
        .collect()
}

/// Steam's template with the grips bound and the title "Romshelf". Line endings as the
/// template has them.
pub fn layout_from_template(template: &str) -> String {
    let crlf = template.contains("\r\n");
    let nl = if crlf { "\r\n" } else { "\n" };
    let mut out = String::with_capacity(template.len() + 400);
    let mut grip: Option<&str> = None;
    let mut retitled = false;
    let mut lines = template.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let bare = line.trim();
        if !retitled && bare.starts_with("\"Title\"") {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push_str(&format!("{indent}\"Title\" \"Romshelf\"{nl}"));
            retitled = true;
            continue;
        }
        if let Some((_, key)) = GRIP_KEYS
            .iter()
            .find(|(name, _)| bare == format!("\"{name}\""))
        {
            grip = Some(key);
        }
        out.push_str(line);
        if bare == "\"bindings\"" {
            if let Some(key) = grip.take() {
                // The opening brace, then Romshelf's binding inside it.
                if let Some(open) = lines.next() {
                    out.push_str(open);
                    let indent = &open[..open.len() - open.trim_start().len()];
                    out.push_str(&format!("{indent}\t\"binding\"\t\t\"key_press {key}\"{nl}"));
                }
            }
        }
    }
    out
}

/// The layout list with Romshelf's entry for `key` present (`autosave`) or gone. Other
/// entries are kept as they are.
pub fn set_entry(configset: Option<&str>, key: &str, present: bool) -> String {
    let text = configset.unwrap_or("\"controller_config\"\n{\n}\n");
    let mut out = String::new();
    let mut skip_depth: Option<i32> = None;
    let mut depth = 0;
    let target = format!("\"{key}\"");
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let bare = line.trim();
        if skip_depth.is_none() && depth == 1 && bare == target {
            skip_depth = Some(depth);
            continue;
        }
        let opens = bare == "{";
        let closes = bare == "}";
        if let Some(d) = skip_depth {
            if opens {
                depth += 1;
            } else if closes {
                depth -= 1;
                if depth == d {
                    skip_depth = None;
                }
            }
            continue;
        }
        if closes && depth == 1 && lines.peek().is_none_or(|l| l.trim().is_empty()) && present {
            out.push_str(&format!(
                "\t{target}\n\t{{\n\t\t\"autosave\"\t\t\"1\"\n\t}}\n"
            ));
        }
        if opens {
            depth += 1;
        } else if closes {
            depth -= 1;
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = "\u{feff}\"controller_mappings\"\r\n{\r\n\t\"localization\"\r\n\t{\r\n\t\t\"english\"\r\n\t\t{\r\n\t\t\t\"Title\" \"Gamepad with Mouse Trackpad\"\r\n\t\t}\r\n\t}\r\n\t\"group\"\r\n\t{\r\n\t\t\"inputs\"\r\n\t\t{\r\n\t\t\t\"button_back_left\"\r\n\t\t\t{\r\n\t\t\t\t\"activators\"\r\n\t\t\t\t{\r\n\t\t\t\t\t\"Full_Press\"\r\n\t\t\t\t\t{\r\n\t\t\t\t\t\t\"bindings\"\r\n\t\t\t\t\t\t{\r\n\t\t\t\t\t\t}\r\n\t\t\t\t\t}\r\n\t\t\t\t}\r\n\t\t\t}\r\n\t\t\t\"button_back_left_upper\"\r\n\t\t\t{\r\n\t\t\t\t\"activators\"\r\n\t\t\t\t{\r\n\t\t\t\t\t\"Full_Press\"\r\n\t\t\t\t\t{\r\n\t\t\t\t\t\t\"bindings\"\r\n\t\t\t\t\t\t{\r\n\t\t\t\t\t\t}\r\n\t\t\t\t\t}\r\n\t\t\t\t}\r\n\t\t\t}\r\n\t\t}\r\n\t}\r\n}\r\n";

    #[test]
    fn the_grips_send_keys_and_the_layout_is_named_romshelf() {
        let out = layout_from_template(TEMPLATE);
        assert!(out.contains("\t\t\t\"Title\" \"Romshelf\"\r\n"));
        assert!(out.contains(
            "\"button_back_left\"\r\n\t\t\t{\r\n\t\t\t\t\"activators\"\r\n\t\t\t\t{\r\n\t\t\t\t\t\"Full_Press\"\r\n\t\t\t\t\t{\r\n\t\t\t\t\t\t\"bindings\"\r\n\t\t\t\t\t\t{\r\n\t\t\t\t\t\t\t\"binding\"\t\t\"key_press F11\"\r\n"
        ));
        assert!(out.contains("\"key_press F9\""));
        assert_eq!(out.matches("key_press").count(), 2);
    }

    #[test]
    fn keys_are_the_name_in_lower_case_without_dots() {
        assert_eq!(
            config_key("romshelf.AppImage kirb"),
            "romshelfappimage kirb"
        );
        assert_eq!(
            config_key("Kirby's Return to Dream Land"),
            "kirby's return to dream land"
        );
        assert_eq!(config_key("A/B"), "ab");
    }

    #[test]
    fn entries_are_added_once_and_removed_cleanly() {
        let base =
            "\"controller_config\"\n{\n\t\"2444750\"\n\t{\n\t\t\"autosave\"\t\t\"1\"\n\t}\n}\n";
        let added = set_entry(Some(base), "kirby", true);
        assert_eq!(
            added,
            "\"controller_config\"\n{\n\t\"2444750\"\n\t{\n\t\t\"autosave\"\t\t\"1\"\n\t}\n\t\"kirby\"\n\t{\n\t\t\"autosave\"\t\t\"1\"\n\t}\n}\n"
        );
        assert_eq!(
            set_entry(Some(&added), "kirby", true),
            added,
            "no second copy"
        );
        assert_eq!(set_entry(Some(&added), "kirby", false), base);
        assert_eq!(
            set_entry(None, "kirby", true),
            "\"controller_config\"\n{\n\t\"kirby\"\n\t{\n\t\t\"autosave\"\t\t\"1\"\n\t}\n}\n"
        );
    }
}
