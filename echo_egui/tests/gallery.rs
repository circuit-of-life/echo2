//! The gallery demo, driven headlessly: clicks and typing go through egui, events through echo.

use std::{cell::RefCell, rc::Rc};

use echo_egui::EguiApp;
use egui::accesskit::Role;
use egui_kittest::{Harness, kittest::Queryable};

macro_rules! __echo_deny {
    ($($t:tt)*) => {};
}
pub(crate) use __echo_deny;

#[path = "../examples/gallery/demo.rs"]
mod demo;

use demo::Demo;

fn harness(state: Rc<RefCell<Demo>>) -> Harness<'static> {
    let app = EguiApp::new();
    Harness::builder()
        .with_size([520.0, 720.0])
        .build_ui(move |ui| {
            let mut s = state.borrow_mut();
            app.show(ui, |b| demo::demo(b.upcast(), &mut s, &mut |_, _| {}));
        })
}

#[test]
fn clicking_a_button_runs_its_on() {
    let state = Rc::new(RefCell::new(Demo::default()));
    let mut h = harness(state.clone());
    h.run();
    h.get_by_label("0 clicks");
    h.get_by_label("Click me").click();
    h.run();
    assert_eq!(state.borrow().clicks, 1);
    h.get_by_label("1 clicks");
    h.get_by_label("Click me").click();
    h.run();
    h.get_by_label("2 clicks");
}

#[test]
fn typing_fires_text_changed_and_enter_submits() {
    let state = Rc::new(RefCell::new(Demo::default()));
    let mut h = harness(state.clone());
    h.run();
    // (a placeholder isn't a label: the name field is the first text input)
    let name = h.get_all_by_role(Role::TextInput).next().unwrap();
    name.focus();
    name.type_text("Ada");
    h.run();
    assert_eq!(state.borrow().name, "Ada");
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(state.borrow().greeting.as_deref(), Some("Hello, Ada!"));
    h.get_by_label("Hello, Ada!");
}

#[test]
fn keyed_todo_rows_follow_their_items() {
    let state = Rc::new(RefCell::new(Demo::default()));
    let mut h = harness(state.clone());
    h.run();
    h.get_by_label("1 of 2 done");
    // add one through the line edit and the Add button
    let draft = h.get_all_by_role(Role::TextInput).nth(1).unwrap();
    draft.focus();
    draft.type_text("Write tests");
    h.run();
    h.get_by_label("Add").click();
    h.run();
    assert_eq!(state.borrow().todos.len(), 3);
    h.get_by_label("1 of 3 done");
    // check the new one: its own checkbox
    h.get_by_label("Write tests").click();
    h.run();
    assert!(state.borrow().todos[2].done);
    h.get_by_label("2 of 3 done");
    // remove the first: the others keep their state
    h.get_all_by_label("×").next().unwrap().click();
    h.run();
    let s = state.borrow();
    let left: Vec<_> = s.todos.iter().map(|t| (t.text.as_str(), t.done)).collect();
    assert_eq!(left, [("Try egui", false), ("Write tests", true)]);
    drop(s);
    h.get_by_label("1 of 2 done");
}

#[test]
fn checkbox_option_button_and_popup() {
    let state = Rc::new(RefCell::new(Demo::default()));
    let mut h = harness(state.clone());
    h.run();
    h.get_by_label("On").click();
    h.run();
    assert!(state.borrow().fullscreen);
    h.get_by_label("fullscreen: true");

    h.get_by_role(Role::ComboBox).click();
    h.run();
    h.get_by_label("High").click();
    h.run();
    assert_eq!(state.borrow().quality, 2);

    state.borrow_mut().clicks = 5;
    h.get_by_label("Menu").click();
    h.run();
    h.get_by_label("Reset clicks").click();
    h.run();
    assert_eq!(state.borrow().clicks, 0);
}

#[test]
fn folding_a_section() {
    let state = Rc::new(RefCell::new(Demo::default()));
    let mut h = harness(state.clone());
    h.run();
    assert!(h.query_by_label("Volume").is_some());
    h.get_by_label("Settings").click();
    h.run();
    assert!(h.query_by_label("Volume").is_none());
    assert!(state.borrow().log.is_empty());
    h.get_by_label("Settings").click();
    h.run();
    assert!(h.query_by_label("Volume").is_some());
}

#[test]
fn ranges_follow_the_state() {
    let state = Rc::new(RefCell::new(Demo::default()));
    let mut h = harness(state.clone());
    h.run();
    state.borrow_mut().volume = 75.0;
    h.run();
    let bar = format!("{:?}", h.get_by_role(Role::ProgressIndicator));
    assert!(bar.contains("numeric_value: 75.0"), "{bar}");
}

/// Renders the gallery to `target/echo_egui_*.png` (needs a GPU adapter; skipped if there's
/// none).
#[test]
fn screenshots() {
    let state = Rc::new(RefCell::new(Demo::default()));
    let app = EguiApp::new();
    let s = state.clone();
    let mut h = Harness::builder()
        .with_size([520.0, 720.0])
        .wgpu()
        .build_ui(move |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let mut s = s.borrow_mut();
                app.show(ui, |b| demo::demo(b.upcast(), &mut s, &mut |_, _| {}));
            });
        });
    let save = |h: &mut Harness, name: &str| match h.render() {
        Ok(image) => {
            let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../target");
            image.save(format!("{dir}/echo_egui_{name}.png")).unwrap();
        }
        Err(e) => eprintln!("no screenshot: {e}"),
    };
    h.run();
    h.get_by_label("Click me").click();
    h.run();
    save(&mut h, "gallery");
    h.get_by_label("Notes").click();
    h.run();
    h.get_by_label("Menu").click();
    h.run();
    save(&mut h, "popup");
}
