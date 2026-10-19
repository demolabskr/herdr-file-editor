//! Fork addition: the in-pane file editor modal (`e`). Opens the selected file in
//! [`crate::file_editor::FileEditor`], routes keys/paste/mouse to it, and refreshes git state and
//! the preview after each explicit save. The editor draws over the active content pane.

use super::*;
use crate::file_editor::{EditorAction, FileEditor, FileEditorView};

impl Controller {
    /// `e` — open the selected file in the in-pane editor. Directories are inert; files the editor
    /// refuses (binary, non-UTF-8, too large) surface the reason as an action notice.
    pub(super) fn edit_in_place(&mut self) -> Effects {
        let Some(node) = self.tree.selected() else {
            return Effects::noop();
        };
        if node.kind != NodeKind::File {
            return Effects::noop();
        }
        let title = node
            .path
            .strip_prefix(&self.root)
            .unwrap_or(&node.path)
            .to_string_lossy()
            .into_owned();
        match FileEditor::open(&node.path, title) {
            Ok(mut editor) => {
                // Start where the user was looking when the preview is a 1:1 source view.
                let source_view = self.active_source().is_some()
                    && self
                        .active_display
                        .presentation()
                        .is_some_and(|p| !p.wrap());
                if source_view && self.content_scroll() > 0 {
                    editor.goto_line(self.content_scroll() as usize + 1);
                }
                self.modal = Modal::FileEditor(Box::new(editor));
                Effects::redraw()
            }
            Err(e) => {
                self.action_notice = Some(format!("편집할 수 없음: {e}"));
                Effects::redraw()
            }
        }
    }

    /// Whether the in-pane editor owns the keyboard.
    pub fn file_editor_open(&self) -> bool {
        matches!(self.modal, Modal::FileEditor(_))
    }

    pub(super) fn file_editor_view(&self) -> Option<FileEditorView> {
        match &self.modal {
            Modal::FileEditor(editor) => Some(editor.view()),
            _ => None,
        }
    }

    /// Route one key press to the editor.
    pub fn handle_file_editor_key(&mut self, key: KeyEvent) -> Effects {
        let action = match &mut self.modal {
            Modal::FileEditor(editor) => editor.handle_key(key),
            _ => return Effects::noop(),
        };
        self.apply_editor_action(action)
    }

    /// Route a bracketed paste to the editor (one undo step).
    pub fn handle_file_editor_paste(&mut self, text: &str) -> Effects {
        match &mut self.modal {
            Modal::FileEditor(editor) => {
                editor.paste(text);
                Effects::redraw()
            }
            _ => Effects::noop(),
        }
    }

    /// Mouse while editing: the wheel scrolls, a left click inside the text area moves the
    /// cursor. Nothing leaks to the tree/content underneath.
    pub(super) fn handle_file_editor_mouse(&mut self, ev: MouseEvent) -> Effects {
        let area = self.file_editor_text_area;
        let step = self.wheel_step.max(1);
        let Modal::FileEditor(editor) = &mut self.modal else {
            return Effects::noop();
        };
        match ev.kind {
            MouseEventKind::ScrollUp => {
                editor.scroll(-step);
                Effects::redraw()
            }
            MouseEventKind::ScrollDown => {
                editor.scroll(step);
                Effects::redraw()
            }
            MouseEventKind::Down(MouseButton::Left) => match area {
                Some(rect)
                    if rect.contains(Position {
                        x: ev.column,
                        y: ev.row,
                    }) =>
                {
                    editor.click(ev.column - rect.x, ev.row - rect.y);
                    Effects::redraw()
                }
                _ => Effects::noop(),
            },
            _ => Effects::noop(),
        }
    }

    /// The presenter reports the editor's drawn text area each frame. Returns whether the
    /// visible window moved (so the loop repaints once more with the right rows).
    pub(super) fn set_file_editor_area(&mut self, area: Option<ratatui::layout::Rect>) -> bool {
        self.file_editor_text_area = area;
        match (&mut self.modal, area) {
            (Modal::FileEditor(editor), Some(rect)) => editor.set_viewport(rect.width, rect.height),
            _ => false,
        }
    }

    /// Focus regained while editing: warn once if the file changed outside.
    pub(super) fn file_editor_focus_gained(&mut self) {
        if let Modal::FileEditor(editor) = &mut self.modal {
            editor.note_external_change();
        }
    }

    fn apply_editor_action(&mut self, action: EditorAction) -> Effects {
        match action {
            EditorAction::None => Effects::noop(),
            EditorAction::Redraw => Effects::redraw(),
            EditorAction::Saved => {
                self.after_editor_save();
                Effects::redraw()
            }
            EditorAction::SavedAndClose => {
                self.after_editor_save();
                self.close_file_editor("저장하고 편집기를 닫았습니다")
            }
            EditorAction::Close => self.close_file_editor("편집기를 닫았습니다"),
        }
    }

    /// A save changed the file: re-read git markers and re-render the preview beneath.
    fn after_editor_save(&mut self) {
        self.refresh_git_state();
        self.dispatch_render();
    }

    fn close_file_editor(&mut self, notice: &str) -> Effects {
        self.modal = Modal::None;
        self.file_editor_text_area = None;
        self.action_notice = Some(notice.to_string());
        self.dispatch_render();
        Effects::redraw()
    }
}
