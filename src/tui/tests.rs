use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

fn key(app: &mut App, code: KeyCode) {
    app.handle(KeyEvent::new(code, KeyModifiers::NONE));
}
fn ch(app: &mut App, c: char) {
    key(app, KeyCode::Char(c));
}
fn ctrl(app: &mut App, c: char) {
    app.handle(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}
fn fixture() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::new(Store::new(dir.path().join("records")), 0).unwrap();
    (dir, app)
}
fn record() -> Record {
    Record {
        version: 1,
        label: None,
        path: "/project".into(),
        command: "program".into(),
        args: vec!["".into(), "hello world".into(), "\\'quoted'\n".into()],
        options: Default::default(),
    }
}
fn type_input(app: &mut App, text: &str) {
    ctrl(app, 'u');
    for c in text.chars() {
        ch(app, c);
    }
    key(app, KeyCode::Enter);
}
fn seeded() -> (tempfile::TempDir, App) {
    let (dir, mut app) = fixture();
    app.store.save(&record()).unwrap();
    app.reload().unwrap();
    (dir, app)
}
#[test]
fn create_edit_and_delete_pattern_then_save_record() {
    let (_dir, mut app) = fixture();
    ch(&mut app, 'n');
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    type_input(&mut app, "program");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    type_input(&mut app, "'hello world' ''");
    key(&mut app, KeyCode::Tab);
    ch(&mut app, 'a');
    type_input(&mut app, "**/*.rs");
    key(&mut app, KeyCode::Enter);
    type_input(&mut app, "**/*.toml");
    assert_eq!(
        app.editor.as_ref().unwrap().draft.options.includes,
        ["**/*.toml"]
    );
    ch(&mut app, 'd');
    assert!(
        app.editor
            .as_ref()
            .unwrap()
            .draft
            .options
            .includes
            .is_empty()
    );
    ch(&mut app, 'a');
    type_input(&mut app, "**/*.rs");
    key(&mut app, KeyCode::Tab);
    ch(&mut app, 'a');
    type_input(&mut app, "**/generated/**");
    assert!(app.store.records().unwrap().is_empty());
    ctrl(&mut app, 's');
    assert!(app.editor.is_none());
    let records = app.store.records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].args, ["hello world", ""]);
    assert_eq!(records[0].options.excludes, ["**/generated/**"]);
}
#[test]
fn invalid_input_stays_open_and_never_mutates_draft() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'e');
    key(&mut app, KeyCode::Tab);
    ch(&mut app, 'a');
    type_input(&mut app, "[");
    assert!(app.error);
    assert!(matches!(app.popup, Some(Popup::Input(_))));
    assert!(
        app.editor
            .as_ref()
            .unwrap()
            .draft
            .options
            .includes
            .is_empty()
    );
    type_input(&mut app, "**/*.rs");
    assert!(!app.error);
    assert!(app.popup.is_none());
    assert_eq!(app.store.records().unwrap(), [record()]);
}
#[test]
fn deletion_requires_confirmation_and_preserves_newer_records() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'd');
    assert_eq!(app.store.records().unwrap().len(), 1);
    ch(&mut app, 'n');
    assert!(app.popup.is_none());
    ch(&mut app, 'd');
    ch(&mut app, 'y');
    assert!(app.store.records().unwrap().is_empty());
    assert_eq!(app.list.selected(), None);
}
#[test]
fn unsaved_changes_require_discard_confirmation() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'e');
    key(&mut app, KeyCode::Tab);
    ch(&mut app, 'a');
    type_input(&mut app, "*.rs");
    key(&mut app, KeyCode::Esc);
    assert!(matches!(app.popup, Some(Popup::Confirm(_))));
    ch(&mut app, 'n');
    assert!(app.editor.is_some());
    key(&mut app, KeyCode::Esc);
    ch(&mut app, 'y');
    assert!(app.editor.is_none());
    assert_eq!(app.store.records().unwrap(), [record()]);
}
#[test]
fn defaults_can_be_customized_without_losing_existing_rules() {
    let mut editor = Editor::new(record(), None);
    editor.tab = 2;
    editor.apply(Field::Exclude(None), "**/private/**").unwrap();
    editor.copy_defaults();
    assert!(!editor.draft.options.default_ignores);
    let count = editor.draft.options.excludes.len();
    assert!(count > 1);
    editor.copy_defaults();
    assert_eq!(editor.draft.options.excludes.len(), count);
    assert_eq!(editor.draft.options.excludes[0], "**/private/**");
    editor.remove_pattern();
    assert_eq!(editor.draft.options.excludes.len(), count - 1);
}
#[test]
fn runtime_toggles_and_optional_values_validate() {
    let mut editor = Editor::new(record(), None);
    editor.tab = 3;
    editor.selected[3] = 4;
    editor.toggle();
    assert_eq!(
        editor.draft.options.policy,
        crate::config::ExecutionPolicy::Restart
    );
    editor.selected[3] = 5;
    editor.toggle();
    assert!(!editor.draft.options.default_ignores);
    editor.apply(Field::Poll, "1000").unwrap();
    editor.apply(Field::Poll, "").unwrap();
    assert_eq!(editor.draft.options.poll_ms, None);
    assert!(editor.apply(Field::Timeout, "0").is_err());
    assert_eq!(editor.draft.options.timeout_ms, None);
}
#[test]
fn stale_edits_do_not_overwrite_external_changes() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'e');
    app.editor
        .as_mut()
        .unwrap()
        .apply(Field::Command, "edited")
        .unwrap();
    let mut external = record();
    external.command = "external".into();
    app.store.replace_record(0, &record(), &external).unwrap();
    ctrl(&mut app, 's');
    assert!(app.error);
    assert!(app.editor.is_some());
    assert_eq!(app.store.records().unwrap(), [external]);
}
#[test]
fn saving_other_fields_preserves_exact_arguments() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'e');
    app.editor
        .as_mut()
        .unwrap()
        .apply(Field::Debounce, "200")
        .unwrap();
    ctrl(&mut app, 's');
    assert_eq!(app.store.records().unwrap()[0].args, record().args);
}
#[test]
fn unicode_input_cursor_and_backspace_preserve_boundaries() {
    let mut input = Input::new(Field::Command, &record());
    input.text.clear();
    input.cursor = 0;
    input.insert("你好🙂");
    input.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    input.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(input.text, "你🙂");
    input.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(input.text, "你");
    input.insert("好");
    assert_eq!(input.text, "你好");
}
#[test]
fn ctrl_c_does_not_silently_lose_an_open_field() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'e');
    key(&mut app, KeyCode::Enter);
    ch(&mut app, 'x');
    ctrl(&mut app, 'c');
    assert!(matches!(app.popup, Some(Popup::Input(_))));
    assert!(!app.quit);
}
#[test]
fn render_all_screens_and_small_terminals() {
    let (_dir, mut app) = seeded();
    for (width, height) in [(110, 34), (80, 24), (60, 18), (20, 8), (0, 0)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| view::draw(frame, &mut app)).unwrap();
        ch(&mut app, 'e');
        for tab in 0..4 {
            app.editor.as_mut().unwrap().tab = tab;
            terminal.draw(|frame| view::draw(frame, &mut app)).unwrap();
        }
        app.editor.as_mut().unwrap().tab = 0;
        key(&mut app, KeyCode::Enter);
        terminal.draw(|frame| view::draw(frame, &mut app)).unwrap();
        key(&mut app, KeyCode::Esc);
        app.editor = None;
        ch(&mut app, 'd');
        terminal.draw(|frame| view::draw(frame, &mut app)).unwrap();
        ch(&mut app, 'n');
        ch(&mut app, '?');
        terminal.draw(|frame| view::draw(frame, &mut app)).unwrap();
        key(&mut app, KeyCode::Esc);
    }
}

#[test]
fn stale_deletion_is_rejected_without_removing_replacement() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'd');
    let mut external = record();
    external.command = "replacement".into();
    app.store.replace_record(0, &record(), &external).unwrap();
    ch(&mut app, 'y');
    assert!(app.error);
    assert_eq!(app.store.records().unwrap(), [external]);
}
#[test]
fn updates_merge_with_concurrent_additions_and_reject_shifted_indices() {
    let (_dir, mut app) = seeded();
    ch(&mut app, 'e');
    let mut added = record();
    added.command = "added".into();
    app.store.save(&added).unwrap();
    app.editor
        .as_mut()
        .unwrap()
        .apply(Field::Command, "edited")
        .unwrap();
    ctrl(&mut app, 's');
    let records = app.store.records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[1], added);
    assert!(app.store.delete_record(0, &record()).is_err());
    app.store.delete_record(0, &records[0]).unwrap();
    assert!(app.store.replace_record(1, &added, &record()).is_err());
    assert_eq!(app.store.records().unwrap(), [added]);
}
#[test]
fn pasted_line_breaks_do_not_concatenate_words() {
    let mut input = Input::new(Field::Command, &record());
    input.text.clear();
    input.cursor = 0;
    input.insert("hello\nworld\tend\x1b");
    assert_eq!(input.text, "hello world end");
}
#[test]
fn rendered_browser_exposes_primary_actions() {
    let (_dir, mut app) = seeded();
    let mut terminal = Terminal::new(TestBackend::new(110, 34)).unwrap();
    terminal.draw(|frame| view::draw(frame, &mut app)).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    for text in [
        "Configuration manager",
        "n: new",
        "d: delete",
        "Configuration details",
        "--use 0",
    ] {
        assert!(screen.contains(text), "missing {text:?}");
    }
}

#[test]
fn labels_can_be_added_renamed_cleared_and_are_hidden_when_absent() {
    let (_dir, mut app) = seeded();
    fn screen(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 34)).unwrap();
        terminal.draw(|frame| view::draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..34 {
            let mut x = 0;
            while x < 110 {
                let symbol = buffer[(x, y)].symbol();
                text.push_str(symbol);
                // Wide glyphs occupy continuation cells that the terminal skips.
                x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
            }
        }
        text
    }
    assert!(!screen(&mut app).contains("Label"));
    for name in ["Build 工具", "Watch frontend", "   "] {
        ch(&mut app, 'e');
        for _ in 0..4 {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Enter);
        assert!(
            matches!(&app.popup, Some(Popup::Input(input)) if matches!(input.field, Field::Label))
        );
        type_input(&mut app, name);
        ctrl(&mut app, 's');
        assert!(!app.error);
        let records = app.store.records().unwrap();
        assert_eq!(records.len(), 1);
        let expected = (!name.trim().is_empty()).then_some(name);
        assert_eq!(records[0].label.as_deref(), expected);
        assert_eq!(records[0].args, record().args);
        let rendered = screen(&mut app);
        if let Some(name) = expected {
            assert!(rendered.contains(name));
            assert!(rendered.contains("Label"));
        } else {
            assert!(!rendered.contains("Label"));
            assert!(rendered.contains("program"));
        }
    }
}
