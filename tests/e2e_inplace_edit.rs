//! e2e (pty): fork feature — `e` edits the selected file inside the content pane, `Ctrl+S`
//! writes it, and `Esc` returns to the viewer. Asserted through the filesystem, plus that typed
//! printable keys (`q`) go into the text instead of quitting the viewer.
#![cfg(unix)]

mod common;

use common::{TempDir, viewer_command};
use expectrl::process::unix::WaitStatus;
use expectrl::{Eof, Expect, Session};
use std::time::{Duration, Instant};

/// Read (and discard) whatever the viewer drew for a moment. The viewer blocks on a full pty
/// output buffer, so a test that sends keys without reading would stall it.
fn settle(s: &mut expectrl::session::OsSession) {
    s.set_expect_timeout(Some(Duration::from_millis(250)));
    let _ = s.expect("\u{0}never-drawn\u{0}");
    s.set_expect_timeout(Some(Duration::from_secs(15)));
}

fn press(s: &mut expectrl::session::OsSession, keys: &str) {
    s.send(keys).expect("send keys");
    settle(s);
}

fn wait_for_content(path: &std::path::Path, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let got = std::fs::read_to_string(path).unwrap_or_default();
        if got == want || Instant::now() >= deadline {
            return got;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn e_edits_in_pane_ctrl_s_saves_and_esc_returns() {
    let dir = TempDir::new();
    let p = dir.path();
    std::fs::write(p.join("note.txt"), "hello\r\nsecond\r\n").unwrap();

    let mut s = Session::spawn(viewer_command(p)).expect("spawn the viewer in a pty");
    s.set_expect_timeout(Some(Duration::from_secs(15)));
    s.expect("note.txt").expect("tree should list the file");

    press(&mut s, "e"); // open the in-pane editor
    // End of the first line, then type text that includes viewer keys (q, e, ?).
    press(&mut s, "\x1b[F"); // xterm End
    press(&mut s, " q e?");
    press(&mut s, "\x13"); // Ctrl+S

    let saved = wait_for_content(&p.join("note.txt"), "hello q e?\r\nsecond\r\n");
    assert_eq!(
        saved, "hello q e?\r\nsecond\r\n",
        "saved with CRLF preserved"
    );

    // Esc closes the (saved) editor; then `q` quits the viewer.
    press(&mut s, "\x1b"); // Esc closes the saved editor
    s.send("q").expect("quit");
    s.expect(Eof).expect("viewer exits");
    match s.get_process().wait().expect("reap") {
        WaitStatus::Exited(_, code) => assert_eq!(code, 0),
        other => panic!("expected a clean exit, got {other:?}"),
    }
}

#[test]
fn closing_with_unsaved_changes_and_discarding_leaves_the_file_untouched() {
    let dir = TempDir::new();
    let p = dir.path();
    std::fs::write(p.join("keep.txt"), "original\n").unwrap();

    let mut s = Session::spawn(viewer_command(p)).expect("spawn the viewer in a pty");
    s.set_expect_timeout(Some(Duration::from_secs(15)));
    s.expect("keep.txt").expect("tree should list the file");

    press(&mut s, "e");
    press(&mut s, "XYZ");
    press(&mut s, "\x1b"); // Esc asks to confirm
    press(&mut s, "d"); // discard
    s.send("q").expect("quit");
    s.expect(Eof).expect("viewer exits");
    assert_eq!(
        std::fs::read_to_string(p.join("keep.txt")).unwrap(),
        "original\n",
        "discarded edits never reach the disk"
    );
}
