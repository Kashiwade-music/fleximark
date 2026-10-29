//! Role: Native workspace notes operations and validation.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::native::paths::safe_workspace_relative_path;
#[cfg(test)]
use crate::native::workspace::validate_config;
use crate::outcomes::{CollectedAdmonitions, CreatedNote, NoteCategory, NoteOptions};
use fleximark_config::{NoteCategoryConfig, is_safe_note_category_name};
use fleximark_model::{Block, BlockKind, Document, Node};

use crate::native::workspace::{open_control_directory, reject_link};
use crate::native::{ApplicationError, path_to_file_uri, workspace_path};

pub fn get_note_options(config: &fleximark_config::FlexiMarkConfig) -> NoteOptions {
    NoteOptions {
        categories: config
            .notes
            .categories
            .iter()
            .map(|(name, category)| note_category_option(name, category))
            .collect(),
        templates: config.notes.templates.keys().cloned().collect(),
    }
}

fn note_category_option(name: &str, category: &NoteCategoryConfig) -> NoteCategory {
    NoteCategory {
        name: name.to_owned(),
        children: category
            .0
            .iter()
            .map(|(name, child)| note_category_option(name, child))
            .collect(),
    }
}

#[cfg(test)]
pub fn create_note(workspace_uri: &str) -> Result<CreatedNote, ApplicationError> {
    let config = validate_config(&workspace_path(workspace_uri)?.join(".fleximark/config.toml"))?;
    create_note_with_options(workspace_uri, &config, None, None, None)
}

pub fn create_note_with_options(
    workspace_uri: &str,
    config: &fleximark_config::FlexiMarkConfig,
    category_path: Option<&[String]>,
    template: Option<&str>,
    file_name: Option<&str>,
) -> Result<CreatedNote, ApplicationError> {
    let root = workspace_path(workspace_uri)?;
    open_control_directory(&root, false)?.ok_or(ApplicationError::NotInitialized)?;
    let config_path = root.join(".fleximark/config.toml");
    if !config_path.is_file() {
        return Err(ApplicationError::NotInitialized);
    }
    let mut notes = create_note_root(&root, &config.notes.root)?;
    if let Some(category_path) = category_path {
        if category_path.is_empty()
            || !category_path_exists(&config.notes.categories, category_path)
        {
            return Err(ApplicationError::InvalidConfig);
        }
        for directory in category_path {
            if !safe_workspace_relative_path(directory) {
                return Err(ApplicationError::InvalidConfig);
            }
            create_directory(&mut notes, directory)?;
        }
    }
    let prefix = expand_note_text(&config.notes.file_name_prefix, None);
    let suffix = expand_note_text(&config.notes.file_name_suffix, None);
    let generated_name;
    let file_name = match file_name {
        Some(file_name) if is_safe_note_category_name(file_name) => file_name,
        Some(_) => return Err(ApplicationError::InvalidNoteName),
        None => {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            generated_name = format!("note-{timestamp}");
            &generated_name
        }
    };
    let stem = format!("{prefix}{file_name}{suffix}");
    if !is_safe_note_category_name(&stem) {
        return Err(ApplicationError::InvalidNoteName);
    }
    let note = notes.join(format!("{stem}.md"));
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
                .ok_or(ApplicationError::InvalidConfig)
        })
        .transpose()?;
    let contents = template
        .map(|lines| expand_note_text(&lines.join("\n"), Some(file_name)))
        .unwrap_or_else(|| "# New note".into());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&note)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ApplicationError::NoteAlreadyExists
            } else {
                error.into()
            }
        })?;
    file.write_all(contents.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(CreatedNote {
        uri: path_to_file_uri(&note)?,
    })
}

fn create_note_root(workspace: &Path, configured_root: &str) -> Result<PathBuf, ApplicationError> {
    let mut root = workspace.to_path_buf();
    if configured_root == "." {
        return Ok(root);
    }
    for directory in configured_root.split('/') {
        create_directory(&mut root, directory)?;
    }
    Ok(root)
}

fn create_directory(path: &mut PathBuf, directory: &str) -> Result<(), ApplicationError> {
    path.push(directory);
    reject_link(path)?;
    fs::create_dir(path).or_else(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            Ok(())
        } else {
            Err(error)
        }
    })?;
    Ok(())
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

fn expand_note_text(text: &str, file_name: Option<&str>) -> String {
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
                if let ("FILE_NAME", Some(file_name)) = (placeholder, file_name) {
                    output.push_str(file_name);
                    rest = &after[end + 1..];
                    continue;
                }
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

pub fn collect_admonitions(
    document: &Document,
    source: &str,
    workspace_uri: &str,
    config: &fleximark_config::FlexiMarkConfig,
) -> Result<CollectedAdmonitions, ApplicationError> {
    fn collect<'a>(blocks: &[Block], source: &'a str, output: &mut Vec<&'a str>) {
        for block in blocks {
            if matches!(block.kind, BlockKind::Admonition { .. }) {
                {
                    let range = &block.range;
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
    open_control_directory(&root, false)?.ok_or(ApplicationError::NotInitialized)?;
    let mut fragments = Vec::new();
    collect(&document.blocks, source, &mut fragments);
    let notes = create_note_root(&root, &config.notes.root)?;
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
    Ok(CollectedAdmonitions {
        uri: path_to_file_uri(&output)?,
        count: fragments.len(),
    })
}

#[cfg(test)]
mod tests {
    //! Role: Verify cached native policies preserve filesystem safety.
    fn collect_admonitions(
        document: &fleximark_model::Document,
        source: &str,
        uri: &str,
    ) -> Result<crate::CollectedAdmonitions, crate::ApplicationError> {
        let config =
            super::validate_config(&super::workspace_path(uri)?.join(".fleximark/config.toml"))?;
        super::collect_admonitions(document, source, uri, &config)
    }
    fn create_note_with_options(
        uri: &str,
        category: Option<&[String]>,
        template: Option<&str>,
        name: Option<&str>,
    ) -> Result<crate::outcomes::CreatedNote, crate::ApplicationError> {
        let config =
            super::validate_config(&super::workspace_path(uri)?.join(".fleximark/config.toml"))?;
        super::create_note_with_options(uri, &config, category, template, name)
    }
    fn get_note_options(
        uri: &str,
    ) -> Result<crate::outcomes::NoteOptions, crate::ApplicationError> {
        let config =
            super::validate_config(&super::workspace_path(uri)?.join(".fleximark/config.toml"))?;
        Ok(super::get_note_options(&config))
    }
    use super::*;
    use crate::native::initialize_workspace;
    use crate::native::test_support::test_workspace;
    #[test]
    fn create_note_applies_category_template_filename_date_and_snippets() {
        let root = test_workspace("note-options-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            r##"schema_version = 1

[notes]
root = "notes"
file_name_prefix = "${CURRENT_YEAR}-"
file_name_suffix = "-draft"

[notes.categories]
Work = { Reports = {} }

[notes.templates]
daily = ["# ${FILE_NAME}", "${1:Title}", "Created ${CURRENT_YEAR}-${CURRENT_MONTH}-${CURRENT_DATE}", "$0"]
"##,
        )
        .unwrap();
        let options = get_note_options(&uri).unwrap();
        assert_eq!(options.categories.len(), 1);
        assert_eq!(options.categories[0].name, "Work");
        assert_eq!(options.categories[0].children[0].name, "Reports");
        assert_eq!(options.templates, ["daily"]);
        let category = ["Work".to_owned(), "Reports".to_owned()];
        let result =
            create_note_with_options(&uri, Some(&category), Some("daily"), Some("session"))
                .unwrap();
        let note = workspace_path(&result.uri).unwrap();
        assert_eq!(
            note.parent().unwrap(),
            root.join("notes/Work/Reports").canonicalize().unwrap()
        );
        let name = note.file_name().unwrap().to_string_lossy();
        assert!(name.ends_with("-session-draft.md"));
        let contents = fs::read_to_string(note).unwrap();
        assert!(contents.starts_with("# session\nTitle\nCreated "));
        assert!(!contents.contains("${") && !contents.contains("$0"));
        assert!(matches!(
            create_note_with_options(&uri, Some(&category), Some("daily"), Some("session")),
            Err(ApplicationError::NoteAlreadyExists)
        ));
    }

    #[test]
    fn default_template_inserts_user_names_literally() {
        let root = test_workspace("note-template-file-name-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            r##"schema_version = 1
[notes]
file_name_prefix = "prefix-"
file_name_suffix = "-suffix"
[notes.templates]
default = ["# ${FILE_NAME}", "Name: ${FILE_NAME}", "${1:Body}", "$0", "${UNKNOWN}"]
"##,
        )
        .unwrap();

        for name in [
            "音楽 ミーティング",
            "release.v0.17",
            "literal-$0-${CURRENT_YEAR}-${FILE_NAME}",
        ] {
            let result = create_note_with_options(&uri, None, None, Some(name)).unwrap();
            let note = workspace_path(&result.uri).unwrap();
            assert_eq!(
                note.file_name().unwrap().to_str().unwrap(),
                format!("prefix-{name}-suffix.md")
            );
            assert_eq!(
                fs::read_to_string(note).unwrap(),
                format!("# {name}\nName: {name}\nBody\n\n${{UNKNOWN}}\n")
            );
        }
    }

    #[test]
    fn template_uses_generated_name_when_no_name_is_supplied() {
        let root = test_workspace("note-template-generated-name-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            "schema_version = 1\n[notes.templates]\ndefault = [\"# ${FILE_NAME}\"]\n",
        )
        .unwrap();
        let result = create_note(&uri).unwrap();
        let note = workspace_path(&result.uri).unwrap();
        let name = note.file_stem().unwrap().to_str().unwrap();
        assert!(name.starts_with("note-"));
        assert_eq!(fs::read_to_string(&note).unwrap(), format!("# {name}\n"));
    }

    #[test]
    fn create_note_rejects_unsafe_user_file_names() {
        let root = test_workspace("note-name-validation-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        for invalid in ["", ".", "..", "a/b", "a\\b", "CON", "trailing."] {
            assert!(matches!(
                create_note_with_options(&uri, None, None, Some(invalid)),
                Err(ApplicationError::InvalidNoteName)
            ));
        }
    }

    #[test]
    fn current_config_defaults_note_root_to_workspace_directory() {
        let root = test_workspace("default-note-root-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            r#"schema_version = 1
[notes]
file_name_prefix = "${CURRENT_YEAR}${CURRENT_MONTH}${CURRENT_DATE}_"
file_name_suffix = ""

[notes.categories]
"DTM関連" = { "DTM入門" = {}, "その他" = {} }
"#,
        )
        .unwrap();
        let category = ["DTM関連".to_owned(), "その他".to_owned()];
        let result =
            create_note_with_options(&uri, Some(&category), None, Some("ミックス")).unwrap();
        let note = workspace_path(&result.uri).unwrap();
        assert_eq!(
            note.parent().unwrap(),
            root.join("DTM関連/その他").canonicalize().unwrap()
        );
        assert!(
            note.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with("_ミックス.md")
        );
        assert!(!root.join("notes").exists());
    }

    #[test]
    fn parent_categories_are_destinations_and_paths_disambiguate_equal_names() {
        let root = test_workspace("note-category-tree-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        fs::write(
            root.join(".fleximark/config.toml"),
            r#"schema_version = 1
[notes]
root = "notes"
[notes.categories]
Left = { Same = {} }
Right = { Same = {} }
"#,
        )
        .unwrap();

        let parent_path = ["Left".to_owned()];
        let parent = create_note_with_options(&uri, Some(&parent_path), None, None).unwrap();
        let parent = workspace_path(&parent.uri).unwrap();
        assert_eq!(
            parent.parent().unwrap(),
            root.join("notes/Left").canonicalize().unwrap()
        );

        let left_child_path = ["Left".to_owned(), "Same".to_owned()];
        let child = create_note_with_options(&uri, Some(&left_child_path), None, None).unwrap();
        let child = workspace_path(&child.uri).unwrap();
        assert_eq!(
            child.parent().unwrap(),
            root.join("notes/Left/Same").canonicalize().unwrap()
        );

        let right_child_path = ["Right".to_owned(), "Same".to_owned()];
        let right = create_note_with_options(&uri, Some(&right_child_path), None, None).unwrap();
        let right = workspace_path(&right.uri).unwrap();
        assert_eq!(
            right.parent().unwrap(),
            root.join("notes/Right/Same").canonicalize().unwrap()
        );
    }

    #[test]
    fn collect_admonitions_writes_only_typed_admonitions() {
        let root = test_workspace("collect-test");
        let uri = path_to_file_uri(&root).unwrap();
        initialize_workspace(&uri).unwrap();
        let source = "# Keep out\n\n:::info\nkeep in\n:::\n\nTrailing\n";
        let session = fleximark_engine::DocumentEngine::open(
            fleximark_model::DocumentUri(format!("{uri}/source.md")),
            1,
            source.to_owned(),
            fleximark_model::PositionEncoding::Utf8,
            Default::default(),
        )
        .unwrap();
        let result = collect_admonitions(session.document(), source, &uri).unwrap();
        assert_eq!(result.count, 1);
        let output = fs::read_to_string(workspace_path(&result.uri).unwrap()).unwrap();
        assert!(output.contains(":::info\nkeep in\n:::"));
        assert!(!output.contains("# Keep out") && !output.contains("Trailing"));
    }
}
