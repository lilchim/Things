use chrono::{NaiveDateTime, TimeDelta};
use eframe::egui;
use things_core::components::{from_ron, to_ron};
use things_core::tasks::{FILE, Repeat, Task, Tasks, Weekday, When, parse_date};

use super::{Editor, Kind, single_file};

pub struct TasksKind;

impl Kind for TasksKind {
    fn label(&self, _path: &str) -> String {
        "Tasks".into()
    }

    fn matches(&self, path: &str) -> bool {
        path == FILE
    }

    fn open(&self, text: &str) -> Result<Box<dyn Editor>, String> {
        let tasks: Tasks = from_ron(text).map_err(|e| e.to_string())?;
        Ok(Box::new(TasksEditor {
            tasks,
            new_name: String::new(),
        }))
    }

    fn create(&self, existing: &[&str]) -> Option<(String, String)> {
        let text = to_ron(&Tasks::default()).expect("tasks always serialise");
        single_file(FILE, existing, text)
    }
}

/// The local time, as tasks see it.
pub fn now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

struct TasksEditor {
    tasks: Tasks,
    /// The "add a task" field.
    new_name: String,
}

impl Editor for TasksEditor {
    fn ui(&mut self, ui: &mut egui::Ui) {
        let now = now();
        let mut remove = None;
        for (i, task) in self.tasks.tasks.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                if task_ui(ui, task, now) {
                    remove = Some(i);
                }
            });
            ui.separator();
        }
        if let Some(i) = remove {
            self.tasks.tasks.remove(i);
        }
        if self.tasks.tasks.is_empty() {
            ui.weak("No tasks yet. Dailies, a weekly reset, a chore every two weeks, an errand by a date…");
        }

        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("New task")
                    .desired_width(ui.available_width() - 80.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ready = !self.new_name.trim().is_empty();
            if ui
                .add_enabled(ready, egui::Button::new("Add task"))
                .clicked()
                || (enter && ready)
            {
                self.tasks.tasks.push(Task::named(&self.new_name));
                self.new_name.clear();
            }
        });
    }

    fn text(&self) -> String {
        to_ron(&self.tasks).expect("tasks always serialise")
    }
}

/// One task's row and form. Returns true when it should be removed.
fn task_ui(ui: &mut egui::Ui, task: &mut Task, now: NaiveDateTime) -> bool {
    let mut remove = false;
    ui.horizontal(|ui| {
        let mut done = task.is_done(now);
        if ui
            .checkbox(&mut done, "")
            .on_hover_text("Done for now")
            .changed()
        {
            if done {
                task.check(now);
            } else {
                task.uncheck();
            }
        }
        ui.add(
            egui::TextEdit::singleline(&mut task.name)
                .hint_text("What to do")
                .desired_width(ui.available_width() - 40.0),
        );
        if ui.small_button("🗑").on_hover_text("Remove task").clicked() {
            remove = true;
        }
    });

    ui.horizontal_wrapped(|ui| {
        repeat_ui(ui, &mut task.repeat);
        match task.repeat {
            Repeat::Once => {
                ui.label("by");
                ui.add(
                    egui::TextEdit::singleline(&mut task.due)
                        .hint_text("YYYY-MM-DD, or empty")
                        .desired_width(110.0),
                );
                if !task.due.trim().is_empty() && parse_date(&task.due).is_none() {
                    ui.colored_label(ui.visuals().warn_fg_color, "Use YYYY-MM-DD");
                }
            }
            _ => {
                ui.label("day starts at");
                ui.add(
                    egui::DragValue::new(&mut task.resets_at)
                        .range(0..=23)
                        .suffix(":00"),
                )
                .on_hover_text("A game's reset hour. Before it, it's still yesterday.");
            }
        }
    });

    ui.horizontal_wrapped(|ui| {
        let when = task.when(now);
        let text = describe(task, when, now);
        if matches!(when, When::Overdue { .. }) {
            ui.colored_label(ui.visuals().warn_fg_color, text);
        } else {
            ui.weak(text);
        }
        if let Some(recent) = task.recent(now, 10) {
            ui.weak(format!("· {recent}"));
        } else if let Some(last) = task.last_done()
            && !matches!(when, When::Done | When::Finished)
        {
            ui.weak(format!("· last done {last}"));
        }
    });
    remove
}

fn repeat_ui(ui: &mut egui::Ui, repeat: &mut Repeat) {
    let kinds = [
        (Repeat::Once, "Once"),
        (Repeat::Daily, "Daily"),
        (Repeat::Weekly(Weekday::Mon), "Weekly"),
        (Repeat::Monthly(1), "Monthly"),
        (Repeat::Every(7), "Every few days"),
    ];
    let same = |a: Repeat, b: Repeat| std::mem::discriminant(&a) == std::mem::discriminant(&b);
    let shown = kinds
        .iter()
        .find(|(r, _)| same(*r, *repeat))
        .map_or("Once", |(_, l)| l);
    egui::ComboBox::from_id_salt("repeat")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (kind, label) in kinds {
                if ui.selectable_label(same(kind, *repeat), label).clicked() && !same(kind, *repeat)
                {
                    *repeat = kind;
                }
            }
        });
    match repeat {
        Repeat::Weekly(day) => {
            ui.label("from");
            egui::ComboBox::from_id_salt("weekday")
                .selected_text(day.label())
                .show_ui(ui, |ui| {
                    for d in Weekday::ALL {
                        ui.selectable_value(day, d, d.label());
                    }
                });
        }
        Repeat::Monthly(day) => {
            ui.label("from day");
            ui.add(egui::DragValue::new(day).range(1..=31));
        }
        Repeat::Every(n) => {
            ui.add(egui::DragValue::new(n).range(1..=365));
            ui.label("days after I last did it");
        }
        Repeat::Once | Repeat::Daily => {}
    }
}

/// Where a task stands, in a few words: "resets in 5h", "3 days overdue".
pub fn describe(task: &Task, when: When, now: NaiveDateTime) -> String {
    let today = task.today(now);
    let days = |n: i64| {
        if n == 1 {
            "1 day".to_owned()
        } else {
            format!("{n} days")
        }
    };
    match when {
        When::Overdue { since } => {
            format!("{} overdue", days((today - since).num_days()))
        }
        When::Now { until } => {
            let left = span(until - now);
            if task.repeat.is_calendar() {
                format!("resets in {left}")
            } else {
                format!("due today, {left} left")
            }
        }
        When::Coming { on } => match (on - today).num_days() {
            1 => format!("tomorrow ({on})"),
            n => format!("in {} ({on})", days(n)),
        },
        When::Anytime => "whenever".into(),
        When::Done if task.repeat.is_calendar() => "done for now".into(),
        When::Done => "done today".into(),
        When::Finished => match task.last_done() {
            Some(d) => format!("done {d}"),
            None => "done".into(),
        },
    }
}

/// "5h", "40m", "3 days".
fn span(t: TimeDelta) -> String {
    if t.num_days() >= 2 {
        format!("{} days", t.num_days())
    } else if t.num_hours() >= 1 {
        format!("{}h", t.num_hours())
    } else {
        format!("{}m", t.num_minutes().max(1))
    }
}
