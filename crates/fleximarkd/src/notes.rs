use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use fleximark_model::{Block, BlockKind, Document, Node};
use fleximark_plugin_sdk::NoteCategoryConfig;
use fleximark_protocol::{CommandMessage, CommandResult, GetNoteOptionsResult, NoteCategoryOption};

use crate::workspace::{
    open_control_directory, reject_link, safe_workspace_relative_path, validate_config,
};
use crate::{ServiceError, path_to_file_uri, workspace_path};

pub fn get_note_options(workspace_uri: &str) -> Result<GetNoteOptionsResult, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    let config = validate_config(&root.join(".fleximark/config.toml"))?;
    Ok(GetNoteOptionsResult {
        categories: config
            .notes
            .categories
            .iter()
            .map(|(name, category)| note_category_option(name, category))
            .collect(),
        templates: config.notes.templates.keys().cloned().collect(),
    })
}

fn note_category_option(name: &str, category: &NoteCategoryConfig) -> NoteCategoryOption {
    NoteCategoryOption {
        name: name.to_owned(),
        children: category
            .0
            .iter()
            .map(|(name, child)| note_category_option(name, child))
            .collect(),
    }
}

pub fn create_note(workspace_uri: &str) -> Result<CommandResult, ServiceError> {
    create_note_with_options(workspace_uri, None, None)
}

pub fn create_note_with_options(
    workspace_uri: &str,
    category_path: Option<&[String]>,
    template: Option<&str>,
) -> Result<CommandResult, ServiceError> {
    let root = workspace_path(workspace_uri)?;
    open_control_directory(&root, false)?.ok_or(ServiceError::NotInitialized)?;
    let config = root.join(".fleximark/config.toml");
    if !config.is_file() {
        return Err(ServiceError::NotInitialized);
    }
    let config = validate_config(&config)?;
    let mut notes = root.join("notes");
    reject_link(&notes)?;
    fs::create_dir_all(&notes)?;
    if let Some(category_path) = category_path {
        if category_path.is_empty()
            || !category_path_exists(&config.notes.categories, category_path)
        {
            return Err(ServiceError::InvalidConfig);
        }
        for directory in category_path {
            if !safe_workspace_relative_path(directory) {
                return Err(ServiceError::InvalidConfig);
            }
            notes.push(directory);
            reject_link(&notes)?;
            fs::create_dir(&notes).or_else(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(error)
                }
            })?;
        }
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let prefix = expand_note_text(&config.notes.file_name_prefix);
    let suffix = expand_note_text(&config.notes.file_name_suffix);
    if !safe_filename_fragment(&prefix) || !safe_filename_fragment(&suffix) {
        return Err(ServiceError::InvalidConfig);
    }
    let note = notes.join(format!("{prefix}note-{timestamp}{suffix}.md"));
    let template = template
        .or_else(|| {
            config
                .notes
                .templates
                .contains_key("default")
                .then_some("default")
        })
        .map(|name| {
            config
                .notes
                .templates
                .get(name)
                .ok_or(ServiceError::InvalidConfig)
        })
        .transpose()?;
    let contents = template
        .map(|lines| expand_note_text(&lines.join("\n")))
        .unwrap_or_else(|| "# New note".into());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&note)?;
    file.write_all(contents.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: fleximark_protocol::CommandMessageLevel::Info,
            text: "Created a new note".into(),
        }),
        open_uri: Some(path_to_file_uri(&note)?),
        data: None,
    })
}

fn category_path_exists(
    categories: &std::collections::BTreeMap<String, NoteCategoryConfig>,
    path: &[String],
) -> bool {
    let mut categories = categories;
    for name in path {
        let Some(category) = categories.get(name) else {
            return false;
        };
        categories = &category.0;
    }
    true
}

fn expand_note_text(text: &str) -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86_400;
    let (year, month, day) = civil_date(days as i64);
    let dated = text
        .replace("${CURRENT_YEAR}", &format!("{year:04}"))
        .replace("${CURRENT_MONTH}", &format!("{month:02}"))
        .replace("${CURRENT_DATE}", &format!("{day:02}"));
    let mut output = String::new();
    let mut rest = dated.as_str();
    while let Some(index) = rest.find('$') {
        output.push_str(&rest[..index]);
        rest = &rest[index..];
        if let Some(after) = rest.strip_prefix("${") {
            if let Some(end) = after.find('}') {
                let placeholder = &after[..end];
                if placeholder
                    .split_once(':')
                    .is_some_and(|(number, _)| number.bytes().all(|byte| byte.is_ascii_digit()))
                {
                    output.push_str(placeholder.split_once(':').unwrap().1);
                    rest = &after[end + 1..];
                    continue;
                }
                if placeholder.bytes().all(|byte| byte.is_ascii_digit()) {
                    rest = &after[end + 1..];
                    continue;
                }
            }
        } else {
            let digits = rest[1..]
                .bytes()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if digits > 0 {
                rest = &rest[digits + 1..];
                continue;
            }
        }
        output.push('$');
        rest = &rest[1..];
    }
    output.push_str(rest);
    output
}

fn civil_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_piece = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_piece + 2) / 5 + 1;
    let month = month_piece + if month_piece < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn safe_filename_fragment(value: &str) -> bool {
    !value.contains(['/', '\\', ':', '\0']) && value != "." && value != ".."
}

pub fn collect_admonitions(
    document: &Document,
    source: &str,
    workspace_uri: &str,
) -> Result<CommandResult, ServiceError> {
    fn collect<'a>(blocks: &[Block], source: &'a str, output: &mut Vec<&'a str>) {
        for block in blocks {
            if matches!(block.kind, BlockKind::Admonition { .. }) {
                if let Some(range) = block.provenance.primary_range() {
                    if let (Ok(start), Ok(end)) = (
                        usize::try_from(range.byte_start),
                        usize::try_from(range.byte_end),
                    ) {
                        if let Some(fragment) = source.get(start..end) {
                            output.push(fragment);
                        }
                    }
                }
            }
            for child in &block.children {
                if let Node::Block(child) = child {
                    collect(std::slice::from_ref(child), source, output);
                }
            }
        }
    }

    let root = workspace_path(workspace_uri)?;
    open_control_directory(&root, false)?.ok_or(ServiceError::NotInitialized)?;
    validate_config(&root.join(".fleximark/config.toml"))?;
    let mut fragments = Vec::new();
    collect(&document.blocks, source, &mut fragments);
    let notes = root.join("notes");
    reject_link(&notes)?;
    fs::create_dir_all(&notes)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let output = notes.join(format!("admonitions-{timestamp}.md"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)?;
    file.write_all(b"# Collected admonitions\n\n")?;
    file.write_all(fragments.join("\n\n").as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(CommandResult {
        message: Some(CommandMessage {
            level: fleximark_protocol::CommandMessageLevel::Info,
            text: format!("Collected {} admonition(s)", fragments.len()),
        }),
        open_uri: Some(path_to_file_uri(&output)?),
        data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initialize_workspace;
    use crate::test_support::test_workspace;
    #[test]
    fn create_note_applies_category_template_filename_date_and_snippets() {
        let root = test_workspace("note-options-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            r##"schema_version = 2

[notes]
file_name_prefix = "${CURRENT_YEAR}-"
file_name_suffix = "-draft"

[notes.categories]
Work = { Reports = {} }

[notes.templates]
daily = ["# ${1:Title}", "Created ${CURRENT_YEAR}-${CURRENT_MONTH}-${CURRENT_DATE}", "$0"]
"##,
        )
        .unwrap();
        let options = get_note_options(&uri).unwrap();
        assert_eq!(options.categories.len(), 1);
        assert_eq!(options.categories[0].name, "Work");
        assert_eq!(options.categories[0].children[0].name, "Reports");
        assert_eq!(options.templates, ["daily"]);
        let category = ["Work".to_owned(), "Reports".to_owned()];
        let result = create_note_with_options(&uri, Some(&category), Some("daily")).unwrap();
        let note = workspace_path(result.open_uri.as_deref().unwrap()).unwrap();
        assert_eq!(
            note.parent().unwrap(),
            root.join("notes/Work/Reports").canonicalize().unwrap()
        );
        let name = note.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with("-draft.md"));
        let contents = fs::read_to_string(note).unwrap();
        assert!(contents.starts_with("# Title\nCreated "));
        assert!(!contents.contains("${") && !contents.contains("$0"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parent_categories_are_destinations_and_paths_disambiguate_equal_names() {
        let root = test_workspace("note-category-tree-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            r#"schema_version = 2
[notes.categories]
Left = { Same = {} }
Right = { Same = {} }
"#,
        )
        .unwrap();

        let parent_path = ["Left".to_owned()];
        let parent = create_note_with_options(&uri, Some(&parent_path), None).unwrap();
        let parent = workspace_path(parent.open_uri.as_deref().unwrap()).unwrap();
        assert_eq!(
            parent.parent().unwrap(),
            root.join("notes/Left").canonicalize().unwrap()
        );

        let left_child_path = ["Left".to_owned(), "Same".to_owned()];
        let child = create_note_with_options(&uri, Some(&left_child_path), None).unwrap();
        let child = workspace_path(child.open_uri.as_deref().unwrap()).unwrap();
        assert_eq!(
            child.parent().unwrap(),
            root.join("notes/Left/Same").canonicalize().unwrap()
        );

        let right_child_path = ["Right".to_owned(), "Same".to_owned()];
        let right = create_note_with_options(&uri, Some(&right_child_path), None).unwrap();
        let right = workspace_path(right.open_uri.as_deref().unwrap()).unwrap();
        assert_eq!(
            right.parent().unwrap(),
            root.join("notes/Right/Same").canonicalize().unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn collect_admonitions_writes_only_typed_admonitions() {
        let root = test_workspace("collect-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        let source = "# Keep out\n\n:::info\nkeep in\n:::\n\nTrailing\n";
        let session = fleximark_engine::DocumentSession::open(
            fleximark_model::DocumentUri(format!("{uri}/source.md")),
            1,
            source.to_owned(),
            fleximark_model::PositionEncoding::Utf8,
        )
        .unwrap();
        let result = collect_admonitions(session.document(), source, &uri).unwrap();
        assert_eq!(
            result.message.as_ref().unwrap().text,
            "Collected 1 admonition(s)"
        );
        let output =
            fs::read_to_string(workspace_path(result.open_uri.as_deref().unwrap()).unwrap())
                .unwrap();
        assert!(output.contains(":::info\nkeep in\n:::"));
        assert!(!output.contains("# Keep out") && !output.contains("Trailing"));
        fs::remove_dir_all(root).unwrap();
    }
}
