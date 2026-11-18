//! The question before writing: photos the transfer would import that the
//! LightCraft library already has. The person chooses once for all of them
//! or per photo; nothing is written until they answer.

use crate::run::{Answer, Question};
use crate::theme::{MEDIUM, SEMIBOLD, Tokens, font};
use crate::widgets::{middle_ellipsis, primary, secondary};
use egui::{Frame, Margin, RichText, Ui};
use lr2lc_core::{ExistingChoice, ExistingDecisions, ExistingPhoto, ExistingReason};
use serde_json::Value;

/// The question being answered: what was found and the choices so far.
pub struct QuestionState {
    pub question: Question,
    /// One choice for every photo, or one per photo.
    pub each: bool,
    /// The choice for all photos (and for photos found only while writing).
    pub all: ExistingChoice,
    /// The choice per photo, in the order of `question.existing`.
    pub per_photo: Vec<ExistingChoice>,
    /// Lists only photos with development settings in LightCraft.
    pub only_edited: bool,
}

impl QuestionState {
    pub fn new(question: Question) -> QuestionState {
        let per_photo = vec![ExistingChoice::default(); question.existing.len()];
        QuestionState {
            question,
            each: false,
            all: ExistingChoice::default(),
            per_photo,
            only_edited: false,
        }
    }

    /// The answer the current choices give.
    pub fn decisions(&self) -> ExistingDecisions {
        let mut decisions = ExistingDecisions::all(self.all);
        if self.each {
            for (photo, choice) in self.question.existing.iter().zip(&self.per_photo) {
                decisions.per_photo.insert(photo.global_id.clone(), *choice);
            }
        }
        decisions
    }

    fn edited(&self) -> usize {
        self.question
            .existing
            .iter()
            .filter(|p| p.live.edited)
            .count()
    }
}

/// What the person did in the question.
pub enum Action {
    Answer(Answer),
    /// Show this LightCraft photo.
    Show(Value),
}

/// The short name of a choice.
pub fn label(choice: ExistingChoice) -> &'static str {
    match choice {
        ExistingChoice::Overwrite => "Overwrite photo",
        ExistingChoice::Develop => "Overwrite development only",
        ExistingChoice::DevelopIfEmpty => "Development only if LightCraft has none",
        ExistingChoice::Skip => "Skip",
    }
}

/// What a choice does.
fn explain(choice: ExistingChoice) -> &'static str {
    match choice {
        ExistingChoice::Overwrite => {
            "Lightroom's rating, flag, label, texts and development settings replace LightCraft's."
        }
        ExistingChoice::Develop => {
            "Lightroom's development settings replace LightCraft's. Other fields are only filled where LightCraft has none; differences are listed as conflicts."
        }
        ExistingChoice::DevelopIfEmpty => {
            "Recommended. Development settings made in LightCraft stay; a photo without any gets Lightroom's. Other fields as above."
        }
        ExistingChoice::Skip => {
            "The photo stays as it is in LightCraft. It is only added to its albums and stacks."
        }
    }
}

fn reason(photo: &ExistingPhoto) -> &'static str {
    match photo.reason {
        ExistingReason::Path => "same file",
        ExistingReason::Content => "same content, other path",
    }
}

fn stars(rating: u8) -> String {
    if rating == 0 {
        "–".to_owned()
    } else {
        "★".repeat(usize::from(rating.min(5)))
    }
}

/// Draws the question as a modal window; returns what the person did.
pub fn ui(ui: &mut Ui, t: &Tokens, state: &mut QuestionState) -> Option<Action> {
    let mut action = None;
    let n = state.question.existing.len();
    let edited = state.edited();
    let max_height = (ui.ctx().content_rect().height() - 80.0).max(320.0);
    egui::Modal::new(egui::Id::unique("lr2lc-existing-photos"))
        .frame(
            Frame::new()
                .fill(t.bg_panel)
                .stroke(egui::Stroke::new(1.0, t.border))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(Margin::same(20)),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(760.0_f32.min(ui.ctx().content_rect().width() - 80.0));
            ui.set_max_height(max_height);
            ui.spacing_mut().item_spacing.y = 10.0;
            let title = if n == 1 {
                "1 photo is already in LightCraft".to_owned()
            } else {
                format!("{n} photos are already in LightCraft")
            };
            ui.label(RichText::new(title).font(font(16.0, Some(SEMIBOLD))));
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "The library has these files already, but not from an earlier lr2lc run. {} Nothing has been written yet: choose what the transfer does with them.",
                        match edited {
                            0 => "None of them has development settings in LightCraft.".to_owned(),
                            1 => "1 of them has development settings in LightCraft.".to_owned(),
                            k => format!("{k} of them have development settings in LightCraft."),
                        }
                    ))
                    .color(t.text_muted),
                )
                .wrap(),
            );
            ui.horizontal(|ui| {
                ui.radio_value(&mut state.each, false, "Same for all photos");
                ui.radio_value(&mut state.each, true, "Decide per photo");
            });
            if state.each {
                per_photo(ui, t, state, max_height - 260.0, &mut action);
            } else {
                for choice in ExistingChoice::ALL {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut state.all, choice, label(choice));
                    });
                    ui.indent(choice.as_str(), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(explain(choice))
                                    .font(font(12.0, None))
                                    .color(t.text_muted),
                            )
                            .wrap(),
                        );
                    });
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "Keywords are only added, never removed. The choice holds for this transfer; the log lists every photo.",
                        )
                        .font(font(12.0, None))
                        .color(t.text_muted),
                    )
                    .wrap(),
                );
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if primary(ui, t, "Continue transfer", true, 0.0).clicked() {
                    action = Some(Action::Answer(Answer::Go(state.decisions())));
                }
                if secondary(ui, t, "Cancel transfer", true, 0.0, 44.0).clicked() {
                    action = Some(Action::Answer(Answer::Abort));
                }
            });
        });
    action
}

/// The table of photos with a choice and a Show button each.
fn per_photo(
    ui: &mut Ui,
    t: &Tokens,
    state: &mut QuestionState,
    height: f32,
    action: &mut Option<Action>,
) {
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.only_edited, "Only photos edited in LightCraft");
        ui.add_space(12.0);
        ui.label(RichText::new("Set all shown to").color(t.text_muted));
        let mut set = None;
        egui::ComboBox::from_id_salt("existing-set-all")
            .selected_text("…")
            .width(260.0)
            .show_ui(ui, |ui| {
                for choice in ExistingChoice::ALL {
                    if ui.selectable_label(false, label(choice)).clicked() {
                        set = Some(choice);
                    }
                }
            });
        if let Some(choice) = set {
            let only_edited = state.only_edited;
            for (photo, slot) in state.question.existing.iter().zip(&mut state.per_photo) {
                if !only_edited || photo.live.edited {
                    *slot = choice;
                }
            }
            if !only_edited {
                state.all = choice;
            }
        }
    });
    let shown: Vec<usize> = (0..state.question.existing.len())
        .filter(|&i| !state.only_edited || state.question.existing[i].live.edited)
        .collect();
    let row_height = 44.0;
    egui::ScrollArea::vertical()
        .max_height(height.max(160.0))
        .auto_shrink([false, true])
        .show_rows(ui, row_height, shown.len(), |ui, range| {
            for &i in &shown[range] {
                let photo = &state.question.existing[i];
                Frame::new()
                    .inner_margin(Margin::symmetric(4, 4))
                    .show(ui, |ui| {
                        ui.set_min_height(row_height - 8.0);
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(250.0);
                                ui.label(
                                    RichText::new(middle_ellipsis(&photo.file_name, 34))
                                        .font(font(13.0, Some(MEDIUM))),
                                )
                                .on_hover_text(photo.path.display().to_string());
                                let folder = photo
                                    .path
                                    .parent()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_default();
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}",
                                        middle_ellipsis(&folder, 26),
                                        reason(photo)
                                    ))
                                    .font(font(11.0, None))
                                    .color(t.text_muted),
                                );
                            });
                            ui.vertical(|ui| {
                                ui.set_width(110.0);
                                ui.label(RichText::new(stars(photo.live.rating)).color(t.text));
                                ui.label(
                                    RichText::new(if photo.live.edited {
                                        "edited in LightCraft"
                                    } else {
                                        "not edited"
                                    })
                                    .font(font(11.0, None))
                                    .color(
                                        if photo.live.edited {
                                            t.conflict
                                        } else {
                                            t.text_muted
                                        },
                                    ),
                                );
                            });
                            egui::ComboBox::from_id_salt(("existing-choice", i))
                                .selected_text(label(state.per_photo[i]))
                                .width(230.0)
                                .show_ui(ui, |ui| {
                                    for choice in ExistingChoice::ALL {
                                        ui.selectable_value(
                                            &mut state.per_photo[i],
                                            choice,
                                            label(choice),
                                        );
                                    }
                                });
                            if secondary(ui, t, "Show in LightCraft", true, 0.0, 32.0).clicked() {
                                *action = Some(Action::Show(photo.lightcraft_id.clone()));
                            }
                        });
                    });
            }
        });
    ui.add(
        egui::Label::new(
            RichText::new(
                "Photos found only while writing get the choice for all photos (\"Same for all photos\").",
            )
            .font(font(12.0, None))
            .color(t.text_muted),
        )
        .wrap(),
    );
}
