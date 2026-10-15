//! The feedback specimen: flash, alert, panel notice, connection banners, the
//! modal and confirmation dialogs and the tab strip on one page, served
//! unauthenticated at `/assets/ui-feedback.html`. It is the screenshot baseline
//! for those components, not the Design Library. The dialogs are `<template>`
//! fragments rendered by the same macros; `static/ui-feedback-demo.js` stands in
//! for the server by inserting and removing them.
use crate::section::sections;
use crate::{FlashMap, Tab};
use askama::Template;

/// The page is cut into sections, each its own template; see `section`.
#[derive(Template)]
#[template(path = "ui_feedback_specimen.html")]
pub struct FeedbackSpecimen {
    tabs_links: Vec<Tab<'static>>,
    tabs_narrow: Vec<Tab<'static>>,
    tabs_buttons: Vec<Tab<'static>>,
    tabs_single: Vec<Tab<'static>>,
    long_paragraphs: Vec<usize>,
    flash_in_dialog: FlashMap<'static>,
}

sections! {
    FeedbackSpecimen {
        flash_group: "ui_feedback_specimen/flash_group.html",
        flash: "ui_feedback_specimen/flash.html",
        alerts: "ui_feedback_specimen/alerts.html",
        connection: "ui_feedback_specimen/connection.html",
        dialogs: "ui_feedback_specimen/dialogs.html",
        tab_strips: "ui_feedback_specimen/tab_strips.html",
        flash_templates: "ui_feedback_specimen/flash_templates.html",
        modal_templates: "ui_feedback_specimen/modal_templates.html",
        confirm_templates: "ui_feedback_specimen/confirm_templates.html",
    }
}

impl FeedbackSpecimen {
    pub fn new() -> Self {
        let link = |label| Tab::link("#component-tabs", label, label == "Overview");
        Self {
            tabs_links: [
                "Overview",
                "History",
                "Settings",
                "Recent activity",
                "Access history",
            ]
            .map(link)
            .into(),
            tabs_narrow: [
                "Overview",
                "History",
                "Settings",
                "Recent activity",
                "Access history",
            ]
            .map(link)
            .into(),
            tabs_buttons: vec![
                Tab::button("switch-tab", "overview", "Overview", true),
                Tab::button("switch-tab", "history", "History", false),
                Tab::button("switch-tab", "settings", "Settings", false),
            ],
            tabs_single: vec![Tab::link("#component-tabs", "Overview", true)],
            long_paragraphs: (1..=18).collect(),
            flash_in_dialog: FlashMap {
                error: "The changes could not be saved.",
                info: "Nothing else changed.",
                ..FlashMap::default()
            },
        }
    }
}

impl Default for FeedbackSpecimen {
    fn default() -> Self {
        Self::new()
    }
}
