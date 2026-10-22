//! Setting keys in an INI file without disturbing the rest of it: other sections, other keys,
//! comments and their order stay as they were. PCSX2 reads its settings this way.

/// `existing` with `section.key = value`; the key is added (and the section) if missing.
pub fn set(existing: &str, section: &str, key: &str, value: &str) -> String {
    set_all(existing, section, &[(key, value)])
}

/// `existing` with each `(key, value)` set in `section`. Keys are matched without regard to
/// spaces around `=`; a new section goes at the end.
pub fn set_all(existing: &str, section: &str, pairs: &[(&str, &str)]) -> String {
    let header = format!("[{section}]");
    let mut out: Vec<String> = Vec::new();
    let mut done = vec![false; pairs.len()];
    let mut in_section = false;
    let mut seen_section = false;

    let flush = |out: &mut Vec<String>, done: &mut Vec<bool>| {
        // Missing keys go after the section's last key, before any blank lines that end it.
        let at = out
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .map_or(out.len(), |i| i + 1);
        let mut add = Vec::new();
        for (i, (k, v)) in pairs.iter().enumerate() {
            if !done[i] {
                add.push(format!("{k} = {v}"));
                done[i] = true;
            }
        }
        out.splice(at..at, add);
    };

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_section {
                flush(&mut out, &mut done);
            }
            in_section = trimmed.eq_ignore_ascii_case(&header);
            seen_section |= in_section;
            out.push(line.to_string());
            continue;
        }
        if in_section {
            if let Some((k, _)) = trimmed.split_once('=') {
                if let Some(i) = pairs
                    .iter()
                    .position(|(key, _)| key.eq_ignore_ascii_case(k.trim()))
                {
                    out.push(format!("{} = {}", pairs[i].0, pairs[i].1));
                    done[i] = true;
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if in_section {
        flush(&mut out, &mut done);
    }
    if !seen_section {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(header);
        for (k, v) in pairs {
            out.push(format!("{k} = {v}"));
        }
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// `section` with exactly these keys: any other keys in it are dropped (a pad's old bindings).
pub fn replace_section(existing: &str, section: &str, pairs: &[(&str, &str)]) -> String {
    let header = format!("[{section}]");
    let mut out: Vec<String> = Vec::new();
    let mut skipping = false;
    let mut written = false;
    let body = |out: &mut Vec<String>| {
        for (k, v) in pairs {
            out.push(format!("{k} = {v}"));
        }
    };
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if skipping {
                out.push(String::new());
            }
            skipping = trimmed.eq_ignore_ascii_case(&header);
            out.push(line.to_string());
            if skipping {
                body(&mut out);
                written = true;
            }
            continue;
        }
        if !skipping {
            out.push(line.to_string());
        }
    }
    if !written {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(header);
        body(&mut out);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// The value of `section.key`, if present.
pub fn get(text: &str, section: &str, key: &str) -> Option<String> {
    let header = format!("[{section}]");
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed.eq_ignore_ascii_case(&header);
        } else if in_section {
            if let Some((k, v)) = trimmed.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "; PCSX2 settings\n[UI]\nTheme = Dark\nSetupWizardIncomplete = true\n\n[EmuCore]\nEnableCheats = false\n";

    #[test]
    fn a_key_is_replaced_in_place_and_nothing_else_moves() {
        let out = set(FILE, "UI", "SetupWizardIncomplete", "false");
        assert_eq!(
            out,
            "; PCSX2 settings\n[UI]\nTheme = Dark\nSetupWizardIncomplete = false\n\n[EmuCore]\nEnableCheats = false\n"
        );
    }

    #[test]
    fn a_missing_key_joins_its_section_and_a_missing_section_goes_last() {
        let out = set(FILE, "UI", "StartFullscreen", "true");
        assert!(
            out.contains("SetupWizardIncomplete = true\nStartFullscreen = true\n\n[EmuCore]"),
            "{out}"
        );
        let out = set(FILE, "Folders", "Bios", "/b");
        assert!(
            out.ends_with("EnableCheats = false\n\n[Folders]\nBios = /b\n"),
            "{out}"
        );
        assert_eq!(
            set("", "Filenames", "BIOS", "a.bin"),
            "[Filenames]\nBIOS = a.bin\n"
        );
    }

    #[test]
    fn keys_match_whatever_the_spacing_and_case() {
        let out = set(
            "[ui]\nsetupwizardincomplete=true\n",
            "UI",
            "SetupWizardIncomplete",
            "false",
        );
        assert_eq!(out, "[ui]\nSetupWizardIncomplete = false\n");
    }

    #[test]
    fn a_whole_section_is_replaced_and_its_old_keys_dropped() {
        let pad = "[Pad1]\nType = DualShock2\nCross = Keyboard/Z\nOld = x\n\n[Hotkeys]\nToggleTurbo = Keyboard/Tab\n";
        let out = replace_section(
            pad,
            "Pad1",
            &[("Type", "DualShock2"), ("Cross", "SDL-0/FaceSouth")],
        );
        assert_eq!(
            out,
            "[Pad1]\nType = DualShock2\nCross = SDL-0/FaceSouth\n\n[Hotkeys]\nToggleTurbo = Keyboard/Tab\n"
        );
        assert!(replace_section("", "Pad1", &[("Type", "DualShock2")])
            .starts_with("[Pad1]\nType = DualShock2"));
    }

    #[test]
    fn get_reads_a_value() {
        assert_eq!(get(FILE, "ui", "theme").as_deref(), Some("Dark"));
        assert_eq!(get(FILE, "EmuCore", "Missing"), None);
    }
}
